# rivet-fuzz

`cargo-fuzz` targets for the rivet parser crates and for rivet-protocol's
serverbound packet decode paths (issue #46). The NBT/DFU targets feed untrusted
bytes into the SNBT `TagParser`, the binary NBT readers (`NbtIo`), and the DFU
codec combinators over `NbtOps`, looking for panics, overflows, infinite loops,
and round-trip failures; the packet targets reuse the real packet body codecs
through the registration-built id-dispatch codec (see "Targets").

The workspace pins a stable toolchain (`rust-toolchain.toml`), but
`cargo-fuzz` requires nightly. Override the toolchain per invocation:

```sh
export RUSTUP_TOOLCHAIN=nightly            # or a pinned nightly, e.g. nightly-2026-08-05
cargo install cargo-fuzz                   # once
```

## Targets

| Target                 | What it exercises |
|------------------------|-------------------|
| `snbt`                 | `TagParser.parse_fully` / `parse_as_argument` / `parse_compound_fully` / `parse_compound_as_argument` over lossy-UTF-8 input |
| `snbt_roundtrip`       | `parse(print(tag)) == tag` for every successfully parsed input — catches printer bugs that emit invalid SNBT and parser bugs that reject its own output |
| `nbt_binary`           | `NbtIo.read_any_tag` / `read_unnamed_tag` / `read` / `read_compressed` over raw bytes (gzip + ungzipped) |
| `nbt_binary_visitor`   | `NbtIo.parse` / `parse_compressed` with an always-`Continue` visitor, forcing full tree traversal through the stream-visitor dispatch |
| `codec_decode`         | DFU codec combinators (`int`/`string`/`bool`/`list`/`pair`/`either`/`unbounded_map`/`compound_list`/`RecordCodecBuilder`/`passthrough`) decoding SNBT-parsed `Tag` values over `NbtOps` |
| `nbt_binary_roundtrip` | `NbtIo.writeUnnamedTagWithFallback` canonicalization idempotence on successfully-parsed input — write → re-read → write is byte-identical (covers the MUTF-8 encoder, NaN re-canonicalization, and the `StringFallbackDataOutput` overflow path) |
| `data_io_modified_utf8` | the modified-UTF-8 wire codec (`decode_modified_utf8` / `write_utf_body`) — canonicalization idempotence `decode(encode(decode(x))) == decode(x)`, no panic on malformed input |
| `codec_compressed_decode` | the DFU compressed-map decode path (`compressMaps() == true`) over `JsonOps::COMPRESSED` + `JsonOps::INSTANCE` — `KeyCompressor`, packed-list slots, null slots, list-length bounds |
| `packet_status`        | `rivet-protocol` status/serverbound decode through the registration-built id-dispatch codec (`status_request` + `ping_request` bodies, issue #46) |
| `packet_login`         | `rivet-protocol` login/serverbound decode (`hello` bounded string + UUID, `login_acknowledged`), offline join path, issue #46 |
| `packet_configuration` | `rivet-protocol` configuration/serverbound decode (`finish_configuration`, `select_known_packs` `list(64)`), issue #46 |
| `packet_play`          | `rivet-protocol` play/serverbound decode (the nine ported issue #97 bodies incl. the `move_player` variants + enum-ordinal reads), issue #46 |
| `packet_client_information` | the shared `ClientInformation` body codec (configuration id 0 / play id 14), issue #46 |

The packet targets reuse the crate's real packet bodies and codecs: each
builds a `serverbound_protocol` template via `ProtocolInfoBuilder` +
`IdDispatchCodec`, mirroring the protocol-layer id-dispatch decode (the port
of Java's `IdDispatchCodec` / `PacketDecoder` path) with no dispatch
duplication. They are not wired to rivet-server's inbound listeners, which
dispatch per-state with `packet_id` + `decode_packet` under a `catch_unwind`
boundary; the targets call the dispatch codec directly, which is why they need
the filtered panic hook below.

## Build & run

```sh
cargo fuzz build              # builds all targets (nightly required)
cargo fuzz run snbt           # runs indefinitely until a crash is found
cargo fuzz run nbt_binary -- -runs=10000
cargo fuzz run codec_decode -- -max_total_time=300   # 5 minutes
cargo fuzz run nbt_binary_roundtrip -- -runs=10000
cargo fuzz run data_io_modified_utf8 -- -runs=10000
cargo fuzz run codec_compressed_decode -- -runs=10000

# The packet targets need the `packets` feature (rivet-protocol's generated
# packet tables + packet bodies), which this package forwards on:
cargo fuzz build --features packets
cargo fuzz run packet_play --features packets -- -runs=10000
```

For a target with a committed seed set, copy the seeds into its corpus first —
see "Seed corpus & regressions".

Crashes are written to `fuzz/artifacts/<target>/`; the reproducing input and the
stack trace are printed. Corpora accumulate in `fuzz/corpus/<target>/`. The
generic workspace invocation (gate.sh `--workspace`) does not enable the
`rivet-fuzz` package's `packets` feature — it is that package feature, not
`rivet-protocol`'s `packets` workspace-wide, that stays off — so the plain
workspace build is unchanged. The merge gate type-checks and lints the packet
targets explicitly via `cargo check/clippy -p rivet-fuzz --features packets`.

## Deterministic seed regressions (`cargo test -p rivet-fuzz`)

`cargo-fuzz` (0.13.x) never reads `fuzz/seeds/` automatically — a plain
`cargo fuzz run <target>` only reads (and writes) `fuzz/corpus/<target>/`. The
deterministic complement lives in the `rivet-fuzz` library (`fuzz/src/`):
every committed seed in `fuzz/seeds/<target>/` is fed through the exact same
target body libFuzzer invokes, and a seed that stops parsing, changes behavior,
or trips a non-faithful panic fails the test:

```sh
cargo test -p rivet-fuzz    # runs on the pinned stable toolchain — no nightly needed
```

The target bodies are factored into `rivet_fuzz::targets` (one callable per
step); each `fuzz_targets/*.rs` bin is a thin shim, so libFuzzer and the
regressions drive identical code. Faithful panics (negative list length,
missing list element type, oversized array, accounter quota/depth,
compressed-map out-of-bounds) are classified against the same
`FAITHFUL_PANIC_FRAGMENTS` table the fuzzers' panic filter uses and are
tolerated — they are the intended outcome for those seeds. Individual guarded
paths are pinned by focused tests in `rivet_fuzz::regress`, e.g. the oversized
byte-array seed declaring `0x01000000` at the byte-array offset (proving the
length guard fires before any 16 MiB allocation) and the
`StringFallbackDataOutput` write-path fallback.

Because a target body silently `return`s when a seed stops parsing (the
roundtrip/SNBT/MUTF-8/compressed bodies all gate on a successful parse),
`intended_reachable_seeds_reach_their_core_work` pins the seeds each target is
documented to run its core assertion on — a seed that regresses to a rejected
form (the bug that previously left the roundtrip write path uncovered) fails
the test instead of silently no-oping.

## Seed corpus & regressions

`fuzz/seeds/<target>/` holds committed seed inputs that pin the known edge
cases so a fresh run starts already covering them. `cargo-fuzz` has no seed
handling of its own — a plain `cargo fuzz run <target>` only reads (and writes)
`fuzz/corpus/<target>/`, which it creates empty — so seed the target's corpus
before a run:

```sh
fuzz/seed_corpus.sh nbt_binary              # copies fuzz/seeds/<target> -> fuzz/corpus/<target>
fuzz/seed_corpus.sh data_io_modified_utf8
fuzz/seed_corpus.sh codec_compressed_decode
cargo fuzz run nbt_binary                   # now starts from the regression seeds
```

`seed_corpus.sh` copies (never moves, never hard-links) the committed seeds
into the mutable corpus, so re-running it re-pins the regression cases after a
long fuzz session has mutated the corpus. Do not pass `fuzz/seeds/<target>` as
a corpus argument yourself: libFuzzer treats the *first* positional corpus
argument as the output corpus and writes every discovered input there, which
would mutate the committed seeds. Keeping the corpus as a separate copy is what
keeps `fuzz/seeds/` clean for committing.

The seeds cover the cases the targets assert on:

- **Binary NBT** (`nbt_binary*`): negative list length, missing list element
  type, oversized array (`>= 1 << 24`), truncated streams, deep nesting,
  gzip-compressed input, malformed modified-UTF-8, non-canonical NaN float/
  double payloads, and — for `nbt_binary_roundtrip` — the write-path
  canonicalization seeds. In the roundtrip corpus the NaN / overlong-MUTF-8 /
  raw-NUL seeds (`nbt_nan_float`, `nbt_nan_double`, `nbt_overlong_utf8`,
  `nbt_raw_nul_utf8`, `nbt_bad_utf8`) are well-formed compounds that parse, so
  the write → re-read → write idempotence assertion actually runs on them: a
  non-canonical NaN float (`0x7fc01234`) and double (`0x7ff8000000000001`) are
  re-canonicalized to `0x7fc00000` / `0x7ff8000000000000`, an overlong `C1 80`
  string re-encodes to its canonical single-byte form, a raw-NUL string
  re-encodes as `C0 80`, and a 3-byte overlong `E0 80 80` re-encodes as the
  2-byte NUL `C0 80`; `nbt_empty_root` and `nbt_rich` round out the
  roundtrip-writeable set. The *same seed names* in `nbt_binary` /
  `nbt_binary_visitor` are the truncated read-rejection forms shared with
  `nbt_truncated`. `too_long_write` is a 40 KB raw-NUL string that re-encodes
  past the 65535-byte write limit and is the seed that exercises the
  `StringFallbackDataOutput` write-path fallback.
- **Modified UTF-8** (`data_io_modified_utf8`): raw NUL, overlong `C1 80`,
  canonical `C0 80`, 2/3-byte forms, astral surrogate pairs, unpaired
  surrogates (high/low), truncated leads, bad continuation bytes, the 4-byte
  form Java rejects, and a 40 KB raw-NUL input whose canonical re-encoding
  exceeds 65535 bytes (`too_long_write`, a faithful `UTFDataFormatException`).
- **Compressed-map decode** (`codec_compressed_decode`): packed-list and object
  JSON forms, short/empty lists, wrong-type slots, null slots, unknown keys
  (slot-0 default), non-list input to the compressed path, deep nesting, and
  trailing garbage (the target parses the first JSON value and tolerates the
  trailing bytes, so the `trailing` seed reaches the codec battery).

## Faithful panics and the panic filter

The rivet-nbt binary readers are a faithful port of Java's `NbtIo`, which
crashes (unchecked `RuntimeException`) on inputs the byte-level codec does not
guard — negative list length, missing list element type, oversized array
(`length >= 1 << 24`), and `NbtAccounter` quota/depth overruns. Feeding
arbitrary bytes
hits those immediately, and a fuzzer that dies on them is useless.

`libfuzzer-sys` installs a panic hook that aborts the process on *every* panic,
so `catch_unwind` alone cannot tolerate faithful panics. The binary targets
therefore install a filtered panic hook (`rivet_fuzz::common`, re-exported via
`fuzz_targets/common.rs`) that swallows panics whose message matches a
known-faithful site and aborts on anything else. A panic that is not recognized
(a genuine bug) still crashes the fuzzer and writes an artifact.

Keep `FAITHFUL_PANIC_FRAGMENTS` in sync with the `panic!` sites in
`crates/rivet-nbt/src/nbt_io.rs` and `nbt_accounter.rs` and with the
compressed-map index panic in `crates/rivet-serialization`. The SNBT targets
and the `codec_decode` (NbtOps) target do not use the filter: `TagParser`
reports errors via `NbtFormatException` and the NbtOps codec combinators return
`DataResult::error`, so any panic there is a real bug. The
`codec_compressed_decode` target *does* use it, because an out-of-range
packed-list index is Java's `IndexOutOfBoundsException` (faithful), not a bug.

The accounter in the binary targets is bounded to the server's default 2 MiB
quota (`NbtAccounter::default_quota`), so a hostile input cannot force a huge
allocation before the quota panic fires.

The packet targets use a shared filtered panic hook (`fuzz_targets/guard.rs`)
instead of the NBT targets' `common.rs`. Packet decode reaches the raw netty
layer's faithful panic sites on hostile input (EOF via the `bytes` crate's
`"advance out of bounds"`, over-length varints, negative collection capacities,
out-of-range enum ordinals), so `guard.rs` swallows exactly those messages and
aborts on anything else — a genuine bug still crashes the fuzzer. The
codec-boundary paths that report an error without panicking (`string_utf8`'s
length cases, an over-limit `list_max` count, the `from_id` enum reads) return
`Err` and are deliberately not panic-message filters — a *negative* `list_max`
count is the one collection-path panic, covered by the `"Illegal Capacity"`
entry. The targets also cap the input length (`guard::MAX_INPUT_LEN`) so the
only input-proportional allocation (`decode_utf8`'s scratch string) stays
small.
