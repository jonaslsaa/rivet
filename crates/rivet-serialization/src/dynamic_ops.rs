//! Port of `com.mojang.serialization.DynamicOps` (plus `MapLike`,
//! `RecordBuilder`, `ListBuilder`, `Keyable`/`Compressable`/`CompressorHolder`/
//! `KeyCompressor`).
//!
//! `DynamicOps<T>` in Java has the element type as a type parameter and every
//! method returns `DataResult<Number>`/`createNumeric(Number)` etc. Following
//! the existing STUB(mc.nbt) surface, `getNumberValue`/`createNumeric` here use
//! `f64` (see `nbt_ops.rs` fidelity note); the primitive `create*` methods keep
//! their exact Java types.
//!
//! `DynamicOps` is not object-safe in Java (`convertTo` is generic over the
//! *other* ops), so `&dyn DynamicOps` is impossible in Rust. The codec traits
//! (`Codec`/`Decoder`/`Encoder`/`MapCodec`) are therefore parameterized by the
//! concrete ops type (`Codec<A, Ops: DynamicOps + 'static>`), mirroring Java's `Codec<A>`
//! but pinned to the ops it is used with. This is a documented, necessary
//! deviation (see `lib.rs`).

pub use crate::pair::Pair;

use crate::data_result::DataResult;
use crate::lifecycle::Lifecycle;
use std::fmt::Debug;
use std::sync::Arc;

/// `getMapEntries` consumer — feeds each map entry pair to the callback.
type MapEntryConsumer<O> = Box<dyn Fn(&mut dyn FnMut(&O, &O))>;

/// `getList` consumer — feeds each list element to the callback.
type ListConsumer<O> = Box<dyn Fn(&mut dyn FnMut(&O))>;

/// `com.mojang.serialization.MapLike<T>`.
///
/// Methods take `&self` and return owned values; Java's `@Nullable T get` maps
/// to `Option<T>`. `entries()` returns the entries in iteration order
/// (deterministic for the underlying ops' map).
pub trait MapLike<T>: Debug {
    fn get(&self, key: &T) -> Option<T>;

    /// `MapLike.get(String)` — `get(ops.createString(key))`.
    fn get_string(&self, key: &str) -> Option<T>;

    fn entries(&self) -> Vec<Pair<T, T>>;
}

/// `MapLike.EMPTY` — the shared empty singleton (`MapLike.empty()`).
#[derive(Debug, Clone, Copy)]
pub struct EmptyMapLike;

impl<T> MapLike<T> for EmptyMapLike {
    fn get(&self, _key: &T) -> Option<T> {
        None
    }

    fn get_string(&self, _key: &str) -> Option<T> {
        None
    }

    fn entries(&self) -> Vec<Pair<T, T>> {
        Vec::new()
    }
}

impl<T> MapLike<T> for Vec<Pair<T, T>>
where
    T: Clone + PartialEq + Debug,
{
    fn get(&self, key: &T) -> Option<T> {
        self.iter()
            .find(|p| &p.first == key)
            .map(|p| p.second.clone())
    }

    fn get_string(&self, _key: &str) -> Option<T> {
        None
    }

    fn entries(&self) -> Vec<Pair<T, T>> {
        self.clone()
    }
}

/// `com.mojang.serialization.RecordBuilder<T>`.
///
/// Backward-compatible with the STUB(mc.nbt) shape that `rivet-nbt`'s
/// `NbtRecordBuilder` implements (`build(&mut self, Option<Output>)`). The
/// mutating `add`/`withErrorsFrom`/`setLifecycle`/`mapError` conveniences
/// default to no-ops (STUB(mc.nbt)) so existing implementors keep compiling —
/// an implementor that does not override them silently swallows encode output
/// until `rivet-nbt`'s full `MapBuilder` port lands. The accumulating
/// reference builder is `RecordBuilderImpl` (the Java `MapBuilder` port).
pub trait RecordBuilder: Debug {
    type Output;

    /// `RecordBuilder.build(T prefix)` — the required method (Java signature
    /// `build(T)`); `prefix: Option` preserves the stub's reduced shape.
    /// Java `AbstractBuilder.build` resets the accumulated state after each
    /// build, hence `&mut self`.
    fn build(&mut self, prefix: Option<Self::Output>) -> DataResult<Self::Output>;

    /// `RecordBuilder.add(T key, T value)`.
    fn add(&mut self, _key: Self::Output, _value: Self::Output) {}

    /// `RecordBuilder.add(String key, T value)` — `add(ops.createString(key), value)`.
    fn add_string(&mut self, _key: &str, _value: Self::Output) {}

    /// `RecordBuilder.add(T key, DataResult<T> value)`.
    fn add_result(&mut self, _key: Self::Output, _value: DataResult<Self::Output>) {}

    /// `RecordBuilder.add(DataResult<T> key, DataResult<T> value)` — Java's
    /// `AbstractUniversalBuilder.add(DataResult, DataResult)`: resolves the key
    /// through the ops and appends when both are present. The default is a
    /// STUB(mc.nbt) no-op for the reduced `NbtRecordBuilder` shape; the full
    /// `RecordBuilderImpl` implements it.
    fn add_result_result(
        &mut self,
        _key: DataResult<Self::Output>,
        _value: DataResult<Self::Output>,
    ) {
    }

    /// `RecordBuilder.add(String key, DataResult<T> value)`.
    fn add_string_result(&mut self, _key: &str, _value: DataResult<Self::Output>) {}

    /// `RecordBuilder.withErrorsFrom(DataResult<?>)`.
    fn with_errors_from(&mut self, _result: &DataResult<()>) {}

    /// `RecordBuilder.setLifecycle(Lifecycle)`.
    fn set_lifecycle(&mut self, _lifecycle: Lifecycle) {}

    /// `RecordBuilder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, _on_error: Box<dyn Fn(String) -> String>) {}

    /// `build(T prefix)` — convenience for a concrete prefix (Java's `build(T)`).
    fn build_with_prefix(&mut self, prefix: Self::Output) -> DataResult<Self::Output> {
        self.build(Some(prefix))
    }

    /// `RecordBuilder.build(DataResult<T> prefix)`.
    fn build_result(&mut self, prefix: DataResult<Self::Output>) -> DataResult<Self::Output> {
        prefix.flat_map(|p| self.build_with_prefix(p))
    }
}

/// `com.mojang.serialization.ListBuilder<T>`.
pub trait ListBuilder: Debug {
    type Output;

    /// `ListBuilder.add(T value)`.
    fn add(&mut self, _value: Self::Output) {}

    /// `ListBuilder.add(DataResult<T> value)`.
    fn add_result(&mut self, _value: DataResult<Self::Output>) {}

    /// `ListBuilder.withErrorsFrom(DataResult<?>)`.
    fn with_errors_from(&mut self, _result: &DataResult<()>) {}

    /// `ListBuilder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, _on_error: Box<dyn Fn(String) -> String>) {}

    /// `ListBuilder.build(T prefix)`.
    fn build(&mut self, prefix: Self::Output) -> DataResult<Self::Output>;

    /// `ListBuilder.build(DataResult<T> prefix)`.
    fn build_result(&mut self, prefix: DataResult<Self::Output>) -> DataResult<Self::Output> {
        prefix.flat_map(|p| self.build(p))
    }
}

/// `com.mojang.serialization.Keyable`.
///
/// Java's `Keyable.keys(DynamicOps<T>)` is generic over the ops; in Rust the
/// ops are pinned as a trait parameter (`Keyable<Ops>`), making `keys` a
/// concrete method so the trait is dyn-compatible.
pub trait Keyable<Ops: DynamicOps + 'static> {
    /// `Keyable.keys(DynamicOps<T>)`.
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output>;
}

/// `com.mojang.serialization.Compressable` — extends `Keyable`.
pub trait Compressable<Ops: DynamicOps + 'static>: Keyable<Ops> {
    /// `Compressable.compressor(DynamicOps<T>)`.
    fn compressor(&self, ops: &Ops) -> KeyCompressor<Ops::Output>;
}

/// `com.mojang.serialization.CompressorHolder` — Java memoizes the
/// `KeyCompressor` per ops identity. `DynamicOps` is not object-safe, so the
/// memoization can't live here; the marker trait keeps the name greppable.
pub trait CompressorHolder<Ops: DynamicOps + 'static>: Compressable<Ops> {}

/// `com.mojang.serialization.KeyCompressor<T>`.
///
/// Java uses fastutil `Int2ObjectArrayMap`/`Object2IntArrayMap` with
/// `defaultReturnValue(-1)`; a `Vec` preserves the same dense index space and
/// the `usize::MAX` sentinel mirrors `-1` for a missing key.
#[derive(Debug, Clone)]
pub struct KeyCompressor<T> {
    decompress: Vec<T>,
    compress: Vec<(T, usize)>,
    compress_string: Vec<(String, usize)>,
    size: usize,
}

impl<T> KeyCompressor<T> {
    /// `KeyCompressor(DynamicOps<T>, Stream<T> keys)`.
    ///
    /// Java's constructor also fills `compressString` from
    /// `ops.getStringValue(key)`; without the ops (and an empty-string
    /// placeholder would be wrong), use `new_with_strings` when the string
    /// table is needed. The `compressString` table starts empty so
    /// `compress_string` returns the `usize::MAX` sentinel for a real key.
    pub fn new(keys: Vec<T>) -> KeyCompressor<T>
    where
        T: Clone + PartialEq,
    {
        let mut decompress = Vec::new();
        let mut compress = Vec::new();

        for key in keys {
            if compress.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let next = compress.len();
            compress.push((key.clone(), next));
            decompress.push(key);
        }

        let size = compress.len();
        KeyCompressor {
            decompress,
            compress,
            compress_string: Vec::new(),
            size,
        }
    }

    /// `KeyCompressor(DynamicOps<T>, Stream<T> keys)` — variant that also
    /// populates the `compressString` table from `ops.getStringValue(key)`.
    pub fn new_with_strings<O: DynamicOps<Output = T>>(ops: &O, keys: Vec<T>) -> KeyCompressor<T>
    where
        T: Clone + PartialEq,
    {
        let mut compressor = KeyCompressor::new(keys);
        let mut out = Vec::new();
        for (k, idx) in &compressor.compress {
            if let Some(s) = ops.get_string_value(k).result() {
                out.push((s.clone(), *idx));
            }
        }
        compressor.compress_string = out;
        compressor
    }

    /// `KeyCompressor.decompress(int)`.
    pub fn decompress(&self, key: usize) -> Option<&T> {
        self.decompress.get(key)
    }

    /// `KeyCompressor.compress(String)`.
    pub fn compress_string(&self, key: &str) -> usize {
        match self.compress_string.iter().find(|(k, _)| k == key) {
            Some((_, id)) => *id,
            None => usize::MAX,
        }
    }

    /// `KeyCompressor.compress(T)`.
    pub fn compress_key(&self, key: &T) -> usize
    where
        T: PartialEq,
    {
        match self.compress.iter().find(|(k, _)| *k == *key) {
            Some((_, id)) => *id,
            None => usize::MAX,
        }
    }

    /// `KeyCompressor.size()`.
    pub fn size(&self) -> usize {
        self.size
    }
}

/// The Java `RecordBuilder.MapBuilder` — an accumulating record builder over a
/// `Vec<Pair<Output, Output>>`, merging into a prefix via
/// `mergeToMap` (buildKeepingLast semantics). `builder` mirrors
/// `AbstractBuilder.builder` (the accumulated `DataResult<R>` error state,
/// starting at `DataResult.success(initBuilder(), Lifecycle.stable())`).
pub struct RecordBuilderImpl<'a, O: DynamicOps> {
    ops: &'a O,
    builder: DataResult<()>,
    entries: Vec<Pair<O::Output, O::Output>>,
}
impl<'a, O: DynamicOps> std::fmt::Debug for RecordBuilderImpl<'a, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecordBuilderImpl")
    }
}

impl<'a, O: DynamicOps> RecordBuilderImpl<'a, O> {
    /// `new MapBuilder<>(ops)`.
    pub fn new(ops: &'a O) -> Self {
        RecordBuilderImpl {
            ops,
            builder: DataResult::success_with_lifecycle((), Lifecycle::stable()),
            entries: Vec::new(),
        }
    }

    /// `AbstractBuilder.ops()`.
    pub fn ops(&self) -> &'a O {
        self.ops
    }
}

impl<'a, O: DynamicOps> RecordBuilder for RecordBuilderImpl<'a, O> {
    type Output = O::Output;

    /// `MapBuilder.append(key, value, builder)` → `builder.put(key, value)`.
    fn add(&mut self, key: O::Output, value: O::Output) {
        self.entries.push(Pair::of(key, value));
    }

    /// `AbstractUniversalBuilder.add(T key, T value)`.
    fn add_string(&mut self, key: &str, value: O::Output) {
        self.add(self.ops.create_string(key.to_string()), value);
    }

    /// `AbstractUniversalBuilder.add(T key, DataResult<T> value)` —
    /// `builder.apply2stable((b, v) -> append(key, v, b), value)`.
    fn add_result(&mut self, key: O::Output, value: DataResult<O::Output>) {
        let opt = value.clone().result_or_partial_silent();
        if let Some(v) = opt {
            self.entries.push(Pair::of(key, v));
        }
        let builder = self.builder.clone();
        self.builder = builder.apply2_stable(|_, _| (), value.map(|_| ()));
    }

    /// `AbstractUniversalBuilder.add(String key, DataResult<T> value)`.
    fn add_string_result(&mut self, key: &str, value: DataResult<O::Output>) {
        self.add_result(self.ops.create_string(key.to_string()), value);
    }

    /// `AbstractUniversalBuilder.add(DataResult<T> key, DataResult<T> value)` —
    /// Java `builder.ap(key.apply2stable((k, v) -> b -> append(k, v, b), value))`.
    /// The resolved key/value pair is appended (mirroring `add_result`'s
    /// result-or-partial push) and the error state is accumulated via `ap`.
    fn add_result_result(&mut self, key: DataResult<O::Output>, value: DataResult<O::Output>) {
        // Push the entry when both key and value have a result-or-partial
        // (Java's `append` inside the ap function).
        let ok = key.clone().result_or_partial_silent();
        let ov = value.clone().result_or_partial_silent();
        if let (Some(k), Some(v)) = (ok, ov) {
            self.entries.push(Pair::of(k, v));
        }
        // Thread the combined key+value error state through `()` values (the
        // builder state is `DataResult<()>`; `O::Output` is not `'static`, so
        // the pair cannot be carried through the `Arc<dyn Fn>` applicative).
        let key_unit = key.map(|_| ());
        let value_unit = value.map(|_| ());
        let combined: DataResult<()> = key_unit.apply2_stable(|_, _| (), value_unit);
        let builder = self.builder.clone();
        let noop: Arc<dyn Fn(&())> = Arc::new(|_| {});
        self.builder = builder.ap(combined.map(|_| noop));
    }

    /// `AbstractBuilder.withErrorsFrom(DataResult<?>)` —
    /// `builder.flatMap(v -> result.map(r -> v))`.
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
        let builder = self.builder.clone();
        self.builder = builder.map_error(on_error);
    }

    /// `MapBuilder.build(ImmutableMap.Builder, T prefix)` —
    /// `ops.mergeToMap(prefix, builder.buildKeepingLast())`, combined with the
    /// accumulated error state (`AbstractBuilder.build`:
    /// `builder.flatMap(b -> build(b, prefix))`). Java resets the accumulated
    /// state after each build, so a reused builder starts fresh.
    fn build(&mut self, prefix: Option<O::Output>) -> DataResult<O::Output> {
        let entries = self.entries.clone();
        let builder = self.builder.clone();
        let prefix = prefix.unwrap_or_else(|| self.ops.empty());
        let result = builder.flat_map(|_| self.ops.merge_to_map_many(&prefix, entries));
        self.builder = DataResult::success_with_lifecycle((), Lifecycle::stable());
        self.entries.clear();
        result
    }
}

/// `com.mojang.serialization.DynamicOps<T>`.
///
/// Required methods are exactly the surface `rivet-nbt`'s `NbtOps` implements;
/// everything added here is defaulted.
pub trait DynamicOps: Debug {
    /// The ops element type — `DynamicOps<T>`'s `T`.
    type Output: Debug + Clone + PartialEq;

    /// `empty()`.
    fn empty(&self) -> Self::Output;

    /// `emptyMap()` — `createMap(ImmutableMap.of())`.
    fn empty_map(&self) -> Self::Output;

    /// `emptyList()` — `createList(Stream.empty())`.
    fn empty_list(&self) -> Self::Output;

    /// `convertTo(DynamicOps<U>, T input)`.
    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &Self::Output) -> U::Output;

    /// `getNumberValue(T input)`.
    ///
    /// STUB(mc.nbt): Java returns a `Number` (capable of `BigInteger`/
    /// `BigDecimal`); the STUB(mc.nbt) surface narrows it to `f64`, so values
    /// larger than f64/i64 truncate or error where Java would not.
    fn get_number_value(&self, input: &Self::Output) -> DataResult<f64>;

    /// `getNumberValue(T input, Number defaultValue)`.
    fn get_number_value_or(&self, input: &Self::Output, default_value: f64) -> f64 {
        self.get_number_value(input)
            .result()
            .copied()
            .unwrap_or(default_value)
    }

    /// `createNumeric(Number)`.
    fn create_numeric(&self, value: f64) -> Self::Output;

    /// `createByte(byte)`.
    fn create_byte(&self, value: i8) -> Self::Output {
        self.create_numeric(value as f64)
    }

    /// `createShort(short)`.
    fn create_short(&self, value: i16) -> Self::Output {
        self.create_numeric(value as f64)
    }

    /// `createInt(int)`.
    fn create_int(&self, value: i32) -> Self::Output {
        self.create_numeric(value as f64)
    }

    /// `createLong(long)`.
    fn create_long(&self, value: i64) -> Self::Output {
        self.create_numeric(value as f64)
    }

    /// `createFloat(float)`.
    fn create_float(&self, value: f32) -> Self::Output {
        self.create_numeric(value as f64)
    }

    /// `createDouble(double)`.
    fn create_double(&self, value: f64) -> Self::Output {
        self.create_numeric(value)
    }

    /// `getBooleanValue(T)`.
    fn get_boolean_value(&self, input: &Self::Output) -> DataResult<bool>;

    /// `createBoolean(boolean)`.
    fn create_boolean(&self, value: bool) -> Self::Output;

    /// `getStringValue(T)`.
    fn get_string_value(&self, input: &Self::Output) -> DataResult<String>;

    /// `createString(String)`.
    fn create_string(&self, value: String) -> Self::Output;

    /// `mergeToList(T list, T value)` — only successful if `list` is a
    /// list/array or empty.
    fn merge_to_list(&self, list: &Self::Output, value: Self::Output) -> DataResult<Self::Output>;

    /// `mergeToList(T list, List<T> values)`.
    fn merge_to_list_many(
        &self,
        list: &Self::Output,
        values: Vec<Self::Output>,
    ) -> DataResult<Self::Output> {
        let mut result: DataResult<Self::Output> = DataResult::success(list.clone());
        for value in values {
            result = result.flat_map(|r| self.merge_to_list(&r, value));
        }
        result
    }

    /// `mergeToMap(T map, T key, T value)` — only successful if `map` is a map
    /// or empty.
    fn merge_to_map(
        &self,
        map: &Self::Output,
        key: Self::Output,
        value: Self::Output,
    ) -> DataResult<Self::Output>;

    /// `mergeToMap(T map, MapLike<T> values)`.
    fn merge_to_map_like(
        &self,
        map: &Self::Output,
        values: &dyn MapLike<Self::Output>,
    ) -> DataResult<Self::Output> {
        let mut result: DataResult<Self::Output> = DataResult::success(map.clone());
        for entry in values.entries() {
            result = result.flat_map(|r| self.merge_to_map(&r, entry.first, entry.second));
        }
        result
    }

    /// `mergeToMap(T map, Map<T, T> values)`.
    fn merge_to_map_many(
        &self,
        map: &Self::Output,
        values: Vec<Pair<Self::Output, Self::Output>>,
    ) -> DataResult<Self::Output> {
        let mut result: DataResult<Self::Output> = DataResult::success(map.clone());
        for entry in values {
            result = result.flat_map(|r| self.merge_to_map(&r, entry.first, entry.second));
        }
        result
    }

    /// `mergeToPrimitive(T prefix, T value)` — only successful if `prefix` is
    /// empty.
    fn merge_to_primitive(
        &self,
        prefix: &Self::Output,
        value: Self::Output,
    ) -> DataResult<Self::Output> {
        if *prefix != self.empty() {
            return DataResult::error_with_partial(
                format!(
                    "Do not know how to append a primitive value {} to {}",
                    debug_value(value.clone()),
                    debug_value(prefix.clone())
                ),
                value,
            );
        }
        DataResult::success(value)
    }

    /// `getMapValues(T input)`.
    fn get_map_values(
        &self,
        input: &Self::Output,
    ) -> DataResult<Vec<Pair<Self::Output, Self::Output>>>;

    /// `getMapEntries(T input)` — a consumer that feeds key/value pairs.
    fn get_map_entries(&self, input: &Self::Output) -> DataResult<MapEntryConsumer<Self::Output>>;

    /// `createMap(Stream<Pair<T, T>>)`.
    fn create_map(&self, map: Vec<Pair<Self::Output, Self::Output>>) -> Self::Output;

    /// `getMap(T input)`.
    fn get_map(&self, input: &Self::Output) -> DataResult<Box<dyn MapLike<Self::Output>>>;

    /// `getStream(T input)`.
    fn get_stream(&self, input: &Self::Output) -> DataResult<Vec<Self::Output>>;

    /// `getList(T input)`.
    fn get_list(&self, input: &Self::Output) -> DataResult<ListConsumer<Self::Output>>;

    /// `createList(Stream<T> input)`.
    fn create_list(&self, input: Vec<Self::Output>) -> Self::Output;

    /// `getByteBuffer(T input)` — STUB(mc.nbt): Java returns a `ByteBuffer`;
    /// narrowed to `Vec<u8>`.
    fn get_byte_buffer(&self, input: &Self::Output) -> DataResult<Vec<u8>>;

    /// `createByteList(ByteBuffer input)`.
    fn create_byte_list(&self, input: &[u8]) -> Self::Output;

    /// `getIntStream(T input)`.
    fn get_int_stream(&self, input: &Self::Output) -> DataResult<Vec<i32>>;

    /// `createIntList(IntStream input)`.
    fn create_int_list(&self, input: Vec<i32>) -> Self::Output;

    /// `getLongStream(T input)`.
    fn get_long_stream(&self, input: &Self::Output) -> DataResult<Vec<i64>>;

    /// `createLongList(LongStream input)`.
    fn create_long_list(&self, input: Vec<i64>) -> Self::Output;

    /// `remove(T input, String key)`.
    fn remove(&self, input: Self::Output, key: &str) -> Self::Output;

    /// `compressMaps()`.
    ///
    /// STUB(mc.nbt): the `true` branch (packed list-of-entries form via
    /// `KeyCompressor`) is not ported — `MapDecoder.compressedDecode`,
    /// `MapEncoder.encoder()` and `MapCodecCodec.encode` all fall back to the
    /// non-compressed map form when an ops overrides this to return `true`.
    fn compress_maps(&self) -> bool {
        false
    }

    /// `get(T input, String key)`.
    fn get(&self, input: &Self::Output, key: &str) -> DataResult<Self::Output> {
        self.get_generic(input, &self.create_string(key.to_string()))
    }

    /// `getGeneric(T input, T key)`.
    fn get_generic(&self, input: &Self::Output, key: &Self::Output) -> DataResult<Self::Output> {
        let map = self.get_map(input);
        map.flat_map(|map| match map.get(key) {
            Some(v) => DataResult::success(v),
            None => DataResult::error(format!(
                "No element {} in the map {}",
                debug_value(key.clone()),
                debug_value(input.clone())
            )),
        })
    }

    /// `set(T input, String key, T value)` — eats the error if input is not a
    /// map.
    fn set(&self, input: &Self::Output, key: &str, value: Self::Output) -> Self::Output {
        self.merge_to_map(input, self.create_string(key.to_string()), value)
            .result()
            .cloned()
            .unwrap_or_else(|| input.clone())
    }

    /// `update(T input, String key, Function<T, T>)` — eats the error if input
    /// is not a map.
    fn update(
        &self,
        input: &Self::Output,
        key: &str,
        function: impl Fn(Self::Output) -> Self::Output,
    ) -> Self::Output {
        match self.get(input, key).result() {
            Some(value) => self.set(input, key, function(value.clone())),
            None => input.clone(),
        }
    }

    /// `updateGeneric(T input, T key, Function<T, T>)` — eats the error if
    /// input is not a map.
    fn update_generic(
        &self,
        input: &Self::Output,
        key: &Self::Output,
        function: impl Fn(Self::Output) -> Self::Output,
    ) -> Self::Output {
        match self.get_generic(input, key).result() {
            Some(value) => self
                .merge_to_map(input, key.clone(), function(value.clone()))
                .result()
                .cloned()
                .unwrap_or_else(|| input.clone()),
            None => input.clone(),
        }
    }

    /// `mapBuilder()` — an empty `RecordBuilder`.
    fn map_builder(&self) -> Box<dyn RecordBuilder<Output = Self::Output> + '_>
    where
        Self: Sized,
    {
        Box::new(RecordBuilderImpl::new(self))
    }

    /// `listBuilder()`.
    fn list_builder(&self) -> Box<dyn ListBuilder<Output = Self::Output> + '_>
    where
        Self: Sized,
    {
        Box::new(ListBuilderImpl::new(self))
    }

    /// `convertList(DynamicOps<U>, T input)`.
    fn convert_list<U: DynamicOps>(&self, out_ops: &U, input: &Self::Output) -> U::Output {
        let stream = self.get_stream(input).result().cloned().unwrap_or_default();
        out_ops.create_list(
            stream
                .into_iter()
                .map(|e| self.convert_to(out_ops, &e))
                .collect(),
        )
    }

    /// `convertMap(DynamicOps<U>, T input)`.
    fn convert_map<U: DynamicOps>(&self, out_ops: &U, input: &Self::Output) -> U::Output {
        let map = self
            .get_map_values(input)
            .result()
            .cloned()
            .unwrap_or_default();
        out_ops.create_map(
            map.into_iter()
                .map(|e| {
                    Pair::of(
                        self.convert_to(out_ops, &e.first),
                        self.convert_to(out_ops, &e.second),
                    )
                })
                .collect(),
        )
    }
}

/// The Java `ListBuilder.Builder` — accumulates into a `Vec` and merges into a
/// prefix via `mergeToList`.
pub struct ListBuilderImpl<'a, O: DynamicOps> {
    ops: &'a O,
    builder: DataResult<()>,
    values: Vec<O::Output>,
}
impl<'a, O: DynamicOps> std::fmt::Debug for ListBuilderImpl<'a, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListBuilderImpl")
    }
}

impl<'a, O: DynamicOps> ListBuilderImpl<'a, O> {
    /// `new Builder<>(ops)`.
    pub fn new(ops: &'a O) -> Self {
        ListBuilderImpl {
            ops,
            builder: DataResult::success_with_lifecycle((), Lifecycle::stable()),
            values: Vec::new(),
        }
    }
}

impl<'a, O: DynamicOps> ListBuilder for ListBuilderImpl<'a, O> {
    type Output = O::Output;

    /// `Builder.add(T value)`.
    fn add(&mut self, value: O::Output) {
        self.values.push(value);
    }

    /// `Builder.add(DataResult<T> value)`.
    fn add_result(&mut self, value: DataResult<O::Output>) {
        let opt = value.clone().result_or_partial_silent();
        if let Some(v) = opt {
            self.values.push(v);
        }
        let builder = self.builder.clone();
        self.builder = builder.apply2_stable(|_, _| (), value.map(|_| ()));
    }

    /// `Builder.withErrorsFrom(DataResult<?>)`.
    fn with_errors_from(&mut self, result: &DataResult<()>) {
        let r = result.clone();
        self.builder = self.builder.clone().flat_map(|v| r.map(|_| v));
    }

    /// `Builder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        let builder = self.builder.clone();
        self.builder = builder.map_error(on_error);
    }

    /// `Builder.build(T prefix)` — `builder.flatMap(b -> ops.mergeToList(prefix, b.build()))`.
    /// Java resets the accumulated state after each build.
    fn build(&mut self, prefix: O::Output) -> DataResult<O::Output> {
        let values = self.values.clone();
        let builder = self.builder.clone();
        let result = builder.flat_map(|_| self.ops.merge_to_list_many(&prefix, values));
        self.builder = DataResult::success_with_lifecycle((), Lifecycle::stable());
        self.values.clear();
        result
    }
}

/// Debug formatting used in error messages (mirrors Java's string
/// concatenation of `T` values).
fn debug_value<T: Debug>(value: T) -> String {
    format!("{:?}", value)
}
