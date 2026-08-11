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

The current codebase already implements an "unsupported-surrogate policy" in several places (flagged for this issue
by a `RivetTodo(#264)` marker in `unicode_name_table.rs`):
`crates/rivet-util/src/data_io.rs` (`unpaired_surrogate()` error),
`crates/rivet-nbt/src/tag_parser.rs` (`apply_hex_escape` / `\N{}` lone-surrogate → `ERROR_INVALID_CODEPOINT`),
`crates/rivet-nbt/src/unicode_name_table.rs` (`CodePointOfError::LoneSurrogate`).
This spec confirms, rationalizes, and canonizes that policy; it does not propose a new type.

## 2. Ground truth (live probes, JDK 25.0.2 + netty 4.2.15 + Gson 2.14.0)

`spikes/surrogate-probe/run.sh` runs both halves and prints JSON Lines. Key results:

| Boundary | Input | Java behavior (ground truth) | Rust today |
|---|---|---|---|
| `DataOutputStream.writeUTF` (NBT write) | `"\uD800"` | `00 03 ED A0 80` (CESU-8 surrogate, 2-byte len) | unreachable: `write_utf_body` takes `&str` |
| `DataInputStream.readUTF` (NBT read) | `ED A0 80` | `String` holding `U+D800` (`codePointCount=1`) | `Err` (`unpaired surrogate in modified UTF-8 …`) |
| netty `ByteBufUtil.writeUtf8` (protocol write) | `"\uD800"` | `3f` (single `?` — UTF-8 encoder replaces) | unreachable |
| netty `Utf8String.read` (protocol read) | `ED A0 80` (varint framed) | `U+FFFD` (1 unit), passes maxLength | `U+FFFD` (WHATWG) — **matches** |
| JDK `new String(b, UTF_8)` | `ED A0 80` | `U+FFFD` | `U+FFFD` (WHATWG) — **matches** |
| Gson parse/serialize | `"\ud800"` / `"\udc00"` | both parse to lone surrogates; re-serialize as `"?"` | serde_json **rejects both** — `"\ud800"` → `unexpected end of hex escape`; `"\udc00"` → `lone leading surrogate in hex escape` (serde_json mislabels the trailing surrogate as "leading") |
| SNBT `\uHHHH` / `\N{name}` | `\uD800` / `HIGH SURROGATES D800` | accepted, `Character.toString(0xD800)` → lone surrogate | `ERROR_INVALID_CODEPOINT` |
| `StringTag.quoteAndEscape` (SNBT printer) | `"\uD800"` | `"` + raw U+D800 + `"` | n/a (value can't exist) |

Provenance of the SNBT rows (Java and Rust): the Java `Character.*` results (`codePointOf`, `toString`,
`isValidCodePoint`) are live-probed via the JDK. The parser *acceptance* claim — `SnbtGrammar.stringEscapeSequence`
reads `\uHHHH` as `Character.isValidCodePoint` + `Character.toString`, so `\uD800` is accepted and yields a `String`
holding the lone surrogate — is read from the pinned Paper source; the real parser lives in the M0 materialized Paper
jar (deferred to #264-b), so the standalone probe has **no live SNBT parser**. The B6 printer row comes from a
*full faithful port* of `StringTag.quoteAndEscape` (including `SnbtGrammar.escapeControlCharacters`), verified against
the pinned source; for the probed surrogate inputs it emits `"` + raw U+D800 + `"`. The Rust side is verified by the
tree unit tests in `tag_parser.rs` (`apply_hex_escape` / `\N{}` → `ERROR_INVALID_CODEPOINT`) and
`unicode_name_table.rs` (`CodePointOfError::LoneSurrogate`), not by the standalone probe (which deliberately avoids a
`rivet-nbt` dependency chain).

Provenance of the protocol rows: the Rust probe's `whatwg` module is a byte-identical mirror of
`rivet-protocol::utf8_string::decode_utf8` (copied so the probe crate need not depend on `rivet-protocol`) — it is
**not** an independent check. The independent ground truth for the protocol boundary is the Java JDK decoder behind
the `jdk_decode_*` rows (and netty `Utf8String.read`). The Rust probe is a *counter*-probe: it reproduces the crate
code, while Java supplies the external oracle.

Two facts are load-bearing:

1. **The protocol boundary is already byte-faithful.** `Utf8String.read` in Java does
   `input.toString(readerIndex, bufferLength, UTF_8)`, i.e. the JDK UTF-8 decoder, which produces U+FFFD for a
   lone-surrogate byte sequence. The Rust port's WHATWG decoder produces the same single U+FFFD (verified
   differentially in `utf8_string.rs` and reconfirmed by the probe — the probe's `whatwg` module mirrors the crate
   decoder, so the independent check here is Java's JDK decoder, per the provenance note above). There is **no**
   divergence to fix on the network path. Only the *length check* afterwards counts UTF-16 units; U+FFFD is 1 unit
   on both sides.
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
| B12 | JVM-adapter FFI | Java `String` → C `char*` | `rivet-ffi` (existing stub crate, empty) | Specify in §4 / §7 Stage 4: marshal UTF-8, lone surrogate → `?` on the Java side (matches netty) or `Err` |
| B13 | Filesystem file **contents** (configs, datapacks) | Java `Files.readString` (UTF-8) / `Properties.load` (ISO-8859-1) | serde/toml (future server-properties, `world/` files) | n/a in current tree (no config/datapack loader yet); specify `from_utf8`+error when the loader wave lands — a lone surrogate in a config file cannot round-trip through Rust `String` |

The inventory is about **lone** surrogates. A *paired* surrogate (`𐀀` = U+10000) is a valid code point and
round-trips on every boundary: Java keeps the two UTF-16 units while Rust yields one scalar (`𐀀`), but both re-encode
to the same bytes (probe `readUTF_pair`/`rust_mutf8_decode_pair`, `writeUTF_pair`/`rust_mutf8_encode_pair`), so it is
not a divergence.

## 4. Canonical decision (D14 candidate)

**D14 — Isolated UTF-16 surrogates: Rust `String` is canonical; unpaired surrogates error, never silently
replace.**

- Internal string type everywhere: Rust `String` (valid UTF-8). No `Vec<u16>` boundary type.
- **Read boundaries that are MUTF-8 or SNBT (B1/B3/B4/B5): error** on an unpaired surrogate — the MUTF-8 decoder
  with a generic `unpaired surrogate in modified UTF-8` message (Stage 0), SNBT naming the code point as `U+XXXX`.
  Java `readUTF` never fails on a surrogate (it preserves it), so there is no byte-offset diagnostic to mirror.
  These are the only boundaries where Java *preserves* a lone surrogate; Rivet rejects because Rust `String`
  cannot hold it. This is a **documented `diverged` parity case**, never a silent data loss.
- **Read boundary that is protocol UTF-8 (B7): lossy-replace to U+FFFD**, which is *already byte-for-byte what
  Java does* — not a divergence.
- **JSON boundary (B9): keep serde_json; document** that `"\ud800"` errors in Rust where Gson accepts it. Do not
  build a Gson-faithful JSON parser for this.
- **Never** map a lone surrogate to `?` in internal state (that is netty/Gson's serialization behavior, not a
  storage invariant). If a future wave needs byte-identity on hostile NBT fixtures, revisit via a `SurrogatePolicy`
  enum at the decoder — explicitly deferred, not speculative now.

### Implications of the decision

- **Fidelity.** Byte-for-byte parity where Java *preserves*: the protocol boundary (B7/B8) is already identical
  (WHATWG decode produces the same single U+FFFD, and the post-decode UTF-16 length check counts 1 unit on both
  sides, so a surrogate byte sequence can never diverge in the bounds check). The only divergent surfaces are the
  MUTF-8/SNBT read paths (B1/B3/B4/B5), classified `diverged` in `rivet-parity`: Java preserves the lone surrogate,
  Rivet errors. No byte stream a Rivet `String` can legitimately produce is ever re-encoded differently from Java;
  the error is deterministic and names the code point (SNBT) or is the generic decoder error (MUTF-8), so it is
  greppable and reproducible, never a silent alteration.
- **Ergonomics.** Keeping `String` canonical means `StringTag.value`, `CompoundTag` keys, `NbtOps`, `Component`,
  `Identifier`, and every codec surface keep Java's own `String` type with no boundary wrapper to unwrap or convert
  at tag-visitor/DFU/Component boundaries. Module mirroring and names stay greppable per PORTING.md. A hostile input
  is rejected once, at the decoder, where the message can name it — not at a downstream consumer that lacks context.
- **Allocation.** `decode_modified_utf8` already mirrors Java's `char[]` → `String` with a transient `Vec<u16>`; the
  unpaired-surrogate error short-circuits before the final `String` is built. The rejected `Vec<u16>` boundary type
  would not save the read-side conversion — it would *move* it to the write/consumer side, costing a copy-and-convert
  on every tag value and compound key that reaches a `&str` consumer (`Identifier`, `Component`, DFU ops), paid for a
  case that cannot legitimately occur.
- **Security.** Erroring — never lossy-replacing — at the MUTF-8/SNBT read boundaries means a lone surrogate can
  never enter internal `String` state, where a later re-encode would silently corrupt byte identity or confuse a
  downstream check. At the protocol boundary, replacing with U+FFFD matches Java exactly, and the WHATWG decode
  never panics on a surrogate byte sequence — it always yields a `String` (a single U+FFFD), so a lone-surrogate
  payload is confined to the same `maxLength` accounting as any other decoded string. serde_json's rejection of
  `"\ud800"` (B9) fails hostile chat JSON at parse time instead of carrying a lone surrogate into a `Component` that
  Gson would later flatten to `?` — a deterministic error instead of silent alteration.
- **Migration.** Because `String` stays canonical, nothing is migrated: every consumer already holds valid UTF-8.
  The single extension point for a future byte-identity need is a `SurrogatePolicy` enum parameter on
  `decode_modified_utf8` (noted above), added only when a concrete wave demonstrates the need. No compatibility
  layer is preserved; the `diverged` classification in `rivet-parity` is the durable record.

## 5. Options compared (why not the alternatives)

| Option | Effect | Rejected because |
|---|---|---|
| **A. `String` + error (chosen)** | Hostile MUTF-8/SNBT input errors; protocol matches Java; JSON divergence documented | — |
| **B. `Vec<u16>` boundary type** | Full wire parity on hostile NBT | Touches every tag/visitor/DFU/Component/Identifier consumer; serde_json still can't round-trip; violates "no speculative broad refactor" |
| **C. `String` + U+FFFD replacement everywhere** | Reads never error | Silent data loss; MUTF-8 re-encode (`EF BF BD` ≠ `ED A0 80`) breaks byte-identity on hostile fixtures; protocol already replaces, NBT should not |
| **D. Swap to a surrogate-parseable JSON lib** | Gson-faithful chat JSON | New heavyweight dep for one hostile-input edge; workspace already standardized on serde_json (CRATES.md) |

## 6. Executable oracle/fixture probes

**Already built and run:** `spikes/surrogate-probe/` — standalone (own `[workspace]`), prints JSON Lines:
- `run-java.sh`: JDK 25 + netty 4.2.15 + Gson 2.14.0 (Paper's pinned versions) ground-truth — the table in §2.
- `cargo run` (Rust counter-probe): `decode_modified_utf8`, `write_utf_body`, WHATWG decode, serde_json.
- `run.sh`: runs both halves back-to-back (Java JSON Lines, then Rust JSON Lines) for manual comparison.
Run: `spikes/surrogate-probe/run.sh`. (Requires netty 4.2.15/Gson 2.14.0 in `~/.gradle`; overridable via `NETTY_BUF`/`NETTY_COMMON`/`GSON`.)

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
5. **Stage 4 — FFI string marshalling policy (#264-d, future M3+).** `rivet-ffi` exists today only as an empty stub
   crate. When its marshalling layer lands, marshal Java `String` to `char*` with `getBytes(UTF_8)` (lone surrogate
   → `?`, byte-for-byte what netty does) OR reject with an ABI error status; document one, chosen when the adapter
   wave lands. Not implemented now.

No crate-cycle impact: all changes are additive (tests/fixtures/docs) or within existing leaves. `rivet-util`,
`rivet-nbt`, `rivet-protocol`, `rivet-serialization` dependency directions are untouched.

## 8. Issue decomposition (sub-issues of #264)

- **#264-a — Docs:** DECISIONS.md D14 + PORTING.md row + PARITY.md divergence note. Small, standalone, no deps.
- **#264-b — Oracle:** reference-oracle lone-surrogate corpus + rivet-parity `diverged` wiring. Depends on M0 Paper
  jar materialization (gate prereq).
- **#264-c — JSON counterfactual test:** serde_json-vs-Gson lone-surrogate lock-in `rivet-text`/`rivet-serialization`.
- **#264-d — FFI policy (future):** string marshalling choice at the JVM adapter boundary (the `rivet-ffi` crate
  exists as an empty stub; the choice is made when its marshalling layer is added). Blocked until M3/M4.

Nothing here is blocked on un-landed waves; #264-a and #264-c are runnable immediately.
