#!/usr/bin/env bash
# Compile and run the Paper-side `SplineProbe` against the pinned Paper 26.2
# runtime, regenerating `fixtures/spline/spline-goldens.json` (issue #372). The
# probe is a single-file Java class that touches only the value-leaf
# `CubicSpline`/`BoundedFloatFunction` arithmetic (no server/registry boot).
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_spline_probe.sh [out-file] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_spline_probe.sh
#
# The default output is `fixtures/spline/spline-goldens.json` (in place).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${1:-$ROOT/fixtures/spline/spline-goldens.json}"
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

CLASSES="$ROOT/.cache/spline-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/SplineProbe.java"
OUT_DIR="$(dirname "$OUT_FILE")"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" SplineProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
echo "wrote $OUT_FILE"
