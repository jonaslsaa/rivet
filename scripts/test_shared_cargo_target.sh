#!/bin/bash
# Validate the shared Cargo target contract without compiling the workspace.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
EXPECTED="/Users/jonass@kahoot.com/Documents/Personal/Rivet/target-agent-shared"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

python3 - "$REPO_DIR/.claude/settings.json" "$EXPECTED" <<'PY'
import json
import sys
settings = json.load(open(sys.argv[1]))
expected = sys.argv[2]
if settings.get("env", {}).get("CARGO_TARGET_DIR") != expected:
    raise SystemExit("settings CARGO_TARGET_DIR is not canonical")
PY
pass "Claude project settings use the canonical shared target"

grep -Fx 'target-dir = "/Users/jonass@kahoot.com/Documents/Personal/Rivet/target-agent-shared"' \
  "$REPO_DIR/.cargo/config.toml" >/dev/null \
  || fail "root Cargo config does not use the canonical shared target"
grep -Fx '/target-agent-shared/' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "shared target directory is not ignored"
grep -Fx '/target-agent-shared.lock' "$REPO_DIR/.gitignore" >/dev/null \
  || fail "shared lock path is not ignored"
pass "Cargo config and gitignore pin the shared target and lock"

metadata_target() {
  "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" env -u CARGO_TARGET_DIR \
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
[ "$(cd "$REPO_DIR" && "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" env -u CARGO_TARGET_DIR \
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$REPO_DIR/Cargo.toml" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" = "$EXPECTED" ] \
  || fail "nested worktree metadata resolved a different target"
pass "nested worktree metadata resolves the canonical target"

[ "$(cd /private/tmp && "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" \
    env CARGO_TARGET_DIR="$EXPECTED" cargo metadata --locked --no-deps --format-version 1 \
    --manifest-path "$REPO_DIR/tools/rivet-client/Cargo.toml" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" = "$EXPECTED" ] \
  || fail "/private/tmp invocation resolved a different target"
pass "/private/tmp invocation resolves the canonical target through the environment"

bash -n "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR/scripts/gate.sh" \
  "$REPO_DIR/scripts/prune-worktrees.sh" "$REPO_DIR/tools/rivet-client/run.sh" \
  "$REPO_DIR/tools/rivet-client/run-scenario.sh"
pass "shared-target shell entry points pass bash syntax checks"

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -e SC2207,SC2009 "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR/scripts/gate.sh" \
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
