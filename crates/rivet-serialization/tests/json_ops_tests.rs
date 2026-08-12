//! Tests for `JsonOps` — a faithful port of `com.mojang.serialization.JsonOps`
//! as `DynamicOps<serde_json::Value>`.
//!
//! Round-trips `serde_json::Value` through DFU `Codec`s (encode → JSON →
//! decode), plus fidelity checks of the `INSTANCE` vs `COMPRESSED` compressed
//! modes and the merge/get surfaces.

use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike};
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::number::Number as TypedNumber;
use rivet_serialization::pair::Pair;
use serde_json::{Number, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// Panic on a `DataResult` error (test convenience — `DataResult` has no
/// `expect`/`unwrap` on the value).
trait ExpectResult {
    type Value;
    fn unwrap_result(self, message: &str) -> Self::Value;
}
impl<T: Clone> ExpectResult for DataResult<T> {
    type Value = T;
    fn unwrap_result(self, message: &str) -> Self::Value {
        self.get_or_throw(message).clone()
    }
}

/// Encode `input` with `codec` and decode the resulting JSON back.
fn round_trip<A, Ops>(ops: &Ops, codec: &Arc<dyn rivet_serialization::Codec<A, Ops>>, input: A) -> A
where
    A: Clone + 'static,
    Ops: DynamicOps + 'static,
{
    let encoded = codec.encode_start(ops, &input).unwrap_result("encode");
    let (decoded, _rest) = codec.decode(ops, &encoded).unwrap_result("decode");
    decoded
}

// ---------------------------------------------------------------------------
// Primitive codecs round-trip through JsonOps
// ---------------------------------------------------------------------------

#[test]
fn primitive_codecs_round_trip_through_json() {
    let ops = JsonOps::INSTANCE;

    let b: bool = round_trip(&ops, &rivet_serialization::codec::bool_codec(), true);
    assert!(b);

    let i: i32 = round_trip(&ops, &rivet_serialization::codec::int_codec(), 1234);
    assert_eq!(i, 1234);

    let l: i64 = round_trip(
        &ops,
        &rivet_serialization::codec::long_codec(),
        9_000_000_000,
    );
    assert_eq!(l, 9_000_000_000);

    let f: f32 = round_trip(&ops, &rivet_serialization::codec::float_codec(), 3.5);
    assert_eq!(f, 3.5);

    let d: f64 = round_trip(&ops, &rivet_serialization::codec::double_codec(), 0.1);
    assert_eq!(d, 0.1);

    let s: String = round_trip(
        &ops,
        &rivet_serialization::codec::string_codec(),
        "hello".to_string(),
    );
    assert_eq!(s, "hello");
}

#[test]
fn primitive_codecs_round_trip_through_compressed() {
    let ops = JsonOps::COMPRESSED;
    let i: i32 = round_trip(&ops, &rivet_serialization::codec::int_codec(), -42);
    assert_eq!(i, -42);
    let s: String = round_trip(
        &ops,
        &rivet_serialization::codec::string_codec(),
        "key".to_string(),
    );
    assert_eq!(s, "key");
}

// ---------------------------------------------------------------------------
// List / map codecs round-trip
// ---------------------------------------------------------------------------

#[test]
fn list_codec_round_trip() {
    let ops = JsonOps::INSTANCE;
    let int_list =
        rivet_serialization::codec::list::<i32, JsonOps>(rivet_serialization::codec::int_codec());
    let values: Vec<i32> = round_trip(&ops, &int_list, vec![1, 2, 3]);
    assert_eq!(values, vec![1, 2, 3]);

    // The encoded form must be a JSON array of numbers.
    let encoded = int_list
        .encode_start(&ops, &vec![4, 5])
        .unwrap_result("encode");
    assert_eq!(encoded, json!([4, 5]));
}

#[test]
fn unbounded_map_codec_round_trip() {
    let ops = JsonOps::INSTANCE;
    let map_codec: Arc<dyn rivet_serialization::Codec<HashMap<String, i32>, JsonOps>> =
        rivet_serialization::codec::unbounded_map(
            rivet_serialization::codec::string_codec(),
            rivet_serialization::codec::int_codec(),
        );
    let mut input = HashMap::new();
    input.insert("a".to_string(), 1);
    input.insert("b".to_string(), 2);
    let result: HashMap<String, i32> = round_trip(&ops, &map_codec, input);
    assert_eq!(result.get("a"), Some(&1));
    assert_eq!(result.get("b"), Some(&2));

    // Encoded form is a JSON object.
    let encoded = map_codec
        .encode_start(&ops, &result)
        .unwrap_result("encode");
    assert_eq!(encoded, json!({"a": 1, "b": 2}));
}

#[test]
fn record_codec_round_trip_through_json() {
    let ops = JsonOps::INSTANCE;
    #[derive(Debug, Clone, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    let field_x = rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|p: &Point| p.x),
        rivet_serialization::codec::field_of(
            rivet_serialization::codec::int_codec(),
            "x".to_string(),
        ),
    );
    let field_y = rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|p: &Point| p.y),
        rivet_serialization::codec::field_of(
            rivet_serialization::codec::int_codec(),
            "y".to_string(),
        ),
    );
    let codec = rivet_serialization::record_builder::create::<Point, JsonOps>(move |instance| {
        instance
            .group(field_x)
            .and(field_y)
            .apply(instance, Arc::new(|x: i32, y: i32| Point { x, y }))
    });

    let point = round_trip(&ops, &codec, Point { x: 10, y: 20 });
    assert_eq!(point, Point { x: 10, y: 20 });

    // Encoded form is `{"x":10,"y":20}` (JsonRecordBuilder, insertion order).
    let encoded = codec
        .encode_start(&ops, &Point { x: 10, y: 20 })
        .unwrap_result("encode");
    assert_eq!(encoded, json!({"x": 10, "y": 20}));

    // Decode a foreign JSON object.
    let decoded = codec
        .decode(&ops, &json!({"x": 1, "y": 2}))
        .unwrap_result("decode");
    assert_eq!(decoded.0, Point { x: 1, y: 2 });
}

// ---------------------------------------------------------------------------
// Passthrough codec: complex nested JSON round-trip
// ---------------------------------------------------------------------------

#[test]
fn passthrough_round_trips_nested_json() {
    let ops = JsonOps::INSTANCE;
    let passthrough = rivet_serialization::codec::passthrough::<JsonOps>();

    let input = json!({
        "name": "test",
        "count": 3,
        "ratio": 0.5,
        "ok": true,
        "missing": null,
        "tags": ["a", "b"],
        "nested": {"deep": {"x": 1}}
    });

    let dynamic = rivet_serialization::dynamic::Dynamic::new(&ops, input.clone());
    let encoded = passthrough
        .encode_start(&ops, &dynamic)
        .unwrap_result("encode");
    let (decoded, _rest) = passthrough.decode(&ops, &encoded).unwrap_result("decode");
    assert_eq!(decoded.get_value(), &input);
}

// ---------------------------------------------------------------------------
// Compressed mode: getNumberValue / getStringValue tolerance
// ---------------------------------------------------------------------------

#[test]
fn compressed_accepts_numeric_strings_in_get_number_value() {
    // Java `JsonOps.COMPRESSED.getNumberValue("5")` → `Integer.parseInt`.
    let ops = JsonOps::COMPRESSED;
    let result = ops.get_number_value(&Value::String("42".to_string()));
    assert_eq!(
        result.result().copied(),
        Some(rivet_serialization::number::Number::Int(42))
    );

    // Out of i32 range → parse error (Java `Integer.parseInt`, not long).
    let result = ops.get_number_value(&Value::String("99999999999999".to_string()));
    assert!(result.is_error());

    // Non-numeric string → error.
    let result = ops.get_number_value(&Value::String("abc".to_string()));
    assert!(result.is_error());

    // `INSTANCE` never accepts strings as numbers.
    let result = JsonOps::INSTANCE.get_number_value(&Value::String("42".to_string()));
    assert!(result.is_error());
}

#[test]
fn compressed_accepts_numbers_in_get_string_value() {
    // Java `JsonOps.COMPRESSED.getStringValue(JsonPrimitive number)` →
    // `input.getAsString()`.
    let ops = JsonOps::COMPRESSED;
    assert_eq!(
        ops.get_string_value(&Value::Number(Number::from(5)))
            .result(),
        Some(&"5".to_string())
    );
    // `INSTANCE` rejects numbers as strings.
    let result = JsonOps::INSTANCE.get_string_value(&Value::Number(Number::from(5)));
    assert!(result.is_error());
}

// ---------------------------------------------------------------------------
// createNumeric / createBoolean / createString and empty forms
// ---------------------------------------------------------------------------

#[test]
fn create_and_empty_forms() {
    let ops = JsonOps::INSTANCE;
    assert_eq!(ops.empty(), Value::Null);
    assert_eq!(ops.empty_map(), json!({}));
    assert_eq!(ops.empty_list(), json!([]));
    assert_eq!(
        ops.create_numeric(rivet_serialization::number::Number::Double(3.5)),
        json!(3.5)
    );
    assert_eq!(ops.create_boolean(true), json!(true));
    assert_eq!(ops.create_string("s".to_string()), json!("s"));

    // JSON cannot represent NaN/Infinity → Gson can, but the port narrows to
    // `Null` (documented deviation, see json_ops.rs).
    assert_eq!(
        ops.create_numeric(rivet_serialization::number::Number::Double(f64::NAN)),
        Value::Null
    );
    assert_eq!(
        ops.create_numeric(rivet_serialization::number::Number::Double(f64::INFINITY)),
        Value::Null
    );
}

// ---------------------------------------------------------------------------
// mergeToList / mergeToMap
// ---------------------------------------------------------------------------

#[test]
fn merge_to_list_behavior() {
    let ops = JsonOps::INSTANCE;
    // Append to an existing array.
    let merged = ops
        .merge_to_list(&json!([1, 2]), json!(3))
        .unwrap_result("merge");
    assert_eq!(merged, json!([1, 2, 3]));
    // Append to empty() → fresh array.
    let merged = ops
        .merge_to_list(&ops.empty(), json!(1))
        .unwrap_result("merge");
    assert_eq!(merged, json!([1]));
    // Non-array non-empty prefix → error with the prefix as partial.
    let result = ops.merge_to_list(&json!({"a": 1}), json!(1));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "mergeToList called with not a list: {\"a\":1}"
    );
}

#[test]
fn merge_to_list_many_behavior() {
    let ops = JsonOps::INSTANCE;
    let merged = ops
        .merge_to_list_many(&json!([1]), vec![json!(2), json!(3)])
        .unwrap_result("merge");
    assert_eq!(merged, json!([1, 2, 3]));
    // Empty values: keep the original list (or emptyList() when prefix empty).
    let merged = ops
        .merge_to_list_many(&json!([1, 2]), vec![])
        .unwrap_result("merge");
    assert_eq!(merged, json!([1, 2]));
    let merged = ops
        .merge_to_list_many(&ops.empty(), vec![])
        .unwrap_result("merge");
    assert_eq!(merged, json!([]));
}

#[test]
fn merge_to_map_behavior() {
    let ops = JsonOps::INSTANCE;
    let merged = ops
        .merge_to_map(&json!({"a": 1}), json!("b"), json!(2))
        .unwrap_result("merge");
    assert_eq!(merged, json!({"a": 1, "b": 2}));
    // Merge into empty() → fresh object.
    let merged = ops
        .merge_to_map(&ops.empty(), json!("k"), json!(true))
        .unwrap_result("merge");
    assert_eq!(merged, json!({"k": true}));
    // Non-object prefix → error.
    let result = ops.merge_to_map(&json!([1]), json!("k"), json!(1));
    assert!(result.is_error());
    // Non-string key (INSTANCE) → error, partial is the original map.
    let result = ops.merge_to_map(&json!({"a": 1}), json!(5), json!(2));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "key is not a string: 5"
    );
}

#[test]
fn compressed_merge_to_map_coerces_numeric_keys() {
    // Java `JsonOps.COMPRESSED.mergeToMap(map, JsonPrimitive 5, value)` coerces
    // the key to "5".
    let ops = JsonOps::COMPRESSED;
    let merged = ops
        .merge_to_map(&ops.empty(), json!(5), json!("v"))
        .unwrap_result("merge");
    assert_eq!(merged, json!({"5": "v"}));
    // `INSTANCE` rejects the numeric key.
    let result = JsonOps::INSTANCE.merge_to_map(&JsonOps::INSTANCE.empty(), json!(5), json!("v"));
    assert!(result.is_error());
}

#[test]
fn merge_to_map_like_reports_non_string_keys() {
    let ops = JsonOps::INSTANCE;
    let entries = vec![
        Pair::of(json!("good"), json!(1)),
        Pair::of(json!(7), json!(2)), // numeric key: only allowed when compressed
    ];
    let map_like = VecMapLike(entries);
    let result = ops.merge_to_map_like(&ops.empty(), &map_like);
    assert!(result.is_error());
    // Partial contains the merged output with the stringified "good" key.
    // The message renders missed keys via Gson's compact `JsonElement.toString`
    // (`JsonPrimitive(7)` → `"7"`), matching Java's output exactly.
    assert_eq!(
        result.error_ref().unwrap().message(),
        "some keys are not strings: [7]"
    );

    // COMPRESSED coerces both keys.
    let ops = JsonOps::COMPRESSED;
    let result = ops
        .merge_to_map_like(&ops.empty(), &map_like)
        .unwrap_result("merge");
    assert_eq!(result, json!({"good": 1, "7": 2}));
}

#[test]
fn merge_to_map_like_empty_entries_returns_map() {
    let ops = JsonOps::INSTANCE;
    let empty = VecMapLike(Vec::new());
    // Empty values on an empty prefix → emptyMap().
    let result = ops
        .merge_to_map_like(&ops.empty(), &empty)
        .unwrap_result("merge");
    assert_eq!(result, json!({}));
    // Empty values on a populated prefix → the prefix unchanged.
    let result = ops
        .merge_to_map_like(&json!({"a": 1}), &empty)
        .unwrap_result("merge");
    assert_eq!(result, json!({"a": 1}));
}

/// `MapLike<Value>` backed by an explicit entry list (keys kept as `Value`).
#[derive(Debug)]
struct VecMapLike(Vec<Pair<Value, Value>>);

impl MapLike<Value> for VecMapLike {
    fn get(&self, key: &Value) -> Option<Value> {
        self.0
            .iter()
            .find(|p| &p.first == key)
            .map(|p| p.second.clone())
    }

    fn get_string(&self, _key: &str) -> Option<Value> {
        None
    }

    fn entries(&self) -> Vec<Pair<Value, Value>> {
        self.0.clone()
    }
}

// ---------------------------------------------------------------------------
// getMap / getMapValues / getMapEntries
// ---------------------------------------------------------------------------

#[test]
fn get_map_and_get_map_values() {
    let ops = JsonOps::INSTANCE;
    let input = json!({"a": 1, "b": "two", "c": null});

    // get_map: keys resolve; JsonNull entries read as absence (Java `null`).
    let map_result = ops.get_map(&input);
    let map = map_result.result().expect("get_map");
    assert_eq!(map.get_string("a"), Some(json!(1)));
    assert_eq!(map.get_string("b"), Some(json!("two")));
    assert_eq!(map.get_string("c"), None); // JsonNull → Java null
    assert_eq!(map.get_string("missing"), None);

    // Generic key lookup.
    assert_eq!(map.get(&json!("a")), Some(json!(1)));
    assert_eq!(map.get(&json!("missing")), None);

    // get_map_values keeps the entries in order.
    let values = ops.get_map_values(&input).unwrap_result("get_map_values");
    let keys: Vec<&str> = values.iter().map(|p| p.first.as_str().unwrap()).collect();
    assert_eq!(keys, vec!["a", "b", "c"]);

    // get_map_entries feeds the same pairs.
    let entries_result = ops.get_map_entries(&input);
    let consumer = entries_result.result().expect("get_map_entries");
    let mut seen = Vec::new();
    consumer(&mut |k, v| seen.push((k.clone(), v.clone())));
    let keys: Vec<&str> = seen.iter().map(|(k, _)| k.as_str().unwrap()).collect();
    assert_eq!(keys, vec!["a", "b", "c"]);

    // Non-object inputs error.
    assert!(ops.get_map(&json!([1])).is_error());
    assert!(ops.get_map_values(&json!(5)).is_error());
    assert!(ops.get_map_entries(&json!("s")).is_error());
}

#[test]
fn create_map_accepts_primitive_keys() {
    // Java `createMap` stringifies any primitive key (both modes).
    let ops = JsonOps::INSTANCE;
    let map = ops.create_map(vec![
        Pair::of(json!(5), json!("v")),
        Pair::of(json!(true), json!(1)),
    ]);
    assert_eq!(map, json!({"5": "v", "true": 1}));
}

// ---------------------------------------------------------------------------
// getStream / getList / createList
// ---------------------------------------------------------------------------

#[test]
fn get_stream_and_get_list() {
    let ops = JsonOps::INSTANCE;
    let input = json!([1, "two", true]);

    let stream = ops.get_stream(&input).unwrap_result("get_stream");
    assert_eq!(stream, vec![json!(1), json!("two"), json!(true)]);

    let list_result = ops.get_list(&input);
    let consumer = list_result.result().expect("get_list");
    let mut seen = Vec::new();
    consumer(&mut |v| seen.push(v.clone()));
    assert_eq!(seen, vec![json!(1), json!("two"), json!(true)]);

    assert!(ops.get_stream(&json!({})).is_error());
    assert!(ops.get_list(&json!(5)).is_error());
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_key_from_object() {
    let ops = JsonOps::INSTANCE;
    assert_eq!(ops.remove(json!({"a": 1, "b": 2}), "a"), json!({"b": 2}));
    // Non-object input passes through unchanged.
    assert_eq!(ops.remove(json!([1]), "a"), json!([1]));
}

// ---------------------------------------------------------------------------
// convertTo narrowing
// ---------------------------------------------------------------------------

#[test]
fn convert_to_narrows_numbers() {
    let ops = JsonOps::INSTANCE;
    let out = JsonOps::INSTANCE;

    // 5 → int.
    assert_eq!(ops.convert_to(&out, &json!(5)), json!(5));
    // 100000 → int (within i32).
    assert_eq!(ops.convert_to(&out, &json!(100000)), json!(100000));
    // 5_000_000_000 → long (beyond i32).
    let converted = ops.convert_to(&out, &json!(5_000_000_000i64));
    assert_eq!(converted, json!(5_000_000_000i64));
    // 3.5 → float (exact in f32).
    assert_eq!(ops.convert_to(&out, &json!(3.5)), json!(3.5));
    // 0.1 → double (not exact in f32).
    assert_eq!(ops.convert_to(&out, &json!(0.1)), json!(0.1));
    // String / bool / null pass through the primitive create path.
    assert_eq!(ops.convert_to(&out, &json!("s")), json!("s"));
    assert_eq!(ops.convert_to(&out, &json!(true)), json!(true));
    assert_eq!(ops.convert_to(&out, &Value::Null), Value::Null);

    // 2^63 — `i64::MAX as f64` rounds up to this, so it is NOT a long
    // (Java `BigDecimal.longValueExact` throws) → double path → float (2^63
    // is exact in f32). This pins the boundary of `exact_integral`. The float
    // path carries `Float.toString(2^63f)` = `9.223372E18` (f32-shortest
    // digits), not the exact u64 — matching Paper.
    let two_pow_63 = 9_223_372_036_854_775_808f64;
    let input = Value::Number(Number::from_f64(two_pow_63).expect("2^63 fits f64"));
    assert_eq!(ops.convert_to(&out, &input), json!(9.223372e18));
}

// ---------------------------------------------------------------------------
// JsonRecordBuilder (map_builder) fidelity
// ---------------------------------------------------------------------------

#[test]
fn map_builder_merges_into_prefix() {
    let ops = JsonOps::INSTANCE;
    let mut builder = ops.map_builder();
    builder.add_string("new", json!(1));
    // Null prefix → plain object.
    let built = builder.build(Some(ops.empty())).unwrap_result("build");
    assert_eq!(built, json!({"new": 1}));

    // Merge into an existing object prefix (prefix entries first).
    let mut builder = ops.map_builder();
    builder.add_string("new", json!(1));
    let built = builder
        .build(Some(json!({"old": 0})))
        .unwrap_result("build");
    assert_eq!(built, json!({"old": 0, "new": 1}));

    // Non-object prefix → error.
    let mut builder = ops.map_builder();
    builder.add_string("new", json!(1));
    let result = builder.build(Some(json!([1])));
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "mergeToMap called with not a map: [1]"
    );

    // Builder is reset after each build (Java `AbstractBuilder.build`).
    let mut builder = ops.map_builder();
    builder.add_string("a", json!(1));
    builder.build(Some(ops.empty())).unwrap_result("build");
    let rebuilt = builder.build(Some(ops.empty())).unwrap_result("build");
    assert_eq!(rebuilt, json!({}));
}

#[test]
fn map_builder_string_key_resolution() {
    // `JsonRecordBuilder.add(T key, T value)` resolves the key through
    // `getStringValue` — a numeric key errors on INSTANCE.
    let ops = JsonOps::INSTANCE;
    let mut builder = ops.map_builder();
    builder.add(json!(5), json!(1));
    let result = builder.build(Some(ops.empty()));
    assert!(result.is_error());

    // COMPRESSED coerces the numeric key.
    let ops = JsonOps::COMPRESSED;
    let mut builder = ops.map_builder();
    builder.add(json!(5), json!(1));
    let built = builder.build(Some(ops.empty())).unwrap_result("build");
    assert_eq!(built, json!({"5": 1}));
}

// ---------------------------------------------------------------------------
// getByteBuffer / getIntStream / getLongStream (DynamicOps defaults over JSON)
// ---------------------------------------------------------------------------

#[test]
fn default_stream_narrowing_over_json() {
    let ops = JsonOps::INSTANCE;
    assert_eq!(
        ops.get_byte_buffer(&json!([0, 255])).result(),
        Some(&vec![0u8, 255u8])
    );
    assert_eq!(
        ops.get_int_stream(&json!([1, 2, 3])).result(),
        Some(&vec![1, 2, 3])
    );
    assert_eq!(
        ops.get_long_stream(&json!([1_000_000_000])).result(),
        Some(&vec![1_000_000_000i64])
    );
    // A non-number element fails the whole stream.
    assert!(ops.get_byte_buffer(&json!([1, "x"])).is_error());
    assert!(ops.get_int_stream(&json!([1, "x"])).is_error());
    assert!(ops.get_long_stream(&json!([1, "x"])).is_error());
    // Non-array input keeps the getStream error.
    assert!(ops.get_int_stream(&json!({"a": 1})).is_error());
}

// ---------------------------------------------------------------------------
// compressMaps
// ---------------------------------------------------------------------------

#[test]
fn compress_maps_flag() {
    assert!(!JsonOps::INSTANCE.compress_maps());
    assert!(JsonOps::COMPRESSED.compress_maps());
}

// ---------------------------------------------------------------------------
// Typed Number boundary semantics (DFU 10.0.21)
// ---------------------------------------------------------------------------

/// `getNumberValue` of an integer JSON literal.
#[test]
fn get_number_value_returns_typed_integer_variants() {
    let ops = JsonOps::INSTANCE;
    // i32 range → Int.
    assert_eq!(
        ops.get_number_value(&json!(42)).result().copied(),
        Some(TypedNumber::Int(42))
    );
    // i64 range → Long.
    assert_eq!(
        ops.get_number_value(&json!(9_000_000_000i64))
            .result()
            .copied(),
        Some(TypedNumber::Long(9_000_000_000))
    );
    // Out of i64 range (u64 → beyond) → Double.
    assert_eq!(
        ops.get_number_value(&json!(18_446_744_073_709_551_615u64))
            .result()
            .copied(),
        Some(TypedNumber::Double(18_446_744_073_709_551_615.0))
    );
    // Non-integral literal → Double.
    assert_eq!(
        ops.get_number_value(&json!(3.5)).result().copied(),
        Some(TypedNumber::Double(3.5))
    );
    // Compressed string form → Integer.parseInt.
    assert_eq!(
        JsonOps::COMPRESSED
            .get_number_value(&Value::String("42".into()))
            .result()
            .copied(),
        Some(TypedNumber::Int(42))
    );
}

/// `Codec.LONG` round-trips i64 min/max and 2^53 ± 1 exactly through JSON.
#[test]
fn long_codec_preserves_i64_precision_through_json() {
    let ops = JsonOps::INSTANCE;
    let long = rivet_serialization::codec::long_codec::<JsonOps>();
    let cases = [
        i64::MIN,
        i64::MAX,
        (1i64 << 53) - 1,
        (1i64 << 53) + 1,
        9_000_000_000,
        -9_000_000_000,
    ];
    for value in cases {
        let rt = round_trip(&ops, &long, value);
        assert_eq!(rt, value, "Codec.LONG did not round-trip {value}");
    }
}

/// The JSON round-trip keeps the full i64 value (serde_json stores integral
/// literals exactly; the old f64 surface would have truncated above 2^53).
#[test]
fn json_encodes_i64_max_exactly() {
    let ops = JsonOps::INSTANCE;
    let long = rivet_serialization::codec::long_codec::<JsonOps>();
    let encoded = long.encode_start(&ops, &i64::MAX).unwrap_result("encode");
    assert_eq!(encoded, json!(i64::MAX));
    // The old f64 path would have lost the low bits.
    assert_ne!(encoded, json!(i64::MAX as f64));
}

/// Float/double codecs read the JSON number via `Number.floatValue`/`doubleValue`.
#[test]
fn float_double_codecs_round_trip_through_json() {
    let ops = JsonOps::INSTANCE;
    let float = rivet_serialization::codec::float_codec::<JsonOps>();
    assert_eq!(round_trip(&ops, &float, 3.5f32), 3.5f32);
    assert_eq!(round_trip(&ops, &float, -0.25f32), -0.25f32);

    let double = rivet_serialization::codec::double_codec::<JsonOps>();
    assert_eq!(round_trip(&ops, &double, 0.1), 0.1);
    assert_eq!(round_trip(&ops, &double, 1.0 / 3.0), 1.0 / 3.0);
}

/// `createFloat` renders the `Float.toString` literal, not the widened `f64`
/// form — Gson prints `0.05` for a `JsonPrimitive(Float(0.05))`, never
/// `0.05000000074505806`.
#[test]
fn create_float_uses_float_to_string_literal() {
    let ops = JsonOps::INSTANCE;
    assert_eq!(ops.create_float(0.05f32), json!(0.05));
    assert_eq!(ops.create_float(0.1f32), json!(0.1));
    assert_eq!(ops.create_float(1.0f32), json!(1.0));
    assert_eq!(ops.create_float(0.001f32), json!(0.001));
}

/// Signed narrowing through the byte codec: reading a JSON number as a byte
/// wraps via `(int)` then `(byte)`.
#[test]
fn byte_codec_narrows_through_json() {
    let ops = JsonOps::INSTANCE;
    let byte = rivet_serialization::codec::byte_codec::<JsonOps>();
    // `300 -> (int)300 -> (byte)300 == 44`.
    assert_eq!(byte.parse(&ops, &json!(300)).unwrap_result("parse"), 44i8);
    assert_eq!(byte.parse(&ops, &json!(-300)).unwrap_result("parse"), -44i8);
    assert_eq!(byte.parse(&ops, &json!(42)).unwrap_result("parse"), 42i8);
}
