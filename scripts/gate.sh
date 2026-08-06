#!/bin/bash
# The merge gate. Run before merging any PR (and at the end of every wave).
# No hosted CI by design — this script IS the gate; a red gate blocks the merge.
#
# Scope: pass crate names or crates/... paths as arguments (or set SCOPE to a
# space/comma-separated list) to gate only those crates (fmt/clippy/test). The
# unused-deps check (cargo machete) and the default full gate stay workspace-wide.
#
#   ./scripts/gate.sh                     # full gate: fmt, clippy, tests, oracle, scenario, machete
#   ./scripts/gate.sh crates/rivet-nbt     # fmt+clippy+test for rivet-nbt only
#   SCOPE="rivet-nbt, rivet-serialization" ./scripts/gate.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# --- scope resolution -------------------------------------------------------
SCOPE_ARGS=("$@")
if [ -n "${SCOPE:-}" ]; then
  # shellcheck disable=SC2206
  SCOPE_ARGS+=($(printf '%s' "$SCOPE" | tr ',' ' '))
fi
PKGS=()
if [ ${#SCOPE_ARGS[@]} -gt 0 ]; then
  for a in "${SCOPE_ARGS[@]}"; do
    a="${a#crates/}"; a="${a%/}"
    [ -n "$a" ] && PKGS+=("$a")
  done
fi
# "${arr[@]+...}" guard: bare "${PKGS[@]}" on an empty array errors under
# `set -u` + bash 3.2 (the default on macOS).
PKG_FLAGS=()
for p in ${PKGS[@]+"${PKGS[@]}"}; do
  PKG_FLAGS+=(-p "$p")
done

# --- cargo fmt --------------------------------------------------------------
echo "==> cargo fmt --check"
if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
  cargo fmt --check "${PKG_FLAGS[@]}"
else
  cargo fmt --all --check
fi

# --- cargo clippy (-Dwarnings) ----------------------------------------------
echo "==> cargo clippy (-Dwarnings)"
if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
  RUSTFLAGS=-Dwarnings cargo clippy "${PKG_FLAGS[@]}" --all-targets
else
  RUSTFLAGS=-Dwarnings cargo clippy --workspace --all-targets
fi

# --- cargo test -------------------------------------------------------------
echo "==> cargo test"
if [ ${#PKG_FLAGS[@]} -gt 0 ]; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run "${PKG_FLAGS[@]}"
  else
    cargo test "${PKG_FLAGS[@]}"
  fi
elif command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi

# --- oracle verify (M0 sanity gate: green against vanilla itself) ------------
# NOTE: PASS depends on the local working/Paper matching the commit pinned in
# tools/rivet-oracle/fixtures/manifest.json (26.2-DEV-main@0a99345). If
# working/Paper advances, regenerate the fixtures (scripts/extract_fixtures.py)
# and re-pin the manifest before relying on this step again.
# Skipped when gating a crate subset (the oracle verifies the whole server).
if [ ${#PKG_FLAGS[@]} -eq 0 ]; then
  echo "==> oracle verify (M0 sanity gate: green against vanilla itself)"
  if [ -n "${RIVET_ORACLE_JAR:-}" ] || [ -f tools/rivet-oracle/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar ] || \
     ls working/Paper/paper-server/build/libs/paper-paperclip*.jar >/dev/null 2>&1; then
    cargo run -q -p rivet-oracle -- verify
  else
    echo "    SKIPPED (no paperclip jar: set RIVET_ORACLE_JAR or materialize working/Paper first)"
  fi
fi

# --- scenario runner (M0 join harness: Paper-vs-Paper + negative case) --------
# The Paper-vs-Paper join harness boots local Paper twice, joins each with the
# Azalea headless client, and requires identical normalized transcripts, plus a
# negative case proving the comparator detects a tampered position. Runs only
# when a paperclip jar is materialized (same guard style as oracle verify);
# skipped when gating a crate subset (the scenario drives a whole server).
if [ ${#PKG_FLAGS[@]} -eq 0 ]; then
  echo "==> scenario runner (join: Paper-vs-Paper + negative case)"
  if [ -n "${RIVET_ORACLE_JAR:-}" ] || [ -f tools/rivet-client/work/jars/paper-paperclip-26.2.local-SNAPSHOT.jar ] || \
     ls working/Paper/paper-server/build/libs/paper-paperclip*.jar >/dev/null 2>&1; then
    tools/rivet-client/run-scenario.sh join
  else
    echo "    SKIPPED (no paperclip jar: set RIVET_ORACLE_JAR or materialize working/Paper first)"
  fi
fi

# --- unused dependencies (cargo-machete) -------------------------------------
echo "==> cargo machete (unused deps)"
if ! command -v cargo-machete >/dev/null 2>&1; then
  echo "    cargo-machete not found; installing (cargo install cargo-machete --locked)"
  cargo install cargo-machete --locked
fi
cargo machete

echo "GATE GREEN"
