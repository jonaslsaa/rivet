#!/bin/bash
# The merge gate. Run before merging any PR (and at the end of every wave).
# No hosted CI by design — this script IS the gate; a red gate blocks the merge.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> cargo fmt --check"
cargo fmt --all --check

echo "==> cargo clippy (-Dwarnings)"
RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets

echo "==> cargo test"
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi

echo "GATE GREEN"
