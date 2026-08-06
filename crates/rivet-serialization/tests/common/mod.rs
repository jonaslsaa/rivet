//! Shared test harness for the DFU-mirroring integration tests.
//!
//! `Value`/`TestOps` form a minimal JSON-like `DynamicOps` (the same shape the
//! crate's existing `codec_tests.rs` uses) so the ported codec surface can be
//! exercised without depending on `rivet-nbt`. Kept in `tests/common/` so each
//! integration test crate includes it via `mod common;`.
//!
//! Each integration test crate compiles this module on its own, so the helpers
//! are individually used by only one test file at a time; `dead_code` is
//! allowed to avoid per-file warnings.

#![allow(dead_code)]

use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike};
use rivet_serialization::pair::Pair;
use std::fmt::Debug;
use std::sync::Arc;

/// Minimal JSON-like value for exercising the codec surface.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

#[derive(Debug)]
pub struct TestOps;

/// A `MapLike` with proper string-key lookup (`Vec<Pair>`'s default
/// `get_string` is a stub that returns `None`).
#[derive(Debug)]
pub struct TestMapLike(pub Vec<Pair<Value, Value>>);

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

    fn get_number_value(&self, input: &Value) -> DataResult<f64> {
        match input {
            Value::Num(n) => DataResult::success(*n),
            other => DataResult::error(format!("Not a number: {other:?}")),
        }
    }

    fn create_numeric(&self, value: f64) -> Value {
        Value::Num(value)
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

    fn merge_to_map_like(&self, map: &Value, values: &dyn MapLike<Value>) -> DataResult<Value> {
        // Start from an empty map when the prefix is `empty()` (Java's
        // `MapOps.mergeToMap` turns an empty prefix into a fresh map, so a
        // zero-entry merge must yield `emptyMap()`, not `empty()`).
        let base = if *map == self.empty() {
            self.empty_map()
        } else {
            map.clone()
        };
        let mut result: DataResult<Value> = DataResult::success(base);
        for entry in values.entries() {
            result = result.flat_map(|r| self.merge_to_map(&r, entry.first, entry.second));
        }
        result
    }

    fn merge_to_map_many(&self, map: &Value, values: Vec<Pair<Value, Value>>) -> DataResult<Value> {
        let base = if *map == self.empty() {
            self.empty_map()
        } else {
            map.clone()
        };
        let mut result: DataResult<Value> = DataResult::success(base);
        for entry in values {
            result = result.flat_map(|r| self.merge_to_map(&r, entry.first, entry.second));
        }
        result
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

/// Test helpers mirroring the `CodecTests` assertions.
///
/// The helper names mirror the upstream `CodecTests` free functions (`fromJava`,
/// `assertFromJavaFails`, ...) but the `from_*` ones take `&self` (they read
/// through the `TestOps`), so clippy's `wrong_self_convention` is allowed for
/// the three that carry the `from_` prefix.
impl TestOps {
    /// `codec.parse(ops, value)` → unwrapped decoded value.
    pub fn parse_or_throw<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &Value,
    ) -> A
    where
        A: 'static + Clone,
    {
        codec.parse(self, value).get_or_throw("parse").clone()
    }

    /// `codec.parse(ops, value).getPartialOrThrow(...)`.
    #[allow(clippy::wrong_self_convention)]
    pub fn from_java_or_partial<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &Value,
    ) -> A
    where
        A: 'static + Clone,
    {
        codec
            .parse(self, value)
            .clone()
            .result_or_partial_silent()
            .expect("expected an error with a partial result")
    }

    /// `codec.parse(ops, value).error().message()`.
    #[allow(clippy::wrong_self_convention)]
    pub fn from_java_error_message<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &Value,
    ) -> String
    where
        A: 'static,
    {
        codec
            .parse(self, value)
            .error_ref()
            .expect("expected an error")
            .message()
            .to_string()
    }

    /// `assertFromJavaFails` — the parse must be an error.
    pub fn assert_from_java_fails<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &Value,
    ) where
        A: 'static + Debug,
    {
        assert!(
            codec.parse(self, value).is_error(),
            "expected a data result error, but got: {:?}",
            codec.parse(self, value).result()
        );
    }

    /// `assertFromJavaFailsPartial` — the parse must be an error with NO
    /// result-or-partial.
    pub fn assert_from_java_fails_partial<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &Value,
    ) where
        A: 'static + Clone,
    {
        assert!(
            codec
                .parse(self, value)
                .clone()
                .result_or_partial_silent()
                .is_none(),
            "expected an error with no partial result"
        );
    }

    /// `assertToJavaFails` — the encode must be an error.
    pub fn assert_to_java_fails<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: &A,
    ) where
        A: 'static + Clone,
    {
        assert!(
            codec.encode_start(self, value).is_error(),
            "expected an encode error, but got: {:?}",
            codec.encode_start(self, value).result()
        );
    }

    /// `assertRoundTrip` — encode then decode reproduce both ends.
    ///
    /// Map equality is order-insensitive (the underlying `HashMap` iteration
    /// order is not deterministic, matching Java's own unordered map values).
    pub fn assert_round_trip<A>(
        &self,
        codec: &Arc<dyn rivet_serialization::Codec<A, TestOps>>,
        value: A,
        encoded: Value,
    ) where
        A: 'static + Clone + PartialEq + Debug,
    {
        let to = codec
            .encode_start(self, &value)
            .get_or_throw("encodeStart")
            .clone();
        assert_map_eq(
            &to,
            &encoded,
            "encodeStart did not match the expected value",
        );
        let from = codec.parse(self, &encoded).get_or_throw("parse").clone();
        assert_eq!(from, value, "parse did not round-trip the value");
    }
}

/// `assert_eq!` on two `Value`s, treating `Map` entries as an unordered set.
fn assert_map_eq(actual: &Value, expected: &Value, context: &str) {
    match (actual, expected) {
        (Value::Map(a), Value::Map(b)) => {
            let mut a = a.clone();
            a.sort_by(|l, r| l.0.cmp(&r.0));
            let mut b = b.clone();
            b.sort_by(|l, r| l.0.cmp(&r.0));
            assert_eq!(a, b, "{context}");
        }
        _ => assert_eq!(actual, expected, "{context}"),
    }
}
