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
# The full gate also runs the oracle steps against the Paper Java oracle, plus the
# scenario runner:
#   - rivet-oracle (default)  verifies ALL committed fixture kinds: the M0 chunk
#                          slice, the M2 worldgen semantic samples, the M2
#                          normal-overworld none-compression region payloads,
#                          the text component-JSON corpus (issue #98), and the
#                          spline value-leaf goldens (issue #372)
#                          (each against its own manifest.json SHA-256s).
#   - rivet-oracle verify  M0 sanity gate: boot a fresh Paper server and diff its
#                          chunk-NBT slice against the committed golden baseline.
#                          Also runs verify --expect-fail, the negative control:
#                          a fresh boot diffed against a corrupted temp baseline
#                          copy that must be detected and named (proves the
#                          boot->extract->diff chain is not vacuously green).
#   - rivet-oracle verify --m2  M2 region gate: boot a fresh normal-overworld
#                          server (region-file-compression=none, D13) and diff its
#                          chunk-NBT slice against the committed region baseline
#                          (proves two fresh boots match). Also runs the
#                          --m2 --expect-fail negative control.
#                          Every oracle boot is pinned to chunk-system 1 worker/1
#                          I/O thread (issue #266): fixtures/paper-global.yml is
#                          copied into the run dir, the boot log must report
#                          exactly "1 worker threads, 1 I/O threads", and the M2
#                          region baseline must carry matching chunk-concurrency
#                          provenance — drift fails the gate loudly.
#   - reference self-test  boot the reference oracle in --self-test mode and assert
#                          stdout is exactly one bare JSON line (no log4j [STDOUT]
#                          prefix, not swallowed) — the load-bearing proof that
#                          selfTest() rides the raw stream. Shares the parity prereqs.
#                          A booted oracle with broken raw stdout is FAILED; any
#                          nonzero run.sh exit is conservatively UNVERIFIED (exit 3) —
#                          the wrapper's single exit channel cannot distinguish a
#                          boot/prereq failure (present-but-stale runtime jar, missing
#                          prereq) from a JVM self-test assertion failure (run.sh
#                          exec's the JVM, whose exit status passes straight through) —
#                          and --require-oracle makes any nonzero exit a hard failure
#                          (exit 1), mirroring rivet-parity's rc=3 handling.
#   - rivet-oracle verify --full  M2 FULL region gate (issue #51): boot a fresh
#                          superflat server (region-file-compression=none, D13) and
#                          diff its chunk-NBT slice against the committed status-FULL
#                          region baseline — the corpus-forced, twin-boot-captured
#                          superflat full-status chunks. The capture injects
#                          level-33 forced tickets for every corpus coordinate into
#                          each dimension, so all 8 corpus coordinates per dimension
#                          reach `minecraft:full`; LastUpdate is normalized to 0
#                          (save-clock artifact). Also runs the --full --expect-fail
#                          negative control.
#   - rivet-parity         byte-for-byte NBT/SNBT diff of rivet-nbt against the Paper
#                          reference oracle — the only gate step that exercises real
#                          Rivet code against Paper.
#   - scenario runner      join: boots Paper twice via the Azalea client and requires
#                          identical normalized transcripts, plus a negative case.
#                          join --server both / move --server both run the Rivet-vs-Paper
#                          differentials (issues #192/#53): the same client boots each server
#                          and must produce equal normalized transcripts, proving Rivet's
#                          join and authoritative-walk behavior matches Paper's. dwell boots
#                          Rivet headlessly and proves the client survives past the 30 s
#                          keepalive kick limit (wall-clock, echoing every live keepalive).
#                          The Paper rows are guarded by the paperclip jar, like oracle
#                          verify; dwell/kick/loaded-world/recenter/generated-world need
#                          only the rivet-server binary (run-scenario.sh builds it on
#                          demand; the loaded-world and generated-world rows additionally
#                          need the rivet-oracle and rivet-client binaries the harness
#                          resolves). Each row exits 0 PASS / 1 FAIL / 3 UNVERIFIED
#                          (never silently green).
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
SCENARIO_RUNNABLE=0

# Directory of this script via bash builtins only (no external tools at load
# time — sourcing must stay side-effect-free so tests can shim PATH).
_script_dir="${BASH_SOURCE[0]%/*}"
[ "$_script_dir" = "${BASH_SOURCE[0]}" ] && _script_dir="."
REPO_DIR="$(cd "$_script_dir/.." && pwd)"

# shellcheck source=scripts/cargo-target-dir.sh
source "$REPO_DIR/scripts/cargo-target-dir.sh"

resolved_target_dir_for() {
  # All workspaces use the same absolute target selected by the lock wrapper;
  # resolving from the git common directory also works before Cargo has run.
  cargo_target_dir_for "$REPO_DIR"
}

# ---- oracle prereq pre-check (full gate only) --------------------------------
#
# Validates the prerequisites the two oracle steps need and prints an actionable
# [ok] / [MISSING] report. Sets the per-step runnability flags VERIFY_RUNNABLE and
# PARITY_RUNNABLE that the step runners consume. Honours the same env overrides the
# tools honour: RIVET_ORACLE_JAR; RIVET_PAPER_JAR / RIVET_PAPER_LIBRARIES /
# RIVET_PAPER_RUNTIME_JAR; RIVET_JAVA_HOME / JAVA_HOME / SDKMAN. With
# REQUIRE_ORACLE=1 any missing prereq is a hard failure (exit 1).
oracle_prereq_check() {
  local missing=0 client_target_dir client_candidate
  JAVA_BARE_OK=0; PYTHON3_OK=0; DISK_OK=0; JAVAC25_OK=0
  PAPERCLIP_JAR=""; COMPILE_JAR=""; LIBRARIES_DIR=""; RUNTIME_JAR=""
  # VERIFY_RUNNABLE / PARITY_RUNNABLE / CAPTURE_RUNNABLE / SCENARIO_RUNNABLE are
  # globals (the step runners read them).
  VERIFY_RUNNABLE=0; PARITY_RUNNABLE=0; CAPTURE_RUNNABLE=0; SCENARIO_RUNNABLE=0

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
  CLIENT_BIN=""
  if [ -n "${RIVET_CLIENT_BIN:-}" ] && [ -f "${RIVET_CLIENT_BIN}" ]; then
    CLIENT_BIN="$RIVET_CLIENT_BIN"
  else
    client_target_dir="$(resolved_target_dir_for "$REPO_DIR/tools/rivet-client/Cargo.toml")"
    client_candidate="$client_target_dir/debug/rivet-client"
    if [ -f "$client_candidate" ]; then
      CLIENT_BIN="$client_candidate"
    fi
  fi
  if [ -n "$CLIENT_BIN" ]; then
    echo "  [ok]      rivet-client binary ($CLIENT_BIN)"
  else
    echo "  [MISSING] rivet-client binary — build it first (cd tools/rivet-client && cargo build --locked) or set RIVET_CLIENT_BIN"
    missing=$((missing + 1))
  fi

  # paperclip jar for the scenario runner's Paper rows (join/move Paper-vs-Rivet
  # differentials). The scenario harness resolves it through its own ensure_jar
  # discovery — RIVET_ORACLE_JAR, tools/rivet-client/work/jars/, then a
  # paper-paperclip-*.jar under working/Paper — so the gate mirrors exactly those
  # locations (RIVET_ORACLE_JAR is already handled by the PAPERCLIP_JAR check
  # above). The Paper rows must never run against a missing jar, and equally must
  # never silently skip: SCENARIO_RUNNABLE is consumed by run_scenario_paper_rows
  # to report UNVERIFIED (or hard-fail under --require-oracle) instead of the bare
  # SKIPPED the pre-#160 gate printed.
  SCENARIO_PAPERCLIP_JAR=""
  if [ -n "$PAPERCLIP_JAR" ]; then
    # A verified paperclip (oracle jars/working) is exactly the bundler the
    # scenario harness boots, so reuse it rather than re-discovering.
    SCENARIO_PAPERCLIP_JAR="$PAPERCLIP_JAR"
  elif [ -f "$REPO_DIR/tools/rivet-client/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar" ]; then
    SCENARIO_PAPERCLIP_JAR="$REPO_DIR/tools/rivet-client/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar"
  else
    for c in "$REPO_DIR"/working/Paper/paper-server/build/libs/paper-paperclip-*.jar; do
      if [ -f "$c" ]; then
        SCENARIO_PAPERCLIP_JAR="$c"
        break
      fi
    done
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
  # The scenario runner's Paper rows (join --server both / move --server both)
  # boot Paper (java + paperclip) AND drive the client binary (the harness's
  # run-scenario.sh builds the rivet-server on demand, so no server binary is a
  # prereq here). The Paper rows therefore run exactly when the paperclip jar is
  # present and the client binary exists.
  if [ "$JAVA_BARE_OK" = 1 ] && [ "$DISK_OK" = 1 ] && [ -n "$SCENARIO_PAPERCLIP_JAR" ] && [ -n "$CLIENT_BIN" ]; then
    SCENARIO_RUNNABLE=1
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
  echo "==> oracle verify (all committed fixture kinds: M0 slice + worldgen samples + M2 regions + text corpus + spline + composed-noise + surface-column)"
  cargo run -q -p rivet-oracle
  if [ "$VERIFY_RUNNABLE" = 1 ]; then
    echo "==> oracle verify (M0 sanity gate: green against vanilla itself)"
    cargo run -q -p rivet-oracle -- verify
    echo "    VERIFIED — fresh Paper boot is byte-identical to the committed golden baseline"
    echo "==> oracle negative control (verify --expect-fail: detects tamper)"
    cargo run -q -p rivet-oracle -- verify --expect-fail
    echo "==> oracle verify (M2 region gate: normal-overworld none-compression region parity)"
    cargo run -q -p rivet-oracle -- verify --m2
    echo "    VERIFIED — fresh normal-overworld boot is byte-identical to the committed region baseline"
    echo "==> oracle negative control (verify --m2 --expect-fail: detects tamper)"
    cargo run -q -p rivet-oracle -- verify --m2 --expect-fail
    echo "==> oracle verify (M2 FULL region gate: superflat status-FULL region capture, issue #51)"
    cargo run -q -p rivet-oracle -- verify --full
    echo "    VERIFIED — fresh corpus-forced superflat boot is byte-identical to the committed status-FULL region baseline"
    echo "==> oracle negative control (verify --full --expect-fail: detects tamper)"
    cargo run -q -p rivet-oracle -- verify --full --expect-fail
  else
    echo "    UNVERIFIED — oracle verify did not run (see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

# The #54 chunk-hash engine (xxh3_64 seed-hash gate). Unlike the other oracle
# stages it does not boot Paper: `hash-self-check` pins the xxh3_64 known-answer
# vectors (a wrong variant/endianness fails loudly, never silently corrupting
# digests), and `hash-paper` rebuilds the committed Paper manifest from the
# committed M2 region payloads — which must be git-clean, proving the FULL-status
# stamping and digest table are deterministic.
#
# The Paper-vs-Rivet `hash-diff` needs FULL chunks at every corpus coordinate on
# both sides. Pre-worldgen there is no Rivet FULL serialization (#231/#15; #51
# must capture status-FULL regions), so with RIVET_HASH_DIR unset (the default)
# the gate records an explicit NOTICE and stays mergeable — it never runs Paper
# against Paper (a self-diff proves nothing about Rivet, and `hash-diff` refuses
# one), and it never claims parity it does not have. This is a milestone-gated
# comparison, not an oracle prereq: an absent comparison is not ORACLE_UNVERIFIED
# and does not fail the gate. Setting RIVET_HASH_DIR opts into the strict check —
# the comparison is then required, and any UNVERIFIED (incomplete corpus coverage
# or a Paper-vs-Paper self-diff) or FAILED divergence is gate-fatal, never a
# silent or vacuous green.
run_oracle_hash() {
  echo "==> chunk-hash engine (issue #54: xxh3_64 seed-hash gate)"
  echo "==> oracle hash self-check (xxh3_64 known-answer vectors)"
  cargo run -q -p rivet-oracle -- hash-self-check
  echo "    VERIFIED — xxh3_64 matches the pinned known-answer vectors"
  echo "==> oracle hash-paper (rebuild committed Paper manifest; must be git-clean)"
  cargo run -q -p rivet-oracle -- hash-paper
  # The byte-identity check needs a git work tree to compare the rebuild against
  # the committed manifest. The gate shell tests drive gate.sh from a non-git
  # sandbox (cargo stubbed), where the manifest cannot be tracked; there the
  # check is recorded as a NOTICE — never a silent skip or a false abort.
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    if ! git diff --exit-code -- tools/rivet-oracle/fixtures/chunk-hash/paper/manifest.json; then
      echo "    FAILED — committed Paper hash manifest drifted from a fresh rebuild; regenerate and commit it"
      exit 1
    fi
    # Narrate the FULL facts from the live manifest, never a hardcoded count (a
    # manifest whose FULL set changes must be caught here, not stale-narrated).
    local full_narration
    full_narration="$(python3 -c '
import json, sys
m = json.load(open(sys.argv[1]))
full = sorted((e["dim"], e["cx"], e["cz"]) for e in m["entries"] if e["status"] == "minecraft:full")
print(f"{len(full)} FULL: " + ", ".join(f"{d}/{cx}.{cz}" for d, cx, cz in full))
' "$REPO_DIR/tools/rivet-oracle/fixtures/chunk-hash/paper/manifest.json")"
    echo "    VERIFIED — Paper manifest rebuilds byte-identically ($full_narration)"
  else
    echo "    NOTICE — skipped the Paper manifest git-clean check (not inside a git work tree)"
  fi
  local paper_dir="$REPO_DIR/tools/rivet-oracle/fixtures/chunk-hash/paper"
  local rivet_dir="${RIVET_HASH_DIR:-}"
  if [ -z "$rivet_dir" ]; then
    # No Rivet chunk manifest exists yet. Record the milestone gap honestly and
    # move on — this is the documented pre-worldgen state, not an oracle failure.
    echo "    NOTICE — Paper-vs-Rivet hash-diff did not run: no Rivet chunk manifest yet"
    echo "      (pre-worldgen, #231/#15 serialization + #51 status-FULL capture pending;"
    echo "      set RIVET_HASH_DIR to a Rivet region tree to enable the comparison)"
    return 0
  fi
  # `hash-diff` itself refuses a Paper-vs-Paper self-diff (a self-comparison can
  # never imply Paper == Rivet parity); if RIVET_HASH_DIR aliases the paper tree,
  # surface that as a hard failure rather than a silent UNVERIFIED.
  local rc=0
  cargo run -q -p rivet-oracle -- hash-diff "$paper_dir" "$rivet_dir" 2>&1 || rc=$?
  if [ "$rc" -eq 3 ]; then
    # RIVET_HASH_DIR was set, so the comparison was requested and could not
    # complete (incomplete corpus coverage, or a self-diff if it aliases the
    # paper tree). A green gate must not claim M2 parity it does not have.
    echo "    UNVERIFIED — Paper-vs-Rivet hash-diff could not complete"
    echo "      (needs FULL chunks at every corpus coordinate on both sides; #51 + #231/#15 pending)"
    exit 1
  elif [ "$rc" -eq 0 ]; then
    echo "    VERIFIED — Paper-vs-Rivet FULL digests match across the corpus matrix"
    echo "==> oracle hash-diff --expect-fail (tamper negatives: every mutation class must be detected)"
    cargo run -q -p rivet-oracle -- hash-diff --expect-fail "$paper_dir" "$rivet_dir" all
    echo "    VERIFIED — block/light/heightmap/NBT-order tampering all detected and named"
  else
    echo "    FAILED — hash-diff exited $rc (see the output above)"
    exit 1
  fi
}

# Returns 0 iff stdin is exactly the bare JSON-Lines summary the oracle
# self-test must emit: a single JSON object whose top-level "ok" is the boolean
# true, with no trailing blank line. Reads RAW bytes from stdin (NOT a $()
# capture, which strips trailing newlines) so the line-count contract is
# checkable. Parsed structurally with python3 (NOT a substring scan — a
# substring glob would accept a nested `{"ok":true}` with no top-level ok, and
# a top-level "ok":false / "ok":"true" / "ok":1 must all be rejected).
#
# The raw-stdout shape is the load-bearing contract: the summary must NOT be a
# log4j `[HH:mm:ss LEVEL]: [STDOUT]: ...` prefix — Bootstrap.bootStrap()
# re-wires System.out through SysOutOverSLF4J, so a regression of selfTest()
# back to System.out.println either prefixes the summary or swallows it
# entirely. Empty stdout, a `[`-prefixed log line, extra lines, a trailing
# blank line, or leading/trailing whitespace around the JSON object means the
# protocol broke — the line must be *exactly* the bare JSON object.
oracle_self_test_stdout_is_raw_json() {
  python3 -c '
import json, sys
raw = sys.stdin.buffer.read()
line = raw.rstrip(b"\n")
if len(raw) - len(line) > 1:
    sys.exit(1)          # trailing blank line (more than the single terminator)
if b"\n" in line:
    sys.exit(1)          # more than one line of content
if line != line.strip():
    sys.exit(1)          # leading/trailing whitespace around the JSON object
if not line:
    sys.exit(1)          # empty stdout
try:
    data = json.loads(line.decode("utf-8"))
except ValueError:
    sys.exit(1)
if not isinstance(data, dict) or data.get("ok") is not True:
    sys.exit(1)
'
}

# The reference oracle's `--self-test` mode (RivetReferenceOracle.selfTest())
# must emit exactly one bare JSON line on stdout. selfTest() writes through
# RAW_STDOUT — the System.out captured before Bootstrap.bootStrap() re-wires it
# through log4j — precisely so the summary is not a `[HH:mm:ss LEVEL]: [STDOUT]:
# {...}` log line (or swallowed). Asserting the real stdout shape is the
# load-bearing proof that a regression back to System.out.println fails the
# gate; it shares the parity step's prerequisites (the compiled oracle boots
# against the same Paper runtime), so it is guarded by PARITY_RUNNABLE, and the
# JSON verdict is parsed with python3 (already a gate prereq for `verify`).
#
# Classification, mirroring run_rivet_parity's 0/1/3 contract:
#   run.sh exits 0 AND stdout is the bare JSON line  -> VERIFIED
#   run.sh exits 0 but stdout is not the bare JSON   -> FAILED (exit 1): the
#     oracle booted, so a broken raw-stdout protocol is a real self-test
#     failure.
#   run.sh exits nonzero                             -> UNVERIFIED: run.sh's single
#     exit channel cannot tell a boot/prereq failure (a present-but-stale runtime
#     jar fails its SHA/commit pin, a prereq is missing) from a JVM self-test
#     assertion failure — selfTest() throws on a failed assertion and run.sh exec's
#     the JVM, so its nonzero exit passes straight through. Either way the
#     RAW_STDOUT verdict was never observed, so this is a conservative
#     classification, never a proven self-test failure. Sets ORACLE_UNVERIFIED
#     (gate exits 3); with --require-oracle any nonzero exit is a hard failure
#     (exit 1), exactly like rivet-parity's rc=3 handling.
run_oracle_self_test() {
  echo "==> reference oracle self-test (raw JSON Lines on stdout, no log prefix)"
  if [ "$PARITY_RUNNABLE" = 1 ] && command -v python3 >/dev/null 2>&1; then
    local out="" rc=0 tmp
    # Capture stdout to a file (not $() — command substitution strips trailing
    # newlines, so a trailing blank line would be invisible to the checker).
    # run.sh exec's the JVM, so its exit status passes straight through; stderr
    # stays on the terminal so a boot/prereq failure's message is visible.
    tmp="$(mktemp)"
    bash "$REPO_DIR/tools/rivet-reference-oracle/run.sh" --self-test >"$tmp" || rc=$?
    out="$(cat "$tmp")"
    if [ "$rc" -ne 0 ]; then
      rm -f "$tmp"
      echo "    UNVERIFIED — reference oracle --self-test did not yield the raw JSON verdict (exit $rc; see output above)"
      ORACLE_UNVERIFIED=1
      if [ "$REQUIRE_ORACLE" = 1 ]; then
        echo "    --require-oracle is set: a nonzero self-test exit is a hard failure"
        exit 1
      fi
      return 0
    fi
    if ! oracle_self_test_stdout_is_raw_json <"$tmp"; then
      rm -f "$tmp"
      echo "    FAILED — reference oracle --self-test did not emit a bare JSON line (got: ${out:-<empty>})"
      exit 1
    fi
    rm -f "$tmp"
    echo "    VERIFIED — reference oracle --self-test emitted raw JSON Lines ($out)"
  else
    echo "    UNVERIFIED — reference oracle self-test did not run (see the prereq report above)"
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
    echo "==> join capture detector discrimination (rivet-capture verify --mutate <kind>: every injected defect must be detected AND named)"
    for kind in reorder delete insert field canon relabel burst entity-id set-time-absent; do
      cargo run -q -p rivet-capture -- verify --mutate "$kind"
    done
  else
    echo "    UNVERIFIED — join capture did not run (see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

# The scenario runner's Paper-vs-Rivet rows: `join --server both` (#192/#159)
# and `move --server both` (#53). They boot a real Paper server and drive the
# client against both servers, so they need the paperclip jar AND the
# rivet-client binary — SCENARIO_RUNNABLE is set by oracle_prereq_check. Like
# every other oracle step they must never silently skip: with the prereqs
# present they run the differentials (0 PASS / 1 FAIL); with a prereq missing
# they report UNVERIFIED and set ORACLE_UNVERIFIED so the gate exits 3 (and
# --require-oracle hard-fails at the prereq pre-check, exit 1). This replaces
# the pre-#160 bare "SKIPPED" which concealed the missing comparison behind a
# green-looking outcome.
run_scenario_paper_rows() {
  if [ "$SCENARIO_RUNNABLE" = 1 ]; then
    echo "==> scenario runner (join: Paper-vs-Paper + negative case)"
    "$REPO_DIR/tools/rivet-client/run-scenario.sh" join
    echo "==> scenario runner (join: Rivet-vs-Paper differential)"
    "$REPO_DIR/tools/rivet-client/run-scenario.sh" join --server both
    echo "==> scenario runner (move: Rivet-vs-Paper authoritative-walk differential)"
    "$REPO_DIR/tools/rivet-client/run-scenario.sh" move --server both
  else
    echo "    UNVERIFIED — scenario Paper rows (join/move Paper-vs-Rivet) did not run (paperclip jar or rivet-client binary missing; see the prereq report above)"
    ORACLE_UNVERIFIED=1
  fi
}

# The loaded-world acceptance row (issue #374): boot Rivet against a disposable
# copy of the safe world under `working/client-worlds/New World` (never the
# launcher save), extract the read-only ground-truth manifest with
# `rivet-oracle extract-world`, drive the real Azalea client in `loaded` mode,
# and compare the observed per-coordinate block content. It is a Rivet-only
# terminal acceptance (like dwell/kick) — no paperclip jar is a prereq, and
# run-scenario.sh builds the rivet-server binary on demand. Its exit code is
# the machine-stable 0 PASS / 1 FAIL / 3 UNVERIFIED contract. The row must
# never silently skip or look green on a missing prereq: an UNVERIFIED (exit 3)
# sets ORACLE_UNVERIFIED so the gate exits 3, and under --require-oracle it is
# a hard failure (exit 1) — exactly the rivet-parity/self-test boundary.
run_scenario_loaded_world() {
  echo "==> scenario runner (loaded-world: official-client acceptance vs the disposable copied world, issue #374)"
  local rc=0
  "$REPO_DIR/tools/rivet-client/run-scenario.sh" loaded-world || rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "    PASS — the real client joined, spawned, and the served block content matched the ground-truth manifest"
  elif [ "$rc" -eq 1 ]; then
    echo "    FAILED — loaded-world acceptance found a divergence (exit $rc; see the output above)"
    exit 1
  elif [ "$rc" -eq 3 ]; then
    # UNVERIFIED: a prerequisite was missing (copied world, binaries) or the
    # server did not reach READY — the comparison never ran to completion.
    echo "    UNVERIFIED — loaded-world acceptance did not complete (exit $rc; see the output above)"
    ORACLE_UNVERIFIED=1
    if [ "$REQUIRE_ORACLE" = 1 ]; then
      echo "    --require-oracle is set: a loaded-world run that cannot complete is a hard failure"
      exit 1
    fi
  else
    # Tool crash / panic / unexpected error: FAILED, never green.
    echo "    FAILED — loaded-world acceptance crashed or errored (exit $rc; see the output above)"
    exit 1
  fi
}

# The recenter acceptance row (issues #185/#561): boot Rivet against a
# disposable copy of the safe world under `working/client-worlds/New World`
# (never the launcher save), drive the real Azalea client's deterministic +x
# route across repeated chunk boundaries, and REQUIRE the positive
# sustained-walking result — the client stays connected and receives every
# beyond-boot chunk the region-backed recenter loads on demand — plus a
# tampered-copy negative control that proves the acceptance is non-vacuous.
# It is a Rivet-only terminal acceptance (like loaded-world) — no paperclip jar
# is a prereq, and run-scenario.sh builds the rivet-server binary on demand.
# Its exit code is the machine-stable 0 PASS / 1 FAIL / 3 UNVERIFIED contract.
# The row must never silently skip or look green on a missing prereq: an
# UNVERIFIED (exit 3) sets ORACLE_UNVERIFIED so the gate exits 3, and under
# --require-oracle it is a hard failure (exit 1) — exactly the
# rivet-parity/self-test boundary.
run_scenario_recenter() {
  echo "==> scenario runner (recenter: sustained-walking acceptance vs the disposable copied world, issues #185/#561)"
  local rc=0
  "$REPO_DIR/tools/rivet-client/run-scenario.sh" recenter || rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "    PASS — the client walked the deterministic +x route across repeated chunk boundaries; the region-backed recenter stayed connected and received every beyond-boot chunk"
  elif [ "$rc" -eq 1 ]; then
    echo "    FAILED — recenter acceptance found a divergence (exit $rc; see the output above)"
    exit 1
  elif [ "$rc" -eq 3 ]; then
    # UNVERIFIED: a prerequisite was missing (copied world, binaries) or the
    # server did not reach READY — the sustained walk never ran to completion.
    echo "    UNVERIFIED — recenter acceptance did not complete (exit $rc; see the output above)"
    ORACLE_UNVERIFIED=1
    if [ "$REQUIRE_ORACLE" = 1 ]; then
      echo "    --require-oracle is set: a recenter run that cannot complete is a hard failure"
      exit 1
    fi
  else
    # Tool crash / panic / unexpected error: FAILED, never green.
    echo "    FAILED — recenter acceptance crashed or errored (exit $rc; see the output above)"
    exit 1
  fi
}

# The generated-world acceptance row (seed-42 contract, ahead of the generator):
# boot Rivet with the explicit generated-world launch option (`--seed 42`) on a
# fresh disposable seed world, drive the real Azalea client in `generated` mode,
# and compare the observed per-coordinate content against the seed-42
# ground-truth handoff (`rivet-oracle generated-expected`). It is a Rivet-only
# terminal acceptance — no paperclip jar is a prereq, and run-scenario.sh builds
# the rivet-server binary on demand.
#
# The row is milestone-gated exactly like the Paper-vs-Rivet hash-diff
# (RIVET_HASH_DIR): the committed seed-42 `generated-expected` golden is
# captured on main, but the rivet-server `--seed` launch option still serves
# the superflat M1 fixture (login is_flat=true), not genuine FULL generated
# chunks, so the runner exits UNVERIFIED (3) with the exact pinned reason — it
# never falls back to a superflat boot or a copied loaded world, so this row
# can never fabricate a PASS. While the server does not genuinely serve
# generated chunks the row is recorded as an explicit NOTICE (never a silent
# skip, never a green-looking pass) and stays mergeable, so it does not block
# the serialized release lane ahead of the generator. Setting
# RIVET_GENERATED_WORLD=1 opts into the strict check: the comparison then runs
# and any UNVERIFIED (exit 3) sets ORACLE_UNVERIFIED so the gate exits 3, and
# under --require-oracle it is a hard failure (exit 1) — exactly the
# rivet-parity/self-test boundary.
run_scenario_generated_world() {
  echo "==> scenario runner (generated-world: official-client acceptance vs a fresh seed-42 generated world)"
  # The opt-in uses the same convention as RIVET_REQUIRE_ORACLE: any non-empty
  # value other than "0" enables the strict check, so an operator can disable
  # the row explicitly with RIVET_GENERATED_WORLD=0 (never silently enabling it).
  if [ -z "${RIVET_GENERATED_WORLD:-}" ] || [ "$RIVET_GENERATED_WORLD" = "0" ]; then
    echo "    NOTICE — generated-world acceptance is UNVERIFIED and milestone-gated: the"
    echo "      server does not yet genuinely serve generated chunks (the --seed build still"
    echo "      boots the superflat M1 fixture, login is_flat=true; the runner exits 3 with the"
    echo "      exact pinned GENERATED_WORLD_UNVERIFIED_REASON; it never falls back to superflat"
    echo "      or a copied loaded world). Set RIVET_GENERATED_WORLD=1 to require this row."
    return 0
  fi
  local rc=0
  "$REPO_DIR/tools/rivet-client/run-scenario.sh" generated-world || rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "    PASS — the real client joined, spawned, and the served seed-42 content matched the ground-truth manifest"
  elif [ "$rc" -eq 1 ]; then
    echo "    FAILED — generated-world acceptance found a divergence (exit $rc; see the output above)"
    exit 1
  elif [ "$rc" -eq 3 ]; then
    # UNVERIFIED: the rivet-server --seed option still serves the superflat M1
    # fixture (login is_flat=true) rather than genuine generated chunks, so the
    # comparison never ran to completion and MUST NOT look green.
    echo "    UNVERIFIED — generated-world acceptance did not complete (exit $rc; see the output above)"
    ORACLE_UNVERIFIED=1
    if [ "$REQUIRE_ORACLE" = 1 ]; then
      echo "    --require-oracle is set: a generated-world run that cannot complete is a hard failure"
      exit 1
    fi
  else
    # Tool crash / panic / unexpected error: FAILED, never green.
    echo "    FAILED — generated-world acceptance crashed or errored (exit $rc; see the output above)"
    exit 1
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

  # --- feature-gated packet tables + packet bodies (rivet-protocol `packets`) ----
  # rivet-protocol's generated packet-ID tables (src/generated) and the packet
  # body codecs + their registration/integration tests (protocol/* modules, e.g.
  # issue #207 server links) live behind the crate's `packets` cargo feature, so
  # the `cargo test --workspace` step above never builds or executes them.
  # Enable the feature for exactly this one crate and run clippy + tests, so the
  # packet tables and bodies are compiled, linted, and exercised on every full
  # merge gate. Never use `--workspace --features packets` or `--all-features`:
  # `--workspace --features packets` would also enable `packets` on every other
  # member that declares it (rivet-fuzz forwards to rivet-protocol/packets), and
  # `--all-features` would enable every feature of every selected package — both
  # widen the step far beyond this one crate's feature. Re-running this one
  # crate's tests with the feature is deliberate — cargo cannot run only the
  # feature-gated tests, and the whole crate is a fast test run. Scoped gates
  # for rivet-protocol also get the feature (same gap otherwise).
  local GATE_PACKETS=false
  for p in ${PKGS[@]+"${PKGS[@]}"}; do
    [ "$p" = "rivet-protocol" ] && GATE_PACKETS=true
  done
  if [ "$FULL_GATE" = true ] || [ "$GATE_PACKETS" = true ]; then
    echo "==> rivet-protocol --features packets (generated packet tables + packet bodies)"
    RUSTFLAGS=-Dwarnings cargo clippy -p rivet-protocol --features packets --all-targets
    if command -v cargo-nextest >/dev/null 2>&1; then
      cargo nextest run -p rivet-protocol --features packets
    else
      cargo test -p rivet-protocol --features packets
    fi
  fi

  # --- rivet-fuzz packet-decode targets (`packets` feature) ----------------------
  # The five packet fuzz targets (fuzz/fuzz_targets/packet_*.rs) are gated behind
  # the rivet-fuzz `packets` feature (forwards to rivet-protocol/packets) via
  # `required-features`, so the generic workspace compile/lint/test steps do not
  # type-check or lint the feature-gated packet bins (fmt still checks their
  # formatting). The fuzz crate is a workspace member and compiles on the pinned
  # stable toolchain (cargo-fuzz/nightly is only needed to RUN the fuzzers), so
  # the gate type-checks and lints them explicitly — a broken packet target must
  # fail the merge. Never use `--all-features` or `--workspace --features`:
  # `--workspace --features packets` would also enable rivet-protocol's `packets`
  # (blurring this step into the rivet-protocol packets step), and
  # `--all-features` would enable every feature of every selected package.
  local GATE_FUZZ=false
  for p in ${PKGS[@]+"${PKGS[@]}"}; do
    [ "$p" = "rivet-fuzz" ] && GATE_FUZZ=true
  done
  if [ "$FULL_GATE" = true ] || [ "$GATE_FUZZ" = true ]; then
    echo "==> rivet-fuzz --features packets (protocol packet-decode fuzz targets)"
    cargo check -p rivet-fuzz --features packets --bins
    RUSTFLAGS=-Dwarnings cargo clippy -p rivet-fuzz --features packets --all-targets
  fi

  # --- manifest regression suite (full gate only) --------------------------------
  # scripts/test_analyze_graph.py proves MANIFEST.tsv generation is deterministic
  # and conserved (nbt + network + game + world class-cluster splits,
  # byte-idempotent regeneration, carry of status/attempts/notes, dep
  # resolution, fail-fast on cross-unit duplicate declarations). Requires the
  # real Paper tree under working/ (analyze_graph.py hard-exits if the source
  # roots are absent) — same prerequisite as the oracle steps. Skipped when
  # gating a crate subset — the manifest is a repo-wide artifact, not a
  # workspace crate — same rule as oracle/scenario.
  if [ "$FULL_GATE" = true ]; then
    echo "==> manifest regression suite (scripts/test_analyze_graph.py)"
    python3 scripts/test_analyze_graph.py
  fi

  # --- marker audit (full gate only) ---------------------------------------------
  # scripts/check_markers.py validates the STUB(...) / RivetTodo(#N) comment
  # conventions offline (git-tracked *.rs files + MANIFEST.tsv, never GitHub):
  # canonical marker shape, STUB unit ids resolving in the manifest, stale STUBs
  # on done units, two marker bodies on one line (cross- and same-form), empty
  # (whitespace-only) reasons, and every todo!()/unimplemented!() carrying an
  # adjacent RivetTodo. The workspace-excluded tools (tools/rivet-codegen,
  # spikes/) are not governed source and are skipped alongside fuzz/tests/
  # generated. scripts/test_check_markers.py is the sandbox suite for the
  # checker itself. Skipped when gating a crate subset — the marker audit is
  # repo-wide and does not target a workspace crate.
  if [ "$FULL_GATE" = true ]; then
    echo "==> marker audit (scripts/check_markers.py)"
    python3 scripts/check_markers.py
    python3 scripts/test_check_markers.py
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
      echo "==> rivet-codegen probe-biomes-tags (live Paper biome id + tag network content)"
      cargo run --release --quiet --manifest-path tools/rivet-codegen/Cargo.toml -- \
        probe-biomes-tags --bundler "$PROBE_BUNDLER"
      echo "==> rivet-codegen probe-block-behaviors (live Paper per-StateId behavior table)"
      cargo run --release --quiet --manifest-path tools/rivet-codegen/Cargo.toml -- \
        probe-block-behaviors --bundler "$PROBE_BUNDLER"
      echo "==> rivet-codegen probe-worldgen (live Paper worldgen noise/biome/preset data)"
      cargo run --release --quiet --manifest-path tools/rivet-codegen/Cargo.toml -- \
        probe-worldgen --bundler "$PROBE_BUNDLER"
      echo "==> rivet-codegen probe-feature-data (live Paper seed-42 FEATURES feature data)"
      cargo run --release --quiet --manifest-path tools/rivet-codegen/Cargo.toml -- \
        probe-feature-data --bundler "$PROBE_BUNDLER"
    else
      echo "    SKIPPED (no Paper bundler jar: build working/Paper (paper-bundler-*.jar) or place it in tools/rivet-oracle/work/jars/)"
    fi
  fi

  # --- oracle steps (full gate only; the oracle verifies the whole server) -----
  if [ "$FULL_GATE" = true ]; then
    run_oracle_verify
    run_oracle_hash
    run_oracle_self_test
    run_rivet_parity
    run_join_capture
  fi

  # --- scenario runner (full gate only; terminal M1 acceptance harness) ------
  # Boots local servers (Paper and/or Rivet), joins each with the Azalea
  # headless client, and requires identical normalized transcripts (plus a
  # negative case proving the comparator detects a tampered transcript). The
  # runner also runs its own unit tests first (port isolation, ServerKind,
  # process-lifecycle cleanup, exit-code classification — issue #155).
  #
  # Rows:
  #   join                  Paper-vs-Paper self-check + tamper negative.
  #   join --server both    Rivet-vs-Paper join differential (issue #192, inverted by #159):
  #                         the same client joins each server and the normalized
  #                         transcripts must match — proving Rivet's offline
  #                         login/configuration/PLAY/chunk-receipt matches Paper.
  #   move --server both    Rivet-vs-Paper authoritative-walk differential:
  #                         the same client walks in each server and the sampled
  #                         walk, teleport acks, and keepalive echoes must match.
  #   dwell                 Rivet-only wall-clock keepalive survival: the client
  #                         stays in PLAY past the 30 s kick limit echoing every
  #                         live keepalive, and the rivet log must show the
  #                         connection and never a `read timeout` kick.
  #   kick                  Rivet-only decoded-disconnect-reason check (issue #86):
  #                         the client sends a NaN movement frame after spawn so
  #                         the anti-cheat gate answers with a
  #                         ClientboundDisconnectPacket, and the client must decode
  #                         exactly `multiplayer.disconnect.invalid_player_movement`
  #                         (plus a tamper negative on the decoded reason key).
  #   loaded-world          Rivet-only official-client acceptance (issue #374):
  #                         boot Rivet against a disposable copy of the safe world
  #                         under working/client-worlds/New World (never the
  #                         launcher save), extract the read-only ground-truth
  #                         manifest (rivet-oracle extract-world), drive the real
  #                         Azalea client in loaded mode, and compare the served
  #                         per-coordinate block content. Needs no jar — only the
  #                         rivet-server binary (built on demand by run-scenario.sh)
  #                         plus the rivet-oracle/rivet-client binaries. Exits
  #                         0 PASS / 1 FAIL / 3 UNVERIFIED; exit 3 sets
  #                         ORACLE_UNVERIFIED so the gate exits 3 (and
  #                         --require-oracle hard-fails it at exit 1).
  #   recenter              Rivet-only sustained-walking acceptance (issues #185/#561):
  #                         boot Rivet against a disposable copy of the safe world
  #                         under working/client-worlds/New World (never the
  #                         launcher save), drive the real Azalea client's
  #                         deterministic +x route across repeated chunk boundaries,
  #                         and REQUIRE the client stays connected and receives every
  #                         beyond-boot chunk the region-backed recenter loads on
  #                         demand — plus a tampered-copy negative control proving the
  #                         acceptance is non-vacuous. Needs no jar — only the
  #                         rivet-server binary (built on demand by run-scenario.sh)
  #                         plus the rivet-client binary the harness resolves.
  #                         Exits 0 PASS / 1 FAIL / 3 UNVERIFIED; exit 3 sets
  #                         ORACLE_UNVERIFIED so the gate exits 3 (and
  #                         --require-oracle hard-fails it at exit 1).
  #   generated-world       Rivet-only official-client acceptance (seed-42
  #                         generated-world contract, ahead of the generator):
  #                         boot Rivet with `--seed 42` (the generated-world
  #                         launch seam) on a fresh disposable seed world, drive
  #                         the real Azalea client in generated mode, and compare
  #                         the served per-coordinate content against the seed-42
  #                         ground-truth handoff (rivet-oracle
  #                         generated-expected). The Paper seed-42 golden is
  #                         captured (PR #595), but the rivet-server `--seed`
  #                         option still serves the superflat M1 fixture — real
  #                         generated-world serving is not wired into the live
  #                         path yet — so the runner stays UNVERIFIED (exit 3)
  #                         with the exact pinned reason. Milestone-gated behind
  #                         RIVET_GENERATED_WORLD=1 (like RIVET_HASH_DIR): while
  #                         genuine generated-world serving is absent the row is
  #                         an explicit NOTICE and stays mergeable — never a
  #                         silent skip or a fabricated PASS. With the flag set
  #                         the exit contract matches loaded-world (0 PASS / 1
  #                         FAIL / 3 UNVERIFIED; exit 3 sets ORACLE_UNVERIFIED,
  #                         and --require-oracle hard-fails it at exit 1).
  #
  # The Paper rows run when the paperclip jar and the rivet-client binary are
  # present (SCENARIO_RUNNABLE, set by the prereq pre-check); when either is
  # missing they report UNVERIFIED and set ORACLE_UNVERIFIED so the gate exits 3
  # (--require-oracle hard-fails at the pre-check) — never the bare "SKIPPED"
  # that could conceal the missing comparison behind a green-looking run (issue
  # #160). The dwell/kick/loaded-world/recenter/generated-world rows are
  # Rivet-only — they need no jar, only the rivet-server binary (which
  # run-scenario.sh builds on demand; the loaded-world row additionally needs
  # the rivet-oracle and rivet-client binaries the harness resolves, the recenter
  # row only the rivet-client binary, and the generated-world row also invokes
  # `rivet-oracle generated-expected` for the seed-42 ground-truth handoff).
  # Every row exits 0 PASS / 1 FAIL / 3 UNVERIFIED, so a missing prereq or a
  # failed scenario can never look green. Skipped when gating a crate subset
  # (the scenario drives a whole server).
  if [ "$FULL_GATE" = true ]; then
    run_scenario_paper_rows
    echo "==> scenario runner (dwell: wall-clock keepalive survival past the 30s kick limit)"
    "$REPO_DIR/tools/rivet-client/run-scenario.sh" dwell --server rivet
    echo "==> scenario runner (kick: decoded disconnect reason from the anti-cheat gate)"
    "$REPO_DIR/tools/rivet-client/run-scenario.sh" kick --server rivet
    run_scenario_loaded_world
    run_scenario_recenter
    run_scenario_generated_world
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

# The strict gate owns the shared build lock for its complete process tree. Sourcing
# this file remains side-effect-free for focused tests.
if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  if [ "${RIVET_BUILD_LOCK_HELD:-0}" = 1 ]; then
    main "$@"
  else
    exec "$REPO_DIR/scripts/with-build-lock.sh" "$REPO_DIR" "$REPO_DIR/scripts/gate.sh" "$@"
  fi
fi
