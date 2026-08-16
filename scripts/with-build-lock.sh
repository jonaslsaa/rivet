#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  printf 'usage: %s REPO_DIR COMMAND [ARGUMENT ...]\n' "$0" >&2
  exit 2
fi

repo_dir=$(cd "$1" && pwd -P)
shift
# shellcheck source=scripts/cargo-target-dir.sh
source "$repo_dir/scripts/cargo-target-dir.sh"
cargo_export_namespace "$repo_dir"

namespace=$(cargo_namespace_json "$repo_dir")
group_lock=$(printf '%s\n' "$namespace" | python3 -c 'import json,sys; print(json.load(sys.stdin)["group_lock"])')
checkout_lock=$(printf '%s\n' "$namespace" | python3 -c 'import json,sys; print(json.load(sys.stdin)["checkout_lock"])')

if [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ] && [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ]; then
  if python3 - "$group_lock" "$checkout_lock" <<'PY'
import fcntl
import os
import stat
import sys


def authenticate(fd_number: int, expected_path: str) -> None:
    expected = os.lstat(expected_path)
    if stat.S_ISLNK(expected.st_mode) or not stat.S_ISREG(expected.st_mode) or expected.st_nlink != 1:
        raise OSError("managed lock is not a unique regular file")
    actual = os.fstat(fd_number)
    if (
        not stat.S_ISREG(actual.st_mode)
        or actual.st_nlink != 1
        or actual.st_dev != expected.st_dev
        or actual.st_ino != expected.st_ino
    ):
        raise OSError("inherited descriptor is not the managed lock inode")
    fcntl.flock(fd_number, fcntl.LOCK_EX | fcntl.LOCK_NB)


try:
    authenticate(8, sys.argv[1])
    authenticate(9, sys.argv[2])
except (OSError, ValueError):
    raise SystemExit(1)
PY
  then
    exec "$@"
  fi
fi

python3 - "$group_lock" "$checkout_lock" "$@" <<'PY'
import fcntl
import os
import stat
import sys


def open_directory(path: str) -> int:
    path = os.path.abspath(path)
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path.anchor if hasattr(path, "anchor") else os.sep, flags)
    components = [item for item in path.split(os.sep) if item]
    try:
        for component in components:
            next_fd = os.open(component, flags, dir_fd=fd)
            info = os.fstat(next_fd)
            if not stat.S_ISDIR(info.st_mode):
                os.close(next_fd)
                raise OSError("lock parent is not a directory")
            os.close(fd)
            fd = next_fd
        return fd
    except BaseException:
        os.close(fd)
        raise


def open_lock(path: str) -> int:
    parent = os.path.dirname(path)
    name = os.path.basename(path)
    parent_fd = open_directory(parent)
    try:
        flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
        fd = os.open(name, flags, 0o600, dir_fd=parent_fd)
    finally:
        os.close(parent_fd)
    expected = os.lstat(path)
    actual = os.fstat(fd)
    if (
        stat.S_ISLNK(expected.st_mode)
        or not stat.S_ISREG(expected.st_mode)
        or expected.st_nlink != 1
        or not stat.S_ISREG(actual.st_mode)
        or actual.st_nlink != 1
        or expected.st_dev != actual.st_dev
        or expected.st_ino != actual.st_ino
    ):
        os.close(fd)
        raise OSError("managed lock inode changed or is not unique")
    fcntl.flock(fd, fcntl.LOCK_EX)
    return fd


paths = sys.argv[1:3]
argv = sys.argv[3:]
fds = []
try:
    for path in paths:
        fds.append(open_lock(path))
    os.dup2(fds[0], 8)
    os.dup2(fds[-1], 9)
    os.set_inheritable(8, True)
    os.set_inheritable(9, True)
    os.environ["RIVET_BUILD_GROUP_LOCK_FD"] = "8"
    os.environ["RIVET_BUILD_LOCK_FD"] = "9"
    os.execvp(argv[0], argv)
finally:
    for fd in fds:
        try:
            os.close(fd)
        except OSError:
            pass
PY
