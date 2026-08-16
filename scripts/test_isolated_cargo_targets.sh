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
if env -u CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$TMP/foreign" RIVET_CARGO_TARGET_DIR="$TMP/foreign" "$SCRIPT_DIR/cargo-target-dir.sh" target "$REPO_DIR" >/dev/null 2>&1; then
  fail "foreign RIVET_CARGO_TARGET_DIR was accepted"
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

# Provenance attacks that Cargo's `Fresh` diagnostics and successful test status
# cannot prove. Each case uses a disposable repository and external namespace.
PROV="$TMP/provenance-repo"
PROV_ROOT="$(mktemp -d "$TMP-provenance-cache.XXXXXX")"
mkdir -p "$PROV"
git -C "$PROV" init -q
git -C "$PROV" config user.email test@example.invalid
git -C "$PROV" config user.name test
printf 'source-a\n' > "$PROV/README"
git -C "$PROV" add README
git -C "$PROV" commit -qm initial
mkdir -p "$PROV/scripts"
cp "$SCRIPT_DIR/cargo-provenance.py" "$PROV/scripts/"
cp "$SCRIPT_DIR/cargo-target-dir.sh" "$PROV/scripts/"
cp "$SCRIPT_DIR/prune-worktrees.sh" "$PROV/scripts/"
chmod +x "$PROV/scripts"/*
prov_env=(RIVET_CARGO_TARGET_ROOT="$PROV_ROOT")
prov_target() { env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" target "$PROV"; }
PROV_TARGET="$(prov_target)"
PROV_BIN="$PROV_TARGET/debug/rivet-client"
mkdir -p "$(dirname "$PROV_BIN")"
printf 'cargo-1.97-fresh\n' > "$PROV_BIN"
printf 'Fresh\n' > "$PROV_BIN.d"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" binary "$PROV" rivet-client >/dev/null 2>&1; then
  fail "Cargo 1.97-style Fresh binary without attestation was accepted"
fi
pass "Fresh and dep-info-shaped artifacts do not prove a binary"

env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" python3 "$PROV/scripts/cargo-provenance.py" sidecar "$PROV" "$PROV_BIN" >/dev/null
[ "$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" binary "$PROV" rivet-client)" = "$PROV_BIN" ] || fail "valid attestation was not resolved"
printf 'source mutation\n' >> "$PROV/README"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" binary "$PROV" rivet-client >/dev/null 2>&1; then
  fail "unchanged old binary survived a source mutation"
fi
pass "source mutation rejects an unchanged old binary"

env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" prepare "$PROV" "$PROV_BIN"
[ ! -e "$PROV_BIN" ] || fail "stale binary was not removed before the iterative build"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" stamp "$PROV" "$PROV_BIN" >/dev/null 2>&1; then
  fail "no-output successful-test attack was stamped"
fi
[ -e "$PROV_TARGET/.rivet-build-receipt" ] || fail "failed/no-output build receipt was not retained"
pass "pre-existing stale and failed/no-output builds cannot be stamped"
MARKER="$PROV_TARGET/.rivet-cargo-target"
rm -f "$MARKER"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" python3 "$PROV/scripts/cargo-provenance.py" prepare "$PROV" "$PROV_TARGET/../escaped" >/dev/null 2>&1; then
  fail "managed deliverable escape was accepted"
fi
[ ! -e "$MARKER" ] || fail "managed deliverable escape created a marker"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" ensure "$PROV" >/dev/null
pass "managed deliverable escapes are rejected before marker creation"
printf 'rebuilt\n' > "$PROV_BIN"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" stamp "$PROV" "$PROV_BIN"
old_hash="$(shasum -a 256 "$PROV_BIN")"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" prepare "$PROV" "$PROV_BIN"
[ "$(shasum -a 256 "$PROV_BIN")" = "$old_hash" ] || fail "valid iterative attestation was not a no-op"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" stamp "$PROV" "$PROV_BIN"
pass "valid iterative attestations are retained and re-verified"

STRICT_TARGET="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_NAMESPACE=strict "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" target "$PROV")"
STRICT_BIN="$STRICT_TARGET/debug/rivet-server"
mkdir -p "$(dirname "$STRICT_BIN")"
printf 'strict-stale\n' > "$STRICT_BIN"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_NAMESPACE=strict "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" prepare "$PROV" "$STRICT_BIN"
[ ! -e "$STRICT_BIN" ] || fail "strict prepare retained a stale binary"
printf 'strict-rebuilt\n' > "$STRICT_BIN"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_NAMESPACE=strict "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" stamp "$PROV" "$STRICT_BIN"
pass "strict builds require removal and fresh recreation"

SYMLINK_BIN="$PROV_TARGET/debug/symlink-bin"
ln -s "$TMP/outside" "$SYMLINK_BIN"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" sidecar "$PROV" "$SYMLINK_BIN" >/dev/null 2>&1; then
  fail "executable symlink escape was attested"
fi
rm -f "$SYMLINK_BIN"
mkdir -p "$TMP/outside" "$PROV_TARGET/escape"
ln -s "$TMP/outside" "$PROV_TARGET/escape/link"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" sidecar "$PROV" "$PROV_TARGET/escape/link/bin" >/dev/null 2>&1; then
  fail "managed parent symlink escape was accepted"
fi
rm -f "$PROV_TARGET/escape/link"
SIDECAR_BIN="$PROV_TARGET/debug/sidecar-bin"
printf sidecar > "$SIDECAR_BIN"
ln -s "$TMP/outside" "$SIDECAR_BIN.rivet-provenance"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-provenance.py" prepare "$PROV" "$SIDECAR_BIN" >/dev/null 2>&1; then
  fail "provenance sidecar symlink was accepted"
fi
rm -f "$SIDECAR_BIN.rivet-provenance" "$SIDECAR_BIN"
pass "executable, parent, and sidecar symlink escapes are rejected"

# Namespace and inherited-target aliases are canonicalized without permitting a
# foreign target. A final target symlink must fail before marker creation.
ALIAS_ROOT="$TMP/root-alias"
ln -s "$PROV_ROOT" "$ALIAS_ROOT"
BASE="$(printf '%s\n' "$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" namespace "$PROV")" | python3 -c 'import json,sys; print(json.load(sys.stdin)["base"])')"
FINAL_SYMLINK="$TMP/final-target"
rm -rf "$PROV_TARGET"
mkdir -p "$BASE"
ln -s "$FINAL_SYMLINK" "$BASE/iterative"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" namespace "$PROV" >/dev/null 2>&1; then
  fail "pre-existing final target symlink was accepted"
fi
rm -f "$BASE/iterative"
REPO_ID_DIR="$(basename "$(dirname "$(dirname "$PROV_TARGET")")")"
CHECKOUT_DIR="$(basename "$(dirname "$PROV_TARGET")")"
ALIAS_TARGET="$ALIAS_ROOT/$REPO_ID_DIR/$CHECKOUT_DIR/iterative/."
[ "$(env CARGO_TARGET_DIR="$ALIAS_TARGET" "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" target "$PROV")" = "$PROV_TARGET" ] || fail "canonical target alias was rejected"
[ "$(env -u CARGO_TARGET_DIR RIVET_CARGO_TARGET_DIR="$ALIAS_TARGET" "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" target "$PROV")" = "$PROV_TARGET" ] || fail "canonical RIVET target alias was rejected"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" ensure "$PROV" >/dev/null
if env CARGO_TARGET_DIR="$TMP/foreign" "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" target "$PROV" >/dev/null 2>&1; then
  fail "foreign inherited target was accepted"
fi
pass "target aliases canonicalize while foreign inherited targets fail"

# A live process must retain the target even when it exports a symlink,
# trailing-slash, and dot-component spelling of the namespace.
find -P "$PROV_TARGET" -type f -exec touch -m -t 200001010000 {} +
LIVE_VALUE="$ALIAS_TARGET"
CARGO_TARGET_DIR="$LIVE_VALUE" env "${prov_env[@]}" python3 -c 'import time; time.sleep(60)' &
LIVE_PID=$!
sleep 1
if ! env "${prov_env[@]}" "$PROV/scripts/prune-worktrees.sh" --idle-hours 1 > "$TMP/live-prune.out"; then
  kill "$LIVE_PID" 2>/dev/null || true
  wait "$LIVE_PID" 2>/dev/null || true
  fail "live target prune probe failed"
fi
kill "$LIVE_PID" 2>/dev/null || true
wait "$LIVE_PID" 2>/dev/null || true
grep -Fq 'managed process is live' "$TMP/live-prune.out" || fail "canonical live target alias was not retained"
[ -d "$PROV_TARGET" ] || fail "live target was pruned through an alias"
pass "live pruning canonicalizes symlink, slash, and dot aliases"

# The worktree parser must flush its final record, while malformed status is a
# keep decision rather than an accidental removal.
git -C "$PROV" update-ref refs/remotes/origin/main HEAD
FINAL_WT="$TMP/final-worktree"
git -C "$PROV" worktree add --detach -q "$FINAL_WT" HEAD
find -P "$PROV_TARGET" -type f -exec touch -m -t 200001010000 {} +
env "${prov_env[@]}" "$PROV/scripts/prune-worktrees.sh" --idle-hours 1 > "$TMP/final-worktree.out"
[ ! -d "$FINAL_WT" ] || fail "final worktree record was not flushed and removed"
pass "final worktree parser record is processed"
STATUS_WT="$TMP/status-worktree"
git -C "$PROV" worktree add --detach -q "$STATUS_WT" HEAD
FAKE_BIN="$TMP/fake-bin"
mkdir -p "$FAKE_BIN"
REAL_GIT="$(command -v git)"
cat > "$FAKE_BIN/git" <<EOF
#!/usr/bin/env bash
case " \$* " in
  *' status '*) printf 'malformed-status'; exit 0 ;;
esac
exec "$REAL_GIT" "\$@"
EOF
chmod +x "$FAKE_BIN/git"
PATH="$FAKE_BIN:$PATH" bash -c 'source "$1"; worktree_sweep "$2" "$3"' bash "$PROV/scripts/prune-worktrees.sh" "$PROV" "$PROV" > "$TMP/status-worktree.out"
[ -d "$STATUS_WT" ] || fail "malformed status was treated as clean"
git -C "$PROV" worktree remove --force "$STATUS_WT"
pass "malformed status retains the worktree"

printf '\nALL ISOLATED CARGO TARGET TESTS PASSED\n'
