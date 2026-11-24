#!/bin/bash
# Focused test for gate.sh's oracle-op and regen steps (VERIFICATION §5 items
# 2-3): run_oracle_ops_live and run_regen_check must classify the tools'
# machine-stable exit codes exactly like run_rivet_parity (0 VERIFIED, 1 FAILED,
# 3 UNVERIFIED, anything else FAILED, never green), and --nightly must refuse
# to combine with --fast or a crate scope. Sources scripts/gate.sh (functions
# only) and shims `cargo`.
#
#   ./scripts/test_gate_oracle_ops.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
CARGO_SHIM="$(mktemp -d)"
trap 'rm -rf "$TMP" "$CARGO_SHIM"' EXIT

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "ok:   $1"; }

# shellcheck source=scripts/gate.sh
# shellcheck disable=SC1091  # sources a sibling script; shellcheck only follows it with -x
source "$SCRIPT_DIR/gate.sh"

shim_cargo() { # $1 = exit code; logs its argv
  printf '#!/bin/bash\necho "$*" >> %q\nexit %s\n' "$TMP/argv" "$1" > "$CARGO_SHIM/cargo"
  chmod +x "$CARGO_SHIM/cargo"
}

# drive <function> <cargo exit> <require-oracle> -> sets RC, OUT
drive() {
  shim_cargo "$2"
  : > "$TMP/argv"
  ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=1; REQUIRE_ORACLE="$3"
  local saved="$PATH"
  PATH="$CARGO_SHIM:$PATH"
  set +e
  ( "$1"; echo "ORACLE_UNVERIFIED=$ORACLE_UNVERIFIED" ) > "$TMP/out" 2>&1
  RC=$?
  set -e
  PATH="$saved"
  REQUIRE_ORACLE=0
  OUT="$(cat "$TMP/out")"
}

for fn in run_oracle_ops_live run_regen_check; do
  drive "$fn" 0 0
  [ "$RC" = 0 ] || fail "$fn: exit 0 should pass (got $RC)"
  grep -q "^    VERIFIED" <<<"$OUT" || fail "$fn: VERIFIED not printed"
  grep -q "ORACLE_UNVERIFIED=0" <<<"$OUT" || fail "$fn: clean run marked unverified"
  pass "$fn: exit 0 is VERIFIED"

  drive "$fn" 1 0
  [ "$RC" = 1 ] || fail "$fn: exit 1 must fail the gate (got $RC)"
  grep -q "^    FAILED" <<<"$OUT" || fail "$fn: FAILED not printed"
  pass "$fn: exit 1 is FAILED"

  drive "$fn" 3 0
  [ "$RC" = 0 ] || fail "$fn: exit 3 without --require-oracle defers to the verdict (got $RC)"
  grep -q "ORACLE_UNVERIFIED=1" <<<"$OUT" || fail "$fn: exit 3 did not set ORACLE_UNVERIFIED"
  grep -q "^    VERIFIED" <<<"$OUT" && fail "$fn: exit 3 printed VERIFIED"
  pass "$fn: exit 3 is UNVERIFIED"

  drive "$fn" 3 1
  [ "$RC" = 1 ] || fail "$fn: exit 3 with --require-oracle must hard-fail (got $RC)"
  pass "$fn: exit 3 + --require-oracle hard-fails"

  drive "$fn" 101 0
  [ "$RC" = 1 ] || fail "$fn: a tool crash must fail the gate (got $RC)"
  grep -q "^    VERIFIED" <<<"$OUT" && fail "$fn: crash printed VERIFIED"
  pass "$fn: crash is FAILED"
done

drive run_oracle_ops_live 0 0
grep -q -- "-p rivet-parity --locked -- ops live" "$TMP/argv" || fail "ops step must run \`rivet-parity ops live\`"
drive run_regen_check 0 0
grep -q -- "-p rivet-oracle --locked -- regen --check" "$TMP/argv" || fail "regen step must run \`rivet-oracle regen --check\`"
pass "steps invoke the right tools"

# A missing parity prereq leaves the ops row UNVERIFIED without running cargo.
shim_cargo 0
: > "$TMP/argv"
ORACLE_UNVERIFIED=0; PARITY_RUNNABLE=0
PATH="$CARGO_SHIM:$PATH" run_oracle_ops_live > "$TMP/out" 2>&1
[ "$ORACLE_UNVERIFIED" = 1 ] || fail "ops step without prereqs must be UNVERIFIED"
[ -s "$TMP/argv" ] && fail "ops step ran cargo without prereqs"
pass "ops step without prereqs is UNVERIFIED"

for combo in "--nightly --fast" "--nightly crates/rivet-nbt"; do
  set +e
  # shellcheck disable=SC2086  # deliberate word splitting of the flag combo
  RIVET_BUILD_LOCK_HELD=1 bash "$SCRIPT_DIR/gate.sh" $combo > "$TMP/out" 2>&1
  rc=$?
  set -e
  [ "$rc" = 1 ] || fail "gate.sh $combo must be rejected (got $rc)"
  grep -q "cannot be combined" "$TMP/out" || fail "gate.sh $combo: no usage error"
  pass "gate.sh $combo is rejected"
done

echo "all gate oracle-op/regen step tests passed"
