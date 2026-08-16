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
cp "$SCRIPT_DIR/with-build-lock.sh" "$PROV/scripts/"
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

SPACE_ROOT="$TMP/cache with space"
mkdir -p "$SPACE_ROOT"
space_env=(RIVET_CARGO_TARGET_ROOT="$SPACE_ROOT")
SPACE_TARGET="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${space_env[@]}" "$PROV/scripts/cargo-target-dir.sh" ensure "$PROV")"
find -P "$SPACE_TARGET" -type f -exec touch -m -t 200001010000 {} +
CARGO_TARGET_DIR="$SPACE_TARGET" env "${space_env[@]}" python3 -c 'import time; time.sleep(60)' &
SPACE_PID=$!
sleep 1
env "${space_env[@]}" "$PROV/scripts/prune-worktrees.sh" --idle-hours 1 > "$TMP/space-prune.out"
grep -Fq 'managed process is live' "$TMP/space-prune.out" || fail "space-containing live target was not retained"
[ -d "$SPACE_TARGET" ] || fail "space-containing live target was pruned"
PS_FAIL_BIN="$TMP/ps-failure-bin"
mkdir -p "$PS_FAIL_BIN"
printf '#!/bin/sh\nexit 1\n' > "$PS_FAIL_BIN/ps"
chmod +x "$PS_FAIL_BIN/ps"
PATH="$PS_FAIL_BIN:$PATH" env "${space_env[@]}" "$PROV/scripts/prune-worktrees.sh" --idle-hours 1 > "$TMP/ps-failure-prune.out"
grep -Fq 'managed process is live' "$TMP/ps-failure-prune.out" || fail "ps failure did not retain the target conservatively"
[ -d "$SPACE_TARGET" ] || fail "ps failure pruned a live target"
kill "$SPACE_PID" 2>/dev/null || true
wait "$SPACE_PID" 2>/dev/null || true
pass "space-containing live targets and ps failures are fail-closed"

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

# Namespace creation must tolerate first-use races without accepting a symlink
# or non-directory component created by another process.
RACE_ROOT="$TMP/race-cache"
mkdir -p "$RACE_ROOT"
RACE_OUT="$TMP/race-out"
RACE_PIDS=()
for _ in $(seq 1 24); do
  (
    env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_CARGO_TARGET_ROOT="$RACE_ROOT" \
      "$SCRIPT_DIR/cargo-target-dir.sh" target "$PROV"
  ) > "$RACE_OUT.$$.$RANDOM" &
  RACE_PIDS+=("$!")
done
for pid in "${RACE_PIDS[@]}"; do
  wait "$pid"
done
RACE_TARGETS=""
for race_file in "$RACE_OUT".*; do
  [ -f "$race_file" ] || continue
  RACE_TARGETS+="$(cat "$race_file")\n"
done
[ -n "$RACE_TARGETS" ] || fail "concurrent first-use namespace creation produced no targets"
[ "$(printf '%b' "$RACE_TARGETS" | sort -u | wc -l | tr -d ' ')" = 1 ] || fail "concurrent namespace creation disagreed on the target"
rm -f "$RACE_OUT".*
pass "concurrent first-use namespace creation is race-safe"

# Lock ownership must be structural, not inferred from caller-controlled file
# descriptor variables. A managed lock held by another process must still block.
PROV_NS="$(env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" namespace "$PROV")"
GROUP_LOCK="$(printf '%s\n' "$PROV_NS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["group_lock"])')"
CHECKOUT_LOCK="$(printf '%s\n' "$PROV_NS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["checkout_lock"])')"
LOCK_HOLDER="$TMP/lock-holder.py"
cat > "$LOCK_HOLDER" <<'PY'
import fcntl
import pathlib
import sys
import time

ready, *paths = map(pathlib.Path, sys.argv[1:])
files = [path.open("a+") for path in paths]
for stream in files:
    fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
ready.write_text("ready")
time.sleep(3)
PY
python3 "$LOCK_HOLDER" "$TMP/lock-ready" "$GROUP_LOCK" "$CHECKOUT_LOCK" &
LOCK_PID=$!
while [ ! -e "$TMP/lock-ready" ]; do sleep 0.05; done
exec 8<> "$TMP/foreign-fd-8"
exec 9<> "$TMP/foreign-fd-9"
LOCK_STARTED="$(date +%s)"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR RIVET_BUILD_GROUP_LOCK_FD=8 RIVET_BUILD_LOCK_FD=9 "${prov_env[@]}" \
  "$PROV/scripts/with-build-lock.sh" "$PROV" true
LOCK_ELAPSED=$(( $(date +%s) - LOCK_STARTED ))
wait "$LOCK_PID"
[ "$LOCK_ELAPSED" -ge 2 ] || fail "foreign inherited lock descriptors bypassed managed lock acquisition"
exec 8>&-
exec 9>&-
pass "caller-controlled lock descriptors cannot bypass serialization"

# Lock paths and provenance metadata are replacement-only objects: symlinks and
# hardlinks must not redirect writes outside the managed namespace.
LOCK_OUTSIDE="$TMP/lock-outside"
printf lock-sentinel > "$LOCK_OUTSIDE"
mkdir -p "$(dirname "$GROUP_LOCK")"
rm -f "$GROUP_LOCK"
ln -s "$LOCK_OUTSIDE" "$GROUP_LOCK"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" \
  "$PROV/scripts/with-build-lock.sh" "$PROV" true >/dev/null 2>&1; then
  fail "symlinked group lock was accepted"
fi
[ "$(cat "$LOCK_OUTSIDE")" = lock-sentinel ] || fail "symlinked group lock changed its outside target"
rm -f "$GROUP_LOCK"
mkdir -p "$(dirname "$CHECKOUT_LOCK")"
rm -f "$CHECKOUT_LOCK"
ln -s "$LOCK_OUTSIDE" "$CHECKOUT_LOCK"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" \
  "$PROV/scripts/with-build-lock.sh" "$PROV" true >/dev/null 2>&1; then
  fail "symlinked checkout lock was accepted"
fi
[ "$(cat "$LOCK_OUTSIDE")" = lock-sentinel ] || fail "symlinked checkout lock changed its outside target"
rm -f "$CHECKOUT_LOCK"
pass "managed lock symlinks are rejected without outside writes"

MARKER="$PROV_TARGET/.rivet-cargo-target"
MARKER_OUTSIDE="$TMP/marker-outside"
printf marker-sentinel > "$MARKER_OUTSIDE"
rm -f "$MARKER"
ln "$MARKER_OUTSIDE" "$MARKER"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" \
  python3 "$PROV/scripts/cargo-provenance.py" ensure "$PROV" >/dev/null 2>&1; then
  fail "hardlinked marker was accepted"
fi
[ "$(cat "$MARKER_OUTSIDE")" = marker-sentinel ] || fail "hardlinked marker changed its outside target"
rm -f "$MARKER"
env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" \
  python3 "$PROV/scripts/cargo-provenance.py" ensure "$PROV" >/dev/null
pass "hardlinked markers cannot overwrite outside files"

SIDECAR_HARDLINK_BIN="$PROV_TARGET/debug/sidecar-hardlink"
SIDECAR_HARDLINK_OUTSIDE="$TMP/sidecar-hardlink-outside"
mkdir -p "$(dirname "$SIDECAR_HARDLINK_BIN")"
printf sidecar-binary > "$SIDECAR_HARDLINK_BIN"
printf sidecar-sentinel > "$SIDECAR_HARDLINK_OUTSIDE"
ln "$SIDECAR_HARDLINK_OUTSIDE" "$SIDECAR_HARDLINK_BIN.rivet-provenance"
if env -u CARGO_TARGET_DIR -u RIVET_CARGO_TARGET_DIR "${prov_env[@]}" \
  python3 "$PROV/scripts/cargo-provenance.py" sidecar "$PROV" "$SIDECAR_HARDLINK_BIN" >/dev/null 2>&1; then
  fail "hardlinked provenance sidecar was accepted"
fi
[ "$(cat "$SIDECAR_HARDLINK_OUTSIDE")" = sidecar-sentinel ] || fail "hardlinked sidecar changed its outside target"
rm -f "$SIDECAR_HARDLINK_BIN.rivet-provenance" "$SIDECAR_HARDLINK_BIN"
pass "hardlinked provenance sidecars cannot overwrite outside files"

RECEIPT="$TMP/receipt"
RECEIPT_OUTSIDE="$TMP/receipt-outside"
printf receipt-sentinel > "$RECEIPT_OUTSIDE"
ln -s "$RECEIPT_OUTSIDE" "$RECEIPT.4242.tmp"
python3 - "$SCRIPT_DIR/cargo-provenance.py" "$RECEIPT" <<'PY'
import importlib.util
import pathlib
import sys

spec = importlib.util.spec_from_file_location("cargo_provenance", sys.argv[1])
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)
module.os.getpid = lambda: 4242
try:
    module.write_receipt(pathlib.Path(sys.argv[2]), {"version": 1})
except (OSError, ValueError):
    pass
PY
[ "$(cat "$RECEIPT_OUTSIDE")" = receipt-sentinel ] || fail "receipt temporary symlink changed its outside target"
[ -L "$RECEIPT.4242.tmp" ] || fail "receipt temporary symlink was unexpectedly replaced"
pass "receipt temporary symlinks cannot redirect writes"

WRAPPER="$TMP/custom-wrapper"
printf 'wrapper-a\n' > "$WRAPPER"
WRAPPER_DIGEST_A="$(env RUSTC_WRAPPER="$WRAPPER" "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" digest "$PROV")"
printf 'wrapper-b\n' > "$WRAPPER"
WRAPPER_DIGEST_B="$(env RUSTC_WRAPPER="$WRAPPER" "${prov_env[@]}" "$PROV/scripts/cargo-target-dir.sh" digest "$PROV")"
[ "$WRAPPER_DIGEST_A" != "$WRAPPER_DIGEST_B" ] || fail "custom RUSTC_WRAPPER content mutation did not invalidate the digest"
pass "custom RUSTC_WRAPPER content changes invalidate provenance"

printf '\nALL ISOLATED CARGO TARGET TESTS PASSED\n'
