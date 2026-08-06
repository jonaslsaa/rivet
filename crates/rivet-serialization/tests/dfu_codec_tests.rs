//! DFU-mirroring tests for the ported `com.mojang.serialization` codec surface.
//!
//! Ports a meaningful subset of the upstream `com.mojang.serialization.CodecTests`
//! (Mojang/DataFixerUpper master) against the minimal `TestOps` DynamicOps:
//! concrete-codec round trips, error/partial semantics, RecordCodecBuilder
//! field accumulation and field order, optional-field strict/lenient behavior,
//! lifecycle propagation, and the `orElse`/`orElseGet` result functions.

mod common;

use common::{TestOps, Value};
use rivet_serialization::DataResult;
use rivet_serialization::codec;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::collections::HashMap;
use std::sync::Arc;

type StrCodec = Arc<dyn rivet_serialization::Codec<String, TestOps>>;
type IntCodec = Arc<dyn rivet_serialization::Codec<i32, TestOps>>;

fn str_codec() -> StrCodec {
    codec::string_codec()
}

fn int_codec() -> IntCodec {
    codec::int_codec()
}

fn to_lower_case() -> StrCodec {
    codec::xmap(
        str_codec(),
        Arc::new(|s: &String| s.to_lowercase()),
        Arc::new(|s: &String| s.to_lowercase()),
    )
}

// ---------------------------------------------------------------------------
// ListCodec (CodecTests.list_roundTrip / list_invalidValues)
// ---------------------------------------------------------------------------

#[test]
fn list_round_trip() {
    let ops = TestOps;
    let list = codec::list(str_codec());
    let value = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
    let encoded = Value::List(vec![
        Value::Str("foo".into()),
        Value::Str("bar".into()),
        Value::Str("baz".into()),
    ]);
    ops.assert_round_trip(&list, value, encoded);
}

#[test]
fn list_invalid_values() {
    let ops = TestOps;
    let list = codec::list(str_codec());

    // assertFromJavaFails: mixed list with a non-string
    let bad = Value::List(vec![
        Value::Str("foo".into()),
        Value::Num(2.0),
        Value::Str("baz".into()),
        Value::Bool(false),
    ]);
    ops.assert_from_java_fails(&list, &bad);

    // partial keeps the valid prefix up to (but excluding) the invalid value
    let partial1 = ops.from_java_or_partial(
        &list,
        &Value::List(vec![
            Value::Str("foo".into()),
            Value::Str("bar".into()),
            Value::Num(2.0),
            Value::Bool(false),
        ]),
    );
    assert_eq!(partial1, vec!["foo".to_string(), "bar".to_string()]);

    let partial2 = ops.from_java_or_partial(
        &list,
        &Value::List(vec![
            Value::Str("foo".into()),
            Value::Num(2.0),
            Value::Str("baz".into()),
            Value::Bool(false),
        ]),
    );
    assert_eq!(partial2, vec!["foo".to_string(), "baz".to_string()]);
}

// ---------------------------------------------------------------------------
// ListCodec with size limits (CodecTests.sizeLimitedList_*)
// ---------------------------------------------------------------------------

#[test]
fn size_limited_list_round_trip() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 2);
    ops.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string()],
        Value::List(vec![Value::Str("foo".into()), Value::Str("bar".into())]),
    );
}

#[test]
fn size_limited_list_too_long() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 2);
    let three = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
    let input = Value::List(vec![
        Value::Str("foo".into()),
        Value::Str("bar".into()),
        Value::Str("baz".into()),
    ]);

    ops.assert_from_java_fails(&limited, &input);
    ops.assert_to_java_fails(&limited, &three);

    // Input is clipped in the partial result to the max size.
    let partial = ops.from_java_or_partial(&limited, &input);
    assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn size_limited_list_too_long_with_invalid() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 2);
    // Input is clipped only by valid entries (invalid entries do not count
    // toward the size).
    let partial = ops.from_java_or_partial(
        &limited,
        &Value::List(vec![
            Value::Str("foo".into()),
            Value::Num(2.0),
            Value::Str("bar".into()),
            Value::Str("baz".into()),
            Value::Bool(false),
        ]),
    );
    assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn size_limited_list_too_short() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 3);
    ops.assert_to_java_fails(&limited, &vec!["foo".to_string()]);
    // No partial can be obtained when the data is too short.
    ops.assert_from_java_fails_partial(&limited, &Value::List(vec![Value::Str("foo".into())]));

    ops.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string()],
        Value::List(vec![Value::Str("foo".into()), Value::Str("bar".into())]),
    );
    ops.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string(), "baz".to_string()],
        Value::List(vec![
            Value::Str("foo".into()),
            Value::Str("bar".into()),
            Value::Str("baz".into()),
        ]),
    );
}

// ---------------------------------------------------------------------------
// unboundedMap (CodecTests.unboundedMap_*)
// ---------------------------------------------------------------------------

#[test]
fn unbounded_map_simple() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let mut value = HashMap::new();
    value.insert("foo".to_string(), 1);
    value.insert("bar".to_string(), 2);
    let encoded = Value::Map(vec![
        ("foo".to_string(), Value::Num(1.0)),
        ("bar".to_string(), Value::Num(2.0)),
    ]);
    ops.assert_round_trip(&map_codec, value, encoded);
}

#[test]
fn unbounded_map_invalid_entry() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let input = Value::Map(vec![
        ("foo".to_string(), Value::Num(1.0)),
        ("bar".to_string(), Value::Str("garbage".into())),
        ("baz".to_string(), Value::Num(3.0)),
    ]);
    ops.assert_from_java_fails(&map_codec, &input);
}

#[test]
fn unbounded_map_invalid_entry_partial() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let partial = ops.from_java_or_partial(
        &map_codec,
        &Value::Map(vec![
            ("foo".to_string(), Value::Num(1.0)),
            ("bar".to_string(), Value::Str("garbage".into())),
            ("baz".to_string(), Value::Num(3.0)),
        ]),
    );
    let mut expected = HashMap::new();
    expected.insert("foo".to_string(), 1);
    expected.insert("baz".to_string(), 3);
    assert_eq!(partial, expected);
}

#[test]
fn unbounded_map_invalid_entry_nested_partial() {
    let ops = TestOps;
    let inner = codec::unbounded_map(str_codec(), int_codec());
    let outer = codec::unbounded_map(str_codec(), inner);
    let partial = ops.from_java_or_partial(
        &outer,
        &Value::Map(vec![
            (
                "foo".to_string(),
                Value::Map(vec![("foo".to_string(), Value::Num(1.0))]),
            ),
            (
                "bar".to_string(),
                Value::Map(vec![
                    ("foo".to_string(), Value::Num(1.0)),
                    ("bar".to_string(), Value::Str("garbage".into())),
                    ("baz".to_string(), Value::Num(3.0)),
                ]),
            ),
        ]),
    );

    let mut inner_expected = HashMap::new();
    inner_expected.insert("foo".to_string(), 1);
    inner_expected.insert("baz".to_string(), 3);
    let mut outer_expected = HashMap::new();
    outer_expected.insert("foo".to_string(), HashMap::from([("foo".to_string(), 1)]));
    outer_expected.insert("bar".to_string(), inner_expected);
    assert_eq!(partial, outer_expected);
}

#[test]
fn unbounded_map_repeated_keys() {
    let ops = TestOps;
    // The lowercasing key codec collapses "foo" and "FOO" onto the same key.
    let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
    let input = Value::Map(vec![
        ("foo".to_string(), Value::Num(1.0)),
        ("FOO".to_string(), Value::Num(2.0)),
    ]);
    ops.assert_from_java_fails(&map_codec, &input);
}

#[test]
fn unbounded_map_repeated_keys_partial() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
    let partial = ops.from_java_or_partial(
        &map_codec,
        &Value::Map(vec![
            ("foo".to_string(), Value::Num(1.0)),
            ("bar".to_string(), Value::Num(2.0)),
            ("FOO".to_string(), Value::Num(3.0)),
        ]),
    );
    // The first entry wins for the partial result.
    let mut expected = HashMap::new();
    expected.insert("foo".to_string(), 1);
    expected.insert("bar".to_string(), 2);
    assert_eq!(partial, expected);
}

// ---------------------------------------------------------------------------
// either / xor (CodecTests.withAlternative_*)
// ---------------------------------------------------------------------------

/// A `Codec<String>` that always fails with the given message.
fn never(message: &'static str) -> StrCodec {
    codec::validate(
        str_codec(),
        Arc::new(move |_: &String| DataResult::<String>::error(message)),
    )
}

/// A `Codec<String>` that always fails with the given message AND a partial
/// equal to `partial_prefix` + the input value (mirroring the upstream
/// `NEVER_WITH_PARTIAL_*` constants).
fn never_with_partial(message: &'static str, partial_prefix: String) -> StrCodec {
    let prefix = partial_prefix.clone();
    codec::validate(
        str_codec(),
        Arc::new(move |s: &String| {
            DataResult::<String>::error_with_partial(message, format!("{}{}", prefix, s))
        }),
    )
}

#[test]
fn with_alternative_primary_partial_alternative_fails() {
    let ops = TestOps;
    let codec = codec::with_alternative(
        never_with_partial(
            "Failed Primary with partial",
            "Partial Primary: ".to_string(),
        ),
        never("Failed Alternative"),
    );
    assert_eq!(
        ops.from_java_or_partial(&codec, &Value::Str("value".into())),
        "Partial Primary: value"
    );
    assert_eq!(
        ops.from_java_error_message(&codec, &Value::Str("value".into())),
        "Failed Primary with partial"
    );
}

#[test]
fn with_alternative_primary_fails_alternative_partial() {
    let ops = TestOps;
    let codec = codec::with_alternative(
        never("Failed Primary"),
        never_with_partial(
            "Failed Alternative with partial",
            "Partial Alternative: ".to_string(),
        ),
    );
    assert_eq!(
        ops.from_java_or_partial(&codec, &Value::Str("value".into())),
        "Partial Alternative: value"
    );
    assert_eq!(
        ops.from_java_error_message(&codec, &Value::Str("value".into())),
        "Failed Alternative with partial"
    );
}

#[test]
fn with_alternative_both_partial_prefers_primary() {
    let ops = TestOps;
    let codec = codec::with_alternative(
        never_with_partial(
            "Failed Primary with partial",
            "Partial Primary: ".to_string(),
        ),
        never_with_partial(
            "Failed Alternative with partial",
            "Partial Alternative: ".to_string(),
        ),
    );
    assert_eq!(
        ops.from_java_or_partial(&codec, &Value::Str("value".into())),
        "Partial Primary: value"
    );
}

#[test]
fn with_alternative_both_fail() {
    let ops = TestOps;
    let codec = codec::with_alternative(never("Failed Primary"), never("Failed Alternative"));
    assert_eq!(
        ops.from_java_error_message(&codec, &Value::Str("value".into())),
        "Failed to parse either. First: Failed Primary; Second: Failed Alternative"
    );
}

#[test]
fn with_alternative_both_successful() {
    let ops = TestOps;
    let codec = codec::with_alternative(str_codec(), to_lower_case());
    ops.assert_round_trip(&codec, "string".to_string(), Value::Str("string".into()));
    // Primary codec is chosen over the alternative.
    ops.assert_round_trip(&codec, "STRING".to_string(), Value::Str("STRING".into()));
}

// ---------------------------------------------------------------------------
// optionalField — strict vs lenient (CodecTests.optionalField_*)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct SimpleOptionals {
    string: Option<String>,
    integer: Option<i32>,
}

fn simple_optionals_codec() -> Arc<dyn rivet_serialization::Codec<SimpleOptionals, TestOps>> {
    rivet_serialization::record_builder::create::<SimpleOptionals, TestOps>(move |instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.string.clone()),
                codec::optional_field::<String, TestOps>("string".to_string(), str_codec(), false),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.integer),
                codec::optional_field::<i32, TestOps>("integer".to_string(), int_codec(), false),
            ))
            .apply(
                instance,
                Arc::new(|s: Option<String>, i: Option<i32>| SimpleOptionals {
                    string: s,
                    integer: i,
                }),
            )
    })
}

fn simple_optionals_lenient_codec() -> Arc<dyn rivet_serialization::Codec<SimpleOptionals, TestOps>>
{
    rivet_serialization::record_builder::create::<SimpleOptionals, TestOps>(move |instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.string.clone()),
                codec::optional_field::<String, TestOps>("string".to_string(), str_codec(), true),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.integer),
                codec::optional_field::<i32, TestOps>("integer".to_string(), int_codec(), true),
            ))
            .apply(
                instance,
                Arc::new(|s: Option<String>, i: Option<i32>| SimpleOptionals {
                    string: s,
                    integer: i,
                }),
            )
    })
}

#[test]
fn optional_field_round_trip() {
    let ops = TestOps;
    for codec in [simple_optionals_codec(), simple_optionals_lenient_codec()] {
        ops.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: Some("foo".into()),
                integer: Some(1),
            },
            Value::Map(vec![
                ("string".to_string(), Value::Str("foo".into())),
                ("integer".to_string(), Value::Num(1.0)),
            ]),
        );
        ops.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: None,
                integer: Some(1),
            },
            Value::Map(vec![("integer".to_string(), Value::Num(1.0))]),
        );
    }
}

#[test]
fn optional_field_strict_invalid_values() {
    let ops = TestOps;
    let strict = simple_optionals_codec();
    ops.assert_from_java_fails(
        &strict,
        &Value::Map(vec![("string".to_string(), Value::Num(54.0))]),
    );
    ops.assert_from_java_fails(
        &strict,
        &Value::Map(vec![(
            "integer".to_string(),
            Value::Str("not an int".into()),
        )]),
    );
}

#[test]
fn optional_field_strict_invalid_values_partial() {
    let ops = TestOps;
    let strict = simple_optionals_codec();
    let partial = ops.from_java_or_partial(
        &strict,
        &Value::Map(vec![
            ("string".to_string(), Value::Bool(false)),
            ("integer".to_string(), Value::Num(23.0)),
        ]),
    );
    assert_eq!(
        partial,
        SimpleOptionals {
            string: None,
            integer: Some(23)
        }
    );
}

#[test]
fn optional_field_lenient_invalid_values() {
    let ops = TestOps;
    let lenient = simple_optionals_lenient_codec();
    let parsed = ops.parse_or_throw(
        &lenient,
        &Value::Map(vec![
            ("string".to_string(), Value::Bool(false)),
            ("integer".to_string(), Value::Num(23.0)),
        ]),
    );
    assert_eq!(
        parsed,
        SimpleOptionals {
            string: None,
            integer: Some(23)
        }
    );
}

// ---------------------------------------------------------------------------
// RecordCodecBuilder — error accumulation + field order
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Simple {
    string: String,
    integer: i32,
}

fn simple_codec() -> Arc<dyn rivet_serialization::Codec<Simple, TestOps>> {
    rivet_serialization::record_builder::create::<Simple, TestOps>(move |instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|o: &Simple| o.string.clone()),
                "string".to_string(),
                str_codec(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|o: &Simple| o.integer),
                "integer".to_string(),
                int_codec(),
            ))
            .apply(
                instance,
                Arc::new(|s: String, i: i32| Simple {
                    string: s,
                    integer: i,
                }),
            )
    })
}

#[test]
fn record_codec_round_trip_and_error_accumulation() {
    let ops = TestOps;
    let codec = simple_codec();
    ops.assert_round_trip(
        &codec,
        Simple {
            string: "hello".into(),
            integer: 1,
        },
        Value::Map(vec![
            ("string".to_string(), Value::Str("hello".into())),
            ("integer".to_string(), Value::Num(1.0)),
        ]),
    );

    // A record that is not a map fails.
    ops.assert_from_java_fails(&codec, &Value::Str("not a map".into()));

    // Both fields invalid: errors accumulate (the apply2 message order).
    let result = codec.parse(
        &ops,
        &Value::Map(vec![
            ("string".to_string(), Value::Num(1.0)),
            ("integer".to_string(), Value::Str("x".into())),
        ]),
    );
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(
        msg.contains("Not a string") && msg.contains("Not a number") && msg.contains(';'),
        "expected the two field errors accumulated, got: {msg}"
    );
}

#[derive(Debug, Clone, PartialEq)]
struct RecordWith4Fields {
    f1: i32,
    f2: i32,
    f3: i32,
    f4: i32,
}

// Note: the Rust port of `RecordCodecBuilder` currently supports group arities
// up to 4 (`Products.P4`), so the upstream 5- and 7-field order tests are
// mirrored with a 4-field record.
fn record_with_4_fields_codec() -> Arc<dyn rivet_serialization::Codec<RecordWith4Fields, TestOps>> {
    rivet_serialization::record_builder::create::<RecordWith4Fields, TestOps>(move |instance| {
        let field = |name: &'static str| {
            RecordCodecBuilder::of_named(
                Arc::new(move |o: &RecordWith4Fields| match name {
                    "f1" => o.f1,
                    "f2" => o.f2,
                    "f3" => o.f3,
                    _ => o.f4,
                }),
                name.to_string(),
                int_codec(),
            )
        };
        instance
            .group(field("f1"))
            .and(field("f2"))
            .and(field("f3"))
            .and(field("f4"))
            .apply(
                instance,
                Arc::new(|f1, f2, f3, f4| RecordWith4Fields { f1, f2, f3, f4 }),
            )
    })
}

#[test]
fn record_codec_maintains_field_order() {
    let ops = TestOps;
    let codec = record_with_4_fields_codec();
    let value = RecordWith4Fields {
        f1: 4,
        f2: 3,
        f3: 2,
        f4: 1,
    };
    let encoded = codec
        .encode_start(&ops, &value)
        .get_or_throw("encodeStart")
        .clone();
    // The encoded map must preserve the field declaration order.
    match encoded {
        Value::Map(entries) => {
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                vec!["f1", "f2", "f3", "f4"],
                "encoded record must keep field order"
            );
        }
        other => panic!("expected a map, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// unit map codec (CodecTests.unitMapCodecEncoding)
// ---------------------------------------------------------------------------

#[test]
fn unit_map_codec_encoding() {
    let ops = TestOps;
    let marker = 42_i32;
    let codec = map_codec::unit_codec::<i32, TestOps>(marker);
    ops.assert_round_trip(&codec, marker, Value::Map(Vec::new()));
}

// ---------------------------------------------------------------------------
// mapResult / orElse (CodecTests.withAlternative_* exercises orElse; here the
// orElseGet result functions)
// ---------------------------------------------------------------------------

#[test]
fn or_else_get_recovers_on_error() {
    let ops = TestOps;
    // A codec that always errors; orElseGet supplies a fallback value.
    let failing: StrCodec = never("nope");
    let recovered = codec::or_else_value(failing.clone(), "fallback".to_string());

    // Reading a string still errors at the primary codec but recovers.
    let read = ops.parse_or_throw(&recovered, &Value::Str("any".into()));
    assert_eq!(read, "fallback");

    // Encoding through the primary still fails (orElse only recovers decode).
    ops.assert_to_java_fails(&failing, &"x".to_string());
}

// ---------------------------------------------------------------------------
// lifecycle: stable/experimental propagation through combinators
// ---------------------------------------------------------------------------

#[test]
fn stable_codec_round_trip_stays_stable() {
    let ops = TestOps;
    let stable_int = codec::stable(int_codec());
    let result = stable_int.decode(&ops, &Value::Num(5.0));
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}

#[test]
fn list_of_stable_elements_round_trip() {
    let ops = TestOps;
    let stable_list = codec::list(codec::stable(str_codec()));
    let value = vec!["a".to_string(), "b".to_string()];
    let encoded = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]);
    ops.assert_round_trip(&stable_list, value, encoded.clone());
    let result = stable_list.decode(&ops, &encoded);
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}
