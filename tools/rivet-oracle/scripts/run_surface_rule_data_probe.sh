#!/usr/bin/env bash
# Compile and run the Paper-side `SurfaceRuleDataProbe` against the pinned
# Paper 26.2 runtime, regenerating
# `fixtures/surface-rule-data/surface-rule-data.json` — the canonical
# `RuleSource.CODEC` / `ConditionSource.CODEC` JSON for every
# `SurfaceRuleData` static surface tree (nether / overworld / overworldLike /
# end / air), plus structural occurrence-count stats and the referenced biome
# holder list. The probe boots the vanilla registries (no server boot) exactly
# like ComposedNoiseProbe so the `BIOME` registry is populated for the
# holder-set `biome_is` fields.
#
# The runtime classpath is the materialized paper-26.2.jar FIRST (its bundled,
# patched LogUtils must win over the library jars) plus every library jar. The
# default materialized runtime is the M0 fixture-boot runtime
# `tools/rivet-oracle/work/run/{versions,libraries}`; `RIVET_PAPER_RUNTIME_JAR`
# and `RIVET_PAPER_LIBRARIES` override it.
#
# Usage:
#   scripts/run_surface_rule_data_probe.sh [out-dir] [paper-pin]
#   RIVET_PAPER_RUNTIME_JAR=/path/to/versions/26.2/paper-26.2.jar \
#   RIVET_PAPER_LIBRARIES=/path/to/libraries scripts/run_surface_rule_data_probe.sh
#
# The default output directory is `fixtures/surface-rule-data/` (in place). The
# probe always writes the file `surface-rule-data.json` inside the given
# directory, and the manifest is regenerated there too.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/fixtures/surface-rule-data}"
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

CLASSES="$ROOT/.cache/surface-rule-data-java"
mkdir -p "$CLASSES"
CP="$RUNTIME_JAR:$LIBS$CLASSES"

javac -cp "$CP" -d "$CLASSES" "$ROOT/src/java/SurfaceRuleDataProbe.java"
mkdir -p "$OUT_DIR"
java -Xms256M -Xmx2G -cp "$CP" SurfaceRuleDataProbe \
  --output "$OUT_DIR" --paper "$PAPER_PIN"
GOLDENS_FILE="$OUT_DIR/surface-rule-data.json"
echo "wrote $GOLDENS_FILE"

# Refresh the fixture manifest so the regenerated goldens hash matches what the
# gate's `rivet-oracle verify` expects (script-driven kinds own their manifest,
# exactly like run_spline_probe.sh).
SHA="$(shasum -a 256 "$GOLDENS_FILE" | awk '{print $1}')"
BYTES="$(wc -c < "$GOLDENS_FILE" | tr -d ' ')"
MANIFEST="$OUT_DIR/manifest.json"
NOTE="Paper-grounded SurfaceRuleData golden harness: \`surface-rule-data.json\` captures the canonical \`RuleSource.CODEC\`/\`ConditionSource.CODEC\` JSON (under RegistryOps) for every static \`SurfaceRuleData\` surface tree (nether/overworld/overworldLike/end/air) at the pinned Paper commit, plus structural occurrence-count stats and the referenced biome holder list. crates/rivet-world/tests/surface_rule_data_golden.rs asserts the nether tree parses and re-encodes byte-exactly through the merged surface-rules codecs (serde-normalized exponent casing) and keeps overworld/overworldLike present but UNVERIFIED until the missing overworld biome/noise/block leaves land. Captured from the pinned Paper runtime via tools/rivet-oracle/src/java/SurfaceRuleDataProbe.java; regenerate with \`scripts/run_surface_rule_data_probe.sh\`."
printf '%s\n' \
  '{' \
  '  "format": 1,' \
  "  \"paper\": \"$PAPER_PIN\"," \
  '  "kind": "surface-rule-data",' \
  "  \"note\": \"$NOTE\"," \
  '  "captured": [' \
  '    {' \
  '      "path": "surface-rule-data.json",' \
  "      \"sha256\": \"$SHA\"," \
  "      \"bytes\": $BYTES" \
  '    }' \
  '  ]' \
  '}' > "$MANIFEST"
echo "wrote $MANIFEST"
