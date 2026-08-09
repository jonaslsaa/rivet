#!/bin/bash
# Focused test for the rivet-protocol `packets`-feature gate step (issue #207).
# The generated packet-ID tables (src/generated), the packet body codecs
# (protocol/* modules), and their registration/integration tests (e.g. the
# server-links registration tests) live behind the crate's `packets` cargo
# feature, so `cargo test --workspace` never builds or executes them. gate.sh
# must run them explicitly on every merge — this test proves that step exists,
# runs, and can never silently disappear or skip.
#
# Properties under test (each run in both the cargo-nextest and cargo-test
# profiles, matching how the step branches on nextest availability):
#   0. A green full gate invokes both the packets-feature clippy AND test
#      commands, and reaches GATE GREEN.
#   1. A failing packets-feature TEST makes the full gate red (set -e aborts).
#   2. A failing packets-feature CLIPPY makes the full gate red.
#   3. A scoped gate for crates/rivet-protocol still runs the packets step (the
#      feature tables/bodies are part of that crate, so a rivet-protocol-scoped
#      gate must cover them the same way).
#   4. A scoped gate for an unrelated crate (crates/rivet-nbt) does NOT run the
#      packets step (the step is not a workspace-wide or crate-blind addition).
#   5. Every scenario guards the anti-pattern: the invocation is always
#      `-p rivet-protocol --features packets` — `--all-features` (every feature
#      of every selected package) or `--workspace --features` (also enables
#      `packets` on rivet-fuzz) never appear in the log.
#
# Like test_gate_features.sh, this runs the real scripts/gate.sh against a
# sandboxed repo layout with stubs for cargo/cargo-machete/cargo-nextest/java
# under a redirected $HOME/.cargo/bin and a controlled PATH, so it is
# deterministic on any host and never depends on ambient PATH or an ambient
# toolchain. No real build, network fetch, or toolchain install happens; the
# sandbox is deleted on exit.
set -euo pipefail
cd "$(dirname "$0")/.."

# Shared dwell-stub setup + counterfactual (scripts/test-stubs/dwell-stub-setup.sh).
source "$PWD/scripts/test-stubs/dwell-stub-setup.sh"

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
         "$SANDBOX/working/Paper/paper-server/build/libs"

# Minimal workspace + codegen manifest so gate.sh's --manifest-path / path
# arguments resolve to existing files.
printf '[workspace]\n' > "$SANDBOX/Cargo.toml"
printf '[package]\nname = "rivet-codegen"\nversion = "0.1.0"\nedition = "2024"\n' \
  > "$SANDBOX/tools/rivet-codegen/Cargo.toml"

# The full gate runs `python3 scripts/test_analyze_graph.py` (the manifest
# regression suite), and the marker audit runs `scripts/check_markers.py` +
# `scripts/test_check_markers.py`. The sandbox has no real Paper tree, so stub
# them all to pass — the suites' own behaviour is covered elsewhere; here they
# only need to not abort the gate on its pass path.
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_analyze_graph.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/check_markers.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_check_markers.py"

# The oracle pre-check now requires the rivet-client binary (join-capture
# harness); provide an existing dummy file so the pre-check reports all
# prerequisites present.
mkdir -p "$SANDBOX/tools/rivet-client/target/debug"
: > "$SANDBOX/tools/rivet-client/target/debug/rivet-client"
# The full gate's scenario runner also invokes tools/rivet-client/run-scenario.sh
# for the unconditional dwell row (issue #160); install the shared sandbox stub.
install_dwell_stub "$SANDBOX"

# The real gate script under test.
cp "$PWD/scripts/gate.sh" "$SANDBOX/scripts/gate.sh"
chmod +x "$SANDBOX/scripts/gate.sh"

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

TEST_LOG="$SANDBOX/invocations.log"
: > "$TEST_LOG"
FAIL_PACKETS_TEST="$SANDBOX/fail_packets_test"     # presence fails the packets-feature `test` step
FAIL_PACKETS_CLIPPY="$SANDBOX/fail_packets_clippy" # presence fails the packets-feature `clippy` step

# Stub cargo: log every invocation; honor the failure markers; everything else
# passes. The workspace-wide steps (`fmt --all --check`, `clippy --workspace
# --all-targets`, `test --workspace`/`nextest run --workspace`) and the oracle
# steps (`run -q -p rivet-oracle -- verify`, `run -q -p rivet-parity -- --require-oracle`)
# have no `-p rivet-protocol --features packets` and never fail. The packets
# step runs as `clippy -p rivet-protocol --features packets --all-targets` and
# as `test -p rivet-protocol --features packets` (nextest: `nextest run -p
# rivet-protocol --features packets`); the two cases below honor the markers for
# those exact invocations. The sibling `-p rivet-registry --features blocks`
# step runs too and is left to pass. Note `cargo machete`/`cargo machete
# tools/rivet-codegen` are dispatched by cargo to the `cargo-machete` binary
# (also stubbed), not to this stub.
cat > "$SANDBOX/home/.cargo/bin/cargo" <<EOF
#!/bin/bash
echo "\$*" >> "$TEST_LOG"
case " \$* " in
  " clippy -p rivet-protocol --features packets "*)
    if [ -f "$FAIL_PACKETS_CLIPPY" ]; then
      echo "stub: failing packets-feature clippy invocation" >&2
      exit 1
    fi
    ;;
  " test -p rivet-protocol --features packets "*|" nextest run -p rivet-protocol --features packets "*)
    if [ -f "$FAIL_PACKETS_TEST" ]; then
      echo "stub: failing packets-feature test invocation" >&2
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
# The RIVET_* oracle env vars are unset so a developer's real oracle installation
# cannot leak in and flip the scenario-runner's paperclip guard (which would
# route the join/move rows into the strict dwell-only stub). DWELL_STUB_LOG /
# DWELL_STUB_FAIL (set by install_dwell_stub) reach the sandbox's run-scenario
# stub through the env.
GATE="env -u RIVET_ORACLE_JAR -u RIVET_PAPER_JAR -u RIVET_PAPER_LIBRARIES -u RIVET_PAPER_RUNTIME_JAR -u RIVET_JAVA_HOME HOME=$SANDBOX/home JAVA_HOME=$SANDBOX/jdk DWELL_STUB_LOG=$DWELL_STUB_LOG DWELL_STUB_FAIL=$DWELL_STUB_FAIL PATH=$SANDBOX/home/.cargo/bin:/usr/bin:/bin $SANDBOX/scripts/gate.sh"

# Assertions shared by every scenario: the packets step must be scoped to
# `-p rivet-protocol --features packets` — never widened to `--all-features` or
# `--workspace --features` (`--all-features` enables every feature of every
# selected package; `--workspace --features packets` also enables `packets` on
# rivet-fuzz).
assert_no_workspace_wide_features() {
  local what="$1" log="$2"
  if grep -q -- "--all-features" "$log"; then
    echo "FAIL ($what): packets step used --all-features" >&2
    exit 1
  fi
  if grep -q -- "--workspace --features" "$log"; then
    echo "FAIL ($what): packets step used --workspace --features" >&2
    exit 1
  fi
}

# assert_packets_ran <profile> <log>: the packets clippy AND test invocations
# both appear in the gate's cargo invocation log.
assert_packets_ran() {
  local profile="$1" log="$2"
  grep -q -- "clippy -p rivet-protocol --features packets" "$log" \
    || { echo "FAIL ($profile): packets-feature clippy invocation missing from cargo log" >&2; exit 1; }
  grep -qE -- "test -p rivet-protocol --features packets|nextest run -p rivet-protocol --features packets" "$log" \
    || { echo "FAIL ($profile): packets-feature test invocation missing from cargo log" >&2; exit 1; }
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

  # --- scenario 0: green full gate runs the packets step and reaches GATE GREEN --
  rm -f "$FAIL_PACKETS_TEST" "$FAIL_PACKETS_CLIPPY"
  : > "$TEST_LOG"
  # The dwell stub's invocation log must be empty going in so
  # assert_dwell_invoked below reflects only this run's gate, not a stale entry
  # left by an earlier profile's counterfactuals.
  : > "$DWELL_STUB_LOG"
  if ! eval "$GATE" > "$SANDBOX/$profile.green.log" 2>&1; then
    echo "FAIL ($profile, scenario 0): green full gate did not exit 0" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): green full gate did not reach GATE GREEN" >&2; exit 1; }
  grep -q "rivet-protocol --features packets (generated packet tables + packet bodies)" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): packets step did not run on the full gate" >&2; exit 1; }
  assert_packets_ran "$profile" "$TEST_LOG"
  assert_no_workspace_wide_features "$profile/scenario-0" "$TEST_LOG"
  # The dwell row (issue #160) must actually have run — a green gate that drops
  # it would leave the stub's invocation log empty.
  assert_dwell_invoked "$profile, scenario 0"
  echo "ok ($profile): green full gate runs packets clippy+test and reaches GATE GREEN"

  # --- scenario 1: full gate is red when the packets-feature test fails ---------
  rm -f "$FAIL_PACKETS_CLIPPY"
  : > "$TEST_LOG"
  touch "$FAIL_PACKETS_TEST"
  if eval "$GATE" > "$SANDBOX/$profile.red-test.log" 2>&1; then
    echo "FAIL ($profile, scenario 1): full gate exited 0 despite a failing packets test" >&2
    exit 1
  fi
  grep -q "stub: failing packets-feature test invocation" "$SANDBOX/$profile.red-test.log" \
    || { echo "FAIL ($profile, scenario 1): the packets test stub was not invoked" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/$profile.red-test.log" && {
    echo "FAIL ($profile, scenario 1): 'GATE GREEN' printed despite a packets failure" >&2; exit 1; }
  grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.red-test.log" && {
    echo "FAIL ($profile, scenario 1): red came from the oracle verdict, not the packets failure" >&2; exit 1; }
  echo "ok ($profile): full gate is red when the packets-feature test step fails"

  # --- scenario 2: full gate is red when the packets-feature clippy fails -------
  rm -f "$FAIL_PACKETS_TEST"
  : > "$TEST_LOG"
  touch "$FAIL_PACKETS_CLIPPY"
  if eval "$GATE" > "$SANDBOX/$profile.red-clippy.log" 2>&1; then
    echo "FAIL ($profile, scenario 2): full gate exited 0 despite a failing packets clippy" >&2
    exit 1
  fi
  grep -q "stub: failing packets-feature clippy invocation" "$SANDBOX/$profile.red-clippy.log" \
    || { echo "FAIL ($profile, scenario 2): the packets clippy stub was not invoked" >&2; exit 1; }
  grep -q "GATE GREEN" "$SANDBOX/$profile.red-clippy.log" && {
    echo "FAIL ($profile, scenario 2): 'GATE GREEN' printed despite a packets clippy failure" >&2; exit 1; }
  grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.red-clippy.log" && {
    echo "FAIL ($profile, scenario 2): red came from the oracle verdict, not the packets clippy failure" >&2; exit 1; }
  echo "ok ($profile): full gate is red when the packets-feature clippy step fails"

  # --- scenario 3: scoped rivet-protocol gate still runs the packets step -------
  rm -f "$FAIL_PACKETS_TEST" "$FAIL_PACKETS_CLIPPY"
  : > "$TEST_LOG"
  if ! eval "$GATE crates/rivet-protocol" > "$SANDBOX/$profile.scoped-protocol.log" 2>&1; then
    echo "FAIL ($profile, scenario 3): scoped rivet-protocol gate failed (exit non-zero)" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.scoped-protocol.log" \
    || { echo "FAIL ($profile, scenario 3): scoped rivet-protocol gate did not reach GATE GREEN" >&2; exit 1; }
  assert_packets_ran "$profile/scenario-3" "$TEST_LOG"
  echo "ok ($profile): scoped rivet-protocol gate still runs the packets step"

  # --- scenario 4: scoped gate for an unrelated crate skips the packets step ----
  : > "$TEST_LOG"
  if ! eval "$GATE crates/rivet-nbt" > "$SANDBOX/$profile.scoped-nbt.log" 2>&1; then
    echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate failed (exit non-zero)" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.scoped-nbt.log" \
    || { echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate did not reach GATE GREEN" >&2; exit 1; }
  if grep -q -- "-p rivet-protocol --features packets" "$TEST_LOG"; then
    echo "FAIL ($profile, scenario 4): scoped rivet-nbt gate ran the packets step" >&2
    exit 1
  fi
  assert_no_workspace_wide_features "$profile/scenario-4" "$TEST_LOG"
  echo "ok ($profile): scoped rivet-nbt gate skips the packets step"

  # --- dwell counterfactuals (shared: failing verdict, wrong invocation, removal, leaked jar) --
  dwell_gate_counterfactuals
}

run_scenarios "nextest" nextest
run_scenarios "fallback-cargo-test" absent

echo "ALL GATE PACKETS TESTS PASSED (nextest + cargo test fallback)"
