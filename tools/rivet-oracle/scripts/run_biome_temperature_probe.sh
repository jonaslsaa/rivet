#!/usr/bin/env bash
# Compile and run the Paper-side `BiomeTemperatureProbe` against the pinned
# Paper 26.2 runtime, regenerating the `fixtures/biome-temperature/
# biome-temperature.json` golden fixture. The probe constructs vanilla `Biome`
# values (plain/cold/frozen/arid) and emits the exact `getTemperature` /
# `coldEnoughToSnow` / `getPrecipitationAt` outputs plus the raw
# `TEMPERATURE_NOISE`/`FROZEN_TEMPERATURE_NOISE`/`BIOME_INFO_NOISE` samples the
# temperature arithmetic consumes, so the Rust port can assert both aggregate
# outputs and the FROZEN branch decisions against real Paper values.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_biome_temperature_probe.sh [out-dir] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_biome_temperature_probe.sh
#
# The first argument is the OUTPUT DIRECTORY, not a file path: the probe
# hardcodes the basename `biome-temperature.json` and writes it (plus a
# refreshed manifest.json) into that directory. The default is
# `fixtures/biome-temperature/` (in place), so after a run the committed
# fixture + its manifest SHA-256s are byte-identical iff the runtime is
# unchanged. Re-run `rivet-oracle verify` after any output change.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/biome-temperature}"
PAPER_PIN="${2:-26.2-DEV-main@0a99345}"

RUNTIME_JAR="${RIVET_PAPER_RUNTIME_JAR:-$ROOT/work/run/versions/26.2/paper-26.2.jar}"
if [ ! -f "$RUNTIME_JAR" ]; then
  echo "materialized server jar not found: $RUNTIME_JAR" >&2
  echo "boot the M0 fixture server once (tools/rivet-oracle/README.md) or set" >&2
  echo "RIVET_PAPER_RUNTIME_JAR to a versions/26.2/paper-26.2.jar" >&2
  exit 1
fi

# The paper pin is stamped into the fixture and manifest, so refuse to run
# against a drifted runtime: capture a golden against a different commit and
# the fixture would silently re-pin to this stamp, passing every gate. The
# materialized server jar carries the `Git-Commit` manifest attribute.
# (run_seq_probe.sh / run_spline_probe.sh predate this guard and are tracked
# separately; this runner is the first to verify the pin it stamps.)
RUNTIME_COMMIT="$(unzip -p "$RUNTIME_JAR" META-INF/MANIFEST.MF 2>/dev/null \
  | awk '/[[:space:]]*Git-Commit:[[:space:]]*/ { v=$0; sub(/^[[:space:]]*Git-Commit:[[:space:]]*/, "", v); sub(/[[:space:]\r]+$/, "", v); if (v != "") { print v; exit } }')"
PIN_COMMIT="${PAPER_PIN##*@}"
if [ -z "$RUNTIME_COMMIT" ]; then
  echo "materialized server jar $RUNTIME_JAR has no Git-Commit attribute; cannot verify the pin" >&2
  exit 1
fi
if [ "$RUNTIME_COMMIT" != "$PIN_COMMIT" ]; then
  echo "materialized server jar is Git-Commit $RUNTIME_COMMIT but the pin is $PAPER_PIN" >&2
  echo "set RIVET_PAPER_RUNTIME_JAR to the jar built from the pinned commit, or change the pin" >&2
  exit 1
fi

LIBS_DIR="${RIVET_PAPER_LIBRARIES:-$ROOT/work/run/libraries}"
LIBS="$(find "$LIBS_DIR" -name '*.jar' 2>/dev/null | tr '\n' ':')"
if [ -z "$LIBS" ]; then
  echo "no library jars under $LIBS_DIR" >&2
  exit 1
fi

CLASSES="$ROOT/.cache/biome-temperature-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/BiomeTemperatureProbe.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" BiomeTemperatureProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
# The probe hardcodes the output basename `biome-temperature.json`
# (BiomeTemperatureProbe writes `output/biome-temperature.json`), so the first
# argument selects the output directory, not a file. Hash what the probe
# actually wrote, never an assumed file path.
FIXTURE_FILE="$OUT_DIR/biome-temperature.json"
echo "wrote $FIXTURE_FILE"

# Refresh the fixture manifest so the regenerated goldens hash matches what the
# gate's `rivet-oracle verify` expects (the text/worldgen kinds regenerate their
# manifests in-process; this kind is script-driven, so the script owns it).
. "$ROOT/scripts/write_fixture_manifest.sh"
NOTE="bit-exact golden samples of net.minecraft.world.level.biome.Biome getTemperature/coldEnoughToSnow/warmEnoughToRain/getPrecipitationAt (and the raw TEMPERATURE_NOISE/FROZEN_TEMPERATURE_NOISE/BIOME_INFO_NOISE samples those read) captured from the pinned Paper 26.2 runtime via BiomeTemperatureProbe. getTemperature is Float.floatToIntBits; the noise values are Double.doubleToLongBits. The FROZEN modifier's branch analysis uses the raw frozenLarge/frozenEdge/frozenEdge01/frozenSmall noise samples plus the per-sample frozenPins flag so the inner and outer sub-checks are independently discriminable. Deterministic across boots."
write_fixture_manifest "$OUT_DIR" "biome-temperature" "$PAPER_PIN" "$NOTE" "$FIXTURE_FILE"
