#!/usr/bin/env bash
# Run both halves of the surrogate ground-truth probe: the Java (JDK+netty+Gson)
# oracle side and the Rust counter-probe. Prints JSON Lines from each; stdout
# of the Java probe first, then the Rust probe.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

echo "== Java probe (JDK/netty/Gson ground truth)" >&2
./run-java.sh

echo "== Rust counter-probe" >&2
PATH="$HOME/.cargo/bin:$PATH" cargo run --quiet --manifest-path Cargo.toml
