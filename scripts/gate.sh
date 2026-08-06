#!/bin/bash
# The merge gate. Run before merging any PR (and at the end of every wave).
# No hosted CI by design — this script IS the gate; a red gate blocks the merge.
#
# Scope: pass crate names or crates/... paths as arguments (or set SCOPE to a
# space/comma-separated list) to gate only those crates (fmt/clippy/test). The
# unused-deps check (cargo machete) and the default full gate stay workspace-wide.
#
#   ./scripts/gate.sh                     # full gate: fmt, clippy, tests, oracle, scenario, machete
#   ./scripts/gate.sh crates/rivet-nbt     # fmt+clippy+test for rivet-nbt only
# The full gate also runs both oracle steps against the Paper Java oracle, plus the
# scenario runner:
#   - rivet-oracle verify  M0 sanity gate: boot a fresh Paper server and diff its
#                          chunk-NBT slice against the committed golden baseline.
#   - rivet-parity         byte-for-byte NBT/SNBT diff of rivet-nbt against the Paper
#                          reference oracle — the only gate step that exercises real
#                          Rivet code against Paper.
#   - scenario runner      join: boots Paper twice via the Azalea client and requires
#                          identical normalized transcripts, plus a negative case.
#                          Guarded by the paperclip jar, like oracle verify.
# Oracle verification is never silently skipped. When its prerequisites are missing
# the pre-check below names each missing item with a fix, the steps report UNVERIFIED,
# and the gate exits with a distinct nonzero code (3) — an unverified merge never looks
# green. Pass --require-oracle (or RIVET_REQUIRE_ORACLE=1) to make any missing oracle
# prereq a hard failure (exit 1) right at the pre-check. The Paper jar SHA-256 / git-commit
# pin guards live in tools/rivet-reference-oracle/run.sh (compile jar == runtime jar SHA,
# Paper commit == manifest pin, exit 1 on mismatch) and stay authoritative; the pre-check
# only validates that the prerequisites exist. Because a present-but-stale runtime jar
# passes the pre-check yet still fails to boot, the rivet-parity step relies on the tool's
# machine-stable exit code — 0 VERIFIED, 1 FAILED, 3 UNVERIFIED — to classify the run.
# We always pass --require-oracle so a dead oracle exits 3 immediately and never degrades
# to a Rust-only run that could be mistaken for a green.
#
#   ./scripts/gate.sh                        # full gate: fmt, clippy, tests, oracle, scenario, machete
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
  # VERIFY_RUNNABLE / PARITY_RUNNABLE are globals (the step runners read them).
  VERIFY_RUNNABLE=0; PARITY_RUNNABLE=0

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
# pinned in tools/rivet-oracle/fixtures/manifest.json (26.2-DEV-main@0a99345). If
# working/Paper advances, regenerate the fixtures (scripts/extract_fixtures.py)
# and re-pin the manifest before relying on this step again.
run_oracle_verify() {
  echo "==> oracle verify (M0 sanity gate: green against vanilla itself)"
  if [ "$VERIFY_RUNNABLE" = 1 ]; then
    cargo run -q -p rivet-oracle -- verify
    echo "    VERIFIED — fresh Paper boot is byte-identical to the committed golden baseline"
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

  # --- oracle steps (full gate only; the oracle verifies the whole server) -----
  if [ "$FULL_GATE" = true ]; then
    run_oracle_verify
    run_rivet_parity
  fi

  # --- scenario runner (full gate only; M0 join harness: Paper-vs-Paper + negative case) --
  # The Paper-vs-Paper join harness boots local Paper twice, joins each with the
  # Azalea headless client, and requires identical normalized transcripts, plus a
  # negative case proving the comparator detects a tampered position. Runs only
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
  echo "==> cargo machete (unused deps)"
  if ! command -v cargo-machete >/dev/null 2>&1; then
    echo "    cargo-machete not found; installing (cargo install cargo-machete --locked)"
    cargo install cargo-machete --locked
  fi
  cargo machete

  # --- final verdict ------------------------------------------------------------
  if [ "$ORACLE_UNVERIFIED" = 1 ]; then
    echo
    echo "GATE: ORACLE UNVERIFIED — the oracle steps did not run, so nothing above compared Rivet"
    echo "      against Paper. See the prereq report for what was missing and how to fix it, or"
    echo "      pass --require-oracle to make this a hard failure instead."
    exit "$ORACLE_EXIT_UNVERIFIED"
  fi
  echo "GATE GREEN"
}

# Run only when executed directly; sourcing this file just defines the functions,
# so tests can drive oracle_prereq_check in isolation.
if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
