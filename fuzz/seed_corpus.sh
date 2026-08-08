#!/bin/bash
# Copy the committed regression seeds in fuzz/seeds/<target> into the
# cargo-fuzz corpus directory fuzz/corpus/<target>, so a plain
# `cargo fuzz run <target>` starts from them.
#
# cargo-fuzz (0.13.x) has no seed handling: it only reads fuzz/corpus/<target>/
# for a target's inputs (creating the directory empty on first use) and never
# looks at fuzz/seeds/. The committed seeds therefore must be copied into the
# mutable corpus before a run. The two directories are kept separate because
# libFuzzer writes every newly discovered input into the *first* corpus path it
# is given on the command line — always fuzz/corpus/<target> for `cargo fuzz
# run` — so the committed seeds are copies, not the live corpus.
#
# The copy is idempotent: re-running re-pins the regression cases after a long
# fuzz session has mutated the corpus.
#
# Usage: fuzz/seed_corpus.sh <target>
set -euo pipefail

target="${1:?usage: seed_corpus.sh <target>}"
fuzz_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
seeds_dir="$fuzz_dir/seeds/$target"
corpus_dir="$fuzz_dir/corpus/$target"

if [ ! -d "$seeds_dir" ]; then
  echo "error: no committed seeds for target '$target' (expected $seeds_dir)" >&2
  exit 1
fi

mkdir -p "$corpus_dir"
cp -R "$seeds_dir"/. "$corpus_dir"/
echo "seeded fuzz/corpus/$target from fuzz/seeds/$target"
