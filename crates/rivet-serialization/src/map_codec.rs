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
use crate::lifecycle::Lifecycle;
use crate::map_decoder::MapDecoder;
use crate::map_encoder::MapEncoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.MapCodec<A>`.
pub trait MapCodec<A, Ops: DynamicOps + 'static>: Debug + Keyable<Ops> {
    /// `MapCodec.decode(DynamicOps<T> ops, MapLike<T> input)`.
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<A>;

    /// `MapCodec.encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix)`.
    fn encode(&self, input: &A, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>);

    /// `MapCodec.compressedDecode(DynamicOps<T> ops, T input)` — default:
    /// non-compressed (`ops.getMap(input)`).
    fn compressed_decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<A> {
        let map = ops.get_map(input).set_lifecycle(Lifecycle::stable());
        map.flat_map(|m| self.decode(ops, m.as_ref()))
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
pub fn unit<A: Clone + 'static, Ops: DynamicOps + 'static>(
    default_value: A,
) -> Arc<dyn MapCodec<A, Ops>> {
    unit_with(Arc::new(move || default_value.clone()))
}

/// `MapCodec.unit(Supplier<A>)`.
pub fn unit_with<A: 'static, Ops: DynamicOps + 'static>(
    value: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapCodec<A, Ops>> {
    Arc::new(UnitMapCodec {
        value,
        _ops: std::marker::PhantomData,
    })
}

/// `MapCodec.unitCodec(A)`.
pub fn unit_codec<A: Clone + 'static, Ops: DynamicOps + 'static>(
    value: A,
) -> Arc<dyn Codec<A, Ops>> {
    unit_codec_with(Arc::new(move || value.clone()))
}

/// `MapCodec.unitCodec(Supplier<A>)`.
pub fn unit_codec_with<A: 'static, Ops: DynamicOps + 'static>(
    value: Arc<dyn Fn() -> A>,
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
    wrapped: Arc<dyn Fn(Arc<dyn Codec<A, Ops>>) -> Arc<dyn MapCodec<A, Ops>>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    Arc::new_cyclic(|weak| RecursiveMapCodec {
        name,
        wrapped,
        cell: std::cell::OnceCell::new(),
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
    to: Arc<dyn Fn(&A) -> B>,
    from: Arc<dyn Fn(&B) -> A>,
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
    to: Arc<dyn Fn(&A) -> DataResult<B>>,
    from: Arc<dyn Fn(&B) -> DataResult<A>>,
) -> Arc<dyn MapCodec<B, Ops>>
where
    A: 'static + Clone,
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
    checker: Arc<dyn Fn(&A) -> DataResult<A>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    flat_xmap(inner, checker.clone(), checker)
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
    on_error: Arc<dyn Fn(&str)>,
    value: A,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    let v = value.clone();
    or_else_get_map_error(
        inner,
        Arc::new(move |_e: String| String::new()),
        Arc::new(move || v.clone()),
    )
}

/// `MapCodec.orElseGet(Consumer<String>, Supplier<A>)`.
pub fn or_else_get<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    on_error: Arc<dyn Fn(&str)>,
    value: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    or_else_get_map_error(inner, Arc::new(move |_e: String| String::new()), value)
}

/// `MapCodec.orElseGet(UnaryOperator<String>, Supplier<A>)`.
pub fn or_else_get_map_error<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    on_error: Arc<dyn Fn(String) -> String>,
    value: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    map_result(inner, Arc::new(OrElseResultFunction { on_error, value }))
}

/// `MapCodec.orElse(A)`.
pub fn or_else_value<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: A,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    let v = value.clone();
    or_else_get_value(inner, Arc::new(move || v.clone()))
}

/// `MapCodec.orElseGet(Supplier<A>)`.
pub fn or_else_get_value<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Clone,
{
    map_result(inner, Arc::new(OrElseValueResultFunction { value }))
}

/// `MapCodec.setPartial(Supplier<A>)`.
pub fn set_partial<A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    value: Arc<dyn Fn() -> A>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    map_result(inner, Arc::new(SetPartialResultFunction { value }))
}

/// `MapCodec.forGetter(Function)` — `RecordCodecBuilder.of(getter, this)`.
pub fn for_getter<O, A, Ops: DynamicOps + 'static>(
    inner: Arc<dyn MapCodec<A, Ops>>,
    getter: Arc<dyn Fn(&O) -> A>,
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
pub trait MapResultFunction<A, Ops: DynamicOps + 'static>: Debug {
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
    on_error: Arc<dyn Fn(String) -> String>,
    value: Arc<dyn Fn() -> A>,
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
    value: Arc<dyn Fn() -> A>,
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
    value: Arc<dyn Fn() -> A>,
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
    name: Arc<dyn Fn() -> String>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for OfMapCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OfMapCodec")
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
        self.codec
            .compressed_decode(ops, input)
            .flat_map(|r| DataResult::success((r, input.clone())))
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for MapCodecCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
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
    value: Arc<dyn Fn() -> A>,
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
    value: Arc<dyn Fn() -> A>,
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
    wrapped: Arc<dyn Fn(Arc<dyn Codec<A, Ops>>) -> Arc<dyn MapCodec<A, Ops>>>,
    cell: std::cell::OnceCell<Arc<dyn MapCodec<A, Ops>>>,
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
            .flat_map(|r| DataResult::success((r, input.clone())))
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for RecursiveMapCodecCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        self.parent.get().encode(input, ops, &mut *builder);
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
