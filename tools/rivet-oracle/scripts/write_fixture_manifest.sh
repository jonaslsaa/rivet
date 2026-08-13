#!/usr/bin/env bash
# write_fixture_manifest — emit a format-1 fixture `manifest.json` for one or
# more captured files. Sourced by the script-driven probe runners so the
# manifest schema (and its SHA-256/bytes capture) is maintained in one place
# instead of hand-copied into each runner.
#
# Usage:
#   write_fixture_manifest <out_dir> <kind> <paper_pin> <note> <fixture_file> [<fixture_file>...]
#
# Hashes each <fixture_file>, then writes <out_dir>/manifest.json with one
# `captured` entry per file (in argument order). The emitted bytes for a
# single file match the historical script block exactly (a `printf '%s\n'`
# JSON object ending in a newline), so swapping a runner onto this helper does
# not churn a committed fixture's manifest.

write_fixture_manifest() {
  local out_dir="$1"
  local kind="$2"
  local paper_pin="$3"
  local note="$4"
  shift 4

  local lines=('{'
    '  "format": 1,'
    "  \"paper\": \"$paper_pin\","
    "  \"kind\": \"$kind\",")
  # JSON-escape the note (backslash first, then double-quote) so a future note
  # containing either cannot silently emit a malformed manifest. The current
  # notes contain neither, so the emitted bytes are unchanged.
  local note_escaped
  note_escaped="${note//\\/\\\\}"
  note_escaped="${note_escaped//\"/\\\"}"
  lines+=("  \"note\": \"$note_escaped\","
    '  "captured": [')
  local file sha bytes last_idx i
  last_idx=$(($# - 1))
  i=0
  for file in "$@"; do
    sha="$(shasum -a 256 "$file" | awk '{print $1}')"
    bytes="$(wc -c < "$file" | tr -d ' ')"
    lines+=('    {'
      "      \"path\": \"$(basename "$file")\","
      "      \"sha256\": \"$sha\","
      "      \"bytes\": $bytes")
    if [ "$i" -eq "$last_idx" ]; then
      lines+=('    }')
    else
      lines+=('    },')
    fi
    i=$((i + 1))
  done
  lines+=('  ]'
    '}')

  printf '%s\n' "${lines[@]}" > "$out_dir/manifest.json"
  echo "wrote $out_dir/manifest.json"
}
