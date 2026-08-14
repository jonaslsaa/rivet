#!/usr/bin/env bash
# Compile and run the Paper-side `SurfaceColumnProbe` against the pinned Paper
# 26.2 runtime, regenerating `fixtures/surface-column/surface-columns.json`
# (the post-surface column oracle for #179). The probe boots the vanilla
# registries (no server boot) and drives the REAL overworld generator
# (`createBiomes` -> `fillFromNoise` -> `buildSurface`) on REAL `ProtoChunk`s
# at seed 42, emitting sampled pre/post block columns plus heightmap + biome
# metadata that proves the capture ran post-surface (not a no-op).
#
# The runtime classpath is the materialized paper-26.2.jar PLUS every library
# jar. The probe ships a Level-free shadow of Paper's
# `OptionallyFlatBedrockConditionSource` (the real class derefs
# `context.level()` for `generateFlatBedrock`); its compiled output directory
# MUST precede the server jar on the classpath so the JVM loads the shadow
# instead of the jar's class. The default materialized runtime is the M0
# fixture-boot runtime `tools/rivet-oracle/work/run/{versions,libraries}`;
# `RIVET_PAPER_RUNTIME_JAR` and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_surface_column_probe.sh <out-dir> [seed] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_surface_column_probe.sh <out-dir>
#
# The seed (default 42) and the paper provenance string (default
# 26.2-DEV-main@0a99345, the pinned commit) are recorded in the emitted
# surface-columns.json so every fixture self-describes its provenance.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:?usage: run_surface_column_probe.sh <out-dir> [seed] [paper-pin]}"
SEED="${2:-42}"
# Resolve a relative out-dir against the invoking cwd, then pin Paper's logs/
# (written into the process cwd) under ROOT, which is already gitignored.
if [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$(pwd)/$OUT_DIR"
fi
cd "$ROOT"
PAPER_PIN="${3:-26.2-DEV-main@0a99345}"

RUNTIME_JAR="${RIVET_PAPER_RUNTIME_JAR:-$ROOT/work/run/versions/26.2/paper-26.2.jar}"
if [ ! -f "$RUNTIME_JAR" ]; then
  echo "materialized server jar not found: $RUNTIME_JAR" >&2
  echo "boot the M0 fixture server once (tools/rivet-oracle/README.md) or set" >&2
  echo "RIVET_PAPER_RUNTIME_JAR to a versions/26.2/paper-26.2.jar" >&2
  exit 3
fi

# Authenticate the runtime's provenance before running: the golden is pinned to
# PAPER_PIN's commit, and regenerating from a jar at a different (or
# unverifiable) commit would relabel fixtures with provenance they do not have.
# Mirrors the verify CLI's PinMismatch/PinUnavailable gating. The jar's
# `Git-Commit` MANIFEST attribute is the same source of truth `check_pin` reads.
# All "runtime prerequisite absent / pin not confirmed" paths exit 3 UNVERIFIED;
# only a genuine probe failure (javac/java) exits 1.
PIN_COMMIT="${PAPER_PIN##*@}"
if [ -z "$PIN_COMMIT" ]; then
  echo "paper pin '$PAPER_PIN' has no '@<commit>' suffix; cannot authenticate runtime" >&2
  exit 3
fi
ACTUAL_COMMIT="$(unzip -p "$RUNTIME_JAR" META-INF/MANIFEST.MF 2>/dev/null \
  | sed -n 's/^Git-Commit:[[:space:]]*//p' | tr -d '\r' | head -n1 || true)"
if [ -z "$ACTUAL_COMMIT" ]; then
  echo "materialized server jar $RUNTIME_JAR carries no Git-Commit attribute; cannot confirm pinned provenance" >&2
  exit 3
fi
if [ "$ACTUAL_COMMIT" != "$PIN_COMMIT" ]; then
  echo "materialized server jar is Git-Commit $ACTUAL_COMMIT but the pinned golden" >&2
  echo "provenance is $PAPER_PIN (commit $PIN_COMMIT); refusing to relabel fixtures" >&2
  exit 3
fi
echo "paper pin OK: $PAPER_PIN matches runtime jar Git-Commit $ACTUAL_COMMIT"

LIBS_DIR="${RIVET_PAPER_LIBRARIES:-$ROOT/work/run/libraries}"
LIBS="$(find "$LIBS_DIR" -name '*.jar' 2>/dev/null | tr '\n' ':')"
if [ -z "$LIBS" ]; then
  echo "no library jars under $LIBS_DIR" >&2
  exit 3
fi

CLASSES="$ROOT/.cache/surface-column-java"
mkdir -p "$CLASSES"
# The probe shadow dir comes FIRST so io.papermc.paper.world.worldgen.
# OptionallyFlatBedrockConditionSource (Level-free, generateFlatBedrock=false)
# wins over the same FQN inside the server jar.
CP="$CLASSES:$RUNTIME_JAR:$LIBS"

javac -cp "$CP" -d "$CLASSES" \
  "$ROOT/src/java/io/papermc/paper/world/worldgen/OptionallyFlatBedrockConditionSource.java" \
  "$ROOT/src/java/SurfaceColumnProbe.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" SurfaceColumnProbe \
  --seed "$SEED" --output "$OUT_DIR" --paper "$PAPER_PIN"
