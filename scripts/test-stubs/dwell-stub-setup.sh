# Shared dwell-stub setup + counterfactual for the gate sandbox tests
# (scripts/test_codegen_gate.sh, scripts/test_gate_features.sh,
# scripts/test_gate_fuzz.sh, scripts/test_gate_packets.sh). Sourced by a test
# script after it cds to the repo root. install_dwell_stub takes the sandbox
# root as $1; dwell_gate_counterfactuals reads the caller's $SANDBOX and $GATE.
# Both set / use the $DWELL_STUB_LOG and $DWELL_STUB_FAIL globals that the
# test's GATE command must carry. Sourced-only (no shebang, never executable).
# shellcheck shell=bash
# shellcheck disable=SC2153 # SANDBOX is set by the sourcing test script.
#
# The full gate (scripts/gate.sh) invokes tools/rivet-client/run-scenario.sh
# dwell --server rivet unconditionally (issue #160, terminal M1 acceptance). The
# stub setup keeps the sandbox faithful to that contract instead of reducing it
# to a vacuous bypass:
#   - install_dwell_stub    copies scripts/test-stubs/run-scenario.sh into the
#                           sandbox and prepares the invocation log;
#   - assert_dwell_invoked   proves the exact `dwell --server rivet` call ran
#                           (a dropped or wrong row fails the assertion);
#   - dwell_gate_counterfactuals  proves four discriminations:
#                               (a) a failing dwell verdict reddens the gate;
#                               (b) a wrong invocation is rejected by the stub;
#                               (c) a removed dwell row leaves the stub log empty
#                                   — the exact case assert_dwell_invoked rejects;
#                               (d) a leaked RIVET_ORACLE_JAR cannot wake the
#                                   join/move rows into the strict stub.

install_dwell_stub() { # $1 = sandbox root
  local sandbox="$1"
  mkdir -p "$sandbox/tools/rivet-client"
  cp "$PWD/scripts/test-stubs/run-scenario.sh" "$sandbox/tools/rivet-client/run-scenario.sh"
  chmod +x "$sandbox/tools/rivet-client/run-scenario.sh"
  DWELL_STUB_LOG="$sandbox/dwell-invocations.log"
  DWELL_STUB_FAIL="$sandbox/fail_dwell"
  : > "$DWELL_STUB_LOG"
}

assert_dwell_invoked() { # $1 = label (e.g. "nextest, scenario 0")
  grep -q -- "dwell --server rivet" "$DWELL_STUB_LOG" \
    || { echo "FAIL ($1): dwell row did not invoke run-scenario.sh with 'dwell --server rivet'" >&2; exit 1; }
}

dwell_gate_counterfactuals() {
  # (a) A failing dwell verdict reddens the gate. With the fail marker present
  # the stub exits nonzero, so gate.sh's set -e aborts before GATE GREEN — the
  # M1 dwell row is never skipped, and the red is not the oracle verdict.
  : > "$DWELL_STUB_LOG"
  touch "$DWELL_STUB_FAIL"
  if eval "$GATE" > "$SANDBOX/dwell-red.log" 2>&1; then
    echo "FAIL (dwell counterfactual): full gate exited 0 despite a failing dwell verdict" >&2
    exit 1
  fi
  grep -q "stub: failing dwell scenario invocation" "$SANDBOX/dwell-red.log" \
    || { echo "FAIL (dwell counterfactual): the dwell stub was not invoked to fail" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/dwell-red.log" && {
    echo "FAIL (dwell counterfactual): 'GATE GREEN' printed despite a dwell failure" >&2; exit 1; }
  grep -q "ORACLE UNVERIFIED" "$SANDBOX/dwell-red.log" && {
    echo "FAIL (dwell counterfactual): red came from the oracle verdict, not the dwell failure" >&2; exit 1; }
  rm -f "$DWELL_STUB_FAIL"
  echo "ok (dwell counterfactual): a failing dwell verdict reddens the gate"

  # (b) A wrong invocation is rejected. Tamper the sandbox's gate.sh copy so the
  # dwell row calls the runner with `dwell --server paper`; the stub must refuse
  # it and the gate must be red, proving the dwell contract cannot drift.
  sed 's/run-scenario.sh" dwell --server rivet/run-scenario.sh" dwell --server paper/' \
    "$SANDBOX/scripts/gate.sh" > "$SANDBOX/scripts/gate.sh.tmp"
  mv "$SANDBOX/scripts/gate.sh.tmp" "$SANDBOX/scripts/gate.sh"
  chmod +x "$SANDBOX/scripts/gate.sh"
  # If the dwell row is ever reformatted the sed above would silently no-op; fail
  # loudly here rather than let the gate run unmodified and redden spuriously.
  grep -q 'run-scenario.sh" dwell --server paper' "$SANDBOX/scripts/gate.sh" \
    || { echo "FAIL (dwell counterfactual): could not tamper the dwell row to '--server paper'" >&2; exit 1; }
  : > "$DWELL_STUB_LOG"
  if eval "$GATE" > "$SANDBOX/dwell-wrong.log" 2>&1; then
    echo "FAIL (dwell counterfactual): full gate exited 0 despite a wrong dwell invocation" >&2
    exit 1
  fi
  grep -q "unexpected scenario invocation" "$SANDBOX/dwell-wrong.log" \
    || { echo "FAIL (dwell counterfactual): the stub did not reject the wrong dwell invocation" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/dwell-wrong.log" && {
    echo "FAIL (dwell counterfactual): 'GATE GREEN' printed despite a wrong dwell invocation" >&2; exit 1; }
  cp "$PWD/scripts/gate.sh" "$SANDBOX/scripts/gate.sh"
  chmod +x "$SANDBOX/scripts/gate.sh"
  echo "ok (dwell counterfactual): a wrong dwell invocation reddens the gate"

  # (c) A removed dwell row is not silently green. Without the row the gate still
  # reaches GATE GREEN, but the stub is never invoked so the invocation log stays
  # empty — exactly the case assert_dwell_invoked (scenario 0) fails on. The
  # sandbox test can therefore never be fooled by a gate that drops the row.
  grep -v 'run-scenario.sh" dwell --server rivet' "$SANDBOX/scripts/gate.sh" \
    > "$SANDBOX/scripts/gate.sh.tmp"
  mv "$SANDBOX/scripts/gate.sh.tmp" "$SANDBOX/scripts/gate.sh"
  chmod +x "$SANDBOX/scripts/gate.sh"
  # If the dwell row is ever reformatted the grep -v above would silently leave
  # it in place; fail loudly here rather than trip the confusing downstream check.
  grep -q 'run-scenario.sh" dwell --server rivet' "$SANDBOX/scripts/gate.sh" \
    && { echo "FAIL (dwell counterfactual): could not remove the dwell row" >&2; exit 1; }
  : > "$DWELL_STUB_LOG"
  if ! eval "$GATE" > "$SANDBOX/dwell-missing.log" 2>&1; then
    echo "FAIL (dwell counterfactual): full gate without the dwell row did not exit 0" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/dwell-missing.log" \
    || { echo "FAIL (dwell counterfactual): full gate without the dwell row did not reach GATE GREEN" >&2; exit 1; }
  if grep -q -- "dwell --server rivet" "$DWELL_STUB_LOG"; then
    echo "FAIL (dwell counterfactual): the dwell stub was invoked despite the row being removed" >&2
    exit 1
  fi
  cp "$PWD/scripts/gate.sh" "$SANDBOX/scripts/gate.sh"
  chmod +x "$SANDBOX/scripts/gate.sh"
  echo "ok (dwell counterfactual): a removed dwell row is caught (stub log stays empty)"

  # (d) A leaked RIVET_ORACLE_JAR must not wake the join/move rows. The test
  # scripts' GATE env unsets it, so a developer's real oracle path cannot route
  # join/move into the strict stub. Prove the leak is neutralized: with
  # RIVET_ORACLE_JAR set to an existing jar, the gate must stay green, the stub
  # log must show only the dwell row, and no unexpected invocation may reach
  # the stub. The leak probe depends on the sandbox paperclip jar existing, so
  # assert it up front rather than let a missing jar vacuously pass the check.
  test -f "$SANDBOX/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar" \
    || { echo "FAIL (dwell counterfactual): sandbox paperclip jar missing for the leak probe" >&2; exit 1; }
  (
    : > "$DWELL_STUB_LOG"
    RIVET_ORACLE_JAR="$SANDBOX/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar" \
      eval "$GATE" > "$SANDBOX/dwell-leak.log" 2>&1
  ) || { echo "FAIL (dwell counterfactual): leaked RIVET_ORACLE_JAR reddened the gate" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/dwell-leak.log" \
    || { echo "FAIL (dwell counterfactual): leaked RIVET_ORACLE_JAR gate did not reach GATE GREEN" >&2; exit 1; }
  grep -q -- "dwell --server rivet" "$DWELL_STUB_LOG" \
    || { echo "FAIL (dwell counterfactual): leaked RIVET_ORACLE_JAR gate did not run the dwell row" >&2; exit 1; }
  grep -q "unexpected scenario invocation" "$SANDBOX/dwell-leak.log" && {
    echo "FAIL (dwell counterfactual): leaked RIVET_ORACLE_JAR woke the join/move rows" >&2; exit 1; }
  echo "ok (dwell counterfactual): a leaked RIVET_ORACLE_JAR stays neutralized"
}
