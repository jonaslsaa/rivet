#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees and tmp scratch.
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
# Agents also build into throwaway CARGO_TARGET_DIRs under /tmp (review checkouts,
# probe dirs, per-ticket target dirs), which no worktree sweep can see. Those
# accumulated to 39GB unnoticed in 2026-08, so the tmp sweep runs by default.
# Scratch is identified by cargo's CACHEDIR.TAG marker rather than by name —
# a name pattern silently missed store-probe/guard-probe/merge-probe. A tmp dir
# holding source but no build cache (a Cargo.toml with no target/) is left alone.
#
# Usage: scripts/prune-worktrees.sh [--dry-run] [--idle-hours N] [--no-tmp]  (default 24)
set -uo pipefail

DRY=0
IDLE_HOURS=24
SWEEP_TMP=1
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --idle-hours) shift; IDLE_HOURS=${1:?--idle-hours needs a value} ;;
    --no-tmp) SWEEP_TMP=0 ;;
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

is_cargo_cache() { [ -f "$1/CACHEDIR.TAG" ]; }

tmp_cache_dirs() { # the dir may be a bare CARGO_TARGET_DIR, or a checkout holding one
  local d=$1 c
  if is_cargo_cache "$d"; then echo "$d"; return 0; fi
  for c in "$d/target" "$d"/tools/*/target; do
    is_cargo_cache "$c" && echo "$c"
  done
  return 0
}

touched_within() { # cargo writes deep, so a root stat alone would call live builds idle
  [ -n "$(find "$1" -maxdepth 3 -mmin "-$2" -print -quit 2>/dev/null)" ]
}

sweep_tmp() {
  local root d caches cache kb mins=$((IDLE_HOURS * 60))
  for root in "$@"; do
    [ -d "$root" ] || continue
    for d in "$root"/*/; do
      d=${d%/}
      [ -O "$d" ] || continue
      caches=$(tmp_cache_dirs "$d")
      [ -n "$caches" ] || continue
      while read -r cache; do
        [ -n "$cache" ] || continue
        if touched_within "$cache" "$mins"; then
          say "KEEP   $cache  [tmp scratch: active within ${IDLE_HOURS}h]"
          continue
        fi
        kb=$(dir_kb "$cache")
        say "PRUNE  $cache  [tmp scratch: idle, $((kb / 1024))MB]"
        run rm -rf "$cache"
        freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
      done <<< "$caches"
    done
  done
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

if [ "$SWEEP_TMP" = 1 ]; then
  sweep_tmp /private/tmp "${TMPDIR:-}"
fi

say "----"
suffix=""
[ "$DRY" = 1 ] && suffix=" (dry-run: nothing touched)"
say "removed $removed worktree(s), pruned $pruned build cache(s), reclaimed ~$((freed_kb / 1024 / 1024))GB$suffix"
