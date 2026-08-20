#!/bin/bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  printf 'usage: %s REPO_DIR COMMAND [ARGUMENT ...]\n' "$0" >&2
  exit 2
fi

repo_dir=$(cd "$1" && pwd -P)
shift

# shellcheck source=scripts/cargo-target-dir.sh
# shellcheck disable=SC1091  # sources a sibling script; shellcheck only follows it with -x
source "$repo_dir/scripts/cargo-target-dir.sh"
target_dir=$(cargo_target_dir_for "$repo_dir")
lock_path="${target_dir}.lock"
mkdir -p "$(dirname "$lock_path")"
touch "$lock_path"
# The lock utility differs by platform: lockf (macOS) and flock (util-linux,
# Linux/WSL) both take a lock file followed by the command to run under it.
# Select whichever the host provides so the shared-target contract works on
# both CI and local development machines.
if command -v /usr/bin/lockf >/dev/null 2>&1; then
  exec /usr/bin/lockf "$lock_path" /usr/bin/env -u RIVET_BUILD_LOCK_HELD \
    RIVET_BUILD_LOCK_HELD=1 CARGO_TARGET_DIR="$target_dir" "$@"
else
  exec /usr/bin/flock "$lock_path" /usr/bin/env -u RIVET_BUILD_LOCK_HELD \
    RIVET_BUILD_LOCK_HELD=1 CARGO_TARGET_DIR="$target_dir" "$@"
fi
