#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_dir="$(cd "$tool_dir/../.." && pwd -P)"

if [ "${RIVET_BUILD_LOCK_HELD:-0}" != 1 ]; then
  exec "$repo_dir/scripts/with-build-lock.sh" "$repo_dir" "$tool_dir/run-scenario.sh" "$@"
fi

# Resolve before the first Cargo command so both the nested tool workspace and
# the main workspace use the same target, even when invoked outside the repo.
# shellcheck source=scripts/cargo-target-dir.sh
# shellcheck disable=SC1091  # sources a sibling script; shellcheck only follows it with -x
source "$repo_dir/scripts/cargo-target-dir.sh"
resolved_target_dir="$(cargo_target_dir_for "$repo_dir")"
export CARGO_TARGET_DIR="$resolved_target_dir"

tool_target_dir="$CARGO_TARGET_DIR"
cd "$tool_dir"
cargo build --locked --bin run-scenario
cargo test --locked --bin run-scenario

desired_server=0
case "${1:-}" in
  dwell | kick | load-world | loaded-world | recenter | generated-world) desired_server=1 ;;
esac
previous=""
for argument in "$@"; do
  if [ "$previous" = "--server" ] && { [ "$argument" = rivet ] || [ "$argument" = both ]; }; then
    desired_server=1
  fi
  previous="$argument"
done
if [ "$desired_server" = 1 ]; then
  (
    cd "$repo_dir"
    cargo build --locked -p rivet-server
  )
fi

exec "$tool_target_dir/debug/run-scenario" "$@"
