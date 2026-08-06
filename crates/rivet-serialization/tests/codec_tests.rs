//! Codec-level DFU tests against a minimal `DynamicOps` implementation.
//!
//! These lock the reviewer-fixed semantics: partial-value error propagation
//! (`DataResult.flat_map`), lifecycle preservation through `map` (Either/Xor),
//! `RecordCodecBuilder` error accumulation, and `BaseMapCodec`'s `apply2stable`.

use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::pair::Pair;
use std::sync::Arc;

/// Minimal JSON-like value for exercising the codec surface.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

#[derive(Debug)]
struct TestOps;

/// A `MapLike` with proper string-key lookup (`Vec<Pair>`'s default
/// `get_string` is a stub that returns `None`).
#[derive(Debug)]
struct TestMapLike(Vec<Pair<Value, Value>>);

impl MapLike<Value> for TestMapLike {
    fn get(&self, key: &Value) -> Option<Value> {
        self.0
            .iter()
            .find(|p| &p.first == key)
            .map(|p| p.second.clone())
    }

    fn get_string(&self, key: &str) -> Option<Value> {
        self.0
            .iter()
            .find(|p| matches!(&p.first, Value::Str(s) if s == key))
            .map(|p| p.second.clone())
    }

    fn entries(&self) -> Vec<Pair<Value, Value>> {
        self.0.clone()
    }
}

impl DynamicOps for TestOps {
    type Output = Value;

    fn empty(&self) -> Value {
        Value::Null
    }

    fn empty_map(&self) -> Value {
        Value::Map(Vec::new())
    }

    fn empty_list(&self) -> Value {
        Value::List(Vec::new())
    }

    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &Value) -> U::Output {
        out_ops.create_string(match input {
            Value::Str(s) => s.clone(),
            Value::Num(n) => n.to_string(),
            _ => String::new(),
        })
    }

    fn get_number_value(&self, input: &Value) -> DataResult<rivet_serialization::number::Number> {
        match input {
            Value::Num(n) => DataResult::success(rivet_serialization::number::Number::Double(*n)),
            other => DataResult::error(format!("Not a number: {other:?}")),
        }
    }

    fn create_numeric(&self, value: rivet_serialization::number::Number) -> Value {
        Value::Num(value.double_value())
    }

    fn get_boolean_value(&self, input: &Value) -> DataResult<bool> {
        match input {
            Value::Bool(b) => DataResult::success(*b),
            other => DataResult::error(format!("Not a boolean: {other:?}")),
        }
    }

    fn create_boolean(&self, value: bool) -> Value {
        Value::Bool(value)
    }

    fn get_string_value(&self, input: &Value) -> DataResult<String> {
        match input {
            Value::Str(s) => DataResult::success(s.clone()),
            other => DataResult::error(format!("Not a string: {other:?}")),
        }
    }

    fn create_string(&self, value: String) -> Value {
        Value::Str(value)
    }

    fn merge_to_list(&self, list: &Value, value: Value) -> DataResult<Value> {
        match list {
            Value::Null => DataResult::success(Value::List(vec![value])),
            Value::List(l) => {
                let mut l = l.clone();
                l.push(value);
                DataResult::success(Value::List(l))
            }
            other => DataResult::error(format!("Cannot merge to list: {other:?}")),
        }
    }

    fn merge_to_map(&self, map: &Value, key: Value, value: Value) -> DataResult<Value> {
        let key = match key {
            Value::Str(k) => k,
            other => return DataResult::error(format!("Map key is not a string: {other:?}")),
        };
        match map {
            Value::Null => DataResult::success(Value::Map(vec![(key, value)])),
            Value::Map(entries) => {
                let mut entries = entries.clone();
                if let Some(existing) = entries.iter_mut().find(|(k, _)| *k == key) {
                    existing.1 = value;
                } else {
                    entries.push((key, value));
                }
                DataResult::success(Value::Map(entries))
            }
            other => DataResult::error(format!("Cannot merge to map: {other:?}")),
        }
    }

    fn get_map_values(&self, input: &Value) -> DataResult<Vec<Pair<Value, Value>>> {
        match input {
            Value::Map(entries) => DataResult::success(
                entries
                    .iter()
                    .map(|(k, v)| Pair::of(Value::Str(k.clone()), v.clone()))
                    .collect(),
            ),
            other => DataResult::error(format!("Not a map: {other:?}")),
        }
    }

    fn get_map_entries(
        &self,
        input: &Value,
    ) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Value, &Value))>> {
        self.get_map_values(input).map_owned(|entries| {
            Box::new(move |consumer: &mut dyn FnMut(&Value, &Value)| {
                for p in &entries {
                    consumer(&p.first, &p.second);
                }
            }) as Box<dyn Fn(&mut dyn FnMut(&Value, &Value))>
        })
    }

    fn create_map(&self, map: Vec<Pair<Value, Value>>) -> Value {
        let entries = map
            .into_iter()
            .map(|p| {
                let key = match p.first {
                    Value::Str(k) => k,
                    _ => String::new(),
                };
                (key, p.second)
            })
            .collect();
        Value::Map(entries)
    }

    fn get_map(&self, input: &Value) -> DataResult<Box<dyn MapLike<Value>>> {
        match input {
            Value::Map(entries) => {
                let entries: Vec<Pair<Value, Value>> = entries
                    .iter()
                    .map(|(k, v)| Pair::of(Value::Str(k.clone()), v.clone()))
                    .collect();
                DataResult::success(Box::new(TestMapLike(entries)) as Box<dyn MapLike<Value>>)
            }
            other => DataResult::error(format!("Not a map: {other:?}")),
        }
    }

    fn get_stream(&self, input: &Value) -> DataResult<Vec<Value>> {
        match input {
            Value::List(l) => DataResult::success(l.clone()),
            other => DataResult::error(format!("Not a list: {other:?}")),
        }
    }

    fn get_list(&self, input: &Value) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Value))>> {
        self.get_stream(input).map_owned(|values| {
            Box::new(move |consumer: &mut dyn FnMut(&Value)| {
                for v in &values {
                    consumer(v);
                }
            }) as Box<dyn Fn(&mut dyn FnMut(&Value))>
        })
    }

    fn create_list(&self, input: Vec<Value>) -> Value {
        Value::List(input)
    }

    fn get_byte_buffer(&self, _input: &Value) -> DataResult<Vec<u8>> {
        DataResult::error("No byte buffer")
    }

    fn create_byte_list(&self, _input: &[u8]) -> Value {
        Value::List(Vec::new())
    }

    fn get_int_stream(&self, _input: &Value) -> DataResult<Vec<i32>> {
        DataResult::error("No int stream")
    }

    fn create_int_list(&self, _input: Vec<i32>) -> Value {
        Value::List(Vec::new())
    }

    fn get_long_stream(&self, _input: &Value) -> DataResult<Vec<i64>> {
        DataResult::error("No long stream")
    }

    fn create_long_list(&self, _input: Vec<i64>) -> Value {
        Value::List(Vec::new())
    }

    fn remove(&self, input: Value, key: &str) -> Value {
        match input {
            Value::Map(entries) => {
                let entries: Vec<_> = entries.into_iter().filter(|(k, _)| k != key).collect();
                Value::Map(entries)
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------
// ListCodec: an element that fails stays an error (result accumulates)
// ---------------------------------------------------------------------------

#[test]
fn list_codec_keeps_failed_element_error() {
    // `Codec.list(intCodec)` over [1, "not-an-int", 3]. The middle element
    // errors with no partial; `ListCodec` must report the error (its result
    // `apply2stable` accumulates) rather than promoting to a success.
    let ops = TestOps;
    let int_codec: Arc<dyn rivet_serialization::Codec<i32, TestOps>> =
        rivet_serialization::codec::int_codec();
    let list_codec = rivet_serialization::codec::list(int_codec);
    let input = Value::List(vec![
        Value::Num(1.0),
        Value::Str("not-an-int".to_string()),
        Value::Num(3.0),
    ]);
    let result = list_codec.decode(&ops, &input);
    assert!(result.is_error());
    assert_eq!(
        result.error_ref().unwrap().message(),
        "Not a number: Str(\"not-an-int\")"
    );
}

// ---------------------------------------------------------------------------
// EitherCodec: lifecycle preserved through the Either wrap (map not flat_map)
// ---------------------------------------------------------------------------

#[test]
fn either_codec_first_alternative_preserves_stable_lifecycle() {
    let ops = TestOps;
    let stable_int: Arc<dyn rivet_serialization::Codec<i32, TestOps>> =
        rivet_serialization::codec::stable(rivet_serialization::codec::int_codec());
    let str_codec: Arc<dyn rivet_serialization::Codec<String, TestOps>> =
        rivet_serialization::codec::string_codec();
    let either = rivet_serialization::codec::either(stable_int, str_codec);
    let input = Value::Num(5.0);
    let result = either.decode(&ops, &input);
    assert!(result.is_success());
    // Java `EitherCodec.decode` wraps with `.map(vo -> vo.mapFirst(Either::left))`
    // which preserves the inner codec's lifecycle — the stable() codec must
    // yield a stable result, not experimental.
    assert_eq!(result.lifecycle(), Lifecycle::stable());
}

// ---------------------------------------------------------------------------
// RecordCodecBuilder: multiple invalid fields accumulate errors (ap2)
// ---------------------------------------------------------------------------

#[test]
fn record_codec_builder_accumulates_multiple_field_errors() {
    // Two required fields, both failing: Java `Instance.ap2` decodes BOTH and
    // reports the accumulated "B; A" error rather than short-circuiting on A.
    let ops = TestOps;
    #[derive(Debug, Clone, PartialEq)]
    struct Pair2(i32, i32);

    let field_a = rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|p: &Pair2| p.0),
        rivet_serialization::codec::field_of(
            rivet_serialization::codec::int_codec(),
            "a".to_string(),
        ),
    );
    let field_b = rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|p: &Pair2| p.1),
        rivet_serialization::codec::field_of(
            rivet_serialization::codec::int_codec(),
            "b".to_string(),
        ),
    );
    let codec = rivet_serialization::record_builder::create::<Pair2, TestOps>(move |instance| {
        instance
            .group(field_a)
            .and(field_b)
            .apply(instance, Arc::new(|x: i32, y: i32| Pair2(x, y)))
    });

    // Both fields are strings — both fail. Java reports the accumulated
    // "second; first" error.
    let input = Value::Map(vec![
        ("a".to_string(), Value::Str("x".to_string())),
        ("b".to_string(), Value::Str("y".to_string())),
    ]);
    let result = codec.decode(&ops, &input);
    assert!(result.is_error());
    let msg = result.error_ref().unwrap().message().to_string();
    assert!(msg.contains("Not a number"), "message: {msg}");
    assert!(msg.contains(';'), "expected accumulated error, got: {msg}");
}

// ---------------------------------------------------------------------------
// RecordCodecBuilder: 1-field group (compose1 / lift1) decode is experimental
// ---------------------------------------------------------------------------

#[test]
fn record_codec_builder_one_field_decode_is_experimental() {
    // Java `Instance.lift1`/`Products.P1.apply` builds the function field via
    // `instance.point(function)` = `RecordCodecBuilder.point(a)` =
    // `o -> Encoder.empty()` / `Decoder.unit(a)`. `Decoder.unit` returns an
    // experimental `DataResult.success`, and the composed decoder flatMaps the
    // field decode through it (`DataResult.flatMap` ADDS lifecycles), so the
    // result is experimental even when the single field's codec is stable.
    let ops = TestOps;
    #[derive(Debug, Clone, PartialEq)]
    struct Single(i32);

    let field_a = rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|s: &Single| s.0),
        rivet_serialization::codec::field_of(
            rivet_serialization::codec::stable(rivet_serialization::codec::int_codec()),
            "a".to_string(),
        ),
    );
    let codec = rivet_serialization::record_builder::create::<Single, TestOps>(move |instance| {
        instance
            .group(field_a)
            .apply(instance, Arc::new(|x: i32| Single(x)))
    });

    let input = Value::Map(vec![("a".to_string(), Value::Num(5.0))]);
    let result = codec.decode(&ops, &input);
    assert!(result.is_success());
    assert_eq!(result.lifecycle(), Lifecycle::experimental());
}

// ---------------------------------------------------------------------------
// BaseMapCodec: apply2stable keeps the function lifecycle stable
// ---------------------------------------------------------------------------

#[test]
fn unbounded_map_codec_decode_has_stable_lifecycle() {
    let ops = TestOps;
    // Java `BaseMapCodec.decode` combines key+value with `apply2stable`, so
    // with stable element codecs the accumulated result stays stable. (Before
    // the fix, `apply2` used an experimental function result, flipping the
    // whole decode to experimental.)
    let str_codec: Arc<dyn rivet_serialization::Codec<String, TestOps>> =
        rivet_serialization::codec::stable(rivet_serialization::codec::string_codec());
    let int_codec: Arc<dyn rivet_serialization::Codec<i32, TestOps>> =
        rivet_serialization::codec::stable(rivet_serialization::codec::int_codec());
    let map_codec = rivet_serialization::codec::unbounded_map(str_codec, int_codec);
    let input = Value::Map(vec![("k".to_string(), Value::Num(1.0))]);
    let result = map_codec.decode(&ops, &input);
    assert!(result.is_success());
    // `getMap` sets stable, `apply2stable` keeps it stable.
    assert_eq!(result.lifecycle(), Lifecycle::stable());
}

// ---------------------------------------------------------------------------
// CompoundListCodec: entries do not accumulate into `read` past a full error
// ---------------------------------------------------------------------------

/// A `Codec<String>` that fails (full error, no partial) on the key `"bad"`,
/// mimicking Java's `FieldDecoder` missing-key error: `result` becomes a full
/// error with no partial, so `CompoundListCodec.decode` must stop adding later
/// entries to `read`.
#[derive(Debug)]
struct FailingKeyCodec;

impl rivet_serialization::Encoder<String, TestOps> for FailingKeyCodec {
    fn encode(&self, input: &String, ops: &TestOps, prefix: &Value) -> DataResult<Value> {
        let _ = (input, ops);
        DataResult::success(prefix.clone())
    }
}

impl rivet_serialization::Decoder<String, TestOps> for FailingKeyCodec {
    fn decode(&self, _ops: &TestOps, input: &Value) -> DataResult<(String, Value)> {
        match input {
            Value::Str(s) if s == "bad" => DataResult::error("bad key"),
            Value::Str(s) => DataResult::success((s.clone(), Value::Null)),
            other => DataResult::error(format!("Not a string: {other:?}")),
        }
    }
}

impl rivet_serialization::Codec<String, TestOps> for FailingKeyCodec {}

#[test]
fn compound_list_codec_skips_entries_past_a_full_error() {
    // Java `CompoundListCodec.decode`: `result.getPlain().apply2stable((u, e) ->
    // { read.add(e); return u; }, readEntry)`. The accumulator runs only while
    // `result` and `readEntry` both carry a value (Instance.ap2 fast path, or
    // the Applicative fallback that maps error partials). Once `result` is a
    // FULL error (no partial), later entries never reach `read.add` — even a
    // succeeding entry must not accumulate. This locks that faithful behavior:
    // the bad key first fully errors `result`; the valid `good` entry after it
    // must NOT appear in the decoded `read` list.
    let ops = TestOps;
    let key_codec: Arc<dyn rivet_serialization::Codec<String, TestOps>> = Arc::new(FailingKeyCodec);
    let value_codec: Arc<dyn rivet_serialization::Codec<String, TestOps>> =
        rivet_serialization::codec::string_codec();
    let codec = rivet_serialization::codec::compound_list(key_codec, value_codec);

    // `bad` first (errors, no partial) then `good` (would succeed).
    let input = Value::Map(vec![
        ("bad".to_string(), Value::Str("v1".to_string())),
        ("good".to_string(), Value::Str("v2".to_string())),
    ]);
    let result = codec.decode(&ops, &input);
    assert!(result.is_error(), "the bad key must error the decode");
    // The error-with-partial carries the accumulated `read` list. Because the
    // full error happened at the first entry, the `good` entry must NOT be in
    // the partial — Java's `read.add` stopped running.
    let partial = result.result_or_partial_silent();
    match partial {
        Some((read, _)) => {
            assert!(
                read.is_empty(),
                "entries after a full error must not accumulate; got {read:?}"
            );
        }
        None => {
            // A full error with no partial is also faithful.
        }
    }
}
