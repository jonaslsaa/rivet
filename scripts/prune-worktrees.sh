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
  [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
  python3 - "$marker" "$target" "$expected_repo" <<'PY'
import json, pathlib, sys
marker = pathlib.Path(sys.argv[1])
target = pathlib.Path(sys.argv[2])
expected_repo = sys.argv[3]
try:
    data = json.loads(marker.read_text())
except (OSError, ValueError):
    raise SystemExit(1)
if data.get("version") != 1 or data.get("repo_id") != expected_repo:
    raise SystemExit(1)
if data.get("target") != str(target):
    raise SystemExit(1)
if target.name != "iterative" or target.is_symlink():
    raise SystemExit(1)
PY
}

lock_free() {
  local group=$1 checkout=$2
  python3 - "$group" "$checkout" <<'PY'
import fcntl, os, sys
fds = []
try:
    for path in sys.argv[1:]:
        fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
        fds.append(fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
except (OSError, ValueError):
    raise SystemExit(1)
finally:
    for fd in fds:
        try: os.close(fd)
        except OSError: pass
PY
}

process_env_live() {
  local target=$1 line pid
  if [ -d /proc ]; then
    for env_file in /proc/[0-9]*/environ; do
      [ -r "$env_file" ] || continue
      pid=${env_file#/proc/}; pid=${pid%/environ}
      [ "$pid" = "$$" ] && continue
      if tr '\0' '\n' < "$env_file" 2>/dev/null | grep -Fxq "CARGO_TARGET_DIR=$target"; then return 0; fi
      if tr '\0' '\n' < "$env_file" 2>/dev/null | grep -Fxq "RIVET_CARGO_TARGET_DIR=$target"; then return 0; fi
    done
  fi
  while IFS= read -r line; do
    pid=${line%% *}
    [ -n "$pid" ] && [ "$pid" != "$$" ] || continue
    case "$line" in
      *"CARGO_TARGET_DIR=$target"*|*"RIVET_CARGO_TARGET_DIR=$target"*) return 0 ;;
    esac
  done < <(ps eww -ax -o pid=,command= 2>/dev/null || true)
  return 1
}

deep_active() {
  local target=$1 minutes=$2
  [ "$minutes" -lt 1 ] && minutes=1
  find -P "$target" -type f ! -name .rivet-cargo-target -mmin "-$minutes" -print -quit 2>/dev/null | grep -q .
}

dir_kb() { du -sk "$1" 2>/dev/null | { IFS=$'\t ' read -r kb _; printf '%s\n' "${kb:-0}"; }; }

prune_namespace() {
  local root=$1 repo_id=$2 group_lock=$3 checkout_lock=$4 minutes=$5
  [ -d "$root" ] || return 0
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
    kb=$(dir_kb "$target")
    say "$(if [ "$DRY" = 1 ]; then printf WOULD; else printf PRUNE; fi)  $target [$((kb / 1024))MB]"
    if run rm -rf -- "$target"; then
      PRUNED=$((PRUNED + 1))
      FREED_KB=$((FREED_KB + kb))
    fi
  done < <(find -P "$root" -type f -name .rivet-cargo-target -print 2>/dev/null)
}

worktree_sweep() {
  local common=$1 current=$2 wt branch head lock dirty status_rc kb
  while IFS=$'\t' read -r wt lock; do
    [ -n "$wt" ] && [ "$wt" != "$current" ] && [ -d "$wt" ] || continue
    branch=$(git -C "$wt" symbolic-ref --short -q HEAD 2>/dev/null || printf '(detached)')
    head=$(git -C "$wt" rev-parse HEAD 2>/dev/null) || { say "KEEP   $wt [git probe failed]"; continue; }
    dirty=$(git -C "$wt" status --porcelain 2>/dev/null); status_rc=$?
    [ "$status_rc" -eq 0 ] || { say "KEEP   $wt [$branch: status probe failed]"; continue; }
    if [ -n "$lock" ]; then say "KEEP   $wt [$branch: locked]"; continue; fi
    if [ -n "$dirty" ]; then say "KEEP   $wt [$branch: dirty]"; continue; fi
    git -C "$wt" merge-base --is-ancestor "$head" refs/remotes/origin/main >/dev/null 2>&1 || { say "KEEP   $wt [$branch: unmerged]"; continue; }
    kb=$(dir_kb "$wt")
    say "$(if [ "$DRY" = 1 ]; then printf WOULD; else printf REMOVE; fi) $wt [$branch: clean, merged, $((kb / 1024))MB]"
    if run git -C "$common" worktree remove "$wt"; then
      REMOVED=$((REMOVED + 1))
      if [ "$branch" != "(detached)" ] && ! run git -C "$common" branch -d "$branch"; then
        STRANDED=$((STRANDED + 1))
        say "KEEP   branch $branch [branch -d refused]"
      fi
    fi
  done < <(python3 - "$common" <<'PY'
import subprocess, sys
lines = subprocess.check_output(["git", "-C", sys.argv[1], "worktree", "list", "--porcelain"], text=True).splitlines()
path = None
locked = False
for line in lines + [""]:
    if line.startswith("worktree "):
        if path is not None:
            print(path + "\t" + ("locked" if locked else ""))
        path = line[9:]
        locked = False
    elif line == "locked":
        locked = True
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
