#!/bin/bash
# prune-worktrees.sh — reclaim disk from accumulated git worktrees and tmp scratch.
#
# Rebuilds are cheap in this workspace (cold cargo check ~8s, cold test build
# ~18s as of 2026-08), so cargo target/ dirs are disposable cache. Policy:
#   - worktree clean AND fully merged into origin/main  -> remove worktree (+ branch);
#     "clean" requires the status probe to succeed — a corrupt/unreadable index is
#     never clean, and removal is a plain `git worktree remove` (no --force) so
#     git's dirty-refusal at removal time backstops the probe; a refused
#     `git branch -d` is reported with the ref left in place
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
#     own when it is clearly cargo scratch: cargo CACHEDIR.TAG + .fingerprint,
#     and no source/VCS evidence. The .rustc_info.json is not required there (a
#     partial cleanup can drop it), but the source/VCS refusal applies to every
#     tier — a nested target/ path can itself be a checkout root, and must never
#     be removed wholesale.
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
stranded=0

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

is_cargo_scratch() { # $1 = dir; clearly cargo build scratch (never a source/VCS root)
  has_cargo_tag "$1" || return 1
  has_cargo_fingerprint "$1" || return 1
  [ -e "$1/Cargo.toml" ] && return 1
  [ -e "$1/.git" ] && return 1
  return 0
}

is_cargo_target() { # $1 = dir; an unambiguous bare CARGO_TARGET_DIR (safe to rm -rf wholesale)
  is_cargo_scratch "$1" || return 1
  [ -f "$1/.rustc_info.json" ] || return 1
  return 0
}

nested_cargo_targets() { # nested target/ dirs inside a checkout; pruned on their own
  local d
  [ -d "$1/target" ] && is_cargo_scratch "$1/target" && echo "$1/target"
  while IFS= read -r d; do
    [ -d "$d" ] && is_cargo_scratch "$d" && echo "$d"
  done < <(find "$1/tools" -maxdepth 2 -type d -name target 2>/dev/null)
  return 0
}

cache_dirs() { # worktree build caches: nested target/ dirs, pruned on their own
  nested_cargo_targets "$1"
}

newest_mtime() { # $1 = path to stat; $2 = cache dirs (one per line), or empty
  local newest m d
  newest=$(stat -f %m "$1" 2>/dev/null || echo 0)
  while read -r d; do
    [ -n "$d" ] || continue
    m=$(stat -f %m "$d" 2>/dev/null || echo 0)
    [ "$m" -gt "$newest" ] && newest=$m
  done <<< "${2:-}"
  echo "$newest"
}

tmp_cache_dirs() { # a bare cargo target dir, or the cargo target dirs in a checkout
  if is_cargo_target "$1"; then echo "$1"; return 0; fi
  nested_cargo_targets "$1"
}

touched_within() { # cargo writes deep, so a root stat alone would call live builds idle
  # maxdepth 4: cargo rewrites existing fingerprint files (e.g. .fingerprint/
  # <hash>/lib-<crate>.json) in place at depth 4, which does not bump the
  # depth-3 hash dir's mtime — a shallower probe would miss an active build.
  [ -n "$(find "$1" -maxdepth 4 -mmin "-$2" -print -quit 2>/dev/null)" ]
}

canonical_dir() { # $1 = path; canonical absolute path if it is a real dir, else empty
  local r=${1:-}
  [ -n "$r" ] && [ -d "$r" ] || return 0
  cd "$r" 2>/dev/null && pwd -P
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
        if run rm -rf "$cache"; then
          freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
        fi
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

  local MAIN NOW mins
  MAIN=$(git rev-parse --path-format=absolute --git-common-dir)/..
  MAIN=$(cd "$MAIN" && pwd)
  NOW=$(date +%s)
  mins=$((IDLE_HOURS * 60))
  freed_kb=0; removed=0; pruned=0

  # Merged-ness is judged against origin/main. A dry run must not touch the
  # network or move refs (its summary claims "nothing touched"), so fetch only
  # when actually pruning; a preview then classifies against the existing ref.
  if [ "$DRY" != 1 ]; then
    git -C "$MAIN" fetch origin main -q 2>/dev/null || true
  fi

  while IFS=$'\t' read -r wt lock; do
    [ "$wt" = "$MAIN" ] && continue
    [ -d "$wt" ] || continue

    branch=$(git -C "$wt" symbolic-ref --short -q HEAD || echo "(detached)")
    head=$(git -C "$wt" rev-parse HEAD 2>/dev/null) || continue
    # A failing status probe must never read as clean: `git status --porcelain`
    # exits nonzero on a corrupt linked-worktree index while printing nothing,
    # so without this guard the sweep would treat such a worktree as clean and
    # remove it wholesale — taking any real uncommitted file with it.
    status_fail=""
    dirty=$(git -C "$wt" status --porcelain 2>/dev/null) || status_fail="status probe failed"
    merged=""
    git -C "$wt" merge-base --is-ancestor "$head" origin/main 2>/dev/null && merged=merged
    state="${dirty:+dirty, }${merged:-unmerged}"
    [ -n "$status_fail" ] && state="status probe failed"
    # A locked worktree is not removable: `git worktree remove` refuses a lock
    # (only remove -f -f overrides), so the sweep must report it as kept
    # rather than count a removal a real run cannot do. The lock reason comes
    # from the porcelain "locked" field (prefix * matches `git worktree list`).
    [ -n "$lock" ] && state="${state:+$state, }locked"

    if [ -z "$status_fail" ] && [ -z "$dirty" ] && [ -n "$merged" ] && [ -z "$lock" ]; then
      kb=$(dir_kb "$wt")
      say "$(act REMOVE) $wt  [$branch: clean, merged, $((kb / 1024))MB]"
      # No --force: git's dirty-worktree refusal at removal time is the backstop
      # for a file that lands between the status probe above and this remove.
      # A plain remove succeeds for every clean+merged+unlocked worktree here.
      if run git -C "$MAIN" worktree remove "$wt"; then
        freed_kb=$((freed_kb + kb)); removed=$((removed + 1))
        if [ "$branch" != "(detached)" ] && ! run git -C "$MAIN" branch -d "$branch"; then
          say "  WARN: branch '$branch' survived worktree removal (branch -d refused; ref left in place, never force-deleted)"
          stranded=$((stranded + 1))
        fi
      fi
      continue
    fi

    caches=$(cache_dirs "$wt")
    if [ -n "$caches" ]; then
      age_h=$(( (NOW - $(newest_mtime "$wt" "$caches")) / 3600 ))
      if [ "$age_h" -ge "$IDLE_HOURS" ]; then
        while read -r cache; do
          [ -n "$cache" ] || continue
          if touched_within "$cache" "$mins"; then
            say "KEEP   $cache  [$branch: $state, active within ${IDLE_HOURS}h]"
            continue
          fi
          kb=$(dir_kb "$cache")
          cache_age_h=$(( (NOW - $(stat -f %m "$cache" 2>/dev/null || echo 0)) / 3600 ))
          say "$(act PRUNE)  $cache  [$branch: $state, idle ${cache_age_h}h, $((kb / 1024))MB]"
          if run rm -rf "$cache"; then
            freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
          fi
        done <<< "$caches"
      else
        say "KEEP   $wt  [$branch: active ${age_h}h ago]"
      fi
    else
      say "KEEP   $wt  [$branch: $state, no build caches]"
    fi
  done < <(git -C "$MAIN" worktree list --porcelain | awk '
    /^worktree /{if (p != "") print p; p = substr($0, 10); r = ""; next}
    /^locked/{if (p != "") {r = substr($0, 7); print p "\t*" r; p = ""; next}}
    {next}
    END{if (p != "") print p}')

  run git -C "$MAIN" worktree prune

  if [ "$SWEEP_TMP" = 1 ]; then
    # /tmp is a symlink to /private/tmp on macOS, so $TMPDIR often names the
    # same tree; sweep each canonical root once.
    local t1 t2
    t1=$(canonical_dir /private/tmp)
    t2=$(canonical_dir "${TMPDIR:-}")
    if [ -n "$t2" ] && [ "$t2" = "$t1" ]; then
      sweep_tmp "$t1"
    else
      sweep_tmp "$t1" "$t2"
    fi
  fi

  say "----"
  if [ "$DRY" = 1 ]; then
    say "would remove $removed worktree(s), would prune $pruned build cache(s), would reclaim ~$((freed_kb / 1024 / 1024))GB (dry-run; nothing touched)"
  else
    say "removed $removed worktree(s), pruned $pruned build cache(s), reclaimed ~$((freed_kb / 1024 / 1024))GB"
  fi
  if [ "$stranded" -gt 0 ]; then
    say "note: $stranded branch ref(s) left in place after worktree removal (branch -d refused; refs are never force-deleted)"
  fi
}

# Run only when executed directly; sourcing this file just defines the
# functions, so tests can drive the classification in isolation. The zsh
# branch covers `zsh scripts/prune-worktrees.sh` (ZSH_EVAL_CONTEXT is "toplevel"
# only for a direct exec, never for `source`), where BASH_SOURCE is empty.
if [[ "${BASH_SOURCE[0]:-}" == "$0" ]] || [[ "${ZSH_EVAL_CONTEXT:-}" == toplevel ]]; then
  main "$@"
fi
