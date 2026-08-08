# SPEC — GitHub #264: canonical boundary representation for Java strings holding isolated UTF-16 surrogates

**Status:** specification (research complete; ground truth captured by `spikes/surrogate-probe`).
**Owner issue:** #264. Parent epic: M0 foundation (rivet-util / rivet-nbt / rivet-protocol).
**Verdict in one line:** keep Rust `String` as the canonical internal string type; at the three boundaries where a
Java `String` can hold an isolated UTF-16 surrogate that Rust `String` cannot, **error deterministically, never
lossily replace**. The protocol (network) boundary is already fully faithful via the WHATWG UTF-8 decoder — Java
itself produces U+FFFD there, so there is no divergence to fix.

---

## 1. Why this matters

Java `String` is a UTF-16 code-unit array and can hold an isolated (unpaired) surrogate `0xD800..0xDFFF`.
Rust `String` is valid UTF-8 and cannot. Every boundary where Paper moves a `String` over a byte stream therefore
has three possible behaviors when a lone surrogate is present: **preserve** (Java does this for MUTF-8 NBT and
SNBT), **lossy-replace** (Java does this for the protocol, via `new String(bytes, UTF_8)` → U+FFFD), or **error**
(the only honest Rust option when the byte stream is not UTF-8). This spec pins the canonical behavior per boundary
so parity checks are deterministic and no code silently drops data.

The current codebase already implements an "unsupported-surrogate policy" in several places, each referencing #264:
`crates/rivet-util/src/data_io.rs` (`unpaired_surrogate()` error),
`crates/rivet-nbt/src/tag_parser.rs` (`apply_hex_escape` / `\N{}` lone-surrogate → `ERROR_INVALID_CODEPOINT`),
`crates/rivet-nbt/src/unicode_name_table.rs` (`CodePointOfError::LoneSurrogate`).
This spec confirms, rationalizes, and canonizes that policy; it does not propose a new type.

## 2. Ground truth (live probes, JDK 25.0.2 + netty 4.2.15 + Gson 2.13.1)

`spikes/surrogate-probe/run.sh` runs both halves and prints JSON Lines. Key results:

| Boundary | Input | Java behavior (ground truth) | Rust today |
|---|---|---|---|
| `DataOutputStream.writeUTF` (NBT write) | `"\uD800"` | `00 03 ED A0 80` (CESU-8 surrogate, 2-byte len) | unreachable: `write_utf_body` takes `&str` |
| `DataInputStream.readUTF` (NBT read) | `ED A0 80` | `String` holding `U+D800` (`codePointCount=1`) | `Err` (`unpaired surrogate in modified UTF-8 …`) |
| netty `ByteBufUtil.writeUtf8` (protocol write) | `"\uD800"` | `3f` (single `?` — UTF-8 encoder replaces) | unreachable |
| netty `Utf8String.read` (protocol read) | `ED A0 80` (varint framed) | `U+FFFD` (1 unit), passes maxLength | `U+FFFD` (WHATWG) — **matches** |
| JDK `new String(b, UTF_8)` | `ED A0 80` | `U+FFFD` | `U+FFFD` (WHATWG) — **matches** |
| Gson parse/serialize | `"\ud800"` | parses to lone surrogate; re-serializes as `"?"` | serde_json **errors** (`lone leading surrogate …`) |
| SNBT `\uHHHH` / `\N{name}` | `\uD800` / `HIGH SURROGATES D800` | accepted, `Character.toString(0xD800)` → lone surrogate | `ERROR_INVALID_CODEPOINT` |
| `StringTag.quoteAndEscape` (SNBT printer) | `"\uD800"` | `"` + raw U+D800 + `"` | n/a (value can't exist) |

Two facts are load-bearing:

1. **The protocol boundary is already byte-faithful.** `Utf8String.read` in Java does
   `input.toString(readerIndex, bufferLength, UTF_8)`, i.e. the JDK UTF-8 decoder, which produces U+FFFD for a
   lone-surrogate byte sequence. The Rust port's WHATWG decoder produces the same single U+FFFD (verified
   differentially in `utf8_string.rs` and reconfirmed by the probe). There is **no** divergence to fix on the
   network path. Only the *length check* afterwards counts UTF-16 units; U+FFFD is 1 unit on both sides.
2. **A surrogate-preserving boundary type is not justified.** A `Vec<u16>`/`String16` type at the NBT/protocol
   boundary would propagate into every `StringTag.value`, `CompoundTag` key, tag visitor, SNBT printer, `NbtOps`
   (DFU), `Component` serialization and `Identifier` — a cross-cutting refactor of the whole foundation for input
   that (a) Paper's own worldgen never produces, (b) only hostile/manually-crafted NBT files or chat JSON can carry,
   and (c) would still hit the serde_json wall on the JSON side. Per the "simplest implementation that fully meets
   current requirements" rule, reject.

## 3. Boundary inventory (all reachable surfaces)

| # | Boundary | Java surface | Rust surface | Canonical behavior |
|---|---|---|---|---|
| B1 | Modified UTF-8 **read** | `DataInputStream.readUTF` | `rivet_util::data_io::decode_modified_utf8` | **error** on unpaired surrogate (current) |
| B2 | Modified UTF-8 **write** | `DataOutputStream.writeUTF` | `rivet_util::data_io::write_utf_body` | n/a (takes `&str`; lone surrogate unrepresentable) |
| B3 | NBT string tag read | `StringTag.TYPE.load` → `readUTF` | `rivet-nbt::nbt_io::load` → `decode_modified_utf8` | **error** (surfaces as `ReportedNbtException`-style `Err` / panic-wrap at `FriendlyByteBuf` bridge) |
| B4 | NBT compound key read | `CompoundTag.readString` → `readUTF` | `nbt_io::read_compound_string` | **error** (same decoder) |
| B5 | SNBT hex/name escape | `SnbtGrammar.stringEscapeSequence` | `rivet-nbt::tag_parser` (`apply_hex_escape`, `\N{}`) | **error** `ERROR_INVALID_CODEPOINT` (current) |
| B6 | SNBT printer | `StringTag.quoteAndEscape` | `rivet-nbt::string_tag::quote_and_escape` | n/a (input can't exist); doc: Java emits raw surrogate |
| B7 | Protocol string read | `Utf8String.read` (WHATWG via JDK) | `rivet-protocol::utf8_string::decode_utf8` | **lossy-replace** — identical to Java (current, correct) |
| B8 | Protocol string write | `Utf8String.write` (netty `writeUtf8`) | `rivet-protocol::utf8_string::write` | n/a (takes `&str`); Java emits `?` — unreachable |
| B9 | JSON (chat/component) | Gson `JsonParser` | `serde_json` (via `rivet-serialization::JsonOps`, `rivet-text`) | **divergence documented**: serde_json rejects lone-surrogate escapes; Gson accepts & emits `?`. Keep serde_json; document |
| B10 | Registry/identifier | `Identifier.parse` (ASCII-only) | `rivet-registry::identifier` | n/a (ASCII), doc-only |
| B11 | Filesystem paths | `java.nio.file.Path` (UTF-8/OS encoding) | `std::path::PathBuf` / `to_string_lossy` | n/a in current tree (no `Path` port yet); specify `to_string_lossy`+document when the `Path` wave lands |
| B12 | JVM-adapter FFI | Java `String` → C `char*` | `rivet-ffi` (future) | Specify below (§6): marshal UTF-8, lone surrogate → `?` on the Java side (matches netty) or `Err` |

## 4. Canonical decision (D14 candidate)

**D14 — Isolated UTF-16 surrogates: Rust `String` is canonical; unpaired surrogates error, never silently
replace.**

- Internal string type everywhere: Rust `String` (valid UTF-8). No `Vec<u16>` boundary type.
- **Read boundaries that are MUTF-8 or SNBT (B1/B3/B4/B5): error** on an unpaired surrogate, with a message naming
  the byte position (MUTF-8) or `U+XXXX` (SNBT). These are the only boundaries where Java *preserves* a lone
  surrogate; Rivet rejects because Rust `String` cannot hold it. This is a **documented `diverged` parity case**,
  never a silent data loss.
- **Read boundary that is protocol UTF-8 (B7): lossy-replace to U+FFFD**, which is *already byte-for-byte what
  Java does* — not a divergence.
- **JSON boundary (B9): keep serde_json; document** that `"\ud800"` errors in Rust where Gson accepts it. Do not
  build a Gson-faithful JSON parser for this.
- **Never** map a lone surrogate to `?` in internal state (that is netty/Gson's serialization behavior, not a
  storage invariant). If a future wave needs byte-identity on hostile NBT fixtures, revisit via a `SurrogatePolicy`
  enum at the decoder — explicitly deferred, not speculative now.

## 5. Options compared (why not the alternatives)

| Option | Effect | Rejected because |
|---|---|---|
| **A. `String` + error (chosen)** | Hostile MUTF-8/SNBT input errors; protocol matches Java; JSON divergence documented | — |
| **B. `Vec<u16>` boundary type** | Full wire parity on hostile NBT | Touches every tag/visitor/DFU/Component/Identifier consumer; serde_json still can't round-trip; violates "no speculative broad refactor" |
| **C. `String` + U+FFFD replacement everywhere** | Reads never error | Silent data loss; MUTF-8 re-encode (`EF BF BD` ≠ `ED A0 80`) breaks byte-identity on hostile fixtures; protocol already replaces, NBT should not |
| **D. Swap to a surrogate-parseable JSON lib** | Gson-faithful chat JSON | New heavyweight dep for one hostile-input edge; workspace already standardized on serde_json (CRATES.md) |

## 6. Executable oracle/fixture probes

**Already built and run:** `spikes/surrogate-probe/` — standalone (own `[workspace]`), prints JSON Lines:
- `run-java.sh`: JDK 25 + netty 4.2.15 + Gson ground-truth (the table in §2).
- `cargo run` (Rust counter-probe): `decode_modified_utf8`, `write_utf_body`, WHATWG decode, serde_json.
- `run.sh`: runs both halves back-to-back (Java JSON Lines, then Rust JSON Lines) for manual comparison.
Run: `spikes/surrogate-probe/run.sh`. (Requires netty 4.2.15/Gson in `~/.gradle`; overridable via `NETTY_BUF`/`NETTY_COMMON`/`GSON`.)

**To add (implementation sub-issue #264-b, requires the M0 materialized Paper jar for the oracle):**
- `rivet-reference-oracle`: add `nbt.decode` corpus cases for the lone-surrogate root named-tag payloads
  `08 00 00 00 03 ED A0 80` (a `StringTag` named `""` whose value is `U+D800`; framing per `NbtIo.read`:
  type byte `08` + empty MUTF-8 name `00 00` + value length `00 03` + CESU-8 bytes `ED A0 80`) and the low form
  `08 00 00 00 03 ED B0 80`; add `snbt.parse` cases `"\uD800"` and `"\udfff"`. Java returns `ok` (preserves);
  Rivet errors → classify as `diverged` in `rivet-parity` (same bucket as the documented `compound_key_order`),
  **never** `mismatched`.
- Keep the fixture set small and committed under `tools/rivet-oracle/fixtures/` following the existing pattern.

## 7. Topologically staged implementation plan

Depends on nothing but the current tree. Each stage is a small PR against #264's sub-issues.

1. **Stage 0 — baseline (already in tree).** The error policy is implemented and tested in `data_io.rs`,
   `tag_parser.rs`, `unicode_name_table.rs`, and the `friendly_byte_buf.rs` NBT-bridge tests. No code change.
2. **Stage 1 — canonize in reference docs (#264-a).** Add `DECISIONS.md` D14 (the §4 text), a `PORTING.md` row in
   the string type table, and a `PARITY.md` divergence note for the lone-surrogate MUTF-8/SNBT cases. Docs only,
   via a dedicated docs PR (per WORKFLOWS: reference docs change via dedicated PRs).
3. **Stage 2 — oracle corpus (#264-b).** Extend `rivet-reference-oracle` + `rivet-parity` per §6; wire the
   `diverged` classification so the gate stays green and the scoreboard shows the count. Requires the M0 Paper run.
4. **Stage 3 — JSON counterfactual test (#264-c).** Add a focused `rivet-text`/`rivet-serialization` test
   asserting serde_json *rejects* `"\ud800"` (locking the documented divergence) and that a Gson-generated chat
   JSON containing `\ud800` is rejected, matching the counterfactual in `spikes/surrogate-probe`.
5. **Stage 4 — FFI string marshalling policy (#264-d, future M3+).** When `rivet-ffi` ships, marshal Java `String`
   to `char*` with `getBytes(UTF_8)` (lone surrogate → `?`, byte-for-byte what netty does) OR reject with an ABI
   error status; document one, chosen when the adapter wave lands. Not implemented now.

No crate-cycle impact: all changes are additive (tests/fixtures/docs) or within existing leaves. `rivet-util`,
`rivet-nbt`, `rivet-protocol`, `rivet-serialization` dependency directions are untouched.

## 8. Issue decomposition (sub-issues of #264)

- **#264-a — Docs:** DECISIONS.md D14 + PORTING.md row + PARITY.md divergence note. Small, standalone, no deps.
- **#264-b — Oracle:** reference-oracle lone-surrogate corpus + rivet-parity `diverged` wiring. Depends on M0 Paper
  jar materialization (gate prereq).
- **#264-c — JSON counterfactual test:** serde_json-vs-Gson lone-surrogate lock-in `rivet-text`/`rivet-serialization`.
- **#264-d — FFI policy (future):** string marshalling choice at the JVM adapter boundary. Blocked until M3/M4.

Nothing here is blocked on un-landed waves; #264-a and #264-c are runnable immediately.
