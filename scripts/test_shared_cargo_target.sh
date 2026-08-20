#!/bin/bash
# Validate checkout-local Cargo targets and the repository-wide build lock
# without compiling the workspace.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
COMMON_DIR="$(git -C "$REPO_DIR" rev-parse --path-format=absolute --git-common-dir)"
PROJECT_ROOT="$(cd "$(dirname "$COMMON_DIR")" && pwd -P)"
EXPECTED="$REPO_DIR/target"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

python3 - "$REPO_DIR/.claude/settings.json" <<'PY'
import json
import sys
settings = json.load(open(sys.argv[1]))
if "CARGO_TARGET_DIR" in settings.get("env", {}):
    raise SystemExit("settings must not hardcode a checkout-specific CARGO_TARGET_DIR")
PY
pass "Claude project settings leave target resolution to the dynamic launcher"

[ ! -e "$REPO_DIR/.cargo/config.toml" ] \
  || fail "root Cargo config hardcodes a nonportable target path"
grep -Fx '/target' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "checkout-local target directory is not ignored"
grep -Fx '/cargo-build.lock' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "repository-wide build lock is not ignored"
pass "tracked config is portable and checkout-local build paths are ignored"

metadata_target() {
  env -u CARGO_TARGET_DIR "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$1" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

[ "$(metadata_target "$REPO_DIR/Cargo.toml")" = "$EXPECTED" ] \
  || fail "root workspace metadata resolved outside this checkout"
pass "root workspace metadata resolves the checkout-local target"

[ "$(metadata_target "$REPO_DIR/tools/rivet-client/Cargo.toml")" = "$EXPECTED" ] \
  || fail "tools workspace metadata resolved outside this checkout"
pass "tools workspace metadata shares this checkout's target"

[ -f "$PROJECT_ROOT/cargo-build.lock" ] \
  || fail "build wrapper did not create the repository-wide lock"
pass "all checkout-local targets serialize through one repository-wide lock"

[ "$(cd /tmp && CARGO_TARGET_DIR="$EXPECTED" "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    cargo metadata --locked --no-deps --format-version 1 \
    --manifest-path "$REPO_DIR/tools/rivet-client/Cargo.toml" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" = "$EXPECTED" ] \
  || fail "out-of-repo invocation resolved a different target"
pass "out-of-repo invocation preserves the checkout-local target"

relative_log=$(mktemp)
if CARGO_TARGET_DIR=relative-target "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    sh -c 'printf "%s\n" "$CARGO_TARGET_DIR"' >"$relative_log" 2>&1; then
  rm -f "$relative_log"
  fail "relative CARGO_TARGET_DIR was accepted"
fi
grep -q "CARGO_TARGET_DIR must be absolute" "$relative_log" \
  || fail "relative CARGO_TARGET_DIR rejection was not actionable"
rm -f "$relative_log"
pass "relative CARGO_TARGET_DIR overrides are rejected before locking"

foreign="$PROJECT_ROOT/foreign-cargo-target"
foreign_log=$(mktemp)
if CARGO_TARGET_DIR="$foreign" "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" true \
    >"$foreign_log" 2>&1; then
  rm -f "$foreign_log"
  fail "foreign absolute CARGO_TARGET_DIR was accepted"
fi
grep -q "must be the current checkout target" "$foreign_log" \
  || fail "foreign target rejection was not actionable"
rm -f "$foreign_log"
pass "absolute overrides cannot collapse distinct checkout targets"

# A linked-worktree regression pins the root cause of the stale fingerprint
# failure found by PR #675: both checkouts of the same package IDs must resolve
# different target directories even though they share one git common directory.
TMP_REPO=$(mktemp -d)
TMP_LINKED="${TMP_REPO}-linked"
cleanup() {
  git -C "$TMP_REPO" worktree remove --force "$TMP_LINKED" >/dev/null 2>&1 || true
  rm -rf "$TMP_REPO" "$TMP_LINKED"
}
trap cleanup EXIT
git -C "$TMP_REPO" init -q
git -C "$TMP_REPO" config user.name test
git -C "$TMP_REPO" config user.email test@example.invalid
printf 'fixture\n' > "$TMP_REPO/README"
git -C "$TMP_REPO" add README
git -C "$TMP_REPO" commit -qm fixture
git -C "$TMP_REPO" worktree add -qb linked "$TMP_LINKED"
# shellcheck source=scripts/cargo-target-dir.sh
# shellcheck disable=SC1091
source "$REPO_DIR/scripts/cargo-target-dir.sh"
primary_target=$(env -u CARGO_TARGET_DIR bash -c 'source "$1"; cargo_target_dir_for "$2"' sh \
  "$REPO_DIR/scripts/cargo-target-dir.sh" "$TMP_REPO")
linked_target=$(env -u CARGO_TARGET_DIR bash -c 'source "$1"; cargo_target_dir_for "$2"' sh \
  "$REPO_DIR/scripts/cargo-target-dir.sh" "$TMP_LINKED")
[ "$primary_target" = "$TMP_REPO/target" ] \
  || fail "temporary primary checkout resolved unexpected target $primary_target"
[ "$linked_target" = "$TMP_LINKED/target" ] \
  || fail "temporary linked checkout resolved unexpected target $linked_target"
[ "$primary_target" != "$linked_target" ] \
  || fail "linked worktrees resolved the same Cargo target"
pass "linked worktrees cannot reuse one another's Cargo fingerprints"

for script in \
  "$REPO_DIR/scripts/cargo-target-dir.sh" \
  "$REPO_DIR/scripts/with-build-lock.sh" \
  "$REPO_DIR/scripts/gate.sh" \
  "$REPO_DIR/scripts/prune-worktrees.sh" \
  "$REPO_DIR/tools/rivet-client/run.sh" \
  "$REPO_DIR/tools/rivet-client/run-scenario.sh"
do
  bash -n "$script"
done
pass "checkout-target shell entry points pass bash syntax checks"

if grep -Eq '/usr/bin/(lockf|flock|env)' "$REPO_DIR/scripts/with-build-lock.sh"; then
  fail "build lock utility paths must be resolved through PATH"
fi
pass "build lock utilities are resolved portably through PATH"

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -e SC2207,SC2009 "$REPO_DIR/scripts/cargo-target-dir.sh" "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR/scripts/gate.sh" \
    "$REPO_DIR/scripts/prune-worktrees.sh" "$REPO_DIR/tools/rivet-client/run.sh" \
    "$REPO_DIR/tools/rivet-client/run-scenario.sh"
  pass "checkout-target shell entry points pass ShellCheck"
else
  pass "ShellCheck unavailable; syntax checks still ran"
fi

if grep -R 'target/debug' "$REPO_DIR/tools/rivet-client/src/bin/run-scenario" \
  "$REPO_DIR/tools/rivet-client/run-scenario.sh" >/dev/null; then
  fail "scenario helpers still contain an unresolved target/debug path"
fi
pass "scenario helper paths use Cargo's resolved target directory"

echo
echo "ALL CHECKOUT-LOCAL CARGO TARGET TESTS PASSED"
