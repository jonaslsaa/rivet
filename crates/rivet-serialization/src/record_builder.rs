//! Port of `com.mojang.serialization.codecs.RecordCodecBuilder`.
//!
//! Java's `RecordCodecBuilder<O, F>` is an `Applicative` over the
//! `Products.Pn` group builder: `instance.group(field1, field2).apply(instance,
//! Foo::new)` assembles a `MapCodec<O>` whose decode reads each field (with
//! error accumulation via `DataResult.apply2`/`ap2`) and whose encode pulls
//! each field's value out of `O` via the getters.
//!
//! The Rust port parameterizes the whole construction by the concrete ops
//! (`RecordCodecBuilder<O, Ops, F>`); the `Products.Pn` arity chain is reduced
//! to `Group`/`Group2`/`Group3`/`Group4` (the observable `and(...)`/`apply`
//! surface). Each composed builder holds a `getter`, an `encoder` closure (that
//! applies the getters to `&O` and writes each field), and a `decoder` that
//! composes the per-field `MapDecoder`s.

use crate::codec::Codec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::lifecycle::Lifecycle;
use crate::map_codec::MapCodec;
use crate::map_decoder::MapDecoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.serialization.codecs.RecordCodecBuilder<O, F>`.
pub struct RecordCodecBuilder<O, Ops: DynamicOps + 'static, F> {
    /// `Function<O, F>` — the getter.
    getter: Arc<dyn Fn(&O) -> F>,
    /// `Function<O, MapEncoder<F>>` applied at encode time — encodes the field
    /// by pulling `getter(o)` out of the input `O`.
    encoder: Arc<dyn Fn(&O, &Ops, &mut dyn RecordBuilder<Output = Ops::Output>)>,
    /// The `MapDecoder<F>` half.
    decoder: Arc<dyn MapDecoder<F, Ops>>,
}

impl<O: 'static, Ops: DynamicOps + 'static, F> Clone for RecordCodecBuilder<O, Ops, F> {
    fn clone(&self) -> Self {
        RecordCodecBuilder {
            getter: self.getter.clone(),
            encoder: self.encoder.clone(),
            decoder: self.decoder.clone(),
        }
    }
}

impl<O: 'static, Ops: DynamicOps + 'static, F> RecordCodecBuilder<O, Ops, F> {
    /// `RecordCodecBuilder.of(Function<O, F>, MapCodec<F>)`.
    pub fn of(getter: Arc<dyn Fn(&O) -> F>, codec: Arc<dyn MapCodec<F, Ops>>) -> Self
    where
        F: 'static,
    {
        let g = getter.clone();
        let enc = codec.clone();
        let dec: Arc<dyn MapDecoder<F, Ops>> =
            Arc::new(crate::map_codec::MapCodecDecoderHalf(codec));
        RecordCodecBuilder {
            getter,
            encoder: Arc::new(move |o, ops, prefix| enc.encode(&g(o), ops, prefix)),
            decoder: dec,
        }
    }

    /// `RecordCodecBuilder.of(Function<O, F>, String, Codec<F>)`.
    pub fn of_named(
        getter: Arc<dyn Fn(&O) -> F>,
        name: String,
        field_codec: Arc<dyn Codec<F, Ops>>,
    ) -> Self
    where
        F: 'static,
    {
        RecordCodecBuilder::of(getter, crate::codec::field_of(field_codec, name))
    }

    /// `RecordCodecBuilder.point(F)`.
    pub fn point(instance: F) -> Self
    where
        F: Clone + 'static,
    {
        RecordCodecBuilder::point_with_lifecycle(instance, Lifecycle::experimental())
    }

    /// `RecordCodecBuilder.stable(F)`.
    pub fn stable(instance: F) -> Self
    where
        F: Clone + 'static,
    {
        RecordCodecBuilder::point_with_lifecycle(instance, Lifecycle::stable())
    }

    /// `RecordCodecBuilder.deprecated(F, int)`.
    pub fn deprecated(instance: F, since: i32) -> Self
    where
        F: Clone + 'static,
    {
        RecordCodecBuilder::point_with_lifecycle(instance, Lifecycle::deprecated(since))
    }

    /// `RecordCodecBuilder.point(F, Lifecycle)`.
    pub fn point_with_lifecycle(instance: F, lifecycle: Lifecycle) -> Self
    where
        F: Clone + 'static,
    {
        let instance_enc = instance.clone();
        let instance_enc_encoder = instance_enc.clone();
        let instance_enc_unit = instance_enc.clone();
        RecordCodecBuilder {
            getter: Arc::new(move |_o| instance.clone()),
            encoder: Arc::new(move |_o, _ops, _prefix| {
                let _ = &instance_enc_encoder;
            }),
            decoder: crate::map_decoder::with_lifecycle::<F, Ops>(
                crate::map_decoder::unit_with::<F, Ops>(Arc::new(move || {
                    instance_enc_unit.clone()
                })),
                lifecycle,
            ),
        }
    }

    /// The `MapDecoder` half.
    pub fn decoder(&self) -> Arc<dyn MapDecoder<F, Ops>> {
        self.decoder.clone()
    }

    /// The `MapEncoder`-shaped getter-driven encoder.
    fn encode(&self, o: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        (self.encoder)(o, ops, prefix)
    }
}

impl<O: 'static, Ops: DynamicOps + 'static, F> Debug for RecordCodecBuilder<O, Ops, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecordCodecBuilder[{:?}]", self.decoder)
    }
}

/// `RecordCodecBuilder.Instance<O>` — the applicative instance (`group`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Instance<O, Ops: DynamicOps + 'static>(std::marker::PhantomData<(O, Ops)>);

impl<O: 'static, Ops: DynamicOps + 'static> Instance<O, Ops> {
    /// `new Instance<>()`.
    pub fn new() -> Self {
        Instance(std::marker::PhantomData)
    }

    /// `Instance.stable(A)`.
    pub fn stable<T: Clone + 'static>(&self, a: T) -> RecordCodecBuilder<O, Ops, T> {
        RecordCodecBuilder::stable(a)
    }

    /// `Instance.deprecated(A, int)`.
    pub fn deprecated<T: Clone + 'static>(
        &self,
        a: T,
        since: i32,
    ) -> RecordCodecBuilder<O, Ops, T> {
        RecordCodecBuilder::deprecated(a, since)
    }

    /// `Instance.point(A, Lifecycle)`.
    pub fn point<T: Clone + 'static>(
        &self,
        a: T,
        lifecycle: Lifecycle,
    ) -> RecordCodecBuilder<O, Ops, T> {
        RecordCodecBuilder::point_with_lifecycle(a, lifecycle)
    }

    /// `Instance.point(A)`.
    pub fn point_default<T: Clone + 'static>(&self, a: T) -> RecordCodecBuilder<O, Ops, T> {
        RecordCodecBuilder::point(a)
    }

    /// `Kind1.group(App<F, T1>)` — starts the builder chain.
    pub fn group<T>(&self, t: RecordCodecBuilder<O, Ops, T>) -> Group<O, Ops, T> {
        Group { t }
    }
}

/// `Products.P1` — a one-field group.
#[derive(Debug, Clone)]
pub struct Group<O: 'static, Ops: DynamicOps + 'static, T> {
    pub(crate) t: RecordCodecBuilder<O, Ops, T>,
}

impl<O: 'static, Ops: DynamicOps + 'static, T> Group<O, Ops, T> {
    /// `Products.P1.and(App<F, T2>)`.
    pub fn and<U>(self, u: RecordCodecBuilder<O, Ops, U>) -> Group2<O, Ops, T, U> {
        Group2 { t: self.t, u }
    }

    /// `Products.P1.apply(Applicative, Function<T1, R>)`.
    pub fn apply<R: 'static>(
        self,
        _instance: &Instance<O, Ops>,
        function: Arc<dyn Fn(T) -> R>,
    ) -> RecordCodecBuilder<O, Ops, R>
    where
        T: Clone + 'static,
    {
        compose1(self.t, function)
    }
}

/// `Products.P2`.
#[derive(Debug, Clone)]
pub struct Group2<O: 'static, Ops: DynamicOps + 'static, T, U> {
    pub(crate) t: RecordCodecBuilder<O, Ops, T>,
    pub(crate) u: RecordCodecBuilder<O, Ops, U>,
}

impl<O: 'static, Ops: DynamicOps + 'static, T, U> Group2<O, Ops, T, U> {
    /// `Products.P2.and(App<F, T3>)`.
    pub fn and<V>(self, v: RecordCodecBuilder<O, Ops, V>) -> Group3<O, Ops, T, U, V> {
        Group3 {
            t: self.t,
            u: self.u,
            v,
        }
    }

    /// `Products.P2.apply(Applicative, Function2<T1, T2, R>)`.
    pub fn apply<R: 'static>(
        self,
        _instance: &Instance<O, Ops>,
        function: Arc<dyn Fn(T, U) -> R>,
    ) -> RecordCodecBuilder<O, Ops, R>
    where
        T: Clone + 'static,
        U: Clone + 'static,
    {
        compose2(self.t, self.u, function)
    }
}

/// `Products.P3`.
#[derive(Debug, Clone)]
pub struct Group3<O: 'static, Ops: DynamicOps + 'static, T, U, V> {
    pub(crate) t: RecordCodecBuilder<O, Ops, T>,
    pub(crate) u: RecordCodecBuilder<O, Ops, U>,
    pub(crate) v: RecordCodecBuilder<O, Ops, V>,
}

impl<O: 'static, Ops: DynamicOps + 'static, T, U, V> Group3<O, Ops, T, U, V> {
    /// `Products.P3.and(App<F, T4>)`.
    pub fn and<W>(self, w: RecordCodecBuilder<O, Ops, W>) -> Group4<O, Ops, T, U, V, W> {
        Group4 {
            t: self.t,
            u: self.u,
            v: self.v,
            w,
        }
    }

    /// `Products.P3.apply(Applicative, Function3<T1, T2, T3, R>)`.
    pub fn apply<R: 'static>(
        self,
        _instance: &Instance<O, Ops>,
        function: Arc<dyn Fn(T, U, V) -> R>,
    ) -> RecordCodecBuilder<O, Ops, R>
    where
        T: Clone + 'static,
        U: Clone + 'static,
        V: Clone + 'static,
    {
        compose3(self.t, self.u, self.v, function)
    }
}

/// `Products.P4`.
#[derive(Debug, Clone)]
pub struct Group4<O: 'static, Ops: DynamicOps + 'static, T, U, V, W> {
    pub(crate) t: RecordCodecBuilder<O, Ops, T>,
    pub(crate) u: RecordCodecBuilder<O, Ops, U>,
    pub(crate) v: RecordCodecBuilder<O, Ops, V>,
    pub(crate) w: RecordCodecBuilder<O, Ops, W>,
}

impl<O: 'static, Ops: DynamicOps + 'static, T, U, V, W> Group4<O, Ops, T, U, V, W> {
    /// `Products.P4.apply(Applicative, Function4<T1, T2, T3, T4, R>)`.
    pub fn apply<R: 'static>(
        self,
        _instance: &Instance<O, Ops>,
        function: Arc<dyn Fn(T, U, V, W) -> R>,
    ) -> RecordCodecBuilder<O, Ops, R>
    where
        T: Clone + 'static,
        U: Clone + 'static,
        V: Clone + 'static,
        W: Clone + 'static,
    {
        compose4(self.t, self.u, self.v, self.w, function)
    }
}

/// `Applicative.lift1` composition.
fn compose1<O: 'static, Ops: DynamicOps + 'static, T: Clone + 'static, R: 'static>(
    t: RecordCodecBuilder<O, Ops, T>,
    function: Arc<dyn Fn(T) -> R>,
) -> RecordCodecBuilder<O, Ops, R> {
    let t_getter = t.getter.clone();
    let t_enc = t.encoder.clone();
    let t_dec = t.decoder.clone();
    let function_enc = function.clone();

    // getter: getter.andThen(func)
    let getter = Arc::new(move |o: &O| function_enc(t_getter(o)));

    // encoder: fEnc.encode(a1 -> input, ...) then aEnc.encode(aFromO, ...);
    // fEnc is an empty encoder, so only the field encoder runs.
    let encoder = Arc::new(
        move |o: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
            t_enc(o, ops, prefix)
        },
    );

    // decoder: t.decode(ops, input).map(func)
    let function_map: Arc<dyn Fn(&T) -> R> = Arc::new(move |t: &T| function(t.clone()));
    let decoder = crate::map_decoder::map(t_dec, function_map);

    RecordCodecBuilder {
        getter,
        encoder,
        decoder,
    }
}

/// `Applicative.ap2` composition — decode with error accumulation, encode both
/// fields via their getters.
fn compose2<
    O: 'static,
    Ops: DynamicOps + 'static,
    T: Clone + 'static,
    U: Clone + 'static,
    R: 'static,
>(
    t: RecordCodecBuilder<O, Ops, T>,
    u: RecordCodecBuilder<O, Ops, U>,
    function: Arc<dyn Fn(T, U) -> R>,
) -> RecordCodecBuilder<O, Ops, R> {
    let t_getter = t.getter.clone();
    let u_getter = u.getter.clone();
    let t_enc = t.encoder.clone();
    let u_enc = u.encoder.clone();
    let t_dec = t.decoder.clone();
    let u_dec = u.decoder.clone();
    let function_enc = function.clone();
    let function_dec = function.clone();

    let getter = Arc::new(move |o: &O| function_enc(t_getter(o), u_getter(o)));

    let encoder = Arc::new(
        move |o: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
            t_enc(o, ops, prefix);
            u_enc(o, ops, prefix);
        },
    );

    // decoder: Java `DataResult.instance().ap2(function.decode, t.decode, u.decode)`
    let decoder = Arc::new(MapDecoderComposed2 {
        t: t_dec,
        u: u_dec,
        function: function_dec,
        _marker: std::marker::PhantomData::<O>,
    });

    RecordCodecBuilder {
        getter,
        encoder,
        decoder,
    }
}

/// `Applicative.ap3` composition.
fn compose3<
    O: 'static,
    Ops: DynamicOps + 'static,
    T: Clone + 'static,
    U: Clone + 'static,
    V: Clone + 'static,
    R: 'static,
>(
    t: RecordCodecBuilder<O, Ops, T>,
    u: RecordCodecBuilder<O, Ops, U>,
    v: RecordCodecBuilder<O, Ops, V>,
    function: Arc<dyn Fn(T, U, V) -> R>,
) -> RecordCodecBuilder<O, Ops, R> {
    let t_getter = t.getter.clone();
    let u_getter = u.getter.clone();
    let v_getter = v.getter.clone();
    let t_enc = t.encoder.clone();
    let u_enc = u.encoder.clone();
    let v_enc = v.encoder.clone();
    let t_dec = t.decoder.clone();
    let u_dec = u.decoder.clone();
    let v_dec = v.decoder.clone();
    let function_enc = function.clone();
    let function_dec = function.clone();

    let getter = Arc::new(move |o: &O| function_enc(t_getter(o), u_getter(o), v_getter(o)));

    let encoder = Arc::new(
        move |o: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
            t_enc(o, ops, prefix);
            u_enc(o, ops, prefix);
            v_enc(o, ops, prefix);
        },
    );

    let decoder = Arc::new(MapDecoderComposed3 {
        t: t_dec,
        u: u_dec,
        v: v_dec,
        function: function_dec,
        _marker: std::marker::PhantomData::<O>,
    });

    RecordCodecBuilder {
        getter,
        encoder,
        decoder,
    }
}

/// `Applicative.ap4` composition.
fn compose4<
    O: 'static,
    Ops: DynamicOps + 'static,
    T: Clone + 'static,
    U: Clone + 'static,
    V: Clone + 'static,
    W: Clone + 'static,
    R: 'static,
>(
    t: RecordCodecBuilder<O, Ops, T>,
    u: RecordCodecBuilder<O, Ops, U>,
    v: RecordCodecBuilder<O, Ops, V>,
    w: RecordCodecBuilder<O, Ops, W>,
    function: Arc<dyn Fn(T, U, V, W) -> R>,
) -> RecordCodecBuilder<O, Ops, R> {
    let t_getter = t.getter.clone();
    let u_getter = u.getter.clone();
    let v_getter = v.getter.clone();
    let w_getter = w.getter.clone();
    let t_enc = t.encoder.clone();
    let u_enc = u.encoder.clone();
    let v_enc = v.encoder.clone();
    let w_enc = w.encoder.clone();
    let t_dec = t.decoder.clone();
    let u_dec = u.decoder.clone();
    let v_dec = v.decoder.clone();
    let w_dec = w.decoder.clone();
    let function_enc = function.clone();
    let function_dec = function.clone();

    let getter =
        Arc::new(move |o: &O| function_enc(t_getter(o), u_getter(o), v_getter(o), w_getter(o)));

    let encoder = Arc::new(
        move |o: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
            t_enc(o, ops, prefix);
            u_enc(o, ops, prefix);
            v_enc(o, ops, prefix);
            w_enc(o, ops, prefix);
        },
    );

    let decoder = Arc::new(MapDecoderComposed4 {
        t: t_dec,
        u: u_dec,
        v: v_dec,
        w: w_dec,
        function: function_dec,
        _marker: std::marker::PhantomData::<O>,
    });

    RecordCodecBuilder {
        getter,
        encoder,
        decoder,
    }
}

/// Two-field composed `MapDecoder` (Java `Instance.ap2` decoder).
pub struct MapDecoderComposed2<O: 'static, Ops: DynamicOps + 'static, T, U, R> {
    pub(crate) t: Arc<dyn MapDecoder<T, Ops>>,
    pub(crate) u: Arc<dyn MapDecoder<U, Ops>>,
    pub(crate) function: Arc<dyn Fn(T, U) -> R>,
    pub(crate) _marker: std::marker::PhantomData<O>,
}
impl<O, Ops: DynamicOps + 'static, T, U, R> std::fmt::Debug
    for MapDecoderComposed2<O, Ops, T, U, R>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapDecoderComposed2")
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, R> Keyable<Ops> for MapDecoderComposed2<O, Ops, T, U, R> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.t.keys(ops);
        keys.extend(self.u.keys(ops));
        keys
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, R> MapDecoder<R, Ops>
    for MapDecoderComposed2<O, Ops, T, U, R>
where
    T: Clone,
    U: Clone,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<R> {
        let t = self.t.clone();
        let u = self.u.clone();
        let function = self.function.clone();
        t.decode(ops, input).flat_map(move |tv| {
            let tv = tv.clone();
            u.decode(ops, input).map(move |uv| function(tv, uv.clone()))
        })
    }
}

/// Three-field composed `MapDecoder`.
pub struct MapDecoderComposed3<O: 'static, Ops: DynamicOps + 'static, T, U, V, R> {
    pub(crate) t: Arc<dyn MapDecoder<T, Ops>>,
    pub(crate) u: Arc<dyn MapDecoder<U, Ops>>,
    pub(crate) v: Arc<dyn MapDecoder<V, Ops>>,
    pub(crate) function: Arc<dyn Fn(T, U, V) -> R>,
    pub(crate) _marker: std::marker::PhantomData<O>,
}
impl<O, Ops: DynamicOps + 'static, T, U, V, R> std::fmt::Debug
    for MapDecoderComposed3<O, Ops, T, U, V, R>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapDecoderComposed3")
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, V, R> Keyable<Ops>
    for MapDecoderComposed3<O, Ops, T, U, V, R>
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.t.keys(ops);
        keys.extend(self.u.keys(ops));
        keys.extend(self.v.keys(ops));
        keys
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, V, R> MapDecoder<R, Ops>
    for MapDecoderComposed3<O, Ops, T, U, V, R>
where
    T: Clone,
    U: Clone,
    V: Clone,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<R> {
        let t = self.t.clone();
        let u = self.u.clone();
        let v = self.v.clone();
        let function = self.function.clone();
        t.decode(ops, input).flat_map(move |tv| {
            let tv = tv.clone();
            u.decode(ops, input).flat_map(move |uv| {
                let uv = uv.clone();
                v.decode(ops, input)
                    .map(move |vv| function(tv, uv, vv.clone()))
            })
        })
    }
}

/// Four-field composed `MapDecoder`.
pub struct MapDecoderComposed4<O: 'static, Ops: DynamicOps + 'static, T, U, V, W, R> {
    pub(crate) t: Arc<dyn MapDecoder<T, Ops>>,
    pub(crate) u: Arc<dyn MapDecoder<U, Ops>>,
    pub(crate) v: Arc<dyn MapDecoder<V, Ops>>,
    pub(crate) w: Arc<dyn MapDecoder<W, Ops>>,
    pub(crate) function: Arc<dyn Fn(T, U, V, W) -> R>,
    pub(crate) _marker: std::marker::PhantomData<O>,
}
impl<O, Ops: DynamicOps + 'static, T, U, V, W, R> std::fmt::Debug
    for MapDecoderComposed4<O, Ops, T, U, V, W, R>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapDecoderComposed4")
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, V, W, R> Keyable<Ops>
    for MapDecoderComposed4<O, Ops, T, U, V, W, R>
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.t.keys(ops);
        keys.extend(self.u.keys(ops));
        keys.extend(self.v.keys(ops));
        keys.extend(self.w.keys(ops));
        keys
    }
}

impl<O, Ops: DynamicOps + 'static, T, U, V, W, R> MapDecoder<R, Ops>
    for MapDecoderComposed4<O, Ops, T, U, V, W, R>
where
    T: Clone,
    U: Clone,
    V: Clone,
    W: Clone,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<R> {
        let t = self.t.clone();
        let u = self.u.clone();
        let v = self.v.clone();
        let w = self.w.clone();
        let function = self.function.clone();
        t.decode(ops, input).flat_map(move |tv| {
            let tv = tv.clone();
            u.decode(ops, input).flat_map(move |uv| {
                let uv = uv.clone();
                v.decode(ops, input).flat_map(move |vv| {
                    let vv = vv.clone();
                    w.decode(ops, input)
                        .map(move |wv| function(tv, uv, vv, wv.clone()))
                })
            })
        })
    }
}

/// `RecordCodecBuilder.build(App<Mu<O>, O>)` — turns the composed builder into
/// a `MapCodec<O>`.
pub fn build<O, Ops: DynamicOps + 'static>(
    builder: RecordCodecBuilder<O, Ops, O>,
) -> Arc<dyn MapCodec<O, Ops>>
where
    O: 'static,
{
    let getter = builder.getter.clone();
    let encoder = builder.encoder.clone();
    let decoder = builder.decoder.clone();
    let name = format!("RecordCodec[{:?}]", decoder);

    crate::map_codec::of(Arc::new(BuiltEncoder { getter, encoder }), decoder, name)
}

/// The `MapEncoder` half of a built record codec — encodes the fields by
/// applying the getters to the input `O`.
pub struct BuiltEncoder<O, Ops: DynamicOps + 'static> {
    getter: Arc<dyn Fn(&O) -> O>,
    encoder: Arc<dyn Fn(&O, &Ops, &mut dyn RecordBuilder<Output = Ops::Output>)>,
}
impl<O, Ops: DynamicOps + 'static> std::fmt::Debug for BuiltEncoder<O, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BuiltEncoder")
    }
}

impl<O, Ops: DynamicOps + 'static> Keyable<Ops> for BuiltEncoder<O, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let _ = ops;
        Vec::new()
    }
}

impl<O, Ops: DynamicOps + 'static> crate::map_encoder::MapEncoder<O, Ops> for BuiltEncoder<O, Ops> {
    fn encode(&self, input: &O, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        let _ = &self.getter;
        (self.encoder)(input, ops, prefix)
    }
}

/// `RecordCodecBuilder.create(Function<Instance<O>, App>)` —
/// `build(builder.apply(instance())).codec()`.
pub fn create<O, Ops: DynamicOps + 'static>(
    builder: impl FnOnce(&Instance<O, Ops>) -> RecordCodecBuilder<O, Ops, O>,
) -> Arc<dyn Codec<O, Ops>>
where
    O: 'static,
{
    let instance = Instance::new();
    let built = build(builder(&instance));
    crate::map_codec::codec_of(built)
}

/// `RecordCodecBuilder.mapCodec(Function<Instance<O>, App>)` —
/// `build(builder.apply(instance()))`.
pub fn map_codec<O, Ops: DynamicOps + 'static>(
    builder: impl FnOnce(&Instance<O, Ops>) -> RecordCodecBuilder<O, Ops, O>,
) -> Arc<dyn MapCodec<O, Ops>>
where
    O: 'static,
{
    let instance = Instance::new();
    build(builder(&instance))
}
