#!/usr/bin/env bash
# Compile and run the Paper-side `SeqProbe` against the pinned Paper 26.2
# runtime, regenerating `fixtures/seq/seq-random.json` (issue #208). The probe
# is a single-file Java class that touches only the value-leaf
# `PositionalRandomFactory` overloads (`at(BlockPos)` / `fromHashOf(Identifier)`)
# over `LegacyRandomSource`/`XoroshiroRandomSource`, so no server/registry boot
# is needed.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_seq_probe.sh [out-dir] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_seq_probe.sh
#
# The default output directory is `fixtures/seq/` (in place). The probe always
# writes the file `seq-random.json` inside the given directory, and the
# manifest is regenerated there too.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/seq}"
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

CLASSES="$ROOT/.cache/seq-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/SeqProbe.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" SeqProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
# The probe hardcodes the output basename `seq-random.json` (SeqProbe writes
# `output/seq-random.json`), so the first argument selects the output
# directory, not a file. Hash what the probe actually wrote, never an assumed
# file path.
FIXTURE_FILE="$OUT_DIR/seq-random.json"
echo "wrote $FIXTURE_FILE"

# Refresh the fixture manifest so the regenerated goldens hash matches what the
# gate's `rivet-oracle verify` expects (the text/worldgen kinds regenerate their
# manifests in-process; the seq-random kind is script-driven, so the script
# owns it).
SHA="$(shasum -a 256 "$FIXTURE_FILE" | awk '{print $1}')"
BYTES="$(wc -c < "$FIXTURE_FILE" | tr -d ' ')"
MANIFEST="$OUT_DIR/manifest.json"
NOTE="golden samples of the PositionalRandomFactory default overloads taking BlockPos / Identifier (at(BlockPos) -> at(x,y,z), fromHashOf(Identifier) -> fromHashOf(id.toString())) over LegacyRandomSource / XoroshiroRandomSource, captured from the pinned Paper 26.2 runtime via SeqProbe. Values are the raw nextInt/nextLong outputs (integral). Deterministic across boots."
printf '%s\n' \
  '{' \
  '  "format": 1,' \
  "  \"paper\": \"$PAPER_PIN\"," \
  '  "kind": "seq-random",' \
  "  \"note\": \"$NOTE\"," \
  '  "captured": [' \
  '    {' \
  '      "path": "seq-random.json",' \
  "      \"sha256\": \"$SHA\"," \
  "      \"bytes\": $BYTES" \
  '    }' \
  '  ]' \
  '}' > "$MANIFEST"
echo "wrote $MANIFEST"
