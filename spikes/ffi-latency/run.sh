#!/usr/bin/env bash
# Build the spike cdylib + Java benchmark and run it.
#
# Toolchain (exact versions captured in the benchmark JSON):
#   - Rust: rustc/cargo pinned by this repo's rust-toolchain.toml (channel 1.97.1)
#   - Java: JDK 22+ (FFM/Panama). Tested on Temurin 25.0.2 (arm64).
#   - OS:   macOS arm64 (dylib). On Linux, the dylib is a .so and
#           SymbolLookup.libraryLookup resolves it the same way.
#
# Usage:
#   ./run.sh                 # default output: results/benchmark.json
#   OUT=results/run-1.json ./run.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

RUST_TOOLCHAIN="$(sed -n 's/^channel = //p' ../../rust-toolchain.toml 2>/dev/null || echo 'default')"

echo "== rust toolchain: $RUST_TOOLCHAIN (rustc $(rustc --version))"
echo "== java: $(java -version 2>&1 | head -1)"

echo "== building cdylib (release)"
cargo build --release --manifest-path Cargo.toml

LIB="$HERE/target/release/libffi_latency_spike.dylib"
if [[ ! -f "$LIB" ]]; then
  echo "error: expected cdylib at $LIB" >&2
  exit 1
fi

echo "== compiling Java benchmark"
mkdir -p java/out
javac --release 22 -d java/out $(find java -name '*.java')

OUT="${OUT:-results/benchmark.json}"
mkdir -p "$(dirname "$OUT")"
echo "== running benchmark (output: $OUT)"
java -Dffi.lib="$LIB" -Dout="$OUT" --enable-native-access=ALL-UNNAMED \
  -cp java/out rivet.ffi.Benchmark

echo "== done: $OUT"
