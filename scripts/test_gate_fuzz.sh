#!/bin/bash
# Focused test for the rivet-fuzz `packets`-feature gate step. The five protocol
# packet-decode fuzz targets (fuzz/fuzz_targets/packet_*.rs) live behind the
# rivet-fuzz `packets` cargo feature (forwards to rivet-protocol/packets) via
# `required-features`, so `cargo fmt/clippy/test --workspace` never builds or
# lints them. gate.sh must type-check and lint them explicitly on every merge —
# this test proves that step exists, runs, and can never silently disappear or
# skip.
#
# Properties under test (each run in both the cargo-nextest and cargo-test
# profiles, matching how the sibling steps branch on nextest availability):
#   0. A green full gate invokes the fuzz `check` AND `clippy` commands, and
#      reaches GATE GREEN.
#   1. A failing fuzz `check` makes the full gate red (set -e aborts).
#   2. A failing fuzz `clippy` makes the full gate red.
#   3. A scoped gate for rivet-fuzz still runs the fuzz step (the packet targets
#      are part of that package, so a rivet-fuzz-scoped gate must cover them).
#   4. A scoped gate for an unrelated crate (crates/rivet-nbt) does NOT run the
#      fuzz step (the step is not a workspace-wide or crate-blind addition).
#   5. Every scenario guards the anti-pattern: the invocation is always
#      `-p rivet-fuzz --features packets` — `--all-features` or
#      `--workspace --features` (which would enable rivet-protocol's `packets`
#      feature workspace-wide, or fail on crates without the feature) never
#      appear in the log.
#
# Like test_gate_features.sh, this runs the real scripts/gate.sh against a
# sandboxed repo layout with stubs for cargo/cargo-machete/cargo-nextest/java
# under a redirected $HOME/.cargo/bin and a controlled PATH, so it is
# deterministic on any host and never depends on ambient PATH or an ambient
# toolchain. No real build, network fetch, or toolchain install happens; the
# sandbox is deleted on exit.
set -euo pipefail
cd "$(dirname "$0")/.."

SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

# --- build the sandbox -------------------------------------------------------
# Same layout the real gate.sh probes (REPO_DIR resolves to $SANDBOX).
mkdir -p "$SANDBOX/scripts" \
         "$SANDBOX/tools/rivet-codegen" \
         "$SANDBOX/home/.cargo/bin" \
         "$SANDBOX/jdk/bin" \
         "$SANDBOX/tools/rivet-oracle/work/jars" \
         "$SANDBOX/tools/rivet-oracle/work/run/libraries" \
         "$SANDBOX/tools/rivet-oracle/work/run/versions/26.2" \
         "$SANDBOX/tools/rivet-reference-oracle" \
         "$SANDBOX/working/Paper/paper-server/build/libs"

# Minimal workspace + codegen manifest so gate.sh's --manifest-path / path
# arguments resolve to existing files.
printf '[workspace]\n' > "$SANDBOX/Cargo.toml"
printf '[package]\nname = "rivet-codegen"\nversion = "0.1.0"\nedition = "2024"\n' \
  > "$SANDBOX/tools/rivet-codegen/Cargo.toml"

# The full gate runs the manifest regression suite and the marker audit via
# python3; stub them all to pass (their own behaviour is covered elsewhere; here
# they only need to not abort the gate on its pass path).
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_analyze_graph.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/check_markers.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_check_markers.py"

# The oracle pre-check requires the rivet-client binary (join-capture harness);
# provide an existing dummy file so the pre-check reports all prereqs present.
mkdir -p "$SANDBOX/target-agent-shared/debug"
: > "$SANDBOX/target-agent-shared/debug/rivet-client"

# gate.sh now also runs the scenario runner's Paper rows (join/move
# Paper-vs-Rivet differentials) whenever the paperclip jar and the client binary
# are present (SCENARIO_RUNNABLE). Stub run-scenario.sh to succeed so a green
# full gate reaches GATE GREEN; the scenario's own behaviour is covered by
# test_gate_prereqs.sh and the runner's unit tests.
mkdir -p "$SANDBOX/tools/rivet-client"
cat > "$SANDBOX/tools/rivet-client/run-scenario.sh" <<'EOF'
#!/bin/bash
[ "${RIVET_BUILD_LOCK_HELD:-0}" = 1 ]
EOF
chmod +x "$SANDBOX/tools/rivet-client/run-scenario.sh"

# The real gate script under test.
cp "$PWD/scripts/gate.sh" "$SANDBOX/scripts/gate.sh"
cp "$PWD/scripts/with-build-lock.sh" "$SANDBOX/scripts/with-build-lock.sh"
chmod +x "$SANDBOX/scripts/gate.sh" "$SANDBOX/scripts/with-build-lock.sh"

# --- satisfy the oracle pre-check --------------------------------------------
# Same dummy-file/version-stub approach as test_gate_features.sh so a green full
# gate really reaches GATE GREEN rather than exiting 3 (ORACLE UNVERIFIED).
# bare java 25+ on PATH (reports openjdk 25).
cat > "$SANDBOX/home/.cargo/bin/java" <<'EOF'
#!/bin/bash
echo 'openjdk version "25.0.2" 2026-01-20' >&2
exit 0
EOF
# Java 25 JDK for the reference oracle (JAVA_HOME => bin/javac).
cat > "$SANDBOX/jdk/bin/javac" <<'EOF'
#!/bin/bash
echo 'javac 25.0.2'
exit 0
EOF
# paperclip bundler jar + Paper compile jar (existence only).
: > "$SANDBOX/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar"
: > "$SANDBOX/working/Paper/paper-server/build/libs/paper-server-26.2.local-SNAPSHOT.jar"
# materialized runtime: the libraries dir (created above) + the runtime jar
# beside it at versions/26.2/paper-26.2.jar.
: > "$SANDBOX/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
# The reference-oracle self-test step (run_oracle_self_test) invokes run.sh;
# stub it to emit the bare JSON summary so a green full gate reaches GATE GREEN.
cat > "$SANDBOX/tools/rivet-reference-oracle/run.sh" <<'EOF'
#!/bin/bash
printf '%s\n' '{"ok":true,"protocol":1,"tests":9}'
EOF
chmod +x "$SANDBOX/tools/rivet-reference-oracle/run.sh"

TEST_LOG="$SANDBOX/invocations.log"
: > "$TEST_LOG"
FAIL_FUZZ_CHECK="$SANDBOX/fail_fuzz_check"     # presence fails the fuzz `check` step
FAIL_FUZZ_CLIPPY="$SANDBOX/fail_fuzz_clippy"   # presence fails the fuzz `clippy` step

# Stub cargo: log every invocation; honor the failure markers; everything else
# passes. The fuzz step runs as `cargo check -p rivet-fuzz --features packets
# --bins` and as `cargo clippy -p rivet-fuzz --features packets --all-targets`;
# the two cases below honor the markers for those exact invocations. The
# workspace-wide steps (`fmt --all --check`, `clippy --workspace --all-targets`,
# `test --workspace`/`nextest run --workspace`) and the oracle steps have no
# `-p rivet-fuzz --features packets` and never fail. Note `cargo machete`/
# `cargo machete tools/rivet-codegen` are dispatched by cargo to the
# `cargo-machete` binary (also stubbed), not to this stub.
cat > "$SANDBOX/home/.cargo/bin/cargo" <<EOF
#!/bin/bash
echo "\$*" >> "$TEST_LOG"
case " \$* " in
  " check -p rivet-fuzz --features packets "*)
    if [ -f "$FAIL_FUZZ_CHECK" ]; then
      echo "stub: failing fuzz check invocation" >&2
      exit 1
    fi
    ;;
  " clippy -p rivet-fuzz --features packets "*)
    if [ -f "$FAIL_FUZZ_CLIPPY" ]; then
      echo "stub: failing fuzz clippy invocation" >&2
      exit 1
    fi
    ;;
esac
exit 0
EOF

# Satisfies gate.sh's `command -v cargo-machete` guard (no install step) and
# receives the `cargo machete` subcommand dispatches.
cat > "$SANDBOX/home/.cargo/bin/cargo-machete" <<'EOF'
#!/bin/bash
exit 0
EOF
# gate.sh only needs `command -v cargo-nextest` to pick the branch; the actual
# nextest invocation is dispatched through the `cargo` stub above, so in the
# nextest profile this binary just needs to exist, and in the fallback profile
# it must be absent entirely.
cat > "$SANDBOX/home/.cargo/bin/cargo-nextest" <<'EOF'
#!/bin/bash
exit 0
EOF
chmod +x "$SANDBOX/home/.cargo/bin/cargo" "$SANDBOX/home/.cargo/bin/cargo-machete" \
        "$SANDBOX/home/.cargo/bin/cargo-nextest" \
        "$SANDBOX/home/.cargo/bin/java" "$SANDBOX/jdk/bin/javac"

# Fully controlled PATH: sandbox bin + minimal system dirs. gate.sh prepends
# $HOME/.cargo/bin itself. JAVA_HOME points at the sandbox jdk so the
# reference-oracle javac probe is deterministic on hosts with their own JDK.
GATE="env HOME=$SANDBOX/home JAVA_HOME=$SANDBOX/jdk CARGO_TARGET_DIR=$SANDBOX/target-agent-shared PATH=$SANDBOX/home/.cargo/bin:/usr/bin:/bin $SANDBOX/scripts/gate.sh"

# Assertions shared by every scenario: the fuzz step must be scoped to
# `-p rivet-fuzz --features packets` — never widened to `--all-features` or
# `--workspace --features` (which would enable rivet-protocol's `packets`
# feature workspace-wide or fail on crates that lack `packets`).
assert_no_workspace_wide_features() {
  local what="$1" log="$2"
  if grep -q -- "--all-features" "$log"; then
    echo "FAIL ($what): fuzz step used --all-features" >&2
    exit 1
  fi
  if grep -q -- "--workspace --features" "$log"; then
    echo "FAIL ($what): fuzz step used --workspace --features" >&2
    exit 1
  fi
}

# assert_fuzz_ran <profile> <log>: the fuzz check AND clippy invocations both
# appear in the gate's cargo invocation log.
assert_fuzz_ran() {
  local profile="$1" log="$2"
  grep -q -- "check -p rivet-fuzz --features packets" "$log" \
    || { echo "FAIL ($profile): fuzz check invocation missing from cargo log" >&2; exit 1; }
  grep -q -- "clippy -p rivet-fuzz --features packets" "$log" \
    || { echo "FAIL ($profile): fuzz clippy invocation missing from cargo log" >&2; exit 1; }
}

# run_scenarios <profile-name> <nextest-presence>
#   nextest-presence: "nextest" installs the cargo-nextest stub; anything else
#   removes it so gate.sh takes the cargo test fallback branch.
run_scenarios() {
  local profile="$1" nextest="$2"
  if [ "$nextest" = nextest ]; then
    chmod +x "$SANDBOX/home/.cargo/bin/cargo-nextest"
  else
    rm -f "$SANDBOX/home/.cargo/bin/cargo-nextest"
  fi

  # --- scenario 0: green full gate runs the fuzz step and reaches GATE GREEN --
  rm -f "$FAIL_FUZZ_CHECK" "$FAIL_FUZZ_CLIPPY"
  : > "$TEST_LOG"
  if ! eval "$GATE" > "$SANDBOX/$profile.green.log" 2>&1; then
    echo "FAIL ($profile, scenario 0): green full gate did not exit 0" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): green full gate did not reach GATE GREEN" >&2; exit 1; }
  grep -q "rivet-fuzz --features packets (protocol packet-decode fuzz targets)" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): fuzz step did not run on the full gate" >&2; exit 1; }
  assert_fuzz_ran "$profile" "$TEST_LOG"
  assert_no_workspace_wide_features "$profile/scenario-0" "$TEST_LOG"
  echo "ok ($profile): green full gate runs fuzz check+clippy and reaches GATE GREEN"

  # --- scenario 1: full gate is red when the fuzz check fails -------------------
  rm -f "$FAIL_FUZZ_CLIPPY"
  : > "$TEST_LOG"
  touch "$FAIL_FUZZ_CHECK"
  if eval "$GATE" > "$SANDBOX/$profile.red-check.log" 2>&1; then
    echo "FAIL ($profile, scenario 1): full gate exited 0 despite a failing fuzz check" >&2
    exit 1
  fi
  grep -q "stub: failing fuzz check invocation" "$SANDBOX/$profile.red-check.log" \
    || { echo "FAIL ($profile, scenario 1): the fuzz check stub was not invoked" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/$profile.red-check.log" && {
    echo "FAIL ($profile, scenario 1): 'GATE GREEN' printed despite a fuzz check failure" >&2; exit 1; }
  grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.red-check.log" && {
    echo "FAIL ($profile, scenario 1): red came from the oracle verdict, not the fuzz check failure" >&2; exit 1; }
  echo "ok ($profile): full gate is red when the fuzz check step fails"

  # --- scenario 2: full gate is red when the fuzz clippy fails ------------------
  rm -f "$FAIL_FUZZ_CHECK"
  : > "$TEST_LOG"
  touch "$FAIL_FUZZ_CLIPPY"
  if eval "$GATE" > "$SANDBOX/$profile.red-clippy.log" 2>&1; then
    echo "FAIL ($profile, scenario 2): full gate exited 0 despite a failing fuzz clippy" >&2
    exit 1
  fi
  grep -q "stub: failing fuzz clippy invocation" "$SANDBOX/$profile.red-clippy.log" \
    || { echo "FAIL ($profile, scenario 2): the fuzz clippy stub was not invoked" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/$profile.red-clippy.log" && {
    echo "FAIL ($profile, scenario 2): 'GATE GREEN' printed despite a fuzz clippy failure" >&2; exit 1; }
  grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.red-clippy.log" && {
    echo "FAIL ($profile, scenario 2): red came from the oracle verdict, not the fuzz clippy failure" >&2; exit 1; }
  echo "ok ($profile): full gate is red when the fuzz clippy step fails"

  # --- scenario 3: scoped rivet-fuzz gate still runs the fuzz step ---------------
  rm -f "$FAIL_FUZZ_CHECK" "$FAIL_FUZZ_CLIPPY"
  : > "$TEST_LOG"
  if ! eval "$GATE rivet-fuzz" > "$SANDBOX/$profile.scoped-fuzz.log" 2>&1; then
    echo "FAIL ($profile, scenario 3): scoped rivet-fuzz gate failed (exit non-zero)" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.scoped-fuzz.log" \
    || { echo "FAIL ($profile, scenario 3): scoped rivet-fuzz gate did not reach GATE GREEN" >&2; exit 1; }
  assert_fuzz_ran "$profile/scenario-3" "$TEST_LOG"
  echo "ok ($profile): scoped rivet-fuzz gate still runs the fuzz step"

  # --- scenario 4: scoped gate for an unrelated crate skips the fuzz step -------
  : > "$TEST_LOG"
  if ! eval "$GATE crates/rivet-nbt" > "$SANDBOX/$profile.scoped-nbt.log" 2>&1; then
    echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate failed (exit non-zero)" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.scoped-nbt.log" \
    || { echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate did not reach GATE GREEN" >&2; exit 1; }
  if grep -q -- "-p rivet-fuzz --features packets" "$TEST_LOG"; then
    echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate ran the fuzz step" >&2
    exit 1
  fi
  assert_no_workspace_wide_features "$profile/scenario-4" "$TEST_LOG"
  echo "ok ($profile): scoped rivet-nbt gate skips the fuzz step"
}

run_scenarios "nextest" nextest
run_scenarios "fallback-cargo-test" absent

echo "ALL GATE FUZZ TESTS PASSED (nextest + cargo test fallback)"
