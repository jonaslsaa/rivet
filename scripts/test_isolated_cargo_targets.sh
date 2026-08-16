#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }
pass() { printf 'ok:   %s\n' "$1"; }

mkdir -p "$TMP/cache"
ROOT="$(cd "$TMP/cache" && pwd -P)"
ns() { env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/cargo-target-dir.sh" namespace "$REPO_DIR"; }
TARGET="$(ns | python3 -c 'import json,sys; print(json.load(sys.stdin)["target"])')"
REPO_ID="$(ns | python3 -c 'import json,sys; print(json.load(sys.stdin)["repo_id"])')"
CHECKOUT_ID="$(ns | python3 -c 'import json,sys; print(json.load(sys.stdin)["checkout_id"])')"

case "$TARGET" in
  "$ROOT"/*/"$CHECKOUT_ID"/iterative) : ;;
  *) fail "target is not rooted at ROOT/repo-id/checkout-id/iterative: $TARGET" ;;
esac
[ "$REPO_ID" != "$CHECKOUT_ID" ] || fail "repo and checkout IDs unexpectedly collide"
[ "$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/cargo-target-dir.sh" target "$REPO_DIR")" = "$TARGET" ] || fail "target is not stable"
pass "canonical repo and checkout IDs produce a stable external iterative target"

if RIVET_CARGO_TARGET_ROOT=relative-cache "$SCRIPT_DIR/cargo-target-dir.sh" namespace "$REPO_DIR" >/dev/null 2>&1; then
  fail "relative target root was accepted"
fi
if env RIVET_CARGO_TARGET_ROOT="$TMP/foreign" CARGO_TARGET_DIR="$TMP/foreign" "$SCRIPT_DIR/cargo-target-dir.sh" target "$REPO_DIR" >/dev/null 2>&1; then
  fail "foreign CARGO_TARGET_DIR was accepted"
fi
# shellcheck disable=SC2016
if env RIVET_CARGO_TARGET_ROOT="$TMP/foreign" CARGO_TARGET_DIR="$TMP/foreign" bash -c \
  'source "$1"; cargo_export_namespace "$2"' bash "$SCRIPT_DIR/cargo-target-dir.sh" "$REPO_DIR" >/dev/null 2>&1; then
  fail "foreign CARGO_TARGET_DIR escaped through namespace export"
fi
pass "relative roots and foreign target overrides are rejected"

env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/cargo-target-dir.sh" ensure "$REPO_DIR" >/dev/null
hostile_file="$REPO_DIR/.isolated-target-digest-hostile"
printf '%s\n' old > "$hostile_file"
old_digest="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/cargo-target-dir.sh" digest "$REPO_DIR")"
touch -m -t 200001010000 "$hostile_file"
printf '%s\n' changed > "$hostile_file"
new_digest="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/cargo-target-dir.sh" digest "$REPO_DIR")"
[ "$old_digest" != "$new_digest" ] || fail "strict digest ignored an untracked nonignored file"
rm -f "$hostile_file"
pass "strict digest detects content changes even with older mtimes"

lock_probe="$TMP/lock-probe.sh"
cat > "$lock_probe" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
repo=$1
exec "$repo/scripts/with-build-lock.sh" "$repo" sh -c '
  [ "${RIVET_BUILD_GROUP_LOCK_FD:-}" = 8 ]
  [ "${RIVET_BUILD_LOCK_FD:-}" = 9 ]
  : >&8
  : >&9
  printf lock-ok
'
SH
chmod +x "$lock_probe"
[ "$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" "$SCRIPT_DIR/with-build-lock.sh" "$REPO_DIR" "$lock_probe" "$REPO_DIR")" = lock-ok ] || fail "nested lock propagation failed"
pass "nested build lock propagation uses an inherited lock descriptor"

BIN="$TARGET/debug/rivet-client"
mkdir -p "$(dirname "$BIN")"
printf 'binary-a\n' > "$BIN"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$ROOT" python3 "$SCRIPT_DIR/cargo-provenance.py" sidecar "$REPO_DIR" "$BIN" >/dev/null
copy="$TMP/copied-binary"
cp "$BIN" "$copy"
cp "$BIN.rivet-provenance" "$copy.rivet-provenance"
if env RIVET_CARGO_TARGET_ROOT="$ROOT" CARGO_TARGET_DIR="$TARGET" python3 "$SCRIPT_DIR/cargo-provenance.py" verify "$REPO_DIR" "$copy" >/dev/null 2>&1; then
  fail "copied artifact with unchanged sidecar was accepted"
fi
printf 'tampered\n' >> "$BIN"
if env RIVET_CARGO_TARGET_ROOT="$ROOT" CARGO_TARGET_DIR="$TARGET" python3 "$SCRIPT_DIR/cargo-provenance.py" verify "$REPO_DIR" "$BIN" >/dev/null 2>&1; then
  fail "tampered artifact was accepted"
fi
pass "copied and tampered artifacts fail provenance"

FAKE="$TMP/fake"
mkdir -p "$FAKE/scripts" "$FAKE/tools"
git -C "$FAKE" init -q
git -C "$FAKE" config user.email test@example.invalid
git -C "$FAKE" config user.name test
printf fake > "$FAKE/README"
git -C "$FAKE" add README
git -C "$FAKE" commit -qm initial
cp "$SCRIPT_DIR/cargo-provenance.py" "$FAKE/scripts/"
cp "$SCRIPT_DIR/cargo-target-dir.sh" "$FAKE/scripts/"
cp "$SCRIPT_DIR/with-build-lock.sh" "$FAKE/scripts/"
cp "$SCRIPT_DIR/prune-worktrees.sh" "$FAKE/scripts/"
chmod +x "$FAKE/scripts"/*.sh "$FAKE/scripts"/*.py
printf root-sentinel > "$TMP/root-sentinel"
FAKE_ROOT="$(mktemp -d "$TMP-fake-cache.XXXXXX")"
trap 'rm -rf "$TMP" "$FAKE_ROOT"' EXIT
FAKE_TARGET="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$FAKE_ROOT" "$FAKE/scripts/cargo-target-dir.sh" ensure "$FAKE")"
printf old > "$FAKE_TARGET/old-file"
touch -m -t 200001010000 "$FAKE_TARGET" "$FAKE_TARGET/old-file"
printf unmarked > "$FAKE_ROOT/unmarked-target"
mkdir -p "$FAKE_ROOT/foreign/repo/checkout/iterative"
printf foreign > "$FAKE_ROOT/foreign/repo/checkout/iterative/file"
printf root > "$FAKE_ROOT/root-sentinel"
if ! env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$FAKE_ROOT" "$FAKE/scripts/prune-worktrees.sh" --idle-hours 1 > "$TMP/prune.out"; then
  fail "disposable pruner failed"
fi
[ ! -e "$FAKE_TARGET" ] || fail "idle marker-owned target was not pruned"
[ -e "$TMP/root-sentinel" ] && [ -e "$FAKE_ROOT/root-sentinel" ] || fail "root sentinel was removed"
[ -e "$FAKE_ROOT/unmarked-target" ] || fail "unmarked target was removed"
[ -e "$FAKE_ROOT/foreign/repo/checkout/iterative" ] || fail "foreign repository namespace was removed"
pass "pruner removes only stale marker-owned namespaces and preserves sentinels"

AB_REPO="$TMP/ab-repo"
AB_A="$TMP/ab-a"
AB_B="$TMP/ab-b"
AB_ROOT="$(mktemp -d "$TMP-ab-cache.XXXXXX")"
mkdir -p "$AB_REPO"
git -C "$AB_REPO" init -q
git -C "$AB_REPO" config user.email test@example.invalid
git -C "$AB_REPO" config user.name test
printf 'worktree-a-b\n' > "$AB_REPO/README"
git -C "$AB_REPO" add README
git -C "$AB_REPO" commit -qm initial
git -C "$AB_REPO" worktree add --detach -q "$AB_A" HEAD
git -C "$AB_REPO" worktree add --detach -q "$AB_B" HEAD
for worktree in "$AB_A" "$AB_B"; do
  mkdir -p "$worktree/scripts"
  cp "$SCRIPT_DIR/cargo-provenance.py" "$worktree/scripts/"
  cp "$SCRIPT_DIR/cargo-target-dir.sh" "$worktree/scripts/"
  chmod +x "$worktree/scripts"/*.sh "$worktree/scripts"/*.py
done
AB_A_NS="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$AB_ROOT" "$AB_A/scripts/cargo-target-dir.sh" namespace "$AB_A")"
AB_B_NS="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$AB_ROOT" "$AB_B/scripts/cargo-target-dir.sh" namespace "$AB_B")"
AB_A_TARGET="$(printf '%s\n' "$AB_A_NS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target"])')"
AB_B_TARGET="$(printf '%s\n' "$AB_B_NS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target"])')"
[ "$AB_A_TARGET" != "$AB_B_TARGET" ] || fail "distinct Git worktrees shared a target namespace"
AB_A_BIN="$AB_A_TARGET/debug/rivet-client"
mkdir -p "$(dirname "$AB_A_BIN")"
printf 'stale-worktree-a\n' > "$AB_A_BIN"
env RIVET_CARGO_TARGET_ROOT="$AB_ROOT" CARGO_TARGET_DIR="$AB_A_TARGET" python3 "$AB_A/scripts/cargo-provenance.py" sidecar "$AB_A" "$AB_A_BIN" >/dev/null
if env RIVET_CARGO_TARGET_ROOT="$AB_ROOT" CARGO_TARGET_DIR="$AB_B_TARGET" RIVET_CLIENT_BIN="$AB_A_BIN" "$AB_B/scripts/cargo-target-dir.sh" binary "$AB_B" rivet-client >/dev/null 2>&1; then
  fail "stale artifact from worktree A was accepted by worktree B"
fi
pass "real Git worktrees isolate targets and reject stale cross-worktree artifacts"

printf '\nALL ISOLATED CARGO TARGET TESTS PASSED\n'
