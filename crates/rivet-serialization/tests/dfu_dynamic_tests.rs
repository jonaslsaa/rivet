//! DFU-mirroring tests for the ported `Dynamic`/`OptionalDynamic` surface and
//! the `MapCodec` combinators (`assumeMapUnsafe`, `unit`, field codecs).
//!
//! Upstream coverage (`com.mojang.serialization.DynamicTest` and
//! `MapCodecTest`) runs ops-parametrically against `TestOps` and
//! `JsonOps::INSTANCE`/`COMPRESSED`. Value equality is canonicalized (via the
//! `Canon` tree), so map iteration order never leaks into the assertions.
//!
//! `JsonOps::COMPRESSED` sets `compressMaps()`, but the port's compressed-map
//! path is `STUB(dfu.compressed-map)`: a faithful COMPRESSED record encode goes
//! through `MapEncoder.compressedBuilder` (a `KeyCompressor`-backed builder
//! producing a packed array) and decode through `MapDecoder.compressedDecode`
//! (`getList` + `KeyCompressor`), neither of which is ported — the port always
//! builds via `ops.map_builder()` and reads via `get_map`. Record/map codec
//! tests whose faithful Java path requires `KeyCompressor`/`compressedBuilder`/
//! `compressedDecode` therefore run only against the non-compressed `INSTANCE`
//! backend, and each such COMPRESSED omission is marked
//! `// STUB(dfu.compressed-map)`, tracked by the dedicated compressed-map
//! sub-issue (epic #6). Surfaces that use the compressed path faithfully without
//! a `KeyCompressor` (`unitCodec` checks `getList` when `compressMaps()`) are
//! exercised against both backends.

mod common;

use common::{Canonical, OpsTestExt, TestOps, Value, v_int, v_list, v_map, v_num, v_str};
use rivet_serialization::DataResult;
use rivet_serialization::codec;
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder;
use rivet_serialization::optional_dynamic::OptionalDynamic;
use std::sync::Arc;

type StrCodec<O> = Arc<dyn rivet_serialization::Codec<String, O>>;

fn str_codec<O: DynamicOps + 'static>() -> StrCodec<O> {
    codec::string_codec()
}

fn int_codec<O: DynamicOps + 'static>() -> Arc<dyn rivet_serialization::Codec<i32, O>> {
    codec::int_codec()
}

const JSON_BACKENDS: [JsonOps; 2] = [JsonOps::INSTANCE, JsonOps::COMPRESSED];

// ---------------------------------------------------------------------------
// Dynamic — value accessors and mutation
// ---------------------------------------------------------------------------

#[test]
fn dynamic_get_and_as_number() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("value", v_num(&ops, 7.0))]));
    // `Dynamic.get("value").asNumber()`.
    let got = dynamic.get(&ops, "value");
    assert_eq!(
        got.result()
            .and_then(|d| d.as_number(&ops).result().copied()),
        Some(rivet_serialization::number::Number::Double(7.0))
    );
}

#[test]
fn dynamic_get_and_as_number_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("value", v_num(&ops, 7.0))]));
        let got = dynamic.get(&ops, "value");
        assert_eq!(
            got.result()
                .and_then(|d| d.as_number(&ops).result().copied()),
            Some(7.0)
        );
    }
}

#[test]
fn dynamic_get_missing_key_errors() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))]));
    let missing = dynamic.get(&ops, "b");
    assert!(missing.result().is_none());
    assert!(
        missing
            .get()
            .error_ref()
            .map(|e| e.message().contains("key missing"))
            .unwrap_or(false),
        "expected a 'key missing' error"
    );
}

#[test]
fn dynamic_get_missing_key_errors_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))]));
        let missing = dynamic.get(&ops, "b");
        assert!(missing.result().is_none());
        assert!(
            missing
                .get()
                .error_ref()
                .map(|e| e.message().contains("key missing"))
                .unwrap_or(false),
            "expected a 'key missing' error"
        );
    }
}

#[test]
fn dynamic_map_transforms_value() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_num(&ops, 1.0));
    let mapped = dynamic.map(|v| match v {
        Value::Num(n) => Value::Num(n + 1.0),
        other => other.clone(),
    });
    assert_eq!(mapped.get_value().canon(), v_num(&ops, 2.0).canon());
}

#[test]
fn dynamic_map_transforms_value_through_json() {
    for ops in JSON_BACKENDS {
        // The transformation is a real change to the value: 1.0 -> 2.0, asserted
        // through the ops' canonical form (not the raw `serde_json::Value`), so
        // it runs against both `INSTANCE` and `COMPRESSED` unchanged.
        let dynamic = Dynamic::new(&ops, v_num(&ops, 1.0));
        let mapped = dynamic.map(|v| match v {
            serde_json::Value::Number(n) => {
                let incremented = n.as_f64().expect("input is a finite number") + 1.0;
                serde_json::Value::Number(
                    serde_json::Number::from_f64(incremented).expect("2.0 is representable"),
                )
            }
            other => other.clone(),
        });
        assert_eq!(mapped.get_value().canon(), v_num(&ops, 2.0).canon());
    }
}

#[test]
fn dynamic_remove_key() {
    let ops = TestOps;
    let dynamic = Dynamic::new(
        &ops,
        v_map(&ops, vec![("a", v_num(&ops, 1.0)), ("b", v_num(&ops, 2.0))]),
    );
    let removed = dynamic.remove(&ops, "a");
    assert_eq!(
        removed.get_value().canon(),
        v_map(&ops, vec![("b", v_num(&ops, 2.0))]).canon()
    );
}

#[test]
fn dynamic_remove_key_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(
            &ops,
            v_map(&ops, vec![("a", v_num(&ops, 1.0)), ("b", v_num(&ops, 2.0))]),
        );
        let removed = dynamic.remove(&ops, "a");
        assert_eq!(
            removed.get_value().canon(),
            v_map(&ops, vec![("b", v_num(&ops, 2.0))]).canon()
        );
    }
}

#[test]
fn dynamic_set_key() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))]));
    let value = Dynamic::new(&ops, v_num(&ops, 9.0));
    let updated = dynamic.set(&ops, "a", &value);
    assert_eq!(
        updated.get_value().canon(),
        v_map(&ops, vec![("a", v_num(&ops, 9.0))]).canon()
    );
}

#[test]
fn dynamic_set_key_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))]));
        let value = Dynamic::new(&ops, v_num(&ops, 9.0));
        let updated = dynamic.set(&ops, "a", &value);
        assert_eq!(
            updated.get_value().canon(),
            v_map(&ops, vec![("a", v_num(&ops, 9.0))]).canon()
        );
    }
}

#[test]
fn dynamic_get_map_values_and_stream() {
    let ops = TestOps;
    let dynamic = Dynamic::new(
        &ops,
        v_map(&ops, vec![("a", v_num(&ops, 1.0)), ("b", v_num(&ops, 2.0))]),
    );
    let map_values = dynamic.get_map_values(&ops).result().cloned().unwrap();
    assert_eq!(map_values.len(), 2);

    let list = Dynamic::new(&ops, v_list(&ops, vec![v_num(&ops, 1.0), v_num(&ops, 2.0)]));
    let stream = list.as_stream_opt(&ops).result().cloned().unwrap();
    assert_eq!(stream.len(), 2);
}

#[test]
fn dynamic_get_map_values_and_stream_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(
            &ops,
            v_map(&ops, vec![("a", v_num(&ops, 1.0)), ("b", v_num(&ops, 2.0))]),
        );
        let map_values = dynamic.get_map_values(&ops).result().cloned().unwrap();
        assert_eq!(map_values.len(), 2);

        let list = Dynamic::new(&ops, v_list(&ops, vec![v_num(&ops, 1.0), v_num(&ops, 2.0)]));
        let stream = list.as_stream_opt(&ops).result().cloned().unwrap();
        assert_eq!(stream.len(), 2);
    }
}

#[test]
fn dynamic_decode_via_decoder() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_int(&ops, 3));
    let (value, _rest) = dynamic
        .decode(&ops, int_codec().as_ref())
        .result()
        .cloned()
        .unwrap();
    assert_eq!(value, 3);
}

#[test]
fn dynamic_decode_via_decoder_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_int(&ops, 3));
        let (value, _rest) = dynamic
            .decode(&ops, int_codec().as_ref())
            .result()
            .cloned()
            .unwrap();
        assert_eq!(value, 3);
    }
}

// ---------------------------------------------------------------------------
// OptionalDynamic — get / flatMap / orElseEmpty*
// ---------------------------------------------------------------------------

#[test]
fn optional_dynamic_get_field_chains() {
    let ops = TestOps;
    // {"outer": {"inner": 5}}
    let dynamic = Dynamic::new(
        &ops,
        v_map(
            &ops,
            vec![("outer", v_map(&ops, vec![("inner", v_num(&ops, 5.0))]))],
        ),
    );
    let outer: OptionalDynamic<Value> = dynamic.get(&ops, "outer");
    let inner = outer.get_field(&ops, "inner");
    assert_eq!(
        inner
            .result()
            .and_then(|d| d.as_number(&ops).result().copied()),
        Some(rivet_serialization::number::Number::Double(5.0))
    );
}

#[test]
fn optional_dynamic_get_field_chains_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(
            &ops,
            v_map(
                &ops,
                vec![("outer", v_map(&ops, vec![("inner", v_num(&ops, 5.0))]))],
            ),
        );
        let outer: OptionalDynamic<serde_json::Value> = dynamic.get(&ops, "outer");
        let inner = outer.get_field(&ops, "inner");
        assert_eq!(
            inner
                .result()
                .and_then(|d| d.as_number(&ops).result().copied()),
            Some(5.0)
        );
    }
}

#[test]
fn optional_dynamic_or_else_empty_map() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_map(&ops, Vec::new()));
    // Missing key falls back to an empty MAP.
    let missing = dynamic.get(&ops, "nope");
    assert_eq!(
        missing.or_else_empty_map(&ops).get_value().canon(),
        v_map(&ops, Vec::new()).canon()
    );
    // Present key returns the value unchanged.
    let present = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))])).get(&ops, "a");
    assert_eq!(
        present.or_else_empty_map(&ops).get_value().canon(),
        v_num(&ops, 1.0).canon()
    );
}

#[test]
fn optional_dynamic_or_else_empty_map_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_map(&ops, Vec::new()));
        // Missing key falls back to an empty MAP.
        let missing = dynamic.get(&ops, "nope");
        assert_eq!(
            missing.or_else_empty_map(&ops).get_value().canon(),
            v_map(&ops, Vec::new()).canon()
        );
        // Present key returns the value unchanged.
        let present = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 1.0))])).get(&ops, "a");
        assert_eq!(
            present.or_else_empty_map(&ops).get_value().canon(),
            v_num(&ops, 1.0).canon()
        );
    }
}

#[test]
fn optional_dynamic_flat_map_through_delegate() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 4.0))]));
    let field: OptionalDynamic<Value> = dynamic.get(&ops, "a");
    // `flatMap` over the inner `Dynamic` result.
    let doubled: DataResult<f64> =
        field.flat_map(|d| d.as_number(&ops).map(|n| n.double_value() * 2.0));
    assert_eq!(doubled.result(), Some(&8.0));
}

#[test]
fn optional_dynamic_flat_map_through_delegate_through_json() {
    for ops in JSON_BACKENDS {
        let dynamic = Dynamic::new(&ops, v_map(&ops, vec![("a", v_num(&ops, 4.0))]));
        let field: OptionalDynamic<serde_json::Value> = dynamic.get(&ops, "a");
        // `flatMap` over the inner `Dynamic` result.
        let doubled: DataResult<f64> = field.flat_map(|d| d.as_number(&ops).map(|n| *n * 2.0));
        assert_eq!(doubled.result(), Some(&8.0));
    }
}

// ---------------------------------------------------------------------------
// MapCodec — assumeMapUnsafe, unit, and field decode
// ---------------------------------------------------------------------------

#[test]
fn assume_map_unsafe_record_codec() {
    let ops = TestOps;
    #[derive(Debug, Clone, PartialEq)]
    struct Simple {
        string: String,
        integer: i32,
    }
    let record = rivet_serialization::record_builder::create::<Simple, TestOps>(move |instance| {
        instance
            .group(
                rivet_serialization::record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|o: &Simple| o.string.clone()),
                    "string".to_string(),
                    str_codec(),
                ),
            )
            .and(
                rivet_serialization::record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|o: &Simple| o.integer),
                    "integer".to_string(),
                    int_codec(),
                ),
            )
            .apply(
                instance,
                Arc::new(|s: String, i: i32| Simple {
                    string: s,
                    integer: i,
                }),
            )
    });
    // `assumeMapUnsafe` wraps a generic codec; the wrapped record's fields are
    // flattened into the object on encode (INSTANCE `AssumeMapCodec.encode` →
    // `get_map` → add each entry) and rebuilt via `createMap(entries)` →
    // `codec.parse` on decode.
    let assumed = map_codec::codec_of(map_codec::assume_map_unsafe(record));
    let value = Simple {
        string: "hello".into(),
        integer: 1,
    };
    let encoded = assumed
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
            ]
        )
        .canon()
    );
    let decoded = assumed.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, value);

    // The wrapped codec expects a map; a non-map fails.
    ops.assert_from_java_fails(&assumed, &v_str(&ops, "not a map"));
}

#[test]
fn assume_map_unsafe_record_codec_through_json() {
    // STUB(dfu.compressed-map): faithful `JsonOps.COMPRESSED` encodes through
    // `MapEncoder.compressedBuilder` (a `KeyCompressor`-backed packed array)
    // and decodes via `compressedDecode` (`getList` + `KeyCompressor`), and the
    // wrapped `AssumeMapCodec` adds the value under the `value` key to that
    // compressed builder. Neither is ported, so the object-map flatten/wrap
    // round trip is exercised only against the non-compressed `INSTANCE`
    // backend; the COMPRESSED case is tracked by the dedicated compressed-map
    // sub-issue (epic #6).
    let ops = JsonOps::INSTANCE;
    #[derive(Debug, Clone, PartialEq)]
    struct Simple {
        string: String,
        integer: i32,
    }
    let record = rivet_serialization::record_builder::create::<Simple, _>(move |instance| {
        instance
            .group(
                rivet_serialization::record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|o: &Simple| o.string.clone()),
                    "string".to_string(),
                    str_codec(),
                ),
            )
            .and(
                rivet_serialization::record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|o: &Simple| o.integer),
                    "integer".to_string(),
                    int_codec(),
                ),
            )
            .apply(
                instance,
                Arc::new(|s: String, i: i32| Simple {
                    string: s,
                    integer: i,
                }),
            )
    });
    let assumed = map_codec::codec_of(map_codec::assume_map_unsafe(record));
    let value = Simple {
        string: "hello".into(),
        integer: 1,
    };
    let encoded = assumed
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
            ]
        )
        .canon()
    );
    let decoded = assumed.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, value);

    // The wrapped codec expects a map; a non-map fails.
    ops.assert_from_java_fails(&assumed, &v_str(&ops, "not a map"));
}

#[test]
fn assume_map_unsafe_primitive_codec_fails() {
    let ops = TestOps;
    let int: Arc<dyn rivet_serialization::Codec<i32, TestOps>> = int_codec();
    let assumed = map_codec::assume_map_unsafe(int);
    let codec = map_codec::codec_of(assumed);
    ops.assert_from_java_fails(&codec, &v_num(&ops, 123.0));
    ops.assert_to_java_fails(&codec, &123);
}

#[test]
fn assume_map_unsafe_primitive_codec_fails_through_json() {
    // STUB(dfu.compressed-map): a faithful `JsonOps.COMPRESSED` encode goes
    // through `compressedBuilder` (`AssumeMapCodec.encode` under `compressMaps()`
    // adds the value under the `value` key to the `KeyCompressor`-backed packed
    // builder), so the encoded form is a packed list, not an object — the port
    // instead falls back to `ops.map_builder()`. Only the non-compressed
    // `INSTANCE` behavior is asserted (decode of a bare number fails, encode of
    // a non-map fails); the COMPRESSED case is tracked by the dedicated
    // compressed-map sub-issue (epic #6).
    let int: Arc<dyn rivet_serialization::Codec<i32, JsonOps>> = int_codec();
    let codec = map_codec::codec_of(map_codec::assume_map_unsafe(int));

    let ops = JsonOps::INSTANCE;
    ops.assert_from_java_fails(&codec, &v_num(&ops, 123.0));
    ops.assert_to_java_fails(&codec, &123);
}

#[test]
fn unit_map_codec_decodes_any_map() {
    let ops = TestOps;
    let unit = map_codec::unit_codec::<i32, TestOps>(42);
    // `MapCodec.unit(...).codec()` decodes any map to the constant.
    assert_eq!(
        unit.parse(&ops, &v_map(&ops, vec![("a", v_num(&ops, 1.0))]))
            .result(),
        Some(&42)
    );
    assert_eq!(
        unit.parse(&ops, &v_map(&ops, Vec::new())).result(),
        Some(&42)
    );
}

#[test]
fn unit_map_codec_decodes_any_map_through_json() {
    // INSTANCE: `UnitCodec` checks `getMap` — any map decodes to the constant.
    let unit = map_codec::unit_codec::<i32, JsonOps>(42);
    let ops = JsonOps::INSTANCE;
    assert_eq!(
        unit.parse(&ops, &v_map(&ops, vec![("a", v_num(&ops, 1.0))]))
            .result(),
        Some(&42)
    );
    assert_eq!(
        unit.parse(&ops, &v_map(&ops, Vec::new())).result(),
        Some(&42)
    );

    // COMPRESSED: `UnitCodec` checks `getList` instead (Java
    // `compressMaps() ? getList : getMap`), so it decodes packed lists rather
    // than maps.
    let ops = JsonOps::COMPRESSED;
    assert_eq!(
        unit.parse(&ops, &v_list(&ops, Vec::new())).result(),
        Some(&42)
    );
    assert!(unit.parse(&ops, &v_map(&ops, Vec::new())).is_error());
}

#[test]
fn field_decoder_missing_key_errors() {
    let ops = TestOps;
    let field = map_decoder::field_decoder::<i32, TestOps>("x".to_string(), int_codec());
    let missing = field.decode(&ops, &common::TestMapLike(Vec::new()));
    assert!(missing.is_error());
    assert!(
        missing.error_ref().unwrap().message().contains("No key x"),
        "expected a 'No key x' error"
    );

    let present = field.decode(
        &ops,
        &common::TestMapLike(vec![rivet_serialization::pair::Pair::of(
            v_str(&ops, "x"),
            v_int(&ops, 5),
        )]),
    );
    assert_eq!(present.result(), Some(&5));
}

/// MapLike over `serde_json::Value` backed by an explicit entry list, mirroring
/// `TestMapLike` for the JsonOps backend.
#[derive(Debug)]
struct JsonMapLike(Vec<rivet_serialization::pair::Pair<serde_json::Value, serde_json::Value>>);

impl rivet_serialization::dynamic_ops::MapLike<serde_json::Value> for JsonMapLike {
    fn get(&self, key: &serde_json::Value) -> Option<serde_json::Value> {
        self.0
            .iter()
            .find(|p| &p.first == key)
            .map(|p| p.second.clone())
    }

    fn get_string(&self, key: &str) -> Option<serde_json::Value> {
        self.0
            .iter()
            .find(|p| matches!(&p.first, serde_json::Value::String(s) if s == key))
            .map(|p| p.second.clone())
    }

    fn entries(
        &self,
    ) -> Vec<rivet_serialization::pair::Pair<serde_json::Value, serde_json::Value>> {
        self.0.clone()
    }
}

#[test]
fn field_decoder_missing_key_errors_through_json() {
    for ops in JSON_BACKENDS {
        let field = map_decoder::field_decoder::<i32, JsonOps>("x".to_string(), int_codec());
        let missing = field.decode(&ops, &JsonMapLike(Vec::new()));
        assert!(missing.is_error());
        assert!(
            missing.error_ref().unwrap().message().contains("No key x"),
            "expected a 'No key x' error"
        );

        let present = field.decode(
            &ops,
            &JsonMapLike(vec![rivet_serialization::pair::Pair::of(
                v_str(&ops, "x"),
                v_int(&ops, 5),
            )]),
        );
        assert_eq!(present.result(), Some(&5));
    }
}
