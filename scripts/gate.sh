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

echo "==> oracle verify (M0 sanity gate: green against vanilla itself)"
if [ -n "${RIVET_ORACLE_JAR:-}" ] || [ -f tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar ] || \
   ls working/Paper/paper-server/build/libs/paper-paperclip*.jar >/dev/null 2>&1; then
  cargo run -q -p rivet-oracle -- verify
else
  echo "    SKIPPED (no paperclip jar: set RIVET_ORACLE_JAR or materialize working/Paper first)"
fi

echo "GATE GREEN"
