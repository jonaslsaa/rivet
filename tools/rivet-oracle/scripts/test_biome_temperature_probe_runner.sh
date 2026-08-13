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

# Shim `javac`/`java` so the guard tests can prove the runner got past every
# guard without coupling to any real JDK's diagnostics. The javac shim
# succeeds (exit 0); the java shim records its args (so the test can assert
# --output/--paper are passed through) and writes a stub fixture the runner's
# manifest step hashes, then exits 0 — exercising the full success path.
BIN_DIR="$TMP/bin"
mkdir -p "$BIN_DIR"
printf '#!/bin/bash\nexit 0\n' > "$BIN_DIR/javac"
MOCK_JAVA_ARGS="$TMP/java_args.txt"
printf '#!/bin/bash\necho "$@" > "%s"\n' "$MOCK_JAVA_ARGS" > "$BIN_DIR/java"
printf 'out=""; while [ $# -gt 0 ]; do if [ "$1" = "--output" ]; then out="$2"; shift 2; else shift; fi; done; mkdir -p "$out"; printf "{}\\n" > "$out/biome-temperature.json"\n' >> "$BIN_DIR/java"
printf 'exit 0\n' >> "$BIN_DIR/java"
chmod +x "$BIN_DIR/javac" "$BIN_DIR/java"

# A real (if tiny) zip carrying a Git-Commit manifest attribute.
JAR_DIR="$TMP/jar"
LIBS_DIR="$TMP/libs"
mkdir -p "$JAR_DIR/META-INF" "$LIBS_DIR"
printf 'Manifest-Version: 1.0\r\nGit-Commit: 0a99345\r\n' > "$JAR_DIR/META-INF/MANIFEST.MF"
( cd "$JAR_DIR" && zip -q -r "$TMP/paper.jar" META-INF )

# A real zip whose manifest also carries a hostile `X-Git-Commit:` substring
# before the real attribute (the guard must anchor to line start and read the
# real commit, mirroring Rust `parse_manifest_commit`'s strip_prefix).
HOSTILE_DIR="$TMP/hostile"
mkdir -p "$HOSTILE_DIR/META-INF"
printf 'Manifest-Version: 1.0\r\nX-Git-Commit: deadbeef\r\nGit-Commit: 0a99345\r\n' > "$HOSTILE_DIR/META-INF/MANIFEST.MF"
( cd "$HOSTILE_DIR" && zip -q -r "$TMP/hostile.jar" META-INF )

# A file that exists but is not a zip.
printf 'not a zip' > "$TMP/notazip.jar"

# Runs the runner with the given jar/libs (and optional extra runner args),
# with the javac shim first on PATH. Sets RUN_STATUS (the runner's exit code)
# and RUN_OUT (its combined output).
run() {
  local jar="$1" libs="$2"; shift 2
  RUN_OUT="$(PATH="$BIN_DIR:$PATH" RIVET_PAPER_RUNTIME_JAR="$jar" RIVET_PAPER_LIBRARIES="$libs" \
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

# T5: valid jar + non-empty libs -> the full success path: guards pass, javac
# (shim) succeeds, java (shim) is invoked with --output/--paper and writes a
# fixture, the manifest step runs, and the fixture lands in OUT_DIR. This
# catches a regression that drops the probe's --output/--paper args, which the
# failure-path tests cannot.
printf 'stub.jar' > "$LIBS_DIR/stub.jar"
run "$TMP/paper.jar" "$LIBS_DIR"
expect_exit 0
for guard in "failed to read META-INF/MANIFEST.MF" "libraries dir not found" \
             "but the pin is" "no library jars under" "has no Git-Commit attribute"; do
  if echo "$RUN_OUT" | grep -q "$guard"; then
    fail "t5 emitted a guard diagnostic ($guard): $RUN_OUT"
  fi
done
# The runner stages the regeneration in a temp dir, so assert the flags are
# present and non-empty (not the specific stage path).
grep -Eq -- "--output [^ ]+" "$MOCK_JAVA_ARGS" || fail "t5 java did not receive --output: $(cat "$MOCK_JAVA_ARGS")"
grep -q -- "--paper 26.2-DEV-main@0a99345" "$MOCK_JAVA_ARGS" || fail "t5 java did not receive --paper: $(cat "$MOCK_JAVA_ARGS")"
[ -f "$TMP/out/biome-temperature.json" ] || fail "t5 fixture not written: $RUN_OUT"
[ -f "$TMP/out/manifest.json" ] || fail "t5 manifest not written: $RUN_OUT"
pass "valid jar + libs -> full success path (java gets --output/--paper, fixture+manifest land in OUT_DIR)"

# T6: a hostile `X-Git-Commit:` substring before the real attribute must not
# be misread as the pin (the awk match is anchored to line start). The guard
# reads the real 0a99345 and passes; the success path runs.
run "$TMP/hostile.jar" "$LIBS_DIR"
expect_exit 0
for guard in "failed to read META-INF/MANIFEST.MF" "libraries dir not found" \
             "but the pin is" "no library jars under" "has no Git-Commit attribute"; do
  if echo "$RUN_OUT" | grep -q "$guard"; then
    fail "t6 emitted a guard diagnostic ($guard): $RUN_OUT"
  fi
done
[ -f "$TMP/out/biome-temperature.json" ] || fail "t6 fixture not written: $RUN_OUT"
pass "hostile X-Git-Commit substring -> real commit read, guards pass"

# T7: a valid jar with no Git-Commit attribute at all -> controlled refusal.
NOPIN_DIR="$TMP/nopin"
mkdir -p "$NOPIN_DIR/META-INF"
printf 'Manifest-Version: 1.0\r\n' > "$NOPIN_DIR/META-INF/MANIFEST.MF"
( cd "$NOPIN_DIR" && zip -q -r "$TMP/nopin.jar" META-INF )
run "$TMP/nopin.jar" "$LIBS_DIR"
expect_exit 1
echo "$RUN_OUT" | grep -q "has no Git-Commit attribute; cannot verify the pin" \
  || fail "t7 diagnostic: $RUN_OUT"
pass "missing Git-Commit attribute -> controlled refusal, exit 1"

echo "all biome-temperature runner guard tests passed"
