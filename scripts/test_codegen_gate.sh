#!/bin/bash
# Focused test for the rivet-codegen gate step (issue #112).
#
# Proves the codegen enforcement properties without running the whole workspace
# build:
#   0. A full gate whose codegen fmt/clippy/test and machete steps pass, and
#      whose oracle prereqs are present, reaches GATE GREEN (exit 0) — this
#      proves the sandbox satisfies the oracle pre-check, so the red scenarios
#      below fail because of codegen, not because the gate exits 3
#      (ORACLE UNVERIFIED).
#   1. A failing codegen fmt/clippy/test step makes the FULL gate red
#      (set -e aborts before "GATE GREEN").
#   2. A SCOPED crate gate skips the codegen step entirely — a codegen
#      failure must not fail a scoped gate (same rule as oracle/scenario).
#   3. A failing codegen cargo-machete check makes the FULL gate red (machete
#      stays workspace-wide, so the codegen manifest is covered on the full
#      gate and scoped gates alike).
#
# Every scenario runs twice — once with cargo-nextest on PATH (the gate's
# preferred `cargo nextest run --manifest-path` branch) and once with it absent
# (the `cargo test --manifest-path` fallback) — so both test-step branches are
# exercised on every host regardless of which one the host actually has.
#
# Runs the real scripts/gate.sh against a sandboxed repo layout, with stubs
# `cargo`/`cargo-machete`/`cargo-nextest` installed under a redirected
# $HOME/.cargo/bin and a fully controlled PATH (sandbox bin + /usr/bin:/bin, no
# ambient ~/.cargo/bin or rustup shims), so the result is deterministic on any
# host and never depends on ambient PATH. The gate's oracle pre-check (added in
# the oracle-gate-hardening merge, #121) is satisfied inside the sandbox with a
# stub `java`/`javac` and dummy jar/library files at the exact paths it probes,
# so a green full gate really reaches GATE GREEN rather than exiting 3. No real
# build, network fetch, or toolchain install happens. The sandbox is deleted on
# exit.
set -euo pipefail
cd "$(dirname "$0")/.."

SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

# --- build the sandbox -------------------------------------------------------
# Repo layout the real gate.sh probes (REPO_DIR resolves to $SANDBOX).
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

# The real gate script under test.
cp "$PWD/scripts/gate.sh" "$SANDBOX/scripts/gate.sh"
cp "$PWD/scripts/with-build-lock.sh" "$SANDBOX/scripts/with-build-lock.sh"
chmod +x "$SANDBOX/scripts/gate.sh" "$SANDBOX/scripts/with-build-lock.sh"

# The full gate runs `python3 scripts/test_analyze_graph.py` (the manifest
# regression suite, added in #65 M1), and the marker audit runs
# `scripts/check_markers.py` + `scripts/test_check_markers.py`. The sandbox has
# no real Paper tree, so stub them all to pass — the suites' own behaviour is
# covered elsewhere; here they only need to not abort the gate on its pass path.
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_analyze_graph.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/check_markers.py"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n' > "$SANDBOX/scripts/test_check_markers.py"

# The oracle pre-check now requires the rivet-client binary (join-capture
# harness); provide an existing dummy file so the pre-check reports all
# prerequisites present.
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

# --- satisfy the oracle pre-check ---------------------------------------------
# oracle_prereq_check (gate.sh, full gate only) probes for java 25+, python3,
# free disk, a paperclip jar, a Java 25 JDK, a Paper compile jar, and the
# materialized runtime (libraries dir + runtime jar). It only checks existence —
# the real SHA/commit pins live in the oracle tools, which are stubbed out here
# — so dummy files suffice. This keeps a green full gate on the exit-0 path;
# otherwise the gate would exit 3 (ORACLE UNVERIFIED) and scenarios 1 and 3
# below would pass for the wrong reason.

# bare java 25+ on PATH (reports openjdk 25).
cat > "$SANDBOX/home/.cargo/bin/java" <<'EOF'
#!/bin/bash
echo 'openjdk version "25.0.2" 2026-01-20' >&2
exit 0
EOF

# Java 25 JDK for the reference oracle (RIVET_JAVA_HOME => bin/javac).
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
FAIL_CODEGEN_TEST="$SANDBOX/fail_codegen_test"   # presence makes codegen `test` fail
FAIL_MACHETE="$SANDBOX/fail_machete"             # presence makes `machete tools/rivet-codegen` fail

# Stub cargo: log every invocation; honor the failure markers; everything else
# passes. The `cargo machete ...` invocations in gate.sh go through this stub,
# so the cargo-machete binary only needs to exist for gate.sh's `command -v`
# install guard (which we satisfy below). The codegen test step runs as
# `cargo nextest run --manifest-path ...` on nextest hosts and as
# `cargo test --manifest-path ...` on the fallback; both carry the
# `--manifest-path` marker, so a single case honors FAIL_CODEGEN_TEST for the
# two branches. Workspace-wide steps (`cargo test --workspace`,
# `cargo nextest run --workspace`) have no `--manifest-path` and never fail.
# The oracle steps run as `cargo run -q -p rivet-oracle -- verify` and
# `cargo run -q -p rivet-parity -- --require-oracle`; the gate reads their exit 0
# as VERIFIED, so the stub passing keeps ORACLE_UNVERIFIED clear.
cat > "$SANDBOX/home/.cargo/bin/cargo" <<EOF
#!/bin/bash
echo "\$*" >> "$TEST_LOG"
case " \$* " in
  " test --manifest-path "*|" nextest run --manifest-path "*)
    if [ -f "$FAIL_CODEGEN_TEST" ]; then
      echo "stub: failing codegen test invocation" >&2
      exit 1
    fi
    ;;
  " machete "*"rivet-codegen"*)
    if [ -f "$FAIL_MACHETE" ]; then
      echo "stub: machete found unused dep in rivet-codegen" >&2
      exit 1
    fi
    ;;
esac
exit 0
EOF

# Satisfies gate.sh's `command -v cargo-machete` guard (no install step).
cat > "$SANDBOX/home/.cargo/bin/cargo-machete" <<'EOF'
#!/bin/bash
exit 0
EOF

# gate.sh itself only needs `command -v cargo-nextest` to pick the branch; the
# actual nextest invocation is dispatched through the `cargo` stub above, so in
# the nextest profile this binary just needs to exist and be executable, and in
# the fallback profile it must be absent entirely.
cat > "$SANDBOX/home/.cargo/bin/cargo-nextest" <<'EOF'
#!/bin/bash
exit 0
EOF
chmod +x "$SANDBOX/home/.cargo/bin/cargo" "$SANDBOX/home/.cargo/bin/cargo-machete" \
        "$SANDBOX/home/.cargo/bin/cargo-nextest" \
        "$SANDBOX/home/.cargo/bin/java" "$SANDBOX/jdk/bin/javac"

# Fully controlled PATH: sandbox bin + minimal system dirs. gate.sh prepends
# $HOME/.cargo/bin itself, so a host with cargo-nextest installed in its own
# ~/.cargo/bin cannot leak in and flip the fallback profile non-deterministically.
# JAVA_HOME points at the sandbox jdk so the reference-oracle javac probe is
# deterministic on hosts that have their own JDK configured.
GATE="env HOME=$SANDBOX/home JAVA_HOME=$SANDBOX/jdk CARGO_TARGET_DIR=$SANDBOX/target-agent-shared PATH=$SANDBOX/home/.cargo/bin:/usr/bin:/bin $SANDBOX/scripts/gate.sh"

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

  # --- scenario 0: green full gate reaches GATE GREEN ---------------------------
  # Prereqs are satisfied and no failure marker is set: the gate must exit 0 and
  # print GATE GREEN. This proves the sandbox exercises the codegen and oracle
  # steps on their pass path, so the red scenarios below cannot be passing on
  # the exit-3 (ORACLE UNVERIFIED) path.
  rm -f "$FAIL_CODEGEN_TEST" "$FAIL_MACHETE"
  : > "$TEST_LOG"
  if ! eval "$GATE" > "$SANDBOX/$profile.green.log" 2>&1; then
    echo "FAIL ($profile, scenario 0): green full gate did not exit 0" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): green full gate did not reach GATE GREEN" >&2; exit 1; }
  grep -q "rivet-codegen (workspace-excluded tool) fmt/clippy/test" "$SANDBOX/$profile.green.log" \
    || { echo "FAIL ($profile, scenario 0): codegen step did not run on the full gate" >&2; exit 1; }
  echo "ok ($profile): green full gate reaches GATE GREEN"

  # --- scenario 1: full gate is red when codegen test fails -------------------
  touch "$FAIL_CODEGEN_TEST"
  rm -f "$FAIL_MACHETE"
  : > "$TEST_LOG"
  if eval "$GATE" > "$SANDBOX/$profile.full.log" 2>&1; then
    echo "FAIL ($profile, scenario 1): full gate exited 0 despite a failing codegen test" >&2
    exit 1
  fi
  grep -q "rivet-codegen (workspace-excluded tool) fmt/clippy/test" "$SANDBOX/$profile.full.log" \
    || { echo "FAIL ($profile, scenario 1): codegen step did not run on the full gate" >&2; exit 1; }
  grep -q "stub: failing codegen test invocation" "$SANDBOX/$profile.full.log" \
    || { echo "FAIL ($profile, scenario 1): the codegen test stub was not invoked" >&2; exit 1; }
  if grep -q "GATE GREEN" "$SANDBOX/$profile.full.log"; then
    echo "FAIL ($profile, scenario 1): 'GATE GREEN' printed despite a codegen failure" >&2
    exit 1
  fi
  if grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.full.log"; then
    echo "FAIL ($profile, scenario 1): red came from the oracle verdict, not the codegen failure" >&2
    exit 1
  fi
  echo "ok ($profile): full gate is red when the codegen test step fails"

  # --- scenario 2: scoped crate gate skips codegen -----------------------------
  # Re-arm the codegen test failure; if a scoped gate ran the codegen step it
  # would fail — the property under test is that it is never invoked at all.
  rm -f "$FAIL_CODEGEN_TEST" "$FAIL_MACHETE"
  : > "$TEST_LOG"
  touch "$FAIL_CODEGEN_TEST"
  if eval "$GATE crates/rivet-nbt" > "$SANDBOX/$profile.scoped.log" 2>&1; then
    :
  else
    echo "FAIL ($profile, scenario 2): scoped crate gate failed (exit non-zero)" >&2
    exit 1
  fi
  if grep -q "rivet-codegen (workspace-excluded tool)" "$SANDBOX/$profile.scoped.log"; then
    echo "FAIL ($profile, scenario 2): scoped gate ran the codegen step" >&2
    exit 1
  fi
  if grep -q -- "--manifest-path" "$TEST_LOG"; then
    echo "FAIL ($profile, scenario 2): scoped gate invoked a codegen (--manifest-path) command" >&2
    exit 1
  fi
  grep -q "GATE GREEN" "$SANDBOX/$profile.scoped.log" \
    || { echo "FAIL ($profile, scenario 2): scoped gate did not reach GATE GREEN" >&2; exit 1; }
  echo "ok ($profile): scoped crate gate skips the codegen step"

  # --- scenario 3: full gate is red when codegen machete fails ------------------
  # machete is workspace-wide by design (runs on full and scoped gates alike), so
  # a codegen unused-dep finding must redden the full gate.
  rm -f "$FAIL_CODEGEN_TEST"
  : > "$TEST_LOG"
  touch "$FAIL_MACHETE"
  if eval "$GATE" > "$SANDBOX/$profile.machete.log" 2>&1; then
    echo "FAIL ($profile, scenario 3): full gate exited 0 despite a codegen machete finding" >&2
    exit 1
  fi
  grep -q "stub: machete found unused dep in rivet-codegen" "$SANDBOX/$profile.machete.log" \
    || { echo "FAIL ($profile, scenario 3): the machete codegen stub was not invoked" >&2; exit 1; }
  if grep -q "GATE GREEN" "$SANDBOX/$profile.machete.log"; then
    echo "FAIL ($profile, scenario 3): 'GATE GREEN' printed despite a codegen machete finding" >&2
    exit 1
  fi
  if grep -q "ORACLE UNVERIFIED" "$SANDBOX/$profile.machete.log"; then
    echo "FAIL ($profile, scenario 3): red came from the oracle verdict, not the machete finding" >&2
    exit 1
  fi
  echo "ok ($profile): full gate is red when the codegen machete check fails"
}

run_scenarios "nextest" nextest
run_scenarios "fallback-cargo-test" absent

echo "ALL CODEGEN GATE TESTS PASSED (nextest + cargo test fallback)"
