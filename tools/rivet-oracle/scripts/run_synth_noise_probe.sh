#!/usr/bin/env bash
# Compile and run the Paper-side `SynthNoiseProbe` against the pinned Paper 26.2
# runtime, regenerating the `fixtures/synth/synth-noise.json` golden fixture
# (issue #177). The probe is a single-file Java class that touches only the
# value-leaf `net.minecraft.world.level.levelgen.synth` classes (plus
# `Bootstrap` for `BuiltInRegistries`), so no server/registry boot is needed.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_synth_noise_probe.sh [out-file] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_synth_noise_probe.sh
#
# The default output is `fixtures/synth/synth-noise.json` (in place), so after
# a run the committed fixture + its manifest SHA-256s are byte-identical iff the
# runtime is unchanged. Re-run `rivet-oracle verify` after any output change.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${1:-$ROOT/fixtures/synth/synth-noise.json}"
PAPER_PIN="${2:-26.2-DEV-main@0a99345}"

RUNTIME_JAR="${RIVET_PAPER_RUNTIME_JAR:-$ROOT/work/run/versions/26.2/paper-26.2.jar}"
if [ ! -f "$RUNTIME_JAR" ]; then
  echo "materialized server jar not found: $RUNTIME_JAR" >&2
  echo "boot the M0 fixture server once (tools/rivet-oracle/README.md) or set" >&2
  echo "RIVET_PAPER_RUNTIME_JAR to a versions/26.2/paper-26.2.jar" >&2
  exit 1
fi

LIBS_DIR="${RIVET_PAPER_LIBRARIES:-$ROOT/work/run/libraries}"
LIBS="$(find "$LIBS_DIR" -name '*.jar' 2>/dev/null | tr '\n' ':')"
if [ -z "$LIBS" ]; then
  echo "no library jars under $LIBS_DIR" >&2
  exit 1
fi

CLASSES="$ROOT/.cache/synth-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/SynthNoiseProbe.java"
OUT_DIR="$(dirname "$OUT_FILE")"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" SynthNoiseProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
echo "wrote $OUT_FILE"
