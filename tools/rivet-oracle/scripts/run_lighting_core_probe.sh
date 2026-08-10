#!/usr/bin/env bash
# Compile and run the Paper-side lighting-core golden probe (issue #184) against
# the pinned Paper 26.2 runtime, emitting a deterministic key=value trace for
# the queue / spatial-set / min-fixed-point / storage-map primitives.
#
# Mirrors run_worldgen_sampler.sh: the runtime classpath is the materialized
# paper-26.2.jar FIRST plus every library jar; RIVET_PAPER_RUNTIME_JAR and
# RIVET_PAPER_LIBRARIES override the defaults.
#
# Usage:
#   scripts/run_lighting_core_probe.sh <out-file>
#   RIVET_PAPER_RUNTIME_JAR=... RIVET_PAPER_LIBRARIES=... scripts/run_lighting_core_probe.sh <out-file>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${1:?usage: run_lighting_core_probe.sh <out-file>}"
case "$OUT_FILE" in
  /*) OUT_FILE="$OUT_FILE" ;;
  *)  OUT_FILE="$(pwd)/$OUT_FILE" ;;
esac
cd "$ROOT"

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

CLASSES="$ROOT/.cache/java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/LightingCoreProbe.java"
mkdir -p "$(dirname "$OUT_FILE")"
java -Xms256M -Xmx1G -cp "$CP" LightingCoreProbe --output "$OUT_FILE"
