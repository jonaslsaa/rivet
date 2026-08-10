#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees.
#
# Rebuilds are cheap in this workspace (cold cargo check ~8s, cold test build
# ~18s as of 2026-08), so target/ dirs are disposable cache. Policy:
#   - worktree clean AND fully merged into origin/main  -> remove worktree (+ branch)
#   - anything else idle for more than IDLE_HOURS        -> delete its build caches
#   - dirty or unmerged checkouts are never removed
#
# tools/*/work is NOT a build cache and is never touched: it holds downloaded
# jars and scenario capture output, which cost a network fetch or a full
# client/server run to reproduce.
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

dir_kb() { local kb; kb=$(du -sk "$1" 2>/dev/null | cut -f1); echo "${kb:-0}"; }

cache_dirs() { # tools/* are excluded from the workspace, so each has its own target/
  local d
  [ -d "$1/target" ] && echo "$1/target"
  for d in "$1"/tools/*/target; do
    [ -d "$d" ] && echo "$d"
  done
  return 0
}

newest_mtime() { # worktree root + every build cache: whichever was touched last
  local newest m d
  newest=$(stat -f %m "$1" 2>/dev/null || echo 0)
  while read -r d; do
    [ -n "$d" ] || continue
    m=$(stat -f %m "$d" 2>/dev/null || echo 0)
    [ "$m" -gt "$newest" ] && newest=$m
  done < <(cache_dirs "$1")
  echo "$newest"
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

  caches=$(cache_dirs "$wt")
  if [ -n "$caches" ]; then
    age_h=$(( (NOW - $(newest_mtime "$wt")) / 3600 ))
    if [ "$age_h" -ge "$IDLE_HOURS" ]; then
      while read -r cache; do
        [ -n "$cache" ] || continue
        kb=$(dir_kb "$cache")
        say "PRUNE  $cache  [$branch: ${dirty:+dirty, }${merged:-unmerged}, idle ${age_h}h, $((kb / 1024))MB]"
        run rm -rf "$cache"
        freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
      done <<< "$caches"
    else
      say "KEEP   $wt  [$branch: active ${age_h}h ago]"
    fi
  else
    say "KEEP   $wt  [$branch: ${dirty:+dirty, }${merged:-unmerged}, no build caches]"
  fi
done < <(git -C "$MAIN" worktree list --porcelain | awk '/^worktree /{print substr($0,10)}')

run git -C "$MAIN" worktree prune

say "----"
suffix=""
[ "$DRY" = 1 ] && suffix=" (dry-run: nothing touched)"
say "removed $removed worktree(s), pruned $pruned build cache(s), reclaimed ~$((freed_kb / 1024 / 1024))GB$suffix"
