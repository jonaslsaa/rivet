#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees and tmp scratch.
#
# Rebuilds are cheap in this workspace (cold cargo check ~8s, cold test build
# ~18s as of 2026-08), so cargo target/ dirs are disposable cache. Policy:
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
#
# Cargo identifies its own build scratch. Every CARGO_TARGET_DIR carries a
# CACHEDIR.TAG that cargo itself wrote (a distinctive "created by cargo" line,
# vs the generic marker any cache tool drops), a .rustc_info.json host-info
# file, and .fingerprint dirs under each profile (debug, release, custom,
# cross-compiled, doc). The generic CACHEDIR.TAG marker alone is not enough —
# the old tag-only check would rm -rf both a source checkout and an unrelated
# cache on the strength of a marker those dirs can also carry.
#
# Two trust tiers, because deleting a whole /tmp child is riskier than pruning
# a nested target/ that is provably inside a known checkout:
#   - a directory is removed WHOLESALE (a bare tmp CARGO_TARGET_DIR) only when
#     it is unambiguous cargo scratch: cargo CACHEDIR.TAG + .rustc_info.json +
#     .fingerprint, and no source/VCS evidence (Cargo.toml/.git).
#   - a nested target/ dir (inside a worktree or tmp checkout) is pruned on its
#     own when it is clearly cargo scratch (cargo CACHEDIR.TAG + .fingerprint);
#     the checkout itself is never removed on that path, so the .rustc_info.json
#     and source/VCS guards are not required there.
# Ambiguous tagged directories are left alone.
#
# Usage: scripts/prune-worktrees.sh [--dry-run] [--idle-hours N] [--no-tmp]  (default 24)
set -uo pipefail

DRY=0
IDLE_HOURS=24
SWEEP_TMP=1
freed_kb=0
removed=0
pruned=0

say() { echo "$@"; }
run() { if [ "$DRY" = 1 ]; then say "  DRY: $*"; else "$@"; fi; }

act() { # mutation verb for this mode; a dry run must not claim it happened
  if [ "$DRY" = 1 ]; then printf 'WOULD %s' "$1"; else printf '%s' "$1"; fi
}

dir_kb() { local kb; kb=$(du -sk "$1" 2>/dev/null | cut -f1); echo "${kb:-0}"; }

# Classification is find/grep based (quoted paths, no shell globs), so sourcing
# this file under zsh — or any shell with failglob semantics — cannot abort on
# an unmatched pattern.

has_cargo_tag() { # $1 = dir; carries the CACHEDIR.TAG that cargo itself wrote
  [ -f "$1/CACHEDIR.TAG" ] || return 1
  grep -q "cache directory tag created by cargo" "$1/CACHEDIR.TAG"
}

has_cargo_fingerprint() { # $1 = dir; cargo writes .fingerprint dirs under each profile
  [ -n "$(find "$1" -maxdepth 4 -type d -name .fingerprint -print -quit 2>/dev/null)" ]
}

is_cargo_scratch() { # $1 = dir; clearly cargo build scratch (nested-target tier)
  has_cargo_tag "$1" || return 1
  has_cargo_fingerprint "$1" || return 1
  return 0
}

is_cargo_target() { # $1 = dir; an unambiguous bare CARGO_TARGET_DIR (safe to rm -rf wholesale)
  is_cargo_scratch "$1" || return 1
  [ -f "$1/.rustc_info.json" ] || return 1
  [ -e "$1/Cargo.toml" ] && return 1
  [ -e "$1/.git" ] && return 1
  return 0
}

cache_dirs() { # worktree build caches: nested target/ dirs, pruned on their own
  local d
  [ -d "$1/target" ] && is_cargo_scratch "$1/target" && echo "$1/target"
  while IFS= read -r d; do
    [ -d "$d" ] && is_cargo_scratch "$d" && echo "$d"
  done < <(find "$1/tools" -maxdepth 2 -type d -name target 2>/dev/null)
  return 0
}

newest_mtime() { # $1 = path to stat; reads cache dirs (one per line) from stdin
  local newest m d
  newest=$(stat -f %m "$1" 2>/dev/null || echo 0)
  while read -r d; do
    [ -n "$d" ] || continue
    m=$(stat -f %m "$d" 2>/dev/null || echo 0)
    [ "$m" -gt "$newest" ] && newest=$m
  done
  echo "$newest"
}

tmp_cache_dirs() { # a bare cargo target dir, or the cargo target dirs in a checkout
  local d=$1 c
  if is_cargo_target "$d"; then echo "$d"; return 0; fi
  [ -d "$d/target" ] && is_cargo_scratch "$d/target" && echo "$d/target"
  while IFS= read -r c; do
    [ -d "$c" ] && is_cargo_scratch "$c" && echo "$c"
  done < <(find "$d/tools" -maxdepth 2 -type d -name target 2>/dev/null)
  return 0
}

touched_within() { # cargo writes deep, so a root stat alone would call live builds idle
  [ -n "$(find "$1" -maxdepth 3 -mmin "-$2" -print -quit 2>/dev/null)" ]
}

sweep_tmp() {
  local root d caches cache kb mins=$((IDLE_HOURS * 60))
  for root in "$@"; do
    [ -d "$root" ] || continue
    while IFS= read -r d; do
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
        say "$(act PRUNE)  $cache  [tmp scratch: idle, $((kb / 1024))MB]"
        run rm -rf "$cache"
        freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
      done <<< "$caches"
    done < <(find "$root" -maxdepth 1 -mindepth 1 -type d -not -name '.*' 2>/dev/null)
  done
}

main() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run) DRY=1 ;;
      --idle-hours) shift; IDLE_HOURS=${1:?--idle-hours needs a value} ;;
      --no-tmp) SWEEP_TMP=0 ;;
      *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
  done

  local MAIN NOW
  MAIN=$(git rev-parse --path-format=absolute --git-common-dir)/..
  MAIN=$(cd "$MAIN" && pwd)
  NOW=$(date +%s)
  freed_kb=0; removed=0; pruned=0

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
      say "$(act REMOVE) $wt  [$branch: clean, merged, $((kb / 1024))MB]"
      run git -C "$MAIN" worktree remove --force "$wt"
      if [ "$branch" != "(detached)" ]; then
        run git -C "$MAIN" branch -d "$branch"
      fi
      freed_kb=$((freed_kb + kb)); removed=$((removed + 1))
      continue
    fi

    caches=$(cache_dirs "$wt")
    if [ -n "$caches" ]; then
      age_h=$(( (NOW - $(newest_mtime "$wt" <<< "$caches")) / 3600 ))
      if [ "$age_h" -ge "$IDLE_HOURS" ]; then
        while read -r cache; do
          [ -n "$cache" ] || continue
          kb=$(dir_kb "$cache")
          say "$(act PRUNE)  $cache  [$branch: ${dirty:+dirty, }${merged:-unmerged}, idle ${age_h}h, $((kb / 1024))MB]"
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
  if [ "$DRY" = 1 ]; then
    say "would remove $removed worktree(s), would prune $pruned build cache(s), would reclaim ~$((freed_kb / 1024 / 1024))GB (dry-run; nothing touched)"
  else
    say "removed $removed worktree(s), pruned $pruned build cache(s), reclaimed ~$((freed_kb / 1024 / 1024))GB"
  fi
}

# Run only when executed directly; sourcing this file just defines the
# functions, so tests can drive the classification in isolation.
if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
