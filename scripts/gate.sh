#!/bin/bash
# The merge gate. Run before merging any PR (and at the end of every wave).
# No hosted CI by design — this script IS the gate; a red gate blocks the merge.
#
# Scope: pass crate names or crates/... paths as arguments (or set SCOPE to a
# space/comma-separated list) to gate only those crates (fmt/clippy/test). The
# unused-deps check (cargo machete, including the workspace-excluded codegen
# tool's own manifest) and the default full gate stay workspace-wide.
#
#   ./scripts/gate.sh                     # full gate: fmt, clippy, tests, manifest, codegen, oracle, scenario, machete
#   ./scripts/gate.sh crates/rivet-nbt     # fmt+clippy+test for rivet-nbt only
# The full gate also runs both oracle steps against the Paper Java oracle, plus the
# scenario runner:
#   - rivet-oracle verify  M0 sanity gate: boot a fresh Paper server and diff its
#                          chunk-NBT slice against the committed golden baseline.
#                          Also runs verify --expect-fail, the negative control:
#                          a fresh boot diffed against a corrupted temp baseline
#                          copy that must be detected and named (proves the
#                          boot->extract->diff chain is not vacuously green).
#   - rivet-parity         byte-for-byte NBT/SNBT diff of rivet-nbt against the Paper
#                          reference oracle — the only gate step that exercises real
#                          Rivet code against Paper.
#   - scenario runner      join: boots Paper twice via the Azalea client and requires
#                          identical normalized transcripts, plus a negative case.
#                          Guarded by the paperclip jar, like oracle verify.
#   - join capture         rivet-capture: boots Paper, joins via the Azalea client
#                          through a byte-transparent proxy, and diffs the normalized
#                          join packets byte-for-byte against the committed fixture,
#                          plus a negative control. Guarded by the paperclip jar + the
#                          rivet-client binary, like oracle verify.
# Oracle verification is never silently skipped. When its prerequisites are missing
# the pre-check below names each missing item with a fix, the steps report UNVERIFIED,
# and the gate exits with a distinct nonzero code (3). (One path is red but not
# normalized to 3: a cargo failure inside oracle verify aborts the gate via errexit
# with that command's own exit code.) An unverified merge never looks green. Pass
# --require-oracle (or RIVET_REQUIRE_ORACLE=1) to make any missing oracle
# prereq a hard failure (exit 1) right at the pre-check. The Paper jar SHA-256 / git-commit
# pin guards live in tools/rivet-reference-oracle/run.sh (compile jar == runtime jar SHA,
# Paper commit == manifest pin, exit 1 on mismatch) and stay authoritative; the pre-check
# only validates that the prerequisites exist. Because a present-but-stale runtime jar
# passes the pre-check yet still fails to boot, the rivet-parity step relies on the tool's
# machine-stable exit code — 0 VERIFIED, 1 FAILED, 3 UNVERIFIED — to classify the run.
# We always pass --require-oracle so a dead oracle exits 3 immediately and never degrades
# to a Rust-only run that could be mistaken for a green.
#
#   ./scripts/gate.sh                        # full gate: fmt, clippy, tests, manifest, codegen, oracle, scenario, machete
#   ./scripts/gate.sh --require-oracle       # full gate; missing oracle prereqs hard-fail
#   ./scripts/gate.sh crates/rivet-nbt       # fmt+clippy+test for rivet-nbt only
#   SCOPE="rivet-nbt, rivet-serialization" ./scripts/gate.sh
set -euo pipefail

# Distinct exit code for "oracle UNVERIFIED" (hard failures are exit 1).
ORACLE_EXIT_UNVERIFIED=3
# Global flags read/written by the functions below; main() sets them from argv/env.
REQUIRE_ORACLE=0
ORACLE_UNVERIFIED=0
# Function outputs (set by oracle_prereq_check, consumed by the step runners and
# by main's final verdict).
VERIFY_RUNNABLE=0
PARITY_RUNNABLE=0
CAPTURE_RUNNABLE=0

# Directory of this script via bash builtins only (no external tools at load
# time — sourcing must stay side-effect-free so tests can shim PATH).
_script_dir="${BASH_SOURCE[0]%/*}"
[ "$_script_dir" = "${BASH_SOURCE[0]}" ] && _script_dir="."
REPO_DIR="$(cd "$_script_dir/.." && pwd)"

# ---- oracle prereq pre-check (full gate only) --------------------------------
#
# Validates the prerequisites the two oracle steps need and prints an actionable
# [ok] / [MISSING] report. Sets the per-step runnability flags VERIFY_RUNNABLE and
# PARITY_RUNNABLE that the step runners consume. Honours the same env overrides the
# tools honour: RIVET_ORACLE_JAR; RIVET_PAPER_JAR / RIVET_PAPER_LIBRARIES /
# RIVET_PAPER_RUNTIME_JAR; RIVET_JAVA_HOME / JAVA_HOME / SDKMAN. With
# REQUIRE_ORACLE=1 any missing prereq is a hard failure (exit 1).
oracle_prereq_check() {
  local missing=0
  JAVA_BARE_OK=0; PYTHON3_OK=0; DISK_OK=0; JAVAC25_OK=0
  PAPERCLIP_JAR=""; COMPILE_JAR=""; LIBRARIES_DIR=""; RUNTIME_JAR=""
  # VERIFY_RUNNABLE / PARITY_RUNNABLE / CAPTURE_RUNNABLE are globals (the step
  # runners read them).
  VERIFY_RUNNABLE=0; PARITY_RUNNABLE=0; CAPTURE_RUNNABLE=0

  # rivet-oracle verify boots `java` directly, so bare java 25+ must be on PATH.
  if command -v java >/dev/null 2>&1; then
    # Parse the version with bash builtins (no head/sed): first quoted token.
    local jv="" ver="" major=""
    jv="$(java -version 2>&1 || true)"
    ver="${jv#*\"}"; ver="${ver%%\"*}"
    major="${ver%%.*}"
    if [ -n "$major" ] && [ "$major" -ge 25 ]; then
      JAVA_BARE_OK=1
      echo "  [ok]      java 25+ on PATH (${jv%%$'\n'*})"
    else
      echo "  [MISSING] java 25+ on PATH (${jv%%$'\n'*}) — Paper 26.2 needs Java 25; install a Temurin 25 JDK and add bin/ to PATH"
      missing=$((missing + 1))
    fi
  else
    echo "  [MISSING] java 25+ on PATH (java not found) — Paper 26.2 needs Java 25; install a Temurin 25 JDK and add bin/ to PATH"
    missing=$((missing + 1))
  fi

  # rivet-oracle verify runs scripts/extract_fixtures.py with python3.
  if command -v python3 >/dev/null 2>&1; then
    PYTHON3_OK=1
    echo "  [ok]      python3 ($(python3 --version 2>&1))"
  else
    echo "  [MISSING] python3 — rivet-oracle verify runs scripts/extract_fixtures.py with it (brew install python3)"
    missing=$((missing + 1))
  fi

  # A first Paper boot materializes ~160MB of libraries under tools/rivet-oracle/work/run/.
  local avail_kb="" mount=""
  if command -v df >/dev/null 2>&1; then
    # `|| true`: df can fail (missing path, odd FS); pipefail must not abort the gate.
    avail_kb="$(df -Pk "$REPO_DIR" 2>/dev/null | awk 'NR==2 {print $4}' || true)"
    mount="$(df -Pk "$REPO_DIR" 2>/dev/null | awk 'NR==2 {print $6}' || true)"
  fi
  if [ -n "$avail_kb" ] && [ "$avail_kb" -ge $((160 * 1024)) ]; then
    DISK_OK=1
    echo "  [ok]      free disk >= 160MB ($((avail_kb / 1024))MB free on $mount)"
  else
    if [ -z "$avail_kb" ]; then
      echo "  [MISSING] cannot check free disk (df unavailable or failed) — a first Paper boot materializes ~160MB of libraries into tools/rivet-oracle/work/run/"
    else
      echo "  [MISSING] free disk >= 160MB on $mount — a first Paper boot materializes ~160MB of libraries into tools/rivet-oracle/work/run/"
    fi
    missing=$((missing + 1))
  fi

  # paperclip bundler jar (rivet-oracle verify boots through it).
  if [ -n "${RIVET_ORACLE_JAR:-}" ] && [ -f "${RIVET_ORACLE_JAR}" ]; then
    PAPERCLIP_JAR="$RIVET_ORACLE_JAR"
  elif [ -f "$REPO_DIR/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar" ]; then
    PAPERCLIP_JAR="$REPO_DIR/tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar"
  else
    for c in "$REPO_DIR"/working/Paper/paper-server/build/libs/paper-paperclip-*.jar; do
      if [ -f "$c" ]; then
        PAPERCLIP_JAR="$c"
        break
      fi
    done
  fi
  if [ -n "$PAPERCLIP_JAR" ]; then
    echo "  [ok]      paperclip jar ($(basename "$PAPERCLIP_JAR"))"
  else
    echo "  [MISSING] paperclip jar (paper-paperclip-26.2.local-SNAPSHOT.jar) — build working/Paper, copy it to tools/rivet-oracle/work/jars/, or set RIVET_ORACLE_JAR"
    missing=$((missing + 1))
  fi

  # rivet-client (the offline Azalea bot the join-capture harness drives). The
  # scenario runner and rivet-capture both need it; the gate never runs the
  # capture step against a missing client binary.
  if [ -n "${RIVET_CLIENT_BIN:-}" ] && [ -f "${RIVET_CLIENT_BIN}" ]; then
    CLIENT_BIN="$RIVET_CLIENT_BIN"
  elif [ -f "$REPO_DIR/tools/rivet-client/target/debug/rivet-client" ]; then
    CLIENT_BIN="$REPO_DIR/tools/rivet-client/target/debug/rivet-client"
  else
    CLIENT_BIN=""
  fi
  if [ -n "$CLIENT_BIN" ]; then
    echo "  [ok]      rivet-client binary ($CLIENT_BIN)"
  else
    echo "  [MISSING] rivet-client binary — build it first (cd tools/rivet-client && cargo build --locked) or set RIVET_CLIENT_BIN"
    missing=$((missing + 1))
  fi

  # Java 25 JDK for the reference oracle (rivet-parity compiles against Paper via
  # run.sh) — mirror run.sh's discovery order.
  local javac25="" h mac_home
  for h in "${RIVET_JAVA_HOME:-}" "${JAVA_HOME:-}" "${SDKMAN_CANDIDATES_DIR:-$HOME/.sdkman/candidates}/java/current"; do
    if [ -n "$h" ] && [ -x "$h/bin/javac" ] && [[ "$("$h/bin/javac" -version 2>&1)" == "javac 25"* ]]; then
      javac25="$h/bin/javac"
      break
    fi
  done
  if [ -z "$javac25" ] && [ "$(uname -s)" = "Darwin" ] && [ -x /usr/libexec/java_home ]; then
    mac_home="$(/usr/libexec/java_home -v 25 2>/dev/null || true)"
    if [ -n "$mac_home" ] && [ -x "$mac_home/bin/javac" ] && [[ "$("$mac_home/bin/javac" -version 2>&1)" == "javac 25"* ]]; then
      javac25="$mac_home/bin/javac"
    fi
  fi
  if [ -z "$javac25" ] && command -v javac >/dev/null 2>&1 && [[ "$(javac -version 2>&1)" == "javac 25"* ]]; then
    javac25="$(command -v javac)"
  fi
  if [ -n "$javac25" ]; then
    JAVAC25_OK=1
    echo "  [ok]      Java 25 JDK for the reference oracle ($("$javac25" -version 2>&1))"
  else
    echo "  [MISSING] Java 25 JDK (javac 25) for the reference oracle — set RIVET_JAVA_HOME / JAVA_HOME, use SDKMAN, or install a Temurin 25 JDK on PATH"
    missing=$((missing + 1))
  fi

  # Paper compile jar (rivet-parity's run.sh compiles the reference oracle against it).
  if [ -n "${RIVET_PAPER_JAR:-}" ] && [ -f "${RIVET_PAPER_JAR}" ]; then
    COMPILE_JAR="$RIVET_PAPER_JAR"
  else
    for c in "$REPO_DIR"/working/Paper/paper-server/build/libs/paper-server-*.jar; do
      if [ -f "$c" ]; then
        COMPILE_JAR="$c"
        break
      fi
    done
  fi
  if [ -n "$COMPILE_JAR" ]; then
    echo "  [ok]      Paper compile jar ($(basename "$COMPILE_JAR"))"
  else
    echo "  [MISSING] Paper compile jar (paper-server-*.jar) — build working/Paper or set RIVET_PAPER_JAR"
    missing=$((missing + 1))
  fi

  # Materialized Paper runtime: the libraries dir and the runtime jar beside it.
  if [ -n "${RIVET_PAPER_LIBRARIES:-}" ] && [ -d "${RIVET_PAPER_LIBRARIES}" ]; then
    LIBRARIES_DIR="$RIVET_PAPER_LIBRARIES"
  elif [ -d "$REPO_DIR/tools/rivet-oracle/work/run/libraries" ]; then
    LIBRARIES_DIR="$REPO_DIR/tools/rivet-oracle/work/run/libraries"
  fi
  if [ -n "$LIBRARIES_DIR" ]; then
    echo "  [ok]      materialized runtime libraries ($LIBRARIES_DIR)"
  else
    echo "  [MISSING] materialized runtime libraries — boot the M0 Paper fixture server once (tools/rivet-oracle/README.md) or set RIVET_PAPER_LIBRARIES"
    missing=$((missing + 1))
  fi

  if [ -n "${RIVET_PAPER_RUNTIME_JAR:-}" ] && [ -f "${RIVET_PAPER_RUNTIME_JAR}" ]; then
    RUNTIME_JAR="$RIVET_PAPER_RUNTIME_JAR"
  elif [ -n "$LIBRARIES_DIR" ] && [ -f "${LIBRARIES_DIR%/*}/versions/26.2/paper-26.2.jar" ]; then
    RUNTIME_JAR="${LIBRARIES_DIR%/*}/versions/26.2/paper-26.2.jar"
  fi
  if [ -n "$RUNTIME_JAR" ]; then
    echo "  [ok]      materialized runtime jar ($RUNTIME_JAR)"
  else
    echo "  [MISSING] materialized runtime jar (versions/26.2/paper-26.2.jar beside the libraries dir) — boot Paper once or set RIVET_PAPER_RUNTIME_JAR"
    missing=$((missing + 1))
  fi

  if [ "$JAVA_BARE_OK" = 1 ] && [ "$PYTHON3_OK" = 1 ] && [ "$DISK_OK" = 1 ] && [ -n "$PAPERCLIP_JAR" ]; then
    VERIFY_RUNNABLE=1
  fi
  # The join-capture harness boots Paper (java + paperclip) AND drives the
  # Azalea client binary.
  if [ "$JAVA_BARE_OK" = 1 ] && [ "$DISK_OK" = 1 ] && [ -n "$PAPERCLIP_JAR" ] && [ -n "$CLIENT_BIN" ]; then
    CAPTURE_RUNNABLE=1
  fi
  if [ "$JAVAC25_OK" = 1 ] && [ -n "$COMPILE_JAR" ] && [ -n "$LIBRARIES_DIR" ] && [ -n "$RUNTIME_JAR" ]; then
    PARITY_RUNNABLE=1
  fi

  echo
  if [ "$missing" -gt 0 ]; then
    echo "  $missing oracle prerequisite(s) MISSING"
    if [ "$REQUIRE_ORACLE" = 1 ]; then
      echo "  --require-oracle is set: oracle verification is mandatory, so the gate stops here."
      return 1
    fi
    echo "  The oracle steps below will report UNVERIFIED (the gate still exits nonzero, code $ORACLE_EXIT_UNVERIFIED)."
  else
    echo "  all oracle prerequisites present"
  fi
  return 0
}

# ---- oracle steps (full gate only) -------------------------------------------

# NOTE: oracle verify PASS depends on the local working/Paper matching the commit
# pinned in tools/rivet-oracle/fixtures/manifest.json (26.2-DEV-main@0a99345). verify
# ENFORCES that pin: after the boot it compares the Git-Commit attribute of the
# server jar the paperclip actually materialized (work/verify/run/versions/26.2/
# paper-26.2.jar) to the manifest's `paper` provenance and fails loudly on
# mismatch/unavailable — a stale Paper never passes green. If working/Paper
# advances, regenerate the fixtures (scripts/extract_fixtures.py) and re-pin the
# manifest before relying on this step again. `verify --expect-fail` (run as the
# negative-control stage below) is the same pipeline's negative test: it diffs a
# fresh boot against a deliberately corrupted temp copy of the baseline and
# requires the tampered chunk to be detected and named — proving the
# boot->extract->diff chain is not vacuously green. A nonzero exit aborts the
# gate like any other oracle stage; the tamper never touches the committed
# fixtures.
run_oracle_verify() {
  echo "==> oracle verify (M0 sanity gate: green against vanilla itself)"
  if [ "$VERIFY_RUNNABLE" = 1 ]; then
    cargo run -q -p rivet-oracle -- verify
    echo "    VERIFIED — fresh Paper boot is byte-identical to the committed golden baseline"
    echo "==> oracle negative control (verify --expect-fail: detects tamper)"
    cargo run -q -p rivet-oracle -- verify --expect-fail
  else
    echo "    UNVERIFIED — oracle verify did not run (see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

run_rivet_parity() {
  echo "==> rivet-parity (byte-for-byte NBT/SNBT vs Paper oracle)"
  if [ "$PARITY_RUNNABLE" = 1 ]; then
    # run.sh enforces the compile-jar == runtime-jar SHA-256 and Paper commit ==
    # manifest pin itself (exit 1 on mismatch); the prereq pre-check above only
    # confirmed the artifacts exist. So a present-but-stale Paper runtime jar can
    # still make the oracle fail to boot here. The tool's exit code is the
    # machine-stable status — no stderr-text inference:
    #   0 = VERIFIED  (oracle booted and ran, no hard mismatches)
    #   1 = FAILED    (oracle ran; parity genuinely diverged)
    #   3 = UNVERIFIED (oracle did not boot / did not run)
    #   any other nonzero (e.g. a panic) = tool failure -> FAILED, never green.
    # We always run with --require-oracle so a dead oracle exits 3 immediately
    # instead of degrading to Rust-only checks that would mask the failure.
    local rc=0 tmp
    tmp="$(mktemp)"
    # `|| rc=$?` (not set +e) so errexit stays on for every other statement and
    # the failed command's real exit status is captured in $rc.
    cargo run -q -p rivet-parity -- --require-oracle 2>"$tmp" || rc=$?
    cat "$tmp" >&2
    if [ "$rc" -eq 0 ]; then
      echo "    VERIFIED — rivet-nbt byte-for-byte parity with Paper (within documented divergences)"
    elif [ "$rc" -eq 1 ]; then
      # HARD MISMATCHES: the oracle ran, so this is a real parity divergence,
      # not an infrastructure problem. Never reported as UNVERIFIED.
      echo "    FAILED — rivet-parity found hard mismatches vs Paper (exit $rc; see the output above)"
      rm -f "$tmp"
      exit 1
    elif [ "$rc" -eq 3 ]; then
      # UNVERIFIED: the oracle could not boot, so nothing was compared against
      # Paper (stale runtime jar, missing prereq, etc.).
      echo "    UNVERIFIED — rivet-parity did not exercise the oracle (exit $rc; see the output above)"
      ORACLE_UNVERIFIED=1
      if [ "$REQUIRE_ORACLE" = 1 ]; then
        echo "    --require-oracle is set: an oracle that cannot boot is a hard failure"
        rm -f "$tmp"
        exit 1
      fi
    else
      # Tool crash / panic / unexpected error: FAILED, never green.
      echo "    FAILED — rivet-parity crashed or errored (exit $rc; see the output above)"
      rm -f "$tmp"
      exit 1
    fi
    rm -f "$tmp"
  else
    echo "    UNVERIFIED — rivet-parity did not run (see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

# NOTE: the join-capture gate (rivet-capture verify) enforces the Paper pin in
# tools/rivet-capture/fixtures/join/manifest.json (26.2-DEV-main@0a99345) the
# same way oracle verify enforces the rivet-oracle pin: after the boot it
# compares the Git-Commit attribute of the server jar the paperclip actually
# materialized to the fixture's `paper` provenance and fails loudly on
# mismatch/unavailable. `verify --expect-fail` (run as the negative-control
# stage below) corrupts a copy of the committed join fixture and requires the
# tampered packet to be detected AND named — proving the capture->normalize->
# byte-compare chain is not vacuously green.
run_join_capture() {
  echo "==> join capture (rivet-capture verify: byte-identity against vanilla join)"
  if [ "$CAPTURE_RUNNABLE" = 1 ]; then
    cargo run -q -p rivet-capture -- verify
    echo "    VERIFIED — fresh Paper join is byte-identical to the committed join fixture"
    echo "==> join capture negative control (rivet-capture verify --expect-fail: detects tamper)"
    cargo run -q -p rivet-capture -- verify --expect-fail
  else
    echo "    UNVERIFIED — join capture did not run (see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

# ---- main --------------------------------------------------------------------

main() {
  export PATH="$HOME/.cargo/bin:$PATH"
  cd "$REPO_DIR"

  # --- flags & scope resolution ----------------------------------------------
  local SCOPE_ARGS=()
  for a in "$@"; do
    case "$a" in
      --require-oracle) REQUIRE_ORACLE=1 ;;
      *) SCOPE_ARGS+=("$a") ;;
    esac
  done
  if [ -n "${RIVET_REQUIRE_ORACLE:-}" ] && [ "$RIVET_REQUIRE_ORACLE" != "0" ]; then
    REQUIRE_ORACLE=1
  fi
  if [ -n "${SCOPE:-}" ]; then
    # shellcheck disable=SC2206
    SCOPE_ARGS+=($(printf '%s' "$SCOPE" | tr ',' ' '))
  fi

  local PKGS=() PKG_FLAGS=()
  if [ ${#SCOPE_ARGS[@]} -gt 0 ]; then
    for a in "${SCOPE_ARGS[@]}"; do
      a="${a#crates/}"; a="${a%/}"
      [ -n "$a" ] && PKGS+=("$a")
    done
  fi
  # "${arr[@]+...}" guard: bare "${PKGS[@]}" on an empty array errors under
  # `set -u` + bash 3.2 (the default on macOS).
  for p in ${PKGS[@]+"${PKGS[@]}"}; do
    PKG_FLAGS+=(-p "$p")
  done

  local FULL_GATE=false
  if [ ${#PKG_FLAGS[@]} -eq 0 ]; then
    FULL_GATE=true
  fi
  if [ "$REQUIRE_ORACLE" = 1 ] && [ "$FULL_GATE" != true ]; then
    echo "NOTE: --require-oracle is ignored for crate-scoped gates (oracle steps are skipped)"
  fi

  # --- oracle prereq pre-check (full gate only) -------------------------------
  if [ "$FULL_GATE" = true ]; then
    echo "==> oracle prereq pre-check"
    oracle_prereq_check
  fi

  # --- cargo fmt --------------------------------------------------------------
  echo "==> cargo fmt --check"
  if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
    cargo fmt --check "${PKG_FLAGS[@]}"
  else
    cargo fmt --all --check
  fi

  # --- cargo clippy (-Dwarnings) ----------------------------------------------
  echo "==> cargo clippy (-Dwarnings)"
  if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
    RUSTFLAGS=-Dwarnings cargo clippy "${PKG_FLAGS[@]}" --all-targets
  else
    RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets
  fi

  # --- cargo test -------------------------------------------------------------
  echo "==> cargo test"
  if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
    if command -v cargo-nextest >/dev/null 2>&1; then
      cargo nextest run "${PKG_FLAGS[@]}"
    else
      cargo test "${PKG_FLAGS[@]}"
    fi
  elif command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run --workspace
  else
    cargo test --workspace
  fi

  # --- feature-gated generated registry tables (rivet-registry `blocks`) ----------
  # rivet-registry's compile-time block + static-builtin tables (src/generated)
  # and their semantic tests (block_id_tests, static_builtin_tests) live behind
  # the crate's `blocks` cargo feature, so the `cargo test --workspace` step above
  # never builds or executes them. Enable the feature for exactly this one crate
  # and run clippy + tests, so the codegen-owned tables are compiled, linted, and
  # executed on every full merge gate. Never use `--workspace --features blocks`
  # or `--workspace --all-features`: `blocks` is per-crate (and rivet-protocol's
  # `packets` feature would leak in under --all-features). Re-running this one
  # crate's fast tests with the feature is deliberate — cargo cannot run only the
  # feature-gated in-module tests, and the whole crate is a ~0.1s test run.
  # Scoped gates for rivet-registry also get the feature (same gap otherwise).
  local GATE_BLOCKS=false
  for p in ${PKGS[@]+"${PKGS[@]}"}; do
    [ "$p" = "rivet-registry" ] && GATE_BLOCKS=true
  done
  if [ "$FULL_GATE" = true ] || [ "$GATE_BLOCKS" = true ]; then
    echo "==> rivet-registry --features blocks (generated registry tables)"
    RUSTFLAGS=-Dwarnings cargo clippy -p rivet-registry --features blocks --all-targets
    if command -v cargo-nextest >/dev/null 2>&1; then
      cargo nextest run -p rivet-registry --features blocks
    else
      cargo test -p rivet-registry --features blocks
    fi
  fi

  # --- manifest regression suite (full gate only) --------------------------------
  # scripts/test_analyze_graph.py proves MANIFEST.tsv generation is deterministic
  # and conserved (nbt + network + game class-cluster splits, byte-idempotent
  # regeneration, carry of status/attempts/notes, dep resolution, fail-fast on
  # cross-unit duplicate declarations). Requires the real Paper tree under
  # working/ (analyze_graph.py hard-exits if the source roots are absent) —
  # same prerequisite as the oracle steps. Skipped when gating a crate subset —
  # the manifest is a repo-wide artifact, not a workspace crate — same rule as
  # oracle/scenario.
  if [ "$FULL_GATE" = true ]; then
    echo "==> manifest regression suite (scripts/test_analyze_graph.py)"
    python3 scripts/test_analyze_graph.py
  fi

  # --- rivet-codegen (workspace-excluded tool) fmt/clippy/test ------------------
  # tools/rivet-codegen is excluded from the cargo workspace, so --workspace
  # fmt/clippy/test never touch it. Its golden drift test
  # (generate::drift_tests::generated_output_matches_committed) enforces that
  # freshly regenerated output is byte-identical to the committed
  # crates/rivet-registry/src/generated/ — a stale regeneration must fail the
  # gate. Regeneration happens in a temp dir, so committed sources are not
  # mutated. A missing toolchain component or fetch failure here fails loudly
  # under `set -e` rather than being skipped.
  # Skipped when gating a crate subset (scoped gates check a specific workspace
  # crate, not the excluded tool's generated output) — same rule as oracle/scenario.
  if [ "$FULL_GATE" = true ]; then
    echo "==> rivet-codegen (workspace-excluded tool) fmt/clippy/test"
    cargo fmt --manifest-path tools/rivet-codegen/Cargo.toml -- --check
    RUSTFLAGS=-Dwarnings cargo clippy --manifest-path tools/rivet-codegen/Cargo.toml --all-targets
    if command -v cargo-nextest >/dev/null 2>&1; then
      cargo nextest run --manifest-path tools/rivet-codegen/Cargo.toml
    else
      cargo test --manifest-path tools/rivet-codegen/Cargo.toml
    fi

    # --- block-state global-id probe (full gate; needs the Paper bundler) ------
    # GlobalPaletteProbe boots the real Paper block-state registry and
    # cross-checks the emitted global-id table: size 32366,
    # per-block contiguous ranges, defaults in range, and the representative
    # anchor ids. The probe compiles/runs against the full server classpath, so
    # it needs the paper-bundler jar (the artifact of a Paper build, which
    # carries the patched paper-<mc>.jar + libraries) — the tool's own default
    # (tools/rivet-codegen/README.md). The paperclip launcher jar is NOT a
    # substitute: it only ships the patch, not the server jar. When absent the
    # fixture-pinned conformance test still guards the table, so this skips
    # cleanly instead of failing (mirrors the oracle guard).
    local PROBE_BUNDLER=""
    if [ -z "$PROBE_BUNDLER" ] || [ ! -f "$PROBE_BUNDLER" ]; then
      PROBE_BUNDLER="$REPO_DIR/working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar"
    fi
    if [ -z "$PROBE_BUNDLER" ] || [ ! -f "$PROBE_BUNDLER" ]; then
      PROBE_BUNDLER="$REPO_DIR/tools/rivet-oracle/work/jars/paper-bundler-26.2.local-SNAPSHOT.jar"
    fi
    if [ -f "$PROBE_BUNDLER" ]; then
      echo "==> rivet-codegen probe-block-states (live Paper block-state registry)"
      cargo run --release --quiet --manifest-path tools/rivet-codegen/Cargo.toml -- \
        probe-block-states --bundler "$PROBE_BUNDLER"
    else
      echo "    SKIPPED (no Paper bundler jar: build working/Paper (paper-bundler-*.jar) or place it in tools/rivet-oracle/work/jars/)"
    fi
  fi

  # --- oracle steps (full gate only; the oracle verifies the whole server) -----
  if [ "$FULL_GATE" = true ]; then
    run_oracle_verify
    run_rivet_parity
    run_join_capture
  fi

  # --- scenario runner (full gate only; M0 join harness: Paper-vs-Paper + negative case) --
  # The Paper-vs-Paper join harness boots local Paper twice, joins each with the
  # Azalea headless client, and requires identical normalized transcripts, plus a
  # negative case proving the comparator detects a tampered position. The runner
  # also runs its own unit tests first (port isolation, ServerKind, process-
  # lifecycle cleanup, exit-code classification — issue #155). Runs only
  # when a paperclip jar is materialized (same guard style as oracle verify);
  # skipped when gating a crate subset (the scenario drives a whole server).
  if [ "$FULL_GATE" = true ]; then
    echo "==> scenario runner (join: Paper-vs-Paper + negative case)"
    if [ -n "${RIVET_ORACLE_JAR:-}" ] || [ -f "$REPO_DIR/tools/rivet-client/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar" ] || \
       ls "$REPO_DIR"/working/Paper/paper-server/build/libs/paper-paperclip*.jar >/dev/null 2>&1; then
      "$REPO_DIR/tools/rivet-client/run-scenario.sh" join
    else
      echo "    SKIPPED (no paperclip jar: set RIVET_ORACLE_JAR or materialize working/Paper first)"
    fi
  fi

  # --- unused dependencies (cargo-machete) -------------------------------------
  # machete stays workspace-wide (even on scoped gates); also cover the
  # workspace-excluded codegen tool's own manifest.
  echo "==> cargo machete (unused deps)"
  if ! command -v cargo-machete >/dev/null 2>&1; then
    echo "    cargo-machete not found; installing (cargo install cargo-machete --locked)"
    cargo install cargo-machete --locked
  fi
  cargo machete
  cargo machete tools/rivet-codegen

  # --- final verdict ------------------------------------------------------------
  if [ "$ORACLE_UNVERIFIED" = 1 ]; then
    echo
    echo "GATE: ORACLE UNVERIFIED — the Rivet-vs-Paper comparison did not run to completion, so"
    echo "      nothing above checked real Rivet code against Paper. See the prereq report for"
    echo "      what was missing and how to fix it, or pass --require-oracle to make this a hard"
    echo "      failure instead."
    exit "$ORACLE_EXIT_UNVERIFIED"
  fi
  echo "GATE GREEN"
}

# Run only when executed directly; sourcing this file just defines the functions,
# so tests can drive oracle_prereq_check in isolation.
if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
