#!/usr/bin/env python3
"""Resolve and attest Rivet's per-checkout Cargo namespace."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import stat
import subprocess
import sys
import time
from typing import Iterable

MARKER = ".rivet-cargo-target"
SIDECAR_SUFFIX = ".rivet-provenance"
BUILD_RECEIPT = ".rivet-build-receipt"
ROOT_ENV = "RIVET_CARGO_TARGET_ROOT"
NAMESPACE_ENV = "RIVET_CARGO_NAMESPACE"


def run_git(repo: pathlib.Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def canonical_existing(path: pathlib.Path) -> pathlib.Path:
    return path.resolve(strict=True)


def canonical_parent(path: pathlib.Path) -> pathlib.Path:
    path = pathlib.Path(os.path.abspath(path))
    missing: list[str] = []
    probe = path
    while not probe.exists():
        missing.append(probe.name)
        if probe == probe.parent:
            raise ValueError(f"cannot resolve parent of {path}")
        probe = probe.parent
    resolved = probe.resolve(strict=True)
    for name in reversed(missing):
        resolved /= name
    return resolved


def repo_top(repo: pathlib.Path) -> pathlib.Path:
    return canonical_existing(pathlib.Path(run_git(repo, "rev-parse", "--show-toplevel")))


def common_dir(repo: pathlib.Path) -> pathlib.Path:
    raw = pathlib.Path(run_git(repo, "rev-parse", "--git-common-dir"))
    if not raw.is_absolute():
        raw = repo / raw
    return canonical_existing(raw)


def root_for(repo: pathlib.Path) -> pathlib.Path:
    value = os.environ.get(ROOT_ENV)
    if not value:
        value = os.environ.get("XDG_CACHE_HOME")
        if not value:
            home = os.environ.get("HOME")
            if not home:
                raise ValueError("HOME is required when XDG_CACHE_HOME is unset")
            value = str(pathlib.Path(home) / ".cache")
        value = str(pathlib.Path(value) / "rivet" / "cargo-targets")
    candidate = pathlib.Path(value)
    if not candidate.is_absolute():
        raise ValueError(f"{ROOT_ENV} must be absolute: {value}")
    root = canonical_parent(candidate)
    top = repo_top(repo)
    common = common_dir(repo)
    for forbidden in (top, common, common.parent):
        try:
            root.relative_to(forbidden)
        except ValueError:
            continue
        raise ValueError(f"Cargo target root {root} is inside repository path {forbidden}")
    return root


def namespace_mode() -> str:
    mode = os.environ.get(NAMESPACE_ENV, "iterative")
    if mode not in {"iterative", "strict"}:
        raise ValueError(f"{NAMESPACE_ENV} must be iterative or strict: {mode}")
    return mode


def digest_text(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def lexical_absolute(path: pathlib.Path) -> pathlib.Path:
    return pathlib.Path(os.path.abspath(path))


def check_directory_path(path: pathlib.Path) -> None:
    """Reject symlink and non-directory components that already exist."""
    path = lexical_absolute(path)
    current = pathlib.Path(path.anchor)
    for component in path.parts[1:]:
        current /= component
        try:
            info = current.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode):
            raise ValueError(f"managed directory is a symlink: {current}")
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError(f"managed directory is not a directory: {current}")


def namespace(repo: pathlib.Path) -> dict[str, str]:
    top = repo_top(repo)
    common = common_dir(repo)
    root = root_for(repo)
    mode = namespace_mode()
    repo_id = digest_text(str(common))
    checkout_id = digest_text(str(top))
    base = root / repo_id / checkout_id
    target = base / mode
    strict = base / "strict"
    result = {
        "root": str(root),
        "common_dir": str(common),
        "top_level": str(top),
        "repo_id": repo_id,
        "checkout_id": checkout_id,
        "base": str(base),
        "mode": mode,
        "target": str(target),
        "strict": str(strict),
        "state_digest": str(strict / "state-digest"),
        "prior_state_digest": str(strict / "prior-state-digest"),
        "group_lock": str(root / repo_id / ".group.lock"),
        "checkout_lock": str(base / ".checkout.lock"),
    }
    for path in (base, target, strict):
        check_directory_path(path)
    return result


def tracked_paths(repo: pathlib.Path) -> list[pathlib.Path]:
    raw = subprocess.check_output(
        ["git", "-C", str(repo), "ls-files", "-z", "--cached", "--others", "--exclude-standard"]
    )
    values = raw.decode(errors="surrogateescape").split("\0")
    return [repo / value for value in values if value]


def submodule_modes(repo: pathlib.Path) -> dict[str, str]:
    result: dict[str, str] = {}
    raw = subprocess.check_output(["git", "-C", str(repo), "ls-files", "-s", "-z"])
    for record in raw.decode(errors="surrogateescape").split("\0"):
        if not record:
            continue
        left, path = record.split("\t", 1)
        mode = left.split()[0]
        if mode == "160000":
            sub = repo / path
            try:
                identity = run_git(sub, "rev-parse", "HEAD")
            except (OSError, subprocess.CalledProcessError):
                identity = "unavailable"
            result[path] = f"{mode}:{identity}"
    return result


def file_record(repo: pathlib.Path, path: pathlib.Path, submodules: dict[str, str]) -> str:
    relative = path.relative_to(repo).as_posix()
    try:
        info = path.lstat()
    except FileNotFoundError:
        return f"missing\0{relative}\n"
    mode = stat.S_IMODE(info.st_mode)
    if stat.S_ISLNK(info.st_mode):
        payload = os.readlink(path).encode(errors="surrogateescape")
        kind = "symlink"
    elif stat.S_ISREG(info.st_mode):
        with path.open("rb") as stream:
            payload = stream.read()
        kind = "file"
    elif stat.S_ISDIR(info.st_mode):
        payload = b""
        kind = "directory"
    else:
        payload = b""
        kind = "other"
    marker = submodules.get(relative, "")
    digest = hashlib.sha256(payload).hexdigest()
    return f"{kind}\0{relative}\0{mode:o}\0{digest}\0{marker}\n"


def build_flags() -> dict[str, str]:
    names = {
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_INCREMENTAL",
        "CARGO_PROFILE_DEV_DEBUG",
        "CARGO_PROFILE_DEV_OPT_LEVEL",
        "CARGO_PROFILE_RELEASE_DEBUG",
        "CARGO_PROFILE_RELEASE_LTO",
        "CARGO_PROFILE_RELEASE_OPT_LEVEL",
        "CARGO_TARGET_DIR",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "RUSTUP_TOOLCHAIN",
    }
    return {name: os.environ.get(name, "") for name in sorted(names)}


def strict_digest(repo: pathlib.Path) -> str:
    ns = namespace(repo)
    lines = ["rivet-strict-state-v1", f"HEAD\0{run_git(repo, 'rev-parse', 'HEAD')}\n"]
    lines.append(f"common\0{ns['common_dir']}\ntop\0{ns['top_level']}\n")
    for key, value in build_flags().items():
        lines.append(f"env\0{key}\0{value}\n")
    submodules = submodule_modes(repo)
    for path in sorted(tracked_paths(repo), key=lambda p: p.relative_to(repo).as_posix()):
        lines.append(file_record(repo, path, submodules))
    return hashlib.sha256("".join(lines).encode()).hexdigest()


def mkdir_no_symlink(path: pathlib.Path) -> None:
    path = lexical_absolute(path)
    missing: list[pathlib.Path] = []
    probe = path
    while True:
        try:
            info = probe.lstat()
        except FileNotFoundError:
            missing.append(probe)
            if probe == probe.parent:
                raise ValueError(f"cannot create managed directory: {path}")
            probe = probe.parent
            continue
        if stat.S_ISLNK(info.st_mode):
            raise ValueError(f"managed directory is a symlink: {probe}")
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError(f"managed directory is not a directory: {probe}")
        break
    for child in reversed(missing):
        try:
            child.mkdir()
        except FileExistsError:
            info = child.lstat()
            if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
                raise ValueError(f"managed directory is not a real directory: {child}")


def ensure_namespace(repo: pathlib.Path) -> dict[str, str]:
    ns = namespace(repo)
    target = pathlib.Path(ns["target"])
    strict = pathlib.Path(ns["strict"])
    mkdir_no_symlink(target)
    mkdir_no_symlink(strict)
    marker = target / MARKER
    try:
        info = marker.lstat()
    except FileNotFoundError:
        info = None
    if info is not None and (stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode)):
        raise ValueError(f"managed target marker is not a regular file: {marker}")
    marker.write_text(
        json.dumps(
            {
                "version": 1,
                "repo_id": ns["repo_id"],
                "checkout_id": ns["checkout_id"],
                "target": ns["target"],
            },
            sort_keys=True,
        )
        + "\n"
    )
    return ns


def managed_path(ns: dict[str, str], raw: pathlib.Path) -> pathlib.Path:
    target = lexical_absolute(pathlib.Path(ns["target"]))
    path = lexical_absolute(raw if raw.is_absolute() else target / raw)
    try:
        relative = path.relative_to(target)
    except ValueError as exc:
        raise ValueError(f"deliverable is outside managed target: {path}") from exc
    if not relative.parts:
        raise ValueError(f"deliverable is the managed target directory: {path}")
    check_directory_path(target)
    current = target
    for component in relative.parts[:-1]:
        current /= component
        try:
            info = current.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode):
            raise ValueError(f"deliverable parent is a symlink: {current}")
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError(f"deliverable parent is not a directory: {current}")
    try:
        info = path.lstat()
    except FileNotFoundError:
        return path
    if stat.S_ISLNK(info.st_mode):
        raise ValueError(f"deliverable is a symlink: {path}")
    if stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode):
        canonical_target = target.resolve(strict=True)
        canonical_path = path.resolve(strict=True)
        try:
            canonical_path.relative_to(canonical_target)
        except ValueError as exc:
            raise ValueError(f"deliverable canonical path escapes managed target: {path}") from exc
    return path


def regular_non_symlink(path: pathlib.Path) -> bool:
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    return stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode)


def reject_symlink(path: pathlib.Path, label: str) -> None:
    try:
        info = path.lstat()
    except FileNotFoundError:
        return
    if stat.S_ISLNK(info.st_mode):
        raise ValueError(f"{label} is a symlink: {path}")


def sidecar_path(path: pathlib.Path) -> pathlib.Path:
    return pathlib.Path(str(path) + SIDECAR_SUFFIX)


def parse_fields(path: pathlib.Path) -> dict[str, str]:
    if not regular_non_symlink(path):
        raise ValueError(f"provenance sidecar is not a regular file: {path}")
    fields: dict[str, str] = {}
    for line in path.read_text().splitlines():
        if not line:
            continue
        if "=" not in line:
            raise ValueError(f"invalid provenance sidecar line: {path}")
        key, value = line.split("=", 1)
        if not key or key in fields:
            raise ValueError(f"invalid provenance sidecar field: {path}")
        fields[key] = value
    return fields


def write_sidecar(repo: pathlib.Path, path: pathlib.Path) -> pathlib.Path:
    ns = namespace(repo)
    path = managed_path(ns, path)
    ensure_namespace(repo)
    reject_symlink(path, "binary")
    if not regular_non_symlink(path):
        raise ValueError(f"binary is not a regular file: {path}")
    canonical_path = canonical_existing(path)
    sidecar = sidecar_path(path)
    reject_symlink(sidecar, "provenance sidecar")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    sidecar.write_text(
        "\n".join(
            [
                "version=1",
                f"repo_id={ns['repo_id']}",
                f"checkout_id={ns['checkout_id']}",
                f"head={run_git(repo, 'rev-parse', 'HEAD')}",
                f"state_digest={strict_digest(repo)}",
                f"target={ns['target']}",
                f"path={canonical_path}",
                f"sha256={digest}",
                "",
            ]
        )
    )
    return sidecar


def verify_sidecar(repo: pathlib.Path, path: pathlib.Path) -> None:
    ns = namespace(repo)
    path = managed_path(ns, path)
    reject_symlink(path, "binary")
    if not regular_non_symlink(path):
        raise ValueError(f"binary is not a regular file: {path}")
    canonical_path = canonical_existing(path)
    sidecar = sidecar_path(path)
    reject_symlink(sidecar, "provenance sidecar")
    fields = parse_fields(sidecar)
    expected = {
        "version": "1",
        "repo_id": ns["repo_id"],
        "checkout_id": ns["checkout_id"],
        "head": run_git(repo, "rev-parse", "HEAD"),
        "state_digest": strict_digest(repo),
        "target": ns["target"],
        "path": str(canonical_path),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }
    if fields != expected:
        for key, value in expected.items():
            if fields.get(key) != value:
                raise ValueError(f"provenance mismatch for {path}: {key}")
        raise ValueError(f"provenance sidecar has unexpected fields: {sidecar}")


def valid_sidecar(repo: pathlib.Path, path: pathlib.Path) -> bool:
    try:
        verify_sidecar(repo, path)
    except (OSError, ValueError, subprocess.CalledProcessError):
        return False
    return True


def receipt_path(ns: dict[str, str]) -> pathlib.Path:
    return pathlib.Path(ns["target"]) / BUILD_RECEIPT


def write_receipt(path: pathlib.Path, data: dict[str, object]) -> None:
    reject_symlink(path, "build receipt")
    temporary = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(data, sort_keys=True) + "\n")
    os.replace(temporary, path)


def read_receipt(path: pathlib.Path) -> dict[str, object]:
    if not regular_non_symlink(path):
        raise ValueError(f"missing build receipt: {path}")
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"invalid build receipt: {path}")
    return value


def normalized_paths(ns: dict[str, str], paths: Iterable[pathlib.Path]) -> list[pathlib.Path]:
    result: list[pathlib.Path] = []
    seen: set[str] = set()
    for raw in paths:
        path = managed_path(ns, raw)
        if str(path) in seen:
            raise ValueError(f"duplicate deliverable: {path}")
        seen.add(str(path))
        result.append(path)
    return result


def prepare_deliverables(repo: pathlib.Path, paths: Iterable[pathlib.Path]) -> None:
    ns = namespace(repo)
    target = pathlib.Path(ns["target"])
    deliverables = normalized_paths(ns, paths)
    if not deliverables:
        raise ValueError("prepare requires at least one deliverable")
    ensure_namespace(repo)
    entries: dict[str, dict[str, object]] = {}
    for path in deliverables:
        sidecar = sidecar_path(path)
        reject_symlink(path, "deliverable")
        reject_symlink(sidecar, "provenance sidecar")
        valid = namespace_mode() == "iterative" and valid_sidecar(repo, path)
        if valid:
            entries[str(path)] = {"valid": True, "recreate": False}
            continue
        if path.exists():
            if not regular_non_symlink(path):
                raise ValueError(f"deliverable is not a regular file: {path}")
            path.unlink()
        if sidecar.exists():
            if not regular_non_symlink(sidecar):
                raise ValueError(f"provenance sidecar is not a regular file: {sidecar}")
            sidecar.unlink()
        if path.exists() or sidecar.exists():
            raise ValueError(f"could not remove stale deliverable: {path}")
        entries[str(path)] = {"valid": False, "recreate": True}
    prepared_ns = time.time_ns()
    write_receipt(
        receipt_path(ns),
        {
            "version": 1,
            "repo_id": ns["repo_id"],
            "checkout_id": ns["checkout_id"],
            "target": ns["target"],
            "mode": ns["mode"],
            "head": run_git(repo, "rev-parse", "HEAD"),
            "state_digest": strict_digest(repo),
            "prepared_ns": prepared_ns,
            "paths": entries,
        },
    )


def stamp_deliverables(repo: pathlib.Path, paths: Iterable[pathlib.Path]) -> None:
    ns = namespace(repo)
    requested_paths = normalized_paths(ns, paths)
    if not requested_paths:
        raise ValueError("stamp requires at least one deliverable")
    receipt = receipt_path(ns)
    data = read_receipt(receipt)
    expected_receipt = {
        "version": 1,
        "repo_id": ns["repo_id"],
        "checkout_id": ns["checkout_id"],
        "target": ns["target"],
        "mode": ns["mode"],
        "head": run_git(repo, "rev-parse", "HEAD"),
        "state_digest": strict_digest(repo),
    }
    for key, value in expected_receipt.items():
        if data.get(key) != value:
            raise ValueError(f"build receipt mismatch for {ns['target']}: {key}")
    raw_entries = data.get("paths")
    if not isinstance(raw_entries, dict):
        raise ValueError(f"invalid build receipt paths: {receipt}")
    requested = {str(path) for path in requested_paths}
    if requested != set(raw_entries):
        raise ValueError("stamp paths do not match the prepared deliverables")
    prepared_ns = data.get("prepared_ns")
    if not isinstance(prepared_ns, int) or prepared_ns <= 0:
        raise ValueError(f"invalid build receipt timestamp: {receipt}")
    for raw_path, raw_entry in raw_entries.items():
        path = managed_path(ns, pathlib.Path(raw_path))
        if str(path) != raw_path or not isinstance(raw_entry, dict):
            raise ValueError(f"invalid build receipt entry: {raw_path}")
        if raw_entry.get("valid") is True:
            verify_sidecar(repo, path)
            continue
        if raw_entry.get("recreate") is not True:
            raise ValueError(f"deliverable was not prepared for rebuild: {path}")
        reject_symlink(path, "deliverable")
        if not regular_non_symlink(path):
            raise ValueError(f"expected deliverable was not recreated: {path}")
        canonical_path = canonical_existing(path)
        if canonical_path != path:
            raise ValueError(f"deliverable canonical path changed: {path}")
        if path.stat().st_mtime_ns < prepared_ns:
            raise ValueError(f"deliverable predates its build receipt: {path}")
        sidecar = sidecar_path(path)
        reject_symlink(sidecar, "provenance sidecar")
        if sidecar.exists():
            raise ValueError(f"unexpected pre-existing provenance sidecar: {sidecar}")
    for raw_path, raw_entry in raw_entries.items():
        if raw_entry.get("recreate") is True:
            write_sidecar(repo, pathlib.Path(raw_path))
    receipt.unlink()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["namespace", "digest", "ensure", "prepare", "stamp", "sidecar", "verify"])
    parser.add_argument("repo")
    parser.add_argument("paths", nargs="*")
    args = parser.parse_args()
    repo = pathlib.Path(args.repo).resolve()
    if args.command == "namespace":
        if args.paths:
            raise ValueError("namespace does not accept paths")
        print(json.dumps(namespace(repo), sort_keys=True))
    elif args.command == "digest":
        if args.paths:
            raise ValueError("digest does not accept paths")
        print(strict_digest(repo))
    elif args.command == "ensure":
        if args.paths:
            raise ValueError("ensure does not accept paths")
        print(json.dumps(ensure_namespace(repo), sort_keys=True))
    elif args.command == "prepare":
        if not args.paths:
            raise ValueError("prepare requires at least one path")
        prepare_deliverables(repo, (pathlib.Path(path) for path in args.paths))
    elif args.command == "stamp":
        if not args.paths:
            raise ValueError("stamp requires at least one path")
        stamp_deliverables(repo, (pathlib.Path(path) for path in args.paths))
    elif args.command == "sidecar":
        if len(args.paths) != 1:
            raise ValueError("sidecar requires one path")
        print(write_sidecar(repo, pathlib.Path(args.paths[0])))
    elif args.command == "verify":
        if len(args.paths) != 1:
            raise ValueError("verify requires one path")
        verify_sidecar(repo, pathlib.Path(args.paths[0]))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"cargo provenance: {exc}", file=sys.stderr)
        raise SystemExit(2)
