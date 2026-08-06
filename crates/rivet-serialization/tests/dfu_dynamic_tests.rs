//! DFU-mirroring tests for the ported `Dynamic`/`OptionalDynamic` surface and
//! the `MapCodec` combinators (`assumeMapUnsafe`, `unit`, field codecs).
//!
//! Upstream coverage (`com.mojang.serialization.DynamicTest` and
//! `MapCodecTest`) is exercised against the minimal `TestOps` DynamicOps.

mod common;

use common::{TestOps, Value};
use rivet_serialization::DataResult;
use rivet_serialization::codec;
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder;
use rivet_serialization::optional_dynamic::OptionalDynamic;
use std::sync::Arc;

type StrCodec = Arc<dyn rivet_serialization::Codec<String, TestOps>>;

fn str_codec() -> StrCodec {
    codec::string_codec()
}

fn int_codec() -> Arc<dyn rivet_serialization::Codec<i32, TestOps>> {
    codec::int_codec()
}

// ---------------------------------------------------------------------------
// Dynamic — value accessors and mutation
// ---------------------------------------------------------------------------

#[test]
fn dynamic_get_and_as_number() {
    let ops = TestOps;
    let dynamic = Dynamic::new(
        &ops,
        Value::Map(vec![("value".to_string(), Value::Num(7.0))]),
    );
    // `Dynamic.get("value").asNumber()`.
    let got = dynamic.get(&ops, "value");
    assert_eq!(
        got.result()
            .and_then(|d| d.as_number(&ops).result().copied()),
        Some(7.0)
    );
}

#[test]
fn dynamic_get_missing_key_errors() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Map(vec![("a".to_string(), Value::Num(1.0))]));
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
fn dynamic_map_transforms_value() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Num(1.0));
    let mapped = dynamic.map(|v| match v {
        Value::Num(n) => Value::Num(n + 1.0),
        other => other.clone(),
    });
    assert_eq!(mapped.get_value(), &Value::Num(2.0));
}

#[test]
fn dynamic_remove_key() {
    let ops = TestOps;
    let dynamic = Dynamic::new(
        &ops,
        Value::Map(vec![
            ("a".to_string(), Value::Num(1.0)),
            ("b".to_string(), Value::Num(2.0)),
        ]),
    );
    let removed = dynamic.remove(&ops, "a");
    assert_eq!(
        removed.get_value(),
        &Value::Map(vec![("b".to_string(), Value::Num(2.0))])
    );
}

#[test]
fn dynamic_set_key() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Map(vec![("a".to_string(), Value::Num(1.0))]));
    let value = Dynamic::new(&ops, Value::Num(9.0));
    let updated = dynamic.set(&ops, "a", &value);
    assert_eq!(
        updated.get_value(),
        &Value::Map(vec![("a".to_string(), Value::Num(9.0))])
    );
}

#[test]
fn dynamic_get_map_values_and_stream() {
    let ops = TestOps;
    let dynamic = Dynamic::new(
        &ops,
        Value::Map(vec![
            ("a".to_string(), Value::Num(1.0)),
            ("b".to_string(), Value::Num(2.0)),
        ]),
    );
    let map_values = dynamic.get_map_values(&ops).result().cloned().unwrap();
    assert_eq!(map_values.len(), 2);

    let list = Dynamic::new(&ops, Value::List(vec![Value::Num(1.0), Value::Num(2.0)]));
    let stream = list.as_stream_opt(&ops).result().cloned().unwrap();
    assert_eq!(stream.len(), 2);
}

#[test]
fn dynamic_decode_via_decoder() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Num(3.0));
    let (value, _rest) = dynamic
        .decode(&ops, int_codec().as_ref())
        .result()
        .cloned()
        .unwrap();
    assert_eq!(value, 3);
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
        Value::Map(vec![(
            "outer".to_string(),
            Value::Map(vec![("inner".to_string(), Value::Num(5.0))]),
        )]),
    );
    let outer: OptionalDynamic<Value> = dynamic.get(&ops, "outer");
    let inner = outer.get_field(&ops, "inner");
    assert_eq!(
        inner
            .result()
            .and_then(|d| d.as_number(&ops).result().copied()),
        Some(5.0)
    );
}

#[test]
fn optional_dynamic_or_else_empty_map() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Map(Vec::new()));
    // Missing key falls back to an empty MAP.
    let missing = dynamic.get(&ops, "nope");
    assert_eq!(
        missing.or_else_empty_map(&ops).get_value(),
        &Value::Map(Vec::new())
    );
    // Present key returns the value unchanged.
    let present =
        Dynamic::new(&ops, Value::Map(vec![("a".to_string(), Value::Num(1.0))])).get(&ops, "a");
    assert_eq!(
        present.or_else_empty_map(&ops).get_value(),
        &Value::Num(1.0)
    );
}

#[test]
fn optional_dynamic_flat_map_through_delegate() {
    let ops = TestOps;
    let dynamic = Dynamic::new(&ops, Value::Map(vec![("a".to_string(), Value::Num(4.0))]));
    let field: OptionalDynamic<Value> = dynamic.get(&ops, "a");
    // `flatMap` over the inner `Dynamic` result.
    let doubled: DataResult<f64> = field.flat_map(|d| d.as_number(&ops).map(|n| *n * 2.0));
    assert_eq!(doubled.result(), Some(&8.0));
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
    // A RecordCodecBuilder codec is already a map codec; assumeMapUnsafe wraps
    // a generic codec. Round trip through the plain record.
    let value = Simple {
        string: "hello".into(),
        integer: 1,
    };
    let encoded = record
        .encode_start(&ops, &value)
        .get_or_throw("encodeStart")
        .clone();
    assert_eq!(
        encoded,
        Value::Map(vec![
            ("string".to_string(), Value::Str("hello".into())),
            ("integer".to_string(), Value::Num(1.0)),
        ])
    );
    let decoded = record.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, value);

    // A record codec expects a map; a non-map fails.
    ops.assert_from_java_fails(&record, &Value::Str("not a map".into()));
}

#[test]
fn assume_map_unsafe_primitive_codec_fails() {
    let ops = TestOps;
    let int: Arc<dyn rivet_serialization::Codec<i32, TestOps>> = int_codec();
    let assumed = map_codec::assume_map_unsafe(int);
    let codec = map_codec::codec_of(assumed);
    ops.assert_from_java_fails(&codec, &Value::Num(123.0));
    ops.assert_to_java_fails(&codec, &123);
}

#[test]
fn unit_map_codec_decodes_any_map() {
    let ops = TestOps;
    let unit = map_codec::unit_codec::<i32, TestOps>(42);
    // `MapCodec.unit(...).codec()` decodes any map to the constant.
    assert_eq!(
        unit.parse(&ops, &Value::Map(vec![("a".to_string(), Value::Num(1.0))]))
            .result(),
        Some(&42)
    );
    assert_eq!(
        unit.parse(&ops, &Value::Map(Vec::new())).result(),
        Some(&42)
    );
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
            Value::Str("x".into()),
            Value::Num(5.0),
        )]),
    );
    assert_eq!(present.result(), Some(&5));
}
