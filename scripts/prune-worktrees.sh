#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees.
#
# Rebuilds are cheap in this workspace (cold cargo check ~8s, cold test build
# ~18s as of 2026-08), so target/ dirs are disposable cache. Policy:
#   - worktree clean AND fully merged into origin/main  -> remove worktree (+ branch)
#   - anything else idle for more than IDLE_HOURS        -> delete its target/ only
#   - dirty or unmerged checkouts are never removed
#
# Usage: scripts/prune-worktrees.sh [--dry-run] [--idle-hours N]   (default 24)
set -uo pipefail

DRY=0
IDLE_HOURS=24
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --idle-hours) shift; IDLE_HOURS=${1:?--idle-hours needs a value} ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

MAIN=$(git rev-parse --path-format=absolute --git-common-dir)/..
MAIN=$(cd "$MAIN" && pwd)
NOW=$(date +%s)
freed_kb=0
removed=0
pruned=0

say() { echo "$@"; }
run() { if [ "$DRY" = 1 ]; then say "  DRY: $*"; else "$@"; fi; }

dir_kb() { du -sk "$1" 2>/dev/null | cut -f1; }

newest_mtime() { # worktree root + target/: whichever was touched last
  local m1 m2
  m1=$(stat -f %m "$1" 2>/dev/null || echo 0)
  m2=$(stat -f %m "$1/target" 2>/dev/null || echo 0)
  [ "$m1" -gt "$m2" ] && echo "$m1" || echo "$m2"
}

git -C "$MAIN" fetch origin main -q 2>/dev/null || true

while read -r wt; do
  [ "$wt" = "$MAIN" ] && continue
  [ -d "$wt" ] || continue

  branch=$(git -C "$wt" symbolic-ref --short -q HEAD || echo "(detached)")
  head=$(git -C "$wt" rev-parse HEAD 2>/dev/null) || continue
  dirty=$(git -C "$wt" status --porcelain 2>/dev/null | head -1)
  merged=""
  git -C "$wt" merge-base --is-ancestor "$head" origin/main 2>/dev/null && merged=merged

  if [ -z "$dirty" ] && [ -n "$merged" ]; then
    kb=$(dir_kb "$wt")
    say "REMOVE $wt  [$branch: clean, merged, $((kb / 1024))MB]"
    run git -C "$MAIN" worktree remove --force "$wt"
    if [ "$branch" != "(detached)" ]; then
      run git -C "$MAIN" branch -d "$branch"
    fi
    freed_kb=$((freed_kb + kb)); removed=$((removed + 1))
    continue
  fi

  if [ -d "$wt/target" ]; then
    age_h=$(( (NOW - $(newest_mtime "$wt")) / 3600 ))
    if [ "$age_h" -ge "$IDLE_HOURS" ]; then
      kb=$(dir_kb "$wt/target")
      say "PRUNE  $wt/target  [$branch: ${dirty:+dirty, }${merged:-unmerged}, idle ${age_h}h, $((kb / 1024))MB]"
      run rm -rf "$wt/target"
      freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
    else
      say "KEEP   $wt  [$branch: active ${age_h}h ago]"
    fi
  else
    say "KEEP   $wt  [$branch: ${dirty:+dirty, }${merged:-unmerged}, no target/]"
  fi
done < <(git -C "$MAIN" worktree list --porcelain | awk '/^worktree /{print substr($0,10)}')

run git -C "$MAIN" worktree prune

say "----"
suffix=""
[ "$DRY" = 1 ] && suffix=" (dry-run: nothing touched)"
say "removed $removed worktree(s), pruned $pruned target dir(s), reclaimed ~$((freed_kb / 1024 / 1024))GB$suffix"
