//! The fuzz-target bodies, factored out of `fuzz_targets/*.rs` so the
//! deterministic seed regressions (`regress`) drive the exact code libFuzzer
//! runs. Each function here mirrors the corresponding bin; the multi-path
//! targets (`nbt_binary`, `nbt_binary_visitor`, `codec_compressed_decode`)
//! expose one step per guarded block so a faithful panic on one path does not
//! skip the others — in the fuzzer each step runs under its own
//! `common::guarded`, and in the regressions each step is classified
//! independently.

use std::io::Cursor;
use std::sync::Arc;

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io::{
    parse, parse_compressed, read, read_any_tag, read_compressed, read_unnamed_tag,
    write_unnamed_tag_with_fallback,
};
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_nbt::tag_parser::TagParser;
use rivet_nbt::tag_type::TagType;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_serialization::{Codec, Dynamic};
use rivet_util::data_io::{
    DataInputStream, DataOutputStream, decode_modified_utf8, encoded_len, write_utf_body,
};
use serde_json::{Deserializer, Value};

// ---------------------------------------------------------------------------
// SNBT (`snbt`, `snbt_roundtrip`)
// ---------------------------------------------------------------------------

/// `TagParser.parse_fully` / `parse_as_argument` / `parse_compound_fully` /
/// `parse_compound_as_argument` over lossy-UTF-8 input. Error paths
/// (`NbtFormatException`) are expected and must not panic.
pub fn snbt(data: &[u8]) {
    let input = String::from_utf8_lossy(data);
    let parser = TagParser::create(NbtOps::instance());
    // `parse_fully` (reject trailing data) and `parse_as_argument` (leave
    // trailing input unconsumed) are the two Java entry points.
    let _ = parser.parse_fully(&input);
    let _ = parser.parse_as_argument(&input);
    let _ = rivet_nbt::tag_parser::parse_compound_fully(&input);
    let _ = rivet_nbt::tag_parser::parse_compound_as_argument(&input);
}

/// SNBT parse → print → re-parse round-trip: `parse(print(tag)) == tag` for
/// every successfully parsed input, catching printer bugs that emit invalid
/// SNBT and parser bugs that reject their own output.
pub fn snbt_roundtrip(data: &[u8]) {
    let input = String::from_utf8_lossy(data);
    let parser = TagParser::create(NbtOps::instance());
    if let Ok(tag) = parser.parse_fully(&input) {
        let printed = StringTagVisitor::to_string(&tag);
        // Re-parse the printer's output — it must parse and be identical.
        let reparsed = parser
            .parse_fully(&printed)
            .expect("printed SNBT must re-parse");
        assert_eq!(reparsed, tag, "round-trip mismatch for {input:?}");
    }
}

// ---------------------------------------------------------------------------
// Binary NBT (`nbt_binary`)
// ---------------------------------------------------------------------------

/// Number of independent read paths in the `nbt_binary` target, one per
/// guarded block.
pub const NBT_BINARY_STEPS: usize = 4;

/// One step of `nbt_binary`: `read_any_tag` (0), `read_unnamed_tag` (1),
/// `read` (2), `read_compressed` (3). The read path panics on inputs that crash
/// Java's parse (missing list element type, negative list length, oversized
/// array, `NbtAccounter` quota/depth) — all faithful, swallowed by the caller's
/// guard. The accounter is bounded to the server's default 2 MiB quota so a
/// hostile input cannot force a huge allocation before the quota panic fires.
pub fn nbt_binary_step(data: &[u8], step: usize) {
    match step {
        0 => {
            let mut dis = DataInputStream::new(Cursor::new(data));
            let mut acc = NbtAccounter::default_quota();
            let _ = read_any_tag(&mut dis, &mut acc);
        }
        1 => {
            let mut dis = DataInputStream::new(Cursor::new(data));
            let mut acc = NbtAccounter::default_quota();
            let _ = read_unnamed_tag(&mut dis, &mut acc);
        }
        2 => {
            let mut dis = DataInputStream::new(Cursor::new(data));
            let mut acc = NbtAccounter::default_quota();
            let _ = read(&mut dis, &mut acc);
        }
        3 => {
            let mut acc = NbtAccounter::default_quota();
            let _ = read_compressed(Cursor::new(data), &mut acc);
        }
        _ => unreachable!("step {step} out of range"),
    }
}

// ---------------------------------------------------------------------------
// Streaming visitor (`nbt_binary_visitor`)
// ---------------------------------------------------------------------------

/// Number of independent parse paths in `nbt_binary_visitor`.
pub const NBT_BINARY_VISITOR_STEPS: usize = 2;

/// A visitor that always accepts, so `parse` walks the whole tree.
#[derive(Debug, Default)]
struct AcceptAll;

impl StreamTagVisitor for AcceptAll {
    fn visit_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_string(&mut self, _value: &str) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_byte(&mut self, _value: i8) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_short(&mut self, _value: i16) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_int(&mut self, _value: i32) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_long(&mut self, _value: i64) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_float(&mut self, _value: f32) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_double(&mut self, _value: f64) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_byte_array(&mut self, _value: &[i8]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_int_array(&mut self, _value: &[i32]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_long_array(&mut self, _value: &[i64]) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_list(&mut self, _element_type: TagType, _size: usize) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_entry(&mut self, _ty: TagType) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_entry_named(&mut self, _ty: TagType, _id: &str) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_element(&mut self, _ty: TagType, _index: usize) -> EntryResult {
        EntryResult::Enter
    }
    fn visit_container_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }
    fn visit_root_entry(&mut self, _ty: TagType) -> ValueResult {
        ValueResult::Continue
    }
}

/// One step of `nbt_binary_visitor`: `parse` (0) and `parse_compressed` (1)
/// with the always-`Continue` visitor, forcing full tree traversal through the
/// stream-visitor dispatch. Faithful read-path panics are swallowed by the
/// caller's guard; the accounter is bounded to the server's 2 MiB default quota.
pub fn nbt_binary_visitor_step(data: &[u8], step: usize) {
    match step {
        0 => {
            let mut dis = DataInputStream::new(Cursor::new(data));
            let mut visitor = AcceptAll;
            let mut acc = NbtAccounter::default_quota();
            let _ = parse(&mut dis, &mut visitor, &mut acc);
        }
        1 => {
            let mut visitor = AcceptAll;
            let mut acc = NbtAccounter::default_quota();
            let _ = parse_compressed(Cursor::new(data), &mut visitor, &mut acc);
        }
        _ => unreachable!("step {step} out of range"),
    }
}

// ---------------------------------------------------------------------------
// Write-path canonicalization (`nbt_binary_roundtrip`)
// ---------------------------------------------------------------------------

/// Binary NBT write-path canonicalization idempotence, routed through
/// `NbtIo`'s `StringFallbackDataOutput` — the same fallback `NbtIo.write`
/// uses for server-disk writes.
///
/// Reads a tag, writes it via `write_unnamed_tag_with_fallback`, re-reads,
/// and writes again; the two writes must be byte-identical. Java justifies
/// this as an invariant of `NbtIo.writeUnnamedTag`: the only non-canonical
/// values a parse can produce are non-canonical NaN payloads (re-canonicalized
/// by `writeFloat`/`writeDouble`) and overlong modified-UTF-8 encodings
/// (decoded and re-encoded in canonical form). A hostile input that violates
/// this is a writer or encoder bug, and fails the assertion.
///
/// `StringFallbackDataOutput.write_utf` writes `""` (instead of failing) when
/// a string's canonical re-encoding exceeds 65535 bytes, matching Java's catch
/// of `UTFDataFormatException` in `NbtIo.writeUnnamedTag`. A seed like
/// `too_long_write` therefore exercises that fallback and still round-trips
/// (the overlong string is canonicalized to `""` and re-writes identically).
/// Because the fallback absorbs the only fallible write path and the buffer is
/// in-memory, a write failure here is a bug.
///
/// Faithful parse panics (negative list length, missing list element type,
/// oversized array, accounter quota/depth) are swallowed by the caller's
/// guard; anything else aborts the fuzzer.
pub fn nbt_binary_roundtrip(data: &[u8]) {
    let mut dis = DataInputStream::new(Cursor::new(data));
    let mut acc = NbtAccounter::default_quota();
    let Ok(tag) = read_unnamed_tag(&mut dis, &mut acc) else {
        return;
    };

    // First write through the fallback wrapper — canonicalizes NaN payloads
    // and overlong MUTF-8 (writing `""` for a >65535-byte re-encoding).
    let mut first = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut first);
        write_unnamed_tag_with_fallback(&tag, &mut out)
            .expect("write to an in-memory buffer cannot fail");
    }

    // Re-read the canonical form (well-formed, since the first write
    // succeeded) and write again.
    let mut dis2 = DataInputStream::new(Cursor::new(&first[..]));
    let mut acc2 = NbtAccounter::default_quota();
    let Ok(reparsed) = read_unnamed_tag(&mut dis2, &mut acc2) else {
        panic!("canonical form written by write_unnamed_tag_with_fallback must re-parse");
    };
    let mut second = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut second);
        write_unnamed_tag_with_fallback(&reparsed, &mut out)
            .expect("write to an in-memory buffer cannot fail");
    }

    assert_eq!(
        first, second,
        "write_unnamed_tag_with_fallback must be canonicalization idempotent (Java NbtIo.writeUnnamedTag)"
    );
}

// ---------------------------------------------------------------------------
// Modified UTF-8 (`data_io_modified_utf8`)
// ---------------------------------------------------------------------------

/// Fuzzer-input cap: the decoder's scratch `Vec` is one `u16` per byte (worst
/// case), so a pathological `-max_len` stays small.
const MUTF8_MAX_INPUT_LEN: usize = 1 << 16;

/// The modified-UTF-8 wire codec canonicalization idempotence:
/// `decode(encode(decode(x))) == decode(x)`. Java justifies this as
/// `readUTF(writeUTF(s)) == s` for the encoder's image — `writeUTF` normalizes
/// raw-NUL / overlong forms, so this is *not* `encode(decode(x)) == x`. The
/// encoder can *legitimately* fail when a decoded string's canonical
/// re-encoding exceeds 65535 bytes (a long raw-NUL run re-encodes 2x) — that
/// faithful `UTFDataFormatException` is an `Err` from `write_utf_body`, not a
/// bug. No panic here is faithful, so any panic is a bug.
pub fn data_io_modified_utf8(data: &[u8]) {
    if data.len() > MUTF8_MAX_INPUT_LEN {
        return;
    }
    let Ok(decoded) = decode_modified_utf8(data) else {
        return;
    };
    let Ok(reencoded) = write_utf_body(&decoded) else {
        // Faithful `UTFDataFormatException`: the canonical re-encoding exceeds
        // 65535 bytes. Not a bug.
        return;
    };
    let roundtripped =
        decode_modified_utf8(&reencoded).expect("the encoder's canonical image must decode");
    assert_eq!(
        roundtripped, decoded,
        "modified-UTF-8 canonicalization not idempotent"
    );
    // The encoder's length pre-check must agree with the encoded body.
    assert_eq!(encoded_len(&decoded), reencoded.len());
}

// ---------------------------------------------------------------------------
// DFU codec decode over NbtOps (`codec_decode`)
// ---------------------------------------------------------------------------

/// A mixed-type record decoded by `RecordCodecBuilder` in the codec batteries.
#[derive(Debug, Clone, PartialEq)]
struct Record {
    id: i32,
    name: String,
    flag: Option<bool>,
}

/// DFU codec combinators decoding a `Tag` over `NbtOps` (`compress_maps()`
/// defaults to `false`, so this never reaches the packed-list decode).
fn nbt_ops_decode_battery(ops: &NbtOps, tag: &rivet_nbt::tag::Tag) {
    let int_codec: Arc<dyn Codec<i32, NbtOps>> = codec::int_codec();
    let _ = int_codec.decode(ops, tag);
    let str_codec: Arc<dyn Codec<String, NbtOps>> = codec::string_codec();
    let _ = str_codec.decode(ops, tag);
    let bool_codec: Arc<dyn Codec<bool, NbtOps>> = codec::bool_codec();
    let _ = bool_codec.decode(ops, tag);
    let byte_codec: Arc<dyn Codec<i8, NbtOps>> = codec::byte_codec();
    let _ = byte_codec.decode(ops, tag);
    let long_codec: Arc<dyn Codec<i64, NbtOps>> = codec::long_codec();
    let _ = long_codec.decode(ops, tag);
    let double_codec: Arc<dyn Codec<f64, NbtOps>> = codec::double_codec();
    let _ = double_codec.decode(ops, tag);

    let list_codec = codec::list(int_codec.clone());
    let _ = list_codec.decode(ops, tag);
    let pair_codec = codec::pair(int_codec.clone(), str_codec.clone());
    let _ = pair_codec.decode(ops, tag);
    let either_codec = codec::either(int_codec.clone(), str_codec.clone());
    let _ = either_codec.decode(ops, tag);
    let map_codec = codec::unbounded_map(str_codec.clone(), int_codec.clone());
    let _ = map_codec.decode(ops, tag);
    let compound_list = codec::compound_list(str_codec.clone(), int_codec.clone());
    let _ = compound_list.decode(ops, tag);

    let id_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.id),
        codec::field_of(int_codec.clone(), "id".to_string()),
    );
    let name_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.name.clone()),
        codec::field_of(str_codec.clone(), "name".to_string()),
    );
    let flag_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.flag),
        codec::optional_field("flag".to_string(), bool_codec.clone(), false),
    );
    let record_codec = record_builder::create::<Record, NbtOps>(move |instance| {
        instance
            .group(id_field)
            .and(name_field)
            .and(flag_field)
            .apply(
                instance,
                Arc::new(|id, name, flag| Record { id, name, flag }),
            )
    });
    let _ = record_codec.decode(ops, tag);

    let passthrough: Arc<dyn Codec<Dynamic<rivet_nbt::tag::Tag>, NbtOps>> = codec::passthrough();
    let _ = passthrough.decode(ops, tag);
}

/// DFU codec combinators decoding an SNBT-parsed `Tag` over `NbtOps`. Input
/// bytes are parsed as SNBT (when valid) to obtain a `Tag`, then fed through
/// the battery. `parse_fully` rejects trailing input; on failure the target
/// falls back to `parse_as_argument` (which leaves trailing input unconsumed)
/// so a trailing-garbage seed like `{} xyz` still reaches decode with the
/// leading value. Error paths surface as `DataResult::error`, so any panic
/// here is a real bug.
pub fn codec_decode(data: &[u8]) {
    let ops = NbtOps::instance();
    let input = String::from_utf8_lossy(data);
    let parser = TagParser::create(ops);
    let tag = match parser.parse_fully(&input) {
        Ok(t) => t,
        Err(_) => match parser.parse_as_argument(&input) {
            Ok(t) => t,
            Err(_) => ops.empty(),
        },
    };
    nbt_ops_decode_battery(&ops, &tag);
}

// ---------------------------------------------------------------------------
// DFU compressed-map decode (`codec_compressed_decode`)
// ---------------------------------------------------------------------------

/// Fuzzer-input cap: JSON nesting and list length are already bounded by
/// serde_json's parser and the codec limits; this caps the only
/// input-proportional allocations (the parsed `Value` tree).
const COMPRESSED_MAX_INPUT_LEN: usize = 1 << 20;

/// Number of decode steps in `codec_compressed_decode`: the compressed ops
/// (0) and the object ops (1).
pub const CODEC_COMPRESSED_STEPS: usize = 2;

/// The `Record` codec used by the compressed decode battery.
fn compressed_record_codec<O: DynamicOps + 'static>() -> Arc<dyn Codec<Record, O>> {
    let int_codec: Arc<dyn Codec<i32, O>> = codec::int_codec();
    let str_codec: Arc<dyn Codec<String, O>> = codec::string_codec();
    let bool_codec: Arc<dyn Codec<bool, O>> = codec::bool_codec();

    let id_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.id),
        codec::field_of(int_codec, "id".to_string()),
    );
    let name_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.name.clone()),
        codec::field_of(str_codec, "name".to_string()),
    );
    let flag_field = RecordCodecBuilder::of(
        Arc::new(|r: &Record| r.flag),
        codec::optional_field("flag".to_string(), bool_codec, false),
    );
    record_builder::create::<Record, O>(move |instance| {
        instance
            .group(id_field)
            .and(name_field)
            .and(flag_field)
            .apply(
                instance,
                Arc::new(|id, name, flag| Record { id, name, flag }),
            )
    })
}

fn compressed_decode_battery<O: DynamicOps + 'static>(ops: &O, value: &O::Output) {
    let int_codec: Arc<dyn Codec<i32, O>> = codec::int_codec();
    let str_codec: Arc<dyn Codec<String, O>> = codec::string_codec();
    let bool_codec: Arc<dyn Codec<bool, O>> = codec::bool_codec();
    let _ = int_codec.decode(ops, value);
    let _ = str_codec.decode(ops, value);
    let _ = bool_codec.decode(ops, value);
    let _ = codec::byte_codec::<O>().decode(ops, value);
    let _ = codec::long_codec::<O>().decode(ops, value);
    let _ = codec::double_codec::<O>().decode(ops, value);

    let list_codec = codec::list(int_codec.clone());
    let _ = list_codec.decode(ops, value);
    let pair_codec = codec::pair(int_codec.clone(), str_codec.clone());
    let _ = pair_codec.decode(ops, value);
    let either_codec = codec::either(int_codec.clone(), str_codec.clone());
    let _ = either_codec.decode(ops, value);
    let map_codec = codec::unbounded_map(str_codec.clone(), int_codec.clone());
    let _ = map_codec.decode(ops, value);
    let compound_list = codec::compound_list(str_codec.clone(), int_codec.clone());
    let _ = compound_list.decode(ops, value);

    let record = compressed_record_codec::<O>();
    let _ = record.decode(ops, value);

    let passthrough: Arc<dyn Codec<Dynamic<O::Output>, O>> = codec::passthrough();
    let _ = passthrough.decode(ops, value);
}

/// The first top-level JSON value in `data` (tolerating trailing bytes, so a
/// document like `{"id":1} trailing` still feeds the codec battery), or `None`
/// when the input is empty, exceeds the cap, or does not start with a value.
/// `from_slice` would reject the whole input instead.
pub fn compressed_decode_input(data: &[u8]) -> Option<Value> {
    if data.len() > COMPRESSED_MAX_INPUT_LEN {
        return None;
    }
    let mut stream = Deserializer::from_slice(data).into_iter::<Value>();
    stream.next()?.ok()
}

/// One step of `codec_compressed_decode`: the codec battery over
/// `JsonOps::COMPRESSED` (0, the packed-list path through the
/// `KeyCompressor`-backed `CompressedMapLike`) or `JsonOps::INSTANCE` (1, the
/// object path). An out-of-range compressed-map index is a *faithful* Java
/// crash (`IndexOutOfBoundsException`), so the caller guards this step; every
/// other panic is a genuine bug.
pub fn codec_compressed_decode_step(data: &[u8], step: usize) {
    let Some(value) = compressed_decode_input(data) else {
        return;
    };
    match step {
        0 => compressed_decode_battery(&JsonOps::COMPRESSED, &value),
        1 => compressed_decode_battery(&JsonOps::INSTANCE, &value),
        _ => unreachable!("step {step} out of range"),
    }
}

// ---------------------------------------------------------------------------
// Regression dispatch
// ---------------------------------------------------------------------------

/// Every target with a committed seed set under `fuzz/seeds/`.
pub const SEEDED_TARGETS: &[&str] = &[
    "snbt",
    "snbt_roundtrip",
    "nbt_binary",
    "nbt_binary_visitor",
    "nbt_binary_roundtrip",
    "data_io_modified_utf8",
    "codec_decode",
    "codec_compressed_decode",
];

/// The number of independently-guarded steps for `target`.
pub fn step_count(target: &str) -> usize {
    match target {
        "nbt_binary" => NBT_BINARY_STEPS,
        "nbt_binary_visitor" => NBT_BINARY_VISITOR_STEPS,
        "codec_compressed_decode" => CODEC_COMPRESSED_STEPS,
        "snbt"
        | "snbt_roundtrip"
        | "nbt_binary_roundtrip"
        | "data_io_modified_utf8"
        | "codec_decode" => 1,
        _ => panic!("unknown target {target}"),
    }
}

/// Run `step` of `target` on `data` — the shared entry point the deterministic
/// seed regressions call for every seed.
pub fn run_step(target: &str, data: &[u8], step: usize) {
    match target {
        "snbt" => snbt(data),
        "snbt_roundtrip" => snbt_roundtrip(data),
        "nbt_binary" => nbt_binary_step(data, step),
        "nbt_binary_visitor" => nbt_binary_visitor_step(data, step),
        "nbt_binary_roundtrip" => nbt_binary_roundtrip(data),
        "data_io_modified_utf8" => data_io_modified_utf8(data),
        "codec_decode" => codec_decode(data),
        "codec_compressed_decode" => codec_compressed_decode_step(data, step),
        _ => panic!("unknown target {target}"),
    }
}
