#!/usr/bin/env python3
"""Resolve and attest Rivet's per-checkout Cargo namespace."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import pathlib
import shutil
import stat
import struct
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
    output = subprocess.check_output(["git", "-C", str(repo), *args])
    if output.endswith(b"\n"):
        output = output[:-1]
    return os.fsdecode(output)


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


def path_bytes(path: pathlib.Path | str) -> bytes:
    return os.fsencode(os.fspath(path))


def digest_record(*fields: bytes | str) -> bytes:
    encoded = bytearray()
    for field in fields:
        value = field if isinstance(field, bytes) else os.fsencode(field)
        encoded.extend(struct.pack(">Q", len(value)))
        encoded.extend(value)
    return bytes(encoded)


def file_record(repo: pathlib.Path, path: pathlib.Path, submodules: dict[str, str]) -> bytes:
    relative = path.relative_to(repo).as_posix()
    relative_bytes = path_bytes(path.relative_to(repo))
    try:
        info = path.lstat()
    except FileNotFoundError:
        return digest_record(b"missing", relative_bytes)
    mode = stat.S_IMODE(info.st_mode)
    if stat.S_ISLNK(info.st_mode):
        payload = path_bytes(os.readlink(path))
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
    return digest_record(kind, relative_bytes, f"{mode:o}", digest, marker)


def wrapper_fingerprint(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        return ""
    candidate = pathlib.Path(value)
    if not candidate.is_absolute():
        resolved_name = shutil.which(value)
        if resolved_name is None:
            return f"{value}\0missing"
        candidate = pathlib.Path(resolved_name)
    try:
        resolved = candidate.resolve(strict=True)
        info = resolved.stat()
        if not stat.S_ISREG(info.st_mode):
            return f"{value}\0{resolved}\0not-regular"
        digest = hashlib.sha256(resolved.read_bytes()).hexdigest()
        return f"{value}\0{resolved}\0{stat.S_IMODE(info.st_mode):o}\0{digest}"
    except (OSError, ValueError):
        return f"{value}\0missing"


def compiler_fingerprint() -> str:
    configured = os.environ.get("RUSTC", "")
    if configured:
        return wrapper_fingerprint("RUSTC")
    resolved = shutil.which("rustc")
    if resolved is None:
        return "rustc\0missing"
    original = os.environ.get("RUSTC")
    try:
        os.environ["RUSTC"] = resolved
        return wrapper_fingerprint("RUSTC")
    finally:
        if original is None:
            os.environ.pop("RUSTC", None)
        else:
            os.environ["RUSTC"] = original


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
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "RUSTUP_TOOLCHAIN",
    }
    flags = {name: os.environ.get(name, "") for name in sorted(names)}
    for name in ("CARGO_BUILD_RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_WRAPPER"):
        flags[f"{name}_CONTENT"] = wrapper_fingerprint(name)
    flags["RUSTC_CONTENT"] = compiler_fingerprint()
    return flags


def strict_digest(repo: pathlib.Path) -> str:
    ns = namespace(repo)
    records = bytearray(b"rivet-strict-state-v2\0")
    records.extend(digest_record("HEAD", run_git(repo, "rev-parse", "HEAD")))
    records.extend(digest_record("common", path_bytes(ns["common_dir"])))
    records.extend(digest_record("top", path_bytes(ns["top_level"])))
    for key, value in build_flags().items():
        records.extend(digest_record("env", key, value))
    submodules = submodule_modes(repo)
    for path in sorted(tracked_paths(repo), key=lambda p: path_bytes(p.relative_to(repo))):
        records.extend(file_record(repo, path, submodules))
    return hashlib.sha256(records).hexdigest()


def mkdir_no_symlink(path: pathlib.Path) -> None:
    fd, _ = open_directory(path, create=True)
    os.close(fd)


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
    reject_hardlink(marker, "managed target marker")
    write_text_atomic(
        marker,
        json.dumps(
            {
                "version": 1,
                "repo_id": ns["repo_id"],
                "checkout_id": ns["checkout_id"],
                "target": ns["target"],
            },
            sort_keys=True,
        )
        + "\n",
        "managed target marker",
        pathlib.Path(ns["root"]),
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


def reject_hardlink(path: pathlib.Path, label: str) -> None:
    try:
        info = path.lstat()
    except FileNotFoundError:
        return
    if stat.S_ISREG(info.st_mode) and info.st_nlink > 1:
        raise ValueError(f"{label} is a hardlink: {path}")


def open_directory(
    path: pathlib.Path,
    create: bool,
    root: pathlib.Path | None = None,
) -> tuple[int, list[tuple[pathlib.Path, os.stat_result]]]:
    path = lexical_absolute(path)
    if root is None:
        root = pathlib.Path(path.anchor)
    else:
        root = lexical_absolute(root)
        try:
            relative = path.relative_to(root)
        except ValueError as exc:
            raise ValueError(f"managed path is outside authenticated root: {path}") from exc
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(root, flags)
    snapshots: list[tuple[pathlib.Path, os.stat_result]] = []
    root_info = os.fstat(fd)
    if not stat.S_ISDIR(root_info.st_mode):
        os.close(fd)
        raise ValueError(f"managed path is not a directory: {root}")
    if root != pathlib.Path(path.anchor):
        snapshots.append((root, root_info))
        components = relative.parts
        current = root
    else:
        components = path.parts[1:]
        current = pathlib.Path(path.anchor)
    try:
        for component in components:
            child = current / component
            try:
                next_fd = os.open(component, flags, dir_fd=fd)
            except FileNotFoundError:
                if not create:
                    raise
                os.mkdir(component, 0o700, dir_fd=fd)
                next_fd = os.open(component, flags, dir_fd=fd)
            info = os.fstat(next_fd)
            if not stat.S_ISDIR(info.st_mode):
                os.close(next_fd)
                raise ValueError(f"managed path is not a directory: {child}")
            snapshots.append((child, info))
            os.close(fd)
            fd = next_fd
            current = child
        return fd, snapshots
    except BaseException:
        os.close(fd)
        raise


def revalidate_directories(snapshots: list[tuple[pathlib.Path, os.stat_result]]) -> None:
    for path, expected in snapshots:
        try:
            actual = path.lstat()
        except OSError as exc:
            raise ValueError(f"managed directory changed during operation: {path}") from exc
        if (
            stat.S_ISLNK(actual.st_mode)
            or not stat.S_ISDIR(actual.st_mode)
            or actual.st_dev != expected.st_dev
            or actual.st_ino != expected.st_ino
        ):
            raise ValueError(f"managed directory changed during operation: {path}")


def read_regular_file_authenticated(
    path: pathlib.Path,
    label: str,
    root: pathlib.Path,
) -> bytes:
    path = lexical_absolute(path)
    parent_fd, snapshots = open_directory(path.parent, create=False, root=root)
    fd: int | None = None
    try:
        name = path.name
        info = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
        if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
            raise ValueError(f"{label} is not a regular file: {path}")
        if info.st_nlink > 1:
            raise ValueError(f"{label} is a hardlink: {path}")
        fd = os.open(name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=parent_fd)
        actual = os.fstat(fd)
        if (
            stat.S_ISLNK(actual.st_mode)
            or not stat.S_ISREG(actual.st_mode)
            or actual.st_nlink > 1
            or actual.st_dev != info.st_dev
            or actual.st_ino != info.st_ino
        ):
            raise ValueError(f"{label} changed during open: {path}")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(fd, 1024 * 1024)
            if not chunk:
                break
            chunks.append(chunk)
        current = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
        if (
            stat.S_ISLNK(current.st_mode)
            or not stat.S_ISREG(current.st_mode)
            or current.st_nlink > 1
            or current.st_dev != actual.st_dev
            or current.st_ino != actual.st_ino
        ):
            raise ValueError(f"{label} changed during read: {path}")
        revalidate_directories(snapshots)
        return b"".join(chunks)
    finally:
        if fd is not None:
            os.close(fd)
        os.close(parent_fd)


def write_text_atomic(
    path: pathlib.Path,
    text: str,
    label: str,
    root: pathlib.Path | None = None,
) -> None:
    path = lexical_absolute(path)
    parent_fd, snapshots = open_directory(path.parent, create=False, root=root)
    temporary_name = f"{path.name}.{os.getpid()}.{time.time_ns()}.tmp"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    existing: os.stat_result | None
    try:
        try:
            existing = os.stat(path.name, dir_fd=parent_fd, follow_symlinks=False)
        except FileNotFoundError:
            existing = None
        if existing is not None:
            if stat.S_ISLNK(existing.st_mode):
                raise ValueError(f"{label} is a symlink: {path}")
            if not stat.S_ISREG(existing.st_mode):
                raise ValueError(f"{label} is not a regular file: {path}")
            if existing.st_nlink > 1:
                raise ValueError(f"{label} is a hardlink: {path}")
        try:
            fd = os.open(temporary_name, flags, 0o600, dir_fd=parent_fd)
        except FileExistsError as exc:
            raise ValueError(f"{label} temporary path already exists: {path.parent / temporary_name}") from exc
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                stream.write(text)
                stream.flush()
                os.fsync(stream.fileno())
            revalidate_directories(snapshots)
            try:
                current = os.stat(path.name, dir_fd=parent_fd, follow_symlinks=False)
            except FileNotFoundError:
                current = None
            if (existing is None) != (current is None) or (
                existing is not None
                and current is not None
                and (existing.st_dev != current.st_dev or existing.st_ino != current.st_ino)
            ):
                raise ValueError(f"{label} changed during operation: {path}")
            os.replace(
                temporary_name,
                path.name,
                src_dir_fd=parent_fd,
                dst_dir_fd=parent_fd,
            )
            os.fsync(parent_fd)
        except BaseException:
            try:
                os.unlink(temporary_name, dir_fd=parent_fd)
            except FileNotFoundError:
                pass
            raise
    finally:
        os.close(parent_fd)


def sidecar_path(path: pathlib.Path) -> pathlib.Path:
    return pathlib.Path(str(path) + SIDECAR_SUFFIX)


def encoded_path(path: pathlib.Path) -> str:
    return base64.b64encode(path_bytes(path)).decode("ascii")


def parse_fields(path: pathlib.Path, root: pathlib.Path) -> dict[str, object]:
    value = json.loads(
        read_regular_file_authenticated(path, "provenance sidecar", root).decode("utf-8")
    )
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise ValueError(f"invalid provenance sidecar object: {path}")
    return value


def write_sidecar(repo: pathlib.Path, path: pathlib.Path) -> pathlib.Path:
    ns = namespace(repo)
    path = managed_path(ns, path)
    ensure_namespace(repo)
    reject_symlink(path, "binary")
    reject_hardlink(path, "binary")
    if not regular_non_symlink(path):
        raise ValueError(f"binary is not a regular file: {path}")
    canonical_path = canonical_existing(path)
    sidecar = sidecar_path(path)
    reject_symlink(sidecar, "provenance sidecar")
    reject_hardlink(sidecar, "provenance sidecar")
    digest = hashlib.sha256(
        read_regular_file_authenticated(path, "binary", pathlib.Path(ns["target"]))
    ).hexdigest()
    write_text_atomic(
        sidecar,
        json.dumps(
            {
                "version": 1,
                "repo_id": ns["repo_id"],
                "checkout_id": ns["checkout_id"],
                "head": run_git(repo, "rev-parse", "HEAD"),
                "state_digest": strict_digest(repo),
                "target_b64": encoded_path(pathlib.Path(ns["target"])),
                "path_b64": encoded_path(canonical_path),
                "sha256": digest,
            },
            sort_keys=True,
        )
        + "\n",
        "provenance sidecar",
        pathlib.Path(ns["root"]),
    )
    return sidecar


def verify_sidecar(repo: pathlib.Path, path: pathlib.Path) -> None:
    ns = namespace(repo)
    path = managed_path(ns, path)
    reject_symlink(path, "binary")
    reject_hardlink(path, "binary")
    if not regular_non_symlink(path):
        raise ValueError(f"binary is not a regular file: {path}")
    canonical_path = canonical_existing(path)
    sidecar = sidecar_path(path)
    reject_symlink(sidecar, "provenance sidecar")
    fields = parse_fields(sidecar, pathlib.Path(ns["target"]))
    expected = {
        "version": 1,
        "repo_id": ns["repo_id"],
        "checkout_id": ns["checkout_id"],
        "head": run_git(repo, "rev-parse", "HEAD"),
        "state_digest": strict_digest(repo),
        "target_b64": encoded_path(pathlib.Path(ns["target"])),
        "path_b64": encoded_path(canonical_path),
        "sha256": hashlib.sha256(
            read_regular_file_authenticated(path, "binary", pathlib.Path(ns["target"]))
        ).hexdigest(),
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


def write_receipt(
    path: pathlib.Path,
    data: dict[str, object],
    root: pathlib.Path | None = None,
) -> None:
    reject_symlink(path, "build receipt")
    reject_hardlink(path, "build receipt")
    write_text_atomic(
        path,
        json.dumps(data, sort_keys=True) + "\n",
        "build receipt",
        root,
    )


def read_receipt(path: pathlib.Path) -> dict[str, object]:
    if not regular_non_symlink(path):
        raise ValueError(f"missing build receipt: {path}")
    reject_hardlink(path, "build receipt")
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"invalid build receipt: {path}")
    return value


def state_file_value(path: pathlib.Path, directory_fd: int, name: str) -> str | None:
    try:
        info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
    except FileNotFoundError:
        return None
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
        raise ValueError(f"strict state file is not a regular file: {path}")
    if info.st_nlink > 1:
        raise ValueError(f"strict state file is a hardlink: {path}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(name, flags, dir_fd=directory_fd)
    try:
        actual = os.fstat(fd)
        if actual.st_dev != info.st_dev or actual.st_ino != info.st_ino:
            raise ValueError(f"strict state file changed during read: {path}")
        if actual.st_size > 1024:
            raise ValueError(f"strict state file is unexpectedly large: {path}")
        return os.read(fd, 1024).decode("utf-8")
    finally:
        os.close(fd)


def write_state_digests(repo: pathlib.Path, expected: str) -> None:
    ns = namespace(repo)
    actual = strict_digest(repo)
    if actual != expected:
        raise ValueError(f"strict state changed before recording: expected {expected}, got {actual}")
    strict = pathlib.Path(ns["strict"])
    root = pathlib.Path(ns["root"])
    directory_fd, snapshots = open_directory(strict, create=False, root=root)
    try:
        current_path = strict / "state-digest"
        prior_path = strict / "prior-state-digest"
        current = state_file_value(current_path, directory_fd, current_path.name)
        state_file_value(prior_path, directory_fd, prior_path.name)
        revalidate_directories(snapshots)
    finally:
        os.close(directory_fd)
    if current is not None:
        write_text_atomic(prior_path, current, "prior state digest", root)
    write_text_atomic(current_path, actual, "state digest", root)


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
        reject_hardlink(path, "deliverable")
        reject_symlink(sidecar, "provenance sidecar")
        reject_hardlink(sidecar, "provenance sidecar")
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
        pathlib.Path(ns["root"]),
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
        reject_hardlink(path, "deliverable")
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
    parser.add_argument(
        "command",
        choices=["namespace", "digest", "ensure", "prepare", "stamp", "sidecar", "verify", "record-state"],
    )
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
    elif args.command == "record-state":
        if len(args.paths) != 1:
            raise ValueError("record-state requires the expected digest")
        write_state_digests(repo, args.paths[0])
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
