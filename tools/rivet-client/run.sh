#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_dir="$(cd "$tool_dir/../.." && pwd -P)"

if [ "${RIVET_BUILD_LOCK_HELD:-0}" != 1 ]; then
  exec "$repo_dir/scripts/with-build-lock.sh" "$repo_dir" "$tool_dir/run.sh" "$@"
fi

cd "$tool_dir"
exec cargo run --locked "$@"
