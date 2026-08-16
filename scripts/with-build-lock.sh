#!/bin/bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  printf 'usage: %s REPO_DIR COMMAND [ARGUMENT ...]\n' "$0" >&2
  exit 2
fi

repo_dir=$(cd "$1" && pwd -P)
shift

# shellcheck source=scripts/cargo-target-dir.sh
source "$repo_dir/scripts/cargo-target-dir.sh"
target_dir=$(cargo_target_dir_for "$repo_dir")
lock_path="${target_dir}.lock"
mkdir -p "$(dirname "$lock_path")"
touch "$lock_path"
exec /usr/bin/lockf "$lock_path" /usr/bin/env -u RIVET_BUILD_LOCK_HELD \
  RIVET_BUILD_LOCK_HELD=1 CARGO_TARGET_DIR="$target_dir" "$@"
