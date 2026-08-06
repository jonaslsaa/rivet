//! Java-grounded tests for the NbtOps byte-array/byte-buffer/int-stream/
//! long-stream ops surface (`NbtOps.java:275-306` + the `DynamicOps` defaults
//! in DFU 10.0.21 `DynamicOps.java:147-191`).
//!
//! Grounding:
//! - `NbtOps.getByteBuffer` fast-paths a `ByteArrayTag` (`ByteBuffer.wrap`),
//!   else `DynamicOps.super.getByteBuffer` — `getStream(input).flatMap(...)`,
//!   all elements must pass `getNumberValue` (`Number.byteValue()` narrowing),
//!   otherwise "Some elements are not bytes: <input>".
//! - `NbtOps.createByteList` copies the whole buffer: `duplicate().clear()`
//!   then `get(0, bytes, 0, capacity)` — the port takes `&[u8]`, so the copy
//!   is the full slice; there is no zero-copy path.
//! - `getIntStream`/`getLongStream` mirror the same shape with
//!   `Number.intValue()`/`longValue()`; non-array inputs fall through to the
//!   defaults over `getStream` (a non-list keeps the "Not a list" error).
//! - `createByteList`/`createIntList`/`createLongList` produce
//!   `ByteArrayTag`/`IntArrayTag`/`LongArrayTag`; DFU's generic `create*List`
//!   default (a `ListTag` of `ByteTag`/`IntTag`/`LongTag`) is what NbtOps
//!   uses when the default overrides call `createList`.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::nbt_ops::NbtOps;
use crate::string_tag::StringTag;
use crate::tag::Tag;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;

fn ops() -> NbtOps {
    NbtOps::instance()
}

fn byte_array(bytes: &[i8]) -> Tag {
    Tag::ByteArray(ByteArrayTag::new(bytes.to_vec()))
}

fn byte_list(bytes: &[i8]) -> Tag {
    Tag::List(ListTag::with_list(
        bytes
            .iter()
            .map(|b| Tag::Byte(ByteTag::value_of(*b)))
            .collect(),
    ))
}

/// A `DataResult` error message with no partial.
fn error_message<T>(result: &rivet_serialization::DataResult<T>) -> String {
    result
        .error_ref()
        .expect("expected an error")
        .message()
        .to_string()
}

// ---------------------------------------------------------------------------
// getByteBuffer / createByteList
// ---------------------------------------------------------------------------

/// `getByteBuffer` of a `ByteArrayTag` fast-paths to its raw bytes (signed
/// bytes are the source of truth; the buffer is the two's-complement view).
#[test]
fn get_byte_buffer_fast_paths_byte_array() {
    let o = ops();
    assert_eq!(
        o.get_byte_buffer(&byte_array(&[0, 1, 127, -128, -1]))
            .result(),
        Some(&vec![0u8, 1u8, 127u8, 128u8, 255u8])
    );
}

/// `getByteBuffer` of a `ListTag` of numeric bytes falls back to
/// `DynamicOps.super.getByteBuffer` — every element narrows via
/// `Number.byteValue()` (`DoubleTag 300 -> (byte)44`).
#[test]
fn get_byte_buffer_falls_back_over_list_of_numbers() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Byte(ByteTag::value_of(1)),
        Tag::Int(IntTag::value_of(300)),
        Tag::Double(DoubleTag::value_of(256.0)),
    ]));
    assert_eq!(
        o.get_byte_buffer(&list).result(),
        Some(&vec![1u8, 44u8, 0u8]),
        "elements narrow via Number.byteValue()"
    );
}

/// A list with a non-number element fails with the exact DFU diagnostic.
#[test]
fn get_byte_buffer_non_number_element_fails() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Byte(ByteTag::value_of(1)),
        Tag::String(StringTag::value_of("x".to_string())),
    ]));
    let err = error_message(&o.get_byte_buffer(&list));
    // `Tag.toString()` (StringTagVisitor) renders a ByteTag as `1b` and a
    // StringTag as `"x"`.
    assert_eq!(err, "Some elements are not bytes: [1b,\"x\"]");
}

/// A non-list (non-array) input keeps the `getStream` "Not a list" error.
#[test]
fn get_byte_buffer_non_list_keeps_not_a_list() {
    let o = ops();
    assert_eq!(
        error_message(&o.get_byte_buffer(&Tag::Int(IntTag::new(1)))),
        "Not a list"
    );
}

/// `createByteList` copies the buffer into a `ByteArrayTag` (Java
/// `duplicate().clear(); get(0, bytes, 0, capacity)`); the unsigned input
/// bytes map to signed bytes.
#[test]
fn create_byte_list_copies_into_byte_array_tag() {
    let o = ops();
    let tag = o.create_byte_list(&[0u8, 127u8, 128u8, 255u8]);
    assert_eq!(tag, byte_array(&[0, 127, -128, -1]));
}

/// `Codec.BYTE_BUFFER` round-trips through NbtOps (encode → `ByteArrayTag`,
/// decode → `getByteBuffer`).
#[test]
fn byte_buffer_codec_round_trips_through_nbt() {
    let o = ops();
    let byte_buffer = codec::byte_buffer_codec::<NbtOps>();
    let value = vec![0u8, 1u8, 127u8, 128u8, 255u8];
    let encoded = byte_buffer
        .encode_start(&o, &value)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, byte_array(&[0, 1, 127, -128, -1]));
    let decoded = byte_buffer
        .parse(&o, &encoded)
        .get_or_throw("parse")
        .clone();
    assert_eq!(decoded, value);
}

// ---------------------------------------------------------------------------
// getIntStream / createIntList
// ---------------------------------------------------------------------------

/// `getIntStream` of an `IntArrayTag` fast-paths to its raw ints.
#[test]
fn get_int_stream_fast_paths_int_array() {
    let o = ops();
    let tag = Tag::IntArray(IntArrayTag::new(vec![i32::MIN, -1, 0, 1, i32::MAX]));
    assert_eq!(
        o.get_int_stream(&tag).result(),
        Some(&vec![i32::MIN, -1, 0, 1, i32::MAX])
    );
}

/// `getIntStream` of a `ListTag` of numbers falls back to the default with
/// `Number.intValue()` narrowing.
#[test]
fn get_int_stream_falls_back_over_list_of_numbers() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Long(LongTag::value_of(70_000)),
        Tag::Int(IntTag::value_of(-1)),
    ]));
    assert_eq!(
        o.get_int_stream(&list).result(),
        Some(&vec![70_000i32, -1i32])
    );
}

/// `getIntStream` errors when an element is not a number.
#[test]
fn get_int_stream_non_number_element_fails() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Int(IntTag::value_of(1)),
        Tag::String(StringTag::value_of("x".to_string())),
    ]));
    assert_eq!(
        error_message(&o.get_int_stream(&list)),
        "Some elements are not ints: [1,\"x\"]"
    );
}

/// `createIntList` builds an `IntArrayTag`.
#[test]
fn create_int_list_builds_int_array_tag() {
    let o = ops();
    assert_eq!(
        o.create_int_list(vec![i32::MIN, 0, i32::MAX]),
        Tag::IntArray(IntArrayTag::new(vec![i32::MIN, 0, i32::MAX]))
    );
}

/// `Codec.INT_STREAM` round-trips (i32 boundaries).
#[test]
fn int_stream_codec_round_trips_through_nbt() {
    let o = ops();
    let int_stream = codec::int_stream_codec::<NbtOps>();
    let value = vec![i32::MIN, -1, 0, 1, i32::MAX];
    let encoded = int_stream
        .encode_start(&o, &value)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, Tag::IntArray(IntArrayTag::new(value.clone())));
    let decoded = int_stream.parse(&o, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, value);
}

// ---------------------------------------------------------------------------
// getLongStream / createLongList
// ---------------------------------------------------------------------------

/// `getLongStream` of a `LongArrayTag` fast-paths to its raw longs (i64
/// boundaries preserved).
#[test]
fn get_long_stream_fast_paths_long_array() {
    let o = ops();
    let tag = Tag::LongArray(LongArrayTag::new(vec![i64::MIN, -1, 0, 1, i64::MAX]));
    assert_eq!(
        o.get_long_stream(&tag).result(),
        Some(&vec![i64::MIN, -1, 0, 1, i64::MAX])
    );
}

/// `getLongStream` of a `ListTag` of numbers falls back with `longValue()`.
#[test]
fn get_long_stream_falls_back_over_list_of_numbers() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Int(IntTag::value_of(42)),
        Tag::Long(LongTag::value_of(i64::MAX)),
    ]));
    assert_eq!(
        o.get_long_stream(&list).result(),
        Some(&vec![42i64, i64::MAX])
    );
}

/// `getLongStream` errors when an element is not a number.
#[test]
fn get_long_stream_non_number_element_fails() {
    let o = ops();
    let list = Tag::List(ListTag::with_list(vec![
        Tag::Long(LongTag::value_of(1)),
        Tag::String(StringTag::value_of("x".to_string())),
    ]));
    assert_eq!(
        error_message(&o.get_long_stream(&list)),
        "Some elements are not longs: [1L,\"x\"]"
    );
}

/// `createLongList` builds a `LongArrayTag`.
#[test]
fn create_long_list_builds_long_array_tag() {
    let o = ops();
    assert_eq!(
        o.create_long_list(vec![i64::MIN, 0, i64::MAX]),
        Tag::LongArray(LongArrayTag::new(vec![i64::MIN, 0, i64::MAX]))
    );
}

/// `Codec.LONG_STREAM` round-trips (i64 boundaries).
#[test]
fn long_stream_codec_round_trips_through_nbt() {
    let o = ops();
    let long_stream = codec::long_stream_codec::<NbtOps>();
    let value = vec![i64::MIN, -1, 0, 1, i64::MAX];
    let encoded = long_stream
        .encode_start(&o, &value)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, Tag::LongArray(LongArrayTag::new(value.clone())));
    let decoded = long_stream
        .parse(&o, &encoded)
        .get_or_throw("parse")
        .clone();
    assert_eq!(decoded, value);
}

// ---------------------------------------------------------------------------
// Array/list fallback interop via the stream surface
// ---------------------------------------------------------------------------

/// `convertTo(JsonOps)` of a `ByteArrayTag` produces a JSON array of numbers —
/// the same observable list value as converting a `ListTag` of byte tags.
#[test]
fn convert_byte_array_matches_list_of_bytes() {
    use rivet_serialization::json_ops::JsonOps;
    let o = ops();
    let out = JsonOps::INSTANCE;
    assert_eq!(
        o.convert_to(&out, &byte_array(&[0, 127, -128, -1])),
        out.create_list(vec![
            out.create_byte(0),
            out.create_byte(127),
            out.create_byte(-128),
            out.create_byte(-1),
        ]),
    );
    assert_eq!(
        o.convert_to(&out, &byte_list(&[0, 127, -128, -1])),
        out.create_list(vec![
            out.create_byte(0),
            out.create_byte(127),
            out.create_byte(-128),
            out.create_byte(-1),
        ]),
    );
}

/// A `ByteArrayTag` and a `ListTag` of the same byte tags both read as the same
/// byte buffer — the array/list fallback is symmetric on the read path.
#[test]
fn byte_array_and_byte_list_read_identically() {
    let o = ops();
    let bytes = &[1i8, 2i8, -3i8];
    assert_eq!(
        o.get_byte_buffer(&byte_array(bytes)).result(),
        o.get_byte_buffer(&byte_list(bytes)).result()
    );
}

/// `mergeToList` onto an empty `ByteArrayTag` starts a fresh generic list
/// (Java `createCollector` returns a `GenericListCollector` for an empty
/// collection) — so appending a non-byte element yields a `ListTag`, not an
/// error.
#[test]
fn merge_to_list_onto_empty_byte_array_is_generic() {
    let o = ops();
    let empty = byte_array(&[]);
    let merged = o
        .merge_to_list(&empty, Tag::String(StringTag::value_of("x".to_string())))
        .result()
        .cloned()
        .unwrap();
    assert_eq!(
        merged,
        Tag::List(ListTag::with_list(vec![Tag::String(StringTag::value_of(
            "x".to_string()
        ))]))
    );
}

// ---------------------------------------------------------------------------
// mergeToMap(Tag, MapLike) — the `MapLike` override (`NbtOps.java:157-178`)
// ---------------------------------------------------------------------------

/// `mergeToMap(Tag, MapLike)` with a `MapLike` source over an empty prefix
/// produces `emptyMap()` (the `empty()` -> `emptyMap()` special case).
#[test]
fn merge_to_map_like_empty_prefix_produces_empty_map() {
    use rivet_serialization::dynamic_ops::MapLike;
    let o = ops();
    #[derive(Debug)]
    struct Empty;
    impl MapLike<Tag> for Empty {
        fn get(&self, _key: &Tag) -> Option<Tag> {
            None
        }
        fn get_string(&self, _key: &str) -> Option<Tag> {
            None
        }
        fn entries(&self) -> Vec<rivet_serialization::pair::Pair<Tag, Tag>> {
            Vec::new()
        }
    }
    // An `EndTag` prefix (the ops' `empty()`) with zero entries -> `emptyMap()`.
    let result = o.merge_to_map_like(&o.empty(), &Empty);
    assert_eq!(
        result.result().cloned(),
        Some(Tag::Compound(CompoundTag::new()))
    );
}

/// `mergeToMap(Tag, MapLike)` accumulates string-keyed entries and reports
/// non-string keys with the exact "some keys are not strings" diagnostic.
#[test]
fn merge_to_map_like_accumulates_and_reports_missed_keys() {
    use rivet_serialization::dynamic_ops::MapLike;
    let o = ops();
    #[derive(Debug)]
    struct Entries(Vec<rivet_serialization::pair::Pair<Tag, Tag>>);
    impl MapLike<Tag> for Entries {
        fn get(&self, key: &Tag) -> Option<Tag> {
            self.0
                .iter()
                .find(|p| &p.first == key)
                .map(|p| p.second.clone())
        }
        fn get_string(&self, key: &str) -> Option<Tag> {
            self.0
                .iter()
                .find(|p| matches!(&p.first, Tag::String(s) if s.value == key))
                .map(|p| p.second.clone())
        }
        fn entries(&self) -> Vec<rivet_serialization::pair::Pair<Tag, Tag>> {
            self.0.clone()
        }
    }
    let values = Entries(vec![
        rivet_serialization::pair::Pair::of(
            Tag::String(StringTag::value_of("a".to_string())),
            Tag::Int(IntTag::value_of(1)),
        ),
        rivet_serialization::pair::Pair::of(
            Tag::Int(IntTag::value_of(7)),
            Tag::Int(IntTag::value_of(2)),
        ),
    ]);
    let result = o.merge_to_map_like(&o.empty(), &values);
    assert!(result.is_error());
    assert_eq!(error_message(&result), "some keys are not strings: [7]");
    // The partial keeps the valid string-keyed entry.
    let partial = result.result_or_partial_silent().unwrap();
    let compound = match partial {
        Tag::Compound(c) => c,
        other => panic!("expected a CompoundTag partial, got {other:?}"),
    };
    assert_eq!(compound.get("a"), Some(&Tag::Int(IntTag::value_of(1))));
    assert!(compound.get("7").is_none());
}
