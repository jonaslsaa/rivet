#!/bin/bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  printf 'usage: %s REPO_DIR COMMAND [ARGUMENT ...]\n' "$0" >&2
  exit 2
fi

repo_dir=$1
shift

common_dir=$(git -C "$repo_dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
if [ -n "$common_dir" ]; then
  project_root=$(cd "$(dirname "$common_dir")" && pwd -P)
else
  project_root=$(cd "$repo_dir" && pwd -P)
fi

target_dir=${CARGO_TARGET_DIR:-$project_root/target-agent-shared}
case "$target_dir" in
  /*) ;;
  *) target_dir="$project_root/$target_dir" ;;
esac
if [ -d "$target_dir" ]; then
  target_dir=$(cd "$target_dir" && pwd -P)
else
  target_parent=$(cd "$(dirname "$target_dir")" && pwd -P)
  target_dir="$target_parent/$(basename "$target_dir")"
fi

lock_path="${target_dir}.lock"
mkdir -p "$(dirname "$lock_path")"
exec /usr/bin/lockf "$lock_path" /usr/bin/env -u RIVET_BUILD_LOCK_HELD RIVET_BUILD_LOCK_HELD=1 "$@"
