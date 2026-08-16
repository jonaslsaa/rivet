#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  printf 'usage: %s REPO_DIR COMMAND [ARGUMENT ...]\n' "$0" >&2
  exit 2
fi

repo_dir=$(cd "$1" && pwd -P)
shift
# shellcheck source=scripts/cargo-target-dir.sh
source "$repo_dir/scripts/cargo-target-dir.sh"
cargo_export_namespace "$repo_dir"

held_lock_fds() {
  [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ] || return 1
  [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ] || return 1
  { : >&8; } 2>/dev/null && { : >&9; } 2>/dev/null
}

if held_lock_fds; then
  exec "$@"
fi

namespace=$(cargo_namespace_json "$repo_dir")
group_lock=$(printf '%s\n' "$namespace" | python3 -c 'import json,sys; print(json.load(sys.stdin)["group_lock"])')
checkout_lock=$(printf '%s\n' "$namespace" | python3 -c 'import json,sys; print(json.load(sys.stdin)["checkout_lock"])')
mkdir -p "$(dirname "$group_lock")" "$(dirname "$checkout_lock")"

python3 - "$group_lock" "$checkout_lock" "$@" <<'PY'
import fcntl
import os
import sys

paths = sys.argv[1:3]
argv = sys.argv[3:]
fds = []
try:
    for path in paths:
        fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
        fcntl.flock(fd, fcntl.LOCK_EX)
        fds.append(fd)
    os.dup2(fds[0], 8)
    os.dup2(fds[-1], 9)
    os.set_inheritable(8, True)
    os.set_inheritable(9, True)
    os.environ["RIVET_BUILD_GROUP_LOCK_FD"] = "8"
    os.environ["RIVET_BUILD_LOCK_FD"] = "9"
    os.execvp(argv[0], argv)
finally:
    for fd in fds:
        try:
            os.close(fd)
        except OSError:
            pass
PY
