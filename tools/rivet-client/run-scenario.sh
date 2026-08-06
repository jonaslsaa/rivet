#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$tool_dir"

# Build all binaries (rivet-client + run-scenario) so the runner can find the
# client binary next to itself in target/debug/.
cargo build --locked

# Run the runner's own unit tests (port isolation, ServerKind, process-lifecycle
# cleanup, exit-code classification) before any scenario. Cargo has just built
# the package, so this adds no new dependency compilation; the azalea build cost
# is already paid by `cargo build --locked` above.
cargo test --locked --bin run-scenario

# Build the rivet-server binary (main workspace, stable toolchain) only when a
# mode needs it (--server rivet / --server both), so the Paper-only self-check
# stays exactly as fast as before. Run from the repo root so cargo resolves the
# main workspace's stable toolchain (the nested workspace pins nightly).
needs_rivet=0
prev=""
for a in "$@"; do
  if [ "$prev" = "--server" ] && { [ "$a" = "rivet" ] || [ "$a" = "both" ]; }; then
    needs_rivet=1
  fi
  prev="$a"
done
if [ "$needs_rivet" = 1 ]; then
  (
    cd "$tool_dir/../.."
    cargo build --locked -p rivet-server
  )
fi

exec ./target/debug/run-scenario "$@"
