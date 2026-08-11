//! Port of `com.mojang.serialization.MapCodec`.
//!
//! `MapCodec<A>` is `MapEncoder<A>` + `MapDecoder<A>` + `keys`. The Rust port
//! pins the ops (`MapCodec<A, Ops>`), keeps the trait minimal (only the
//! non-generic `decode`/`encode`/`codec`/`compressedDecode`) so it is
//! dyn-compatible, and exposes Java's default combinators as free functions
//! (`xmap`, `flatXmap`, `orElse`, `withLifecycle`, `unit`, `recursive`,
//! `assumeMapUnsafe`, `fieldOf`). Small adapter structs re-expose a
//! `Arc<dyn MapCodec>` as its `MapEncoder`/`MapDecoder` halves (Java's
//! sub-interface dispatch; dyn upcasting is not stable in Rust).

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::functions::DecoderFn;
use crate::lifecycle::Lifecycle;
use crate::map_decoder::MapDecoder;
use crate::map_encoder::MapEncoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `MapCodec.recursive` wrapper — `Function<Codec<A>, MapCodec<A>>`.
/// The `Ops: DynamicOps` bound is needed on the RHS (`Codec<A, Ops>`) but is
/// not enforced at alias usage sites, so the `type_alias_bounds` lint is
/// allowed here.
#[allow(type_alias_bounds)]
type RecursiveMapFn<A, Ops: DynamicOps + 'static> =
    Arc<dyn Fn(Arc<dyn Codec<A, Ops>>) -> Arc<dyn MapCodec<A, Ops>> + Send + Sync>;

/// `com.mojang.serialization.MapCodec<A>`.
///
/// `Send + Sync` mirrors Paper: the game's codecs are `static final` values
/// shared across netty threads (and the packet `StreamCodec` a status response
/// is built from is itself `Send + Sync`), so a codec must be usable from any
/// connection thread.
pub trait MapCodec<A, Ops: DynamicOps + 'static>: Debug + Keyable<Ops> + Send + Sync {
    /// `MapCodec.decode(DynamicOps<T> ops, MapLike<T> input)`.
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A>;

    /// `MapCodec.encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix)`.
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>);

    /// `MapCodec.compressedDecode(DynamicOps<T> ops, T input)` — same default
    /// as `MapDecoder.compressedDecode`: with `compressMaps()` the input must
    /// be a list read through a `KeyCompressor`-backed `MapLike`, otherwise the
    /// non-compressed `getMap` path.
    fn compressed_decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<A> {
        if ops.compress_maps() {
            match crate::dynamic_ops::compressed_map_like(ops, self.keys(ops), input) {
                Some(map) => self.decode(ops, &map),
                None => DataResult::error("Input is not a list".to_string()),
            }
        } else {
            let map = ops.get_map(input).set_lifecycle(Lifecycle::stable());
            map.flat_map(|m| self.decode(ops, m.as_ref()))
        }
    }
}

/// `MapCodec.codec()` — `new MapCodecCodec<>(this)` as a free function (the
/// method cannot be invoked on `Arc<dyn MapCodec>` since it consumes `Arc<Self>`).
pub fn codec_of<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn MapCodec<A, Ops>>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapCodecCodec { codec })
}

// ---------------------------------------------------------------------------
// Adapters exposing a MapCodec as its halves
// ---------------------------------------------------------------------------

/// The `MapEncoder` half of a `MapCodec`.
pub struct MapCodecEncoderHalf<A, Ops: DynamicOps + 'static>(pub Arc<dyn MapCodec<A, Ops>>);
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapCodecEncoderHalf<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapCodecEncoderHalf")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for MapCodecEncoderHalf<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.0.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapEncoder<A, Ops> for MapCodecEncoderHalf<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.0.encode(input, ops, prefix)
    }
}

/// The `MapDecoder` half of a `MapCodec`.
pub struct MapCodecDecoderHalf<A, Ops: DynamicOps + 'static>(pub Arc<dyn MapCodec<A, Ops>>);
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapCodecDecoderHalf<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapCodecDecoderHalf")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for MapCodecDecoderHalf<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.0.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapDecoder<A, Ops> for MapCodecDecoderHalf<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.0.decode(ops, input)
    }
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// `MapCodec.of(MapEncoder, MapDecoder, Supplier<String>)`.
pub fn of<A, Ops: DynamicOps + 'static>(
    encoder: Arc<dyn MapEncoder<A, Ops>>,
    decoder: Arc<dyn MapDecoder<A, Ops>>,
    name: String,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new(OfMapCodec {
        encoder,
        decoder,
        name: Arc::new(move || name.clone()),
    })
}

/// `MapCodec.unit(A)`.
pub fn unit<A: Clone + Send + Sync + 'static, Ops: DynamicOps + 'static>(
    default_value: A,
) -> Arc<dyn MapCodec<A, Ops>> {
    unit_with(Arc::new(move || default_value.clone()))
}

/// `MapCodec.unit(Supplier<A>)`.
pub fn unit_with<A: 'static, Ops: DynamicOps + 'static>(
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn MapCodec<A, Ops>> {
    Arc::new(UnitMapCodec {
        value,
        _ops: std::marker::PhantomData,
    })
}

/// `MapCodec.unitCodec(A)`.
pub fn unit_codec<A: Clone + Send + Sync + 'static, Ops: DynamicOps + 'static>(
    value: A,
) -> Arc<dyn Codec<A, Ops>> {
    unit_codec_with(Arc::new(move || value.clone()))
}

/// `MapCodec.unitCodec(Supplier<A>)`.
pub fn unit_codec_with<A: 'static, Ops: DynamicOps + 'static>(
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>> {
    Arc::new(UnitCodec {
        value,
        _ops: std::marker::PhantomData,
    })
}

/// `MapCodec.fieldOf(String)` — `FieldEncoder` + `FieldDecoder` over the given
/// `Encoder`/`Decoder` halves.
pub fn field_of<A, Ops: DynamicOps + 'static>(
    name: String,
    encoder: Arc<dyn crate::Encoder<A, Ops>>,
    decoder: Arc<dyn crate::Decoder<A, Ops>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    let name_for_format = format!("Field[{}: {:?}]", name, decoder);
    of(
        crate::map_encoder::field_encoder(name.clone(), encoder),
        crate::map_decoder::field_decoder(name, decoder),
        name_for_format,
    )
}

/// `MapCodec.recursive(String, Function<Codec<A>, MapCodec<A>>)` — Java's
/// `RecursiveMapCodec`.
pub fn recursive<A, Ops: DynamicOps + 'static>(
    name: String,
    wrapped: RecursiveMapFn<A, Ops>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new_cyclic(|weak| RecursiveMapCodec {
        name,
        wrapped,
        cell: std::sync::OnceLock::new(),
        weak: weak.clone(),
    })
}

/// `MapCodec.assumeMapUnsafe(Codec<A>)`.
pub fn assume_map_unsafe<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new(AssumeMapCodec { codec })
}

// ---------------------------------------------------------------------------
// Combinators
// ---------------------------------------------------------------------------

/// `MapCodec.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    lifecycle: Lifecycle,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleMapCodec { lifecycle, inner })
}

/// `MapCodec.stable()`.
pub fn stable<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    with_lifecycle(inner, Lifecycle::stable())
}

/// `MapCodec.deprecated(int)`.
pub fn deprecated<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    since: i32,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    with_lifecycle(inner, Lifecycle::deprecated(since))
}

/// `MapCodec.xmap(Function, Function)`.
pub fn xmap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    to: Arc<dyn Fn(&A) -> B + Send + Sync>,
    from: Arc<dyn Fn(&B) -> A + Send + Sync>,
) -> Arc<dyn MapCodec<B, Ops>>
where
    A: 'static,
    B: 'static,
{
    of(
        crate::map_encoder::comap(Arc::new(MapCodecEncoderHalf(inner.clone())), from),
        crate::map_decoder::map(Arc::new(MapCodecDecoderHalf(inner.clone())), to),
        format!("{:?}[xmapped]", inner),
    )
}

/// `MapCodec.flatXmap(Function, Function)`.
pub fn flat_xmap<A, B, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    to: DecoderFn<A, B>,
    from: DecoderFn<B, A>,
) -> Arc<dyn MapCodec<B, Ops>>
where
    A: 'static + Clone + Send + Sync,
    B: 'static,
{
    of(
        crate::map_encoder::flat_comap(Arc::new(MapCodecEncoderHalf(inner.clone())), from),
        crate::map_decoder::flat_map(Arc::new(MapCodecDecoderHalf(inner.clone())), to),
        format!("{:?}[flatXmapped]", inner),
    )
}

/// `MapCodec.validate(Function)`.
pub fn validate<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    checker: DecoderFn<A, A>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    flat_xmap(inner, checker.clone(), checker)
}

/// `Codec.mapPair(MapCodec<F>, MapCodec<S>)` — `PairMapCodec`, the pair
/// combinator the `StateDefinition` property-codec fold uses (Java
/// `StateDefinition.appendPropertyCodec`: `Codec.mapPair(codec,
/// property.valueCodec().fieldOf(name))`).
///
/// Java decodes the first half, then flat-maps the second; encodes the first
/// half into the builder that the second half produced. The Rust port's
/// `encode` mutates a single `RecordBuilder` directly (Java's chained builders
/// merge into the same map), so the halves encode in sequence into the same
/// prefix — the map key order is not semantically significant.
pub fn map_pair<F, S, Ops: DynamicOps + 'static>(
    first: Arc<dyn MapCodec<F, Ops>>,
    second: Arc<dyn MapCodec<S, Ops>>,
) -> Arc<dyn MapCodec<crate::pair::Pair<F, S>, Ops>>
where
    F: 'static,
    S: 'static,
{
    Arc::new(PairMapCodec { first, second })
}

/// `com.mojang.serialization.codecs.PairMapCodec`.
pub struct PairMapCodec<F, S, Ops: DynamicOps + 'static> {
    first: Arc<dyn MapCodec<F, Ops>>,
    second: Arc<dyn MapCodec<S, Ops>>,
}

impl<F, S, Ops: DynamicOps + 'static> std::fmt::Debug for PairMapCodec<F, S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PairMapCodec")
    }
}

impl<F, S, Ops: DynamicOps + 'static> Keyable<Ops> for PairMapCodec<F, S, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.first.keys(ops);
        keys.extend(self.second.keys(ops));
        keys
    }
}

impl<F, S, Ops: DynamicOps + 'static> MapCodec<crate::pair::Pair<F, S>, Ops>
    for PairMapCodec<F, S, Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<crate::pair::Pair<F, S>> {
        self.first.decode(ops, input).flat_map(|f| {
            self.second
                .decode(ops, input)
                .flat_map(|s| DataResult::success(crate::pair::Pair::of(f, s)))
        })
    }

    fn encode(
        &self,
        input: &crate::pair::Pair<F, S>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.second.encode(&input.second, ops, prefix);
        self.first.encode(&input.first, ops, prefix);
    }
}

/// `MapCodec.mapResult(ResultFunction)`.
pub fn map_result<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    function: Arc<dyn MapResultFunction<A, Ops>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapResultMapCodec { function, inner })
}

/// `MapCodec.orElse(Consumer<String>, A)`.
pub fn or_else<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    on_error: Arc<dyn Fn(&str) + Send + Sync>,
    value: A,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    let v = value.clone();
    // `DataFixUtils.consumerToFunction`: invoke the callback and return the
    // message unchanged.
    or_else_get_map_error(
        inner,
        Arc::new(move |e: String| {
            on_error(&e);
            e
        }),
        Arc::new(move || v.clone()),
    )
}

/// `MapCodec.orElseGet(Consumer<String>, Supplier<A>)`.
pub fn or_else_get<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    on_error: Arc<dyn Fn(&str) + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    or_else_get_map_error(
        inner,
        Arc::new(move |e: String| {
            on_error(&e);
            e
        }),
        value,
    )
}

/// `MapCodec.orElseGet(UnaryOperator<String>, Supplier<A>)`.
pub fn or_else_get_map_error<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    on_error: Arc<dyn Fn(String) -> String + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    map_result(inner, Arc::new(OrElseResultFunction { on_error, value }))
}

/// `MapCodec.orElse(A)`.
pub fn or_else_value<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: A,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    let v = value.clone();
    or_else_get_value(inner, Arc::new(move || v.clone()))
}

/// `MapCodec.orElseGet(Supplier<A>)`.
pub fn or_else_get_value<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    map_result(inner, Arc::new(OrElseValueResultFunction { value }))
}

/// `MapCodec.setPartial(Supplier<A>)`.
pub fn set_partial<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    map_result(inner, Arc::new(SetPartialResultFunction { value }))
}

/// `MapCodec.forGetter(Function)` — `RecordCodecBuilder.of(getter, this)`.
pub fn for_getter<O, A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    getter: Arc<dyn Fn(&O) -> A + Send + Sync>,
) -> crate::record_builder::RecordCodecBuilder<O, Ops, A>
where
    A: 'static,
    O: 'static,
{
    crate::record_builder::RecordCodecBuilder::of(getter, inner)
}

// ---------------------------------------------------------------------------
// ResultFunction
// ---------------------------------------------------------------------------

/// `MapCodec.ResultFunction` — `apply`/`coApply`.
///
/// `Send + Sync` mirrors Paper: result functions are stored inside codecs that
/// are shared across netty threads.
pub trait MapResultFunction<A, Ops: DynamicOps + 'static>: Debug + Send + Sync {
    fn apply(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>, a: DataResult<A>)
    -> DataResult<A>;

    fn co_apply(&self, ops: &Ops, input: &A, t: &mut dyn RecordBuilder<Output = Ops::Output>);
}

/// `MapCodec.mapResult(ResultFunction)` result.
pub struct MapResultMapCodec<A, Ops: DynamicOps + 'static> {
    function: Arc<dyn MapResultFunction<A, Ops>>,
    inner: Arc<dyn MapCodec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapResultMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapResultMapCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for MapResultMapCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<A, Ops> for MapResultMapCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.function
            .apply(ops, input, self.inner.decode(ops, input))
    }

    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        // Java: `function.coApply(ops, input, MapCodec.this.encode(input, ops, prefix))`.
        self.inner.encode(input, ops, prefix);
        self.function.co_apply(ops, input, prefix);
    }
}

/// `MapCodec.orElseGet(UnaryOperator<String>, Supplier<A>)` result function.
pub struct OrElseResultFunction<A> {
    on_error: Arc<dyn Fn(String) -> String + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
}
impl<A> std::fmt::Debug for OrElseResultFunction<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrElseResultFunction")
    }
}

impl<A, Ops: DynamicOps + 'static> MapResultFunction<A, Ops> for OrElseResultFunction<A>
where
    A: Clone,
{
    fn apply(
        &self,
        _ops: &Ops,
        _input: &dyn MapLike<Ops::Output>,
        a: DataResult<A>,
    ) -> DataResult<A> {
        let mapped = a.map_error(move |e| (self.on_error)(e));
        match mapped.result() {
            Some(v) => DataResult::success(v.clone()),
            None => DataResult::success((self.value)()),
        }
    }

    fn co_apply(&self, _ops: &Ops, _input: &A, t: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let on_error = self.on_error.clone();
        t.map_error(Box::new(move |e| on_error(e)));
    }
}

/// `MapCodec.orElseGet(Supplier<A>)` result function.
pub struct OrElseValueResultFunction<A> {
    value: Arc<dyn Fn() -> A + Send + Sync>,
}
impl<A> std::fmt::Debug for OrElseValueResultFunction<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrElseValueResultFunction")
    }
}

impl<A, Ops: DynamicOps + 'static> MapResultFunction<A, Ops> for OrElseValueResultFunction<A>
where
    A: Clone,
{
    fn apply(
        &self,
        _ops: &Ops,
        _input: &dyn MapLike<Ops::Output>,
        a: DataResult<A>,
    ) -> DataResult<A> {
        match a.result() {
            Some(v) => DataResult::success(v.clone()),
            None => DataResult::success((self.value)()),
        }
    }

    fn co_apply(&self, _ops: &Ops, _input: &A, _t: &mut dyn RecordBuilder<Output = Ops::Output>) {}
}

/// `MapCodec.setPartial(Supplier<A>)` result function.
pub struct SetPartialResultFunction<A> {
    value: Arc<dyn Fn() -> A + Send + Sync>,
}
impl<A> std::fmt::Debug for SetPartialResultFunction<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SetPartialResultFunction")
    }
}

impl<A, Ops: DynamicOps + 'static> MapResultFunction<A, Ops> for SetPartialResultFunction<A> {
    fn apply(
        &self,
        _ops: &Ops,
        _input: &dyn MapLike<Ops::Output>,
        a: DataResult<A>,
    ) -> DataResult<A> {
        a.set_partial((self.value)())
    }

    fn co_apply(&self, _ops: &Ops, _input: &A, _t: &mut dyn RecordBuilder<Output = Ops::Output>) {}
}

// ---------------------------------------------------------------------------
// Concrete codecs
// ---------------------------------------------------------------------------

/// `MapCodec.of(...)` result.
pub struct OfMapCodec<A, Ops: DynamicOps + 'static> {
    encoder: Arc<dyn MapEncoder<A, Ops>>,
    decoder: Arc<dyn MapDecoder<A, Ops>>,
    name: Arc<dyn Fn() -> String + Send + Sync>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for OfMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OfMapCodec[{}]", (self.name)())
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for OfMapCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.encoder.keys(ops);
        keys.extend(self.decoder.keys(ops));
        keys
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<A, Ops> for OfMapCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.decoder.decode(ops, input)
    }

    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.encoder.encode(input, ops, prefix)
    }
}

/// `MapCodecCodec` — `MapCodec.codec()`.
pub struct MapCodecCodec<A, Ops: DynamicOps + 'static> {
    codec: Arc<dyn MapCodec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapCodecCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapCodecCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for MapCodecCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        // Java `MapCodecCodec.decode`: `codec.compressedDecode(ops, input).map(r
        // -> Pair.of(r, input))`.
        self.codec
            .compressed_decode(ops, input)
            .map_owned(|r| (r, input.clone()))
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for MapCodecCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // Java `MapCodecCodec.encode`:
        // `codec.encode(input, ops, codec.compressedBuilder(ops)).build(prefix)`.
        let mut builder = crate::map_encoder::compressed_builder(&*self.codec, ops);
        self.codec.encode(input, ops, &mut *builder);
        builder.build(Some(prefix.clone()))
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for MapCodecCodec<A, Ops> {}

/// `MapCodec.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleMapCodec<A, Ops: DynamicOps + 'static> {
    lifecycle: Lifecycle,
    inner: Arc<dyn MapCodec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleMapCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for WithLifecycleMapCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<A, Ops> for WithLifecycleMapCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.inner.decode(ops, input).set_lifecycle(self.lifecycle)
    }

    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.inner.encode(input, ops, prefix);
        prefix.set_lifecycle(self.lifecycle);
    }
}

/// `MapCodec.unit(Supplier<A>)` result.
pub struct UnitMapCodec<A, Ops: DynamicOps + 'static> {
    value: Arc<dyn Fn() -> A + Send + Sync>,
    _ops: std::marker::PhantomData<Ops>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for UnitMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnitMapCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for UnitMapCodec<A, Ops> {
    fn keys(&self, _ops: &Ops) -> Vec<Ops::Output> {
        Vec::new()
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<A, Ops> for UnitMapCodec<A, Ops> {
    fn decode(&self, _ops: &Ops, _input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        DataResult::success((self.value)())
    }

    fn encode(
        &self,
        _input: &A,
        _ops: &Ops,
        _prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
    }
}

/// `MapCodec.unitCodec(Supplier<A>)` result.
pub struct UnitCodec<A, Ops: DynamicOps + 'static> {
    value: Arc<dyn Fn() -> A + Send + Sync>,
    _ops: std::marker::PhantomData<Ops>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for UnitCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnitCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for UnitCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        // Check required mostly for parsing of optional fields in data fixers:
        // `ops.compressMaps() ? ops.getList(input) : ops.getMap(input)`.
        let check: DataResult<()> = if ops.compress_maps() {
            ops.get_list(input).map(|_| ())
        } else {
            ops.get_map(input).map(|_| ())
        };
        check.map(|_| ((self.value)(), input.clone()))
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for UnitCodec<A, Ops> {
    fn encode(&self, _input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // Enforces type, but also updates empty() to emptyMap()
        ops.merge_to_map_like(prefix, &crate::dynamic_ops::EmptyMapLike)
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for UnitCodec<A, Ops> {}

/// `MapCodec.recursive(...)` result — Java's `RecursiveMapCodec`.
pub struct RecursiveMapCodec<A, Ops: DynamicOps + 'static> {
    name: String,
    wrapped: RecursiveMapFn<A, Ops>,
    cell: std::sync::OnceLock<Arc<dyn MapCodec<A, Ops>>>,
    weak: std::sync::Weak<RecursiveMapCodec<A, Ops>>,
}

impl<A, Ops: DynamicOps + 'static> Debug for RecursiveMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecursiveMapCodec[{}]", self.name)
    }
}

impl<A: 'static, Ops: DynamicOps + 'static> RecursiveMapCodec<A, Ops> {
    fn get(&self) -> &Arc<dyn MapCodec<A, Ops>> {
        let parent = self
            .weak
            .upgrade()
            .expect("recursive map codec's parent Arc must outlive the weak reference");
        self.cell
            .get_or_init(|| (self.wrapped)(Arc::new(RecursiveMapCodecCodec { parent })))
    }
}

/// The `Codec<A>` view handed to the recursive wrapper function.
pub struct RecursiveMapCodecCodec<A: 'static, Ops: DynamicOps + 'static> {
    parent: std::sync::Arc<RecursiveMapCodec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for RecursiveMapCodecCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecursiveMapCodecCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for RecursiveMapCodecCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.parent
            .get()
            .compressed_decode(ops, input)
            .map_owned(|r| (r, input.clone()))
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for RecursiveMapCodecCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let inner = self.parent.get();
        let mut builder = crate::map_encoder::compressed_builder(&**inner, ops);
        inner.encode(input, ops, &mut *builder);
        builder.build(Some(prefix.clone()))
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for RecursiveMapCodecCodec<A, Ops> {}

impl<A: 'static, Ops: DynamicOps + 'static> Keyable<Ops> for RecursiveMapCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.get().keys(ops)
    }
}

impl<A: 'static, Ops: DynamicOps + 'static> MapCodec<A, Ops> for RecursiveMapCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        self.get().decode(ops, input)
    }

    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        self.get().encode(input, ops, prefix)
    }
}

/// `MapCodec.assumeMapUnsafe(...)` result.
pub struct AssumeMapCodec<A, Ops: DynamicOps + 'static> {
    codec: Arc<dyn Codec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for AssumeMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AssumeMapCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> Keyable<Ops> for AssumeMapCodec<A, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string("value".to_string())]
    }
}

impl<A, Ops: DynamicOps + 'static> MapCodec<A, Ops> for AssumeMapCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A> {
        if ops.compress_maps() {
            match input.get_string("value") {
                Some(v) => self.codec.parse(ops, &v),
                None => DataResult::error("Missing value"),
            }
        } else {
            let entries = input.entries();
            let map = ops.create_map(entries);
            self.codec.parse(ops, &map)
        }
    }

    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let encoded = self.codec.encode_start(ops, input);
        if ops.compress_maps() {
            prefix.add_string_result("value", encoded);
        } else {
            let encoded_map = encoded.flat_map(|v| ops.get_map(&v));
            match encoded_map.result() {
                Some(map) => {
                    for pair in map.entries() {
                        prefix.add(pair.first, pair.second);
                    }
                }
                None => {
                    prefix.with_errors_from(&encoded_map.map(|_| ()));
                }
            }
        }
    }
}
