#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_DIR=$(cd "$SCRIPT_DIR/../../.." && pwd)
FIXTURES="$REPO_DIR/tools/rivet-oracle/fixtures"
TARGET_DIR=$(mktemp -d "${TMPDIR:-/tmp}/rivet-anvil-roundtrip-target.XXXXXX")
SCRATCH_DIR=$(mktemp -d "${TMPDIR:-/tmp}/rivet-anvil-roundtrip-cli.XXXXXX")
trap 'rm -rf "$TARGET_DIR" "$SCRATCH_DIR"' EXIT

run_oracle() {
  CARGO_TARGET_DIR="$TARGET_DIR" cargo run -q -p rivet-oracle -- anvil-roundtrip-v1a "$@"
}

expect_rc() {
  local expected=$1
  shift
  set +e
  "$@" >"$SCRATCH_DIR/stdout" 2>"$SCRATCH_DIR/stderr"
  local actual=$?
  set -e
  if [ "$actual" -ne "$expected" ]; then
    printf 'expected exit %s, got %s\n' "$expected" "$actual" >&2
    cat "$SCRATCH_DIR/stdout" "$SCRATCH_DIR/stderr" >&2
    exit 1
  fi
}

copy_fixtures() {
  local name=$1
  cp -R "$FIXTURES" "$SCRATCH_DIR/$name"
  printf '%s\n' "$SCRATCH_DIR/$name"
}

mutate_manifest() {
  local root=$1
  local expression=$2
  python3 - "$root/manifest.json" "$expression" <<'PY'
import json
import sys
path, expression = sys.argv[1:]
data = json.load(open(path))
exec(expression, {"data": data})
with open(path, "w") as stream:
    json.dump(data, stream, indent=2)
    stream.write("\n")
PY
}

inventory() {
  python3 - "$1" <<'PY'
import hashlib
import os
import sys
root = os.path.realpath(sys.argv[1])
material = bytearray()
for current, dirs, files in os.walk(root, followlinks=False):
    dirs.sort()
    files.sort()
    for name in dirs:
        path = os.path.join(current, name)
        rel = os.path.relpath(path, root).replace(os.sep, "/")
        material.extend(b"D\0" + rel.encode() + b"\n")
    for name in files:
        path = os.path.join(current, name)
        rel = os.path.relpath(path, root).replace(os.sep, "/")
        material.extend(b"F\0" + rel.encode() + b"\0")
        material.extend(open(path, "rb").read())
        material.extend(b"\0")
print(hashlib.sha256(material).hexdigest())
PY
}

BEFORE_INVENTORY=$(inventory "$FIXTURES")
PASS_OUT="$SCRATCH_DIR/pass"
expect_rc 0 run_oracle --out "$PASS_OUT"
AFTER_INVENTORY=$(inventory "$FIXTURES")
[ "$BEFORE_INVENTORY" = "$AFTER_INVENTORY" ]

python3 - "$PASS_OUT/report.json" <<'PY'
import json
import sys
report = json.load(open(sys.argv[1]))
assert report["verdict"] == "PASS"
assert report["source_chunk_count"] == 432
assert report["source_tree_hash_before_roundtrip"] == report["source_tree_hash_after_roundtrip"]
assert len(report["chunks"]) == 432
assert len(report["corruption_negatives"]) == 6
for negative in report["corruption_negatives"]:
    assert negative["mutation"]
    assert negative["artifact"] == "overworld/r.0.0.mca"
    assert "slot " + str(negative["slot"]) in negative["detected"]
    assert negative["chunk"] in negative["detected"]
    assert negative["mutation"] in negative["detected"]
    assert negative["rejection_stage"] in negative["detected"]
truncation = next(n for n in report["corruption_negatives"] if n["mutation"] == "truncation")
assert truncation["rejection_stage"] == "payload-read"
assert "corrupt chunk [0, 0]" in truncation["detected"]
assert "local chunk (1,0)" not in truncation["detected"]
assert report["corruption_negatives"][3]["slot"] == 1
manifest = json.load(open(sys.argv[1].replace("report.json", "evidence/manifest.json")))
assert len(manifest["chunks"]) == 432
PY

MISSING_MANIFEST=$(copy_fixtures missing-manifest)
rm "$MISSING_MANIFEST/manifest.json"
MISSING_MANIFEST_BEFORE=$(inventory "$MISSING_MANIFEST")
expect_rc 3 run_oracle "$MISSING_MANIFEST" --out "$SCRATCH_DIR/missing-manifest-out"
grep -q "source provenance prerequisite unavailable" "$SCRATCH_DIR/stderr"
MISSING_MANIFEST_AFTER=$(inventory "$MISSING_MANIFEST")
[ "$MISSING_MANIFEST_BEFORE" = "$MISSING_MANIFEST_AFTER" ]

MISSING_ARTIFACT=$(copy_fixtures missing-artifact)
rm "$MISSING_ARTIFACT/chunk/overworld/0.0/0.0.nbt"
expect_rc 3 run_oracle "$MISSING_ARTIFACT" --out "$SCRATCH_DIR/missing-artifact-out"
grep -q "source capture prerequisite unavailable" "$SCRATCH_DIR/stderr"

HASH_CORRUPTION=$(copy_fixtures hash-corruption)
python3 - "$HASH_CORRUPTION/chunk/overworld/0.0/0.0.nbt" <<'PY'
import sys
path = sys.argv[1]
data = bytearray(open(path, "rb").read())
data[-1] ^= 1
open(path, "wb").write(data)
PY
expect_rc 1 run_oracle "$HASH_CORRUPTION" --out "$SCRATCH_DIR/hash-corruption-out"

default_output="$FIXTURES"
expect_rc 1 run_oracle --out "$default_output"
expect_rc 1 run_oracle --out "$FIXTURES/anvil-roundtrip-descendant"
expect_rc 1 run_oracle --out "$(dirname "$FIXTURES")"
ln -s "$FIXTURES" "$SCRATCH_DIR/output-alias"
expect_rc 1 run_oracle --out "$SCRATCH_DIR/output-alias"

OUTPUT_VICTIM="$SCRATCH_DIR/output-victim"
mkdir "$OUTPUT_VICTIM"
printf 'keep\n' >"$OUTPUT_VICTIM/KEEP"
ln -s "$OUTPUT_VICTIM" "$SCRATCH_DIR/output-victim-alias"
expect_rc 1 run_oracle --out "$SCRATCH_DIR/output-victim-alias"
[ -f "$OUTPUT_VICTIM/KEEP" ]

ln -s "$FIXTURES" "$SCRATCH_DIR/root-alias"
expect_rc 1 run_oracle "$SCRATCH_DIR/root-alias" --out "$SCRATCH_DIR/root-alias-out"

SYMLINK_MANIFEST=$(copy_fixtures symlink-manifest)
mv "$SYMLINK_MANIFEST/manifest.json" "$SYMLINK_MANIFEST/manifest.real"
ln -s manifest.real "$SYMLINK_MANIFEST/manifest.json"
expect_rc 1 run_oracle "$SYMLINK_MANIFEST" --out "$SCRATCH_DIR/symlink-manifest-out"

SYMLINK_CHUNK_ROOT=$(copy_fixtures symlink-chunk-root)
mv "$SYMLINK_CHUNK_ROOT/chunk" "$SYMLINK_CHUNK_ROOT/chunk.real"
ln -s chunk.real "$SYMLINK_CHUNK_ROOT/chunk"
expect_rc 1 run_oracle "$SYMLINK_CHUNK_ROOT" --out "$SCRATCH_DIR/symlink-chunk-root-out"

SYMLINK_CAPTURE=$(copy_fixtures symlink-capture)
mv "$SYMLINK_CAPTURE/server.properties" "$SYMLINK_CAPTURE/server.real"
ln -s server.real "$SYMLINK_CAPTURE/server.properties"
expect_rc 1 run_oracle "$SYMLINK_CAPTURE" --out "$SCRATCH_DIR/symlink-capture-out"

CORPUS_MISMATCH=$(copy_fixtures corpus-mismatch)
mutate_manifest "$CORPUS_MISMATCH" 'data["chunk-count"] = 433'
expect_rc 1 run_oracle "$CORPUS_MISMATCH" --out "$SCRATCH_DIR/corpus-mismatch-out"

METADATA_MISMATCH=$(copy_fixtures metadata-mismatch)
mutate_manifest "$METADATA_MISMATCH" 'next(c for c in data["captured"] if c["path"].startswith("chunk/"))["chunk"] = "99.99"'
expect_rc 1 run_oracle "$METADATA_MISMATCH" --out "$SCRATCH_DIR/metadata-mismatch-out"

TRAILING_PAYLOAD=$(copy_fixtures trailing-payload)
python3 - "$TRAILING_PAYLOAD/chunk/overworld/0.0/0.0.nbt" <<'PY'
import sys
path = sys.argv[1]
data = bytearray(open(path, "rb").read())
data.extend(b"\x00")
open(path, "wb").write(data)
PY
expect_rc 1 run_oracle "$TRAILING_PAYLOAD" --out "$SCRATCH_DIR/trailing-payload-out"

echo 'anvil-roundtrip-v1a CLI hostile tri-state tests passed'
