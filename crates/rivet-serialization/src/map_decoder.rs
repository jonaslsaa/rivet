//! Port of `com.mojang.serialization.MapDecoder`.
//!
//! `MapDecoder<A>` extends `Keyable`. The trait is kept minimal (only the
//! non-generic `decode` plus the non-generic `compressedDecode` default);
//! Java's default combinators (`flatMap`, `map`, `ap`, `decoder`,
//! `withLifecycle`, `unit`, `error`) are free functions.

use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike};
use crate::functions::DecoderFn;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.MapDecoder<A>`.
pub trait MapDecoder<A, Ops: DynamicOps + 'static>: Debug + Keyable<Ops> {
    /// `MapDecoder.decode(DynamicOps<T> ops, MapLike<T> input)`.
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A>;

    /// `MapDecoder.compressedDecode(DynamicOps<T> ops, T input)` — default:
    /// non-compressed (`ops.getMap(input)`), using the lifecycle of `decode`.
    ///
    /// STUB(mc.nbt): Java's `compressMaps() == true` branch (a
    /// `KeyCompressor`-backed `MapLike` over a packed list of entries) is not
    /// ported. Any ops that overrides `compressMaps()` to return `true` will
    /// decode through the non-compressed `getMap` path instead of the packed
    /// list form.
    fn compressed_decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<A> {
        let map = ops
            .get_map(input)
            .set_lifecycle(crate::lifecycle::Lifecycle::stable());
        map.flat_map(|m| self.decode(ops, m.as_ref()))
    }
}

/// `FieldDecoder` — `Decoder.fieldOf(name)` / `codecs.FieldDecoder`.
pub fn field_decoder<A, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn crate::Decoder<A, Ops>>,
) -> Arc<dyn MapDecoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(FieldDecoder {
        name,
        element_codec,
    })
}

/// `MapDecoder.decoder()`.
pub fn decoder<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapDecoder<A, Ops>>,
) -> Arc<dyn crate::Decoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapDecoderAsDecoder { inner })
}

/// `MapDecoder.flatMap(Function)`.
pub fn flat_map<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapDecoder<A, Ops>>,
    function: DecoderFn<A, B>,
) -> Arc<dyn MapDecoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(FlatMappedMapDecoder { function, inner })
}

/// `MapDecoder.map(Function)`.
pub fn map<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapDecoder<A, Ops>>,
    function: Arc<dyn Fn(&A) -> B>,
) -> Arc<dyn MapDecoder<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    Arc::new(MappedMapDecoder { function, inner })
}

/// `MapDecoder.ap(MapDecoder<Function<A, E>>)`.
pub fn ap<A, E, Ops: DynamicOps + 'static>(
    first: Arc<dyn MapDecoder<A, Ops>>,
    decoder: Arc<dyn MapDecoder<Arc<dyn Fn(A) -> E>, Ops>>,
) -> Arc<dyn MapDecoder<E, Ops>>
where
    A: 'static,
    E: 'static,
{
    Arc::new(AppliedMapDecoder { first, decoder })
}

/// `MapDecoder.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapDecoder<A, Ops>>,
    lifecycle: crate::lifecycle::Lifecycle,
) -> Arc<dyn MapDecoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleMapDecoder { lifecycle, inner })
}

/// `MapDecoder.unit(A)`.
pub fn unit<A: Clone + 'static, Ops: DynamicOps + 'static>(
    instance: A,
) -> Arc<dyn MapDecoder<A, Ops>> {
    unit_with(Arc::new(move || instance.clone()))
}

/// `MapDecoder.unit(Supplier<A>)`.
pub fn unit_with<A: 'static, Ops: DynamicOps + 'static>(
    instance: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapDecoder<A, Ops>> {
    Arc::new(MapUnitDecoder {
        instance,
        _ops: std::marker::PhantomData,
    })
}

/// `MapDecoder.error(String)`.
pub fn error<A, Ops: DynamicOps + 'static>(error_message: String) -> Arc<dyn MapDecoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapErrorDecoder {
        error: error_message,
    })
}

/// `codecs.FieldDecoder`.
pub struct FieldDecoder<A, Ops: DynamicOps + 'static> {
    name: String,
    element_codec: Arc<dyn crate::Decoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for FieldDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FieldDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for FieldDecoder<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string(self.name.clone())]
    }
}

impl<A, Ops: DynamicOps + 'static> MapDecoder<A, Ops> for FieldDecoder<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        match input.get_string(&self.name) {
            Some(value) => self.element_codec.parse(ops, &value),
            // Java: `"No key " + name + " in " + input` where `input` is the
            // `MapLike` toString — `MapLike[<entries>]`.
            None => DataResult::error(format!(
                "No key {} in MapLike[{:?}]",
                self.name,
                input.entries()
            )),
        }
    }
}

/// `MapDecoder.decoder()` result.
pub struct MapDecoderAsDecoder<A, Ops: DynamicOps + 'static> {
    inner: Arc<dyn MapDecoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapDecoderAsDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapDecoderAsDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for MapDecoderAsDecoder<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        // Java `MapDecoder.decoder()`: `compressedDecode(ops, input).map(r ->
        // Pair.of(r, input))`.
        self.inner
            .compressed_decode(ops, input)
            .map_owned(|r| (r, input.clone()))
    }
}

/// `MapDecoder.flatMap(Function)` result.
pub struct FlatMappedMapDecoder<A, B, Ops: DynamicOps + 'static> {
    function: DecoderFn<A, B>,
    inner: Arc<dyn MapDecoder<A, Ops>>,
}
impl<A, B, Ops: DynamicOps + 'static> std::fmt::Debug for FlatMappedMapDecoder<A, B, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FlatMappedMapDecoder")
    }
}

impl<A, B, Ops: DynamicOps + 'static> Keyable<Ops> for FlatMappedMapDecoder<A, B, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, B, Ops: DynamicOps + 'static> MapDecoder<B, Ops> for FlatMappedMapDecoder<A, B, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<B> {
        self.inner
            .decode(ops, input)
            .flat_map(|b| (self.function)(&b))
    }
}

/// `MapDecoder.map(Function)` result.
pub struct MappedMapDecoder<A, B, Ops: DynamicOps + 'static> {
    function: Arc<dyn Fn(&A) -> B>,
    inner: Arc<dyn MapDecoder<A, Ops>>,
}
impl<A, B, Ops: DynamicOps + 'static> std::fmt::Debug for MappedMapDecoder<A, B, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MappedMapDecoder")
    }
}

impl<A, B, Ops: DynamicOps + 'static> Keyable<Ops> for MappedMapDecoder<A, B, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, B, Ops: DynamicOps + 'static> MapDecoder<B, Ops> for MappedMapDecoder<A, B, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<B> {
        self.inner.decode(ops, input).map(|v| (self.function)(v))
    }
}

/// `MapDecoder.ap(MapDecoder<Function<A, E>>)` result.
pub struct AppliedMapDecoder<A, E, Ops: DynamicOps + 'static> {
    first: Arc<dyn MapDecoder<A, Ops>>,
    decoder: Arc<dyn MapDecoder<Arc<dyn Fn(A) -> E>, Ops>>,
}
impl<A, E, Ops: DynamicOps + 'static> std::fmt::Debug for AppliedMapDecoder<A, E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppliedMapDecoder")
    }
}

impl<A, E, Ops: DynamicOps + 'static> Keyable<Ops> for AppliedMapDecoder<A, E, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.first.keys(ops);
        keys.extend(self.decoder.keys(ops));
        keys
    }
}

impl<A, E, Ops: DynamicOps + 'static> MapDecoder<E, Ops> for AppliedMapDecoder<A, E, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<E> {
        self.first.decode(ops, input).flat_map(move |f| {
            let f = f;
            self.decoder.decode(ops, input).map(move |e| e(f))
        })
    }
}

/// `MapDecoder.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleMapDecoder<A, Ops: DynamicOps + 'static> {
    lifecycle: crate::lifecycle::Lifecycle,
    inner: Arc<dyn MapDecoder<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleMapDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleMapDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for WithLifecycleMapDecoder<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapDecoder<A, Ops> for WithLifecycleMapDecoder<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.inner.decode(ops, input).set_lifecycle(self.lifecycle)
    }
}

/// `MapDecoder.unit(Supplier)` result.
pub struct MapUnitDecoder<A, Ops: DynamicOps + 'static> {
    instance: Arc<dyn Fn() -> A>,
    _ops: std::marker::PhantomData<Ops>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapUnitDecoder<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapUnitDecoder")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for MapUnitDecoder<A, Ops> {
    fn keys(&self, _ops: &Ops) -> Vec<Ops::Output> {
        Vec::new()
    }
}

impl<A, Ops: DynamicOps + 'static> MapDecoder<A, Ops> for MapUnitDecoder<A, Ops> {
    fn decode(&self, _ops: &Ops, _input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        DataResult::success((self.instance)())
    }
}

/// `MapDecoder.error(String)` result.
pub struct MapErrorDecoder {
    error: String,
}
impl std::fmt::Debug for MapErrorDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapErrorDecoder")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for MapErrorDecoder {
    fn keys(&self, _ops: &Ops) -> Vec<Ops::Output> {
        Vec::new()
    }
}

impl<A, Ops: DynamicOps + 'static> MapDecoder<A, Ops> for MapErrorDecoder {
    fn decode(&self, _ops: &Ops, _input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        DataResult::error(self.error.clone())
    }
}
