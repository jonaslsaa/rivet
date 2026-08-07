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

# The packet targets need the `packets` feature (rivet-protocol's generated
# packet tables + packet bodies), which this package forwards on:
cargo fuzz build --features packets
cargo fuzz run packet_play --features packets -- -runs=10000
```

Crashes are written to `fuzz/artifacts/<target>/`; the reproducing input and the
stack trace are printed. Corpora accumulate in `fuzz/corpus/<target>/`. The
workspace build (gate.sh `--workspace`) leaves `packets` off, so the fuzz
package does not change what the gate compiles.

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
