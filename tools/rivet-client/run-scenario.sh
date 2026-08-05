#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$tool_dir"

# Build all binaries (rivet-client + run-scenario) so the runner can find the
# client binary next to itself in target/debug/.
cargo build --locked
exec ./target/debug/run-scenario "$@"
