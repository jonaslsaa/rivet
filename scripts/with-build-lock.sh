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
project_root=$(cargo_project_root_for "$repo_dir")
lock_path="$project_root/cargo-build.lock"
if [ -L "$lock_path" ] || { [ -e "$lock_path" ] && [ ! -f "$lock_path" ]; }; then
  printf 'repository build lock must be a regular file: %s\n' "$lock_path" >&2
  exit 2
fi
touch "$lock_path"
# The lock utility differs by platform: lockf (macOS) and flock (util-linux,
# Linux/WSL) both take a lock file followed by the command to run under it.
# Targets stay checkout-local for fingerprint correctness, while this one
# repository-wide lock keeps heavyweight builds from competing for resources.
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
