//! Port of `com.mojang.serialization.MapEncoder`.
//!
//! `MapEncoder<A>` extends `Keyable`. The trait is kept minimal (only the
//! non-generic `encode`); Java's default combinators (`comap`, `flatComap`,
//! `encoder`, `withLifecycle`, `empty`) are free functions.

use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, RecordBuilder};
use crate::functions::DecoderFn;
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
///
/// STUB(mc.nbt): Java's `encoder()` uses `compressedBuilder(ops)`, which
/// returns a `KeyCompressor`-backed builder when `ops.compressMaps()`. The
/// compressed builder is not ported; `MapEncoderAsEncoder` always builds via
/// `ops.map_builder()`.
pub fn encoder<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapEncoder<A, Ops>>,
) -> Arc<dyn crate::Encoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapEncoderAsEncoder { inner })
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
        let mut builder = ops.map_builder();
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
