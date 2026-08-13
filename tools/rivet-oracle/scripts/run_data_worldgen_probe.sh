#!/usr/bin/env bash
# Compile and run the Paper-side `TerrainProviderProbe` + `NoiseDataProbe`
# against the pinned Paper 26.2 runtime, regenerating
# `fixtures/data-worldgen/terrain-provider-goldens.json` and
# `fixtures/data-worldgen/noise-data-goldens.json` (the `mc.data.worldgen.prereq`
# unit). Both probes are single-file Java classes touching only value-leaf
# arithmetic: `TerrainProvider` builds its overworld offset/factor/jaggedness
# `CubicSpline`s and samples `peaksAndValleys`; `NoiseData.bootstrap` runs into
# a recording `BootstrapContext`. No server/registry boot.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_data_worldgen_probe.sh [out-dir] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_data_worldgen_probe.sh
#
# The default output directory is `fixtures/data-worldgen/` (in place). The
# probes write `terrain-provider-goldens.json` and `noise-data-goldens.json`
# inside the given directory, and the manifest is regenerated there too.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/data-worldgen}"
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

CLASSES="$ROOT/.cache/data-worldgen-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

mkdir -p "$OUT_DIR"
javac -cp "$CP" -d "$CLASSES" \
  "$ROOT/src/java/TerrainProviderProbe.java" \
  "$ROOT/src/java/NoiseDataProbe.java"
java -Xms256M -Xmx2G -cp "$CP" TerrainProviderProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
java -Xms256M -Xmx2G -cp "$CP" NoiseDataProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"

# Hash what the probes actually wrote, never an assumed file path.
TERRAIN_GOLDENS="$OUT_DIR/terrain-provider-goldens.json"
NOISE_GOLDENS="$OUT_DIR/noise-data-goldens.json"
echo "wrote $TERRAIN_GOLDENS"
echo "wrote $NOISE_GOLDENS"

# Refresh the fixture manifest so the regenerated goldens hash matches what the
# gate's `rivet-oracle verify` expects.
. "$ROOT/scripts/write_fixture_manifest.sh"
NOTE="Paper-grounded TerrainProvider/NoiseData value-leaf goldens (the mc.data.worldgen.prereq unit): \`terrain-provider-goldens.json\` records the overworld offset/factor/jaggedness CubicSpline min/max/sample outputs and the peaksAndValleys sweep as hex-float, plus Paper's \`parityString()\` output, asserted bit-exactly by crates/rivet-world/src/data/worldgen/terrain_provider.rs tests; \`noise-data-goldens.json\` records NoiseData.bootstrap's registered key/parameter order, asserted by crates/rivet-world/src/data/worldgen/noise_data.rs tests. Captured from the pinned Paper runtime via tools/rivet-oracle/src/java/TerrainProviderProbe.java and NoiseDataProbe.java; regenerate with \`scripts/run_data_worldgen_probe.sh\`."
write_fixture_manifest "$OUT_DIR" "data-worldgen" "$PAPER_PIN" "$NOTE" "$TERRAIN_GOLDENS" "$NOISE_GOLDENS"
