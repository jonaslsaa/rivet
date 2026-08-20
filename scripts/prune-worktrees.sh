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
#   - a clean merged checkout with preserved tools/*/work is never removed
#
# tools/*/work is NOT a build cache and is never touched: it holds downloaded
# jars and scenario capture output, which cost a network fetch or a full
# client/server run to reproduce.
#
# Claude sessions share the canonical target-agent-shared cache. The tmp sweep
# reclaims legacy or explicitly overridden CARGO_TARGET_DIRs under /tmp, which
# accumulated to 39GB unnoticed in 2026-08. Cargo's CACHEDIR.TAG plus its
# fingerprint marker identifies build scratch; generic tagged directories and
# source checkouts remain untouched.
#
# Under disk pressure the idle threshold drops to PRESSURE_IDLE_MIN minutes.
# Caches named by a live cargo/rustc command are spared even when old.
# Rebuilds are cheap, so stale cache removal is preferable to a full disk.
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
# Usage: scripts/prune-worktrees.sh [--dry-run] [--idle-hours N] [--no-tmp]
#                                   [--pressure-gb N] [--pressure-idle-min N]
# (default idle threshold: 24 hours)
set -uo pipefail

DRY=0
IDLE_HOURS=24
SWEEP_TMP=1
PRESSURE_GB=40
PRESSURE_IDLE_MIN=20
IDLE_MIN=1440
SHARED_TARGET=""
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

file_mtime() { # $1 = path; mtime in epoch seconds, or 0 if it cannot be read
  # BSD stat (macOS) and GNU/uutils stat (Linux) spell this differently and each
  # rejects the other's flag. Probing one spelling only looks like a missing file:
  # the mtime reads 0, every cache dates to the epoch, and live build caches get
  # pruned as "idle". Try both.
  local m
  if m=$(stat -c %Y "$1" 2>/dev/null); then :
  elif m=$(stat -f %m "$1" 2>/dev/null); then :
  else m=0
  fi
  echo "${m:-0}"
}

newest_mtime() { # $1 = path to stat; $2 = cache dirs (one per line), or empty
  local newest m d
  newest=$(file_mtime "$1")
  while read -r d; do
    [ -n "$d" ] || continue
    m=$(file_mtime "$d")
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

canonical_path() {
  local candidate=$1 parent base
  if [ -d "$candidate" ]; then
    canonical_dir "$candidate"
    return
  fi
  parent=$(cd "$(dirname "$candidate")" 2>/dev/null && pwd -P) || return 0
  base=$(basename "$candidate")
  printf '%s/%s\n' "$parent" "$base"
}

has_preserved_work() {
  local work_dir
  [ -d "$1/tools" ] || return 1
  while IFS= read -r work_dir; do
    [ -n "$work_dir" ] || continue
    [ -d "$work_dir" ] || continue
    [ -n "$(find -H "$work_dir" -mindepth 1 -print -quit 2>/dev/null)" ] && return 0
  done < <(find "$1/tools" -mindepth 2 -maxdepth 2 \( -type d -o -type l \) -name work -print 2>/dev/null)
  return 1
}

require_nonnegative_integer() {
  local option=$1 value=${2-} max normalized too_large
  case "$value" in
    ''|*[!0-9]*)
      printf '%s requires a non-negative integer (got %s)\n' "$option" "${value:-<missing>}" >&2
      return 2
      ;;
  esac

  normalized=$value
  while [ "${#normalized}" -gt 1 ] && [ "${normalized#0}" != "$normalized" ]; do
    normalized=${normalized#0}
  done

  case "$option" in
    --idle-hours) max=153722867280912930 ;;
    *) max=9223372036854775807 ;;
  esac
  # shellcheck disable=SC2071  # equal-length decimal strings need lexical comparison
  if [ "${#normalized}" -gt "${#max}" ]; then
    too_large=1
  elif [ "${#normalized}" -eq "${#max}" ] && [[ "$normalized" > "$max" ]]; then
    too_large=1
  else
    too_large=0
  fi
  if [ "$too_large" -eq 1 ]; then
    printf '%s must be at most %s (got %s)\n' "$option" "$max" "$value" >&2
    return 2
  fi
  printf '%s\n' "$normalized"
}

prune_cache() {
  local cache=$1 wholesale=${2:-0} canonical
  canonical=$(canonical_dir "$cache")
  if [ -n "$SHARED_TARGET" ] && [ "$canonical" = "$SHARED_TARGET" ]; then
    say "REFUSE $cache  [shared cargo target]"
    return 1
  fi
  if [ "$wholesale" = 1 ]; then
    # A tmp child is removed wholesale only with Cargo's root identity marker.
    # Nested targets are passed with wholesale=0 and may survive partial cleanup.
    if ! is_cargo_target "$cache"; then
      say "REFUSE $cache  [not an unambiguous bare cargo target]"
      return 1
    fi
  elif ! is_cargo_scratch "$cache"; then
    say "REFUSE $cache  [not a cargo cache]"
    return 1
  fi
  run rm -rf "$cache"
}

free_gb() {
  local available_kb
  available_kb=$(df -Pk "$1" 2>/dev/null | awk 'NR == 2 { print $4 }')
  case "$available_kb" in
    ''|*[!0-9]*) return 0 ;;
  esac
  echo $((available_kb / 1024 / 1024))
}

is_live_build() {
  local cache=$1 build_processes
  # `ps ewwx` includes the process environment, where CARGO_TARGET_DIR lives
  # when Cargo receives it from the shell rather than as an argv argument.
  # shellcheck disable=SC2009  # ps is required here to inspect process environments
  build_processes=$(ps ewwx 2>/dev/null | grep -E '(^|[[:space:]/])(cargo|rustc)([[:space:]]|$)' || true)
  case "$build_processes" in
    *"$cache"*) return 0 ;;
  esac
  return 1
}

sweep_tmp() {
  local root child cache kb mins=${IDLE_MIN:-$((IDLE_HOURS * 60))}
  for root in "$@"; do
    [ -d "$root" ] || continue
    # Classify each direct tmp child first. A tagged container is never a
    # wholesale candidate: only a strict bare target or an exact nested target
    # returned by tmp_cache_dirs may reach the deletion funnel. This preserves
    # unrelated sentinels in ambiguous containers.
    while IFS= read -r child; do
      [ -n "$child" ] || continue
      while IFS= read -r cache; do
        [ -n "$cache" ] || continue
        [ -O "$cache" ] || continue
        if is_live_build "$cache"; then
          say "KEEP   $cache  [tmp scratch: live build]"
          continue
        fi
        if touched_within "$cache" "$mins"; then
          say "KEEP   $cache  [tmp scratch: active within ${mins}min]"
          continue
        fi
        kb=$(dir_kb "$cache")
        say "$(act PRUNE)  $cache  [tmp scratch: idle, $((kb / 1024))MB]"
        if is_cargo_target "$cache"; then
          wholesale=1
        else
          wholesale=0
        fi
        if prune_cache "$cache" "$wholesale"; then
          freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
        fi
      done < <(tmp_cache_dirs "$child")
    done < <(find "$root" -mindepth 1 -maxdepth 1 -type d -user "$(id -un)" -print 2>/dev/null)
  done
}

main() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run) DRY=1 ;;
      --idle-hours)
        [ "$#" -ge 2 ] || { printf '%s requires a non-negative integer (got <missing>)\n' "$1" >&2; return 2; }
        IDLE_HOURS=$(require_nonnegative_integer "$1" "$2") || return 2
        shift
        ;;
      --no-tmp) SWEEP_TMP=0 ;;
      --pressure-gb)
        [ "$#" -ge 2 ] || { printf '%s requires a non-negative integer (got <missing>)\n' "$1" >&2; return 2; }
        PRESSURE_GB=$(require_nonnegative_integer "$1" "$2") || return 2
        shift
        ;;
      --pressure-idle-min)
        [ "$#" -ge 2 ] || { printf '%s requires a non-negative integer (got <missing>)\n' "$1" >&2; return 2; }
        PRESSURE_IDLE_MIN=$(require_nonnegative_integer "$1" "$2") || return 2
        shift
        ;;
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

  SHARED_TARGET=$(canonical_path "$MAIN/target-agent-shared")
  IDLE_MIN=$((IDLE_HOURS * 60))
  free=$(free_gb "$MAIN")
  if [ -n "$free" ] && [ "$free" -lt "$PRESSURE_GB" ]; then
    IDLE_MIN=$PRESSURE_IDLE_MIN
    say "PRESSURE ${free}GB free (< ${PRESSURE_GB}GB): pruning caches idle >${IDLE_MIN}min, sparing live builds"
  fi

  while IFS=$'\t' read -r wt lock; do
    [ -d "$wt" ] || continue
    is_main=0
    [ "$wt" = "$MAIN" ] && is_main=1

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

    if [ "$is_main" = 0 ] && [ -z "$status_fail" ] && [ -z "$dirty" ] \
      && [ -n "$merged" ] && [ -z "$lock" ]; then
      if has_preserved_work "$wt"; then
        say "KEEP   $wt  [$branch: clean and merged, preserved tools/*/work]"
      else
        kb=$(dir_kb "$wt")
        say "$(act REMOVE) $wt  [$branch: clean, merged, $((kb / 1024))MB]"
        # No --force: git's dirty-worktree refusal at removal time is the backstop
        # for a file that lands between the status probe above and this remove.
        if run git -C "$MAIN" worktree remove "$wt"; then
          freed_kb=$((freed_kb + kb)); removed=$((removed + 1))
          if [ "$branch" != "(detached)" ] && ! run git -C "$MAIN" branch -d "$branch"; then
            say "  WARN: branch '$branch' survived worktree removal (branch -d refused; ref left in place, never force-deleted)"
            stranded=$((stranded + 1))
          fi
        fi
        continue
      fi
    fi

    caches=$(cache_dirs "$wt")
    if [ -n "$caches" ]; then
      while read -r cache; do
        [ -n "$cache" ] || continue
        if is_live_build "$cache"; then
          say "KEEP   $cache  [$branch: live build]"
          continue
        fi
        if touched_within "$cache" "$IDLE_MIN"; then
          say "KEEP   $cache  [$branch: $state, active within ${IDLE_MIN}min]"
          continue
        fi
        kb=$(dir_kb "$cache")
        cache_age_h=$(( (NOW - $(file_mtime "$cache")) / 3600 ))
        say "$(act PRUNE)  $cache  [$branch: $state, idle ${cache_age_h}h, $((kb / 1024))MB]"
        if prune_cache "$cache"; then
          freed_kb=$((freed_kb + kb)); pruned=$((pruned + 1))
        fi
      done <<< "$caches"
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
    # same tree; canonical_dir resolves both to one root, swept once.
    local t1 t2
    t1=$(canonical_dir /tmp)
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
