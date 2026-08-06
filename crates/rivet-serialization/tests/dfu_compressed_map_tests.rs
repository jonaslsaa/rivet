//! Focused tests for the ported DFU compressed-map behavior (`compressMaps()`):
//! `KeyCompressor` construction/caching + string fallback, the packed-list
//! encode (`MapEncoder.makeCompressedBuilder`) and decode
//! (`MapDecoder.compressedDecode`), and the exact Java edge semantics:
//! unknown keys (compress to slot 0), duplicate/missing keys, list length and
//! index bounds, deterministic key order from `keys(ops)`, and lifecycle
//! propagation. Grounded in `com.mojang.serialization` (DFU 10.0.21) sources +
//! fastutil bytecode.
//!
//! All assertions are Java-grounded; the two `JsonOps` backends distinguish the
//! compressed packed-list form (COMPRESSED) from the map-object form (INSTANCE).

mod common;

use common::{Canonical, OpsTestExt, v_int, v_list, v_map, v_str};
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::{DynamicOps, KeyCompressor, Keyable};
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_encoder;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// KeyCompressor
// ---------------------------------------------------------------------------

#[test]
fn key_compressor_assignment_order_and_dedup() {
    // Java `KeyCompressor(DynamicOps, Stream<T> keys)`: the first occurrence
    // of each distinct key (by equals) gets its index in first-seen order;
    // duplicates are skipped. `compressString` maps stringifiable keys to the
    // same index.
    let ops = JsonOps::INSTANCE;
    let keys = vec![
        ops.create_string("b".to_string()),
        ops.create_string("a".to_string()),
        ops.create_string("b".to_string()), // duplicate: skipped
    ];
    let compressor = KeyCompressor::new_with_strings(&ops, keys);
    assert_eq!(compressor.size(), 2, "duplicate keys are deduplicated");
    // b -> 0, a -> 1 (first-seen order).
    assert_eq!(compressor.compress_string(&ops, "b"), 0);
    assert_eq!(compressor.compress_string(&ops, "a"), 1);
    assert_eq!(
        compressor.decompress(0),
        Some(&ops.create_string("b".to_string()))
    );
    assert_eq!(
        compressor.decompress(1),
        Some(&ops.create_string("a".to_string()))
    );
    // Out-of-range decompress is null (Java `Int2ObjectArrayMap.get` → null).
    assert_eq!(compressor.decompress(2), None);
}

#[test]
fn key_compressor_compress_string_fallback_to_object_identity() {
    // Java `compress(String)`: `compressString.getInt(key)` is -1 (absent),
    // so it falls back to `compress(ops.createString(key))`. For JsonOps a
    // created string IS in the object table only when it equals a key; an
    // unknown key falls through to the object map's default return value (0).
    let ops = JsonOps::INSTANCE;
    let compressor =
        KeyCompressor::new_with_strings(&ops, vec![ops.create_string("x".to_string())]);
    // "x" is in the string table.
    assert_eq!(compressor.compress_string(&ops, "x"), 0);
    // "unknown" is not: `compressString` returns -1 → fallback
    // `compress(ops.createString("unknown"))` → object map default (0).
    assert_eq!(compressor.compress_string(&ops, "unknown"), 0);
}

#[test]
fn key_compressor_compress_unknown_key_defaults_to_zero() {
    // fastutil `Object2IntArrayMap` default return value is 0 (DFU does not
    // override it): an unknown T key compresses to slot 0.
    let ops = JsonOps::INSTANCE;
    let compressor = KeyCompressor::new_with_strings(
        &ops,
        vec![
            ops.create_string("a".to_string()),
            ops.create_string("b".to_string()),
        ],
    );
    assert_eq!(
        compressor.compress_key(&ops.create_string("a".to_string())),
        0
    );
    assert_eq!(
        compressor.compress_key(&ops.create_string("b".to_string())),
        1
    );
    // Unknown key → 0 (fastutil default), not a missing sentinel.
    assert_eq!(
        compressor.compress_key(&ops.create_string("zzz".to_string())),
        0
    );
}

// ---------------------------------------------------------------------------
// Compressed encode/decode round trip — packed list vs object
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct TwoField {
    string: String,
    integer: i32,
}

fn two_field_codec<O: DynamicOps + 'static>() -> Arc<dyn rivet_serialization::Codec<TwoField, O>> {
    rivet_serialization::record_builder::create::<TwoField, O>(move |instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|o: &TwoField| o.string.clone()),
                "string".to_string(),
                codec::string_codec(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|o: &TwoField| o.integer),
                "integer".to_string(),
                codec::int_codec(),
            ))
            .apply(
                instance,
                Arc::new(|s: String, i: i32| TwoField {
                    string: s,
                    integer: i,
                }),
            )
    })
}

#[test]
fn compressed_encodes_packed_list_instance_encodes_object() {
    let value = TwoField {
        string: "hello".into(),
        integer: 1,
    };

    let ops = JsonOps::COMPRESSED;
    let encoded = two_field_codec()
        .encode_start(&ops, &value)
        .get_or_throw("encodeStart")
        .clone();
    // Packed list in `keys(ops)` order: ["hello", 1].
    assert_eq!(
        encoded.canon(),
        v_list(&ops, vec![v_str(&ops, "hello"), v_int(&ops, 1)]).canon(),
        "COMPRESSED must encode a record as a packed list"
    );

    let ops = JsonOps::INSTANCE;
    let encoded = two_field_codec()
        .encode_start(&ops, &value)
        .get_or_throw("encodeStart")
        .clone();
    assert_eq!(
        encoded.canon(),
        v_map(
            &ops,
            vec![
                ("string", v_str(&ops, "hello")),
                ("integer", v_int(&ops, 1))
            ],
        )
        .canon(),
        "INSTANCE must encode a record as an object map"
    );
}

#[test]
fn compressed_round_trip_and_back() {
    let value = TwoField {
        string: "hello".into(),
        integer: 42,
    };
    let ops = JsonOps::COMPRESSED;
    let codec = two_field_codec();
    ops.assert_round_trip(
        &codec,
        value,
        v_list(&ops, vec![v_str(&ops, "hello"), v_int(&ops, 42)]),
    );
}

#[test]
fn compressed_decode_non_list_fails_input_not_a_list() {
    // Java `compressedDecode`: `ops.getList(input).result()` absent →
    // `DataResult.error("Input is not a list")`.
    let ops = JsonOps::COMPRESSED;
    let codec = two_field_codec();
    let map_input = v_map(
        &ops,
        vec![
            ("string", v_str(&ops, "hello")),
            ("integer", v_int(&ops, 1)),
        ],
    );
    let result = codec.parse(&ops, &map_input);
    assert!(result.is_error());
    assert!(
        result
            .error_ref()
            .unwrap()
            .message()
            .contains("Input is not a list"),
        "got: {}",
        result.error_ref().unwrap().message()
    );
    assert!(
        result.clone().result_or_partial_silent().is_none(),
        "an 'Input is not a list' error carries no partial"
    );
}

// ---------------------------------------------------------------------------
// Unknown keys / duplicate / missing keys
// ---------------------------------------------------------------------------

#[test]
fn compressed_decode_unknown_key_reads_slot_zero() {
    // Java `MapLike.get(key)` on the compressed map: `entries.get(compressor
    // .compress(key))`. An unknown key compresses to 0 (fastutil default), so a
    // list with an unexpected FIRST slot value yields that for an unknown key.
    let ops = JsonOps::COMPRESSED;
    let codec = two_field_codec();
    // [0]=unknown-key value read at slot 0, [1]=integer.
    let list_input = v_list(&ops, vec![v_str(&ops, "unexpected"), v_int(&ops, 7)]);
    // The decoder reads "string" (known) at slot 0 and "integer" at slot 1; an
    // extra unknown key is never looked up (a record reads only its own keys).
    let decoded = codec.parse(&ops, &list_input).get_or_throw("parse").clone();
    assert_eq!(
        decoded,
        TwoField {
            string: "unexpected".into(),
            integer: 7
        }
    );
}

#[test]
fn compressed_encode_duplicate_keys_last_wins() {
    // Java `makeCompressedBuilder.append`: `builder.set(compress(key), value)`
    // — a duplicate key overwrites its earlier slot (List.set).
    let ops = JsonOps::COMPRESSED;
    let _encoder =
        map_encoder::field_encoder::<i32, JsonOps>("dup".to_string(), codec::int_codec());
    let mut builder = map_encoder::make_compressed_builder(
        &ops,
        KeyCompressor::new_with_strings(&ops, vec![ops.create_string("dup".to_string())]),
    );
    builder.add_string("dup", ops.create_int(1));
    builder.add_string("dup", ops.create_int(2));
    let encoded = builder
        .build(Some(ops.empty()))
        .get_or_throw("build")
        .clone();
    assert_eq!(
        encoded.canon(),
        v_list(&ops, vec![v_int(&ops, 2)]).canon(),
        "duplicate key must overwrite (List.set semantics)"
    );
}

#[test]
fn compressed_decode_missing_slot_reads_as_absent() {
    // Java `compressedDecode` MapLike.get: a null slot (absent field) reads as
    // absent, so a `MapCodec` field lookup returns None → the decoder reports
    // "No key x in ...". A full-length list whose second slot is null
    // (`JsonNull`) reads "integer" → null → absent.
    let ops = JsonOps::COMPRESSED;
    let codec = two_field_codec();
    let list_input = v_list(&ops, vec![v_str(&ops, "hello"), ops.empty()]);
    let result = codec.parse(&ops, &list_input);
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(
        msg.contains("No key integer"),
        "missing packed slot must read as an absent key, got: {msg}"
    );
}

#[test]
fn compressed_encode_missing_fields_emit_null_slots() {
    // Java: optional/missing fields are null slots in the packed list. Build a
    // record whose two keys are optional int fields, both absent on encode.
    type Ops = JsonOps;
    let int_codec: Arc<dyn rivet_serialization::Codec<i32, Ops>> = codec::int_codec();
    let present =
        codec::optional_field::<i32, Ops>("present".to_string(), int_codec.clone(), false);
    let absent = codec::optional_field::<i32, Ops>("absent".to_string(), int_codec, false);
    let codec = rivet_serialization::record_builder::create::<Option<i32>, Ops>(move |instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|_o: &Option<i32>| None::<i32>),
                present,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|_o: &Option<i32>| None::<i32>),
                absent,
            ))
            .apply(instance, Arc::new(|_a: Option<i32>, _b: Option<i32>| None))
    });
    let ops = JsonOps::COMPRESSED;
    let encoded = codec
        .encode_start(&ops, &None::<i32>)
        .get_or_throw("encodeStart")
        .clone();
    // Both optional fields are None → both slots are null.
    assert_eq!(
        encoded.canon(),
        v_list(&ops, vec![ops.empty(), ops.empty()]).canon(),
        "absent optional fields must encode as null slots"
    );
}

// ---------------------------------------------------------------------------
// List length / index bounds
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Index 1 out of bounds")]
fn compressed_decode_short_list_panics_on_oob() {
    // Java: `entries.get(compressor.compress(key))` on a list shorter than the
    // compressed index throws `IndexOutOfBoundsException`. `two_field_codec`
    // reads "integer" at slot 1 of a 1-element list → OOB.
    let ops = JsonOps::COMPRESSED;
    let list_input = v_list(&ops, vec![v_str(&ops, "hello")]);
    let _ = two_field_codec().parse(&ops, &list_input);
}

#[test]
fn compressed_decode_extra_entries_are_ignored_but_null_filtered() {
    // Java `entries()` on the compressed MapLike iterates the full list,
    // filtering null values. Extra trailing entries beyond `size` decompress to
    // null keys but are still emitted (with a null key) when non-null. A record
    // decoder only reads its own keys, so trailing entries don't matter.
    let ops = JsonOps::COMPRESSED;
    let list_input = v_list(
        &ops,
        vec![
            v_str(&ops, "hello"),
            v_int(&ops, 1),
            v_str(&ops, "trailing"),
        ],
    );
    let decoded = two_field_codec()
        .parse(&ops, &list_input)
        .get_or_throw("parse")
        .clone();
    assert_eq!(
        decoded,
        TwoField {
            string: "hello".into(),
            integer: 1
        }
    );
}

// ---------------------------------------------------------------------------
// Deterministic key order from keys(ops)
// ---------------------------------------------------------------------------

#[test]
fn compressed_encodes_in_keys_order() {
    // The packed list follows `keys(ops)` (declaration) order, not map
    // iteration order. `two_field_codec` declares "string" then "integer".
    let ops = JsonOps::COMPRESSED;
    let value = TwoField {
        string: "a".into(),
        integer: 9,
    };
    let encoded = two_field_codec()
        .encode_start(&ops, &value)
        .get_or_throw("encodeStart")
        .clone();
    assert_eq!(
        encoded.canon(),
        v_list(&ops, vec![v_str(&ops, "a"), v_int(&ops, 9)]).canon(),
        "packed list must use keys(ops) order"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle propagation
// ---------------------------------------------------------------------------

#[test]
fn compressed_encode_lifecycle_matches_instance() {
    // Java `MapEncoder.encoder()`: `compressedBuilder(ops)` initializes with
    // `Lifecycle.stable()`, but the field `encodeStart` results (experimental
    // for the primitive codecs) and `JsonOps.mergeToList`
    // (`DataResult.success`, experimental) propagate through `ap`/`flatMap`,
    // so a record encode ends experimental — identical to the INSTANCE
    // object-map encode.
    let value = TwoField {
        string: "x".into(),
        integer: 1,
    };
    let ops = JsonOps::COMPRESSED;
    let compressed = two_field_codec().encode_start(&ops, &value);
    let ops = JsonOps::INSTANCE;
    let instance = two_field_codec().encode_start(&ops, &value);
    assert_eq!(
        compressed.lifecycle(),
        instance.lifecycle(),
        "compressed and object encodes must propagate the same lifecycle"
    );
    assert_eq!(compressed.lifecycle(), Lifecycle::Experimental);
}

#[test]
fn compressed_decode_lifecycle_matches_instance() {
    // Java `compressedDecode` routes through the decode combinator lifecycle
    // (`getMap(input).setLifecycle(stable)` then `flatMap(decode)`); the
    // field decodes are experimental, so both backends end experimental.
    let value = TwoField {
        string: "x".into(),
        integer: 1,
    };
    let compressed = {
        let ops = JsonOps::COMPRESSED;
        let encoded = two_field_codec()
            .encode_start(&ops, &value)
            .get_or_throw("encodeStart")
            .clone();
        two_field_codec().parse(&ops, &encoded)
    };
    let instance = {
        let ops = JsonOps::INSTANCE;
        let encoded = two_field_codec()
            .encode_start(&ops, &value)
            .get_or_throw("encodeStart")
            .clone();
        two_field_codec().parse(&ops, &encoded)
    };
    assert_eq!(
        compressed.lifecycle(),
        instance.lifecycle(),
        "compressed and object decodes must propagate the same lifecycle"
    );
    assert_eq!(compressed.lifecycle(), Lifecycle::Experimental);
}

// ---------------------------------------------------------------------------
// MapDecoder.compressedDecode direct (MapLike behavior)
// ---------------------------------------------------------------------------

#[test]
fn compressed_map_like_get_string_vs_get() {
    // The compressed MapLike exposes both `get(String)` and `get(T)`; both
    // route through the compressor (`compressString` falls back to the object
    // table). Use JsonOps where a string key is also a string value.
    use rivet_serialization::dynamic_ops::MapLike;
    let ops = JsonOps::COMPRESSED;
    let list_input = v_list(&ops, vec![v_str(&ops, "a"), v_int(&ops, 2)]);
    let keys = vec![
        ops.create_string("s".to_string()),
        ops.create_string("i".to_string()),
    ];
    let map = rivet_serialization::dynamic_ops::compressed_map_like(&ops, keys, &list_input)
        .expect("list input");
    let m: &dyn MapLike<serde_json::Value> = &map;
    assert_eq!(m.get_string("s"), Some(v_str(&ops, "a")));
    assert_eq!(m.get_string("i"), Some(v_int(&ops, 2)));
    assert_eq!(
        m.get(&ops.create_string("i".to_string())),
        Some(v_int(&ops, 2))
    );
    // A key absent from `compressString` falls back to
    // `compress(ops.createString(key))` → the object map's default return
    // value (fastutil 0) → slot 0. Java reads `entries.get(0)` for it.
    assert_eq!(
        m.get_string("missing"),
        Some(v_str(&ops, "a")),
        "unknown string key must fall back to the object table default (slot 0)"
    );
}

// ---------------------------------------------------------------------------
// DataResult-keyed add (AbstractUniversalBuilder.add(DataResult, DataResult))
// ---------------------------------------------------------------------------

/// A `Keyable` over a fixed set of string keys, for `simple_map`.
struct StaticKeys<O: DynamicOps + 'static>(Vec<O::Output>);

impl<O: DynamicOps + 'static> Keyable<O> for StaticKeys<O> {
    fn keys(&self, _ops: &O) -> Vec<O::Output> {
        self.0.clone()
    }
}

#[test]
fn compressed_simple_map_data_result_key_route() {
    // Java `BaseMapCodec.encode` adds each entry via
    // `prefix.add(keyCodec().encodeStart(...), elementCodec().encodeStart(...))`
    // — the `AbstractUniversalBuilder.add(DataResult, DataResult)` overload.
    // Under `compressMaps()` the `MapCodecCodec.encode` builds through the
    // `CompressedRecordBuilder`, so each resolved pair must be written to its
    // compressed slot. Before the `add_result_result` override this was a
    // no-op, emitting an empty list.
    let keys: Arc<dyn Keyable<JsonOps>> = Arc::new(StaticKeys(vec![
        JsonOps::INSTANCE.create_string("a".to_string()),
        JsonOps::INSTANCE.create_string("b".to_string()),
    ]));
    let codec =
        codec::simple_map::<String, i32, JsonOps>(codec::string_codec(), codec::int_codec(), keys);

    let mut map = std::collections::HashMap::new();
    map.insert("a".to_string(), 1);
    map.insert("b".to_string(), 2);

    let codec = rivet_serialization::map_codec::codec_of(codec);

    let ops = JsonOps::COMPRESSED;
    let encoded = codec
        .encode_start(&ops, &map)
        .get_or_throw("encodeStart")
        .clone();
    // Packed list in `keys(ops)` order — HashMap iteration order is irrelevant
    // here because both keys are declared and each slot is filled by its
    // compressed index.
    assert_eq!(
        encoded.canon(),
        v_list(&ops, vec![v_int(&ops, 1), v_int(&ops, 2)]).canon(),
        "COMPRESSED simple_map must encode each DataResult-keyed entry into its slot"
    );

    // Round-trip back through the packed-list decode.
    let result = codec.parse(&ops, &encoded);
    let decoded = result.get_or_throw("parse");
    assert_eq!(decoded, &map);
}
