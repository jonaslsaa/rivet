#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
# shellcheck source=../../scripts/cargo-target-dir.sh
# shellcheck disable=SC1091
source "$repo_dir/scripts/cargo-target-dir.sh"
if ! { [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ] && [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ] \
    && { : >&8; } 2>/dev/null && { : >&9; } 2>/dev/null; }; then
  exec "$repo_dir/scripts/with-build-lock.sh" "$repo_dir" "$0" "$@"
fi
cargo_export_namespace "$repo_dir"
cd "$repo_dir"
exec cargo run --locked -p rivet-oracle -- "$@"
