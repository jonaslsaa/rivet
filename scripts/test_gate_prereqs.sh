#!/bin/bash
# Focused test for gate.sh's oracle prereq pre-check and parity-boot failure
# path (issues #47/#102). Sources scripts/gate.sh (which only defines functions
# when sourced) and drives oracle_prereq_check against a fake repo tree with a
# shimmed PATH, so every prerequisite is controllable: absent prereqs -> both
# steps not runnable; --require-oracle -> hard failure (exit 1); all prereqs
# present -> both steps runnable. Also drives run_rivet_parity with a shimmed
# `cargo` to assert a dead oracle (stale-jar boot failure) is never reported
# VERIFIED. The shimmed PATH is applied only inside run_check()/the parity
# tests, so the test's own tools keep working.
#
#   ./scripts/test_gate_prereqs.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

SHIM="$(mktemp -d)"
TMP="$(mktemp -d)"
CARGO_SHIM="$(mktemp -d)"
trap 'rm -rf "$SHIM" "$TMP" "$CARGO_SHIM"' EXIT

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# --- source gate.sh with the real repo only to load its functions -------------
# shellcheck source=scripts/gate.sh
# shellcheck disable=SC1091  # sources a sibling script; shellcheck only follows it with -x
source "$SCRIPT_DIR/gate.sh"
if [ -z "${REQUIRE_ORACLE:-}" ] && [ -z "${VERIFY_RUNNABLE:-}" ]; then
  fail "gate.sh did not define its globals (was it executed instead of sourced?)"
fi
pass "sourcing gate.sh defines functions without running the gate"

# --- shim executables ----------------------------------------------------------
add_stub() { # $1=name, $2=stdout line
  printf '#!/bin/bash\nprintf "%%s\\n" %q\n' "$2" > "$SHIM/$1"
  chmod +x "$SHIM/$1"
}
# df stub: df -Pk output shape; ignores args.
printf '#!/bin/bash\nprintf "%%s\\n" "Filesystem 1024-blocks Used Available Capacity Mounted on"\nprintf "%%s\\n" "/shim 1000000 100000 900000 51%% /shim"\n' > "$SHIM/df"
chmod +x "$SHIM/df"
# awk stub: consume stdin fully (so df doesn't SIGPIPE) using only bash builtins
# (cat is not on the shimmed PATH), then pull fields 4 / 6 off the second line
# for the two invocations gate.sh makes.
printf '#!/bin/bash\nwhile IFS= read -r _; do :; done\nif [[ "$*" == *"NR==2 {print \\$4}"* ]]; then\n  printf "%%s\\n" 900000\nelif [[ "$*" == *"NR==2 {print \\$6}"* ]]; then\n  printf "%%s\\n" "/shim"\nfi\n' > "$SHIM/awk"
chmod +x "$SHIM/awk"
# basename stub.
printf '#!/bin/bash\nprintf "%%s\\n" "${1##*/}"\n' > "$SHIM/basename"
chmod +x "$SHIM/basename"
# uname stub: not Darwin, so the macOS java_home path is skipped.
printf '#!/bin/bash\nprintf "%%s\\n" "Linux"\n' > "$SHIM/uname"
chmod +x "$SHIM/uname"

# --- fake repo trees ------------------------------------------------------------
# FAKE_EMPTY: a repo-shaped tree with no jars and no materialized runtime.
FAKE_EMPTY="$TMP/empty"
mkdir -p "$FAKE_EMPTY/tools/rivet-oracle" "$FAKE_EMPTY/working/Paper/paper-server/build/libs"

# FAKE_FULL: a repo-shaped tree with every artifact present.
FAKE_FULL="$TMP/full"
mkdir -p "$FAKE_FULL/tools/rivet-oracle/work/jars"
mkdir -p "$FAKE_FULL/working/Paper/paper-server/build/libs"
mkdir -p "$FAKE_FULL/tools/rivet-oracle/work/run/libraries"
mkdir -p "$FAKE_FULL/tools/rivet-oracle/work/run/versions/26.2"
mkdir -p "$FAKE_FULL/target/debug"
touch "$FAKE_FULL/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar"
touch "$FAKE_FULL/working/Paper/paper-server/build/libs/paper-server-26.2.local-SNAPSHOT.jar"
touch "$FAKE_FULL/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
touch "$FAKE_FULL/target/debug/rivet-client"

# Neutralise host JDK discovery via absolute paths (SDKMAN / JAVA_HOME) so the
# test controls the Java 25 JDK check entirely through the shimmed PATH.
unset RIVET_ORACLE_JAR RIVET_PAPER_JAR RIVET_PAPER_LIBRARIES RIVET_PAPER_RUNTIME_JAR \
      RIVET_CLIENT_BIN RIVET_JAVA_HOME JAVA_HOME SDKMAN_CANDIDATES_DIR

# run_check: run oracle_prereq_check in the CURRENT shell (so its global outputs
# VERIFY_RUNNABLE/PARITY_RUNNABLE are visible after it returns) with a shimmed
# PATH, restoring the real PATH immediately after. Returns the oracle's exit
# code. $1 = repo dir.
run_check() {
  local repo="$1" saved_path="$PATH" saved_target="${CARGO_TARGET_DIR-}" rc=0
  PATH="$SHIM"
  CARGO_TARGET_DIR="$repo/target"
  REPO_DIR="$repo"
  # set +e: REQUIRE_ORACLE=1 exits 1; we must not abort on that.
  set +e
  oracle_prereq_check
  rc=$?
  set -e
  PATH="$saved_path"
  if [ -n "$saved_target" ]; then
    CARGO_TARGET_DIR="$saved_target"
  else
    unset CARGO_TARGET_DIR
  fi
  return "$rc"
}

# --- test 1: prereqs absent ------------------------------------------------------
run_check "$FAKE_EMPTY" > "$TMP/out1" 2>&1
[ "$VERIFY_RUNNABLE" = 0 ] || fail "verify runnable with no prereqs"
[ "$PARITY_RUNNABLE" = 0 ] || fail "parity runnable with no prereqs"
[ "$SCENARIO_RUNNABLE" = 0 ] || fail "scenario runnable with no prereqs"
# In this scenario java/python3/javac are not on PATH, the fake repo has no jars
# and no materialized runtime, but df IS present (so free disk should be [ok]).
missing_marker() { # $1 = item name
  # -F: item names contain regex metachars (e.g. "java 25+ on PATH").
  grep -F "$1" "$TMP/out1" | grep -q "MISSING" || fail "item '$1' not marked MISSING"
}
# Note: `Java 25 JDK` is NOT asserted missing here — gate.sh's SDKMAN-fallback
# discovery may legitimately resolve a real JDK 25 at $HOME/.sdkman/candidates
# even when the shimmed PATH has none. What matters for the gate contract is
# that PARITY_RUNNABLE stays 0 without a compile jar / runtime (asserted above).
for name in "java 25+ on PATH" "python3" "paperclip jar" \
            "Paper compile jar" "materialized runtime libraries" \
            "materialized runtime jar"; do
  missing_marker "$name"
done
grep -E "^  \[ok\].*free disk" "$TMP/out1" >/dev/null || fail "free disk should be [ok] (df present)"
pass "absent prereqs: genuinely-missing items marked MISSING, neither step runnable"

# --- test 2: --require-oracle hard-fails on missing prereqs -----------------------
REQUIRE_ORACLE=1
rc=0
run_check "$FAKE_EMPTY" > "$TMP/out2" 2>&1 || rc=$?
REQUIRE_ORACLE=0
[ "$rc" = 1 ] || fail "--require-oracle should exit 1 on missing prereqs (got $rc)"
grep -q "gate stops here" "$TMP/out2" || fail "require-oracle message missing"
pass "--require-oracle with missing prereqs exits 1"

# --- test 3: all prereqs present ---------------------------------------------------
add_stub java 'openjdk version "25.0.2" 2026-01-20 LTS'
add_stub javac 'javac 25.0.2'
add_stub python3 'Python 3.14.6'
run_check "$FAKE_FULL" > "$TMP/out3" 2>&1
[ "$VERIFY_RUNNABLE" = 1 ] || fail "verify not runnable with all prereqs present"
[ "$PARITY_RUNNABLE" = 1 ] || fail "parity not runnable with all prereqs present"
[ "$SCENARIO_RUNNABLE" = 1 ] || { cat "$TMP/out3"; fail "scenario not runnable with all prereqs present"; }
[ "$RIVET_CLIENT_BIN" = "$FAKE_FULL/target/debug/rivet-client" ] \
  || fail "resolved client binary was not exported to downstream capture/scenario tools"
grep -q "all oracle prerequisites present" "$TMP/out3" || fail "present-report missing"
grep -q "MISSING" "$TMP/out3" && fail "MISSING reported despite all prereqs present"
pass "all prereqs present: all steps runnable, no MISSING reported"

# --- test 4: run_rivet_parity must not report VERIFIED when the oracle cannot boot --
# run_rivet_parity invokes `cargo run -q -p rivet-parity -- --require-oracle`; shim only
# `cargo` (mktemp/cat/grep stay real via the prepended PATH). The shims emit the tool's
# machine-stable exit codes: 0 = oracle ran clean (VERIFIED), 1 = oracle ran but parity
# diverged (FAILED), 3 = oracle could not boot (UNVERIFIED), 101 = tool crash (FAILED).
dead_cargo() {
  printf '#!/bin/bash\nprintf "%%s\\n" "   [rivet-parity] ORACLE BLOCKER: oracle failed to boot (compile jar SHA mismatch)" >&2\nexit 3\n' > "$CARGO_SHIM/cargo"
  chmod +x "$CARGO_SHIM/cargo"
}
healthy_cargo() {
  printf '#!/bin/bash\nprintf "%%s\\n" "[rivet-parity] oracle: Paper 26.2 (abc123) sha256 0123456789ab" >&2\nprintf "%%s\\n" "  STATUS: VERIFIED (all oracle checks ran)" >&2\nexit 0\n' > "$CARGO_SHIM/cargo"
  chmod +x "$CARGO_SHIM/cargo"
}
crash_cargo() {
  printf '#!/bin/bash\nprintf "%%s\\n" "[rivet-parity] panicked at main.rs:123" >&2\nexit 101\n' > "$CARGO_SHIM/cargo"
  chmod +x "$CARGO_SHIM/cargo"
}

# Dead oracle, gate not in --require-oracle mode: must set ORACLE_UNVERIFIED,
# return 0 (main decides the exit), and never print VERIFIED.
dead_cargo
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0
saved_path="$PATH"
PATH="$CARGO_SHIM:$PATH"
set +e
run_rivet_parity > "$TMP/out4" 2>&1
rc4=$?
set -e
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "dead oracle: ORACLE_UNVERIFIED not set"
[ "$rc4" = 0 ] || fail "dead oracle without --require-oracle should not exit nonzero (got $rc4)"
grep -q "^    VERIFIED" "$TMP/out4" && fail "dead oracle: VERIFIED printed despite boot failure"
grep -q "^    UNVERIFIED" "$TMP/out4" || fail "dead oracle: no UNVERIFIED report"
pass "dead oracle: reports UNVERIFIED, never VERIFIED"

# Dead oracle with --require-oracle: hard failure (exit 1). run_rivet_parity
# exits the shell here, so it must run in a subshell.
dead_cargo
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=1
PATH="$CARGO_SHIM:$PATH"
set +e
( run_rivet_parity > "$TMP/out5" 2>&1 )
rc5=$?
set -e
PATH="$saved_path"
REQUIRE_ORACLE=0
[ "$rc5" = 1 ] || fail "dead oracle + --require-oracle should exit 1 (got $rc5)"
grep -q "hard failure" "$TMP/out5" || fail "--require-oracle hard-failure message missing"
pass "dead oracle + --require-oracle exits 1"

# Healthy oracle: VERIFIED, ORACLE_UNVERIFIED stays 0.
healthy_cargo
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0
PATH="$CARGO_SHIM:$PATH"
run_rivet_parity > "$TMP/out6" 2>&1
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "healthy oracle: ORACLE_UNVERIFIED should stay 0"
grep -q "^    VERIFIED" "$TMP/out6" || fail "healthy oracle: VERIFIED not printed"
pass "healthy oracle: VERIFIED, stays verified"

# Hard mismatches (oracle booted, parity genuinely diverged): must exit 1 and be
# reported FAILED — a real parity divergence is a hard gate failure regardless of
# --require-oracle, and must never be mislabeled UNVERIFIED (that would suggest an
# infrastructure problem). run_rivet_parity exits the shell in this branch, so it
# runs in a subshell.
hard_mismatch_cargo() {
  printf '#!/bin/bash\nprintf "%%s\\n" "[rivet-parity] oracle: Paper 26.2 (abc123) sha256 0123456789ab" >&2\nprintf "%%s\\n" "  HARD MISMATCHES (1):" >&2\nprintf "%%s\\n" "    - parse.legacy-colon-keys" >&2\nexit 1\n' > "$CARGO_SHIM/cargo"
  chmod +x "$CARGO_SHIM/cargo"
}
hard_mismatch_cargo
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0
PATH="$CARGO_SHIM:$PATH"
set +e
( run_rivet_parity > "$TMP/out7" 2>&1 )
rc7=$?
set -e
PATH="$saved_path"
[ "$rc7" = 1 ] || fail "hard mismatch should exit 1 (got $rc7)"
grep -q "^    FAILED" "$TMP/out7" || fail "hard mismatch: FAILED not printed"
grep -q "^    UNVERIFIED" "$TMP/out7" && fail "hard mismatch: UNVERIFIED printed for a real parity divergence"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "hard mismatch: ORACLE_UNVERIFIED should stay 0"
pass "hard mismatch: FAILED, exit 1, never UNVERIFIED"

# Tool crash (panic/error, exit 101): must exit 1 and be reported FAILED, never green.
crash_cargo
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0
PATH="$CARGO_SHIM:$PATH"
set +e
( run_rivet_parity > "$TMP/out8" 2>&1 )
rc8=$?
set -e
PATH="$saved_path"
[ "$rc8" = 1 ] || fail "tool crash should exit 1 (got $rc8)"
grep -q "^    FAILED" "$TMP/out8" || fail "tool crash: FAILED not printed"
grep -q "^    VERIFIED" "$TMP/out8" && fail "tool crash: VERIFIED printed despite crash"
pass "tool crash: FAILED, exit 1, never VERIFIED"

# --- test 5: run_oracle_self_test must prove the raw-JSON-Lines contract -------
# The self-test step runs `tools/rivet-reference-oracle/run.sh --self-test` and
# asserts stdout is exactly one bare JSON line (no log4j `[HH:mm:ss LEVEL]:
# [STDOUT]: ...` prefix, and not swallowed). This is the load-bearing test for
# RivetReferenceOracle.selfTest()'s RAW_STDOUT emission: a regression back to
# System.out.println (re-wired through SysOutOverSLF4J by Bootstrap.bootStrap())
# produces a prefixed line or empty stdout and must FAIL the gate. The verdict
# is a *structural* JSON parse (top-level "ok" must be the boolean true): a
# nested `{"ok":true}` (top-level ok absent) and a top-level `"ok":false` are
# both counterfeits and must FAIL. The step resolves run.sh via REPO_DIR, so
# only that file is shimmed inside FAKE_FULL; PARITY_RUNNABLE is set directly,
# as in the parity tests above.
SELF_SHIM_DIR="$FAKE_FULL/tools/rivet-reference-oracle"
mkdir -p "$SELF_SHIM_DIR"
self_run_sh() { # $1 = literal stdout line the fake run.sh should emit
  printf '#!/bin/bash\nprintf "%%s\\n" %q\n' "$1" > "$SELF_SHIM_DIR/run.sh"
  chmod +x "$SELF_SHIM_DIR/run.sh"
}

# Healthy self-test: stdout is the bare JSON summary.
self_run_sh '{"ok":true,"protocol":1,"tests":9}'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
run_oracle_self_test > "$TMP/out9" 2>&1
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "self-test: ORACLE_UNVERIFIED set on success"
grep -q "^    VERIFIED" "$TMP/out9" || fail "self-test: VERIFIED not printed for bare JSON line"
pass "self-test: bare JSON line -> VERIFIED"

# Regression: the summary routed through System.out after Bootstrap.bootStrap()
# vanishes entirely (stdout empty) — must be FAILED, exit 1.
self_run_sh ''
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out10" 2>&1 )
rc10=$?
set -e
[ "$rc10" = 1 ] || fail "self-test: empty stdout should exit 1 (got $rc10)"
grep -q "^    FAILED" "$TMP/out10" || fail "self-test: FAILED not printed for empty stdout"
grep -q "^    VERIFIED" "$TMP/out10" && fail "self-test: VERIFIED printed despite empty stdout"
pass "self-test: empty stdout (System.out regression) -> FAILED"

# Regression: a log4j-prefixed line must be FAILED, exit 1.
self_run_sh '[2026-08-08T14:00:00Z INFO]: [STDOUT]: {"ok":true,"protocol":1,"tests":9}'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out11" 2>&1 )
rc11=$?
set -e
[ "$rc11" = 1 ] || fail "self-test: log-prefixed line should exit 1 (got $rc11)"
grep -q "^    FAILED" "$TMP/out11" || fail "self-test: FAILED not printed for log-prefixed line"
pass "self-test: log4j-prefixed line -> FAILED"

# Multi-line stdout (a trailing diagnostic) must be FAILED, exit 1.
self_run_sh $'{"ok":true,"protocol":1,"tests":9}\njunk'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out12" 2>&1 )
rc12=$?
set -e
[ "$rc12" = 1 ] || fail "self-test: multi-line stdout should exit 1 (got $rc12)"
grep -q "^    FAILED" "$TMP/out12" || fail "self-test: FAILED not printed for multi-line stdout"
pass "self-test: multi-line stdout -> FAILED"

# Leading/trailing whitespace around the JSON object (a padded line, or a JVM
# that emitted a stray blank/indent) is not the bare JSON line the contract
# requires — json.loads would tolerate it, so the exact-shape check must reject.
self_run_sh '  {"ok":true,"protocol":1,"tests":9}'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out12a" 2>&1 )
rc12a=$?
set -e
[ "$rc12a" = 1 ] || fail "self-test: whitespace-padded stdout should exit 1 (got $rc12a)"
grep -q "^    FAILED" "$TMP/out12a" || fail "self-test: FAILED not printed for whitespace-padded stdout"
grep -q "^    VERIFIED" "$TMP/out12a" && fail "self-test: VERIFIED printed despite whitespace-padded stdout"
pass "self-test: whitespace-padded stdout -> FAILED"

# Counterfeit: a *nested* {"ok":true} with no top-level ok. The old substring
# glob accepted this ("...ok":true..." matches); the structural parse must fail.
self_run_sh '{"result":{"ok":true,"protocol":1,"tests":9}}'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out12b" 2>&1 )
rc12b=$?
set -e
[ "$rc12b" = 1 ] || fail "self-test: nested-only ok:true should exit 1 (got $rc12b)"
grep -q "^    FAILED" "$TMP/out12b" || fail "self-test: FAILED not printed for nested-only ok:true"
pass "self-test: nested-only ok:true -> FAILED"

# Counterfeit: top-level ok present but false (or "true"/1, the non-boolean
# forms) — the verdict is not the boolean true and must FAIL.
self_run_sh '{"ok":false,"protocol":1,"tests":9}'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out12c" 2>&1 )
rc12c=$?
set -e
[ "$rc12c" = 1 ] || fail "self-test: top-level ok:false should exit 1 (got $rc12c)"
grep -q "^    FAILED" "$TMP/out12c" || fail "self-test: FAILED not printed for top-level ok:false"
pass "self-test: top-level ok:false -> FAILED"

# Counterfeit: a trailing blank line after the JSON (`{"ok":true}\n\n`). The
# old `$()` capture stripped trailing newlines, so the summary "passed"; the
# protocol is exactly one bare JSON line, so a trailing blank line is a broken
# raw-stdout shape and must FAIL (exit 1).
self_run_sh $'{"ok":true,"protocol":1,"tests":9}\n'
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out12d" 2>&1 )
rc12d=$?
set -e
[ "$rc12d" = 1 ] || fail "self-test: trailing blank line should exit 1 (got $rc12d)"
grep -q "^    FAILED" "$TMP/out12d" || fail "self-test: FAILED not printed for trailing blank line"
grep -q "^    VERIFIED" "$TMP/out12d" && fail "self-test: VERIFIED printed despite trailing blank line"
pass "self-test: trailing blank line -> FAILED"

# Not runnable: UNVERIFIED, ORACLE_UNVERIFIED=1, no hard failure.
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=0
run_oracle_self_test > "$TMP/out13" 2>&1
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "self-test: ORACLE_UNVERIFIED not set when not runnable"
grep -q "^    UNVERIFIED" "$TMP/out13" || fail "self-test: UNVERIFIED not printed when not runnable"
pass "self-test: not runnable -> UNVERIFIED"

# A run.sh that exits nonzero — whether the oracle failed to boot (a
# present-but-stale runtime jar fails its SHA/commit pin, a prereq is missing)
# or selfTest() threw a JVM assertion after booting (run.sh exec's the JVM, so
# its exit status passes straight through) — never exercised the RAW_STDOUT
# verdict in a way the wrapper can distinguish. The classification is therefore
# conservatively UNVERIFIED (gate exit 3 via ORACLE_UNVERIFIED), NEVER FAILED —
# the distinguishing property from the booted-but-bad-stdout cases above, where a
# run.sh that exits 0 while emitting a non-bare-JSON line is a real self-test
# failure (FAILED, exit 1). Mirrors run_rivet_parity's dead-oracle (rc=3)
# handling.
dead_self_run_sh() { # $1 = exit code run.sh should return
  printf '#!/bin/bash\nprintf "%%s\\n" "Paper compile jar and materialized runtime jar do not match" >&2\nexit %s\n' "$1" > "$SELF_SHIM_DIR/run.sh"
  chmod +x "$SELF_SHIM_DIR/run.sh"
}

# Dead oracle, gate not in --require-oracle mode: UNVERIFIED, returns 0 (main
# turns ORACLE_UNVERIFIED into exit 3), never FAILED. Runs in the CURRENT shell
# (not a subshell) so the ORACLE_UNVERIFIED global propagates to the assertions.
dead_self_run_sh 1
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
run_oracle_self_test > "$TMP/out14" 2>&1
rc14=$?
set -e
[ "$rc14" = 0 ] || fail "self-test: dead oracle without --require-oracle should return 0 (got $rc14)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "self-test: ORACLE_UNVERIFIED not set on dead oracle"
grep -q "^    UNVERIFIED" "$TMP/out14" || fail "self-test: UNVERIFIED not printed for dead oracle"
grep -q "^    FAILED" "$TMP/out14" && fail "self-test: FAILED printed for nonzero run.sh exit (classification is conservatively UNVERIFIED, never FAILED)"
grep -q "^    VERIFIED" "$TMP/out14" && fail "self-test: VERIFIED printed despite dead oracle"
pass "self-test: dead oracle -> UNVERIFIED, never FAILED/VERIFIED"

# Dead oracle with --require-oracle: hard failure (exit 1), after reporting
# UNVERIFIED. run_oracle_self_test exits the shell here, so it runs in a
# subshell.
dead_self_run_sh 1
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_oracle_self_test > "$TMP/out15" 2>&1 )
rc15=$?
set -e
REQUIRE_ORACLE=0
[ "$rc15" = 1 ] || fail "self-test: dead oracle + --require-oracle should exit 1 (got $rc15)"
grep -q "^    UNVERIFIED" "$TMP/out15" || fail "self-test: UNVERIFIED not printed before the hard failure"
grep -q "hard failure" "$TMP/out15" || fail "self-test: --require-oracle hard-failure message missing"
pass "self-test: dead oracle + --require-oracle exits 1"

# A non-1 nonzero exit (run.sh aborts for an unanticipated reason, or the JVM
# exits with a nonstandard status) is still conservatively UNVERIFIED, not FAILED —
# classification keys on the raw-JSON verdict being unobservable, not on the
# specific exit code.
dead_self_run_sh 7
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
run_oracle_self_test > "$TMP/out16" 2>&1
rc16=$?
set -e
[ "$rc16" = 0 ] || fail "self-test: any nonzero run.sh exit should return 0 (got $rc16)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "self-test: ORACLE_UNVERIFIED not set for nonzero exit 7"
grep -q "^    UNVERIFIED" "$TMP/out16" || fail "self-test: UNVERIFIED not printed for exit 7"
grep -q "^    FAILED" "$TMP/out16" && fail "self-test: FAILED printed for exit 7 (classification is conservatively UNVERIFIED, never FAILED)"
pass "self-test: nonzero exit 7 -> UNVERIFIED, never FAILED"

# --- test 6: scenario Paper rows (join/move Paper-vs-Rivet) never silently skip -
# issue #160: pre-#160 the Paper rows printed a bare "SKIPPED (no paperclip jar)"
# when the jar was absent, so a merge with no Paper comparison could still look
# green. run_scenario_paper_rows now mirrors the oracle-step contract: with the
# prereqs present (SCENARIO_RUNNABLE=1) it runs all three Paper rows (exit 0
# PASS / 1 FAIL); with a prereq missing it reports UNVERIFIED and sets
# ORACLE_UNVERIFIED so main exits 3, and --require-oracle hard-fails at the
# prereq pre-check (exit 1). run_scenario_paper_rows resolves run-scenario.sh via
# REPO_DIR, so only that file is shimmed, as in the self-test block above.
SCENARIO_SHIM_DIR="$FAKE_FULL/tools/rivet-client"
mkdir -p "$SCENARIO_SHIM_DIR"
# Shim run-scenario.sh to record its argv (so we can assert the correct three
# Paper rows are invoked) and exit 0 on success; a marker path (baked in below)
# makes it fail for the FAIL classification test. The log/fail paths are baked
# directly into the heredoc because the shim runs as a child process that does
# not inherit the test's shell variables.
SCENARIO_LOG="$TMP/scenario-invocations.log"
FAIL_SCENARIO_MARKER="$TMP/fail-scenario"
: > "$SCENARIO_LOG"
cat > "$SCENARIO_SHIM_DIR/run-scenario.sh" <<SCEOF
#!/bin/bash
echo "\$@" >> "$SCENARIO_LOG"
if [ -f "$FAIL_SCENARIO_MARKER" ]; then
  exit 1
fi
exit 0
SCEOF
chmod +x "$SCENARIO_SHIM_DIR/run-scenario.sh"

# Runnable (SCENARIO_RUNNABLE=1) with all prereqs present: runs all three Paper
# rows, sets no UNVERIFIED, returns 0 (main's verdict stays green).
SCENARIO_RUNNABLE=1; ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$SCENARIO_LOG"
run_scenario_paper_rows > "$TMP/out_sc1" 2>&1
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "scenario: ORACLE_UNVERIFIED set on a runnable Paper-row run"
grep -q "join --server both" "$SCENARIO_LOG" || fail "scenario: join --server both not invoked when runnable"
grep -q "move --server both" "$SCENARIO_LOG" || fail "scenario: move --server both not invoked when runnable"
grep -q "join$" "$SCENARIO_LOG" || fail "scenario: Paper-vs-Paper join self-check not invoked when runnable"
[ "$(wc -l < "$SCENARIO_LOG" | tr -d ' ')" = 3 ] || fail "scenario: expected exactly 3 run-scenario invocations, got $(wc -l < "$SCENARIO_LOG")"
grep -q "UNVERIFIED" "$TMP/out_sc1" && fail "scenario: UNVERIFIED printed despite runnable Paper rows"
pass "scenario: runnable Paper rows invoke join + join both + move both, stay VERIFIED"

# Missing prereq (SCENARIO_RUNNABLE=0): must report UNVERIFIED, set
# ORACLE_UNVERIFIED, and return 0 (main turns it into exit 3) — never a bare
# skip. Must NOT invoke run-scenario.sh at all.
SCENARIO_RUNNABLE=0; ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$SCENARIO_LOG"
set +e
run_scenario_paper_rows > "$TMP/out_sc2" 2>&1
rc_sc2=$?
set -e
[ "$rc_sc2" = 0 ] || fail "scenario: missing prereq without --require-oracle should return 0 (got $rc_sc2)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "scenario: ORACLE_UNVERIFIED not set when Paper rows not runnable"
grep -q "UNVERIFIED" "$TMP/out_sc2" || fail "scenario: UNVERIFIED not printed when Paper rows not runnable"
grep -q "SKIPPED" "$TMP/out_sc2" && fail "scenario: old bare SKIPPED still printed — issue #160 guard missing"
[ -s "$SCENARIO_LOG" ] && fail "scenario: run-scenario.sh invoked despite missing prereq"
pass "scenario: missing prereq -> UNVERIFIED (not SKIPPED), no run-scenario invocation"

# A failing Paper row (run-scenario.sh exits nonzero) must be a hard FAIL (exit 1),
# never UNVERIFIED or green — the rows are a real differential, so a divergence is
# a genuine failure, not an infrastructure problem. run_scenario_paper_rows exits
# the shell in this branch, so it runs in a subshell (set -e aborts on the nonzero).
SCENARIO_RUNNABLE=1; ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$FAIL_SCENARIO_MARKER"
set +e
( run_scenario_paper_rows > "$TMP/out_sc3" 2>&1 )
rc_sc3=$?
set -e
[ "$rc_sc3" = 1 ] || fail "scenario: a failing Paper row should exit 1 (got $rc_sc3)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "scenario: FAILED Paper row should not set ORACLE_UNVERIFIED"
grep -q "UNVERIFIED" "$TMP/out_sc3" && fail "scenario: UNVERIFIED printed for a FAILED Paper row (classification is FAIL, exit 1)"
pass "scenario: failing Paper row -> hard exit 1 (FAIL), never UNVERIFIED"

# --- test 7: loaded-world scenario row (issue #374) never silently skips --------
# run_scenario_loaded_world always invokes `run-scenario.sh loaded-world` — the
# Rivet-only row needs no paperclip jar — and classifies its machine-stable exit
# code: 0 PASS, 1 FAIL (hard exit 1, never UNVERIFIED), 3 UNVERIFIED (sets
# ORACLE_UNVERIFIED so the gate exits 3, or hard exit 1 under --require-oracle),
# and any other nonzero (crash) FAIL (exit 1, never green). The shim records its
# argv and exits with the code in $LW_EXIT_FILE.
LW_SCENARIO_LOG="$TMP/lw-invocations.log"
LW_EXIT_FILE="$TMP/lw-exit"
printf '%s\n' 0 > "$LW_EXIT_FILE"
set_lw_exit() { printf '%s\n' "$1" > "$LW_EXIT_FILE"; }
# The existing test-6 shim is replaced here (test 6 already ran); the argv log
# is separate so the loaded-world assertions stay isolated.
cat > "$SCENARIO_SHIM_DIR/run-scenario.sh" <<LWEOF
#!/bin/bash
echo "\$@" >> '$LW_SCENARIO_LOG'
exit "\$(cat '$LW_EXIT_FILE')"
LWEOF
chmod +x "$SCENARIO_SHIM_DIR/run-scenario.sh"

# PASS (exit 0): the row reports PASS, sets no UNVERIFIED, and the wrapper is
# invoked with exactly `loaded-world`.
set_lw_exit 0
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$LW_SCENARIO_LOG"
run_scenario_loaded_world > "$TMP/out_lw1" 2>&1
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "loaded-world: ORACLE_UNVERIFIED set on a PASS"
grep -q "^    PASS" "$TMP/out_lw1" || fail "loaded-world: PASS not printed for exit 0"
[ "$(wc -l < "$LW_SCENARIO_LOG" | tr -d ' ')" = 1 ] || fail "loaded-world: expected exactly 1 run-scenario invocation, got $(wc -l < "$LW_SCENARIO_LOG")"
grep -qx "loaded-world" "$LW_SCENARIO_LOG" || fail "loaded-world: run-scenario.sh not invoked with exactly 'loaded-world' (got $(cat "$LW_SCENARIO_LOG"))"
pass "loaded-world: exit 0 -> PASS, stays verified, invoked with exactly 'loaded-world'"

# FAIL (exit 1): hard exit 1, reported FAILED, never UNVERIFIED, no
# ORACLE_UNVERIFIED. run_scenario_loaded_world exits the shell in this branch,
# so it runs in a subshell.
set_lw_exit 1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_loaded_world > "$TMP/out_lw2" 2>&1 )
rc_lw2=$?
set -e
[ "$rc_lw2" = 1 ] || fail "loaded-world: FAIL should exit 1 (got $rc_lw2)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "loaded-world: FAIL should not set ORACLE_UNVERIFIED"
grep -q "^    FAILED" "$TMP/out_lw2" || fail "loaded-world: FAILED not printed for exit 1"
grep -q "^    UNVERIFIED" "$TMP/out_lw2" && fail "loaded-world: UNVERIFIED printed for a FAIL (exit 1)"
grep -q "^    PASS" "$TMP/out_lw2" && fail "loaded-world: PASS printed for a FAIL"
pass "loaded-world: exit 1 -> FAILED, hard exit 1, never UNVERIFIED"

# UNVERIFIED (exit 3) without --require-oracle: sets ORACLE_UNVERIFIED, returns
# 0 (main turns it into gate exit 3).
set_lw_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
run_scenario_loaded_world > "$TMP/out_lw3" 2>&1
rc_lw3=$?
set -e
[ "$rc_lw3" = 0 ] || fail "loaded-world: UNVERIFIED without --require-oracle should return 0 (got $rc_lw3)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "loaded-world: ORACLE_UNVERIFIED not set on exit 3"
grep -q "^    UNVERIFIED" "$TMP/out_lw3" || fail "loaded-world: UNVERIFIED not printed for exit 3"
grep -q "^    PASS" "$TMP/out_lw3" && fail "loaded-world: PASS printed for an UNVERIFIED run"
pass "loaded-world: exit 3 -> UNVERIFIED, ORACLE_UNVERIFIED set"

# UNVERIFIED (exit 3) with --require-oracle: hard failure (exit 1), after
# reporting UNVERIFIED. run_scenario_loaded_world exits the shell here.
set_lw_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_loaded_world > "$TMP/out_lw4" 2>&1 )
rc_lw4=$?
set -e
REQUIRE_ORACLE=0
[ "$rc_lw4" = 1 ] || fail "loaded-world: UNVERIFIED + --require-oracle should exit 1 (got $rc_lw4)"
grep -q "^    UNVERIFIED" "$TMP/out_lw4" || fail "loaded-world: UNVERIFIED not printed before the hard failure"
grep -q "hard failure" "$TMP/out_lw4" || fail "loaded-world: --require-oracle hard-failure message missing"
pass "loaded-world: exit 3 + --require-oracle -> hard exit 1"

# Crash / unexpected exit (101): FAIL (exit 1), reported FAILED, never green.
set_lw_exit 101
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_loaded_world > "$TMP/out_lw5" 2>&1 )
rc_lw5=$?
set -e
[ "$rc_lw5" = 1 ] || fail "loaded-world: crash should exit 1 (got $rc_lw5)"
grep -q "^    FAILED" "$TMP/out_lw5" || fail "loaded-world: FAILED not printed for exit 101"
grep -q "^    PASS" "$TMP/out_lw5" && fail "loaded-world: PASS printed despite a crash"
grep -q "^    UNVERIFIED" "$TMP/out_lw5" && fail "loaded-world: UNVERIFIED printed for a crash (classification is FAIL, exit 1)"
pass "loaded-world: exit 101 -> FAILED, hard exit 1, never UNVERIFIED"

# --- test 8: generated-world scenario row (seed-42 contract) milestone-gates ----
# run_scenario_generated_world is milestone-gated like the RIVET_HASH_DIR
# hash-diff: while real generated-world serving is absent (the rivet-server
# `--seed` option still serves the superflat M1 fixture; RIVET_GENERATED_WORLD
# unset) the row prints an explicit NOTICE and never invokes run-scenario.sh —
# it stays mergeable and must NOT set ORACLE_UNVERIFIED. Setting
# RIVET_GENERATED_WORLD=1 opts into the strict check, where the machine-stable
# exit code is classified exactly like loaded-world: 0 PASS, 1 FAIL (hard exit
# 1, never UNVERIFIED), 3 UNVERIFIED (sets ORACLE_UNVERIFIED, or hard exit 1
# under --require-oracle), and any other nonzero (crash) FAIL (exit 1, never
# green). The shim records its argv and exits with the code in $GW_EXIT_FILE.
GW_SCENARIO_LOG="$TMP/gw-invocations.log"
GW_EXIT_FILE="$TMP/gw-exit"
printf '%s\n' 0 > "$GW_EXIT_FILE"
set_gw_exit() { printf '%s\n' "$1" > "$GW_EXIT_FILE"; }
cat > "$SCENARIO_SHIM_DIR/run-scenario.sh" <<GWEOF
#!/bin/bash
echo "\$@" >> '$GW_SCENARIO_LOG'
exit "\$(cat '$GW_EXIT_FILE')"
GWEOF
chmod +x "$SCENARIO_SHIM_DIR/run-scenario.sh"

# Milestone NOTICE (RIVET_GENERATED_WORLD unset): the row must not invoke
# run-scenario.sh at all, must not set ORACLE_UNVERIFIED, and must report the
# exact UNVERIFIED nature as a NOTICE — the release lane stays mergeable ahead
# of the live generated-world serving path.
unset RIVET_GENERATED_WORLD
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$GW_SCENARIO_LOG"
set +e
run_scenario_generated_world > "$TMP/out_gw0" 2>&1
rc_gw0=$?
set -e
[ "$rc_gw0" = 0 ] || fail "generated-world: milestone NOTICE should return 0 (got $rc_gw0)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-world: NOTICE must not set ORACLE_UNVERIFIED"
grep -q "NOTICE" "$TMP/out_gw0" || fail "generated-world: NOTICE not printed when the capability is absent"
grep -q "RIVET_GENERATED_WORLD=1" "$TMP/out_gw0" || fail "generated-world: NOTICE must name the opt-in flag"
[ -s "$GW_SCENARIO_LOG" ] && fail "generated-world: run-scenario.sh must not be invoked on the NOTICE path (got $(cat "$GW_SCENARIO_LOG"))"
pass "generated-world: capability absent -> explicit NOTICE, no invocation, stays mergeable"

# Milestone NOTICE with RIVET_GENERATED_WORLD=0: an explicit "0" must keep the
# row off (the same '0 = off' convention as RIVET_REQUIRE_ORACLE), never
# silently enable it and then block the release lane with an UNVERIFIED exit.
RIVET_GENERATED_WORLD=0
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$GW_SCENARIO_LOG"
set +e
run_scenario_generated_world > "$TMP/out_gw0b" 2>&1
rc_gw0b=$?
set -e
[ "$rc_gw0b" = 0 ] || fail "generated-world: RIVET_GENERATED_WORLD=0 must keep the row off (got $rc_gw0b)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-world: RIVET_GENERATED_WORLD=0 must not set ORACLE_UNVERIFIED"
grep -q "NOTICE" "$TMP/out_gw0b" || fail "generated-world: RIVET_GENERATED_WORLD=0 must still print the NOTICE"
[ -s "$GW_SCENARIO_LOG" ] && fail "generated-world: RIVET_GENERATED_WORLD=0 must not invoke run-scenario.sh"
pass "generated-world: RIVET_GENERATED_WORLD=0 -> NOTICE (0 = off, never silent enable)"

# PASS (exit 0) with the strict flag: the row reports PASS, sets no UNVERIFIED,
# and the wrapper is invoked with exactly `generated-world`.
set_gw_exit 0
RIVET_GENERATED_WORLD=1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$GW_SCENARIO_LOG"
run_scenario_generated_world > "$TMP/out_gw1" 2>&1
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-world: ORACLE_UNVERIFIED set on a PASS"
grep -q "^    PASS" "$TMP/out_gw1" || fail "generated-world: PASS not printed for exit 0"
[ "$(wc -l < "$GW_SCENARIO_LOG" | tr -d ' ')" = 1 ] || fail "generated-world: expected exactly 1 run-scenario invocation, got $(wc -l < "$GW_SCENARIO_LOG")"
grep -qx "generated-world" "$GW_SCENARIO_LOG" || fail "generated-world: run-scenario.sh not invoked with exactly 'generated-world' (got $(cat "$GW_SCENARIO_LOG"))"
pass "generated-world: exit 0 -> PASS, stays verified, invoked with exactly 'generated-world'"

# FAIL (exit 1) with the strict flag: hard exit 1, reported FAILED, never
# UNVERIFIED, no ORACLE_UNVERIFIED. run_scenario_generated_world exits the
# shell in this branch, so it runs in a subshell.
set_gw_exit 1
RIVET_GENERATED_WORLD=1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_generated_world > "$TMP/out_gw2" 2>&1 )
rc_gw2=$?
set -e
[ "$rc_gw2" = 1 ] || fail "generated-world: FAIL should exit 1 (got $rc_gw2)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-world: FAIL should not set ORACLE_UNVERIFIED"
grep -q "^    FAILED" "$TMP/out_gw2" || fail "generated-world: FAILED not printed for exit 1"
grep -q "^    UNVERIFIED" "$TMP/out_gw2" && fail "generated-world: UNVERIFIED printed for a FAIL (exit 1)"
grep -q "^    PASS" "$TMP/out_gw2" && fail "generated-world: PASS printed for a FAIL"
pass "generated-world: exit 1 -> FAILED, hard exit 1, never UNVERIFIED"

# UNVERIFIED (exit 3) with the strict flag and no --require-oracle: sets
# ORACLE_UNVERIFIED, returns 0 (main turns it into gate exit 3).
set_gw_exit 3
RIVET_GENERATED_WORLD=1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
run_scenario_generated_world > "$TMP/out_gw3" 2>&1
rc_gw3=$?
set -e
[ "$rc_gw3" = 0 ] || fail "generated-world: UNVERIFIED without --require-oracle should return 0 (got $rc_gw3)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "generated-world: ORACLE_UNVERIFIED not set on exit 3"
grep -q "^    UNVERIFIED" "$TMP/out_gw3" || fail "generated-world: UNVERIFIED not printed for exit 3"
grep -q "^    PASS" "$TMP/out_gw3" && fail "generated-world: PASS printed for an UNVERIFIED run"
pass "generated-world: exit 3 -> UNVERIFIED, ORACLE_UNVERIFIED set"

# UNVERIFIED (exit 3) with the strict flag and --require-oracle: hard failure
# (exit 1), after reporting UNVERIFIED. run_scenario_generated_world exits the
# shell here.
set_gw_exit 3
RIVET_GENERATED_WORLD=1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_generated_world > "$TMP/out_gw4" 2>&1 )
rc_gw4=$?
set -e
REQUIRE_ORACLE=0
[ "$rc_gw4" = 1 ] || fail "generated-world: UNVERIFIED + --require-oracle should exit 1 (got $rc_gw4)"
grep -q "^    UNVERIFIED" "$TMP/out_gw4" || fail "generated-world: UNVERIFIED not printed before the hard failure"
grep -q "hard failure" "$TMP/out_gw4" || fail "generated-world: --require-oracle hard-failure message missing"
pass "generated-world: exit 3 + --require-oracle -> hard exit 1"

# Crash / unexpected exit (101) with the strict flag: FAIL (exit 1), reported
# FAILED, never green.
set_gw_exit 101
RIVET_GENERATED_WORLD=1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_generated_world > "$TMP/out_gw5" 2>&1 )
rc_gw5=$?
set -e
[ "$rc_gw5" = 1 ] || fail "generated-world: crash should exit 1 (got $rc_gw5)"
grep -q "^    FAILED" "$TMP/out_gw5" || fail "generated-world: FAILED not printed for exit 101"
grep -q "^    PASS" "$TMP/out_gw5" && fail "generated-world: PASS printed despite a crash"
grep -q "^    UNVERIFIED" "$TMP/out_gw5" && fail "generated-world: UNVERIFIED printed for a crash (classification is FAIL, exit 1)"
pass "generated-world: exit 101 -> FAILED, hard exit 1, never UNVERIFIED"

# --- test 9: recenter scenario row (issues #185/#561) never silently skips ------
# run_scenario_recenter always invokes `run-scenario.sh recenter` — the
# Rivet-only row needs no paperclip jar — and classifies its machine-stable exit
# code exactly like run_scenario_loaded_world: 0 PASS, 1 FAIL (hard exit 1,
# never UNVERIFIED), 3 UNVERIFIED (sets ORACLE_UNVERIFIED so the gate exits 3,
# or hard exit 1 under --require-oracle), and any other nonzero (crash) FAIL
# (exit 1, never green). The shim records its argv and exits with the code in
# $RC_EXIT_FILE.
RC_SCENARIO_LOG="$TMP/rc-invocations.log"
RC_EXIT_FILE="$TMP/rc-exit"
printf '%s\n' 0 > "$RC_EXIT_FILE"
set_rc_exit() { printf '%s\n' "$1" > "$RC_EXIT_FILE"; }
# The existing shim is replaced here (the loaded-world test already ran); the
# argv log is separate so the recenter assertions stay isolated.
cat > "$SCENARIO_SHIM_DIR/run-scenario.sh" <<RCEOF
#!/bin/bash
echo "\$@" >> '$RC_SCENARIO_LOG'
exit "\$(cat '$RC_EXIT_FILE')"
RCEOF
chmod +x "$SCENARIO_SHIM_DIR/run-scenario.sh"

# PASS (exit 0): the row reports PASS, sets no UNVERIFIED, and the wrapper is
# invoked with exactly `recenter`.
set_rc_exit 0
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
: > "$RC_SCENARIO_LOG"
run_scenario_recenter > "$TMP/out_rc1" 2>&1
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "recenter: ORACLE_UNVERIFIED set on a PASS"
grep -q "^    PASS" "$TMP/out_rc1" || fail "recenter: PASS not printed for exit 0"
[ "$(wc -l < "$RC_SCENARIO_LOG" | tr -d ' ')" = 1 ] || fail "recenter: expected exactly 1 run-scenario invocation, got $(wc -l < "$RC_SCENARIO_LOG")"
grep -qx "recenter" "$RC_SCENARIO_LOG" || fail "recenter: run-scenario.sh not invoked with exactly 'recenter' (got $(cat "$RC_SCENARIO_LOG"))"
pass "recenter: exit 0 -> PASS, stays verified, invoked with exactly 'recenter'"

# FAIL (exit 1): hard exit 1, reported FAILED, never UNVERIFIED, no
# ORACLE_UNVERIFIED. run_scenario_recenter exits the shell in this branch, so it
# runs in a subshell.
set_rc_exit 1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_recenter > "$TMP/out_rc2" 2>&1 )
rc_rc2=$?
set -e
[ "$rc_rc2" = 1 ] || fail "recenter: FAIL should exit 1 (got $rc_rc2)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "recenter: FAIL should not set ORACLE_UNVERIFIED"
grep -q "^    FAILED" "$TMP/out_rc2" || fail "recenter: FAILED not printed for exit 1"
grep -q "^    UNVERIFIED" "$TMP/out_rc2" && fail "recenter: UNVERIFIED printed for a FAIL (exit 1)"
grep -q "^    PASS" "$TMP/out_rc2" && fail "recenter: PASS printed for a FAIL"
pass "recenter: exit 1 -> FAILED, hard exit 1, never UNVERIFIED"

# UNVERIFIED (exit 3) without --require-oracle: sets ORACLE_UNVERIFIED, returns
# 0 (main turns it into gate exit 3).
set_rc_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
run_scenario_recenter > "$TMP/out_rc3" 2>&1
rc_rc3=$?
set -e
[ "$rc_rc3" = 0 ] || fail "recenter: UNVERIFIED without --require-oracle should return 0 (got $rc_rc3)"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "recenter: ORACLE_UNVERIFIED not set on exit 3"
grep -q "^    UNVERIFIED" "$TMP/out_rc3" || fail "recenter: UNVERIFIED not printed for exit 3"
grep -q "^    PASS" "$TMP/out_rc3" && fail "recenter: PASS printed for an UNVERIFIED run"
pass "recenter: exit 3 -> UNVERIFIED, ORACLE_UNVERIFIED set"

# UNVERIFIED (exit 3) with --require-oracle: hard failure (exit 1), after
# reporting UNVERIFIED. run_scenario_recenter exits the shell here.
set_rc_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=1; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_recenter > "$TMP/out_rc4" 2>&1 )
rc_rc4=$?
set -e
REQUIRE_ORACLE=0
[ "$rc_rc4" = 1 ] || fail "recenter: UNVERIFIED + --require-oracle should exit 1 (got $rc_rc4)"
grep -q "^    UNVERIFIED" "$TMP/out_rc4" || fail "recenter: UNVERIFIED not printed before the hard failure"
grep -q "hard failure" "$TMP/out_rc4" || fail "recenter: --require-oracle hard-failure message missing"
pass "recenter: exit 3 + --require-oracle -> hard exit 1"

# Crash / unexpected exit (101): FAIL (exit 1), reported FAILED, never green.
set_rc_exit 101
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0; REPO_DIR="$FAKE_FULL"
set +e
( run_scenario_recenter > "$TMP/out_rc5" 2>&1 )
rc_rc5=$?
set -e
[ "$rc_rc5" = 1 ] || fail "recenter: crash should exit 1 (got $rc_rc5)"
grep -q "^    FAILED" "$TMP/out_rc5" || fail "recenter: FAILED not printed for exit 101"
grep -q "^    PASS" "$TMP/out_rc5" && fail "recenter: PASS printed despite a crash"
grep -q "^    UNVERIFIED" "$TMP/out_rc5" && fail "recenter: UNVERIFIED printed for a crash (classification is FAIL, exit 1)"
pass "recenter: exit 101 -> FAILED, hard exit 1, never UNVERIFIED"

# --- test 10: generated-full oracle row keeps the dedicated 0/1/3 contract --
# The row is activated only by RIVET_GENERATED_FULL=1: a complete four-seed
# corpus runs the verifier and its tamper controls, absent genuine output is
# UNVERIFIED (3), malformed/divergent output is FAIL (1), and --require-oracle
# promotes exit 3 to a hard exit 1. The cargo shim records exact argv.
GEN_FULL_LOG="$TMP/generated-full-invocations.log"
GEN_FULL_EXIT_FILE="$TMP/generated-full-exit"
# The generated FULL row is an explicit promotion activation, not an ordinary
# pre-G4 prerequisite.  The inactive path must be an explicit NOTICE and must
# not invoke Cargo or set the global UNVERIFIED verdict.
unset RIVET_GENERATED_FULL
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0
: > "$GEN_FULL_LOG"
PATH="$CARGO_SHIM:$PATH"
run_oracle_generated_full > "$TMP/out_gf_pre_g4" 2>&1
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-full: pre-G4 NOTICE set ORACLE_UNVERIFIED"
grep -q "NOTICE" "$TMP/out_gf_pre_g4" || fail "generated-full: pre-G4 inactive path did not print NOTICE"
grep -q "pre-G4" "$TMP/out_gf_pre_g4" || fail "generated-full: pre-G4 NOTICE did not name the promotion gate"
[ ! -s "$GEN_FULL_LOG" ] || fail "generated-full: pre-G4 inactive path invoked Cargo"
pass "generated-full: pre-G4 inactive path is explicit NOTICE with no verifier invocation"

RIVET_GENERATED_FULL=""
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0
: > "$GEN_FULL_LOG"
PATH="$CARGO_SHIM:$PATH"
run_oracle_generated_full > "$TMP/out_gf_empty" 2>&1
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-full: empty env must stay pre-G4 inactive"
grep -q "NOTICE" "$TMP/out_gf_empty" || fail "generated-full: empty env did not print NOTICE"
[ ! -s "$GEN_FULL_LOG" ] || fail "generated-full: empty env invoked Cargo"
pass "generated-full: empty env is explicitly inactive"

RIVET_GENERATED_FULL=1
export RIVET_GENERATED_FULL
printf '%s\n' 0 > "$GEN_FULL_EXIT_FILE"
set_generated_full_exit() { printf '%s\n' "$1" > "$GEN_FULL_EXIT_FILE"; }
cat > "$CARGO_SHIM/cargo" <<GFEof
#!/bin/bash
echo "\$@" >> '$GEN_FULL_LOG'
exit "\$(cat '$GEN_FULL_EXIT_FILE')"
GFEof
chmod +x "$CARGO_SHIM/cargo"

set_generated_full_exit 0
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0
: > "$GEN_FULL_LOG"
PATH="$CARGO_SHIM:$PATH"
run_oracle_generated_full > "$TMP/out_gf0" 2>&1
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-full: PASS set ORACLE_UNVERIFIED"
grep -q "^    PASS" "$TMP/out_gf0" || fail "generated-full: PASS not printed for exit 0"
[ "$(wc -l < "$GEN_FULL_LOG" | tr -d ' ')" = 2 ] || fail "generated-full: PASS should run verifier plus tamper control"
grep -qx -- "run -q -p rivet-oracle -- verify-generated-full" "$GEN_FULL_LOG" || fail "generated-full: verifier argv drifted"
grep -qx -- "run -q -p rivet-oracle -- verify-generated-full --tamper all" "$GEN_FULL_LOG" || fail "generated-full: tamper-all argv drifted"
pass "generated-full: exit 0 -> PASS plus exact tamper-all argv"

set_generated_full_exit 1
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0
PATH="$CARGO_SHIM:$PATH"
set +e
( run_oracle_generated_full > "$TMP/out_gf1" 2>&1 )
rc_gf1=$?
set -e
PATH="$saved_path"
[ "$rc_gf1" = 1 ] || fail "generated-full: FAIL should exit 1 (got $rc_gf1)"
[ "$ORACLE_UNVERIFIED" = 0 ] || fail "generated-full: FAIL set ORACLE_UNVERIFIED"
grep -q "^    FAILED" "$TMP/out_gf1" || fail "generated-full: FAILED not printed for exit 1"
pass "generated-full: exit 1 -> FAILED, hard exit 1"

set_generated_full_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=0
PATH="$CARGO_SHIM:$PATH"
run_oracle_generated_full > "$TMP/out_gf3" 2>&1
PATH="$saved_path"
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "generated-full: exit 3 did not set ORACLE_UNVERIFIED"
grep -q "^    UNVERIFIED" "$TMP/out_gf3" || fail "generated-full: UNVERIFIED not printed for exit 3"
pass "generated-full: exit 3 -> UNVERIFIED, gate remains mergeable"

set_generated_full_exit 3
ORACLE_UNVERIFIED=0; REQUIRE_ORACLE=1
PATH="$CARGO_SHIM:$PATH"
set +e
( run_oracle_generated_full > "$TMP/out_gf4" 2>&1 )
rc_gf4=$?
set -e
PATH="$saved_path"
REQUIRE_ORACLE=0
[ "$rc_gf4" = 1 ] || fail "generated-full: exit 3 + --require-oracle should exit 1 (got $rc_gf4)"
grep -q "hard failure" "$TMP/out_gf4" || fail "generated-full: require-oracle hard-failure message missing"
pass "generated-full: exit 3 + --require-oracle -> hard exit 1"

# --- test 11: loaded-world + recenter + generated-world rows wired into the ----
# --- full-gate block ------------------------------------------------------------
# The classification tests above drive run_scenario_loaded_world,
# run_scenario_recenter, and run_scenario_generated_world directly, but a row
# could still silently drop off the gate if its invocation sat outside the
# `if [ "$FULL_GATE" = true ]` guard (or vanished from main()). Assert the
# source wires all three rows inside the guard. The scenario block is the
# FULL_GATE-guarded block whose first call is run_scenario_paper_rows, so anchor
# the extraction on that unique call (not the last FULL_GATE guard in the file):
# a new full-gate-guarded step appended later (e.g. before machete) must not
# move the extraction to the wrong block.
GATE_SOURCE="$SCRIPT_DIR/gate.sh"
# `|| true` on each assignment: under `set -euo pipefail`, a grep/awk with no
# match would abort the $(...) assignment (silent, no diagnostic) before the
# [ -n ... ] guard below can print its named failure. Let the assignment
# complete with an empty value so the guard fires the intended message.
PAPER_LINE="$(grep -n '^    run_scenario_paper_rows$' "$GATE_SOURCE" | cut -d: -f1 || true)"
[ -n "$PAPER_LINE" ] || fail "gate: '    run_scenario_paper_rows' not found in gate.sh (scenario block anchor missing)"
GATE_GUARD_LINE="$(grep -nF 'if [ "$FULL_GATE" = true ]; then' "$GATE_SOURCE" | awk -F: -v target="$PAPER_LINE" '$1 <= target { last = $1 } END { print last }' || true)"
[ -n "$GATE_GUARD_LINE" ] || fail "gate: no FULL_GATE guard at or before line $PAPER_LINE (scenario block not guarded)"
SCENARIO_BLOCK="$(sed -n "${GATE_GUARD_LINE},\$p" "$GATE_SOURCE" | awk '/^  fi$/ { print; exit } { print }')"
printf '%s\n' "$SCENARIO_BLOCK" | grep -q '^    run_scenario_loaded_world$' \
  || fail "gate: run_scenario_loaded_world not invoked in the full-gate scenario block"
printf '%s\n' "$SCENARIO_BLOCK" | grep -q '^    run_scenario_recenter$' \
  || fail "gate: run_scenario_recenter not invoked in the full-gate scenario block"
printf '%s\n' "$SCENARIO_BLOCK" | grep -q '^    run_scenario_generated_world$' \
  || fail "gate: run_scenario_generated_world not invoked in the full-gate scenario block"
pass "gate: loaded-world + recenter + generated-world rows are wired inside the full-gate scenario block"

echo
echo "ALL GATE PREREQ TESTS PASSED"
