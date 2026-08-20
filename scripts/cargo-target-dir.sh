#!/bin/bash
# Resolve the Cargo target directory for one checkout. Linked worktrees must not
# share Cargo fingerprints: Cargo keys packages by package identity, not by
# checkout path, so a shared target can reuse a dependency built from another
# worktree when that other checkout's source mtimes still satisfy the dep-info.

cargo_project_root_for() {
  local repo_dir=$1 common_dir
  common_dir=$(git -C "$repo_dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
  if [ -n "$common_dir" ]; then
    (cd "${common_dir%/*}" && pwd -P)
  else
    (cd "$repo_dir" && pwd -P)
  fi
}

cargo_target_dir_for() {
  if [ "$#" -ne 1 ]; then
    printf 'usage: cargo_target_dir_for REPO_DIR\n' >&2
    return 2
  fi

  local repo_dir expected override canonical override_parent override_name
  repo_dir=$(cd "$1" && pwd -P) || return 2
  expected="$repo_dir/target"
  override=${CARGO_TARGET_DIR:-}

  if [ -n "$override" ]; then
    case "$override" in
      /*) ;;
      *)
        printf 'CARGO_TARGET_DIR must be absolute for checkout-isolated Cargo builds (got %s)\n' "$override" >&2
        return 2
        ;;
    esac
    override_parent=${override%/*}
    override_name=${override##*/}
    [ -n "$override_parent" ] || override_parent=/
    canonical=$(cd "$override_parent" 2>/dev/null && printf '%s/%s\n' "$(pwd -P)" "$override_name") || {
      printf 'cannot resolve CARGO_TARGET_DIR parent for %s\n' "$override" >&2
      return 2
    }
    if [ "$canonical" != "$expected" ]; then
      printf 'CARGO_TARGET_DIR must be the current checkout target %s (got %s)\n' "$expected" "$canonical" >&2
      return 2
    fi
  fi

  mkdir -p "$expected"
  (cd "$expected" && pwd -P)
}

if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  cargo_target_dir_for "$@"
fi
