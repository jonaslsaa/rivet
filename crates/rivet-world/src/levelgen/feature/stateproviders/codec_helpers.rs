//! Local copies of the manual multi-field map-codec composition helpers,
//! extending `density_functions.rs`' module-private helpers from arity 6 to 8.
//!
//! The shared `record_builder` infrastructure caps at `Group6` (and
//! `rivet-serialization::data_result` caps its applicatives at `ap6`). The
//! 7-field `DualNoiseProvider` and 8-field `NoiseThresholdProvider` records are
//! the first to exceed that cap, so they are composed manually with
//! `map_encoder`/`map_decoder` halves — the same pattern `density_functions.rs`
//! uses for its 5/6-field records (whose `map_encoder_fieldsN`/`map_decoder_apN`
//! helpers are module-private, hence the local copies here, extended to arity
//! 8 with local `ap7`/`ap8`).

use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::Arc;

/// `Fn7` — the 7-arg function alias (functions.rs stops at `Fn6`).
pub type Fn7<A, B, C, D, E, F, G, R> = Arc<dyn Fn(&A, &B, &C, &D, &E, &F, &G) -> R + Send + Sync>;
/// `Fn8` — the 8-arg function alias (functions.rs stops at `Fn6`).
pub type Fn8<A, B, C, D, E, F, G, H, R> =
    Arc<dyn Fn(&A, &B, &C, &D, &E, &F, &G, &H) -> R + Send + Sync>;

/// `DataResult.INSTANCE.ap7` — Java's default `ap7`, chaining `ap3`/`ap4`:
/// `ap4(ap3(map(Function7::curry3, func), t1, t2, t3), t4, t5, t6, t7)`.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn ap7<
    T1: Clone + Send + Sync + 'static,
    T2: Clone + Send + Sync + 'static,
    T3: Clone + Send + Sync + 'static,
    T4: Clone + Send + Sync + 'static,
    T5: Clone + Send + Sync + 'static,
    T6: Clone + Send + Sync + 'static,
    T7: Clone + Send + Sync + 'static,
    R: 'static,
>(
    fr: DataResult<Fn7<T1, T2, T3, T4, T5, T6, T7, R>>,
    a: DataResult<T1>,
    b: DataResult<T2>,
    c: DataResult<T3>,
    d: DataResult<T4>,
    e: DataResult<T5>,
    f: DataResult<T6>,
    g: DataResult<T7>,
) -> DataResult<R> {
    // `curry3`: `(t1..t3) -> (t4..t7) -> f(t1..t7)`.
    let curried: DataResult<Fn3<T1, T2, T3, Fn4<T4, T5, T6, T7, R>>> = fr.map(|func| {
        let func = func.clone();
        let curried_fn: Fn3<T1, T2, T3, Fn4<T4, T5, T6, T7, R>> =
            Arc::new(move |x1: &T1, x2: &T2, x3: &T3| {
                let func = func.clone();
                let x1 = x1.clone();
                let x2 = x2.clone();
                let x3 = x3.clone();
                let inner: Fn4<T4, T5, T6, T7, R> =
                    Arc::new(move |y1: &T4, y2: &T5, y3: &T6, y4: &T7| {
                        func(&x1, &x2, &x3, y1, y2, y3, y4)
                    });
                inner
            });
        curried_fn
    });
    let step1 = rivet_serialization::data_result::ap3(curried, a, b, c);
    rivet_serialization::data_result::ap4(step1, d, e, f, g)
}

/// `DataResult.INSTANCE.ap8` — Java's default `ap8`, chaining `ap4`/`ap4`:
/// `ap4(ap4(map(Function8::curry4, func), t1, t2, t3, t4), t5, t6, t7, t8)`.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn ap8<
    T1: Clone + Send + Sync + 'static,
    T2: Clone + Send + Sync + 'static,
    T3: Clone + Send + Sync + 'static,
    T4: Clone + Send + Sync + 'static,
    T5: Clone + Send + Sync + 'static,
    T6: Clone + Send + Sync + 'static,
    T7: Clone + Send + Sync + 'static,
    T8: Clone + Send + Sync + 'static,
    R: 'static,
>(
    fr: DataResult<Fn8<T1, T2, T3, T4, T5, T6, T7, T8, R>>,
    a: DataResult<T1>,
    b: DataResult<T2>,
    c: DataResult<T3>,
    d: DataResult<T4>,
    e: DataResult<T5>,
    f: DataResult<T6>,
    g: DataResult<T7>,
    h: DataResult<T8>,
) -> DataResult<R> {
    // `curry4`: `(t1..t4) -> (t5..t8) -> f(t1..t8)`.
    let curried: DataResult<Fn4<T1, T2, T3, T4, Fn4<T5, T6, T7, T8, R>>> = fr.map(|func| {
        let func = func.clone();
        let curried_fn: Fn4<T1, T2, T3, T4, Fn4<T5, T6, T7, T8, R>> =
            Arc::new(move |x1: &T1, x2: &T2, x3: &T3, x4: &T4| {
                let func = func.clone();
                let x1 = x1.clone();
                let x2 = x2.clone();
                let x3 = x3.clone();
                let x4 = x4.clone();
                let inner: Fn4<T5, T6, T7, T8, R> =
                    Arc::new(move |y1: &T5, y2: &T6, y3: &T7, y4: &T8| {
                        func(&x1, &x2, &x3, &x4, y1, y2, y3, y4)
                    });
                inner
            });
        curried_fn
    });
    let step1 = rivet_serialization::data_result::ap4(curried, a, b, c, d);
    rivet_serialization::data_result::ap4(step1, e, f, g, h)
}

/// Compose 7 field encoders into a single `MapEncoder<C>` (Java's
/// `Products.P7` encoder).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn map_encoder_fields7<C, Ops: DynamicOps + 'static, T, U, V, W, X, Y, Z>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    z: Arc<dyn MapCodec<Z, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
    w_getter: Arc<dyn Fn(&C) -> W + Send + Sync>,
    x_getter: Arc<dyn Fn(&C) -> X + Send + Sync>,
    y_getter: Arc<dyn Fn(&C) -> Y + Send + Sync>,
    z_getter: Arc<dyn Fn(&C) -> Z + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
    W: 'static,
    X: 'static,
    Y: 'static,
    Z: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let w_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(w));
    let x_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(x));
    let y_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(y));
    let z_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(z));
    let (t_enc_e, t_enc_k) = (t_enc.clone(), t_enc.clone());
    let (u_enc_e, u_enc_k) = (u_enc.clone(), u_enc.clone());
    let (v_enc_e, v_enc_k) = (v_enc.clone(), v_enc.clone());
    let (w_enc_e, w_enc_k) = (w_enc.clone(), w_enc.clone());
    let (x_enc_e, x_enc_k) = (x_enc.clone(), x_enc.clone());
    let (y_enc_e, y_enc_k) = (y_enc.clone(), y_enc.clone());
    let (z_enc_e, z_enc_k) = (z_enc.clone(), z_enc.clone());
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
                w_enc_e.encode(&w_getter(input), ops, prefix);
                x_enc_e.encode(&x_getter(input), ops, prefix);
                y_enc_e.encode(&y_getter(input), ops, prefix);
                z_enc_e.encode(&z_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys.extend(w_enc_k.keys(ops));
            keys.extend(x_enc_k.keys(ops));
            keys.extend(y_enc_k.keys(ops));
            keys.extend(z_enc_k.keys(ops));
            keys
        }),
    )
}

/// Compose 8 field encoders into a single `MapEncoder<C>` (Java's
/// `Products.P8` encoder).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn map_encoder_fields8<C, Ops: DynamicOps + 'static, T, U, V, W, X, Y, Z, A>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    z: Arc<dyn MapCodec<Z, Ops>>,
    a: Arc<dyn MapCodec<A, Ops>>,
    t_getter: Arc<dyn Fn(&C) -> T + Send + Sync>,
    u_getter: Arc<dyn Fn(&C) -> U + Send + Sync>,
    v_getter: Arc<dyn Fn(&C) -> V + Send + Sync>,
    w_getter: Arc<dyn Fn(&C) -> W + Send + Sync>,
    x_getter: Arc<dyn Fn(&C) -> X + Send + Sync>,
    y_getter: Arc<dyn Fn(&C) -> Y + Send + Sync>,
    z_getter: Arc<dyn Fn(&C) -> Z + Send + Sync>,
    a_getter: Arc<dyn Fn(&C) -> A + Send + Sync>,
) -> Arc<dyn MapEncoder<C, Ops>>
where
    C: 'static,
    T: 'static,
    U: 'static,
    V: 'static,
    W: 'static,
    X: 'static,
    Y: 'static,
    Z: 'static,
    A: 'static,
{
    let t_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(t));
    let u_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(u));
    let v_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(v));
    let w_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(w));
    let x_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(x));
    let y_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(y));
    let z_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(z));
    let a_enc = Arc::new(rivet_serialization::map_codec::MapCodecEncoderHalf(a));
    let (t_enc_e, t_enc_k) = (t_enc.clone(), t_enc.clone());
    let (u_enc_e, u_enc_k) = (u_enc.clone(), u_enc.clone());
    let (v_enc_e, v_enc_k) = (v_enc.clone(), v_enc.clone());
    let (w_enc_e, w_enc_k) = (w_enc.clone(), w_enc.clone());
    let (x_enc_e, x_enc_k) = (x_enc.clone(), x_enc.clone());
    let (y_enc_e, y_enc_k) = (y_enc.clone(), y_enc.clone());
    let (z_enc_e, z_enc_k) = (z_enc.clone(), z_enc.clone());
    let (a_enc_e, a_enc_k) = (a_enc.clone(), a_enc.clone());
    rivet_serialization::map_encoder::of(
        Arc::new(
            move |input: &C, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                t_enc_e.encode(&t_getter(input), ops, prefix);
                u_enc_e.encode(&u_getter(input), ops, prefix);
                v_enc_e.encode(&v_getter(input), ops, prefix);
                w_enc_e.encode(&w_getter(input), ops, prefix);
                x_enc_e.encode(&x_getter(input), ops, prefix);
                y_enc_e.encode(&y_getter(input), ops, prefix);
                z_enc_e.encode(&z_getter(input), ops, prefix);
                a_enc_e.encode(&a_getter(input), ops, prefix);
            },
        ),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_enc_k.keys(ops);
            keys.extend(u_enc_k.keys(ops));
            keys.extend(v_enc_k.keys(ops));
            keys.extend(w_enc_k.keys(ops));
            keys.extend(x_enc_k.keys(ops));
            keys.extend(y_enc_k.keys(ops));
            keys.extend(z_enc_k.keys(ops));
            keys.extend(a_enc_k.keys(ops));
            keys
        }),
    )
}

/// `ap7` over seven field decoders — decode all fields (error accumulation),
/// apply the constructor.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn map_decoder_ap7<T, U, V, W, X, Y, Z, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    z: Arc<dyn MapCodec<Z, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V, &W, &X, &Y, &Z) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    W: Clone + Send + Sync + 'static,
    X: Clone + Send + Sync + 'static,
    Y: Clone + Send + Sync + 'static,
    Z: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let w_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(w));
    let x_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(x));
    let y_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(y));
    let z_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(z));
    let (t_dec_d, t_dec_k) = (t_dec.clone(), t_dec.clone());
    let (u_dec_d, u_dec_k) = (u_dec.clone(), u_dec.clone());
    let (v_dec_d, v_dec_k) = (v_dec.clone(), v_dec.clone());
    let (w_dec_d, w_dec_k) = (w_dec.clone(), w_dec.clone());
    let (x_dec_d, x_dec_k) = (x_dec.clone(), x_dec.clone());
    let (y_dec_d, y_dec_k) = (y_dec.clone(), y_dec.clone());
    let (z_dec_d, z_dec_k) = (z_dec.clone(), z_dec.clone());
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let w_r = w_dec_d.decode(ops, input);
            let x_r = x_dec_d.decode(ops, input);
            let y_r = y_dec_d.decode(ops, input);
            let z_r = z_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            let fr: DataResult<Fn7<T, U, V, W, X, Y, Z, C>> = DataResult::success(Arc::new(
                move |tv: &T, uv: &U, vv: &V, wv: &W, xv: &X, yv: &Y, zv: &Z| {
                    constructor(tv, uv, vv, wv, xv, yv, zv)
                },
            ));
            ap7(fr, t_r, u_r, v_r, w_r, x_r, y_r, z_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys.extend(w_dec_k.keys(ops));
            keys.extend(x_dec_k.keys(ops));
            keys.extend(y_dec_k.keys(ops));
            keys.extend(z_dec_k.keys(ops));
            keys
        }),
    )
}

/// `ap8` over eight field decoders.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn map_decoder_ap8<T, U, V, W, X, Y, Z, A, C, Ops: DynamicOps + 'static>(
    t: Arc<dyn MapCodec<T, Ops>>,
    u: Arc<dyn MapCodec<U, Ops>>,
    v: Arc<dyn MapCodec<V, Ops>>,
    w: Arc<dyn MapCodec<W, Ops>>,
    x: Arc<dyn MapCodec<X, Ops>>,
    y: Arc<dyn MapCodec<Y, Ops>>,
    z: Arc<dyn MapCodec<Z, Ops>>,
    a: Arc<dyn MapCodec<A, Ops>>,
    constructor: Arc<dyn Fn(&T, &U, &V, &W, &X, &Y, &Z, &A) -> C + Send + Sync>,
) -> Arc<dyn MapDecoder<C, Ops>>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    W: Clone + Send + Sync + 'static,
    X: Clone + Send + Sync + 'static,
    Y: Clone + Send + Sync + 'static,
    Z: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    C: 'static,
{
    let t_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(t));
    let u_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(u));
    let v_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(v));
    let w_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(w));
    let x_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(x));
    let y_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(y));
    let z_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(z));
    let a_dec = Arc::new(rivet_serialization::map_codec::MapCodecDecoderHalf(a));
    let (t_dec_d, t_dec_k) = (t_dec.clone(), t_dec.clone());
    let (u_dec_d, u_dec_k) = (u_dec.clone(), u_dec.clone());
    let (v_dec_d, v_dec_k) = (v_dec.clone(), v_dec.clone());
    let (w_dec_d, w_dec_k) = (w_dec.clone(), w_dec.clone());
    let (x_dec_d, x_dec_k) = (x_dec.clone(), x_dec.clone());
    let (y_dec_d, y_dec_k) = (y_dec.clone(), y_dec.clone());
    let (z_dec_d, z_dec_k) = (z_dec.clone(), z_dec.clone());
    let (a_dec_d, a_dec_k) = (a_dec.clone(), a_dec.clone());
    rivet_serialization::map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let t_r = t_dec_d.decode(ops, input);
            let u_r = u_dec_d.decode(ops, input);
            let v_r = v_dec_d.decode(ops, input);
            let w_r = w_dec_d.decode(ops, input);
            let x_r = x_dec_d.decode(ops, input);
            let y_r = y_dec_d.decode(ops, input);
            let z_r = z_dec_d.decode(ops, input);
            let a_r = a_dec_d.decode(ops, input);
            let constructor = constructor.clone();
            let fr: DataResult<Fn8<T, U, V, W, X, Y, Z, A, C>> = DataResult::success(Arc::new(
                move |tv: &T, uv: &U, vv: &V, wv: &W, xv: &X, yv: &Y, zv: &Z, av: &A| {
                    constructor(tv, uv, vv, wv, xv, yv, zv, av)
                },
            ));
            ap8(fr, t_r, u_r, v_r, w_r, x_r, y_r, z_r, a_r)
        }),
        Arc::new(move |ops: &Ops| {
            let mut keys = t_dec_k.keys(ops);
            keys.extend(u_dec_k.keys(ops));
            keys.extend(v_dec_k.keys(ops));
            keys.extend(w_dec_k.keys(ops));
            keys.extend(x_dec_k.keys(ops));
            keys.extend(y_dec_k.keys(ops));
            keys.extend(z_dec_k.keys(ops));
            keys.extend(a_dec_k.keys(ops));
            keys
        }),
    )
}
