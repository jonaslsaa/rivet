//! Port of `com.mojang.serialization.MapEncoder`.
//!
//! `MapEncoder<A>` extends `Keyable`. The trait is kept minimal (only the
//! non-generic `encode`); Java's default combinators (`comap`, `flatComap`,
//! `encoder`, `withLifecycle`, `empty`) are free functions.

use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, KeyCompressor, Keyable, RecordBuilder};
use crate::functions::DecoderFn;
use crate::lifecycle::Lifecycle;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.MapEncoder<A>`.
pub trait MapEncoder<A, Ops: DynamicOps + 'static>: Debug + Keyable<Ops> {
    /// `MapEncoder.encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix)`.
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>);
}

/// `MapEncoder.comap(Function)`.
pub fn comap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapEncoder<A, Ops>>,
    function: Arc<dyn Fn(&B) -> A>,
) -> Arc<dyn MapEncoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(ComappedMapEncoder { function, inner })
}

/// `MapEncoder.flatComap(Function)`.
pub fn flat_comap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapEncoder<A, Ops>>,
    function: DecoderFn<B, A>,
) -> Arc<dyn MapEncoder<B, Ops>>
where
    A: 'static + Clone,
    B: 'static,
{
    Arc::new(FlatComappedMapEncoder { function, inner })
}

/// `MapEncoder.encoder()` — an `Encoder` that builds into a fresh builder and
/// merges into the prefix.
pub fn encoder<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapEncoder<A, Ops>>,
) -> Arc<dyn crate::Encoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapEncoderAsEncoder { inner })
}

/// `MapEncoder.compressedBuilder(DynamicOps<T>)` (free form, usable on any
/// `Keyable` — including `MapCodec`s and their `MapEncoder` halves) —
/// `makeCompressedBuilder(ops, new KeyCompressor<>(ops, keys(ops)))` when
/// `ops.compressMaps()`, else `ops.mapBuilder()`.
pub fn compressed_builder<'a, Ops: DynamicOps + 'static>(
    inner: &dyn Keyable<Ops>,
    ops: &'a Ops,
) -> Box<dyn RecordBuilder<Output = Ops::Output> + 'a> {
    if ops.compress_maps() {
        make_compressed_builder(ops, KeyCompressor::new_with_strings(ops, inner.keys(ops)))
    } else {
        ops.map_builder()
    }
}

/// `MapEncoder.makeCompressedBuilder(DynamicOps<T>, KeyCompressor<T>)` — a
/// `RecordBuilder` over a `Vec<T>` sized to `compressor.size()`, pre-filled
/// with `ops.empty()` null slots. `append` writes the value at the slot named
/// by the compressed key (a duplicate key overwrites its earlier slot, matching
/// Java's `List.set`), and `build` merges the list into the prefix via
/// `ops.mergeToList(prefix, values)`.
pub fn make_compressed_builder<'a, O: DynamicOps>(
    ops: &'a O,
    compressor: KeyCompressor<O::Output>,
) -> Box<dyn RecordBuilder<Output = O::Output> + 'a> {
    Box::new(CompressedRecordBuilder {
        ops,
        compressor,
        builder: DataResult::success_with_lifecycle((), Lifecycle::stable()),
        slots: None,
    })
}

/// `MapEncoder.makeCompressedBuilder`'s `CompressedRecordBuilder` — Java's
/// private class in `makeCompressedBuilder` (an
/// `AbstractUniversalBuilder<T, List<T>>`). `builder` mirrors the accumulated
/// error state, `slots` the `List<T>`; both are reset after each `build`
/// (`AbstractBuilder.build`).
pub struct CompressedRecordBuilder<'a, O: DynamicOps> {
    ops: &'a O,
    compressor: KeyCompressor<O::Output>,
    builder: DataResult<()>,
    slots: Option<Vec<O::Output>>,
}

impl<'a, O: DynamicOps> std::fmt::Debug for CompressedRecordBuilder<'a, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompressedRecordBuilder")
    }
}

impl<'a, O: DynamicOps> CompressedRecordBuilder<'a, O> {
    /// `initBuilder()` — `new ArrayList<>(compressor.size())` pre-filled with
    /// `null` slots (the ops' empty value).
    fn init_builder(&self) -> Vec<O::Output> {
        let mut list = Vec::with_capacity(self.compressor.size());
        for _ in 0..self.compressor.size() {
            list.push(self.ops.empty());
        }
        list
    }
}

impl<'a, O: DynamicOps> RecordBuilder for CompressedRecordBuilder<'a, O> {
    type Output = O::Output;

    /// `AbstractBuilder.build(T prefix)` — `builder.flatMap(b ->
    /// build(b, prefix))`, then reset the accumulated state.
    fn build(&mut self, prefix: Option<O::Output>) -> DataResult<O::Output> {
        let prefix = prefix.unwrap_or_else(|| self.ops.empty());
        let slots = self.slots.take().unwrap_or_else(|| self.init_builder());
        let builder = self.builder.clone();
        let result = builder.flat_map(|_| self.ops.merge_to_list_many(&prefix, slots));
        self.builder = DataResult::success_with_lifecycle((), Lifecycle::stable());
        self.slots = None;
        result
    }

    /// `AbstractUniversalBuilder.add(T key, T value)` —
    /// `builder.map(b -> append(key, value, b))`.
    fn add(&mut self, key: O::Output, value: O::Output) {
        if self.slots.is_none() {
            self.slots = Some(self.init_builder());
        }
        let slots = self.slots.as_mut().expect("slots initialized");
        // `append(T key, T value, List<T> builder)` →
        // `builder.set(compressor.compress(key), value)`. An out-of-range slot
        // index panics, matching Java's `IndexOutOfBoundsException`.
        let idx = self.compressor.compress_key(&key);
        slots[idx] = value;
    }

    /// `AbstractUniversalBuilder.add(T key, DataResult<T> value)` —
    /// `builder.apply2stable((b, v) -> append(key, v, b), value)`. The value's
    /// error state is threaded through the accumulated `()` state.
    fn add_result(&mut self, key: O::Output, value: DataResult<O::Output>) {
        if self.slots.is_none() {
            self.slots = Some(self.init_builder());
        }
        let slots = self.slots.as_mut().expect("slots initialized");
        let idx = self.compressor.compress_key(&key);
        let opt = value.clone().result_or_partial_silent();
        if let Some(v) = opt {
            slots[idx] = v;
        }
        let builder = self.builder.clone();
        self.builder = builder.apply2_stable(|_, _| (), value.map(|_| ()));
    }

    /// `AbstractUniversalBuilder.add(DataResult<T> key, DataResult<T> value)` —
    /// `builder.ap(key.apply2stable((k, v) -> b -> append(k, v, b), value))`.
    /// The resolved key/value pair (from each result-or-partial) is written to
    /// its slot via `compressor.compress`; the combined key+value error state
    /// is threaded through the accumulated `()` builder state via `ap`.
    fn add_result_result(&mut self, key: DataResult<O::Output>, value: DataResult<O::Output>) {
        if self.slots.is_none() {
            self.slots = Some(self.init_builder());
        }
        let slots = self.slots.as_mut().expect("slots initialized");
        let ok = key.clone().result_or_partial_silent();
        let ov = value.clone().result_or_partial_silent();
        if let (Some(k), Some(v)) = (ok, ov) {
            let idx = self.compressor.compress_key(&k);
            slots[idx] = v;
        }
        // Thread the combined key+value error state through `()` values (the
        // builder state is `DataResult<()>`; `O::Output` is not `'static`, so
        // the appended list cannot be carried through the applicative).
        let key_unit = key.map(|_| ());
        let value_unit = value.map(|_| ());
        let combined: DataResult<()> = key_unit.apply2_stable(|_, _| (), value_unit);
        let builder = self.builder.clone();
        let noop: Arc<dyn Fn(&())> = Arc::new(|_| {});
        self.builder = builder.ap(combined.map(|_| noop));
    }

    /// `RecordBuilder.add(String key, T value)` default → `add(createString(key), value)`.
    fn add_string(&mut self, key: &str, value: O::Output) {
        self.add(self.ops.create_string(key.to_string()), value);
    }

    /// `RecordBuilder.add(String key, DataResult<T> value)` default →
    /// `add(createString(key), value)`.
    fn add_string_result(&mut self, key: &str, value: DataResult<O::Output>) {
        self.add_result(self.ops.create_string(key.to_string()), value);
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

/// `MapEncoder.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapEncoder<A, Ops>>,
    lifecycle: crate::lifecycle::Lifecycle,
) -> Arc<dyn MapEncoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleMapEncoder { lifecycle, inner })
}

/// `MapEncoder.empty()`.
pub fn empty<A, Ops: DynamicOps + 'static>() -> Arc<dyn MapEncoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(EmptyMapEncoder)
}

/// `FieldEncoder` — `Encoder.fieldOf(name)` / `codecs.FieldEncoder`.
pub fn field_encoder<A, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn crate::Encoder<A, Ops>>,
) -> Arc<dyn MapEncoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(FieldEncoder {
        name,
        element_codec,
    })
}

/// `MapEncoder.comap(Function)` result.
pub struct ComappedMapEncoder<B, A, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&B) -> A>,
    inner: Arc<dyn MapEncoder<A, Ops>>,
}
impl<B, A, Ops: DynamicOps + 'static> std::fmt::Debug for ComappedMapEncoder<B, A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ComappedMapEncoder")
    }
}

impl<B, A, Ops: DynamicOps + 'static> Keyable<Ops> for ComappedMapEncoder<B, A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<B, A, Ops: DynamicOps + 'static> MapEncoder<B, Ops> for ComappedMapEncoder<B, A, Ops> {
    fn encode(&self, input: &B, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.inner.encode(&(self.function)(input), ops, prefix)
    }
}

/// `MapEncoder.flatComap(Function)` result.
pub struct FlatComappedMapEncoder<B, A, Ops: DynamicOps + 'static> {
    function: DecoderFn<B, A>,
    inner: Arc<dyn MapEncoder<A, Ops>>,
}
impl<B, A, Ops: DynamicOps + 'static> std::fmt::Debug for FlatComappedMapEncoder<B, A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FlatComappedMapEncoder")
    }
}

impl<B, A, Ops: DynamicOps + 'static> Keyable<Ops> for FlatComappedMapEncoder<B, A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<B, A: Clone, Ops: DynamicOps + 'static> MapEncoder<B, Ops>
    for FlatComappedMapEncoder<B, A, Ops>
{
    fn encode(&self, input: &B, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let a_result = (self.function)(input);
        let mapped = a_result.clone().map(|_| ());
        prefix.with_errors_from(&mapped);
        if let Some(a) = a_result.result() {
            self.inner.encode(a, ops, prefix);
        }
    }
}

/// `MapEncoder.encoder()` result.
pub struct MapEncoderAsEncoder<A, Ops: DynamicOps + 'static> {
    inner: Arc<dyn MapEncoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapEncoderAsEncoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapEncoderAsEncoder")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for MapEncoderAsEncoder<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // Java `MapEncoder.encoder()`:
        // `encode(input, ops, compressedBuilder(ops)).build(prefix)`.
        let mut builder = compressed_builder(&*self.inner, ops);
        self.inner.encode(input, ops, &mut *builder);
        builder.build(Some(prefix.clone()))
    }
}

/// `MapEncoder.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleMapEncoder<A, Ops: DynamicOps + 'static> {
    lifecycle: crate::lifecycle::Lifecycle,
    inner: Arc<dyn MapEncoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleMapEncoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleMapEncoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for WithLifecycleMapEncoder<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapEncoder<A, Ops> for WithLifecycleMapEncoder<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.inner.encode(input, ops, prefix);
        prefix.set_lifecycle(self.lifecycle);
    }
}

/// `MapEncoder.empty()` result.
pub struct EmptyMapEncoder;
impl std::fmt::Debug for EmptyMapEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EmptyMapEncoder")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for EmptyMapEncoder {
    fn keys(&self, _ops: &Ops) -> Vec<Ops::Output> {
        Vec::new()
    }
}

impl<A, Ops: DynamicOps + 'static> MapEncoder<A, Ops> for EmptyMapEncoder {
    fn encode(
        &self,
        _input: &A,
        _ops: &Ops,
        _prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
    }
}

/// `codecs.FieldEncoder`.
pub struct FieldEncoder<A, Ops: DynamicOps + 'static> {
    name: String,
    element_codec: Arc<dyn crate::Encoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for FieldEncoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FieldEncoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for FieldEncoder<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string(self.name.clone())]
    }
}

impl<A, Ops: DynamicOps + 'static> MapEncoder<A, Ops> for FieldEncoder<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let encoded = self.element_codec.encode_start(ops, input);
        prefix.add_string_result(&self.name, encoded);
    }
}
