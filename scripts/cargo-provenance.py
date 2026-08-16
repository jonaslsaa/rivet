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
from typing import Iterable

MARKER = ".rivet-cargo-target"
SIDECAR_SUFFIX = ".rivet-provenance"
ROOT_ENV = "RIVET_CARGO_TARGET_ROOT"
TARGET_ENV = "CARGO_TARGET_DIR"


def run_git(repo: pathlib.Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def canonical_existing(path: pathlib.Path) -> pathlib.Path:
    return path.resolve(strict=True)


def canonical_parent(path: pathlib.Path) -> pathlib.Path:
    path = path.absolute()
    missing: list[str] = []
    probe = path
    while not probe.exists():
        missing.append(probe.name)
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
    if value is None or value == "":
        value = os.environ.get("XDG_CACHE_HOME")
        if value is None or value == "":
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


def digest_text(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def namespace(repo: pathlib.Path) -> dict[str, str]:
    top = repo_top(repo)
    common = common_dir(repo)
    root = root_for(repo)
    repo_id = digest_text(str(common))
    checkout_id = digest_text(str(top))
    base = root / repo_id / checkout_id
    target = base / "iterative"
    strict = base / "strict"
    return {
        "root": str(root),
        "common_dir": str(common),
        "top_level": str(top),
        "repo_id": repo_id,
        "checkout_id": checkout_id,
        "base": str(base),
        "target": str(target),
        "strict": str(strict),
        "state_digest": str(strict / "state-digest"),
        "prior_state_digest": str(strict / "prior-state-digest"),
        "group_lock": str(root / repo_id / ".group.lock"),
        "checkout_lock": str(base / ".checkout.lock"),
    }


def tracked_paths(repo: pathlib.Path) -> list[pathlib.Path]:
    raw = subprocess.check_output(
        ["git", "-C", str(repo), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
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


def ensure_namespace(repo: pathlib.Path) -> dict[str, str]:
    ns = namespace(repo)
    pathlib.Path(ns["target"]).mkdir(parents=True, exist_ok=True)
    pathlib.Path(ns["strict"]).mkdir(parents=True, exist_ok=True)
    marker = pathlib.Path(ns["target"]) / MARKER
    marker.write_text(
        json.dumps({"version": 1, "repo_id": ns["repo_id"], "checkout_id": ns["checkout_id"], "target": ns["target"]}, sort_keys=True)
        + "\n"
    )
    return ns


def write_sidecar(repo: pathlib.Path, path: pathlib.Path) -> pathlib.Path:
    ns = ensure_namespace(repo)
    if not path.is_file():
        raise ValueError(f"binary is not a file: {path}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    sidecar = pathlib.Path(str(path) + SIDECAR_SUFFIX)
    sidecar.write_text(
        "\n".join(
            [
                "version=1",
                f"repo_id={ns['repo_id']}",
                f"checkout_id={ns['checkout_id']}",
                f"head={run_git(repo, 'rev-parse', 'HEAD')}",
                f"state_digest={strict_digest(repo)}",
                f"target={ns['target']}",
                f"path={path.resolve()}",
                f"sha256={digest}",
                "",
            ]
        )
    )
    return sidecar


def stamp_tree(repo: pathlib.Path) -> None:
    ns = ensure_namespace(repo)
    target = pathlib.Path(ns["target"])
    if not target.is_dir():
        return
    for path in target.rglob("*"):
        if not path.is_file() or path.name.endswith(SIDECAR_SUFFIX):
            continue
        try:
            mode = path.stat().st_mode
        except OSError:
            continue
        if mode & stat.S_IXUSR:
            try:
                write_sidecar(repo, path)
            except (OSError, ValueError):
                pass


def verify_sidecar(repo: pathlib.Path, path: pathlib.Path) -> None:
    ns = namespace(repo)
    sidecar = pathlib.Path(str(path) + SIDECAR_SUFFIX)
    if not sidecar.is_file():
        raise ValueError(f"missing provenance sidecar: {sidecar}")
    fields: dict[str, str] = {}
    for line in sidecar.read_text().splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            fields[key] = value
    expected = {
        "version": "1",
        "repo_id": ns["repo_id"],
        "checkout_id": ns["checkout_id"],
        "head": run_git(repo, "rev-parse", "HEAD"),
        "state_digest": strict_digest(repo),
        "target": ns["target"],
        "path": str(path.resolve()),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }
    for key, value in expected.items():
        if fields.get(key) != value:
            raise ValueError(f"provenance mismatch for {path}: {key}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["namespace", "digest", "ensure", "stamp", "sidecar", "verify"])
    parser.add_argument("repo")
    parser.add_argument("path", nargs="?")
    args = parser.parse_args()
    repo = pathlib.Path(args.repo).resolve()
    if args.command == "namespace":
        print(json.dumps(namespace(repo), sort_keys=True))
    elif args.command == "digest":
        print(strict_digest(repo))
    elif args.command == "ensure":
        print(json.dumps(ensure_namespace(repo), sort_keys=True))
    elif args.command == "stamp":
        stamp_tree(repo)
    elif args.command == "sidecar":
        if not args.path:
            raise ValueError("sidecar requires a path")
        print(write_sidecar(repo, pathlib.Path(args.path)))
    elif args.command == "verify":
        if not args.path:
            raise ValueError("verify requires a path")
        verify_sidecar(repo, pathlib.Path(args.path))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"cargo provenance: {exc}", file=sys.stderr)
        raise SystemExit(2)
