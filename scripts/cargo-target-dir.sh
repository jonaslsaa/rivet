#!/usr/bin/env bash
set -euo pipefail

_cargo_target_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
_cargo_provenance="$_cargo_target_script_dir/cargo-provenance.py"

cargo_namespace_json() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_namespace_json REPO_DIR\n' >&2; return 2; }
  python3 "$_cargo_provenance" namespace "$1"
}

cargo_namespace_value() {
  [ "$#" -eq 2 ] || { printf 'usage: cargo_namespace_value REPO_DIR KEY\n' >&2; return 2; }
  cargo_namespace_json "$1" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d[sys.argv[1]])' "$2"
}

cargo_project_root_for() {
  cargo_namespace_value "$1" top_level
}

cargo_repo_id_for() {
  cargo_namespace_value "$1" repo_id
}

cargo_checkout_id_for() {
  cargo_namespace_value "$1" checkout_id
}

cargo_target_dir_for() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_target_dir_for REPO_DIR\n' >&2; return 2; }
  local repo_dir=$1 expected override
  expected=$(cargo_namespace_value "$repo_dir" target) || return
  local override_name
  for override_name in CARGO_TARGET_DIR RIVET_CARGO_TARGET_DIR; do
    override=${!override_name:-}
    if [ -z "$override" ]; then
      continue
    fi
    case "$override" in
      /*) ;;
      *) printf '%s must be absolute: %s\n' "$override_name" "$override" >&2; return 2 ;;
    esac
    local canonical
    canonical=$(python3 - "$override" <<'PY'
import pathlib, stat, sys
p = pathlib.Path(sys.argv[1])
try:
    info = p.lstat()
except FileNotFoundError:
    info = None
if info is not None and (stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode)):
    print(f"cargo target: managed target must be a real directory: {p}", file=sys.stderr)
    raise SystemExit(2)
try:
    print((p.parent.resolve(strict=True) / p.name))
except OSError as exc:
    print(f"cargo target: cannot canonicalize {p}: {exc}", file=sys.stderr)
    raise SystemExit(2)
PY
) || return 2
    if [ "$canonical" != "$expected" ]; then
      printf '%s is foreign; expected %s, got %s\n' "$override_name" "$expected" "$canonical" >&2
      return 2
    fi
  done
  python3 - "$expected" <<'PY'
import pathlib, stat, sys
path = pathlib.Path(sys.argv[1]).absolute()
current = pathlib.Path(path.anchor)
for component in path.parts[1:]:
    current /= component
    try:
        info = current.lstat()
    except FileNotFoundError:
        current.mkdir()
        info = current.lstat()
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        print(f"cargo target: managed path is not a real directory: {current}", file=sys.stderr)
        raise SystemExit(2)
PY
  printf '%s\n' "$expected"
}

cargo_prepare_namespace() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_prepare_namespace REPO_DIR\n' >&2; return 2; }
  local repo_dir=$1 target
  target=$(cargo_target_dir_for "$repo_dir") || return
  python3 "$_cargo_provenance" ensure "$repo_dir" >/dev/null || return
  printf '%s\n' "$target"
}

cargo_state_digest_for() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_state_digest_for REPO_DIR\n' >&2; return 2; }
  python3 "$_cargo_provenance" digest "$1"
}

cargo_prepare_binaries() {
  [ "$#" -ge 2 ] || { printf 'usage: cargo_prepare_binaries REPO_DIR PATH ...\n' >&2; return 2; }
  local repo_dir=$1
  shift
  python3 "$_cargo_provenance" prepare "$repo_dir" "$@"
}

cargo_stamp_binaries() {
  [ "$#" -ge 2 ] || { printf 'usage: cargo_stamp_binaries REPO_DIR PATH ...\n' >&2; return 2; }
  local repo_dir=$1
  shift
  python3 "$_cargo_provenance" stamp "$repo_dir" "$@"
}

cargo_binary_for() {
  [ "$#" -ge 2 ] && [ "$#" -le 3 ] || {
    printf 'usage: cargo_binary_for REPO_DIR NAME [PROFILE]\n' >&2
    return 2
  }
  local repo_dir=$1 name=$2 profile=${3:-debug} override env_name target path
  case "$name" in
    rivet-client) env_name=CLIENT ;;
    rivet-server) env_name=SERVER ;;
    rivet-oracle) env_name=ORACLE ;;
    rivet-capture) env_name=CAPTURE ;;
    *) env_name=$(printf '%s' "$name" | tr '[:lower:]-' '[:upper:]_') ;;
  esac
  override="RIVET_${env_name}_BIN"
  if [ -n "${!override:-}" ]; then
    path=${!override}
    case "$path" in /*) ;; *) printf '%s must be absolute: %s\n' "$override" "$path" >&2; return 2 ;; esac
    [ -f "$path" ] || { printf '%s is not a file: %s\n' "$override" "$path" >&2; return 2; }
    python3 "$_cargo_provenance" verify "$repo_dir" "$path" || return 2
    printf '%s\n' "$path"
    return 0
  fi
  target=$(cargo_target_dir_for "$repo_dir") || return
  path="$target/$profile/$name"
  [ -f "$path" ] || { printf 'managed binary is missing: %s\n' "$path" >&2; return 3; }
  python3 "$_cargo_provenance" verify "$repo_dir" "$path" >/dev/null || {
    printf 'managed binary provenance is invalid: %s\n' "$path" >&2
    return 3
  }
  printf '%s\n' "$path"
}

cargo_export_namespace() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_export_namespace REPO_DIR\n' >&2; return 2; }
  local repo_dir=$1 target
  target=$(cargo_prepare_namespace "$repo_dir") || return
  export CARGO_TARGET_DIR="$target"
  export RIVET_CARGO_TARGET_DIR="$target"
  local repo_id checkout_id
  repo_id=$(cargo_repo_id_for "$repo_dir") || return
  checkout_id=$(cargo_checkout_id_for "$repo_dir") || return
  export RIVET_CARGO_REPO_ID="$repo_id"
  export RIVET_CARGO_CHECKOUT_ID="$checkout_id"
  local head state_digest
  head=$(git -C "$repo_dir" rev-parse HEAD) || return
  state_digest=$(cargo_state_digest_for "$repo_dir") || return
  export RIVET_CARGO_HEAD="$head"
  export RIVET_CARGO_STATE_DIGEST="$state_digest"
}

if [[ "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  case "${1:-}" in
    namespace) shift; cargo_namespace_json "$@" ;;
    target) shift; cargo_target_dir_for "$@" ;;
    digest) shift; cargo_state_digest_for "$@" ;;
    ensure) shift; cargo_prepare_namespace "$@" ;;
    binary) shift; cargo_binary_for "$@" ;;
    *) printf 'usage: %s {namespace|target|digest|ensure|binary} ...\n' "$0" >&2; exit 2 ;;
  esac
fi
