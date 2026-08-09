#!/bin/bash
# Sandbox stub for tools/rivet-client/run-scenario.sh, installed by the gate
# sandbox tests (scripts/test_gate_*.sh) so the full gate's unconditional dwell
# row (issue #160) can run without a real cargo build. It models the terminal M1
# dwell contract instead of passing vacuously:
#   - the invocation MUST be exactly `dwell --server rivet` (dwell is Rivet-only
#     per #160); anything else is gate drift and fails the sandbox;
#   - every accepted invocation is appended to $DWELL_STUB_LOG so a test can
#     assert the dwell row really ran — a gate that drops the row leaves the log
#     empty and fails scenario 0's exact-invocation check;
#   - a file at $DWELL_STUB_FAIL forces the row to fail, proving the gate turns
#     red when the M1 dwell verdict fails (never green because the step was
#     skipped).
# The join/move rows never reach this stub: gate.sh guards them behind a
# paperclip jar — RIVET_ORACLE_JAR, tools/rivet-client/work/jars/, or a
# paper-paperclip*.jar under working/Paper — and the sandbox neutralizes all
# three branches (the tests unset RIVET_ORACLE_JAR; the sandbox jar lives at
# tools/rivet-oracle/work/jars/ for oracle verify, not tools/rivet-client; the
# working/Paper lib is paper-server-*.jar), so the strict rejection of every
# other invocation is safe. Only the 0/1 verdicts are modeled — the real
# runner's UNVERIFIED (3) is treated like any nonzero exit by gate.sh's set -e
# and is unreachable with prereqs stubbed.
set -euo pipefail

if [ "$*" != "dwell --server rivet" ]; then
  echo "stub: unexpected scenario invocation: $* (expected: dwell --server rivet)" >&2
  exit 1
fi

if [ -n "${DWELL_STUB_FAIL:-}" ] && [ -f "$DWELL_STUB_FAIL" ]; then
  echo "stub: failing dwell scenario invocation (marker present)" >&2
  exit 1
fi

if [ -n "${DWELL_STUB_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$DWELL_STUB_LOG"
fi

exit 0
