#!/bin/bash
# Validate the shared Cargo target contract without compiling the workspace.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
COMMON_DIR="$(git -C "$REPO_DIR" rev-parse --path-format=absolute --git-common-dir)"
PROJECT_ROOT="$(cd "$(dirname "$COMMON_DIR")" && pwd -P)"
EXPECTED="$PROJECT_ROOT/target-agent-shared"

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
grep -Fx '/target-agent-shared/' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "shared target directory is not ignored"
grep -Fx '/target-agent-shared.lock' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "shared lock path is not ignored"
pass "tracked config is portable and shared target paths are ignored"

metadata_target() {
  env -u CARGO_TARGET_DIR "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$1" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

[ "$(metadata_target "$REPO_DIR/Cargo.toml")" = "$EXPECTED" ] \
  || fail "primary-equivalent root metadata resolved a different target"
pass "primary-equivalent root metadata resolves the canonical target"

[ "$(metadata_target "$REPO_DIR/tools/rivet-client/Cargo.toml")" = "$EXPECTED" ] \
  || fail "tools workspace metadata resolved a different target"
pass "tools workspace metadata resolves the canonical target"

nested_target="$REPO_DIR/target-agent-shared"
mkdir -p "$nested_target"
[ "$(cd "$REPO_DIR" && env -u CARGO_TARGET_DIR "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$REPO_DIR/Cargo.toml" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" = "$EXPECTED" ] \
  || fail "nested worktree metadata resolved a different target"
pass "nested worktree metadata resolves the canonical target"

[ "$(cd /tmp && "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    env CARGO_TARGET_DIR="$EXPECTED" cargo metadata --locked --no-deps --format-version 1 \
    --manifest-path "$REPO_DIR/tools/rivet-client/Cargo.toml" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" = "$EXPECTED" ] \
  || fail "out-of-repo invocation resolved a different target"
pass "out-of-repo invocation resolves the canonical target through the environment"

relative_log=$(mktemp)
if CARGO_TARGET_DIR=relative-target "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    sh -c 'printf "%s\\n" "$CARGO_TARGET_DIR"' >"$relative_log" 2>&1; then
  rm -f "$relative_log"
  fail "relative CARGO_TARGET_DIR was accepted without a child-cwd contract"
fi
grep -q "CARGO_TARGET_DIR must be absolute" "$relative_log" \
  || fail "relative CARGO_TARGET_DIR rejection was not actionable"
rm -f "$relative_log"
pass "relative CARGO_TARGET_DIR overrides are rejected before locking"

new_parent="$PROJECT_ROOT/.tmp-shared-target-tests/deep/nested/target"
rm -rf "$PROJECT_ROOT/.tmp-shared-target-tests"
CARGO_TARGET_DIR="$new_parent" "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
  sh -c '[ "$CARGO_TARGET_DIR" = "$1" ] && [ -f "$CARGO_TARGET_DIR.lock" ]' sh "$new_parent"
[ -d "$(dirname "$new_parent")" ] || fail "missing target parents were not created before locking"
rm -rf "$PROJECT_ROOT/.tmp-shared-target-tests"
pass "absolute overrides create nested parents and lock the exact target"

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
pass "shared-target shell entry points pass bash syntax checks"

if grep -Eq '/usr/bin/(lockf|flock|env)' "$REPO_DIR/scripts/with-build-lock.sh"; then
  fail "build lock utility paths must be resolved through PATH"
fi
pass "build lock utilities are resolved portably through PATH"

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -e SC2207,SC2009 "$REPO_DIR/scripts/cargo-target-dir.sh" "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR/scripts/gate.sh" \
    "$REPO_DIR/scripts/prune-worktrees.sh" "$REPO_DIR/tools/rivet-client/run.sh" \
    "$REPO_DIR/tools/rivet-client/run-scenario.sh"
  pass "shared-target shell entry points pass ShellCheck"
else
  pass "ShellCheck unavailable; syntax checks still ran"
fi

if grep -R 'target/debug' "$REPO_DIR/tools/rivet-client/src/bin/run-scenario" \
  "$REPO_DIR/tools/rivet-client/run-scenario.sh" >/dev/null; then
  fail "scenario helpers still contain a worktree-local target/debug path"
fi
pass "scenario helper paths use Cargo's resolved target directory"

echo
 echo "ALL SHARED CARGO TARGET TESTS PASSED"
