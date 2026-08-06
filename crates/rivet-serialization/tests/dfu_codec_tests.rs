//! DFU-mirroring tests for the ported `com.mojang.serialization` codec surface.
//!
//! Ports a meaningful subset of the upstream `com.mojang.serialization.CodecTests`
//! (Mojang/DataFixerUpper master) and runs it ops-parametrically against three
//! backends: the minimal `TestOps` DynamicOps, `JsonOps::INSTANCE` and
//! `JsonOps::COMPRESSED`. Coverage: concrete-codec round trips, error/partial
//! semantics, RecordCodecBuilder field accumulation and field order,
//! optional-field strict/lenient behavior, lifecycle propagation, and the
//! `orElse`/`orElseGet` result functions.
//!
//! Expected/input values are built through the ops (never by pattern-matching a
//! concrete output type), so the identical test body runs against each backend.
//! Map equality is order-insensitive; field order is asserted explicitly where
//! the port guarantees it.
//!
//! `JsonOps::COMPRESSED` sets `compressMaps()`, but the port's compressed-map
//! path is `STUB(dfu.compressed-map)`: a faithful COMPRESSED encode goes through
//! `MapEncoder.compressedBuilder` (a `KeyCompressor`-backed builder producing a
//! packed array) and decode through `MapDecoder.compressedDecode` (`getList` +
//! `KeyCompressor`), neither of which is ported — the port always builds via
//! `ops.map_builder()` and reads via `get_map`. Record/map codec tests whose
//! faithful Java path requires `KeyCompressor`/`compressedBuilder`/
//! `compressedDecode` therefore run only against the non-compressed `INSTANCE`
//! backend, and each such COMPRESSED omission is marked `// STUB(dfu.compressed-map)`,
//! tracked by the dedicated compressed-map sub-issue under epic #6. `COMPRESSED`
//! remains only for surfaces proven faithful without a `KeyCompressor`:
//! `Codec`-level map codecs that bypass `MapCodecCodec` (`unboundedMap` uses
//! `getMap`/`mapBuilder` directly in Java), `unitCodec` (checks
//! `getList` when `compressMaps()`), and list/scalar surfaces.

mod common;

use common::{OpsTestExt, TestOps, ordered_map_keys, v_bool, v_int, v_list, v_map, v_num, v_str};
use rivet_serialization::DataResult;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::collections::HashMap;
use std::sync::Arc;

type StrCodec<O> = Arc<dyn rivet_serialization::Codec<String, O>>;
type IntCodec<O> = Arc<dyn rivet_serialization::Codec<i32, O>>;

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

/// The TestOps backend (a concrete type — `DynamicOps` is not object-safe, so
/// the loop cannot be over `dyn CanonOps`).
const BACKENDS: [&TestOps; 1] = [&TestOps];

/// The JsonOps backends (`INSTANCE` + `COMPRESSED`; both are the same
/// `JsonOps` type, so they share one generic loop).
const JSON_BACKENDS: [JsonOps; 2] = [JsonOps::INSTANCE, JsonOps::COMPRESSED];

// ---------------------------------------------------------------------------
// ListCodec (CodecTests.list_roundTrip / list_invalidValues)
// ---------------------------------------------------------------------------

#[test]
fn list_round_trip() {
    for ops in BACKENDS {
        let list = codec::list(str_codec());
        let value = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
        let encoded = v_list(
            ops,
            vec![v_str(ops, "foo"), v_str(ops, "bar"), v_str(ops, "baz")],
        );
        ops.assert_round_trip(&list, value, encoded);
    }
}

#[test]
fn list_round_trip_through_json() {
    for ops in JSON_BACKENDS {
        let list = codec::list(str_codec());
        let value = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
        let encoded = v_list(
            &ops,
            vec![v_str(&ops, "foo"), v_str(&ops, "bar"), v_str(&ops, "baz")],
        );
        ops.assert_round_trip(&list, value, encoded);
    }
}

#[test]
fn list_invalid_values() {
    let ops = TestOps;
    let list = codec::list(str_codec());

    // assertFromJavaFails: mixed list with a non-string
    let bad = v_list(
        &ops,
        vec![
            v_str(&ops, "foo"),
            v_int(&ops, 2),
            v_str(&ops, "baz"),
            v_bool(&ops, false),
        ],
    );
    ops.assert_from_java_fails(&list, &bad);

    // partial keeps the valid prefix up to (but excluding) the invalid value
    let partial1 = ops.from_java_or_partial(
        &list,
        &v_list(
            &ops,
            vec![
                v_str(&ops, "foo"),
                v_str(&ops, "bar"),
                v_int(&ops, 2),
                v_bool(&ops, false),
            ],
        ),
    );
    assert_eq!(partial1, vec!["foo".to_string(), "bar".to_string()]);

    let partial2 = ops.from_java_or_partial(
        &list,
        &v_list(
            &ops,
            vec![
                v_str(&ops, "foo"),
                v_int(&ops, 2),
                v_str(&ops, "baz"),
                v_bool(&ops, false),
            ],
        ),
    );
    assert_eq!(partial2, vec!["foo".to_string(), "baz".to_string()]);
}

#[test]
fn list_invalid_values_through_json() {
    for ops in JSON_BACKENDS {
        // Invalid elements are booleans, which no JsonOps mode accepts as a
        // string (`COMPRESSED` tolerates numbers via `getAsString`, so a number
        // would be a valid element there).
        let list = codec::list(str_codec());
        // assertFromJavaFails: mixed list with a non-string.
        let bad = v_list(
            &ops,
            vec![
                v_str(&ops, "foo"),
                v_bool(&ops, false),
                v_str(&ops, "baz"),
                v_bool(&ops, true),
            ],
        );
        ops.assert_from_java_fails(&list, &bad);

        // partial keeps the valid prefix up to (but excluding) the invalid value.
        let partial1 = ops.from_java_or_partial(
            &list,
            &v_list(
                &ops,
                vec![
                    v_str(&ops, "foo"),
                    v_str(&ops, "bar"),
                    v_bool(&ops, false),
                    v_bool(&ops, true),
                ],
            ),
        );
        assert_eq!(partial1, vec!["foo".to_string(), "bar".to_string()]);

        let partial2 = ops.from_java_or_partial(
            &list,
            &v_list(
                &ops,
                vec![
                    v_str(&ops, "foo"),
                    v_bool(&ops, false),
                    v_str(&ops, "baz"),
                    v_bool(&ops, true),
                ],
            ),
        );
        assert_eq!(partial2, vec!["foo".to_string(), "baz".to_string()]);
    }
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
        v_list(&ops, vec![v_str(&ops, "foo"), v_str(&ops, "bar")]),
    );
}

#[test]
fn size_limited_list_round_trip_through_json() {
    for ops in JSON_BACKENDS {
        let limited = codec::list_with_range(str_codec(), 2, 2);
        ops.assert_round_trip(
            &limited,
            vec!["foo".to_string(), "bar".to_string()],
            v_list(&ops, vec![v_str(&ops, "foo"), v_str(&ops, "bar")]),
        );
    }
}

#[test]
fn size_limited_list_too_long() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 2);
    let three = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
    let input = v_list(
        &ops,
        vec![v_str(&ops, "foo"), v_str(&ops, "bar"), v_str(&ops, "baz")],
    );

    ops.assert_from_java_fails(&limited, &input);
    ops.assert_to_java_fails(&limited, &three);

    // Input is clipped in the partial result to the max size.
    let partial = ops.from_java_or_partial(&limited, &input);
    assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn size_limited_list_too_long_through_json() {
    for ops in JSON_BACKENDS {
        let limited = codec::list_with_range(str_codec(), 2, 2);
        let three = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
        let input = v_list(
            &ops,
            vec![v_str(&ops, "foo"), v_str(&ops, "bar"), v_str(&ops, "baz")],
        );

        ops.assert_from_java_fails(&limited, &input);
        ops.assert_to_java_fails(&limited, &three);

        // Input is clipped in the partial result to the max size.
        let partial = ops.from_java_or_partial(&limited, &input);
        assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
    }
}

#[test]
fn size_limited_list_too_long_with_invalid() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 2);
    // Input is clipped only by valid entries (invalid entries do not count
    // toward the size).
    let partial = ops.from_java_or_partial(
        &limited,
        &v_list(
            &ops,
            vec![
                v_str(&ops, "foo"),
                v_int(&ops, 2),
                v_str(&ops, "bar"),
                v_str(&ops, "baz"),
                v_bool(&ops, false),
            ],
        ),
    );
    assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn size_limited_list_too_long_with_invalid_through_json() {
    for ops in JSON_BACKENDS {
        let limited = codec::list_with_range(str_codec(), 2, 2);
        // Input is clipped only by valid entries (invalid entries do not count
        // toward the size). Booleans are invalid as strings in every JsonOps
        // mode (`COMPRESSED` tolerates numbers), keeping both backends aligned.
        let partial = ops.from_java_or_partial(
            &limited,
            &v_list(
                &ops,
                vec![
                    v_str(&ops, "foo"),
                    v_bool(&ops, false),
                    v_str(&ops, "bar"),
                    v_str(&ops, "baz"),
                    v_bool(&ops, true),
                ],
            ),
        );
        assert_eq!(partial, vec!["foo".to_string(), "bar".to_string()]);
    }
}

#[test]
fn size_limited_list_too_short() {
    let ops = TestOps;
    let limited = codec::list_with_range(str_codec(), 2, 3);
    ops.assert_to_java_fails(&limited, &vec!["foo".to_string()]);
    // No partial can be obtained when the data is too short.
    ops.assert_from_java_fails_partial(&limited, &v_list(&ops, vec![v_str(&ops, "foo")]));

    ops.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string()],
        v_list(&ops, vec![v_str(&ops, "foo"), v_str(&ops, "bar")]),
    );
    ops.assert_round_trip(
        &limited,
        vec!["foo".to_string(), "bar".to_string(), "baz".to_string()],
        v_list(
            &ops,
            vec![v_str(&ops, "foo"), v_str(&ops, "bar"), v_str(&ops, "baz")],
        ),
    );
}

#[test]
fn size_limited_list_too_short_through_json() {
    for ops in JSON_BACKENDS {
        let limited = codec::list_with_range(str_codec(), 2, 3);
        ops.assert_to_java_fails(&limited, &vec!["foo".to_string()]);
        // No partial can be obtained when the data is too short.
        ops.assert_from_java_fails_partial(&limited, &v_list(&ops, vec![v_str(&ops, "foo")]));

        ops.assert_round_trip(
            &limited,
            vec!["foo".to_string(), "bar".to_string()],
            v_list(&ops, vec![v_str(&ops, "foo"), v_str(&ops, "bar")]),
        );
        ops.assert_round_trip(
            &limited,
            vec!["foo".to_string(), "bar".to_string(), "baz".to_string()],
            v_list(
                &ops,
                vec![v_str(&ops, "foo"), v_str(&ops, "bar"), v_str(&ops, "baz")],
            ),
        );
    }
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
    let encoded = v_map(&ops, vec![("foo", v_int(&ops, 1)), ("bar", v_int(&ops, 2))]);
    ops.assert_round_trip(&map_codec, value, encoded);
}

#[test]
fn unbounded_map_simple_through_json() {
    for ops in JSON_BACKENDS {
        let map_codec = codec::unbounded_map(str_codec(), int_codec());
        let mut value = HashMap::new();
        value.insert("foo".to_string(), 1);
        value.insert("bar".to_string(), 2);
        let encoded = v_map(&ops, vec![("foo", v_int(&ops, 1)), ("bar", v_int(&ops, 2))]);
        ops.assert_round_trip(&map_codec, value, encoded);
    }
}

#[test]
fn unbounded_map_invalid_entry() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let input = v_map(
        &ops,
        vec![
            ("foo", v_int(&ops, 1)),
            ("bar", v_str(&ops, "garbage")),
            ("baz", v_int(&ops, 3)),
        ],
    );
    ops.assert_from_java_fails(&map_codec, &input);
}

#[test]
fn unbounded_map_invalid_entry_through_json() {
    for ops in JSON_BACKENDS {
        let map_codec = codec::unbounded_map(str_codec(), int_codec());
        let input = v_map(
            &ops,
            vec![
                ("foo", v_int(&ops, 1)),
                ("bar", v_str(&ops, "garbage")),
                ("baz", v_int(&ops, 3)),
            ],
        );
        ops.assert_from_java_fails(&map_codec, &input);
    }
}

#[test]
fn unbounded_map_invalid_entry_partial() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(str_codec(), int_codec());
    let partial = ops.from_java_or_partial(
        &map_codec,
        &v_map(
            &ops,
            vec![
                ("foo", v_int(&ops, 1)),
                ("bar", v_str(&ops, "garbage")),
                ("baz", v_int(&ops, 3)),
            ],
        ),
    );
    let mut expected = HashMap::new();
    expected.insert("foo".to_string(), 1);
    expected.insert("baz".to_string(), 3);
    assert_eq!(partial, expected);
}

#[test]
fn unbounded_map_invalid_entry_partial_through_json() {
    for ops in JSON_BACKENDS {
        let map_codec = codec::unbounded_map(str_codec(), int_codec());
        let partial = ops.from_java_or_partial(
            &map_codec,
            &v_map(
                &ops,
                vec![
                    ("foo", v_int(&ops, 1)),
                    ("bar", v_str(&ops, "garbage")),
                    ("baz", v_int(&ops, 3)),
                ],
            ),
        );
        let mut expected = HashMap::new();
        expected.insert("foo".to_string(), 1);
        expected.insert("baz".to_string(), 3);
        assert_eq!(partial, expected);
    }
}

#[test]
fn unbounded_map_invalid_entry_nested_partial() {
    let ops = TestOps;
    let inner = codec::unbounded_map(str_codec(), int_codec());
    let outer = codec::unbounded_map(str_codec(), inner);
    let partial = ops.from_java_or_partial(
        &outer,
        &v_map(
            &ops,
            vec![
                ("foo", v_map(&ops, vec![("foo", v_int(&ops, 1))])),
                (
                    "bar",
                    v_map(
                        &ops,
                        vec![
                            ("foo", v_int(&ops, 1)),
                            ("bar", v_str(&ops, "garbage")),
                            ("baz", v_int(&ops, 3)),
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
fn unbounded_map_invalid_entry_nested_partial_through_json() {
    for ops in JSON_BACKENDS {
        let inner = codec::unbounded_map(str_codec(), int_codec());
        let outer = codec::unbounded_map(str_codec(), inner);
        let partial = ops.from_java_or_partial(
            &outer,
            &v_map(
                &ops,
                vec![
                    ("foo", v_map(&ops, vec![("foo", v_int(&ops, 1))])),
                    (
                        "bar",
                        v_map(
                            &ops,
                            vec![
                                ("foo", v_int(&ops, 1)),
                                ("bar", v_str(&ops, "garbage")),
                                ("baz", v_int(&ops, 3)),
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
}

#[test]
fn unbounded_map_repeated_keys() {
    let ops = TestOps;
    // The lowercasing key codec collapses "foo" and "FOO" onto the same key.
    let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
    let input = v_map(&ops, vec![("foo", v_int(&ops, 1)), ("FOO", v_int(&ops, 2))]);
    ops.assert_from_java_fails(&map_codec, &input);
}

#[test]
fn unbounded_map_repeated_keys_through_json() {
    for ops in JSON_BACKENDS {
        let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
        let input = v_map(&ops, vec![("foo", v_int(&ops, 1)), ("FOO", v_int(&ops, 2))]);
        ops.assert_from_java_fails(&map_codec, &input);
    }
}

#[test]
fn unbounded_map_repeated_keys_partial() {
    let ops = TestOps;
    let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
    let partial = ops.from_java_or_partial(
        &map_codec,
        &v_map(
            &ops,
            vec![
                ("foo", v_int(&ops, 1)),
                ("bar", v_int(&ops, 2)),
                ("FOO", v_int(&ops, 3)),
            ],
        ),
    );
    // The first entry wins for the partial result.
    let mut expected = HashMap::new();
    expected.insert("foo".to_string(), 1);
    expected.insert("bar".to_string(), 2);
    assert_eq!(partial, expected);
}

#[test]
fn unbounded_map_repeated_keys_partial_through_json() {
    for ops in JSON_BACKENDS {
        let map_codec = codec::unbounded_map(to_lower_case(), int_codec());
        let partial = ops.from_java_or_partial(
            &map_codec,
            &v_map(
                &ops,
                vec![
                    ("foo", v_int(&ops, 1)),
                    ("bar", v_int(&ops, 2)),
                    ("FOO", v_int(&ops, 3)),
                ],
            ),
        );
        // The first entry wins for the partial result.
        let mut expected = HashMap::new();
        expected.insert("foo".to_string(), 1);
        expected.insert("bar".to_string(), 2);
        assert_eq!(partial, expected);
    }
}

// ---------------------------------------------------------------------------
// either / xor (CodecTests.withAlternative_*)
// ---------------------------------------------------------------------------

/// A `Codec<String>` that always fails with the given message.
fn never<O: DynamicOps + 'static>(message: &'static str) -> StrCodec<O> {
    codec::validate(
        str_codec(),
        Arc::new(move |_: &String| DataResult::<String>::error(message)),
    )
}

/// A `Codec<String>` that always fails with the given message AND a partial
/// equal to `partial_prefix` + the input value (mirroring the upstream
/// `NEVER_WITH_PARTIAL_*` constants).
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
    let ops = TestOps;
    let codec = codec::with_alternative(
        never_with_partial(
            "Failed Primary with partial",
            "Partial Primary: ".to_string(),
        ),
        never("Failed Alternative"),
    );
    assert_eq!(
        ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
        "Partial Primary: value"
    );
    assert_eq!(
        ops.from_java_error_message(&codec, &v_str(&ops, "value")),
        "Failed Primary with partial"
    );
}

#[test]
fn with_alternative_primary_partial_alternative_fails_through_json() {
    for ops in JSON_BACKENDS {
        let codec = codec::with_alternative(
            never_with_partial(
                "Failed Primary with partial",
                "Partial Primary: ".to_string(),
            ),
            never("Failed Alternative"),
        );
        assert_eq!(
            ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
            "Partial Primary: value"
        );
        assert_eq!(
            ops.from_java_error_message(&codec, &v_str(&ops, "value")),
            "Failed Primary with partial"
        );
    }
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
        ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
        "Partial Alternative: value"
    );
    assert_eq!(
        ops.from_java_error_message(&codec, &v_str(&ops, "value")),
        "Failed Alternative with partial"
    );
}

#[test]
fn with_alternative_primary_fails_alternative_partial_through_json() {
    for ops in JSON_BACKENDS {
        let codec = codec::with_alternative(
            never("Failed Primary"),
            never_with_partial(
                "Failed Alternative with partial",
                "Partial Alternative: ".to_string(),
            ),
        );
        assert_eq!(
            ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
            "Partial Alternative: value"
        );
        assert_eq!(
            ops.from_java_error_message(&codec, &v_str(&ops, "value")),
            "Failed Alternative with partial"
        );
    }
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
        ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
        "Partial Primary: value"
    );
}

#[test]
fn with_alternative_both_partial_prefers_primary_through_json() {
    for ops in JSON_BACKENDS {
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
            ops.from_java_or_partial(&codec, &v_str(&ops, "value")),
            "Partial Primary: value"
        );
    }
}

#[test]
fn with_alternative_both_fail() {
    let ops = TestOps;
    let codec = codec::with_alternative(never("Failed Primary"), never("Failed Alternative"));
    assert_eq!(
        ops.from_java_error_message(&codec, &v_str(&ops, "value")),
        "Failed to parse either. First: Failed Primary; Second: Failed Alternative"
    );
}

#[test]
fn with_alternative_both_fail_through_json() {
    for ops in JSON_BACKENDS {
        let codec = codec::with_alternative(never("Failed Primary"), never("Failed Alternative"));
        assert_eq!(
            ops.from_java_error_message(&codec, &v_str(&ops, "value")),
            "Failed to parse either. First: Failed Primary; Second: Failed Alternative"
        );
    }
}

#[test]
fn with_alternative_both_successful() {
    let ops = TestOps;
    let codec = codec::with_alternative(str_codec(), to_lower_case());
    ops.assert_round_trip(&codec, "string".to_string(), v_str(&ops, "string"));
    // Primary codec is chosen over the alternative.
    ops.assert_round_trip(&codec, "STRING".to_string(), v_str(&ops, "STRING"));
}

#[test]
fn with_alternative_both_successful_through_json() {
    for ops in JSON_BACKENDS {
        let codec = codec::with_alternative(str_codec(), to_lower_case());
        ops.assert_round_trip(&codec, "string".to_string(), v_str(&ops, "string"));
        // Primary codec is chosen over the alternative.
        ops.assert_round_trip(&codec, "STRING".to_string(), v_str(&ops, "STRING"));
    }
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
    let ops = TestOps;
    for codec in [simple_optionals_codec(), simple_optionals_lenient_codec()] {
        ops.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: Some("foo".into()),
                integer: Some(1),
            },
            v_map(
                &ops,
                vec![("string", v_str(&ops, "foo")), ("integer", v_int(&ops, 1))],
            ),
        );
        ops.assert_round_trip(
            &codec,
            SimpleOptionals {
                string: None,
                integer: Some(1),
            },
            v_map(&ops, vec![("integer", v_int(&ops, 1))]),
        );
    }
}

#[test]
fn optional_field_round_trip_through_json() {
    // STUB(dfu.compressed-map): `optionalField` is a `MapCodec`, so a faithful
    // COMPRESSED round trip needs `compressedBuilder`/`compressedDecode` (a
    // `KeyCompressor`-backed packed list), which is not ported. The map-form
    // round trip is asserted only against `INSTANCE`; the COMPRESSED case is
    // tracked by the dedicated compressed-map sub-issue (epic #6).
    let ops = JsonOps::INSTANCE;
    {
        for codec in [simple_optionals_codec(), simple_optionals_lenient_codec()] {
            ops.assert_round_trip(
                &codec,
                SimpleOptionals {
                    string: Some("foo".into()),
                    integer: Some(1),
                },
                v_map(
                    &ops,
                    vec![("string", v_str(&ops, "foo")), ("integer", v_int(&ops, 1))],
                ),
            );
            ops.assert_round_trip(
                &codec,
                SimpleOptionals {
                    string: None,
                    integer: Some(1),
                },
                v_map(&ops, vec![("integer", v_int(&ops, 1))]),
            );
        }
    }
}

#[test]
fn optional_field_strict_invalid_values() {
    let ops = TestOps;
    let strict = simple_optionals_codec();
    ops.assert_from_java_fails(&strict, &v_map(&ops, vec![("string", v_num(&ops, 54.0))]));
    ops.assert_from_java_fails(
        &strict,
        &v_map(&ops, vec![("integer", v_str(&ops, "not an int"))]),
    );
}

#[test]
fn optional_field_strict_invalid_values_through_json() {
    // STUB(dfu.compressed-map): `optionalField` is a `MapCodec`; a faithful
    // COMPRESSED decode of a map first calls `compressedDecode` → `getList`,
    // which fails on an object with "Input is not a list" before any field
    // validation runs. The map-form field-validation semantics asserted here
    // therefore hold only for the non-compressed `INSTANCE` backend (the
    // port's always-map `compressed_decode` fallback is not asserted); the
    // COMPRESSED case is tracked by the dedicated compressed-map sub-issue
    // (epic #6).
    let ops = JsonOps::INSTANCE;
    let strict = simple_optionals_codec();
    // A boolean is invalid as a string (`COMPRESSED` tolerates numbers, so a
    // number would decode to `Some("54.0")` and not fail).
    ops.assert_from_java_fails(&strict, &v_map(&ops, vec![("string", v_bool(&ops, false))]));
    ops.assert_from_java_fails(
        &strict,
        &v_map(&ops, vec![("integer", v_str(&ops, "not an int"))]),
    );
}

#[test]
fn optional_field_strict_invalid_values_partial() {
    let ops = TestOps;
    let strict = simple_optionals_codec();
    let partial = ops.from_java_or_partial(
        &strict,
        &v_map(
            &ops,
            vec![
                ("string", v_bool(&ops, false)),
                ("integer", v_int(&ops, 23)),
            ],
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
fn optional_field_strict_invalid_values_partial_through_json() {
    // STUB(dfu.compressed-map): a faithful COMPRESSED decode of this map fails
    // in `compressedDecode` → `getList` ("Input is not a list") before any
    // field validation, so no partial is produced; the partial result asserted
    // here is the non-compressed `INSTANCE` behavior. The port's always-map
    // `compressed_decode` fallback is not asserted; the COMPRESSED case is
    // tracked by the dedicated compressed-map sub-issue (epic #6).
    let ops = JsonOps::INSTANCE;
    let strict = simple_optionals_codec();
    let partial = ops.from_java_or_partial(
        &strict,
        &v_map(
            &ops,
            vec![
                ("string", v_bool(&ops, false)),
                ("integer", v_int(&ops, 23)),
            ],
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
    let ops = TestOps;
    let lenient = simple_optionals_lenient_codec();
    let parsed = ops.parse_or_throw(
        &lenient,
        &v_map(
            &ops,
            vec![
                ("string", v_bool(&ops, false)),
                ("integer", v_int(&ops, 23)),
            ],
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

#[test]
fn optional_field_lenient_invalid_values_through_json() {
    // STUB(dfu.compressed-map): a faithful COMPRESSED decode of this map fails
    // in `compressedDecode` → `getList` ("Input is not a list") before the
    // lenient field logic runs; the lenient parse asserted here is the
    // non-compressed `INSTANCE` behavior. The port's always-map
    // `compressed_decode` fallback is not asserted; the COMPRESSED case is
    // tracked by the dedicated compressed-map sub-issue (epic #6).
    let ops = JsonOps::INSTANCE;
    let lenient = simple_optionals_lenient_codec();
    let parsed = ops.parse_or_throw(
        &lenient,
        &v_map(
            &ops,
            vec![
                ("string", v_bool(&ops, false)),
                ("integer", v_int(&ops, 23)),
            ],
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
// RecordCodecBuilder — error accumulation + field order
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
    let ops = TestOps;
    let codec = simple_codec();
    ops.assert_round_trip(
        &codec,
        Simple {
            string: "hello".into(),
            integer: 1,
        },
        v_map(
            &ops,
            vec![
                ("string", v_str(&ops, "hello")),
                ("integer", v_int(&ops, 1)),
            ],
        ),
    );

    // A record that is not a map fails.
    ops.assert_from_java_fails(&codec, &v_str(&ops, "not a map"));

    // Both fields invalid: errors accumulate (the apply2 message order).
    let result = codec.parse(
        &ops,
        &v_map(
            &ops,
            vec![("string", v_num(&ops, 1.0)), ("integer", v_str(&ops, "x"))],
        ),
    );
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(
        msg.contains("Not a string") && msg.contains("Not a number") && msg.contains(';'),
        "expected the two field errors accumulated, got: {msg}"
    );
}

#[test]
fn record_codec_round_trip_and_error_accumulation_through_json() {
    // STUB(dfu.compressed-map): faithful `JsonOps.COMPRESSED` encodes a record
    // through `MapCodecCodec.encode` → `MapEncoder.compressedBuilder` (a
    // `KeyCompressor`-backed packed array), not an object map, and decodes via
    // `compressedDecode` (`getList` + `KeyCompressor`). Neither is ported, so the
    // map-form round trip is exercised only against `INSTANCE`; the COMPRESSED
    // case is tracked by the dedicated compressed-map sub-issue (epic #6).
    let ops = JsonOps::INSTANCE;
    {
        let codec = simple_codec();
        ops.assert_round_trip(
            &codec,
            Simple {
                string: "hello".into(),
                integer: 1,
            },
            v_map(
                &ops,
                vec![
                    ("string", v_str(&ops, "hello")),
                    ("integer", v_int(&ops, 1)),
                ],
            ),
        );

        // A record that is not a map fails.
        ops.assert_from_java_fails(&codec, &v_str(&ops, "not a map"));

        // Both fields invalid: errors accumulate (the apply2 message order).
        // The string field uses a boolean so it is invalid in every JsonOps
        // mode (`COMPRESSED` tolerates numbers as strings).
        let result = codec.parse(
            &ops,
            &v_map(
                &ops,
                vec![
                    ("string", v_bool(&ops, false)),
                    ("integer", v_str(&ops, "x")),
                ],
            ),
        );
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Not a string") && msg.contains("Not a number") && msg.contains(';'),
            "expected the two field errors accumulated, got: {msg}"
        );
    }
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
fn record_with_4_fields_codec<O: DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::Codec<RecordWith4Fields, O>> {
    rivet_serialization::record_builder::create::<RecordWith4Fields, O>(move |instance| {
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
    assert_eq!(
        ordered_map_keys(&encoded),
        vec![
            "f1".to_string(),
            "f2".to_string(),
            "f3".to_string(),
            "f4".to_string()
        ],
        "encoded record must keep field order"
    );
}

#[test]
fn record_codec_maintains_field_order_through_json() {
    // STUB(dfu.compressed-map): faithful `JsonOps.COMPRESSED` encodes a record
    // through `compressedBuilder` (a `KeyCompressor`-backed packed array), not
    // an object, so `ordered_map_keys` is inapplicable there. Field order in map
    // form is asserted only against the non-compressed `INSTANCE` backend; the
    // COMPRESSED case is tracked by the dedicated compressed-map sub-issue
    // (epic #6).
    let ops = JsonOps::INSTANCE;
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
    assert_eq!(
        ordered_map_keys(&encoded),
        vec![
            "f1".to_string(),
            "f2".to_string(),
            "f3".to_string(),
            "f4".to_string()
        ],
        "encoded record must keep field order"
    );
}

// ---------------------------------------------------------------------------
// unit map codec (CodecTests.unitMapCodecEncoding)
// ---------------------------------------------------------------------------

#[test]
fn unit_map_codec_encoding() {
    let ops = TestOps;
    let marker = 42_i32;
    let codec = map_codec::unit_codec::<i32, _>(marker);
    ops.assert_round_trip(&codec, marker, v_map(&ops, Vec::new()));
}

#[test]
fn unit_map_codec_encoding_through_json() {
    let marker = 42_i32;
    let codec = map_codec::unit_codec::<i32, JsonOps>(marker);
    // INSTANCE round-trips through the map form.
    JsonOps::INSTANCE.assert_round_trip(&codec, marker, v_map(&JsonOps::INSTANCE, Vec::new()));
    // COMPRESSED decodes through the packed-list form (`UnitCodec` only checks
    // the input shape — Java `compressMaps() ? getList : getMap` — and does
    // not implement a compressed encode, so a map round trip is impossible).
    // This is faithful without a `KeyCompressor`, so COMPRESSED coverage stays.
    let ops = JsonOps::COMPRESSED;
    assert_eq!(
        codec.parse(&ops, &v_list(&ops, Vec::new())).result(),
        Some(&marker)
    );
}

// ---------------------------------------------------------------------------
// mapResult / orElse (CodecTests.withAlternative_* exercises orElse; here the
// orElseGet result functions)
// ---------------------------------------------------------------------------

#[test]
fn or_else_get_recovers_on_error() {
    let ops = TestOps;
    // A codec that always errors; orElseGet supplies a fallback value.
    let failing: StrCodec<TestOps> = never("nope");
    let recovered = codec::or_else_value(failing.clone(), "fallback".to_string());

    // Reading a string still errors at the primary codec but recovers.
    let read = ops.parse_or_throw(&recovered, &v_str(&ops, "any"));
    assert_eq!(read, "fallback");

    // Encoding through the primary still fails (orElse only recovers decode).
    ops.assert_to_java_fails(&failing, &"x".to_string());
}

#[test]
fn or_else_get_recovers_on_error_through_json() {
    for ops in JSON_BACKENDS {
        // A codec that always errors; orElseGet supplies a fallback value.
        let failing: StrCodec<JsonOps> = never("nope");
        let recovered = codec::or_else_value(failing.clone(), "fallback".to_string());

        // Reading a string still errors at the primary codec but recovers.
        let read = ops.parse_or_throw(&recovered, &v_str(&ops, "any"));
        assert_eq!(read, "fallback");

        // Encoding through the primary still fails (orElse only recovers decode).
        ops.assert_to_java_fails(&failing, &"x".to_string());
    }
}

// ---------------------------------------------------------------------------
// lifecycle: stable/experimental propagation through combinators
// ---------------------------------------------------------------------------

#[test]
fn stable_codec_round_trip_stays_stable() {
    let ops = TestOps;
    let stable_int = codec::stable(int_codec());
    let result = stable_int.decode(&ops, &v_int(&ops, 5));
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}

#[test]
fn stable_codec_round_trip_stays_stable_through_json() {
    for ops in JSON_BACKENDS {
        let stable_int = codec::stable(int_codec());
        let result = stable_int.decode(&ops, &v_int(&ops, 5));
        assert_eq!(
            result.lifecycle(),
            rivet_serialization::lifecycle::Lifecycle::stable()
        );
    }
}

#[test]
fn list_of_stable_elements_round_trip() {
    let ops = TestOps;
    let stable_list = codec::list(codec::stable(str_codec()));
    let value = vec!["a".to_string(), "b".to_string()];
    let encoded = v_list(&ops, vec![v_str(&ops, "a"), v_str(&ops, "b")]);
    ops.assert_round_trip(&stable_list, value, encoded.clone());
    let result = stable_list.decode(&ops, &encoded);
    assert_eq!(
        result.lifecycle(),
        rivet_serialization::lifecycle::Lifecycle::stable()
    );
}

#[test]
fn list_of_stable_elements_round_trip_through_json() {
    for ops in JSON_BACKENDS {
        let stable_list = codec::list(codec::stable(str_codec()));
        let value = vec!["a".to_string(), "b".to_string()];
        let encoded = v_list(&ops, vec![v_str(&ops, "a"), v_str(&ops, "b")]);
        ops.assert_round_trip(&stable_list, value, encoded.clone());
        let result = stable_list.decode(&ops, &encoded);
        assert_eq!(
            result.lifecycle(),
            rivet_serialization::lifecycle::Lifecycle::stable()
        );
    }
}
