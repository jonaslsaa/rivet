#!/usr/bin/env bash
# Compile and run PlacementCountProbe against the materialized Paper 26.2
# runtime (no full server boot needed), writing placement-count.json +
# manifest.json into fixtures/placement-count/.
#
# Usage:
#   scripts/run_placement_count_probe.sh [out-dir] [paper-pin]
#
# Env overrides:
#   RIVET_PAPER_RUNTIME_JAR  path to paper-26.2.jar
#                            (default: work/run/versions/26.2/paper-26.2.jar)
#   RIVET_PAPER_LIBRARIES    path to materialized libraries dir
#                            (default: work/run/libraries)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/placement-count}"
PAPER_PIN="${2:-26.2-DEV-main@0a99345}"

PAPER_JAR="${RIVET_PAPER_RUNTIME_JAR:-$ROOT/work/run/versions/26.2/paper-26.2.jar}"
LIB_DIR="${RIVET_PAPER_LIBRARIES:-$ROOT/work/run/libraries}"
SRC="$ROOT/src/java/PlacementCountProbe.java"
BUILD="$ROOT/work/placement-count-probe-classes"

if [ ! -f "$PAPER_JAR" ]; then
  echo "error: Paper runtime jar not found at $PAPER_JAR" >&2
  echo "boot the M0 Paper fixture server once (see README.md) or set RIVET_PAPER_RUNTIME_JAR" >&2
  exit 2
fi
if [ ! -d "$LIB_DIR" ]; then
  echo "error: Paper libraries dir not found at $LIB_DIR" >&2
  exit 2
fi

# Verify the runtime jar's META-INF/MANIFEST.MF Git-Commit matches the requested
# paper pin before capturing anything, so a stale materialized jar cannot
# silently stamp the wrong commit as truth.
EXPECTED_SHA="${PAPER_PIN##*@}"
if [ -z "$EXPECTED_SHA" ] || [ "$EXPECTED_SHA" = "$PAPER_PIN" ]; then
  echo "error: PAPER_PIN '$PAPER_PIN' must include '@<commit>' (e.g. 26.2-DEV-main@0a99345)" >&2
  exit 2
fi
JAR_COMMIT="$(unzip -p "$PAPER_JAR" META-INF/MANIFEST.MF 2>/dev/null \
  | tr -d '\r' \
  | awk -F': ' '/^Git-Commit:/ { print $2; exit }')"
if [ -z "$JAR_COMMIT" ]; then
  echo "error: could not read Git-Commit from $PAPER_JAR META-INF/MANIFEST.MF" >&2
  exit 2
fi
if [ "${JAR_COMMIT#"$EXPECTED_SHA"}" = "$JAR_COMMIT" ]; then
  echo "error: Paper runtime jar Git-Commit '$JAR_COMMIT' does not match pin '$PAPER_PIN' (expected prefix '$EXPECTED_SHA')" >&2
  exit 2
fi

# Build classpath: paper-26.2.jar first, then every jar under libraries/.
CP="$PAPER_JAR"
while IFS= read -r -d '' jar; do
  CP="$CP:$jar"
done < <(find "$LIB_DIR" -name '*.jar' -print0 | sort -z)

rm -rf "$BUILD"
mkdir -p "$BUILD"
javac -cp "$CP" -d "$BUILD" "$SRC"

# Stage into a temp dir so a mid-run failure cannot leave partial output in OUT_DIR.
STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/placement-count-probe.XXXXXX")"
trap 'rm -rf "$STAGE_DIR"' EXIT

java -Xmx1G -cp "$BUILD:$CP" PlacementCountProbe \
  --output "$STAGE_DIR" \
  --paper "$PAPER_PIN"

FIXTURE="$STAGE_DIR/placement-count.json"
source "$ROOT/scripts/write_fixture_manifest.sh"
write_fixture_manifest \
  "$STAGE_DIR" \
  "placement-count" \
  "$PAPER_PIN" \
  "golden samples of net.minecraft.world.level.levelgen.placement.NoiseBasedCountPlacement#count(RandomSource, BlockPos) captured from the pinned Paper 26.2 runtime via PlacementCountProbe. Deterministic across boots." \
  "$FIXTURE"

mkdir -p "$OUT_DIR"
mv -f "$FIXTURE" "$OUT_DIR/placement-count.json"
mv -f "$STAGE_DIR/manifest.json" "$OUT_DIR/manifest.json"
echo "updated $OUT_DIR/placement-count.json and $OUT_DIR/manifest.json"
