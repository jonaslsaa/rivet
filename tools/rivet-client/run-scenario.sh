#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_dir="$(cd "$tool_dir/../.." && pwd -P)"

if [ "${RIVET_BUILD_LOCK_HELD:-0}" != 1 ]; then
  exec "$repo_dir/scripts/with-build-lock.sh" "$repo_dir" "$tool_dir/run-scenario.sh" "$@"
fi

resolved_target_dir() {
  local manifest=$1
  cargo metadata --locked --no-deps --format-version 1 --manifest-path "$manifest" \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

cd "$tool_dir"
cargo build --locked
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
  previous=$argument
done
if [ "$desired_server" = 1 ]; then
  (
    cd "$repo_dir"
    cargo build --locked -p rivet-server
  )
fi

tool_target_dir="$(resolved_target_dir "$tool_dir/Cargo.toml")"
export CARGO_TARGET_DIR="$tool_target_dir"
exec "$tool_target_dir/debug/run-scenario" "$@"
