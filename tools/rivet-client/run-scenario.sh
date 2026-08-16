#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_dir="$(cd "$tool_dir/../.." && pwd -P)"
# shellcheck source=../../scripts/cargo-target-dir.sh
# shellcheck disable=SC1091
source "$repo_dir/scripts/cargo-target-dir.sh"
if ! { [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ] && [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ] \
    && { : >&8; } 2>/dev/null && { : >&9; } 2>/dev/null; }; then
  exec "$repo_dir/scripts/with-build-lock.sh" "$repo_dir" "$0" "$@"
fi
cargo_export_namespace "$repo_dir"
target_dir="$CARGO_TARGET_DIR"
cd "$tool_dir"

cargo build --locked
cargo test --locked --bin run-scenario

needs_rivet=0
case "${1:-}" in
  dwell | kick | load-world | loaded-world | recenter | generated-world) needs_rivet=1 ;;
esac
previous=""
for argument in "$@"; do
  if [ "$previous" = "--server" ] && { [ "$argument" = rivet ] || [ "$argument" = both ]; }; then
    needs_rivet=1
  fi
  previous=$argument
done
if [ "$needs_rivet" = 1 ]; then
  (cd "$repo_dir" && cargo build --locked -p rivet-server)
fi
cargo_stamp_binaries "$repo_dir"
exec "$target_dir/debug/run-scenario" "$@"
