#!/bin/bash
# Ledger check for the fast gate: COVERAGE.tsv and VERIFY.tsv must regenerate
# byte-identically from the tree (the committed ledgers are current), both must
# be well-formed, and the ledger tools' own tests must pass. Offline; needs the
# Paper sources under working/ (same prerequisite as the manifest suite).
#
#   scripts/ledger_check.sh            # exit 0 = ledgers current and well-formed
#
# On a stale ledger, regenerate and commit it:
#   python3 scripts/coverage_ledger.py && python3 scripts/verify_ledger.py
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$repo_dir"

python3 scripts/coverage_ledger.py --check
python3 scripts/verify_ledger.py --check
python3 scripts/verify_debt.py --check
python3 scripts/test_coverage_ledger.py
python3 scripts/test_verify_ledger.py
