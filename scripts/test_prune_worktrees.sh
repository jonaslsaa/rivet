#!/bin/bash
# Focused tests for prune-worktrees.sh's tmp-scratch classification.
#
# Regression: the tmp sweep originally treated any /tmp child whose root had
# CACHEDIR.TAG as disposable cargo scratch and rm -rf'd the whole directory.
# CACHEDIR.TAG is a generic cache marker — an unrelated cache tool or a source
# checkout can carry one at its root — so the sweep could destroy a checkout or
# a generic cache. These tests pin the tightened classifier: only an
# unambiguous bare cargo target dir (CACHEDIR.TAG + .rustc_info.json + a
# profile dir, no Cargo.toml/.git) is removable wholesale; a checkout's nested
# target/ is pruned on its own; a tagged generic cache or a tagged source
# checkout is left untouched.
#
# Sources scripts/prune-worktrees.sh (its main body is guarded) and drives
# is_cargo_target / tmp_cache_dirs / sweep_tmp against a sandbox tree.
#
#   ./scripts/test_prune_worktrees.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

source "$SCRIPT_DIR/prune-worktrees.sh"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# --- sandbox fixtures (all old, so sweep_tmp treats them as idle) ------------
mk_cargo_target() { # $1 dir; a recognizable cargo CARGO_TARGET_DIR
  mkdir -p "$1/debug/.fingerprint"
  printf 'Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag.\n' > "$1/CACHEDIR.TAG"
  printf '{}\n' > "$1/.rustc_info.json"
  touch -m -t 202001010000 "$1" "$1/CACHEDIR.TAG" "$1/.rustc_info.json" "$1/debug" "$1/debug/.fingerprint"
}
mk_tagged() { # $1 dir; generic cache: CACHEDIR.TAG only, no cargo artifacts
  mkdir -p "$1"
  printf 'Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag.\n' > "$1/CACHEDIR.TAG"
  touch -m -t 202001010000 "$1" "$1/CACHEDIR.TAG"
}

classify() { tmp_cache_dirs "$1"; }  # prints eligible cache dirs, one per line

# --- classification: is_cargo_target ----------------------------------------
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
touch -m -t 202001010000 "$SANDBOX/$R/tag-no-profile/.rustc_info.json"
if is_cargo_target "$SANDBOX/$R/tag-no-profile"; then
  fail "tag + .rustc_info.json but no profile dir was classified disposable"
else
  pass "tag + .rustc_info.json but no profile dir is refused"
fi

mk_cargo_target "$SANDBOX/$R/source-root"
touch "$SANDBOX/$R/source-root/Cargo.toml"
mkdir "$SANDBOX/$R/source-root/.git"
if is_cargo_target "$SANDBOX/$R/source-root"; then
  fail "dir with cargo markers PLUS Cargo.toml/.git was classified disposable"
else
  pass "cargo-marked dir that is also a source/VCS root is refused"
fi

# --- classification: tmp_cache_dirs (what the sweep would consider) ----------
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

# --- sweep_tmp end to end ----------------------------------------------------
SWEEP_ROOT="$SANDBOX/root-sweep"
mkdir -p "$SWEEP_ROOT"
# positive: a bare cargo target dir is deleted
mk_cargo_target "$SWEEP_ROOT/bare"
# hostile: tagged generic cache survives
mk_tagged "$SWEEP_ROOT/cache"
touch "$SWEEP_ROOT/cache/data.bin"
# hostile: tagged source checkout survives wholesale; its target is deleted
CK4="$SWEEP_ROOT/checkout"
mkdir -p "$CK4/target"
printf '[workspace]\n' > "$CK4/Cargo.toml"
mkdir "$CK4/.git"
mk_tagged "$CK4"
mk_cargo_target "$CK4/target"

DRY=0; IDLE_HOURS=24; freed_kb=0; pruned=0
sweep_tmp "$SWEEP_ROOT" >/dev/null

[ -d "$SWEEP_ROOT/bare" ] && fail "bare cargo target dir not deleted" || true
[ -d "$SWEEP_ROOT/cache" ] || fail "generic cache was deleted"
[ -d "$SWEEP_ROOT/checkout" ] || fail "tagged source checkout was deleted"
[ -d "$CK4/target" ] && fail "checkout's nested target/ not deleted" || true
[ -f "$SWEEP_ROOT/cache/data.bin" ] || fail "generic cache contents were deleted"
pass "real sweep: bare target pruned; generic cache and tagged checkout survive; nested target pruned"

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
