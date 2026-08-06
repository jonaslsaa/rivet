//! Shared test harness for the DFU-mirroring integration tests.
//!
//! `Value`/`TestOps` form a minimal JSON-like `DynamicOps` (the same shape the
//! crate's existing `codec_tests.rs` uses); the suite also runs against
//! `JsonOps::INSTANCE`/`COMPRESSED`. All assertion helpers are ops-parametric
//! (`OpsTestExt`, implemented for every `DynamicOps` whose output is
//! `Canonical`), so a test body written against `&O: CanonOps` runs unchanged
//! against each backend.
//!
//! Expected/input values are built through the ops (`v_str`/`v_num`/`v_int`/
//! `v_list`/`v_map`), never by pattern-matching a concrete output type. Number
//! equality preserves the int/float distinction (`v_int` for integer-typed
//! fields, `v_num` for floats), matching Java's `JsonElement.equals`. Map
//! equality is order-insensitive (`Canon::sorted`) where Java/Gson semantics
//! require it (encoding a `HashMap`-backed source iterates nondeterministically);
//! ordered keys remain available via `ordered_map_keys` for field-order
//! assertions.
//!
//! Each integration test crate compiles this module on its own, so the helpers
//! are individually used by only one test file at a time; `dead_code` is
//! allowed to avoid per-file warnings.

#![allow(dead_code)]

use rivet_serialization::codec::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike};
use rivet_serialization::pair::Pair;
use serde_json::Value as JsonValue;
use std::fmt::Debug;
use std::sync::Arc;

/// Minimal JSON-like value for exercising the `TestOps` backend.
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

// ---------------------------------------------------------------------------
// Cross-ops value comparison
// ---------------------------------------------------------------------------

/// JSON number kind. Preserves the int/float typing Java's `JsonElement.equals`
/// distinguishes (`JsonPrimitive(1)` != `JsonPrimitive(1.0)`), so a codec that
/// emits an integer as a float (a Double NBT tag downstream) is caught by the
/// round-trip assertions. `TestOps` is f64-only and maps every number to
/// `Float`.
#[derive(Debug, Clone, PartialEq)]
pub enum CanonNumber {
    /// An integral JSON number (serde_json `PosInt`/`NegInt`).
    Integer(i64),
    /// A JSON float (serde_json `Float`).
    Float(f64),
}

/// Canonical tree used for cross-ops value equality.
#[derive(Debug, Clone, PartialEq)]
pub enum Canon {
    Null,
    Bool(bool),
    Number(CanonNumber),
    Str(String),
    List(Vec<Canon>),
    /// Map entries in insertion order; `sorted` reorders by key.
    Map(Vec<(String, Canon)>),
}

impl Canon {
    /// Recursively sort map entries by key — order-insensitive equality, which
    /// Java/Gson semantics require for map values (iteration order of a
    /// `HashMap`-backed source is not deterministic).
    pub fn sorted(self) -> Canon {
        match self {
            Canon::List(items) => Canon::List(items.into_iter().map(Canon::sorted).collect()),
            Canon::Map(mut entries) => {
                for value in entries.iter_mut().map(|(_, v)| v) {
                    *value = std::mem::replace(value, Canon::Null).sorted();
                }
                entries.sort_by(|l, r| l.0.cmp(&r.0));
                Canon::Map(entries)
            }
            other => other,
        }
    }
}

/// A `DynamicOps` output that can be normalized to `Canon` for comparison.
pub trait Canonical: Clone + Debug + PartialEq {
    fn canon(&self) -> Canon;
}

impl Canonical for Value {
    fn canon(&self) -> Canon {
        match self {
            Value::Null => Canon::Null,
            Value::Bool(b) => Canon::Bool(*b),
            Value::Num(n) => Canon::Number(CanonNumber::Float(*n)),
            Value::Str(s) => Canon::Str(s.clone()),
            Value::List(items) => Canon::List(items.iter().map(|i| i.canon()).collect()),
            Value::Map(entries) => Canon::Map(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.canon()))
                    .collect(),
            ),
        }
    }
}

impl Canonical for JsonValue {
    fn canon(&self) -> Canon {
        match self {
            JsonValue::Null => Canon::Null,
            JsonValue::Bool(b) => Canon::Bool(*b),
            // serde_json stores integer literals as `PosInt`/`NegInt` and floats
            // as `Float`; keep the distinction (Java `JsonElement.equals` does),
            // so an integer emitted as a float is not silently equal.
            JsonValue::Number(n) => Canon::Number(
                n.as_i64()
                    .or_else(|| n.as_u64().map(|u| u as i64))
                    .map(CanonNumber::Integer)
                    .unwrap_or_else(|| CanonNumber::Float(n.as_f64().unwrap_or(f64::NAN))),
            ),
            JsonValue::String(s) => Canon::Str(s.clone()),
            JsonValue::Array(items) => Canon::List(items.iter().map(|i| i.canon()).collect()),
            JsonValue::Object(map) => {
                Canon::Map(map.iter().map(|(k, v)| (k.clone(), v.canon())).collect())
            }
        }
    }
}

/// `NbtOps` output (`net.minecraft.nbt.Tag`) canonicalization.
///
/// Mirrors Java `Tag.equals`/`NumericTag.equals` at the int/float level only:
/// integer-typed tags (`ByteTag`/`ShortTag`/`IntTag`/`LongTag`) all collapse
/// to `Canon::Number(Integer)` and float-typed tags (`FloatTag`/`DoubleTag`)
/// to `Canon::Number(Float)`. Individual NBT widths are not preserved — Java
/// distinguishes `IntTag(1)` from `ByteTag(1)` by class, this canonical form
/// does not — but the int/float split keeps a Double-widened encode
/// (`NbtOps.createNumeric` → `DoubleTag`) distinguishable from an `IntTag`.
/// Byte arrays map to a `Canon::List` of bytes, so a byte array and a plain
/// list of byte tags compare equal — the
/// same observable value (both are a sequence of signed bytes; `NbtOps`/DFU
/// treat `ByteArrayTag` and a `ListTag` of `ByteTag` interchangeably in the
/// stream/byte-buffer surface).
impl Canonical for rivet_nbt::tag::Tag {
    fn canon(&self) -> Canon {
        use rivet_nbt::tag::Tag;
        match self {
            Tag::Byte(v) => Canon::Number(CanonNumber::Integer(v.value as i64)),
            Tag::Short(v) => Canon::Number(CanonNumber::Integer(v.value as i64)),
            Tag::Int(v) => Canon::Number(CanonNumber::Integer(v.value as i64)),
            Tag::Long(v) => Canon::Number(CanonNumber::Integer(v.value)),
            Tag::Float(v) => Canon::Number(CanonNumber::Float(v.value as f64)),
            Tag::Double(v) => Canon::Number(CanonNumber::Float(v.value)),
            Tag::ByteArray(v) => Canon::List(
                v.data
                    .iter()
                    .map(|b| Canon::Number(CanonNumber::Integer(*b as i64)))
                    .collect(),
            ),
            Tag::String(v) => Canon::Str(v.value.clone()),
            Tag::List(v) => Canon::List(v.list.iter().map(|t| t.canon()).collect()),
            Tag::Compound(v) => Canon::Map(
                v.entry_set()
                    .map(|(k, tag)| (k.clone(), tag.canon()))
                    .collect(),
            ),
            Tag::IntArray(v) => Canon::List(
                v.data
                    .iter()
                    .map(|i| Canon::Number(CanonNumber::Integer(*i as i64)))
                    .collect(),
            ),
            Tag::LongArray(v) => Canon::List(
                v.data
                    .iter()
                    .map(|l| Canon::Number(CanonNumber::Integer(*l)))
                    .collect(),
            ),
            Tag::End(_) => Canon::Null,
        }
    }
}

/// Convenience bound for the DFU suite: a `DynamicOps` whose output is
/// canonicalizable (and therefore comparable across backends).
pub trait CanonOps: DynamicOps
where
    Self::Output: Canonical,
{
}
impl<O: DynamicOps> CanonOps for O where O::Output: Canonical {}

// ---------------------------------------------------------------------------
// Ops-parametric value builders
// ---------------------------------------------------------------------------

/// `ops.create_boolean(value)`.
pub fn v_bool<O: DynamicOps>(ops: &O, value: bool) -> O::Output {
    ops.create_boolean(value)
}

/// `ops.create_numeric(Number::Double(value))` — the float form.
///
/// `JsonOps` distinguishes an integer from a float (`create_int(1)` vs
/// `create_numeric(Double(1.0))`), so the float form must wrap in
/// `Number::Double`; `TestOps` collapses every number to `Value::Num`.
pub fn v_num<O: DynamicOps>(ops: &O, value: f64) -> O::Output {
    ops.create_numeric(rivet_serialization::number::Number::Double(value))
}

/// `ops.create_int(value)` — the integer form. `JsonOps` distinguishes int
/// from float (`create_int(1)` vs `create_numeric(1.0)`), so int-typed codec
/// fields must build their expected values with `v_int`, matching Java's
/// `JsonElement.equals`; `TestOps` is f64-only and collapses both.
pub fn v_int<O: DynamicOps>(ops: &O, value: i32) -> O::Output {
    ops.create_int(value)
}

/// `ops.create_string(value)`.
pub fn v_str<O: DynamicOps>(ops: &O, value: &str) -> O::Output {
    ops.create_string(value.to_string())
}

/// `ops.create_list(items)`.
pub fn v_list<O: DynamicOps>(ops: &O, items: Vec<O::Output>) -> O::Output {
    ops.create_list(items)
}

/// `ops.create_map(entries)` — string keys, values in insertion order.
pub fn v_map<O: DynamicOps>(ops: &O, entries: Vec<(&str, O::Output)>) -> O::Output {
    ops.create_map(
        entries
            .into_iter()
            .map(|(k, v)| Pair::of(ops.create_string(k.to_string()), v))
            .collect(),
    )
}

/// The map keys in insertion order (panics if `output` is not a map).
pub fn ordered_map_keys<O: Canonical>(output: &O) -> Vec<String> {
    match output.canon() {
        Canon::Map(entries) => entries.into_iter().map(|(k, _)| k).collect(),
        other => panic!("expected a map, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Ops-parametric assertion helpers
// ---------------------------------------------------------------------------

/// Assertion helpers mirroring the upstream `CodecTests` free functions
/// (`fromJava`, `assertFromJavaFails`, ...), implemented for any ops whose
/// output is `Canonical`.
///
/// The `from_*` methods take `&self` (they read through the ops), so clippy's
/// `wrong_self_convention` is allowed for the three that carry the `from_`
/// prefix.
pub trait OpsTestExt: DynamicOps + Sized + 'static
where
    Self::Output: Canonical,
{
    /// `codec.parse(ops, value)` → unwrapped decoded value.
    fn parse_or_throw<A>(&self, codec: &Arc<dyn Codec<A, Self>>, value: &Self::Output) -> A
    where
        A: 'static + Clone,
    {
        codec.parse(self, value).get_or_throw("parse").clone()
    }

    /// `codec.parse(ops, value).getPartialOrThrow(...)`.
    #[allow(clippy::wrong_self_convention)]
    fn from_java_or_partial<A>(&self, codec: &Arc<dyn Codec<A, Self>>, value: &Self::Output) -> A
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
    fn from_java_error_message<A>(
        &self,
        codec: &Arc<dyn Codec<A, Self>>,
        value: &Self::Output,
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
    fn assert_from_java_fails<A>(&self, codec: &Arc<dyn Codec<A, Self>>, value: &Self::Output)
    where
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
    fn assert_from_java_fails_partial<A>(
        &self,
        codec: &Arc<dyn Codec<A, Self>>,
        value: &Self::Output,
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
    fn assert_to_java_fails<A>(&self, codec: &Arc<dyn Codec<A, Self>>, value: &A)
    where
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
    /// The encode half is compared order-insensitively (a `HashMap`-backed
    /// source iterates nondeterministically, matching Java's unordered map
    /// values); the decode half compares the value directly.
    fn assert_round_trip<A>(&self, codec: &Arc<dyn Codec<A, Self>>, value: A, encoded: Self::Output)
    where
        A: 'static + Clone + PartialEq + Debug,
    {
        let to = codec
            .encode_start(self, &value)
            .get_or_throw("encodeStart")
            .clone();
        assert_eq!(
            to.canon().sorted(),
            encoded.canon().sorted(),
            "encodeStart did not match the expected value"
        );
        let from = codec.parse(self, &encoded).get_or_throw("parse").clone();
        assert_eq!(from, value, "parse did not round-trip the value");
    }
}

impl<O: DynamicOps + 'static> OpsTestExt for O where O::Output: Canonical {}
