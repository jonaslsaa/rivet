#!/usr/bin/env bash
# Compile and run the M2 Paper-side worldgen sampling harness against the pinned
# Paper 26.2 runtime. The sampler is a single-file Java class that boots the
# vanilla registries (no server boot) and emits stable semantic JSON fixtures
# for density / biome / surface worldgen output.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it (note `rivet-oracle verify` boots a
# separate scratch runtime under `work/verify/run`).
#
# Usage:
#   scripts/run_worldgen_sampler.sh <out-dir> [seed] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_worldgen_sampler.sh <out-dir>
#
# The seed (default 42) and the paper provenance string (default
# 26.2-DEV-main@0a99345, the pinned commit) are recorded in the emitted
# samples.json so every fixture self-describes its provenance.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:?usage: run_worldgen_sampler.sh <out-dir> [seed] [paper-pin]}"
SEED="${2:-42}"
# Resolve a relative out-dir against the invoking cwd, then pin Paper's logs/
# (written into the process cwd) under ROOT, which is already gitignored.
case "$OUT_DIR" in
  /*) OUT_DIR="$OUT_DIR" ;;
  *)  OUT_DIR="$(pwd)/$OUT_DIR" ;;
esac
cd "$ROOT"
PAPER_PIN="${3:-26.2-DEV-main@0a99345}"

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

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/WorldGenSampler.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx1G -cp "$CP" WorldGenSampler \
  --seed "$SEED" --output "$OUT_DIR" --paper "$PAPER_PIN"
