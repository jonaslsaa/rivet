# rivet-parity — byte-for-byte NBT/SNBT parity diff vs the Paper Java oracle

Task #34: the strongest M0 parity proof for `rivet-nbt`. This tool drives the
Java reference oracle (`tools/rivet-reference-oracle`, which compiles against
the pinned Paper 26.2 jar and calls its own `NbtIo` / `TagParser` /
`SnbtPrinterTagVisitor`) and compares its output byte-for-byte against the Rust
port, over the 432 committed M0 chunk-NBT fixtures plus a hand-built SNBT
corpus.

## What it checks

Every check is one JSON-Lines object on stdout (`{"kind": ..., "id": ...,
"ok": ..., "fields": [...]}`), followed by a `{"kind":"stats"}` line. A
human-readable summary goes to stderr.

| kind | input | compares |
| --- | --- | --- |
| `snbt.parse` | SNBT text | canonical SNBT, pretty SNBT, tag type, tag id — Rust `TagParser` + `StringTagVisitor`/`SnbtPrinterTagVisitor` vs oracle `snbt.parse` |
| `snbt.parse` (`parse-invalid.*`) | deliberately invalid SNBT | accept/reject parity (error text is soft/informational) |
| `nbt.decode` | binary NBT bytes (fixtures) | canonical + pretty SNBT after Rust `NbtIo.read` vs oracle `nbt.decode` |
| `nbt.encode` | compound SNBT | binary NBT bytes — **byte-for-byte** for single-key-deep compounds; for multi-key compounds the binary field order is the documented HashMap-iteration-order divergence, so the binding comparison is a *semantic* one (both binaries must re-read to the same canonical SNBT) |
| `idem` | binary NBT bytes | Rust-internal read->write->read structural equality (always runs, even without the oracle) |

### Known divergences

- **`compound_key_order`** — Java writes `CompoundTag` fields in fastutil
  `Object2ObjectOpenHashMap` iteration order; Rust uses `std::HashMap` with a
  randomized per-process seed. Binary field order therefore never matches
  Java's and is not even stable across Rust processes. Documented in
  `crates/rivet-nbt/src/compound_tag.rs` and `nbt_io.rs`. The transcript marks
  these checks `"divergences": ["compound_key_order"]` and reports them
  separately from hard mismatches. **The SNBT text checks do not suffer this**:
  both printers sort compound keys, so `snbt.parse` and `nbt.decode` compare
  byte-for-byte.

## How to run

The oracle needs the M0-materialized Paper runtime (Java 25 JDK + the pinned
Paper 26.2 jar + the libraries a Paper run materialized under
`tools/rivet-oracle/work/run/`). When that worktree is pruned, point the
launcher at the main checkout's artifacts:

```sh
RIVET_PAPER_JAR="$HOME/.../working/Paper/paper-server/build/libs/paper-server-26.2.local-SNAPSHOT.jar" \
RIVET_PAPER_LIBRARIES="$HOME/.../tools/rivet-oracle/work/run/libraries" \
cargo run -p rivet-parity
```

Useful flags:

- `--limit-fixtures=N` — cap the fixture corpus for a fast smoke run.
- `--no-oracle` — skip the oracle (only Rust-internal `idem` checks run; every
  oracle-dependent check is emitted with `"skipped": true`). The run is
  UNVERIFIED (exit 3) because nothing was compared against Paper.
- `--require-oracle` — a dead oracle (boot failure) is UNVERIFIED (exit 3) and
  the run stops immediately instead of degrading to Rust-only checks. The merge
  gate always passes this flag. Mutually exclusive with `--no-oracle`.
- `--scoreboard` — emit/refresh the checked-in `PARITY.md` scoreboard at the
  workspace root from the live run's stats (see below). Rows are rendered only
  for checks that actually ran; an oracle-less run therefore records only the
  `idem` row instead of claiming a parity failure.

### Exit codes (machine-stable status)

`scripts/gate.sh` classifies the parity step purely from the exit code — never
by scraping stderr text:

| exit | status | meaning |
| --- | --- | --- |
| 0 | `VERIFIED` | the oracle booted and ran; no hard mismatches |
| 1 | `FAILED` | the oracle ran but parity diverged (hard mismatches) |
| 3 | `UNVERIFIED` | the oracle did not boot / did not run; nothing was compared against Paper |
| other (e.g. 101) | `FAILED` | the tool crashed or errored |

Keep `EXIT_UNVERIFIED` in `src/main.rs` in sync with
`ORACLE_EXIT_UNVERIFIED` in `scripts/gate.sh`.

### Reading the transcript

Each check object:

```json
{
  "kind": "nbt.encode",
  "id": "encode.byte-array",
  "input": "{a:[B;1B,-1B,2B]}",
  "ok": true,
  "skipped": false,
  "divergences": [],
  "fields": [
    {"name": "tag_type", "ok": true},
    {"name": "canonical", "ok": true},
    {"name": "byte_for_byte", "ok": true}
  ]
}
```

`fields` with `"ok": false` carry `expected` and `got` strings. `"soft": true`
fields (e.g. error text on rejected inputs) are informational and do not flip
`ok`. Checks with `"divergences"` are `ok` but carry the documented
divergence; only checks with a failing non-soft field are hard mismatches.

The exit code encodes the status — see the exit-code table above.

## Output on the M0 corpus

With the pinned Paper 26.2 (`0a99345`) and all 432 fixtures:

```
=== rivet-nbt vs Paper Java oracle — parity summary ===
  snbt.parse         total=...  matched=...  diverged=0    mismatched=0
  nbt.decode         total=432  matched=432  diverged=0    mismatched=0
  nbt.encode         total=...  matched=...  diverged=...  mismatched=0
  idem               total=432  matched=432  diverged=0    mismatched=0
```

`snbt.parse` and `nbt.decode` are byte-for-byte identical to Paper. Every
`nbt.encode` divergence is the documented `compound_key_order` (semantic
re-read equality holds for all of them).

## Scoreboard

`--scoreboard` writes a `PARITY.md` at the workspace root with one row per
check kind — `inputs | matched | diverged | mismatched | date` — derived from
the run's stats. Run it after a milestone run so the checked-in numbers stay
current:

```sh
cargo run -p rivet-parity -- --scoreboard
```

The scoreboard is driven purely by the live run, so a check that stops being
exercised disappears and a red gate (hard mismatches) shows up in the
`mismatched` column instead of being papered over. `diverged` counts only
`ok` checks carrying the documented `compound_key_order` divergence. The file
is written in place at the workspace root (the crate resolves it relative to
`CARGO_MANIFEST_DIR`), so it works from any worktree. A run with
`--limit-fixtures=N` appends a `_Snapshot:_` note recording the cap, so a
capped snapshot is not mistaken for full-corpus coverage.
