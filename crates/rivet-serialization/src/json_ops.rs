//! Port of `com.mojang.serialization.JsonOps` — `DynamicOps<serde_json::Value>`.
//!
//! Java's `JsonElement` (Gson) maps to `serde_json::Value`. `INSTANCE` is the
//! non-compressed ops; `COMPRESSED` has `compressMaps() == true` and additionally
//! tolerates numeric strings in `getNumberValue`, numbers in `getStringValue`,
//! and non-string primitive keys in `mergeToMap` (coerced via `getAsString`).
//!
//! Translation notes:
//! - Gson `JsonNull` is a singleton that DFU treats as Java `null` (absence)
//!   throughout `getMapValues`/`getStream`/etc. `serde_json::Value::Null` is a
//!   real value that cannot be dropped from a `Vec<Pair<Value, Value>>`, so it is
//!   retained there; the *absence* signal is only honored where `Option` can
//!   express it (`MapLike.get`/`get_string` return `None` for a `Null` value,
//!   matching Java's `getMap().get(key)` → `null`).
//! - Java `getNumberValue` returns a boxed `Number`; ported as the typed
//!   [`Number`] enum. Gson stores an integer-literal `JsonPrimitive` as a
//!   `LazilyParsedNumber` whose `intValue()`/`longValue()` parse the literal
//!   (`Integer.parseInt` for `intValue`, `Long.parseLong` for `longValue`);
//!   `Number::Int`/`Number::Long` reproduce the parsed values, and
//!   `Number::Double` covers non-integral literals (`LazilyParsedNumber.doubleValue`
//!   is a plain `Double.parseDouble`).
//! - `createNumeric` wraps a Gson `JsonPrimitive(Number)`. Integral variants
//!   become exact `serde_json::Number`s; `Float`/`Double` go through
//!   `serde_json::Number::from_f64`, which rejects NaN/Infinity (JSON cannot
//!   represent them) — such values fall back to `Value::Null` (documented
//!   deviation; Gson would emit a `JsonPrimitive` that serializes as
//!   `NaN`/`Infinity`).
//! - `serde_json` is built with `preserve_order`, so `Value::Object` keeps
//!   insertion order like Gson's `LinkedTreeMap`.
//! - Java's `JsonOps` has no "pretty" variant in DFU 10.0.21 (`INSTANCE` and
//!   `COMPRESSED` only), so none is ported.
//! - Java's `JsonOps.listBuilder()` overrides the default with an
//!   `ArrayBuilder` (error text "Cannot append a list to not a list" and a
//!   `stable` lifecycle on build). This port uses the default `ListBuilder`
//!   (`mergeToList`, experimental lifecycle), which matches Java's *generic*
//!   `ListBuilder.Builder`; the deviation is limited to that error message and
//!   the build lifecycle, neither of which the DFU-mirroring tests assert.

use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, MapLike, Pair, RecordBuilder};
use crate::lifecycle::Lifecycle;
use crate::number::Number;
use serde_json::Number as JsonNumber;
use serde_json::{Map, Value};

/// `JsonOps.INSTANCE` / `JsonOps.COMPRESSED` — a `DynamicOps` over
/// `serde_json::Value`, faithful to `com.mojang.serialization.JsonOps`.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonOps {
    compressed: bool,
}

impl JsonOps {
    /// `JsonOps.INSTANCE`.
    pub const INSTANCE: JsonOps = JsonOps { compressed: false };

    /// `JsonOps.COMPRESSED`.
    pub const COMPRESSED: JsonOps = JsonOps { compressed: true };

    /// `new JsonOps(compressed)`.
    pub fn new(compressed: bool) -> Self {
        JsonOps { compressed }
    }
}

impl JsonOps {
    /// Resolve a `mergeToMap` key: string primitives always; other primitives
    /// (numbers/booleans) only when compressed — Java
    /// `key instanceof JsonPrimitive && (key.isString() || compressed)`,
    /// coerced via `getAsString()`.
    fn merge_key_string(&self, key: &Value) -> Option<String> {
        match key {
            Value::String(s) => Some(s.clone()),
            Value::Number(_) | Value::Bool(_) if self.compressed => Some(key.to_string()),
            _ => None,
        }
    }

    /// `getAsString()` for a primitive key — coerces strings, numbers and
    /// booleans to their string form (Gson `JsonElement.getAsString()`).
    /// Object/array/null keys are not valid map keys.
    fn primitive_key_string(key: &Value) -> Option<String> {
        match key {
            Value::String(s) => Some(s.clone()),
            Value::Number(_) | Value::Bool(_) => Some(key.to_string()),
            _ => None,
        }
    }
}

/// `MapLike<Value>` over a `serde_json::Map` (owns a clone so the box is
/// `'static`). `get` returns `None` for a `Null` value, mirroring Java's
/// `MapLike.get` → `null` for a `JsonNull` entry (absence).
#[derive(Debug)]
struct JsonMapLike {
    map: Map<String, Value>,
}

impl MapLike<Value> for JsonMapLike {
    fn get(&self, key: &Value) -> Option<Value> {
        let key = JsonOps::primitive_key_string(key)?;
        match self.map.get(&key) {
            Some(Value::Null) | None => None,
            Some(value) => Some(value.clone()),
        }
    }

    fn get_string(&self, key: &str) -> Option<Value> {
        match self.map.get(key) {
            Some(Value::Null) | None => None,
            Some(value) => Some(value.clone()),
        }
    }

    fn entries(&self) -> Vec<Pair<Value, Value>> {
        // Java `MapLike.entries()` keeps `JsonNull` values (only `getMapValues`
        // collapses them to null), so `Value::Null` is retained here.
        self.map
            .iter()
            .map(|(k, v)| Pair::of(Value::String(k.clone()), v.clone()))
            .collect()
    }
}

impl DynamicOps for JsonOps {
    type Output = Value;

    fn empty(&self) -> Value {
        Value::Null
    }

    fn empty_map(&self) -> Value {
        Value::Object(Map::new())
    }

    fn empty_list(&self) -> Value {
        Value::Array(Vec::new())
    }

    /// `convertTo(DynamicOps<U>, JsonElement)`.
    ///
    /// Java narrows the numeric value via `BigDecimal.longValueExact()`:
    /// byte/short/int/long when integral and in range, otherwise the double
    /// path (`(float) d == d` → float, else double). serde_json parses integer
    /// literals as `i64`/`u64` exactly and floats as `f64`; the f64 path
    /// applies the same BigDecimal-like integrality check.
    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &Value) -> U::Output {
        match input {
            Value::Object(_) => self.convert_map(out_ops, input),
            Value::Array(_) => self.convert_list(out_ops, input),
            Value::Null => out_ops.empty(),
            Value::String(s) => out_ops.create_string(s.clone()),
            Value::Bool(b) => out_ops.create_boolean(*b),
            Value::Number(n) => match exact_integral(n) {
                Some(l) => {
                    if l as i8 as i64 == l {
                        return out_ops.create_byte(l as i8);
                    }
                    if l as i16 as i64 == l {
                        return out_ops.create_short(l as i16);
                    }
                    if l as i32 as i64 == l {
                        return out_ops.create_int(l as i32);
                    }
                    out_ops.create_long(l)
                }
                None => {
                    let d = n.as_f64().unwrap_or(f64::NAN);
                    if d as f32 as f64 == d {
                        out_ops.create_float(d as f32)
                    } else {
                        out_ops.create_double(d)
                    }
                }
            },
        }
    }

    fn get_number_value(&self, input: &Value) -> DataResult<Number> {
        match input {
            Value::Number(n) => DataResult::success(number_from_json(n)),
            Value::String(s) if self.compressed => match s.parse::<i32>() {
                // Java `Integer.parseInt` (not long) for the compressed
                // string form.
                Ok(v) => DataResult::success(Number::Int(v)),
                Err(e) => DataResult::error(format!("Not a number: {e} {input}")),
            },
            _ => DataResult::error(format!("Not a number: {input}")),
        }
    }

    fn create_numeric(&self, value: Number) -> Value {
        json_from_number(value)
    }

    fn create_byte(&self, value: i8) -> Value {
        Value::Number(JsonNumber::from(value))
    }

    fn create_short(&self, value: i16) -> Value {
        Value::Number(JsonNumber::from(value))
    }

    fn create_int(&self, value: i32) -> Value {
        Value::Number(JsonNumber::from(value))
    }

    fn create_long(&self, value: i64) -> Value {
        Value::Number(JsonNumber::from(value))
    }

    fn create_float(&self, value: f32) -> Value {
        json_from_number(Number::Float(value))
    }

    fn create_double(&self, value: f64) -> Value {
        json_from_number(Number::Double(value))
    }

    fn get_boolean_value(&self, input: &Value) -> DataResult<bool> {
        match input {
            Value::Bool(b) => DataResult::success(*b),
            _ => DataResult::error(format!("Not a boolean: {input}")),
        }
    }

    fn create_boolean(&self, value: bool) -> Value {
        Value::Bool(value)
    }

    fn get_string_value(&self, input: &Value) -> DataResult<String> {
        match input {
            Value::String(s) => DataResult::success(s.clone()),
            // Java: `isNumber() && compressed` → `input.getAsString()`.
            Value::Number(_) if self.compressed => DataResult::success(input.to_string()),
            _ => DataResult::error(format!("Not a string: {input}")),
        }
    }

    fn create_string(&self, value: String) -> Value {
        Value::String(value)
    }

    fn merge_to_list(&self, list: &Value, value: Value) -> DataResult<Value> {
        if !matches!(list, Value::Array(_)) && *list != self.empty() {
            return DataResult::error_with_partial(
                format!("mergeToList called with not a list: {list}"),
                list.clone(),
            );
        }
        let mut result = match list {
            Value::Array(array) => array.clone(),
            _ => Vec::new(),
        };
        result.push(value);
        DataResult::success(Value::Array(result))
    }

    fn merge_to_list_many(&self, list: &Value, values: Vec<Value>) -> DataResult<Value> {
        if !matches!(list, Value::Array(_)) && *list != self.empty() {
            return DataResult::error_with_partial(
                format!("mergeToList called with not a list: {list}"),
                list.clone(),
            );
        }
        if values.is_empty() {
            return if *list == self.empty() {
                DataResult::success(self.empty_list())
            } else {
                DataResult::success(list.clone())
            };
        }
        let mut result = match list {
            Value::Array(array) => array.clone(),
            _ => Vec::new(),
        };
        result.extend(values);
        DataResult::success(Value::Array(result))
    }

    fn merge_to_map(&self, map: &Value, key: Value, value: Value) -> DataResult<Value> {
        if !matches!(map, Value::Object(_)) && *map != self.empty() {
            return DataResult::error_with_partial(
                format!("mergeToMap called with not a map: {map}"),
                map.clone(),
            );
        }
        let key = match self.merge_key_string(&key) {
            Some(k) => k,
            None => {
                return DataResult::error_with_partial(
                    format!("key is not a string: {key}"),
                    map.clone(),
                );
            }
        };
        let mut output = match map {
            Value::Object(object) => object.clone(),
            _ => Map::new(),
        };
        output.insert(key, value);
        DataResult::success(Value::Object(output))
    }

    fn merge_to_map_like(&self, map: &Value, values: &dyn MapLike<Value>) -> DataResult<Value> {
        if !matches!(map, Value::Object(_)) && *map != self.empty() {
            return DataResult::error_with_partial(
                format!("mergeToMap called with not a map: {map}"),
                map.clone(),
            );
        }
        let mut entries = values.entries().into_iter();
        let Some(first) = entries.next() else {
            return if *map == self.empty() {
                DataResult::success(self.empty_map())
            } else {
                DataResult::success(map.clone())
            };
        };
        let mut output = match map {
            Value::Object(object) => object.clone(),
            _ => Map::new(),
        };
        let mut missed = Vec::new();
        for entry in std::iter::once(first).chain(entries) {
            match self.merge_key_string(&entry.first) {
                Some(k) => {
                    output.insert(k, entry.second);
                }
                None => missed.push(entry.first),
            }
        }
        if !missed.is_empty() {
            DataResult::error_with_partial(
                format!("some keys are not strings: {missed:?}"),
                Value::Object(output),
            )
        } else {
            DataResult::success(Value::Object(output))
        }
    }

    fn get_map_values(&self, input: &Value) -> DataResult<Vec<Pair<Value, Value>>> {
        match input {
            Value::Object(object) => DataResult::success(
                object
                    .iter()
                    .map(|(k, v)| Pair::of(Value::String(k.clone()), v.clone()))
                    .collect(),
            ),
            _ => DataResult::error(format!("Not a JSON object: {input}")),
        }
    }

    fn get_map_entries(
        &self,
        input: &Value,
    ) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Value, &Value))>> {
        match input {
            Value::Object(object) => {
                let entries: Vec<(String, Value)> =
                    object.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                DataResult::success(Box::new(move |consumer: &mut dyn FnMut(&Value, &Value)| {
                    for (k, v) in &entries {
                        consumer(&Value::String(k.clone()), v);
                    }
                }))
            }
            _ => DataResult::error(format!("Not a JSON object: {input}")),
        }
    }

    fn get_map(&self, input: &Value) -> DataResult<Box<dyn MapLike<Value>>> {
        match input {
            Value::Object(object) => DataResult::success(Box::new(JsonMapLike {
                map: object.clone(),
            })),
            _ => DataResult::error(format!("Not a JSON object: {input}")),
        }
    }

    fn create_map(&self, map: Vec<Pair<Value, Value>>) -> Value {
        let mut result = Map::new();
        for entry in map {
            let key = match JsonOps::primitive_key_string(&entry.first) {
                Some(k) => k,
                // Java's `createMap` stringifies any primitive key; object,
                // array and null keys are invalid (Gson `getAsString` throws on
                // object/array; `null` would silently key on "null" — rejected
                // here deliberately, matching the `NbtOps` panic convention).
                None => panic!(
                    "Cannot create map with non-primitive key: {:?}",
                    entry.first
                ),
            };
            result.insert(key, entry.second);
        }
        Value::Object(result)
    }

    fn get_stream(&self, input: &Value) -> DataResult<Vec<Value>> {
        match input {
            Value::Array(array) => DataResult::success(array.clone()),
            _ => DataResult::error(format!("Not a json array: {input}")),
        }
    }

    fn get_list(&self, input: &Value) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Value))>> {
        match input {
            Value::Array(array) => {
                let items = array.clone();
                DataResult::success(Box::new(move |consumer: &mut dyn FnMut(&Value)| {
                    for v in &items {
                        consumer(v);
                    }
                }))
            }
            _ => DataResult::error(format!("Not a json array: {input}")),
        }
    }

    fn create_list(&self, input: Vec<Value>) -> Value {
        Value::Array(input)
    }

    fn get_byte_buffer(&self, input: &Value) -> DataResult<Vec<u8>> {
        // Java `DynamicOps.getByteBuffer` default: every stream element must be
        // a number (narrowed to byte).
        self.get_stream(input).map_or_else(
            |elements| {
                let mut buffer = Vec::with_capacity(elements.len());
                let mut all_numbers = true;
                for e in elements {
                    match self.get_number_value(e).result() {
                        // Java `Number.byteValue()`.
                        Some(n) => buffer.push(n.byte_value() as u8),
                        None => {
                            all_numbers = false;
                            break;
                        }
                    }
                }
                if all_numbers {
                    DataResult::success(buffer)
                } else {
                    DataResult::error(format!("Some elements are not bytes: {input}"))
                }
            },
            |error| {
                DataResult::error_with_lifecycle(error.message().to_string(), error.lifecycle())
            },
        )
    }

    fn create_byte_list(&self, input: &[u8]) -> Value {
        Value::Array(input.iter().map(|v| self.create_byte(*v as i8)).collect())
    }

    fn get_int_stream(&self, input: &Value) -> DataResult<Vec<i32>> {
        // Java `DynamicOps.getIntStream` default — same shape as
        // `getByteBuffer` but casting to int.
        self.get_stream(input).map_or_else(
            |elements| {
                let mut stream = Vec::with_capacity(elements.len());
                let mut all_numbers = true;
                for e in elements {
                    match self.get_number_value(e).result() {
                        // Java `Number.intValue()`.
                        Some(n) => stream.push(n.int_value()),
                        None => {
                            all_numbers = false;
                            break;
                        }
                    }
                }
                if all_numbers {
                    DataResult::success(stream)
                } else {
                    DataResult::error(format!("Some elements are not ints: {input}"))
                }
            },
            |error| {
                DataResult::error_with_lifecycle(error.message().to_string(), error.lifecycle())
            },
        )
    }

    fn create_int_list(&self, input: Vec<i32>) -> Value {
        Value::Array(input.iter().map(|v| self.create_int(*v)).collect())
    }

    fn get_long_stream(&self, input: &Value) -> DataResult<Vec<i64>> {
        // Java `DynamicOps.getLongStream` default — same shape as
        // `getByteBuffer` but casting to long.
        self.get_stream(input).map_or_else(
            |elements| {
                let mut stream = Vec::with_capacity(elements.len());
                let mut all_numbers = true;
                for e in elements {
                    match self.get_number_value(e).result() {
                        // Java `Number.longValue()`.
                        Some(n) => stream.push(n.long_value()),
                        None => {
                            all_numbers = false;
                            break;
                        }
                    }
                }
                if all_numbers {
                    DataResult::success(stream)
                } else {
                    DataResult::error(format!("Some elements are not longs: {input}"))
                }
            },
            |error| {
                DataResult::error_with_lifecycle(error.message().to_string(), error.lifecycle())
            },
        )
    }

    fn create_long_list(&self, input: Vec<i64>) -> Value {
        Value::Array(input.iter().map(|v| self.create_long(*v)).collect())
    }

    fn remove(&self, input: Value, key: &str) -> Value {
        match input {
            Value::Object(mut object) => {
                object.remove(key);
                Value::Object(object)
            }
            other => other,
        }
    }

    fn compress_maps(&self) -> bool {
        self.compressed
    }

    fn map_builder(&self) -> Box<dyn RecordBuilder<Output = Value> + '_> {
        Box::new(JsonRecordBuilder::new(self))
    }
}

/// The Java `JsonOps.JsonRecordBuilder` — `AbstractStringBuilder<JsonElement,
/// JsonObject>`: accumulates string-keyed entries (errors propagate via the
/// `DataResult` state) and merges into a prefix on `build`.
struct JsonRecordBuilder<'a> {
    ops: &'a JsonOps,
    builder: DataResult<Map<String, Value>>,
}

impl<'a> std::fmt::Debug for JsonRecordBuilder<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsonRecordBuilder")
    }
}

impl<'a> JsonRecordBuilder<'a> {
    /// `new JsonRecordBuilder(JsonOps)`.
    fn new(ops: &'a JsonOps) -> Self {
        JsonRecordBuilder {
            ops,
            builder: DataResult::success_with_lifecycle(Map::new(), Lifecycle::stable()),
        }
    }

    /// `AbstractStringBuilder.build(R builder, T prefix)`.
    fn build_inner(&self, builder: Map<String, Value>, prefix: Option<Value>) -> DataResult<Value> {
        match prefix {
            None | Some(Value::Null) => DataResult::success(Value::Object(builder)),
            Some(Value::Object(prefix)) => {
                // Prefix entries first, then builder entries overwrite.
                let mut result = prefix;
                for (k, v) in builder {
                    result.insert(k, v);
                }
                DataResult::success(Value::Object(result))
            }
            Some(other) => DataResult::error_with_partial(
                format!("mergeToMap called with not a map: {other}"),
                other,
            ),
        }
    }
}

impl<'a> RecordBuilder for JsonRecordBuilder<'a> {
    type Output = Value;

    /// `AbstractBuilder.build(T prefix)` — resets the accumulated state.
    fn build(&mut self, prefix: Option<Value>) -> DataResult<Value> {
        let builder = self.builder.clone();
        let result = builder.flat_map(|b| self.build_inner(b, prefix));
        self.builder = DataResult::success_with_lifecycle(Map::new(), Lifecycle::stable());
        result
    }

    /// `AbstractStringBuilder.add(T key, T value)` — the key must resolve
    /// through `getStringValue`; a failing key replaces the builder state with
    /// the error (Java `ops().getStringValue(key).flatMap(...)`).
    fn add(&mut self, key: Value, value: Value) {
        let key_result = self.ops.get_string_value(&key);
        let prev = self.builder.clone();
        self.builder = key_result.flat_map(move |k| {
            prev.map_owned(move |mut b| {
                b.insert(k.clone(), value.clone());
                b
            })
        });
    }

    /// `AbstractStringBuilder.add(String key, T value)`.
    fn add_string(&mut self, key: &str, value: Value) {
        let prev = self.builder.clone();
        let k = key.to_string();
        self.builder = prev.map_owned(move |mut b| {
            b.insert(k.clone(), value);
            b
        });
    }

    /// `AbstractStringBuilder.add(T key, DataResult<T> value)`.
    fn add_result(&mut self, key: Value, value: DataResult<Value>) {
        let key_result = self.ops.get_string_value(&key);
        let prev = self.builder.clone();
        self.builder = key_result.flat_map(move |k| {
            prev.apply2_stable(
                move |b: &Map<String, Value>, v: &Value| {
                    let mut b = b.clone();
                    b.insert(k.clone(), v.clone());
                    b
                },
                value,
            )
        });
    }

    /// `AbstractStringBuilder.add(DataResult<T> key, DataResult<T> value)`.
    fn add_result_result(&mut self, key: DataResult<Value>, value: DataResult<Value>) {
        let key_string = key.flat_map(|k| self.ops.get_string_value(&k));
        let prev = self.builder.clone();
        self.builder = key_string.flat_map(move |k| {
            prev.apply2_stable(
                move |b: &Map<String, Value>, v: &Value| {
                    let mut b = b.clone();
                    b.insert(k.clone(), v.clone());
                    b
                },
                value,
            )
        });
    }

    /// `AbstractStringBuilder.add(String key, DataResult<T> value)`.
    fn add_string_result(&mut self, key: &str, value: DataResult<Value>) {
        let prev = self.builder.clone();
        let k = key.to_string();
        self.builder = prev.apply2_stable(
            move |b: &Map<String, Value>, v: &Value| {
                let mut b = b.clone();
                b.insert(k.clone(), v.clone());
                b
            },
            value,
        );
    }

    /// `AbstractBuilder.withErrorsFrom(DataResult<?>)`.
    fn with_errors_from(&mut self, result: &DataResult<()>) {
        let r = result.clone();
        self.builder = self.builder.clone().flat_map(|v| r.map(|_| v));
    }

    /// `AbstractBuilder.setLifecycle(Lifecycle)`.
    fn set_lifecycle(&mut self, lifecycle: Lifecycle) {
        self.builder = self.builder.clone().set_lifecycle(lifecycle);
    }

    /// `AbstractBuilder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        self.builder = self.builder.clone().map_error(on_error);
    }
}

/// `BigDecimal.longValueExact()` for a `serde_json::Number`: `Some(i64)` when
/// the value is integral and within long range, else `None` (Java throws
/// `ArithmeticException`, falling through to the double path).
fn exact_integral(number: &JsonNumber) -> Option<i64> {
    if number.is_f64() {
        let d = number.as_f64()?;
        // `i64::MAX as f64` rounds up to 2^63, so `d < i64::MAX as f64`
        // (not `<=`) rejects d == 2^63 — Java `BigDecimal.longValueExact`
        // throws for that value, falling through to the double/float path.
        // The largest f64 below 2^63 (2^63 - 2048) still fits in i64.
        if d.fract() == 0.0 && d >= i64::MIN as f64 && d < i64::MAX as f64 {
            return Some(d as i64);
        }
        return None;
    }
    if let Some(i) = number.as_i64() {
        return Some(i);
    }
    number
        .as_u64()
        .and_then(|u| (u <= i64::MAX as u64).then_some(u as i64))
}

/// `getNumberValue` on a `serde_json::Number` → typed `Number`.
///
/// Mirrors Gson `JsonPrimitive.getAsNumber()`: an integer literal yields a
/// `LazilyParsedNumber` whose `intValue()`/`longValue()` are
/// `Integer.parseInt`/`Long.parseLong` of the literal, and `doubleValue()` is
/// `Double.parseDouble`. serde_json parses integer literals as `i64`/`u64`
/// exactly and floats as `f64`, so:
/// - integral value in i32 range → `Number::Int` (matches `LazilyParsedNumber.intValue`)
/// - integral value in i64 range → `Number::Long` (matches `longValue`)
/// - otherwise (float, or out-of-i64-range integral) → `Number::Double`.
///
/// Gson wraps the *original* literal so `longValue()` of a value stored as a
/// float literal returns the parse of that literal; here a float-literal
/// `JsonNumber` is `f64`, so `longValue()` goes through `f64 → i64` — the same
/// value Java's `doubleValue()` then `longValue()` would yield for the double,
/// but Java's `LazilyParsedNumber` would parse the literal exactly. This
/// matches how the DFU suite round-trips numbers (integral literals are parsed
/// exactly by serde_json).
fn number_from_json(number: &JsonNumber) -> Number {
    if let Some(i) = number.as_i64() {
        return if i32::try_from(i).is_ok() {
            Number::Int(i as i32)
        } else {
            Number::Long(i)
        };
    }
    if let Some(u) = number.as_u64() {
        return if u <= i64::MAX as u64 {
            Number::Long(u as i64)
        } else {
            Number::Double(u as f64)
        };
    }
    Number::Double(number.as_f64().unwrap_or(f64::NAN))
}

/// `createNumeric(Number)` — typed `Number` → `serde_json::Number` in a `Value`.
///
/// Java `JsonOps.createNumeric` is `new JsonPrimitive(Number)`, which stores
/// the exact `Number`. Integral variants become the exact `serde_json::Number`;
/// `Float`/`Double` go through `serde_json::Number::from_f64`, which returns
/// `None` for NaN/Infinity — those fall back to `Value::Null` (documented
/// deviation, see module docs).
fn json_from_number(number: Number) -> Value {
    match number {
        Number::Byte(v) => Value::Number(JsonNumber::from(v)),
        Number::Short(v) => Value::Number(JsonNumber::from(v)),
        Number::Int(v) => Value::Number(JsonNumber::from(v)),
        Number::Long(v) => Value::Number(JsonNumber::from(v)),
        Number::Float(v) => Value::Number(match JsonNumber::from_f64(v as f64) {
            Some(n) => n,
            None => return Value::Null,
        }),
        Number::Double(v) => Value::Number(match JsonNumber::from_f64(v) {
            Some(n) => n,
            None => return Value::Null,
        }),
    }
}
