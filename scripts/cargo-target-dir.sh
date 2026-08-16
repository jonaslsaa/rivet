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

cargo_namespace_value_nul() {
  [ "$#" -eq 2 ] || { printf 'usage: cargo_namespace_value_nul REPO_DIR KEY\n' >&2; return 2; }
  python3 - "$_cargo_provenance" "$1" "$2" <<'PY'
import importlib.util
import os
import pathlib
import sys

spec = importlib.util.spec_from_file_location("cargo_provenance", sys.argv[1])
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)
value = module.namespace(pathlib.Path(sys.argv[2]))[sys.argv[3]]
sys.stdout.buffer.write(os.fsencode(value) + b"\0")
PY
}

cargo_namespace_path_for() {
  [ "$#" -eq 2 ] || { printf 'usage: cargo_namespace_path_for REPO_DIR KEY\n' >&2; return 2; }
  local value
  IFS= read -r -d '' value < <(cargo_namespace_value_nul "$1" "$2") || return 2
  printf -v CARGO_NAMESPACE_PATH_RESULT '%s' "$value"
}

cargo_build_locks_held() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_build_locks_held REPO_DIR\n' >&2; return 2; }
  [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ] || return 1
  [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ] || return 1
  python3 - "$_cargo_provenance" "$1" <<'PY'
import fcntl
import importlib.util
import os
import pathlib
import stat
import sys

spec = importlib.util.spec_from_file_location("cargo_provenance", sys.argv[1])
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)
namespace = module.namespace(pathlib.Path(sys.argv[2]))

def held(fd_number: int, path: str) -> bool:
    expected = os.lstat(path)
    actual = os.fstat(fd_number)
    if (
        stat.S_ISLNK(expected.st_mode)
        or not stat.S_ISREG(expected.st_mode)
        or expected.st_nlink != 1
        or not stat.S_ISREG(actual.st_mode)
        or actual.st_nlink != 1
        or expected.st_dev != actual.st_dev
        or expected.st_ino != actual.st_ino
    ):
        return False
    try:
        fcntl.flock(fd_number, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        return False
    else:
        return True

try:
    if not held(8, namespace["group_lock"]) or not held(9, namespace["checkout_lock"]):
        raise SystemExit(1)
except (OSError, ValueError, KeyError):
    raise SystemExit(1)
PY
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
  cargo_namespace_path_for "$repo_dir" target || return
  expected=$CARGO_NAMESPACE_PATH_RESULT
  CARGO_TARGET_DIR_RESULT=$expected
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
import os
import pathlib
import stat
import sys

path = pathlib.Path(sys.argv[1]).absolute()
missing = []
probe = path
while True:
    try:
        info = probe.lstat()
    except FileNotFoundError:
        missing.append(probe.name)
        if probe == probe.parent:
            print(f"cargo target: cannot resolve parent of {path}", file=sys.stderr)
            raise SystemExit(2)
        probe = probe.parent
        continue
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        print(f"cargo target: managed path is not a real directory: {probe}", file=sys.stderr)
        raise SystemExit(2)
    resolved = probe.resolve(strict=True)
    for name in reversed(missing):
        resolved /= name
    sys.stdout.buffer.write(os.fsencode(str(resolved)) + b"\x1f")
    break
PY
) || return 2
    canonical=${canonical%$'\x1f'}
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
        try:
            current.mkdir()
        except FileExistsError:
            pass
        try:
            info = current.lstat()
        except FileNotFoundError:
            print(f"cargo target: managed path disappeared during creation: {current}", file=sys.stderr)
            raise SystemExit(2)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        print(f"cargo target: managed path is not a real directory: {current}", file=sys.stderr)
        raise SystemExit(2)
PY
  printf '%s\n' "$expected"
}

cargo_prepare_namespace() {
  [ "$#" -eq 1 ] || { printf 'usage: cargo_prepare_namespace REPO_DIR\n' >&2; return 2; }
  local repo_dir=$1 target
  cargo_target_dir_for "$repo_dir" >/dev/null || return
  target=$CARGO_TARGET_DIR_RESULT
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
  cargo_prepare_namespace "$repo_dir" >/dev/null || return
  target=$CARGO_TARGET_DIR_RESULT
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
