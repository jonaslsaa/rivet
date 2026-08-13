#!/usr/bin/env bash
# Compile and run the Paper-side `ChunkLevelProbe` against the pinned Paper
# 26.2 runtime, regenerating `fixtures/chunk-level/chunk-level-goldens.json`
# (the `mc.server.level.pipeline.level` value layer). The probe samples
# `ChunkLevel`'s level<->status mappings — which are derived from the
# generation pyramid's FULL step accumulated dependencies — plus the
# `FullChunkStatus` ordinal ladder, so the emitted values are exactly what the
# Rust port must reproduce.
#
# The probe needs `SharedConstants.tryDetectVersion()` +
# `Bootstrap.bootStrap()` because `ChunkStatus`' static registration touches
# `BuiltInRegistries`, but it never boots a world. The runtime classpath is the
# materialized paper-26.2.jar FIRST (its bundled, patched LogUtils must win
# over the library jars) plus every library jar. The default materialized
# runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_chunk_level_probe.sh [out-dir] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_chunk_level_probe.sh
#
# The default output directory is `fixtures/chunk-level/` (in place). The probe
# always writes the file `chunk-level-goldens.json` inside the given directory,
# and the manifest is regenerated there too.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/chunk-level}"
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

CLASSES="$ROOT/.cache/chunk-level-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/ChunkLevelProbe.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" ChunkLevelProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
# The probe hardcodes the output basename `chunk-level-goldens.json` (it writes
# `output/chunk-level-goldens.json`), so the first argument selects the output
# directory, not a file. Hash what the probe actually wrote, never an assumed
# file path.
FIXTURE_FILE="$OUT_DIR/chunk-level-goldens.json"
echo "wrote $FIXTURE_FILE"

# Refresh the fixture manifest so the regenerated goldens hash matches what the
# gate's `rivet-oracle verify` expects (the chunk-level kind is script-driven,
# so the script owns it, like seq-random).
SHA="$(shasum -a 256 "$FIXTURE_FILE" | awk '{print $1}')"
BYTES="$(wc -c < "$FIXTURE_FILE" | tr -d ' ')"
MANIFEST="$OUT_DIR/manifest.json"
NOTE="golden samples of the ChunkLevel value layer (FULL_CHUNK_LEVEL/BLOCK_TICKING_LEVEL/ENTITY_TICKING_LEVEL/RADIUS_AROUND_FULL_CHUNK/MAX_LEVEL; generationStatus(level); getStatusAroundFullChunk(distance[, default]); byStatus(ChunkStatus)/byStatus(FullChunkStatus); fullStatus(level); isEntityTicking/isBlockTicking/isLoaded; the FullChunkStatus ordinal ladder + isOrAfter), captured from the pinned Paper 26.2 runtime via ChunkLevelProbe. ChunkLevel's mappings derive from ChunkPyramid.GENERATION_PYRAMID.getStepTo(ChunkStatus.FULL).accumulatedDependencies(); the values are the exact Java outputs (level ints, status names, booleans). Deterministic across boots."
printf '%s\n' \
  '{' \
  '  "format": 1,' \
  "  \"paper\": \"$PAPER_PIN\"," \
  '  "kind": "chunk-level",' \
  "  \"note\": \"$NOTE\"," \
  '  "captured": [' \
  '    {' \
  '      "path": "chunk-level-goldens.json",' \
  "      \"sha256\": \"$SHA\"," \
  "      \"bytes\": $BYTES" \
  '    }' \
  '  ]' \
  '}' > "$MANIFEST"
echo "wrote $MANIFEST"
