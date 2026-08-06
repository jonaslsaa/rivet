# rivet-fuzz

`cargo-fuzz` targets for the rivet parser crates. These feed untrusted bytes
into the SNBT `TagParser`, the binary NBT readers (`NbtIo`), and the DFU codec
combinators over `NbtOps`, looking for panics, overflows, infinite loops, and
round-trip failures.

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

## Build & run

```sh
cargo fuzz build              # builds all targets (nightly required)
cargo fuzz run snbt           # runs indefinitely until a crash is found
cargo fuzz run nbt_binary -- -runs=10000
cargo fuzz run codec_decode -- -max_total_time=300   # 5 minutes
```

Crashes are written to `fuzz/artifacts/<target>/`; the reproducing input and the
stack trace are printed. Corpora accumulate in `fuzz/corpus/<target>/`.

## Faithful panics and the panic filter

The rivet-nbt binary readers are a faithful port of Java's `NbtIo`, which
crashes (unchecked `RuntimeException`) on inputs the byte-level codec does not
guard — negative list length, missing list element type, oversized array
(`< 1 << 24`), and `NbtAccounter` quota/depth overruns. Feeding arbitrary bytes
hits those immediately, and a fuzzer that dies on them is useless.

`libfuzzer-sys` installs a panic hook that aborts the process on *every* panic,
so `catch_unwind` alone cannot tolerate faithful panics. The binary targets
therefore install a filtered panic hook (`fuzz_targets/common.rs`) that swallows
panics whose message matches a known-faithful site and aborts on anything else.
A panic that is not recognized (a genuine bug) still crashes the fuzzer and
writes an artifact.

Keep `FAITHFUL_PANIC_FRAGMENTS` in sync with the `panic!` sites in
`crates/rivet-nbt/src/nbt_io.rs` and `nbt_accounter.rs`. Note that the SNBT and
codec targets do not use the filter: `TagParser` reports errors via
`NbtFormatException` (no panics), so any panic there is a real bug.

The accounter in the binary targets is bounded to the server's default 2 MiB
quota (`NbtAccounter::default_quota`), so a hostile input cannot force a huge
allocation before the quota panic fires.
