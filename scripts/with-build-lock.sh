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
env_cmd=$(command -v env) || {
  echo "env not found in PATH" >&2
  exit 1
}
if lock_cmd=$(command -v lockf); then
  :
elif lock_cmd=$(command -v flock); then
  :
else
  echo "neither lockf nor flock found in PATH" >&2
  exit 1
fi
exec "$lock_cmd" "$lock_path" "$env_cmd" -u RIVET_BUILD_LOCK_HELD \
  RIVET_BUILD_LOCK_HELD=1 CARGO_TARGET_DIR="$target_dir" "$@"
