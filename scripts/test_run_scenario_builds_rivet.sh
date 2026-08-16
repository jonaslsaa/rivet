#!/bin/bash
# Focused tests for run-scenario.sh's on-demand builds and resolved Cargo target
# paths. Cargo is shimmed, so the scenarios never start a real server or build.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

TOOL_DIR="$TMP/tools/rivet-client"
TARGET_DIR="$TMP/shared-target"
BIN_DIR="$TARGET_DIR/debug"
mkdir -p "$TOOL_DIR" "$BIN_DIR" "$TMP/scripts"
cp "$SCRIPT_DIR/../tools/rivet-client/run-scenario.sh" "$TOOL_DIR/run-scenario.sh"
cp "$SCRIPT_DIR/with-build-lock.sh" "$TMP/scripts/with-build-lock.sh"
chmod +x "$TMP/scripts/with-build-lock.sh"

SCENARIO_ARGV_LOG="$TMP/argv.log"
cat > "$BIN_DIR/run-scenario" <<EOF
#!/bin/bash
echo "\$@" >> "$SCENARIO_ARGV_LOG"
exit 0
EOF
chmod +x "$BIN_DIR/run-scenario"

CARGO_LOG="$TMP/cargo.log"
cat > "$TMP/cargo" <<EOF
#!/bin/bash
echo "\$@" >> "$CARGO_LOG"
case " \$* " in
  *" metadata "*) printf '%s\\n' '{"target_directory":"$TARGET_DIR"}' ;;
esac
exit 0
EOF
chmod +x "$TMP/cargo"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

run_wrapper() {
  : > "$CARGO_LOG"
  : > "$SCENARIO_ARGV_LOG"
  RIVET_BUILD_LOCK_HELD=1 CARGO_TARGET_DIR="$TARGET_DIR" PATH="$TMP:$PATH" \
    "$TOOL_DIR/run-scenario.sh" "$@"
}

run_unlocked() {
  : > "$CARGO_LOG"
  : > "$SCENARIO_ARGV_LOG"
  env -u RIVET_BUILD_LOCK_HELD CARGO_TARGET_DIR="$TARGET_DIR" PATH="$TMP:$PATH" \
    "$TOOL_DIR/run-scenario.sh" "$@"
}

built_rivet() {
  grep -q -- "-p" "$CARGO_LOG" && grep -q -- "rivet-server" "$CARGO_LOG"
}

for mode in dwell kick load-world loaded-world recenter generated-world; do
  run_wrapper "$mode"
  built_rivet || fail "$mode must build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
  [ "$(wc -l < "$SCENARIO_ARGV_LOG" | tr -d ' ')" = 1 ] || fail "$mode: expected exactly 1 run-scenario invocation"
  grep -qx "$mode" "$SCENARIO_ARGV_LOG" || fail "$mode: mode not passed through (got $(cat "$SCENARIO_ARGV_LOG"))"
  grep -q -- "metadata.*manifest-path.*rivet-client/Cargo.toml" "$CARGO_LOG" || fail "$mode: tool metadata was not queried"
  pass "$mode builds rivet-server and uses Cargo's resolved target directory"
done

for sel in rivet both; do
  run_wrapper join --server "$sel"
  built_rivet || fail "join --server $sel must build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
  grep -qx "join --server $sel" "$SCENARIO_ARGV_LOG" || fail "join --server $sel not passed through (got $(cat "$SCENARIO_ARGV_LOG"))"
  pass "join --server $sel builds rivet-server"
done

run_wrapper join
built_rivet && fail "join (Paper-only default) must not build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
grep -qx "join" "$SCENARIO_ARGV_LOG" || fail "join not passed through (got $(cat "$SCENARIO_ARGV_LOG"))"
pass "join (Paper-only default) does not build rivet-server"

run_wrapper move --server paper
built_rivet && fail "move --server paper must not build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
pass "move --server paper does not build rivet-server"

run_wrapper loaded-world
grep -q -- "test --locked --bin run-scenario" "$CARGO_LOG" || fail "loaded-world: runner self-test was not run"
pass "the runner's own unit tests run before the scenario"

# The script resolves both manifests from its own location, so changing cwd to
# the macOS /private/tmp spelling must not send execution back to a stale local
# target/debug path.
(
  cd /private/tmp
  run_wrapper loaded-world
)
grep -qx loaded-world "$SCENARIO_ARGV_LOG" || fail "invocation from /private/tmp did not reach the resolved binary"
pass "invocation from /private/tmp uses the resolved target directory"

# An unlocked standalone invocation acquires the same lock that the gate owns;
# the locked path above bypasses acquisition and therefore cannot deadlock.
run_unlocked join
grep -qx join "$SCENARIO_ARGV_LOG" || fail "standalone locked invocation did not reach the binary"
grep -q -- "metadata.*manifest-path.*rivet-client/Cargo.toml" "$CARGO_LOG" \
  || fail "standalone invocation did not resolve the Cargo target directory"
pass "standalone run-scenario acquires the shared lock without recursion"

echo
echo "ALL RUN-SCENARIO RIVET-BUILD TESTS PASSED"
