#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
# shellcheck source=scripts/cargo-target-dir.sh
source "$SCRIPT_DIR/cargo-target-dir.sh"

DRY=0
IDLE_HOURS=24
REMOVED=0
PRUNED=0
FREED_KB=0
STRANDED=0

say() { printf '%s\n' "$*"; }
run() { if [ "$DRY" = 1 ]; then say "  DRY: $*"; else "$@"; fi; }

valid_hours() {
  case "${1:-}" in
    ''|*[!0-9]*) return 1 ;;
  esac
  local value=$1
  while [ "${value#0}" != "$value" ]; do value=${value#0}; done
  value=${value:-0}
  [ "$value" -le 8760 ] 2>/dev/null
}

canonical_dir() {
  [ -d "$1" ] || return 1
  (cd "$1" 2>/dev/null && pwd -P)
}

owned_marker() {
  local marker=$1 target=$2 expected_repo=$3
  python3 - "$marker" "$target" "$expected_repo" <<'PY'
import json
import os
import pathlib
import re
import stat
import sys

marker = pathlib.Path(sys.argv[1])
target = pathlib.Path(sys.argv[2])
expected_repo = sys.argv[3]
try:
    marker_info = marker.lstat()
    target_info = target.lstat()
    if (
        stat.S_ISLNK(marker_info.st_mode)
        or not stat.S_ISREG(marker_info.st_mode)
        or marker_info.st_nlink != 1
        or stat.S_ISLNK(target_info.st_mode)
        or not stat.S_ISDIR(target_info.st_mode)
    ):
        raise OSError("marker or target is not an authenticated object")
    fd = os.open(marker, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        actual = os.fstat(fd)
        if actual.st_dev != marker_info.st_dev or actual.st_ino != marker_info.st_ino:
            raise OSError("marker changed during read")
        data = json.loads(os.read(fd, 1024).decode("utf-8"))
    finally:
        os.close(fd)
except (OSError, ValueError, UnicodeError):
    raise SystemExit(1)
if data.get("version") != 1 or data.get("repo_id") != expected_repo:
    raise SystemExit(1)
if data.get("target") != str(target) or data.get("checkout_id") != target.parent.name:
    raise SystemExit(1)
if target.name != "iterative":
    raise SystemExit(1)
if target.parent.parent.name != expected_repo:
    raise SystemExit(1)
if not re.fullmatch(r"[0-9a-f]{64}", target.parent.name):
    raise SystemExit(1)
PY
}

lock_free() {
  local group=$1 checkout=$2
  python3 - "$DRY" "$group" "$checkout" <<'PY'
import fcntl
import os
import stat
import sys


def open_directory(path: str) -> int:
    path = os.path.abspath(path)
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(os.sep, flags)
    try:
        for component in [item for item in path.split(os.sep) if item]:
            next_fd = os.open(component, flags, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        return fd
    except BaseException:
        os.close(fd)
        raise


def open_lock(path: str, dry: bool) -> int | None:
    parent_fd = open_directory(os.path.dirname(path))
    name = os.path.basename(path)
    try:
        try:
            flags = os.O_RDWR | getattr(os, "O_NOFOLLOW", 0)
            fd = os.open(name, flags | (0 if dry else os.O_CREAT), 0o600, dir_fd=parent_fd)
        except FileNotFoundError:
            if dry:
                return None
            raise
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
        raise OSError("lock is not a unique regular managed inode")
    return fd


dry = sys.argv[1] == "1"
fds = []
try:
    for path in sys.argv[2:]:
        fd = open_lock(path, dry)
        if fd is None:
            continue
        fds.append(fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
except (OSError, ValueError):
    raise SystemExit(1)
finally:
    for fd in fds:
        try:
            os.close(fd)
        except OSError:
            pass
PY
}

process_env_live() {
  local target=$1 expected
  expected=$(canonical_dir "$target" 2>/dev/null) || return 1
  python3 - "$expected" "$$" <<'PY'
import os
import pathlib
import re
import subprocess
import sys

expected = sys.argv[1]
self_pid = sys.argv[2]
values: list[str] = []
proc = pathlib.Path("/proc")
if proc.is_dir():
    try:
        entries = list(proc.iterdir())
    except OSError:
        raise SystemExit(0)
    for entry in entries:
        if not entry.name.isdigit() or entry.name == self_pid:
            continue
        env_file = entry / "environ"
        try:
            raw = env_file.read_bytes()
        except OSError:
            raise SystemExit(0)
        for item in raw.split(b"\0"):
            if not item:
                continue
            if b"=" not in item:
                raise SystemExit(0)
            key, value = item.split(b"=", 1)
            if key in (b"CARGO_TARGET_DIR", b"RIVET_CARGO_TARGET_DIR"):
                values.append(value.decode(errors="surrogateescape"))
else:
    try:
        output = subprocess.check_output(
            ["ps", "eww", "-ax", "-o", "pid=,command="],
            text=True,
            errors="surrogateescape",
        )
    except (OSError, subprocess.CalledProcessError):
        raise SystemExit(0)
    lines = output.splitlines()
    if not lines:
        raise SystemExit(0)
    assignment = re.compile(r"(?:^|\s)(?:CARGO_TARGET_DIR|RIVET_CARGO_TARGET_DIR)=")
    for line in lines:
        fields = line.split(None, 1)
        if len(fields) != 2 or not fields[0].isdigit():
            raise SystemExit(0)
        if fields[0] == self_pid:
            continue
        payload = fields[1]
        matches = list(assignment.finditer(payload))
        for index, match in enumerate(matches):
            start = match.end()
            stop = matches[index + 1].start() if index + 1 < len(matches) else len(payload)
            segment = payload[start:stop]
            if not segment.strip():
                raise SystemExit(0)
            boundaries = [len(segment)]
            boundaries.extend(item.start() for item in re.finditer(r"\s+", segment))
            for end in boundaries:
                value = segment[:end].strip()
                if value:
                    values.append(value)
for value in values:
    path = pathlib.Path(value)
    if not path.is_absolute():
        raise SystemExit(0)
    try:
        if os.path.realpath(os.path.normpath(str(path))) == expected:
            raise SystemExit(0)
    except OSError:
        raise SystemExit(0)
raise SystemExit(1)
PY
}

deep_active() {
  local target=$1 minutes=$2 recent
  [ "$minutes" -lt 1 ] && minutes=1
  if ! recent=$(find -P "$target" -type f ! -name .rivet-cargo-target -mmin "-$minutes" -print -quit 2>/dev/null); then
    return 0
  fi
  [ -n "$recent" ]
}

dir_kb() {
  local output kb rest
  output=$(du -sk -- "$1" 2>/dev/null) || return 1
  IFS=$'\t ' read -r kb rest <<< "$output"
  case "$kb" in ''|*[!0-9]*) return 1 ;; esac
  printf '%s\n' "$kb"
}

remove_namespace() {
  local root=$1 target=$2 marker=$3
  python3 - "$root" "$target" "$marker" <<'PY'
import json
import os
import stat
import sys

root = os.path.abspath(sys.argv[1])
target = os.path.abspath(sys.argv[2])
marker = os.path.abspath(sys.argv[3])
flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
try:
    relative = os.path.relpath(target, root)
    if relative == os.curdir or relative.startswith(os.pardir + os.sep):
        raise OSError("target is outside managed root")
    root_info = os.lstat(root)
    if stat.S_ISLNK(root_info.st_mode) or not stat.S_ISDIR(root_info.st_mode):
        raise OSError("managed root is not a real directory")
    root_fd = os.open(root, flags)
except OSError:
    raise SystemExit(1)

fds = [root_fd]
snapshots = [(root, root_info)]
try:
    parent = root
    current = root
    for component in relative.split(os.sep):
        next_fd = os.open(component, flags, dir_fd=fds[-1])
        info = os.fstat(next_fd)
        if not stat.S_ISDIR(info.st_mode):
            raise OSError("namespace component is not a directory")
        fds.append(next_fd)
        current = os.path.join(current, component)
        snapshots.append((current, info))
    target_fd = fds[-1]
    marker_name = os.path.basename(marker)
    marker_info = os.stat(marker_name, dir_fd=target_fd, follow_symlinks=False)
    if (
        stat.S_ISLNK(marker_info.st_mode)
        or not stat.S_ISREG(marker_info.st_mode)
        or marker_info.st_nlink != 1
    ):
        raise OSError("marker is not an authenticated unique file")
    marker_fd = os.open(marker_name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=target_fd)
    try:
        marker_actual = os.fstat(marker_fd)
        if marker_actual.st_dev != marker_info.st_dev or marker_actual.st_ino != marker_info.st_ino:
            raise OSError("marker changed during authentication")
        data = json.loads(os.read(marker_fd, 1024).decode("utf-8"))
    finally:
        os.close(marker_fd)
    if data.get("target") != target:
        raise OSError("marker target mismatch")
    for path, expected in snapshots:
        actual = os.lstat(path)
        if (
            stat.S_ISLNK(actual.st_mode)
            or not stat.S_ISDIR(actual.st_mode)
            or actual.st_dev != expected.st_dev
            or actual.st_ino != expected.st_ino
        ):
            raise OSError("namespace changed during authentication")

    def remove_directory(directory_fd: int) -> None:
        for name in os.listdir(directory_fd):
            info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
            if stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode):
                child_fd = os.open(name, flags, dir_fd=directory_fd)
                try:
                    remove_directory(child_fd)
                finally:
                    os.close(child_fd)
                current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if current.st_dev != info.st_dev or current.st_ino != info.st_ino:
                    raise OSError("namespace child changed during deletion")
                os.rmdir(name, dir_fd=directory_fd)
            else:
                current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if current.st_dev != info.st_dev or current.st_ino != info.st_ino:
                    raise OSError("namespace entry changed during deletion")
                os.unlink(name, dir_fd=directory_fd)

    remove_directory(target_fd)
    target_actual = os.lstat(target)
    target_expected = snapshots[-1][1]
    if (
        stat.S_ISLNK(target_actual.st_mode)
        or not stat.S_ISDIR(target_actual.st_mode)
        or target_actual.st_dev != target_expected.st_dev
        or target_actual.st_ino != target_expected.st_ino
    ):
        raise OSError("namespace target changed during deletion")
    os.rmdir(os.path.basename(target), dir_fd=fds[-2])
except (OSError, ValueError, UnicodeError):
    raise SystemExit(1)
finally:
    for fd in reversed(fds):
        try:
            os.close(fd)
        except OSError:
            pass
PY
}

prune_namespace() {
  local root=$1 repo_id=$2 group_lock=$3 checkout_lock=$4 minutes=$5
  [ -d "$root" ] || return 0
  local markers
  if ! markers=$(find -P "$root" -type f -name .rivet-cargo-target -print 2>/dev/null); then
    say "KEEP   $root [marker scan failed]"
    return 0
  fi
  while IFS= read -r marker; do
    [ -n "$marker" ] || continue
    local target kb
    target=$(dirname "$marker")
    owned_marker "$marker" "$target" "$repo_id" || { say "KEEP   $target [unrecognized marker]"; continue; }
    [ "$(canonical_dir "$target" 2>/dev/null || true)" = "$target" ] || { say "KEEP   $target [symlinked namespace]"; continue; }
    if ! lock_free "$group_lock" "$checkout_lock"; then
      say "KEEP   $target [lock state uncertain or active]"
      continue
    fi
    if process_env_live "$target"; then
      say "KEEP   $target [managed process is live]"
      continue
    fi
    if deep_active "$target" "$minutes"; then
      say "KEEP   $target [activity within ${IDLE_HOURS}h]"
      continue
    fi
    if ! kb=$(dir_kb "$target"); then
      say "KEEP   $target [size scan failed]"
      continue
    fi
    say "$(if [ "$DRY" = 1 ]; then printf WOULD; else printf PRUNE; fi)  $target [$((kb / 1024))MB]"
    if [ "$DRY" = 1 ]; then
      say "  DRY: remove namespace $target"
      PRUNED=$((PRUNED + 1))
      FREED_KB=$((FREED_KB + kb))
    elif remove_namespace "$root" "$target" "$marker"; then
      PRUNED=$((PRUNED + 1))
      FREED_KB=$((FREED_KB + kb))
    else
      say "KEEP   $target [namespace changed during deletion]"
    fi
  done <<< "$markers"
}

worktree_sweep() {
  local common=$1 current=$2 wt branch head dirty status_rc merge_output merge_rc lock kb
  while IFS=$'\t' read -r wt lock; do
    [ -n "$wt" ] && [ "$wt" != "$current" ] && [ -d "$wt" ] || continue
    branch=$(git -C "$wt" symbolic-ref --short -q HEAD 2>/dev/null || printf '(detached)')
    head=$(git -C "$wt" rev-parse --verify HEAD^{commit} 2>/dev/null) || { say "KEEP   $wt [git HEAD probe failed]"; continue; }
    case "$head" in
      ''|*[!0-9a-fA-F]*) say "KEEP   $wt [$branch: malformed HEAD]"; continue ;;
    esac
    [ "${#head}" -eq 40 ] || [ "${#head}" -eq 64 ] || { say "KEEP   $wt [$branch: malformed HEAD]"; continue; }
    dirty=$(git -C "$wt" status --porcelain=v1 --untracked-files=all 2>/dev/null); status_rc=$?
    [ "$status_rc" -eq 0 ] || { say "KEEP   $wt [$branch: status probe failed]"; continue; }
    [ -z "$dirty" ] || { say "KEEP   $wt [$branch: dirty or malformed status]"; continue; }
    if [ -n "$lock" ]; then say "KEEP   $wt [$branch: locked]"; continue; fi
    merge_output=$(git -C "$wt" merge-base --is-ancestor "$head" refs/remotes/origin/main 2>/dev/null); merge_rc=$?
    [ "$merge_rc" -eq 0 ] && [ -z "$merge_output" ] || { say "KEEP   $wt [$branch: merge-base probe failed or malformed]"; continue; }
    kb=$(dir_kb "$wt")
    say "$(if [ "$DRY" = 1 ]; then printf WOULD; else printf REMOVE; fi) $wt [$branch, clean, merged, $((kb / 1024))MB]"
    if run git -C "$common" worktree remove "$wt"; then
      REMOVED=$((REMOVED + 1))
      if [ "$branch" != "(detached)" ] && ! run git -C "$common" branch -d "$branch"; then
        STRANDED=$((STRANDED + 1))
        say "KEEP   branch $branch [branch -d refused]"
      fi
    fi
  done < <(python3 - "$common" <<'PY'
import subprocess
import sys

lines = subprocess.check_output(
    ["git", "-C", sys.argv[1], "worktree", "list", "--porcelain"],
    text=True,
).splitlines()
path = None
locked = False

def flush():
    if path is not None:
        print(path + "\t" + ("locked" if locked else ""))

for line in lines + [""]:
    if line.startswith("worktree "):
        flush()
        path = line[9:]
        locked = False
    elif line == "locked":
        locked = True
    elif line == "":
        flush()
        path = None
        locked = False
PY
)
}

main() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --dry-run) DRY=1 ;;
      --no-tmp) : ;;
      --idle-hours)
        shift
        valid_hours "${1:-}" || { say "invalid --idle-hours: ${1:-}" >&2; return 2; }
        IDLE_HOURS=${1#"${1%%[!0]*}"}; IDLE_HOURS=${IDLE_HOURS:-0}
        ;;
      *) say "unknown argument: $1" >&2; return 2 ;;
    esac
    shift
done
  local ns root repo_id group_lock checkout_lock common current minutes
  ns=$(cargo_namespace_json "$REPO_DIR") || return 2
  root=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["root"])')
  repo_id=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["repo_id"])')
  group_lock=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["group_lock"])')
  checkout_lock=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["checkout_lock"])')
  common=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["common_dir"])')
  current=$(printf '%s\n' "$ns" | python3 -c 'import json,sys; print(json.load(sys.stdin)["top_level"])')
  minutes=$((IDLE_HOURS * 60))
  prune_namespace "$root/$repo_id" "$repo_id" "$group_lock" "$checkout_lock" "$minutes"
  worktree_sweep "$(dirname "$common")" "$current"
  if [ "$DRY" = 1 ]; then
    say "would remove $REMOVED worktree(s), would prune $PRUNED marker-owned target(s), reclaim ~$((FREED_KB / 1024 / 1024))GB (dry-run; nothing touched)"
  else
    say "removed $REMOVED worktree(s), pruned $PRUNED marker-owned target(s), reclaimed ~$((FREED_KB / 1024 / 1024))GB"
  fi
  [ "$STRANDED" -eq 0 ] || say "note: $STRANDED branch ref(s) left in place after branch -d refusal"
  say "legacy unmarked temporary targets are manual cleanup and were not scanned"
}

if [[ "${BASH_SOURCE[0]:-}" == "$0" ]] || [[ "${ZSH_EVAL_CONTEXT:-}" == toplevel ]]; then
  main "$@"
fi
