# rivet-decode — the serverbound play decode/capture harness (issue #97)

A reusable, deterministic decode/capture tool over the `rivet-protocol`
serverbound play slice. It decodes real protocol-776 packets from a raw byte
stream, produces a normalized JSONL transcript, verifies a capture corpus
against a SHA-256 provenance manifest, runs a hostile-input mutation matrix,
and checks that framing is independent of how the TCP bytes are chunked.

## Why not the `ProtocolInfoBuilder`?

`ProtocolInfoBuilder` assigns network ids in `addPacket` registration order, so
a slice of the nine ported packets gets ids `0..8`. A real capture is framed
with the **vanilla** ids (`accept_teleportation 0`, `chunk_batch_received 11`,
..., `player_action 41`), so the harness builds a 69-entry table indexed by the
generated protocol id. The nine ported packets (`rivet_protocol::game`) decode
with their real codecs; the other sixty are raw passthrough — their body bytes
are captured, not interpreted.

## Subcommands

```bash
cargo run -p rivet-decode -- decode <capture.bin>        # stream -> JSONL transcript
cargo run -p rivet-decode -- capture <dir> <id:hex>...   # payloads -> corpus dir
cargo run -p rivet-decode -- verify <dir>                # corpus vs manifest + decode
cargo run -p rivet-decode -- mutate <dir>                # hostile-input mutation matrix
cargo run -p rivet-decode -- frag <capture.bin>          # fragmentation/coalescing checks
```

Exit codes (the `rivet-oracle` gate contract):
`0` = VERIFIED / all checks passed; `1` = FAILED; `3` = UNVERIFIED (a required
input is missing or unreadable). Any other nonzero exit is a tool failure.

### `decode`

Splits a varint21-framed capture stream into frames and prints one JSON object
per packet to stdout (JSONL). The transcript is normalized and deterministic:
floats/doubles are emitted as raw IEEE-754 bits (`0x40600000`), so NaN/Inf and
negative zero survive exactly; enums use the Java constant name. Every
`fields` object emits the same key order, and `frame_hex` records the exact
wire bytes for byte-exact round-trip checking.

### `capture`

Writes a corpus directory from `id:hex` packet payloads (the payload is
`[packet id varint][body]`, i.e. what the varint21 frame wraps). Writes one
`.frame` file per packet and a `manifest.json` with a SHA-256 per file plus
provenance (`tool`, `format`, `state`, `flow`, `protocol`). This is a manual
build tool, not a Paper-boot capture: the join-scenario capture extraction
that records a *real* client join lives in `rivet-oracle`.

### `verify`

Reads a corpus, checks every file's SHA-256 and byte count against the
manifest, then re-decodes every frame and re-encodes it (a decode failure or a
re-encode that diverges from the corpus bytes fails the run).

### `mutate`

Applies the hostile-input mutation matrix to the captured frames:
- `unknown_id` — packet id outside the 0..69 table → must reject with
  `Received unknown packet id n`.
- `enum_out_of_range` — `client_command`/`player_action` ordinal past its
  range → must reject with Java's `ArrayIndexOutOfBoundsException` text.
- `nan_inf` — the first four body bytes overwritten with quiet-NaN bits. For a
  `chunk_batch_received` float that is a true NaN; for a `move_player_*` double
  it is a finite, huge value (the bits only cover the high half). Either way
  the raw-bit codecs must still decode (benign).
- `varint_boundary` — a 5-byte continuation varint → must reject (`VarInt too
  big`).
- `truncation` — body cut short → must reject.

A row whose target packet is not in the corpus is reported as skipped (honest
non-application, never a silent pass).

### `frag`

Feeds a capture stream to the varint21 frame decoder three ways — byte-at-a-time
(maximal fragmentation), coalesced (whole stream at once), and fixed-size
chunks — and requires every split to reproduce the identical frame sequence.

## Committed fixture

`fixtures/corpus/` is the canonical nine-packet capture (one per ported
packet). `tests/integration.rs::committed_corpus_fixture_is_byte_stable` pins
it: every frame must decode with a real codec and re-encode byte-exactly, and
the manifest SHA-256s must hold. Regenerate deliberately with `capture`, never
edit the fixture to make a test pass.

## Conventions

- The crate depends on `rivet-protocol` with the `packets` feature (the
  `game`/`generated`/`protocol` modules are feature-gated).
- `serde_json` uses `preserve_order` so the normalized transcript is
  byte-stable across runs.
- The binary silences the default panic hook: hostile input deliberately
  triggers panics (unchecked Java exceptions map to panics in this port), which
  are caught and reported as `Err`s.
