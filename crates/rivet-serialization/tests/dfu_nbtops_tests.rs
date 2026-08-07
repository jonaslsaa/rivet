//! DFU-mirroring tests against `NbtOps` (`net.minecraft.nbt.NbtOps`) — the
//! final backend of the epic-#6 DoD ("DFU's tests green against NbtOps/JsonOps").
//!
//! The shared `common` harness is ops-parametric (`OpsTestExt` + `Canonical`);
//! `NbtOps` is added as a `Canonical` backend (`impl Canonical for Tag` in
//! `common`). The mirror covers every generic-suite surface that NbtOps
//! honestly supports, citing the mirrored upstream/`dfu_codec_tests.rs`/
//! `dfu_dynamic_tests.rs` test name:
//!
//! - list + size-limited list (`list_roundTrip`, `sizeLimitedList_*`)
//! - `unboundedMap` (`unboundedMap_*`)
//! - `withAlternative` (`withAlternative_*`)
//! - `optionalField` strict/lenient (`optionalField_*`)
//! - record codec round-trip + error accumulation (`recordCodecRoundTrip`)
//! - `unit`/`orElseGet`/lifecycle (`stable`, `listOfStable`)
//! - `Dynamic`/`OptionalDynamic` accessors (`DynamicTest`)
//! - `assumeMapUnsafe`/`unit`/field decoder (`MapCodecTest`)
//!
//! Not portable (honest deferrals, noted in prose — test files are excluded
//! from the `STUB`/`RivetTodo` marker convention):
//! - `record_codec_maintains_field_order` — `NbtOps` map values are an
//!   insertion-ordered `IndexMap`-backed `CompoundTag` (DECISIONS.md D12), so
//!   Rust field order is Rust's put sequence, which differs from Java's
//!   fastutil hash order — the `compound_key_order` divergence. The
//!   order-insensitive round-trip surfaces above still fully cover the
//!   record/`NbtRecordBuilder` encode+decode path.
//! - `unbounded_map_repeated_keys_partial` — the "first colliding key wins the
//!   partial" assertion is likewise iteration-order-dependent (only the
//!   order-insensitive `unbounded_map_repeated_keys` duplicate-key error
//!   surface is portable and is mirrored above).
//! - `JsonOps.COMPRESSED` forms (`compressMaps()` is false for NbtOps — there
//!   is no compressed mode, matching Java).

mod common;

use common::{Canonical, OpsTestExt, v_bool, v_int, v_list, v_map, v_num, v_str};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_serialization::DataResult;
use rivet_serialization::codec;
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::dynamic_ops::MapLike;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder;
use rivet_serialization::number::Number;
use rivet_serialization::optional_dynamic::OptionalDynamic;
use rivet_serialization::pair::Pair;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::collections::HashMap;
use std::sync::Arc;

type StrCodec<O> = Arc<dyn rivet_serialization::Codec<String, O>>;
type IntCodec<O> = Arc<dyn rivet_serialization::Codec<i32, O>>;

fn ops() -> NbtOps {
    NbtOps::instance()
}

fn str_codec<O: DynamicOps + 'static>() -> StrCodec<O> {
    codec::string_codec()
}

fn int_codec<O: DynamicOps + 'static>() -> IntCodec<O> {
    codec::int_codec()
}

fn to_lower_case<O: DynamicOps + 'static>() -> StrCodec<O> {
    codec::xmap(
        str_codec(),
        Arc::new(|s: &String| s.to_lowercase()),
        Arc::new(|s: &String| s.to_lowercase()),
    )
}

fn snbt(tag: &Tag) -> String {
    rivet_nbt::string_tag_visitor::StringTagVisitor::to_string(tag)
}

// ---------------------------------------------------------------------------
// ListCodec (CodecTests.list_roundTrip / list_invalidValues)
// ---------------------------------------------------------------------------

#[test]
fn list_round_trip() {
    let o = ops();
    let list = codec::list(str_codec());
    let value = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
    let encoded = v_list(
        &o,
        vec![v_str(&o, "foo"), v_str(&o, "bar"), v_str(&o, "baz")],
    );
    o.assert_round_trip(&list, value, encoded);
}

#[test]
fn list_invalid_values() {
    let o = ops();
    let list = codec::list(str_codec());

    // assertFromJavaFails: mixed list with a non-string (an IntTag) fails.
    let bad = v_list(
        &o,
        vec![
            v_str(&o, "foo"),
            v_int(&o, 2),
            v_str(&o, "baz"),
            v_bool(&o, false),
        ],
    );
    o.assert_from_java_fails(&list, &bad);

    // partial keeps the valid prefix up to (but excluding) the invalid value.
    let prefix_partial = o.from_java_or_partial(
        &list,
        &v_list(
            &o,
            vec![
                v_str(&o, "foo"),
                v_str(&o, "bar"),
                v_int(&o, 2),
                v_bool(&o, false),
            ],
        ),
    );
    assert_eq!(prefix_partial, vec!["foo".to_string(), "bar".to_string()]);

    // Invalid elements (an IntTag and a ByteTag, the NBT boolean form) are
    // skipped rather than terminating the partial.
    let skip_partial = o.from_java_or_partial(
        &list,
        &v_list(
            &o,
            vec![
                v_str(&o, "foo"),
                v_int(&o, 2),
                v_str(&o, "baz"),
                v_bool(&o, false),
            ],
        ),
    );
    assert_eq!(skip_partial, vec!["foo".to_string(), "baz".to_string()]);
}

// ---------------------------------------------------------------------------
// ListCodec with size limits (CodecTests.sizeLimitedList_*)
// ---------------------------------------------------------------------------

#[test]
fn size_limited_list_round_trip_and_clipping() {
    let o = ops();
    let limited = codec::list_with_range(str_codec(), 2, 2);
    o.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string()],
        v_list(&o, vec![v_str(&o, "foo"), v_str(&o, "bar")]),
    );
    // Too long: the parse fails and clips the partial to the max size.
    let input = v_list(
        &o,
        vec![v_str(&o, "foo"), v_str(&o, "bar"), v_str(&o, "baz")],
    );
    o.assert_from_java_fails(&limited, &input);
    assert_eq!(
        o.from_java_or_partial(&limited, &input),
        vec!["foo".to_string(), "bar".to_string()]
    );
    // Too short: no partial.
    o.assert_to_java_fails(&limited, &vec!["foo".to_string()]);
    o.assert_from_java_fails_partial(&limited, &v_list(&o, vec![v_str(&o, "foo")]));
}

#[test]
fn size_limited_list_too_long_with_invalid() {
    let o = ops();
    let limited = codec::list_with_range(str_codec(), 2, 2);
    // Input is clipped only by valid entries (invalid entries do not count
    // toward the size). Invalid elements: an IntTag and a ByteTag boolean form.
    let partial = o.from_java_or_partial(
        &limited,
        &v_list(
            &o,
            vec![
                v_str(&o, "foo"),
                v_int(&o, 2),
                v_str(&o, "bar"),
                v_str(&o, "baz"),
                v_bool(&o, false),
            ],
        ),
    );
    assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
}

// ---------------------------------------------------------------------------
// unboundedMap (CodecTests.unboundedMap_*)
// ---------------------------------------------------------------------------

#[test]
fn unbounded_map_simple() {
    let o = ops();
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let mut value = HashMap::new();
    value.insert("foo".to_string(), 1);
    value.insert("bar".to_string(), 2);
    let encoded = v_map(&o, vec![("foo", v_int(&o, 1)), ("bar", v_int(&o, 2))]);
    o.assert_round_trip(&map_codec, value, encoded);
}

#[test]
fn unbounded_map_invalid_entry_partial() {
    let o = ops();
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let partial = o.from_java_or_partial(
        &map_codec,
        &v_map(
            &o,
            vec![
                ("foo", v_int(&o, 1)),
                ("bar", v_str(&o, "garbage")),
                ("baz", v_int(&o, 3)),
            ],
        ),
    );
    let mut expected = HashMap::new();
    expected.insert("foo".to_string(), 1);
    expected.insert("baz".to_string(), 3);
    assert_eq!(partial, expected);
}

#[test]
fn unbounded_map_invalid_entry_nested_partial() {
    let o = ops();
    let inner = codec::unbounded_map(str_codec(), int_codec());
    let outer = codec::unbounded_map(str_codec(), inner);
    let partial = o.from_java_or_partial(
        &outer,
        &v_map(
            &o,
            vec![
                ("foo", v_map(&o, vec![("foo", v_int(&o, 1))])),
                (
                    "bar",
                    v_map(
                        &o,
                        vec![
                            ("foo", v_int(&o, 1)),
                            ("bar", v_str(&o, "garbage")),
                            ("baz", v_int(&o, 3)),
                        ],
                    ),
                ),
            ],
        ),
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
    let o = ops();
    // The lowercasing key codec collapses "foo" and "FOO" onto the same key.
    let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
    let input = v_map(&o, vec![("foo", v_int(&o, 1)), ("FOO", v_int(&o, 2))]);
    o.assert_from_java_fails(&map_codec, &input);
    // The duplicate surfaces as a 'Duplicate entry for key' error regardless of
    // map iteration order.
    let msg = o.from_java_error_message(&map_codec, &input);
    assert!(
        msg.contains("Duplicate entry for key: 'foo'"),
        "expected a duplicate-key error, got: {msg}"
    );
}

// Deferred: `unbounded_map_repeated_keys_partial` (in the TestOps/
// JsonOps suites) asserts which of two lowercasing-colliding keys ("foo"/"FOO")
// wins the partial — the *first in iteration order*. NbtOps map values are now
// an insertion-ordered `IndexMap`-backed `CompoundTag` (DECISIONS.md D12), so
// Rust's "first inserted wins" order is deterministic — but the colliding-key
// order is produced by `NbtOps`'s map-building, and this duplicate-key-partial
// surface is out of scope for the byte-identity gate. The order-insensitive
// surfaces (`unbounded_map_simple`, `unbounded_map_invalid_entry_partial`,
// `unbounded_map_invalid_entry_nested_partial`, `unbounded_map_repeated_keys`)
// still cover the unbounded-map decode path.

// ---------------------------------------------------------------------------
// withAlternative (CodecTests.withAlternative_*)
// ---------------------------------------------------------------------------

fn never<O: DynamicOps + 'static>(message: &'static str) -> StrCodec<O> {
    codec::validate(
        str_codec(),
        Arc::new(move |_: &String| DataResult::<String>::error(message)),
    )
}

fn never_with_partial<O: DynamicOps + 'static>(
    message: &'static str,
    partial_prefix: String,
) -> StrCodec<O> {
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
    let o = ops();
    let codec = codec::with_alternative(
        never_with_partial(
            "Failed Primary with partial",
            "Partial Primary: ".to_string(),
        ),
        never("Failed Alternative"),
    );
    assert_eq!(
        o.from_java_or_partial(&codec, &v_str(&o, "value")),
        "Partial Primary: value"
    );
    assert_eq!(
        o.from_java_error_message(&codec, &v_str(&o, "value")),
        "Failed Primary with partial"
    );
}

#[test]
fn with_alternative_primary_fails_alternative_partial() {
    let o = ops();
    let codec = codec::with_alternative(
        never("Failed Primary"),
        never_with_partial(
            "Failed Alternative with partial",
            "Partial Alternative: ".to_string(),
        ),
    );
    assert_eq!(
        o.from_java_or_partial(&codec, &v_str(&o, "value")),
        "Partial Alternative: value"
    );
    assert_eq!(
        o.from_java_error_message(&codec, &v_str(&o, "value")),
        "Failed Alternative with partial"
    );
}

#[test]
fn with_alternative_both_fail() {
    let o = ops();
    let codec = codec::with_alternative(never("Failed Primary"), never("Failed Alternative"));
    assert_eq!(
        o.from_java_error_message(&codec, &v_str(&o, "value")),
        "Failed to parse either. First: Failed Primary; Second: Failed Alternative"
    );
}

#[test]
fn with_alternative_both_successful() {
    let o = ops();
    let codec = codec::with_alternative(str_codec(), to_lower_case());
    o.assert_round_trip(&codec, "string".to_string(), v_str(&o, "string"));
    o.assert_round_trip(&codec, "STRING".to_string(), v_str(&o, "STRING"));
}

#[test]
fn with_alternative_both_partial_prefers_primary() {
    let o = ops();
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
        o.from_java_or_partial(&codec, &v_str(&o, "value")),
        "Partial Primary: value"
    );
}

// ---------------------------------------------------------------------------
// optionalField — strict vs lenient (CodecTests.optionalField_*)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct SimpleOptionals {
    string: Option<String>,
    integer: Option<i32>,
}

fn simple_optionals_codec<O: DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::Codec<SimpleOptionals, O>> {
    rivet_serialization::record_builder::create::<SimpleOptionals, O>(move |instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.string.clone()),
                codec::optional_field::<String, O>("string".to_string(), str_codec(), false),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.integer),
                codec::optional_field::<i32, O>("integer".to_string(), int_codec(), false),
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

fn simple_optionals_lenient_codec<O: DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::Codec<SimpleOptionals, O>> {
    rivet_serialization::record_builder::create::<SimpleOptionals, O>(move |instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.string.clone()),
                codec::optional_field::<String, O>("string".to_string(), str_codec(), true),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|o: &SimpleOptionals| o.integer),
                codec::optional_field::<i32, O>("integer".to_string(), int_codec(), true),
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
    let o = ops();
    for codec in [simple_optionals_codec(), simple_optionals_lenient_codec()] {
        o.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: Some("foo".into()),
                integer: Some(1),
            },
            v_map(
                &o,
                vec![("string", v_str(&o, "foo")), ("integer", v_int(&o, 1))],
            ),
        );
        o.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: None,
                integer: Some(1),
            },
            v_map(&o, vec![("integer", v_int(&o, 1))]),
        );
    }
}

#[test]
fn optional_field_strict_invalid_values() {
    let o = ops();
    let strict = simple_optionals_codec();
    // A DoubleTag is invalid as a string; a StringTag is invalid as an int.
    o.assert_from_java_fails(&strict, &v_map(&o, vec![("string", v_num(&o, 54.0))]));
    o.assert_from_java_fails(
        &strict,
        &v_map(&o, vec![("integer", v_str(&o, "not an int"))]),
    );
}

#[test]
fn optional_field_strict_invalid_values_partial() {
    let o = ops();
    let strict = simple_optionals_codec();
    // The invalid string field (a ByteTag boolean) is skipped; the valid int
    // field yields a partial — Java's strict optionalField keeps the valid
    // siblings and drops the invalid field's value.
    let partial = o.from_java_or_partial(
        &strict,
        &v_map(
            &o,
            vec![("string", v_bool(&o, false)), ("integer", v_int(&o, 23))],
        ),
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
    let o = ops();
    let lenient = simple_optionals_lenient_codec();
    let parsed = o.parse_or_throw(
        &lenient,
        &v_map(
            &o,
            vec![("string", v_bool(&o, false)), ("integer", v_int(&o, 23))],
        ),
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
// RecordCodecBuilder — round trip + error accumulation (not field order;
// see the file doc deferral note)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Simple {
    string: String,
    integer: i32,
}

fn simple_codec<O: DynamicOps + 'static>() -> Arc<dyn rivet_serialization::Codec<Simple, O>> {
    rivet_serialization::record_builder::create::<Simple, O>(move |instance| {
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
    let o = ops();
    let codec = simple_codec();
    o.assert_round_trip(
        &codec,
        Simple {
            string: "hello".into(),
            integer: 1,
        },
        v_map(
            &o,
            vec![("string", v_str(&o, "hello")), ("integer", v_int(&o, 1))],
        ),
    );
    // A record that is not a map fails.
    o.assert_from_java_fails(&codec, &v_str(&o, "not a map"));

    // Both fields invalid: errors accumulate (the apply2 message order).
    let result = codec.parse(
        &o,
        &v_map(
            &o,
            vec![("string", v_bool(&o, false)), ("integer", v_str(&o, "x"))],
        ),
    );
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(
        msg.contains("Not a string") && msg.contains("Not a number") && msg.contains(';'),
        "expected the two field errors accumulated, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// unit map codec + orElseGet + lifecycle
// ---------------------------------------------------------------------------

#[test]
fn unit_map_codec_encoding() {
    let o = ops();
    let marker = 42_i32;
    let codec = map_codec::unit_codec::<i32, _>(marker);
    o.assert_round_trip(&codec, marker, v_map(&o, Vec::new()));
}

#[test]
fn or_else_get_recovers_on_error() {
    let o = ops();
    let failing: StrCodec<NbtOps> = never("nope");
    let recovered = codec::or_else_value(failing.clone(), "fallback".to_string());
    let read = o.parse_or_throw(&recovered, &v_str(&o, "any"));
    assert_eq!(read, "fallback");
    o.assert_to_java_fails(&failing, &"x".to_string());
}

#[test]
fn stable_codec_round_trip_stays_stable() {
    let o = ops();
    let stable_int = codec::stable(int_codec());
    let result = stable_int.decode(&o, &v_int(&o, 5));
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}

#[test]
fn list_of_stable_elements_round_trip() {
    let o = ops();
    let stable_list = codec::list(codec::stable(str_codec()));
    let value = vec!["a".to_string(), "b".to_string()];
    let encoded = v_list(&o, vec![v_str(&o, "a"), v_str(&o, "b")]);
    o.assert_round_trip(&stable_list, value, encoded.clone());
    let result = stable_list.decode(&o, &encoded);
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}

// ---------------------------------------------------------------------------
// Dynamic / OptionalDynamic (DynamicTest)
// ---------------------------------------------------------------------------

#[test]
fn dynamic_get_and_as_number() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_map(&o, vec![("value", v_num(&o, 7.0))]));
    let got = dynamic.get(&o, "value");
    assert_eq!(
        got.result().and_then(|d| d.as_number(&o).result().copied()),
        Some(Number::Double(7.0))
    );
}

#[test]
fn dynamic_map_transforms_value() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_num(&o, 1.0));
    let mapped = dynamic.map(|v| match v {
        Tag::Double(t) => Tag::Double(rivet_nbt::double_tag::DoubleTag::value_of(t.value + 1.0)),
        other => other.clone(),
    });
    assert_eq!(mapped.get_value().canon(), v_num(&o, 2.0).canon());
}

#[test]
fn dynamic_get_missing_key_errors() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_map(&o, vec![("a", v_num(&o, 1.0))]));
    let missing = dynamic.get(&o, "b");
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
fn dynamic_remove_key() {
    let o = ops();
    let dynamic = Dynamic::new(
        &o,
        v_map(&o, vec![("a", v_num(&o, 1.0)), ("b", v_num(&o, 2.0))]),
    );
    let removed = dynamic.remove(&o, "a");
    assert_eq!(
        removed.get_value().canon(),
        v_map(&o, vec![("b", v_num(&o, 2.0))]).canon()
    );
}

#[test]
fn dynamic_set_key() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_map(&o, vec![("a", v_num(&o, 1.0))]));
    let value = Dynamic::new(&o, v_num(&o, 9.0));
    let updated = dynamic.set(&o, "a", &value);
    assert_eq!(
        updated.get_value().canon(),
        v_map(&o, vec![("a", v_num(&o, 9.0))]).canon()
    );
}

#[test]
fn dynamic_get_map_values_and_stream() {
    let o = ops();
    let dynamic = Dynamic::new(
        &o,
        v_map(&o, vec![("a", v_num(&o, 1.0)), ("b", v_num(&o, 2.0))]),
    );
    let map_values = dynamic.get_map_values(&o).result().cloned().unwrap();
    assert_eq!(map_values.len(), 2);

    let list = Dynamic::new(&o, v_list(&o, vec![v_num(&o, 1.0), v_num(&o, 2.0)]));
    let stream = list.as_stream_opt(&o).result().cloned().unwrap();
    assert_eq!(stream.len(), 2);
}

#[test]
fn dynamic_decode_via_decoder() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_int(&o, 3));
    let (value, _rest) = dynamic
        .decode(&o, int_codec().as_ref())
        .result()
        .cloned()
        .unwrap();
    assert_eq!(value, 3);
}

#[test]
fn optional_dynamic_get_field_chains() {
    let o = ops();
    let dynamic = Dynamic::new(
        &o,
        v_map(
            &o,
            vec![("outer", v_map(&o, vec![("inner", v_num(&o, 5.0))]))],
        ),
    );
    let outer: OptionalDynamic<Tag> = dynamic.get(&o, "outer");
    let inner = outer.get_field(&o, "inner");
    assert_eq!(
        inner
            .result()
            .and_then(|d| d.as_number(&o).result().copied()),
        Some(Number::Double(5.0))
    );
}

#[test]
fn optional_dynamic_or_else_empty_map() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_map(&o, Vec::new()));
    let missing = dynamic.get(&o, "nope");
    assert_eq!(
        missing.or_else_empty_map(&o).get_value().canon(),
        v_map(&o, Vec::new()).canon()
    );
    let present = Dynamic::new(&o, v_map(&o, vec![("a", v_num(&o, 1.0))])).get(&o, "a");
    assert_eq!(
        present.or_else_empty_map(&o).get_value().canon(),
        v_num(&o, 1.0).canon()
    );
}

#[test]
fn optional_dynamic_flat_map_through_delegate() {
    let o = ops();
    let dynamic = Dynamic::new(&o, v_map(&o, vec![("a", v_num(&o, 4.0))]));
    let field: OptionalDynamic<Tag> = dynamic.get(&o, "a");
    let doubled: DataResult<f64> =
        field.flat_map(|d| d.as_number(&o).map(|n| n.double_value() * 2.0));
    assert_eq!(doubled.result(), Some(&8.0));
}

// ---------------------------------------------------------------------------
// MapCodec — assumeMapUnsafe, unit, field decode (MapCodecTest)
// ---------------------------------------------------------------------------

#[test]
fn assume_map_unsafe_record_codec() {
    let o = ops();
    #[derive(Debug, Clone, PartialEq)]
    struct Simple {
        string: String,
        integer: i32,
    }
    let record = rivet_serialization::record_builder::create::<Simple, NbtOps>(move |instance| {
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
    });
    let assumed = map_codec::codec_of(map_codec::assume_map_unsafe(record));
    let value = Simple {
        string: "hello".into(),
        integer: 1,
    };
    let encoded = assumed
        .encode_start(&o, &value)
        .get_or_throw("encodeStart")
        .clone();
    // The wrapped record's fields are flattened into a `CompoundTag` — map
    // values are order-insensitive (HashMap-backed), so compare sorted like the
    // JsonOps variant.
    assert_eq!(
        encoded.canon().sorted(),
        v_map(
            &o,
            vec![("string", v_str(&o, "hello")), ("integer", v_int(&o, 1))]
        )
        .canon()
        .sorted()
    );
    let decoded = assumed.parse(&o, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, value);
    o.assert_from_java_fails(&assumed, &v_str(&o, "not a map"));
}

#[test]
fn assume_map_unsafe_primitive_codec_fails() {
    let o = ops();
    let int: Arc<dyn rivet_serialization::Codec<i32, NbtOps>> = int_codec();
    let codec = map_codec::codec_of(map_codec::assume_map_unsafe(int));
    o.assert_from_java_fails(&codec, &v_num(&o, 123.0));
    o.assert_to_java_fails(&codec, &123);
}

#[test]
fn unit_map_codec_decodes_any_map() {
    let o = ops();
    let unit = map_codec::unit_codec::<i32, NbtOps>(42);
    assert_eq!(
        unit.parse(&o, &v_map(&o, vec![("a", v_num(&o, 1.0))]))
            .result(),
        Some(&42)
    );
    assert_eq!(unit.parse(&o, &v_map(&o, Vec::new())).result(), Some(&42));
}

/// `MapLike<Tag>` over `CompoundTag` backed by an explicit entry list,
/// mirroring `TestMapLike` for the NbtOps backend (used by the field decoder).
#[derive(Debug)]
struct NbtMapLike(Vec<Pair<Tag, Tag>>);

impl MapLike<Tag> for NbtMapLike {
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

    fn entries(&self) -> Vec<Pair<Tag, Tag>> {
        self.0.clone()
    }
}

#[test]
fn field_decoder_missing_key_errors() {
    let o = ops();
    let field = map_decoder::field_decoder::<i32, NbtOps>("x".to_string(), int_codec());
    let missing = field.decode(&o, &NbtMapLike(Vec::new()));
    assert!(missing.is_error());
    assert!(
        missing.error_ref().unwrap().message().contains("No key x"),
        "expected a 'No key x' error"
    );

    let present = field.decode(
        &o,
        &NbtMapLike(vec![Pair::of(v_str(&o, "x"), v_int(&o, 5))]),
    );
    assert_eq!(present.result(), Some(&5));
}

// ---------------------------------------------------------------------------
// NbtOps-specific byte-array surfaces through the generic codecs
// (the ops-surface prerequisite this sub-issue resolves)
// ---------------------------------------------------------------------------

/// `Codec.BYTE_BUFFER` round-trips a `ByteArrayTag` through NbtOps (encode →
/// `createByteList` → `ByteArrayTag`, decode → `getByteBuffer` fast-path).
#[test]
fn byte_buffer_codec_round_trip() {
    let o = ops();
    let byte_buffer = codec::byte_buffer_codec::<NbtOps>();
    let value = vec![0u8, 1u8, 127u8, 128u8, 255u8];
    o.assert_round_trip(
        &byte_buffer,
        value,
        o.create_byte_list(&[0u8, 1u8, 127u8, 128u8, 255u8]),
    );
}

/// `Codec.INT_STREAM`/`LONG_STREAM` round-trip the i32/i64 boundaries.
#[test]
fn int_and_long_stream_codecs_round_trip() {
    let o = ops();
    let int_stream = codec::int_stream_codec::<NbtOps>();
    let value = vec![i32::MIN, -1, 0, 1, i32::MAX];
    o.assert_round_trip(&int_stream, value.clone(), o.create_int_list(value.clone()));

    let long_stream = codec::long_stream_codec::<NbtOps>();
    let value = vec![i64::MIN, -1, 0, 1, i64::MAX];
    o.assert_round_trip(
        &long_stream,
        value.clone(),
        o.create_long_list(value.clone()),
    );
}

/// The SNBT rendering of an NBT value used in `DataResult` messages.
#[test]
fn snbt_renders_tags_for_error_messages() {
    assert_eq!(
        snbt(&Tag::Byte(rivet_nbt::byte_tag::ByteTag::value_of(1))),
        "1b"
    );
    assert_eq!(
        snbt(&Tag::String(rivet_nbt::string_tag::StringTag::value_of(
            "x".to_string()
        ))),
        "\"x\""
    );
    assert_eq!(snbt(&Tag::Compound(CompoundTag::new())), "{}");
}
