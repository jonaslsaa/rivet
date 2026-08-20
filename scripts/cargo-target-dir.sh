#!/bin/bash
# Resolve the one Cargo target directory shared by this repository's worktrees.
# The path is derived from the git common directory, never from a checkout path.

cargo_project_root_for() {
  local repo_dir=$1 common_dir
  common_dir=$(git -C "$repo_dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
  if [ -n "$common_dir" ]; then
    (cd "$(dirname "$common_dir")" && pwd -P)
  else
    (cd "$repo_dir" && pwd -P)
  fi
}

cargo_target_dir_for() {
  if [ "$#" -ne 1 ]; then
    printf 'usage: cargo_target_dir_for REPO_DIR\n' >&2
    return 2
  fi

  local repo_dir=$1 project_root target_dir target_parent
  project_root=$(cargo_project_root_for "$repo_dir") || {
    printf 'cannot resolve repository directory %s\n' "$repo_dir" >&2
    return 2
  }
  target_dir=${CARGO_TARGET_DIR:-$project_root/target-agent-shared}
  case "$target_dir" in
    /*) ;;
    *)
      printf 'CARGO_TARGET_DIR must be absolute for shared Cargo locking (got %s)\n' "$target_dir" >&2
      return 2
      ;;
  esac

  target_parent=$(dirname "$target_dir")
  mkdir -p "$target_parent"
  if [ -d "$target_dir" ]; then
    (cd "$target_dir" && pwd -P)
  else
    target_parent=$(cd "$target_parent" && pwd -P) || return 2
    printf '%s/%s\n' "$target_parent" "$(basename "$target_dir")"
  fi
}

if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  cargo_target_dir_for "$@"
fi
