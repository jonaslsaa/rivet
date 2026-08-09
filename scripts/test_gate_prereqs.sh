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
mkdir -p "$FAKE_FULL/tools/rivet-client/target/debug"
touch "$FAKE_FULL/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar"
touch "$FAKE_FULL/working/Paper/paper-server/build/libs/paper-server-26.2.local-SNAPSHOT.jar"
touch "$FAKE_FULL/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
touch "$FAKE_FULL/tools/rivet-client/target/debug/rivet-client"

# Neutralise host JDK discovery via absolute paths (SDKMAN / JAVA_HOME) so the
# test controls the Java 25 JDK check entirely through the shimmed PATH.
unset RIVET_ORACLE_JAR RIVET_PAPER_JAR RIVET_PAPER_LIBRARIES RIVET_PAPER_RUNTIME_JAR \
      RIVET_JAVA_HOME JAVA_HOME SDKMAN_CANDIDATES_DIR

# run_check: run oracle_prereq_check in the CURRENT shell (so its global outputs
# VERIFY_RUNNABLE/PARITY_RUNNABLE are visible after it returns) with a shimmed
# PATH, restoring the real PATH immediately after. Returns the oracle's exit
# code. $1 = repo dir.
run_check() {
  local repo="$1" saved_path="$PATH" rc=0
  PATH="$SHIM"
  REPO_DIR="$repo"
  # set +e: REQUIRE_ORACLE=1 exits 1; we must not abort on that.
  set +e
  oracle_prereq_check
  rc=$?
  set -e
  PATH="$saved_path"
  return "$rc"
}

# --- test 1: prereqs absent ------------------------------------------------------
run_check "$FAKE_EMPTY" > "$TMP/out1" 2>&1
[ "$VERIFY_RUNNABLE" = 0 ] || fail "verify runnable with no prereqs"
[ "$PARITY_RUNNABLE" = 0 ] || fail "parity runnable with no prereqs"
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
grep -q "all oracle prerequisites present" "$TMP/out3" || fail "present-report missing"
grep -q "MISSING" "$TMP/out3" && fail "MISSING reported despite all prereqs present"
pass "all prereqs present: both steps runnable, no MISSING reported"

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

echo
echo "ALL GATE PREREQ TESTS PASSED"
