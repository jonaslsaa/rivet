#!/usr/bin/env bash
# write_fixture_manifest — emit a format-1 fixture `manifest.json` for a
# single captured file. Sourced by the script-driven probe runners so the
# manifest schema (and its SHA-256/bytes capture) is maintained in one place
# instead of hand-copied into each runner.
#
# Usage: write_fixture_manifest <out_dir> <kind> <paper_pin> <note> <fixture_file>
#
# Hashes <fixture_file>, then writes <out_dir>/manifest.json. The emitted
# bytes match the historical script block exactly (a `printf '%s\n'` JSON
# object ending in a newline), so swapping a runner onto this helper does not
# churn a committed fixture's manifest.

write_fixture_manifest() {
  local out_dir="$1"
  local kind="$2"
  local paper_pin="$3"
  local note="$4"
  local fixture_file="$5"

  local sha bytes
  sha="$(shasum -a 256 "$fixture_file" | awk '{print $1}')"
  bytes="$(wc -c < "$fixture_file" | tr -d ' ')"

  printf '%s\n' \
    '{' \
    '  "format": 1,' \
    "  \"paper\": \"$paper_pin\"," \
    "  \"kind\": \"$kind\"," \
    "  \"note\": \"$note\"," \
    '  "captured": [' \
    '    {' \
    "      \"path\": \"$(basename "$fixture_file")\"," \
    "      \"sha256\": \"$sha\"," \
    "      \"bytes\": $bytes" \
    '    }' \
    '  ]' \
    '}' > "$out_dir/manifest.json"
  echo "wrote $out_dir/manifest.json"
}
