#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees and tmp scratch.
#
# Rebuilds are cheap in this workspace (cold cargo check ~8s, cold test build
# ~18s as of 2026-08), so target/ dirs are disposable cache. Policy:
#   - worktree clean AND fully merged into origin/main  -> remove worktree (+ branch)
#   - anything else idle for more than IDLE_HOURS        -> delete its build caches
#   - dirty or unmerged checkouts are never removed
#   - the main checkout is never removed; legacy checkout-local caches are
#     disposable, while the canonical shared target-agent-shared cache is kept
#
# tools/*/work is NOT a build cache and is never touched: it holds downloaded
# jars and scenario capture output, which cost a network fetch or a full
# client/server run to reproduce.
#
# Claude sessions and nested worktrees now share target-agent-shared through
# project settings and .cargo/config.toml. The tmp sweep reclaims legacy or
# explicitly overridden CARGO_TARGET_DIRs under /tmp, which accumulated to 39GB
# unnoticed in 2026-08. Scratch is identified by cargo's CACHEDIR.TAG marker
# rather than by name — a name pattern silently missed store-probe/guard-probe/
# merge-probe. A tmp dir holding source but no build cache (a Cargo.toml with no
# target/) is left alone.
#
# An idle threshold alone cannot catch active churn: a fleet of parallel agents
# keeps every cache "recently touched", so a 24h rule pruned 0GB while the disk
# filled five times in 2026-08. Below PRESSURE_GB free the threshold drops to
# PRESSURE_IDLE_MIN minutes, and only caches a live rustc/cargo is writing to
# are spared. Rebuilds being cheap is what makes that trade sound.
#
# Usage: scripts/prune-worktrees.sh [--dry-run] [--idle-hours N] [--no-tmp]
#                                   [--pressure-gb N] [--pressure-idle-min N]
set -uo pipefail

DRY=0
IDLE_HOURS=24
SWEEP_TMP=1
PRESSURE_GB=40
PRESSURE_IDLE_MIN=20
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --idle-hours) shift; IDLE_HOURS=${1:?--idle-hours needs a value} ;;
    --no-tmp) SWEEP_TMP=0 ;;
    --pressure-gb) shift; PRESSURE_GB=${1:?--pressure-gb needs a value} ;;
    --pressure-idle-min) shift; PRESSURE_IDLE_MIN=${1:?--pressure-idle-min needs a value} ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

MAIN=$(git rev-parse --path-format=absolute --git-common-dir)/..
MAIN=$(cd "$MAIN" && pwd)
SHARED_TARGET="$MAIN/target-agent-shared"
freed_kb=0
removed=0
pruned=0

say() { echo "$@"; }
run() { if [ "$DRY" = 1 ]; then say "  DRY: $*"; else "$@"; fi; }

dir_kb() { local kb; kb=$(du -sk "$1" 2>/dev/null | cut -f1); echo "${kb:-0}"; }

is_cargo_cache() { [ -f "$1/CACHEDIR.TAG" ]; }

cache_dirs() { # tools/* are excluded from the workspace, so each has its own target/
  local d
  for d in "$1/target" "$1"/tools/*/target; do
    is_cargo_cache "$d" && echo "$d"
  done
  return 0
}

has_preserved_work() {
  local d
  for d in "$1"/tools/*/work; do
    [ -d "$d" ] || continue
    [ -n "$(find "$d" -mindepth 1 -print -quit 2>/dev/null)" ] && return 0
  done
  return 1
}

# Single funnel for every deletion. Re-checking the marker here means a path can
# only be removed while it still looks like a cargo cache, however it was derived.
prune_cache() {
  if [ "$1" = "$SHARED_TARGET" ]; then
    say "REFUSE $1  [shared cargo target]"
    return 1
  fi
  if ! is_cargo_cache "$1"; then
    say "REFUSE $1  [not a cargo cache]"
    return 1
  fi
  run rm -rf "$1"
}

touched_within() { # cargo writes deep, so a root stat alone would call live builds idle
  [ -n "$(find "$1" -maxdepth 3 -mmin "-$2" -print -quit 2>/dev/null)" ]
}

free_gb() { df -g /System/Volumes/Data 2>/dev/null | awk 'NR==2{print $4}'; }

# rustc receives its target dir as --out-dir, so a live build names the cache it
# writes. This is what makes pressure mode safe to run mid-build. Matching needs
# the full argument list, which pgrep cannot print on macOS.
# shellcheck disable=SC2009
BUILD_ARGS=$(ps -eo args 2>/dev/null | grep -E 'rustc|cargo' | grep -v grep || true)
is_live() { case "$BUILD_ARGS" in *"$1"*) return 0 ;; esac; return 1; }

IDLE_MIN=$((IDLE_HOURS * 60))
FREE_GB=$(free_gb)
if [ -n "$FREE_GB" ] && [ "$FREE_GB" -lt "$PRESSURE_GB" ]; then
  IDLE_MIN=$PRESSURE_IDLE_MIN
  say "PRESSURE ${FREE_GB}GB free (< ${PRESSURE_GB}GB): pruning caches idle >${IDLE_MIN}min, sparing live builds"
fi

sweep_tmp() {
  local root tag cache kb
  for root in "$@"; do
    [ -d "$root" ] || continue
    while read -r tag; do
      [ -n "$tag" ] || continue
      cache=${tag%/CACHEDIR.TAG}
      if is_live "$cache"; then
        say "KEEP   $cache  [tmp scratch: live build]"
        continue
      fi
      if touched_within "$cache" "$IDLE_MIN"; then
        say "KEEP   $cache  [tmp scratch: active within ${IDLE_MIN}min]"
        continue
      fi
      kb=$(dir_kb "$cache")
      say "PRUNE  $cache  [tmp scratch: idle, $((kb / 1024))MB]"
      prune_cache "$cache" || continue
      freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
    done < <(find "$root" -maxdepth 8 -type f -name CACHEDIR.TAG -user "$(id -un)" -print 2>/dev/null)
  done
}

git -C "$MAIN" fetch origin main -q 2>/dev/null || true

while read -r wt; do
  [ -d "$wt" ] || continue
  is_main=0; [ "$wt" = "$MAIN" ] && is_main=1

  branch=$(git -C "$wt" symbolic-ref --short -q HEAD || echo "(detached)")
  head=$(git -C "$wt" rev-parse HEAD 2>/dev/null) || continue
  dirty=$(git -C "$wt" status --porcelain 2>/dev/null | head -1)
  merged=""
  git -C "$wt" merge-base --is-ancestor "$head" origin/main 2>/dev/null && merged=merged

  if [ "$is_main" = 0 ] && [ -z "$dirty" ] && [ -n "$merged" ]; then
    if has_preserved_work "$wt"; then
      say "KEEP   $wt  [$branch: clean and merged, preserved tools/*/work]"
    else
      kb=$(dir_kb "$wt")
      say "REMOVE $wt  [$branch: clean, merged, $((kb / 1024))MB]"
      run git -C "$MAIN" worktree remove --force "$wt"
      if [ "$branch" != "(detached)" ]; then
        run git -C "$MAIN" branch -d "$branch"
      fi
      freed_kb=$((freed_kb + kb)); removed=$((removed + 1))
      continue
    fi
  fi

  caches=$(cache_dirs "$wt")
  if [ -n "$caches" ]; then
    while read -r cache; do
      [ -n "$cache" ] || continue
      if is_live "$cache"; then
        say "KEEP   $cache  [$branch: live build]"
        continue
      fi
      if touched_within "$cache" "$IDLE_MIN"; then
        say "KEEP   $cache  [$branch: active within ${IDLE_MIN}min]"
        continue
      fi
      kb=$(dir_kb "$cache")
      say "PRUNE  $cache  [$branch: ${dirty:+dirty, }${merged:-unmerged}, idle, $((kb / 1024))MB]"
      prune_cache "$cache" || continue
      freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
    done <<< "$caches"
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
