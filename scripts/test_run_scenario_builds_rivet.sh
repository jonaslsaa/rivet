#!/bin/bash
# Focused test for run-scenario.sh's on-demand rivet-server build (issues #157,
# #86, #316, #374). The wrapper must build `rivet-server` for every mode that
# boots exactly one Rivet server (dwell/kick/load-world/loaded-world) and for
# `--server rivet|both`, and must NOT build it for the Paper-only default (join
# with no --server). Cargo is shimmed to record `-p rivet-server` builds; the
# run-scenario binary is a stub that records its argv so the test can also
# assert the requested mode is passed through verbatim.
#
#   ./scripts/test_run_scenario_builds_rivet.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

TOOL_DIR="$TMP/rivet-client"
BIN_DIR="$TOOL_DIR/target/debug"
mkdir -p "$BIN_DIR"

# The real wrapper script under test, copied into the fake tool dir so its
# relative `./target/debug/run-scenario` resolution points at the stub below.
cp "$SCRIPT_DIR/../tools/rivet-client/run-scenario.sh" "$TOOL_DIR/run-scenario.sh"

# The run-scenario binary stub: records argv, exits 0. The log path is baked
# in (the stub runs as a child process that does not inherit the test's shell
# variables); $TMP from mktemp -d is absolute and space-free.
SCENARIO_ARGV_LOG="$TMP/argv.log"
cat > "$BIN_DIR/run-scenario" <<EOF
#!/bin/bash
echo "\$@" >> "$SCENARIO_ARGV_LOG"
exit 0
EOF
chmod +x "$BIN_DIR/run-scenario"

# Cargo shim: records every invocation and exits 0. The wrapper's stable-toolchain
# build runs from the repo root (`cd "$tool_dir/../.."`), which is $TMP here;
# the shim is found via the prepended PATH, so its baked-in log path must be
# absolute and space-free (mktemp -d guarantees that on macOS/Linux).
CARGO_LOG="$TMP/cargo.log"
cat > "$TMP/cargo" <<EOF
#!/bin/bash
echo "\$@" >> "$CARGO_LOG"
exit 0
EOF
chmod +x "$TMP/cargo"

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# run_wrapper: run run-scenario.sh in a fresh state, recording cargo invocations
# and the run-scenario argv. $1.. = the wrapper's argv.
run_wrapper() {
  : > "$CARGO_LOG"
  : > "$SCENARIO_ARGV_LOG"
  PATH="$TMP:$PATH" "$TOOL_DIR/run-scenario.sh" "$@"
}

# built_rivet: does the recorded cargo log contain the `-p rivet-server` build?
built_rivet() {
  grep -q -- "-p" "$CARGO_LOG" && grep -q -- "rivet-server" "$CARGO_LOG"
}

# Rivet-only modes must always trigger the server build (server selection is
# pinned to Rivet even without --server).
for mode in dwell kick load-world loaded-world; do
  run_wrapper "$mode"
  built_rivet || fail "$mode must build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
  [ "$(wc -l < "$SCENARIO_ARGV_LOG" | tr -d ' ')" = 1 ] || fail "$mode: expected exactly 1 run-scenario invocation"
  grep -qx "$mode" "$SCENARIO_ARGV_LOG" || fail "$mode: mode not passed through verbatim (got $(cat "$SCENARIO_ARGV_LOG"))"
  pass "$mode builds rivet-server and passes the mode through"
done

# --server rivet|both selects Rivet for the join/move/capture modes.
for sel in rivet both; do
  run_wrapper join --server "$sel"
  built_rivet || fail "join --server $sel must build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
  grep -qx "join --server $sel" "$SCENARIO_ARGV_LOG" || fail "join --server $sel not passed through verbatim (got $(cat "$SCENARIO_ARGV_LOG"))"
  pass "join --server $sel builds rivet-server"
done

# The Paper-only default must stay exactly as fast as before: no server build.
run_wrapper join
built_rivet && fail "join (Paper-only default) must NOT build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
grep -qx "join" "$SCENARIO_ARGV_LOG" || fail "join not passed through verbatim (got $(cat "$SCENARIO_ARGV_LOG"))"
pass "join (Paper-only default) does not build rivet-server"

# An explicit --server paper must not build rivet-server either.
run_wrapper move --server paper
built_rivet && fail "move --server paper must NOT build rivet-server (cargo log: $(cat "$CARGO_LOG"))"
pass "move --server paper does not build rivet-server"

# Every invocation still runs the runner's own unit tests first (the wrapper's
# self-test step), so the cargo log must carry the package test command.
run_wrapper loaded-world
grep -q -- "test --locked --bin run-scenario" "$CARGO_LOG" || fail "loaded-world: runner self-test not run before the scenario (cargo log: $(cat "$CARGO_LOG"))"
pass "the runner's own unit tests run before the scenario"

echo
echo "ALL RUN-SCENARIO RIVET-BUILD TESTS PASSED"
