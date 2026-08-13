#!/bin/bash
# Focused test for run_biome_temperature_probe.sh's runtime guards: each
# failure path must emit a controlled diagnostic and exit 1 instead of dying
# silently under `set -euo pipefail`. Drives the runner with synthetic jars and
# library dirs via RIVET_PAPER_RUNTIME_JAR / RIVET_PAPER_LIBRARIES:
#   - a jar that is not a zip         -> "failed to read META-INF/MANIFEST.MF ... (unzip exit N)"
#   - a missing libraries dir         -> "libraries dir not found"
#   - a valid jar with a pin mismatch -> "materialized server jar is Git-Commit ... but the pin is"
#   - an empty libraries dir          -> "no library jars under"
#   - a valid jar + non-empty libs    -> the guards pass (the script proceeds to javac,
#                                        which fails on a synthetic jar — but no guard
#                                        diagnostic is emitted)
#
#   ./scripts/test_biome_temperature_probe_runner.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# A real (if tiny) zip carrying a Git-Commit manifest attribute.
JAR_DIR="$TMP/jar"
LIBS_DIR="$TMP/libs"
mkdir -p "$JAR_DIR/META-INF" "$LIBS_DIR"
printf 'Manifest-Version: 1.0\r\nGit-Commit: 0a99345\r\n' > "$JAR_DIR/META-INF/MANIFEST.MF"
( cd "$JAR_DIR" && zip -q -r "$TMP/paper.jar" META-INF )

# A file that exists but is not a zip.
printf 'not a zip' > "$TMP/notazip.jar"

# Runs the runner with the given jar/libs (and optional extra runner args).
# Sets RUN_STATUS (the runner's exit code) and RUN_OUT (its combined output).
run() {
  local jar="$1" libs="$2"; shift 2
  RUN_OUT="$(RIVET_PAPER_RUNTIME_JAR="$jar" RIVET_PAPER_LIBRARIES="$libs" \
    bash "$SCRIPT_DIR/run_biome_temperature_probe.sh" "$TMP/out" "$@" 2>&1)" \
    && RUN_STATUS=0 || RUN_STATUS=$?
}

expect_exit() { # $1=expected status
  [ "$RUN_STATUS" = "$1" ] || fail "expected exit $1, got $RUN_STATUS: $RUN_OUT"
}

# T1: jar exists but is not a zip -> unzip fails with a controlled diagnostic.
run "$TMP/notazip.jar" "$LIBS_DIR"
expect_exit 1
echo "$RUN_OUT" | grep -q "failed to read META-INF/MANIFEST.MF" || fail "t1 diagnostic: $RUN_OUT"
echo "$RUN_OUT" | grep -q "unzip exit" || fail "t1 unzip-exit code: $RUN_OUT"
pass "not-a-zip jar -> controlled unzip-failure diagnostic, exit 1"

# T2: valid jar but missing libraries dir.
run "$TMP/paper.jar" "$TMP/missing-libs"
expect_exit 1
echo "$RUN_OUT" | grep -q "libraries dir not found" || fail "t2 diagnostic: $RUN_OUT"
pass "missing libraries dir -> controlled diagnostic, exit 1"

# T3: valid jar but the pin does not match the jar's Git-Commit.
run "$TMP/paper.jar" "$LIBS_DIR" "26.2-DEV-main@deadbeef"
expect_exit 1
echo "$RUN_OUT" | grep -q "Git-Commit 0a99345 but the pin is 26.2-DEV-main@deadbeef" \
  || fail "t3 diagnostic: $RUN_OUT"
pass "pin mismatch -> controlled diagnostic naming both commits, exit 1"

# T4: valid jar, existing but empty libraries dir.
run "$TMP/paper.jar" "$LIBS_DIR"
expect_exit 1
echo "$RUN_OUT" | grep -q "no library jars under" || fail "t4 diagnostic: $RUN_OUT"
pass "empty libraries dir -> controlled diagnostic, exit 1"

# T5: valid jar + non-empty libs -> the guards pass; the script proceeds to
# javac, which fails on the synthetic jar (no net.minecraft classes). Assert no
# guard diagnostic is emitted (the failure is the javac one, not a guard).
printf 'stub.jar' > "$LIBS_DIR/stub.jar"
run "$TMP/paper.jar" "$LIBS_DIR"
for guard in "failed to read META-INF/MANIFEST.MF" "libraries dir not found" \
             "but the pin is" "no library jars under" "has no Git-Commit attribute"; do
  if echo "$RUN_OUT" | grep -q "$guard"; then
    fail "t5 emitted a guard diagnostic ($guard): $RUN_OUT"
  fi
done
echo "$RUN_OUT" | grep -q "error reading" || fail "t5 did not reach javac (no classpath error): $RUN_OUT"
pass "valid jar + libs -> guards pass (proceeds to javac)"

echo "all biome-temperature runner guard tests passed"
