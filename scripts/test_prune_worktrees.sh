#!/bin/bash
# shellcheck disable=SC2034  # DRY/IDLE_HOURS/freed_kb/pruned are consumed by the sourced functions
# Focused tests for prune-worktrees.sh's cargo-scratch classification and sweeps.
#
# Regression: the tmp sweep originally treated any /tmp child whose root had
# CACHEDIR.TAG as disposable cargo scratch and rm -rf'd the whole directory.
# CACHEDIR.TAG is a generic cache marker — an unrelated cache tool or a source
# checkout can carry one at its root — so the sweep could destroy a checkout or
# a generic cache. These tests pin the tightened classifier: only an
# unambiguous bare cargo target dir (CACHEDIR.TAG + .rustc_info.json +
# .fingerprint, no Cargo.toml/.git) is removable wholesale; a checkout's nested
# target/ is pruned on its own via the lighter nested-target tier (CACHEDIR.TAG
# + .fingerprint); a tagged generic cache or a tagged source checkout is left
# untouched.
#
# The nested-target tier is deliberately lighter: it must recognize genuine
# cargo layouts that a debug/release-name check would miss (custom profiles
# build into target/dist/, cargo doc adds target/doc/, a partial cleanup can
# drop .rustc_info.json), while never deleting the enclosing checkout.
#
# Sources scripts/prune-worktrees.sh (its main body is guarded) and drives
# is_cargo_target / is_cargo_scratch / cache_dirs / newest_mtime /
# tmp_cache_dirs / sweep_tmp against a sandbox tree, under bash and zsh.
#
#   ./scripts/test_prune_worktrees.sh
set -euo pipefail

# zsh has no BASH_SOURCE and this harness is also run directly under zsh; fall back to $0.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

# shellcheck source=scripts/prune-worktrees.sh
# shellcheck disable=SC1091  # sources a sibling script; shellcheck only follows it with -x
source "$SCRIPT_DIR/prune-worktrees.sh"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# CLI thresholds are arithmetic/find inputs, so reject hostile values before
# repository discovery or any deletion path can run.
for option in --idle-hours --pressure-gb --pressure-idle-min; do
  for value in -1 1.5 invalid; do
    invalid_log="$SANDBOX/invalid-${option#--}-${value//[^[:alnum:]]/_}.log"
    rc=0
    bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp "$option" "$value" >"$invalid_log" 2>&1 || rc=$?
    [ "$rc" -eq 2 ] || fail "$option $value returned $rc instead of 2"
    grep -q "requires a non-negative integer" "$invalid_log" \
      || fail "$option $value did not report an actionable validation error"
  done
done
pass "negative and non-integer threshold values exit 2 before pruning"

for option in --idle-hours --pressure-gb --pressure-idle-min; do
  normalized_log="$SANDBOX/normalized-${option#--}.log"
  rc=0
  bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp "$option" 08 >"$normalized_log" 2>&1 || rc=$?
  [ "$rc" -eq 0 ] || fail "$option 08 returned $rc instead of accepting decimal leading zeros"
done
pass "threshold values with decimal leading zeros are accepted"

rc=0
bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp --idle-hours 9223372036854775807 \
  >"$SANDBOX/overflow-idle-hours.log" 2>&1 || rc=$?
[ "$rc" -eq 2 ] || fail "overflowing --idle-hours returned $rc instead of 2"
grep -q "must be at most" "$SANDBOX/overflow-idle-hours.log" \
  || fail "overflowing --idle-hours did not report its safe upper bound"
pass "idle-hours values that overflow minute arithmetic exit 2 before pruning"

# --- sandbox fixtures (all old, so sweeps treat them as idle) ---------------
CARGO_TAG='Signature: 8a477f597d28d172789f06886806bc55
# This file is a cache directory tag created by cargo.
# For information about cache directory tags see https://bford.info/cachedir/'
GENERIC_TAG='Signature: 8a477f597d28d172789f06886806bc55
# This file is a cache directory tag.
# For information about cache directory tags see https://bford.info/cachedir/'

oldtouch() { touch -m -t 202001010000 "$@"; }

mk_cargo_target() { # $1 dir; a recognizable cargo CARGO_TARGET_DIR (debug build)
  mkdir -p "$1/debug/.fingerprint"
  printf '%s\n' "$CARGO_TAG" > "$1/CACHEDIR.TAG"
  printf '{}\n' > "$1/.rustc_info.json"
  oldtouch "$1" "$1/CACHEDIR.TAG" "$1/.rustc_info.json" "$1/debug" "$1/debug/.fingerprint"
}
mk_custom_target() { # $1 dir; cargo target built with a custom profile (dist/)
  mkdir -p "$1/dist/.fingerprint"
  printf '%s\n' "$CARGO_TAG" > "$1/CACHEDIR.TAG"
  printf '{}\n' > "$1/.rustc_info.json"
  oldtouch "$1" "$1/CACHEDIR.TAG" "$1/.rustc_info.json" "$1/dist" "$1/dist/.fingerprint"
}
mk_tagged() { # $1 dir; generic cache: CACHEDIR.TAG only, no cargo artifacts
  mkdir -p "$1"
  printf '%s\n' "$GENERIC_TAG" > "$1/CACHEDIR.TAG"
  oldtouch "$1" "$1/CACHEDIR.TAG"
}

classify() { tmp_cache_dirs "$1"; }  # prints eligible cache dirs, one per line

# --- classification: strict bare-dir tier (is_cargo_target) -----------------
R="root-classify"
mkdir -p "$SANDBOX/$R"

mk_cargo_target "$SANDBOX/$R/bare-target"
if is_cargo_target "$SANDBOX/$R/bare-target"; then
  pass "bare cargo target dir is classified disposable"
else
  fail "bare cargo target dir was refused"
fi

mk_tagged "$SANDBOX/$R/generic-cache"
if is_cargo_target "$SANDBOX/$R/generic-cache"; then
  fail "generic CACHEDIR.TAG cache was classified disposable"
else
  pass "generic CACHEDIR.TAG cache is refused"
fi

mk_tagged "$SANDBOX/$R/tag-no-profile"
printf '{}\n' > "$SANDBOX/$R/tag-no-profile/.rustc_info.json"
oldtouch "$SANDBOX/$R/tag-no-profile/.rustc_info.json"
if is_cargo_target "$SANDBOX/$R/tag-no-profile"; then
  fail "tag + .rustc_info.json but no .fingerprint was classified disposable"
else
  pass "tag + .rustc_info.json but no .fingerprint is refused"
fi

# hostile: generic (non-cargo) CACHEDIR.TAG content plus cargo-shaped extras
# must still be refused — the tag's origin line is the cargo discriminator
foreign="$SANDBOX/$R/foreign-tag"
mkdir -p "$foreign/debug/.fingerprint"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag.\n' > "$foreign/CACHEDIR.TAG"
printf '{}\n' > "$foreign/.rustc_info.json"
oldtouch "$foreign" "$foreign/CACHEDIR.TAG" "$foreign/.rustc_info.json" "$foreign/debug" "$foreign/debug/.fingerprint"
if is_cargo_target "$foreign"; then
  fail "dir with generic tag content + cargo-shaped extras was classified disposable"
else
  pass "generic tag content is refused even with cargo-shaped extras"
fi

mk_cargo_target "$SANDBOX/$R/source-root"
touch "$SANDBOX/$R/source-root/Cargo.toml"
mkdir "$SANDBOX/$R/source-root/.git"
if is_cargo_target "$SANDBOX/$R/source-root"; then
  fail "dir with cargo markers PLUS Cargo.toml/.git was classified disposable"
else
  pass "cargo-marked dir that is also a source/VCS root is refused"
fi

# genuine custom-profile layout (target/dist/ only, no debug/release) is
# recognized wholesale when it is a bare target dir
mk_custom_target "$SANDBOX/$R/custom-bare"
if is_cargo_target "$SANDBOX/$R/custom-bare"; then
  pass "custom-profile-only bare target dir is classified disposable"
else
  fail "custom-profile-only bare target dir was refused"
fi

# --- classification: nested-target tier (is_cargo_scratch) -------------------
# a nested target/ that lost .rustc_info.json to a partial cleanup is still
# clearly cargo scratch and must be prunable on its own
SCRATCH="$SANDBOX/$R/nested-no-rustc"
mkdir -p "$SCRATCH/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$SCRATCH/CACHEDIR.TAG"
oldtouch "$SCRATCH" "$SCRATCH/CACHEDIR.TAG" "$SCRATCH/debug" "$SCRATCH/debug/.fingerprint"
if is_cargo_scratch "$SCRATCH"; then
  pass "nested cargo target without .rustc_info.json is prunable"
else
  fail "nested cargo target without .rustc_info.json was refused"
fi
if is_cargo_target "$SCRATCH"; then
  fail "bare-dir tier accepted a dir without .rustc_info.json"
else
  pass "bare-dir tier still refuses a dir without .rustc_info.json"
fi

# the nested tier still requires cargo's .fingerprint marker: a cargo-text tag
# with no .fingerprint anywhere is ambiguous and must be refused
SCRATCH2="$SANDBOX/$R/nested-tag-no-fingerprint"
mkdir -p "$SCRATCH2"
printf '%s\n' "$CARGO_TAG" > "$SCRATCH2/CACHEDIR.TAG"
oldtouch "$SCRATCH2" "$SCRATCH2/CACHEDIR.TAG"
if is_cargo_scratch "$SCRATCH2"; then
  fail "nested tier accepted cargo-tagged dir without .fingerprint"
else
  pass "nested tier refuses a cargo-tagged dir without .fingerprint"
fi

# a cargo-marked nested dir that is itself a source/VCS root must never be
# pruned wholesale, even though it sits at a target/ path
SCRATCH3="$SANDBOX/$R/nested-checkout-root"
mkdir -p "$SCRATCH3/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$SCRATCH3/CACHEDIR.TAG"
printf '[workspace]\n' > "$SCRATCH3/Cargo.toml"
mkdir "$SCRATCH3/.git"
oldtouch "$SCRATCH3" "$SCRATCH3/CACHEDIR.TAG" "$SCRATCH3/Cargo.toml" "$SCRATCH3/.git" "$SCRATCH3/debug" "$SCRATCH3/debug/.fingerprint"
if is_cargo_scratch "$SCRATCH3"; then
  fail "nested tier accepted a cargo-marked dir that is a checkout root"
else
  pass "nested tier refuses a cargo-marked dir that is a checkout root"
fi

# --- classification: tmp_cache_dirs (what the tmp sweep would consider) ------
R2="root-dirs"
mkdir -p "$SANDBOX/$R2"

mk_cargo_target "$SANDBOX/$R2/bare-target"
got=$(classify "$SANDBOX/$R2/bare-target")
[ "$got" = "$SANDBOX/$R2/bare-target" ] || fail "bare target not returned as its own cache"
pass "bare cargo target dir returned for wholesale prune"

# hostile: source checkout whose ROOT carries CACHEDIR.TAG must never be
# returned wholesale — only its nested target/ may be
CK="$SANDBOX/$R2/tagged-checkout"
mkdir -p "$CK/target"
printf '[workspace]\n' > "$CK/Cargo.toml"
mkdir "$CK/.git"
mk_tagged "$CK"
mk_cargo_target "$CK/target"
got=$(classify "$CK")
[ "$got" = "$CK/target" ] || fail "tagged source checkout returned unexpected caches: [$got]"
pass "tagged source checkout yields only its nested target/"

# hostile: the tmp child's nested target/ is ITSELF a git checkout root that
# carries a cargo CACHEDIR.TAG + .fingerprint (e.g. CARGO_TARGET_DIR pointed at
# a subdir, or a vendored target tree). It must never be returned for wholesale
# prune — only a genuine nested target/ that is not a source/VCS root may be.
CKN="$SANDBOX/$R2/nested-checkout-root"
mkdir -p "$CKN/target/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$CKN/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$CKN/target/Cargo.toml"
mkdir "$CKN/target/.git"
oldtouch "$CKN/target" "$CKN/target/CACHEDIR.TAG" "$CKN/target/Cargo.toml" "$CKN/target/.git" "$CKN/target/debug" "$CKN/target/debug/.fingerprint"
got=$(classify "$CKN")
[ -z "$got" ] || fail "nested checkout-root at a target/ path returned caches: [$got]"
pass "nested target-path checkout root is never returned for wholesale prune"

# hostile: unrelated generic cache must not be touched at all
mk_tagged "$SANDBOX/$R2/unrelated-cache"
touch "$SANDBOX/$R2/unrelated-cache/data.bin"
got=$(classify "$SANDBOX/$R2/unrelated-cache")
[ -z "$got" ] || fail "unrelated generic cache returned caches: [$got]"
pass "unrelated generic cache yields no caches"

# checkout without a cargo target yields nothing
CK2="$SANDBOX/$R2/plain-checkout"
mkdir -p "$CK2"
printf '[workspace]\n' > "$CK2/Cargo.toml"
mkdir "$CK2/.git"
got=$(classify "$CK2")
[ -z "$got" ] || fail "source checkout without build cache returned caches: [$got]"
pass "source checkout without build cache yields nothing"

# tools/*/target inside a tmp checkout is pruned too
CK3="$SANDBOX/$R2/checkout-with-tools"
mkdir -p "$CK3/tools/rivet-oracle/target"
mk_cargo_target "$CK3/tools/rivet-oracle/target"
got=$(classify "$CK3")
[ "$got" = "$CK3/tools/rivet-oracle/target" ] || fail "tools target not returned: [$got]"
pass "tools/*/target inside a tmp checkout is pruned"

# custom-profile-only nested target is pruned on its own even without
# .rustc_info.json
CK4="$SANDBOX/$R2/custom-nested"
mkdir -p "$CK4/target/dist/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$CK4/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$CK4/Cargo.toml"
mkdir "$CK4/.git"
oldtouch "$CK4/target" "$CK4/target/CACHEDIR.TAG" "$CK4/target/dist" "$CK4/target/dist/.fingerprint"
got=$(classify "$CK4")
[ "$got" = "$CK4/target" ] || fail "custom-profile nested target not returned: [$got]"
pass "custom-profile nested target (dist/ only, no .rustc_info.json) is pruned"

# --- cache_dirs / newest_mtime (worktree sweep path) -------------------------
R3="wt-caches"
mkdir -p "$SANDBOX/$R3"

# a worktree whose nested target lost .rustc_info.json is still pruned, not
# stranded: cache_dirs must return it under the nested-target tier
W1="$SANDBOX/$R3/wt-no-rustc"
mkdir -p "$W1/target/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$W1/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$W1/Cargo.toml"
mkdir "$W1/.git"
oldtouch "$W1/target" "$W1/target/CACHEDIR.TAG" "$W1/target/debug" "$W1/target/debug/.fingerprint"
got=$(cache_dirs "$W1")
[ "$got" = "$W1/target" ] || fail "cache_dirs stranded a nested target without .rustc_info.json: [$got]"
pass "cache_dirs prunes a nested target without .rustc_info.json"

# cache_dirs also returns tools/*/target dirs
W2="$SANDBOX/$R3/wt-with-tools"
mkdir -p "$W2/target/debug/.fingerprint" "$W2/tools/rivet-oracle/target/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$W2/target/CACHEDIR.TAG"
printf '%s\n' "$CARGO_TAG" > "$W2/tools/rivet-oracle/target/CACHEDIR.TAG"
printf '{}\n' > "$W2/target/.rustc_info.json"
printf '{}\n' > "$W2/tools/rivet-oracle/target/.rustc_info.json"
printf '[workspace]\n' > "$W2/Cargo.toml"
mkdir "$W2/.git"
oldtouch "$W2/target" "$W2/tools/rivet-oracle/target"
got=$(cache_dirs "$W2" | sort)
exp=$(printf '%s\n' "$W2/target" "$W2/tools/rivet-oracle/target" | sort)
[ "$got" = "$exp" ] || fail "cache_dirs missing tools target: [$got]"
pass "cache_dirs returns root target and tools/*/target"

# newest_mtime takes the cache list as an explicit argument and maxes the
# root + cache mtimes
W3="$SANDBOX/$R3/mtime"
mkdir -p "$W3/target/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$W3/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$W3/Cargo.toml"
mkdir "$W3/.git"
oldtouch "$W3"
oldtouch "$W3/target" "$W3/target/CACHEDIR.TAG" "$W3/target/debug" "$W3/target/debug/.fingerprint"
# the cache dir itself is the newest; the root stays older. mtimes are read
# back via stat (touch -t is local-time, so no fixed epoch constants).
touch -m -t 202002020000 "$W3/target"
want=$(file_mtime "$W3/target")
root_m=$(file_mtime "$W3")
[ "$root_m" -lt "$want" ] || fail "fixture: root mtime should be older than cache"
newest=$(newest_mtime "$W3" "$W3/target")
[ "$newest" -eq "$want" ] || fail "newest_mtime did not max cache mtimes: got $newest want $want"
pass "newest_mtime takes the cache list as an argument and maxes mtimes"

# newest_mtime with an empty cache list falls back to the root mtime
noinput=$(newest_mtime "$W3" "")
[ "$noinput" -eq "$root_m" ] || fail "newest_mtime with an empty list: got $noinput want $root_m"
pass "newest_mtime with an empty cache list returns the root mtime"

# a bare call (no $2 at all) must not abort under set -u: the documented
# contract is "$2 ... or empty", so a missing argument means no cache list
barecall=$(newest_mtime "$W3")
[ "$barecall" -eq "$root_m" ] || fail "newest_mtime without a cache list: got $barecall want $root_m"
pass "newest_mtime without a cache list returns the root mtime (no set -u abort)"

# regression: newest_mtime must never consume the caller's stdin. The old
# implementation read its list with `cat` whenever stdin was not a tty, which
# swallowed the remaining lines of a pipe-fed while loop (the worktree sweep's
# input) and silently dropped every worktree after the first.
seen=$(printf '%s\n' a b c | while read -r wt; do
  newest_mtime "$W3" "" >/dev/null  # list from the argument, never stdin
  printf '%s ' "$wt"
done)
[ "$seen" = "a b c " ] || fail "newest_mtime consumed the caller's stdin: saw [$seen]"
pass "newest_mtime with an explicit list leaves the caller's stdin intact"

# a worktree target/ that was actively built has deep cargo files that never
# bump the root or the target/ dir itself: the deep touched_within probe must
# keep it, even when newest_mtime says it is idle
W4="$SANDBOX/$R3/wt-deep-fresh"
mkdir -p "$W4/target/debug/.fingerprint/hash1"
printf '%s\n' "$CARGO_TAG" > "$W4/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$W4/Cargo.toml"
mkdir "$W4/.git"
printf '{}\n' > "$W4/target/debug/.fingerprint/hash1/lib-x.json"
oldtouch "$W4"
oldtouch "$W4/target" "$W4/target/CACHEDIR.TAG" "$W4/target/debug" "$W4/target/debug/.fingerprint" "$W4/target/debug/.fingerprint/hash1" "$W4/target/debug/.fingerprint/hash1/lib-x.json"
touched_within "$W4/target" 1440 && fail "fixture: deep files should be fresh" || true
# cargo rewrites an existing fingerprint file (lib-<crate>.json) IN PLACE at
# depth 4, which does not bump the depth-3 hash dir's mtime: only a maxdepth-4
# probe can see this as fresh.
touch -m "$W4/target/debug/.fingerprint/hash1/lib-x.json"
touched_within "$W4/target" 1440 || fail "deep-fresh target was not detected by touched_within"
pass "touched_within detects a fresh depth-4 fingerprint write in a worktree target/"

# A live Cargo process can receive CARGO_TARGET_DIR from its environment rather
# than argv. The process listing used by is_live_build must still spare its cache.
LIVE_CACHE="$SANDBOX/$R3/live-target"
mk_cargo_target "$LIVE_CACHE"
# shellcheck disable=SC2329  # is_live_build invokes this ps shim indirectly
ps() {
  printf '/usr/local/bin/cargo build --locked CARGO_TARGET_DIR=%s\n' "$LIVE_CACHE"
}
is_live_build "$LIVE_CACHE" || fail "live Cargo process with an environment target was not detected"
unset -f ps
pass "is_live_build detects CARGO_TARGET_DIR in a live Cargo process environment"

# --- sweep_tmp end to end ----------------------------------------------------
SWEEP_ROOT="$SANDBOX/root-sweep"
mkdir -p "$SWEEP_ROOT"
# positive: a bare cargo target dir is deleted
mk_cargo_target "$SWEEP_ROOT/bare"
# hostile: tagged generic cache survives
mk_tagged "$SWEEP_ROOT/cache"
touch "$SWEEP_ROOT/cache/data.bin"
# hostile: tagged source checkout survives wholesale; its target is deleted
CK5="$SWEEP_ROOT/checkout"
mkdir -p "$CK5/target"
printf '[workspace]\n' > "$CK5/Cargo.toml"
mkdir "$CK5/.git"
mk_tagged "$CK5"
mk_cargo_target "$CK5/target"
# hostile: the sweep root itself may carry Cargo markers (for example a
# shared TMPDIR cache marker). It is never a candidate, even with strict Cargo
# identity; only children are classified.
printf '%s\n' "$CARGO_TAG" > "$SWEEP_ROOT/CACHEDIR.TAG"
printf '{}\n' > "$SWEEP_ROOT/.rustc_info.json"
mkdir -p "$SWEEP_ROOT/debug/.fingerprint"
touch "$SWEEP_ROOT/sentinel.txt"
oldtouch "$SWEEP_ROOT" "$SWEEP_ROOT/CACHEDIR.TAG" "$SWEEP_ROOT/.rustc_info.json" "$SWEEP_ROOT/debug" "$SWEEP_ROOT/debug/.fingerprint" "$SWEEP_ROOT/sentinel.txt"
# custom-profile-only nested target inside a checkout is deleted too
CK6="$SWEEP_ROOT/custom-checkout"
mkdir -p "$CK6/target/dist/.fingerprint"
printf '[workspace]\n' > "$CK6/Cargo.toml"
mkdir "$CK6/.git"
printf '%s\n' "$CARGO_TAG" > "$CK6/target/CACHEDIR.TAG"
oldtouch "$CK6/target" "$CK6/target/CACHEDIR.TAG" "$CK6/target/dist" "$CK6/target/dist/.fingerprint"
# hostile: the nested target/ path is ITSELF a git checkout root carrying a
# cargo tag + .fingerprint — it must survive wholesale, never be pruned
CK7="$SWEEP_ROOT/checkout-as-target"
mkdir -p "$CK7/target/debug/.fingerprint"
printf '[workspace]\n' > "$CK7/target/Cargo.toml"
mkdir "$CK7/target/.git"
printf '%s\n' "$CARGO_TAG" > "$CK7/target/CACHEDIR.TAG"
oldtouch "$CK7" "$CK7/target" "$CK7/target/CACHEDIR.TAG" "$CK7/target/Cargo.toml" "$CK7/target/.git" "$CK7/target/debug" "$CK7/target/debug/.fingerprint"
# hostile: an ambiguous tagged container holds a genuine nested target and an
# unrelated sentinel. The exact target may be pruned, but the container itself
# must survive because it lacks the root .rustc_info.json identity marker.
AMB="$SWEEP_ROOT/ambiguous-container"
mkdir -p "$AMB/target"
printf '%s\n' "$CARGO_TAG" > "$AMB/CACHEDIR.TAG"
touch "$AMB/sentinel.txt"
mk_cargo_target "$AMB/target"
oldtouch "$AMB" "$AMB/CACHEDIR.TAG" "$AMB/sentinel.txt"

DRY=0; IDLE_HOURS=24; freed_kb=0; pruned=0
sweep_tmp "$SWEEP_ROOT" >/dev/null

[ -d "$SWEEP_ROOT/bare" ] && fail "bare cargo target dir not deleted" || true
[ -d "$SWEEP_ROOT/cache" ] || fail "generic cache was deleted"
[ -d "$SWEEP_ROOT/checkout" ] || fail "tagged source checkout was deleted"
[ -d "$CK5/target" ] && fail "checkout's nested target/ not deleted" || true
[ -f "$SWEEP_ROOT/cache/data.bin" ] || fail "generic cache contents were deleted"
[ -d "$CK6" ] || fail "custom-profile checkout was deleted wholesale"
[ -d "$CK6/target" ] && fail "custom-profile checkout's nested target/ not deleted" || true
[ -d "$CK7" ] || fail "checkout-at-target-path was deleted wholesale"
[ -d "$CK7/target" ] || fail "checkout-at-target-path's nested checkout was pruned wholesale"
[ -d "$AMB" ] || fail "ambiguous tagged container was deleted wholesale"
[ -f "$AMB/sentinel.txt" ] || fail "ambiguous container sentinel was deleted"
[ ! -d "$AMB/target" ] || fail "ambiguous container's exact nested target was not pruned"
[ -d "$SWEEP_ROOT" ] || fail "sweep root carrying Cargo markers was deleted"
[ -f "$SWEEP_ROOT/sentinel.txt" ] || fail "sweep root sentinel was deleted"
pass "real sweep: strict bare targets + exact nested targets pruned; containers and sentinels survive"

# --- dry-run reporting is prospective, not a claim of completed deletion -----
SWEEP_ROOT2="$SANDBOX/root-sweep2"
mkdir -p "$SWEEP_ROOT2"
mk_cargo_target "$SWEEP_ROOT2/bare"

DRY=1; IDLE_HOURS=24; freed_kb=0; pruned=0
out=$(sweep_tmp "$SWEEP_ROOT2")
echo "$out" | grep -q "WOULD PRUNE" || fail "dry-run line does not say WOULD PRUNE"
[ -d "$SWEEP_ROOT2/bare" ] || fail "dry-run deleted the bare target dir"
echo "$out" | grep -q "DRY: rm -rf" || fail "dry-run did not report the pending rm -rf"
pass "dry-run reports WOULD PRUNE and touches nothing"

# --- a failed rm is not counted as pruned/reclaimed ---------------------------
# run() must propagate the mutation's exit status: a rm that fails (here an
# immutable file blocks deletion) must not bump pruned/freed_kb, or the summary
# would over-claim reclaimed space.
if command -v chflags >/dev/null 2>&1; then
  SWEEP_ROOT3="$SANDBOX/root-sweep3"
  mkdir -p "$SWEEP_ROOT3"
  mk_cargo_target "$SWEEP_ROOT3/bare"
  chflags uchg "$SWEEP_ROOT3/bare/.rustc_info.json" 2>/dev/null && immutable=1 || immutable=0
  if [ "$immutable" = 1 ]; then
    DRY=0; IDLE_HOURS=24; freed_kb=0; pruned=0
    sweep_tmp "$SWEEP_ROOT3" >/dev/null 2>&1
    [ "$pruned" -eq 0 ] || fail "failed rm was still counted as pruned: $pruned"
    [ "$freed_kb" -eq 0 ] || fail "failed rm was still counted as reclaimed: $freed_kb"
    [ -e "$SWEEP_ROOT3/bare/.rustc_info.json" ] || fail "failed rm deleted the blocked file"
    chflags nouchg "$SWEEP_ROOT3/bare/.rustc_info.json" 2>/dev/null || true
    pass "failed rm is not counted as pruned/reclaimed"
  else
    pass "chflags unavailable; skipping failed-rm accounting test"
  fi
else
  pass "chflags unavailable; skipping failed-rm accounting test"
fi

# --- TMPDIR dedup: two names for one tree sweep once --------------------------
# /tmp is a symlink to /private/tmp on macOS, so $TMPDIR can name the same
# directory as the primary tmp root; canonical_dir must resolve both to one
# path so the sweep does not walk the tree twice.
R4="$SANDBOX/root-canon"
mkdir -p "$R4/real"
ln -s "$R4/real" "$R4/alias"
c_alias=$(canonical_dir "$R4/alias")
c_real=$(canonical_dir "$R4/real")
[ -n "$c_alias" ] && [ "$c_alias" = "$c_real" ] || fail "canonical_dir did not dedup a symlinked root: [$c_alias] vs [$c_real]"
c_missing=$(canonical_dir "$R4/nonexistent")
[ -z "$c_missing" ] || fail "canonical_dir returned a path for a missing dir: [$c_missing]"
pass "canonical_dir resolves a symlinked tmp root and ignores missing dirs"

# --- end to end: main() dry-run is prospective; real run removes a merged wt --
E2E="$SANDBOX/e2e"
mkdir -p "$E2E/main"
git init -q "$E2E/main"
git -C "$E2E/main" config user.email test@example.com
git -C "$E2E/main" config user.name "test"
git -C "$E2E/main" config commit.gpgsign false
printf 'x\n' > "$E2E/main/a.txt"
git -C "$E2E/main" add a.txt
git -C "$E2E/main" commit -qm c1
git -C "$E2E/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E/main" worktree add -q -b feature/merged "$E2E/wt" HEAD

cd "$E2E/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1)
echo "$out" | grep -q "WOULD REMOVE .*feature/merged" || fail "e2e dry-run missing WOULD REMOVE line: $out"
echo "$out" | grep -q "would reclaim .*dry-run; nothing touched" || fail "e2e dry-run summary not prospective: $out"
[ -d "$E2E/wt" ] || fail "e2e dry-run removed the merged worktree"
pass "e2e dry-run reports WOULD REMOVE and touches nothing"

out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "REMOVE .*feature/merged" || fail "e2e real run missing REMOVE line: $out"
[ -d "$E2E/wt" ] && fail "e2e real run did not remove the merged worktree" || true
git -C "$E2E/main" branch --list feature/merged | grep -q . && fail "e2e real run did not delete the merged branch" || true
pass "e2e real run removes the merged worktree and branch"

# --- worktree sweep prunes a dirty worktree's nested target, never the wt ----
E2E2="$SANDBOX/e2e2"
mkdir -p "$E2E2/main"
git init -q "$E2E2/main"
git -C "$E2E2/main" config user.email test@example.com
git -C "$E2E2/main" config user.name "test"
git -C "$E2E2/main" config commit.gpgsign false
printf 'x\n' > "$E2E2/main/a.txt"
git -C "$E2E2/main" add a.txt
git -C "$E2E2/main" commit -qm c1
git -C "$E2E2/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E2/main" worktree add -q -b feature/dirty "$E2E2/wt" HEAD
# dirty the worktree so it is never removed wholesale
printf 'y\n' >> "$E2E2/wt/a.txt"
# give it an idle nested target without .rustc_info.json (would have been
# stranded by the old strict-only classifier). Everything under the worktree
# root must be old — the root's own mtime is what newest_mtime starts from.
mkdir -p "$E2E2/wt/target/debug/.fingerprint"
printf '%s\n' "$CARGO_TAG" > "$E2E2/wt/target/CACHEDIR.TAG"
printf '[workspace]\n' > "$E2E2/wt/Cargo.toml"
oldtouch "$E2E2/wt"
oldtouch "$E2E2/wt/target" "$E2E2/wt/target/CACHEDIR.TAG" "$E2E2/wt/target/debug" "$E2E2/wt/target/debug/.fingerprint"

cd "$E2E2/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "PRUNE .*wt/target" || fail "wt sweep missing PRUNE line for nested target: $out"
[ -d "$E2E2/wt/target" ] && fail "wt sweep did not prune the dirty worktree's nested target" || true
[ -d "$E2E2/wt" ] || fail "wt sweep removed the dirty worktree"
pass "wt sweep prunes a dirty worktree's nested target and keeps the worktree"

# --- e2e: a corrupted linked-worktree index is never read as "clean" -----------
# A merged worktree whose index is corrupt makes `git status --porcelain` exit
# nonzero with empty output; the old `| head -1` swallowed that failure and the
# worktree — holding a real uncommitted file — was removed wholesale. The sweep
# must refuse instead: the file survives, the worktree stays, the branch stays.
E2E3="$SANDBOX/e2e3"
mkdir -p "$E2E3/main"
git init -q "$E2E3/main"
git -C "$E2E3/main" config user.email test@example.com
git -C "$E2E3/main" config user.name "test"
git -C "$E2E3/main" config commit.gpgsign false
printf 'x\n' > "$E2E3/main/a.txt"
git -C "$E2E3/main" add a.txt
git -C "$E2E3/main" commit -qm c1
git -C "$E2E3/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E3/main" worktree add -q -b feature/corrupt "$E2E3/wt" HEAD
printf 'uncommitted\n' > "$E2E3/wt/keep.txt"
idx=$(git -C "$E2E3/wt" rev-parse --git-path index)
printf 'not a git index\n' > "$idx"
cd "$E2E3/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "status probe failed" || fail "corrupt-index worktree not reported as probe failure: $out"
[ -f "$E2E3/wt/keep.txt" ] || fail "corrupt-index worktree removed; real uncommitted file lost"
[ -d "$E2E3/wt" ] || fail "corrupt-index worktree removed wholesale"
git -C "$E2E3/main" branch --list feature/corrupt | grep -q . || fail "corrupt-index branch was deleted"
pass "e2e corrupted linked-worktree index: uncommitted file survives, removal refused"

# --- e2e: a refused branch -d is visible, accounted, and never force-deleted ---
# The worktree is clean and merged into origin/main, so it is removed. But the
# branch tip is NOT merged into MAIN's HEAD and has no upstream, so `git branch
# -d` refuses: the ref must survive and the summary must say so honestly.
E2E4="$SANDBOX/e2e4"
mkdir -p "$E2E4/main"
git init -q "$E2E4/main"
git -C "$E2E4/main" config user.email test@example.com
git -C "$E2E4/main" config user.name "test"
git -C "$E2E4/main" config commit.gpgsign false
printf 'x\n' > "$E2E4/main/a.txt"
git -C "$E2E4/main" add a.txt
git -C "$E2E4/main" commit -qm c0
git -C "$E2E4/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E4/main" worktree add -q -b feature/stranded "$E2E4/wt" HEAD
# advance the worktree branch past main@c0 so it is merged into origin/main but
# not into MAIN's HEAD (no upstream => branch -d checks HEAD and refuses)
printf 'y\n' >> "$E2E4/wt/a.txt"
git -C "$E2E4/wt" add a.txt
git -C "$E2E4/wt" commit -qm c1
origin_head=$(git -C "$E2E4/wt" rev-parse HEAD)
git -C "$E2E4/main" update-ref refs/remotes/origin/main "$origin_head"
cd "$E2E4/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "REMOVE .*feature/stranded" || fail "stranded: missing REMOVE line: $out"
echo "$out" | grep -q "WARN: branch 'feature/stranded' survived" || fail "stranded: missing WARN line: $out"
echo "$out" | grep -q "note: 1 branch ref(s) left in place" || fail "stranded: missing stranded note: $out"
[ -d "$E2E4/wt" ] && fail "stranded: worktree was not removed" || true
git -C "$E2E4/main" branch --list feature/stranded | grep -q . || fail "stranded: branch was force-deleted"
pass "e2e refused branch -d is reported and the ref survives (never force-deleted)"

# --- e2e: preserved tools/*/work blocks clean merged removal --------------------
E2EP="$SANDBOX/e2e-preserved"
mkdir -p "$E2EP/main"
git init -q "$E2EP/main"
git -C "$E2EP/main" config user.email test@example.com
git -C "$E2EP/main" config user.name "test"
git -C "$E2EP/main" config commit.gpgsign false
printf 'tools/*/work/\n' > "$E2EP/main/.gitignore"
printf 'x\n' > "$E2EP/main/a.txt"
git -C "$E2EP/main" add .
git -C "$E2EP/main" commit -qm c1
git -C "$E2EP/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2EP/main" worktree add -q -b feature/preserved "$E2EP/wt" HEAD
mkdir -p "$E2EP/wt/tools/rivet-oracle/work"
printf 'capture\n' > "$E2EP/wt/tools/rivet-oracle/work/manifest.json"
cd "$E2EP/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1)
echo "$out" | grep -q "KEEP .*feature/preserved.*preserved tools/\*/work" \
  || fail "preserved work did not block clean merged removal: $out"
echo "$out" | grep -q "WOULD REMOVE .*feature/preserved" \
  && fail "preserved work was reported for removal: $out"
[ -d "$E2EP/wt" ] || fail "preserved worktree disappeared during dry-run"
pass "nonempty tools/*/work blocks clean merged worktree removal"

# An ignored tools/*/work symlink can point at an external capture directory;
# find's default non-following behavior must not let it be removed as disposable.
E2EPS="$SANDBOX/e2e-preserved-symlink"
mkdir -p "$E2EPS/main"
git init -q "$E2EPS/main"
git -C "$E2EPS/main" config user.email test@example.com
git -C "$E2EPS/main" config user.name "test"
git -C "$E2EPS/main" config commit.gpgsign false
printf 'tools/*/work\n' > "$E2EPS/main/.gitignore"
printf 'x\n' > "$E2EPS/main/a.txt"
git -C "$E2EPS/main" add .
git -C "$E2EPS/main" commit -qm c1
git -C "$E2EPS/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2EPS/main" worktree add -q -b feature/preserved-symlink "$E2EPS/wt" HEAD
E2EPS_CAPTURE="$SANDBOX/external-capture"
mkdir -p "$E2EPS_CAPTURE" "$E2EPS/wt/tools/rivet-oracle"
ln -s "$E2EPS_CAPTURE" "$E2EPS/wt/tools/rivet-oracle/work"
printf 'capture\n' > "$E2EPS_CAPTURE/manifest.json"
[ -z "$(git -C "$E2EPS/wt" status --porcelain)" ] || fail "ignored work symlink dirtied its clean worktree"
git -C "$E2EPS/wt" check-ignore -q tools/rivet-oracle/work \
  || fail "work symlink fixture was not ignored"
cd "$E2EPS/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "KEEP .*feature/preserved-symlink.*preserved tools/\*/work" \
  || fail "preserved work symlink did not block clean merged removal: $out"
[ -d "$E2EPS/wt" ] || fail "preserved work symlink worktree was removed"
pass "nonempty symlinked tools/*/work blocks clean merged worktree removal"

# --- e2e: a locked worktree is never counted as removed ------------------------
# `git worktree remove --force` refuses a locked worktree (only remove -f -f
# overrides), so the sweep must report it as kept — not WOULD REMOVE/REMOVE and
# not a removal count a real run cannot perform. Both the dry run and the real
# run must leave the locked worktree, its branch, and its lock intact.
E2E5="$SANDBOX/e2e5"
mkdir -p "$E2E5/main"
git init -q "$E2E5/main"
git -C "$E2E5/main" config user.email test@example.com
git -C "$E2E5/main" config user.name "test"
git -C "$E2E5/main" config commit.gpgsign false
printf 'x\n' > "$E2E5/main/a.txt"
git -C "$E2E5/main" add a.txt
git -C "$E2E5/main" commit -qm c1
git -C "$E2E5/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E5/main" worktree add -q -b feature/locked "$E2E5/wt" HEAD
git -C "$E2E5/wt" worktree lock "$E2E5/wt" --reason "active manual work"
cd "$E2E5/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1)
echo "$out" | grep -q "KEEP .*feature/locked.*locked" || fail "locked e2e dry-run missing locked KEEP line: $out"
echo "$out" | grep -q "WOULD REMOVE .*feature/locked" && fail "locked e2e dry-run claimed a WOULD REMOVE: $out"
echo "$out" | grep -q "would remove 0 worktree(s)" || fail "locked e2e dry-run counted a removal it cannot do: $out"
[ -d "$E2E5/wt" ] || fail "locked e2e dry-run removed the locked worktree"
git -C "$E2E5/wt" worktree list --porcelain | grep -q "^locked" || fail "locked e2e dry-run dropped the lock"
pass "e2e locked worktree: dry-run reports kept, counts no removal"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --no-tmp 2>&1)
echo "$out" | grep -q "KEEP .*feature/locked.*locked" || fail "locked e2e real run missing locked KEEP line: $out"
[ -d "$E2E5/wt" ] || fail "locked e2e real run removed the locked worktree"
git -C "$E2E5/main" branch --list feature/locked | grep -q . || fail "locked e2e real run deleted the locked branch"
git -C "$E2E5/wt" worktree list --porcelain | grep -q "^locked" || fail "locked e2e real run dropped the lock"
pass "e2e locked worktree: real run keeps worktree, branch, and lock"

# --- e2e: removal is a plain `worktree remove` (no --force); git's dirty-
# --- refusal at removal time backstops the TOCTOU window ----------------------
# The clean probe and the remove are adjacent lines, so the only backstop for a
# file that lands dirty between them is git's own refusal inside `worktree
# remove`. `--force` would strip that backstop, so the emitted command is pinned
# two ways: the dry-run line names the exact command (catches a re-added
# --force), and the same plain command refuses a dirtied worktree with the file
# preserved.
E2E6="$SANDBOX/e2e6"
mkdir -p "$E2E6/main"
git init -q "$E2E6/main"
git -C "$E2E6/main" config user.email test@example.com
git -C "$E2E6/main" config user.name "test"
git -C "$E2E6/main" config commit.gpgsign false
printf 'x\n' > "$E2E6/main/a.txt"
git -C "$E2E6/main" add a.txt
git -C "$E2E6/main" commit -qm c1
git -C "$E2E6/main" update-ref refs/remotes/origin/main HEAD
git -C "$E2E6/main" worktree add -q -b feature/backstop "$E2E6/wt" HEAD
cd "$E2E6/main"
out=$(bash "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1)
echo "$out" | grep -q "worktree remove --force" && fail "e2e backstop: emitted worktree remove --force (reopens TOCTOU window)"
wt_c=$(cd "$E2E6/wt" && pwd -P)  # git records the physical path (/tmp -> /private/tmp)
echo "$out" | grep -q "worktree remove $wt_c" || fail "e2e backstop: dry-run missing plain worktree remove line for $wt_c"
pass "e2e removal emits a plain worktree remove (no --force)"

# the backstop itself: the same plain command refuses a worktree dirtied after a
# clean probe and preserves the file
printf 'y\n' >> "$E2E6/wt/a.txt"
if git -C "$E2E6/main" worktree remove "$E2E6/wt" 2>/dev/null; then
  fail "e2e backstop: plain worktree remove deleted a dirty worktree"
fi
[ -d "$E2E6/wt" ] || fail "e2e backstop: dirty worktree was removed"
[ -f "$E2E6/wt/a.txt" ] || fail "e2e backstop: dirty file was lost"
pass "e2e plain worktree remove refuses a dirtied worktree; file survives"

# --- zsh: sourcing + classification works (no failglob abort) ----------------
if command -v zsh >/dev/null 2>&1; then
  zrc=0
  zsh -c '
    set -o pipefail
    cd "$1"
    source ./scripts/prune-worktrees.sh
    S=$(mktemp -d)
    mkdir -p "$S/debug/.fingerprint"
    printf "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n" > "$S/CACHEDIR.TAG"
    printf "{}\n" > "$S/.rustc_info.json"
    touch -m -t 202001010000 "$S" "$S/CACHEDIR.TAG" "$S/.rustc_info.json" "$S/debug" "$S/debug/.fingerprint"
    is_cargo_target "$S" || { echo "zsh: strict tier failed"; exit 1; }
    # a custom-profile-only dir (dist/, no debug/release) must not abort on the
    # fingerprint probe even though no debug/release glob would match
    S2=$(mktemp -d)
    mkdir -p "$S2/dist/.fingerprint"
    printf "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n" > "$S2/CACHEDIR.TAG"
    printf "{}\n" > "$S2/.rustc_info.json"
    touch -m -t 202001010000 "$S2" "$S2/CACHEDIR.TAG" "$S2/.rustc_info.json" "$S2/dist" "$S2/dist/.fingerprint"
    is_cargo_target "$S2" || { echo "zsh: custom-profile strict tier failed"; exit 1; }
    # a dir with no profile at all must be refused without aborting
    S3=$(mktemp -d)
    printf "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n" > "$S3/CACHEDIR.TAG"
    printf "{}\n" > "$S3/.rustc_info.json"
    if is_cargo_target "$S3"; then echo "zsh: no-profile dir accepted"; exit 1; fi
    rm -rf "$S" "$S2" "$S3"
    echo "zsh-ok"
  ' _ "$SCRIPT_DIR/.." || zrc=$?
  if [ "$zrc" -eq 0 ]; then
    pass "zsh: source + classification works, no failglob abort"
  else
    fail "zsh source/use test failed (rc=$zrc)"
  fi

  # regression: direct exec under zsh must run main (BASH_SOURCE is empty there,
  # so the old bash-only guard silently no-op'd). A --no-tmp --dry-run run must
  # print the summary line.
  zout=$(zsh "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1) || zrc=$?
  echo "$zout" | grep -q "would remove" || fail "zsh direct exec did not run main(): $zout"
  pass "zsh direct exec runs main()"

  # A clean merged worktree must be discoverable and removable under zsh too;
  # this exercises the full main() path rather than only sourced classifiers.
  E2EZ="$SANDBOX/e2e-zsh"
  mkdir -p "$E2EZ/main"
  git init -q "$E2EZ/main"
  git -C "$E2EZ/main" config user.email test@example.com
  git -C "$E2EZ/main" config user.name "test"
  git -C "$E2EZ/main" config commit.gpgsign false
  printf 'x\\n' > "$E2EZ/main/a.txt"
  git -C "$E2EZ/main" add a.txt
  git -C "$E2EZ/main" commit -qm c1
  git -C "$E2EZ/main" update-ref refs/remotes/origin/main HEAD
  git -C "$E2EZ/main" worktree add -q -b feature/merged-zsh "$E2EZ/wt" HEAD
  zout=$(cd "$E2EZ/main" && zsh "$SCRIPT_DIR/prune-worktrees.sh" --dry-run --no-tmp 2>&1)
  echo "$zout" | grep -q "WOULD REMOVE .*feature/merged-zsh" \
    || fail "zsh clean merged worktree was not classified for removal: $zout"
  [ -d "$E2EZ/wt" ] || fail "zsh clean merged dry-run removed the worktree"
  pass "zsh clean merged worktree dry-run classifies removal without deleting"

  # interactive sourcing must NOT run main (guard false-positive would sweep).
  # Source with --dry-run --no-tmp: a regression stays harmless (no fetch, no
  # removal, no /private/tmp sweep) and is caught by main()'s own output, not
  # by the incidental exit-2 a stray positional arg currently produces.
  zrc=0
  zout=$(zsh -i -c 'source "$1" --dry-run --no-tmp; echo "sourced-ok"' _ "$SCRIPT_DIR/prune-worktrees.sh" 2>/dev/null) || zrc=$?
  if [ "$zrc" -eq 0 ] \
     && printf '%s\n' "$zout" | grep -q "sourced-ok" \
     && ! printf '%s\n' "$zout" | grep -qE 'WOULD|would remove|would prune|DRY:'; then
    pass "zsh interactive source does not run main()"
  else
    fail "zsh interactive source ran main() (or failed): $zout"
  fi
else
  pass "zsh not installed; skipping zsh source/use test"
fi

echo
echo "ALL TESTS PASSED"
