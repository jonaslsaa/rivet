//! Port of `com.mojang.serialization.Codec`.
//!
//! `Codec<A>` is `Encoder<A> + Decoder<A>`. The Rust port pins the ops
//! (`Codec<A, Ops>`); both supertraits are minimal (non-generic) so `Codec` is
//! dyn-compatible and codecs compose via `Arc<dyn Codec<A, Ops>>`. Java's
//! static constructors (`of`, `list`, `pair`, ...) and default combinators
//! (`xmap`, `flatXmap`, `fieldOf`, `orElse`, ...) are free functions. The
//! `CodecEncoderHalf`/`CodecDecoderHalf` adapters re-expose a `Codec` as its
//! `Encoder`/`Decoder` halves (dyn upcasting is not stable in Rust).

use crate::codecs::compound_list_codec::CompoundListCodec;
use crate::codecs::either_codec::EitherCodec;
use crate::codecs::list_codec::ListCodec;
use crate::codecs::optional_field_codec::OptionalFieldCodec;
use crate::codecs::pair_codec::PairCodec;
use crate::codecs::simple_map_codec::SimpleMapCodec;
use crate::codecs::unbounded_map_codec::UnboundedMapCodec;
use crate::codecs::xor_codec::XorCodec;
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable};
use crate::either::Either;
use crate::float_format::{
    java_double_compare, java_double_to_string, java_float_compare, java_float_to_string,
};
use crate::functions::DecoderFn;
use crate::lifecycle::Lifecycle;
use crate::map_codec::{self, MapCodec};
use crate::pair::Pair;
use std::fmt::Debug;
use std::sync::Arc;

/// `Codec.recursive`/`lazyInitialized` wrapper — `Function<Codec<A>, Codec<A>>`.
/// The `Ops: DynamicOps` bound is needed on the RHS (`Codec<A, Ops>`) but is
/// not enforced at alias usage sites, so the `type_alias_bounds` lint is
/// allowed here.
#[allow(type_alias_bounds)]
type RecursiveFn<A, Ops: DynamicOps + 'static> =
    Arc<dyn Fn(Arc<dyn Codec<A, Ops>>) -> Arc<dyn Codec<A, Ops>> + Send + Sync>;

/// `Codec.stringResolver` encode half — `Function<E, Optional<String>>`.
type ToStringFn<E> = Arc<dyn Fn(&E) -> Option<String> + Send + Sync>;

/// `Codec.stringResolver` decode half — `Function<String, Optional<E>>`.
type FromStringFn<E> = Arc<dyn Fn(&String) -> Option<E> + Send + Sync>;

/// `PrimitiveCodec` read half — `BiFunction<Ops, T, DataResult<A>>`.
/// See `RecursiveFn` for why `type_alias_bounds` is allowed.
#[allow(type_alias_bounds)]
type PrimitiveReadFn<A, Ops: DynamicOps + 'static> =
    Arc<dyn Fn(&Ops, &Ops::Output) -> DataResult<A> + Send + Sync>;

/// `PrimitiveCodec` write half — `BiFunction<Ops, A, T>`.
#[allow(type_alias_bounds)]
type PrimitiveWriteFn<A, Ops: DynamicOps + 'static> =
    Arc<dyn Fn(&Ops, &A) -> Ops::Output + Send + Sync>;

/// `com.mojang.serialization.Codec<A>`.
///
/// `Send + Sync` mirrors Paper: the game's codecs are `static final` values
/// shared across netty threads (and the packet `StreamCodec` a status response
/// is built from is itself `Send + Sync`), so a codec must be usable from any
/// connection thread.
pub trait Codec<A, Ops: DynamicOps + 'static>:
    crate::Encoder<A, Ops> + crate::Decoder<A, Ops> + Send + Sync
{
}

// ---------------------------------------------------------------------------
// Adapters exposing a Codec as its Encoder/Decoder halves
// ---------------------------------------------------------------------------

/// The `Encoder` half of a `Codec`.
pub struct CodecEncoderHalf<A, Ops: DynamicOps + 'static>(pub Arc<dyn Codec<A, Ops>>);
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for CodecEncoderHalf<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodecEncoderHalf")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for CodecEncoderHalf<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.0.encode(input, ops, prefix)
    }
}

/// The `Decoder` half of a `Codec`.
pub struct CodecDecoderHalf<A, Ops: DynamicOps + 'static>(pub Arc<dyn Codec<A, Ops>>);
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for CodecDecoderHalf<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodecDecoderHalf")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for CodecDecoderHalf<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.0.decode(ops, input)
    }
}

/// `encoder_of_codec` — the `Encoder` half of a `Codec`.
pub fn encoder_of_codec<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
) -> Arc<dyn crate::Encoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(CodecEncoderHalf(codec))
}

/// `decoder_of_codec` — the `Decoder` half of a `Codec`.
pub fn decoder_of_codec<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
) -> Arc<dyn crate::Decoder<A, Ops>>
where
    A: 'static,
{
    Arc::new(CodecDecoderHalf(codec))
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// `Codec.of(Encoder, Decoder, String)`.
pub fn of<A, Ops: DynamicOps + 'static>(
    encoder: Arc<dyn crate::Encoder<A, Ops>>,
    decoder: Arc<dyn crate::Decoder<A, Ops>>,
    name: String,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    Arc::new(OfCodec {
        encoder,
        decoder,
        name: Arc::new(move || name.clone()),
    })
}

/// `Codec.of(MapEncoder, MapDecoder, Supplier<String>)` — returns a
/// `MapCodec`.
pub fn of_map<A, Ops: DynamicOps + 'static>(
    encoder: Arc<dyn crate::map_encoder::MapEncoder<A, Ops>>,
    decoder: Arc<dyn crate::map_decoder::MapDecoder<A, Ops>>,
    name: String,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    crate::map_codec::of(encoder, decoder, name)
}

/// `Codec.pair(Codec<F>, Codec<S>)`.
pub fn pair<F, S, Ops: DynamicOps + 'static>(
    first: Arc<dyn Codec<F, Ops>>,
    second: Arc<dyn Codec<S, Ops>>,
) -> Arc<dyn Codec<Pair<F, S>, Ops>>
where
    F: 'static,
    S: 'static,
{
    Arc::new(PairCodec { first, second })
}

/// `Codec.mapPair(MapCodec<F>, MapCodec<S>)` — `new PairMapCodec<>(first,
/// second)`, the map-keyed twin of [`pair`].
///
/// Java `Codec.mapPair` combines two `MapCodec`s into one `MapCodec` over the
/// pair (the `appendPropertyCodec` fold in `StateDefinition` chains these).
/// Decode is sequential (no error accumulation); encode applies the second
/// field first.
pub fn map_pair<F, S, Ops: DynamicOps + 'static>(
    first: Arc<dyn crate::map_codec::MapCodec<F, Ops>>,
    second: Arc<dyn crate::map_codec::MapCodec<S, Ops>>,
) -> Arc<dyn crate::map_codec::MapCodec<Pair<F, S>, Ops>>
where
    F: 'static + Clone,
    S: 'static + Clone,
{
    use crate::codecs::pair_map_codec::PairMapCodec;
    Arc::new(PairMapCodec { first, second })
}

/// `Codec.either(Codec<F>, Codec<S>)`.
pub fn either<F, S, Ops: DynamicOps + 'static>(
    first: Arc<dyn Codec<F, Ops>>,
    second: Arc<dyn Codec<S, Ops>>,
) -> Arc<dyn Codec<Either<F, S>, Ops>>
where
    F: 'static,
    S: 'static,
{
    Arc::new(EitherCodec { first, second })
}

/// `Codec.xor(Codec<F>, Codec<S>)`.
pub fn xor<F, S, Ops: DynamicOps + 'static>(
    first: Arc<dyn Codec<F, Ops>>,
    second: Arc<dyn Codec<S, Ops>>,
) -> Arc<dyn Codec<Either<F, S>, Ops>>
where
    F: 'static + Clone + Send + Sync + Debug,
    S: 'static + Clone + Send + Sync + Debug,
{
    Arc::new(XorCodec { first, second })
}

/// `Codec.list(Codec<E>)`.
pub fn list<E, Ops: DynamicOps + 'static>(
    element_codec: Arc<dyn Codec<E, Ops>>,
) -> Arc<dyn Codec<Vec<E>, Ops>>
where
    E: 'static + Clone + Send + Sync,
{
    list_with_range(element_codec, 0, i32::MAX)
}

/// `Codec.list(Codec<E>, int minSize, int maxSize)`.
pub fn list_with_range<E, Ops: DynamicOps + 'static>(
    element_codec: Arc<dyn Codec<E, Ops>>,
    min_size: i32,
    max_size: i32,
) -> Arc<dyn Codec<Vec<E>, Ops>>
where
    E: 'static + Clone + Send + Sync,
{
    Arc::new(ListCodec {
        element_codec,
        min_size,
        max_size,
    })
}

/// `Codec.compoundList(Codec<K>, Codec<V>)`.
pub fn compound_list<K, V, Ops: DynamicOps + 'static>(
    key_codec: Arc<dyn Codec<K, Ops>>,
    element_codec: Arc<dyn Codec<V, Ops>>,
) -> Arc<dyn Codec<Vec<Pair<K, V>>, Ops>>
where
    K: 'static + Clone + Send + Sync,
    V: 'static + Clone + Send + Sync,
{
    Arc::new(CompoundListCodec {
        key_codec,
        element_codec,
    })
}

/// `Codec.simpleMap(Codec<K>, Codec<V>, Keyable keys)`.
pub fn simple_map<K, V, Ops: DynamicOps + 'static>(
    key_codec: Arc<dyn Codec<K, Ops>>,
    element_codec: Arc<dyn Codec<V, Ops>>,
    keys: Arc<dyn Keyable<Ops>>,
) -> Arc<dyn MapCodec<std::collections::HashMap<K, V>, Ops>>
where
    K: 'static + Clone + Send + Sync + std::hash::Hash + Eq + std::fmt::Display,
    V: 'static + Clone + Send + Sync,
{
    Arc::new(SimpleMapCodec {
        key_codec,
        element_codec,
        keys,
    })
}

/// `Codec.unboundedMap(Codec<K>, Codec<V>)`.
pub fn unbounded_map<K, V, Ops: DynamicOps + 'static>(
    key_codec: Arc<dyn Codec<K, Ops>>,
    element_codec: Arc<dyn Codec<V, Ops>>,
) -> Arc<dyn Codec<std::collections::HashMap<K, V>, Ops>>
where
    K: 'static + Clone + Send + Sync + std::hash::Hash + Eq + std::fmt::Display,
    V: 'static + Clone + Send + Sync,
{
    Arc::new(UnboundedMapCodec {
        key_codec,
        element_codec,
    })
}

/// `Codec.optionalField(String, Codec<F>, boolean lenient)`.
pub fn optional_field<F, Ops: DynamicOps + 'static>(
    name: String,
    element_codec: Arc<dyn Codec<F, Ops>>,
    lenient: bool,
) -> Arc<dyn MapCodec<Option<F>, Ops>>
where
    F: 'static + Clone + Send + Sync,
{
    Arc::new(OptionalFieldCodec {
        name,
        element_codec,
        lenient,
    })
}

/// `Codec.lenientOptionalFieldOf(String, F default)` — the with-default form
/// of a lenient optional field.
///
/// Java (DFU 10.0.21, verified from the pinned jar's bytecode):
/// `optionalField(name, codec, true).xmap(o -> o.orElse(default), a ->
/// Objects.equals(a, default) ? Optional.empty() : Optional.of(a))`. The field
/// value defaults on decode (absent OR a present-but-malformed value falls back
/// to `default` via the lenient error path), and is OMITTED on encode when
/// value-equal to `default`.
pub fn lenient_optional_field_of<F, Ops: DynamicOps + 'static>(
    name: &str,
    element_codec: Arc<dyn Codec<F, Ops>>,
    default: F,
) -> Arc<dyn MapCodec<F, Ops>>
where
    F: 'static + Clone + PartialEq + Send + Sync,
{
    let inner = optional_field(name.to_string(), element_codec, true);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<F>| o.clone().unwrap_or_else(|| default_for_decode.clone())),
        Arc::new(move |a: &F| {
            if *a == default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

/// `Codec.optionalFieldOf(String, F default)` — the with-default form of a
/// NON-lenient optional field.
///
/// Java (DFU 10.0.21, verified from the pinned jar's bytecode):
/// `optionalField(name, codec, false).xmap(o -> o.orElse(default), a ->
/// Objects.equals(a, default) ? Optional.empty() : Optional.of(a))`. Unlike
/// [`lenient_optional_field_of`], a present-but-malformed value is a decode
/// error (the optional field is NOT lenient). The field value defaults on
/// decode when absent, and is OMITTED on encode when value-equal to `default`.
pub fn optional_field_of<F, Ops: DynamicOps + 'static>(
    name: &str,
    element_codec: Arc<dyn Codec<F, Ops>>,
    default: F,
) -> Arc<dyn MapCodec<F, Ops>>
where
    F: 'static + Clone + PartialEq + Send + Sync,
{
    let inner = optional_field(name.to_string(), element_codec, false);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<F>| o.clone().unwrap_or_else(|| default_for_decode.clone())),
        Arc::new(move |a: &F| {
            if *a == default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

/// `Codec.recursive(String, Function<Codec<A>, Codec<A>>)`.
pub fn recursive<A, Ops: DynamicOps + 'static>(
    name: String,
    wrapped: RecursiveFn<A, Ops>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    // `Arc::new_cyclic` supplies the parent `Arc<RecursiveCodec>` that the
    // `RecursiveSelf` handed to `wrapped` holds. This is the same strong
    // self-capture the pinned DFU `RecursiveCodec` makes: the lazily-built inner
    // codec embeds a `RecursiveSelf` holding a strong reference back to the
    // parent, so the graph is a strong `Arc` cycle. Java's GC collects
    // unreachable cycles; `Arc` cannot, so here the cycle is permanent.
    // `recursive` is a registration-time constructor: build the codec once per
    // process and reuse it, never per connection.
    Arc::new_cyclic(|weak| RecursiveCodec {
        name,
        wrapped,
        cell: std::sync::OnceLock::new(),
        weak: weak.clone(),
    })
}

/// `Codec.lazyInitialized(Supplier<Codec<A>>)`.
pub fn lazy_initialized<A, Ops: DynamicOps + 'static>(
    delegate: Arc<dyn Fn() -> Arc<dyn Codec<A, Ops>> + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    Arc::new_cyclic(|weak| RecursiveCodec {
        name: "LazyInitialized".to_string(),
        wrapped: Arc::new(move |_self| delegate()),
        cell: std::sync::OnceLock::new(),
        weak: weak.clone(),
    })
}

/// `Codec.fieldOf(String)`.
pub fn field_of<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    name: String,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static,
{
    crate::map_codec::field_of(
        name,
        encoder_of_codec(codec.clone()),
        decoder_of_codec(codec),
    )
}

/// `Codec.EMPTY` — `MapCodec.unit(Unit.INSTANCE).codec()`.
pub fn empty<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<crate::unit::Unit, Ops>> {
    crate::map_codec::codec_of(crate::map_codec::unit_with::<crate::unit::Unit, Ops>(
        Arc::new(|| crate::unit::Unit),
    ))
}

/// `Codec.PASSTHROUGH`.
pub fn passthrough<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<crate::dynamic::Dynamic<Ops::Output>, Ops>> {
    Arc::new(PassthroughCodec {
        _ops: std::marker::PhantomData,
    })
}

// ---------------------------------------------------------------------------
// Primitive codecs
// ---------------------------------------------------------------------------

/// `Codec.BOOL`.
pub fn bool_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<bool, Ops>> {
    Arc::new(PrimitiveCodecImpl::<bool, Ops> {
        read: Arc::new(|ops, input| ops.get_boolean_value(input)),
        write: Arc::new(|ops, value| ops.create_boolean(*value)),
        name: "Bool",
    })
}

/// `Codec.BYTE`.
pub fn byte_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i8, Ops>> {
    Arc::new(PrimitiveCodecImpl::<i8, Ops> {
        // Java `PrimitiveCodec<Byte>`: `getNumberValue(input).map(Number::byteValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.byte_value())),
        write: Arc::new(|ops, value| ops.create_byte(*value)),
        name: "Byte",
    })
}

/// `Codec.SHORT`.
pub fn short_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i16, Ops>> {
    Arc::new(PrimitiveCodecImpl::<i16, Ops> {
        // Java `PrimitiveCodec<Short>`: `.map(Number::shortValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.short_value())),
        write: Arc::new(|ops, value| ops.create_short(*value)),
        name: "Short",
    })
}

/// `Codec.INT`.
pub fn int_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    Arc::new(PrimitiveCodecImpl::<i32, Ops> {
        // Java `PrimitiveCodec<Integer>`: `.map(Number::intValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.int_value())),
        write: Arc::new(|ops, value| ops.create_int(*value)),
        name: "Int",
    })
}

/// `Codec.LONG`.
pub fn long_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i64, Ops>> {
    Arc::new(PrimitiveCodecImpl::<i64, Ops> {
        // Java `PrimitiveCodec<Long>`: `.map(Number::longValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.long_value())),
        write: Arc::new(|ops, value| ops.create_long(*value)),
        name: "Long",
    })
}

/// `Codec.FLOAT`.
pub fn float_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f32, Ops>> {
    Arc::new(PrimitiveCodecImpl::<f32, Ops> {
        // Java `PrimitiveCodec<Float>`: `.map(Number::floatValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.float_value())),
        write: Arc::new(|ops, value| ops.create_float(*value)),
        name: "Float",
    })
}

/// `Codec.DOUBLE`.
pub fn double_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f64, Ops>> {
    Arc::new(PrimitiveCodecImpl::<f64, Ops> {
        // Java `PrimitiveCodec<Double>`: `.map(Number::doubleValue)`.
        read: Arc::new(|ops, input| ops.get_number_value(input).map(|n| n.double_value())),
        write: Arc::new(|ops, value| ops.create_double(*value)),
        name: "Double",
    })
}

/// `Codec.STRING`.
pub fn string_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<String, Ops>> {
    Arc::new(PrimitiveCodecImpl::<String, Ops> {
        read: Arc::new(|ops, input| ops.get_string_value(input)),
        write: Arc::new(|ops, value| ops.create_string(value.clone())),
        name: "String",
    })
}

/// `Codec.BYTE_BUFFER`.
pub fn byte_buffer_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec<u8>, Ops>> {
    Arc::new(PrimitiveCodecImpl::<Vec<u8>, Ops> {
        read: Arc::new(|ops, input| ops.get_byte_buffer(input)),
        write: Arc::new(|ops, value| ops.create_byte_list(value)),
        name: "ByteBuffer",
    })
}

/// `Codec.INT_STREAM`.
pub fn int_stream_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec<i32>, Ops>> {
    Arc::new(PrimitiveCodecImpl::<Vec<i32>, Ops> {
        read: Arc::new(|ops, input| ops.get_int_stream(input)),
        write: Arc::new(|ops, value| ops.create_int_list(value.clone())),
        name: "IntStream",
    })
}

/// `Codec.LONG_STREAM`.
pub fn long_stream_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec<i64>, Ops>> {
    Arc::new(PrimitiveCodecImpl::<Vec<i64>, Ops> {
        read: Arc::new(|ops, input| ops.get_long_stream(input)),
        write: Arc::new(|ops, value| ops.create_long_list(value.clone())),
        name: "LongStream",
    })
}

// ---------------------------------------------------------------------------
// Range/validate/string helpers
// ---------------------------------------------------------------------------

/// `Codec.string(int minSize, int maxSize)`.
pub fn string_range<Ops: DynamicOps + 'static>(
    min_size: i32,
    max_size: i32,
) -> Arc<dyn Codec<String, Ops>> {
    validate(
        string_codec::<Ops>(),
        Arc::new(move |value: &String| {
            // Java `String.length()` counts UTF-16 code units; an astral-plane
            // char (e.g. an emoji) is 1 scalar value but 2 UTF-16 units.
            let length = value.encode_utf16().count() as i32;
            if length < min_size {
                return DataResult::error(format!(
                    "String \"{}\" is too short: {}, expected range [{}-{}]",
                    value, length, min_size, max_size
                ));
            }
            if length > max_size {
                return DataResult::error(format!(
                    "String \"{}\" is too long: {}, expected range [{}-{}]",
                    value, length, min_size, max_size
                ));
            }
            DataResult::success(value.clone())
        }),
    )
}

/// `Codec.sizeLimitedString(int maxSize)`.
pub fn size_limited_string<Ops: DynamicOps + 'static>(
    max_size: i32,
) -> Arc<dyn Codec<String, Ops>> {
    string_range(0, max_size)
}

/// `Codec.intRange(int, int)`.
pub fn int_range<Ops: DynamicOps + 'static>(
    min_inclusive: i32,
    max_inclusive: i32,
) -> Arc<dyn Codec<i32, Ops>> {
    flat_xmap(
        int_codec::<Ops>(),
        Arc::new(move |v: &i32| check_range(*v, min_inclusive, max_inclusive)),
        Arc::new(move |v: &i32| check_range(*v, min_inclusive, max_inclusive)),
    )
}

/// `Codec.floatRange(float, float)`.
pub fn float_range<Ops: DynamicOps + 'static>(
    min_inclusive: f32,
    max_inclusive: f32,
) -> Arc<dyn Codec<f32, Ops>> {
    flat_xmap(
        float_codec::<Ops>(),
        Arc::new(move |v: &f32| check_range_f32(*v, min_inclusive, max_inclusive)),
        Arc::new(move |v: &f32| check_range_f32(*v, min_inclusive, max_inclusive)),
    )
}

/// `Codec.doubleRange(double, double)`.
pub fn double_range<Ops: DynamicOps + 'static>(
    min_inclusive: f64,
    max_inclusive: f64,
) -> Arc<dyn Codec<f64, Ops>> {
    flat_xmap(
        double_codec::<Ops>(),
        Arc::new(move |v: &f64| check_range_f64(*v, min_inclusive, max_inclusive)),
        Arc::new(move |v: &f64| check_range_f64(*v, min_inclusive, max_inclusive)),
    )
}

/// `Codec.checkRange(N minInclusive, N maxInclusive)` — the private helper
/// (integer overloads). Integer `compareTo` and `PartialOrd` agree.
fn check_range<T: PartialOrd + std::fmt::Display>(value: T, min: T, max: T) -> DataResult<T> {
    if value >= min && value <= max {
        DataResult::success(value)
    } else {
        DataResult::error(format!(
            "Value {} outside of range [{}:{}]",
            value, min, max
        ))
    }
}

/// `Codec.checkRange(Float, Float)` — the f32 overload. Java's generic
/// `checkRange` calls `Comparable.compareTo`, which for `Float` is the IEEE
/// **total order** (`Float.compare`): `-0.0f < 0.0f`, `NaN` compares greater
/// than every value, and distinct NaN payloads compare equal (Java
/// canonicalizes to `0x7fc00000`). The port uses [`java_float_compare`], and
/// renders the message with Java's `Float.toString` (Rust `Display` prints
/// `NaN`/`-0.0` identically but `1.0` as `1`).
fn check_range_f32(value: f32, min: f32, max: f32) -> DataResult<f32> {
    let in_range = java_float_compare(value, min) != std::cmp::Ordering::Less
        && java_float_compare(value, max) != std::cmp::Ordering::Greater;
    if in_range {
        DataResult::success(value)
    } else {
        DataResult::error(format!(
            "Value {} outside of range [{}:{}]",
            java_float_to_string(value),
            java_float_to_string(min),
            java_float_to_string(max)
        ))
    }
}

/// `Codec.checkRange(Double, Double)` — the f64 overload (`Double.compare`).
fn check_range_f64(value: f64, min: f64, max: f64) -> DataResult<f64> {
    let in_range = java_double_compare(value, min) != std::cmp::Ordering::Less
        && java_double_compare(value, max) != std::cmp::Ordering::Greater;
    if in_range {
        DataResult::success(value)
    } else {
        DataResult::error(format!(
            "Value {} outside of range [{}:{}]",
            java_double_to_string(value),
            java_double_to_string(min),
            java_double_to_string(max)
        ))
    }
}

/// `Codec.stringResolver(Function<E, String>, Function<String, E>)`.
///
/// Java encodes via `Optional.ofNullable(toString.apply(e))` — only a `null`
/// return is an error, so the encode side is `Fn(&E) -> Option<String>`
/// (`None` = unknown element).
pub fn string_resolver<E, Ops: DynamicOps + 'static>(
    to_string: ToStringFn<E>,
    from_string: FromStringFn<E>,
) -> Arc<dyn Codec<E, Ops>>
where
    E: 'static + std::fmt::Display,
{
    let str_codec = string_codec::<Ops>();
    let f = from_string.clone();
    flat_xmap(
        str_codec,
        Arc::new(move |name: &String| match f(name) {
            Some(e) => DataResult::success(e),
            None => DataResult::error(format!("Unknown element name:{}", name)),
        }),
        Arc::new(move |e: &E| match to_string(e) {
            Some(s) => DataResult::success(s),
            None => DataResult::error(format!("Element with unknown name: {}", e)),
        }),
    )
}

// ---------------------------------------------------------------------------
// Combinators
// ---------------------------------------------------------------------------

/// `Codec.withLifecycle(Lifecycle)`.
pub fn with_lifecycle<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    lifecycle: Lifecycle,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    Arc::new(WithLifecycleCodec {
        lifecycle,
        inner: codec,
    })
}

/// `Codec.stable()`.
pub fn stable<A, Ops: DynamicOps + 'static>(codec: Arc<dyn Codec<A, Ops>>) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    with_lifecycle(codec, Lifecycle::stable())
}

/// `Codec.deprecated(int)`.
pub fn deprecated<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    since: i32,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    with_lifecycle(codec, Lifecycle::deprecated(since))
}

/// `Codec.xmap(Function, Function)`.
pub fn xmap<A, S, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    to: Arc<dyn Fn(&A) -> S + Send + Sync>,
    from: Arc<dyn Fn(&S) -> A + Send + Sync>,
) -> Arc<dyn Codec<S, Ops>>
where
    A: 'static,
    S: 'static,
{
    of(
        crate::encoder::comap(encoder_of_codec(codec.clone()), from),
        crate::decoder::map(decoder_of_codec(codec.clone()), to),
        format!("{:?}[xmapped]", codec),
    )
}

/// `Codec.comapFlatMap(Function, Function)`.
pub fn comap_flat_map<A, S, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    to: DecoderFn<A, S>,
    from: Arc<dyn Fn(&S) -> A + Send + Sync>,
) -> Arc<dyn Codec<S, Ops>>
where
    A: 'static,
    S: 'static,
{
    of(
        crate::encoder::comap(encoder_of_codec(codec.clone()), from),
        crate::decoder::flat_map(decoder_of_codec(codec.clone()), to),
        format!("{:?}[comapFlatMapped]", codec),
    )
}

/// `Codec.flatComapMap(Function, Function)`.
pub fn flat_comap_map<A, S, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    to: Arc<dyn Fn(&A) -> S + Send + Sync>,
    from: DecoderFn<S, A>,
) -> Arc<dyn Codec<S, Ops>>
where
    A: 'static,
    S: 'static,
{
    of(
        crate::encoder::flat_comap(encoder_of_codec(codec.clone()), from),
        crate::decoder::map(decoder_of_codec(codec.clone()), to),
        format!("{:?}[flatComapMapped]", codec),
    )
}

/// `Codec.flatXmap(Function, Function)`.
pub fn flat_xmap<A, S, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    to: DecoderFn<A, S>,
    from: DecoderFn<S, A>,
) -> Arc<dyn Codec<S, Ops>>
where
    A: 'static,
    S: 'static,
{
    of(
        crate::encoder::flat_comap(encoder_of_codec(codec.clone()), from),
        crate::decoder::flat_map(decoder_of_codec(codec.clone()), to),
        format!("{:?}[flatXmapped]", codec),
    )
}

/// `Codec.validate(Function<A, DataResult<A>>)`.
pub fn validate<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    checker: DecoderFn<A, A>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    flat_xmap(codec, checker.clone(), checker)
}

/// `Codec.withAlternative(Codec<? extends A>)`.
pub fn with_alternative<A, Ops: DynamicOps + 'static>(
    primary: Arc<dyn Codec<A, Ops>>,
    alternative: Arc<dyn Codec<A, Ops>>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync,
{
    xmap(
        either(primary, alternative),
        Arc::new(|e: &Either<A, A>| Either::<A, A>::unwrap_ref(e).clone()),
        Arc::new(|v: &A| Either::left(v.clone())),
    )
}

/// `Codec.mapResult(ResultFunction)`.
pub fn map_result<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    function: Arc<dyn ResultFunction<A, Ops>>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    Arc::new(MapResultCodec {
        function,
        inner: codec,
    })
}

/// `Codec.orElse(Consumer<String>, A)`.
pub fn or_else<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    on_error: Arc<dyn Fn(&str) + Send + Sync>,
    value: A,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync + Send + Sync,
{
    let v = value.clone();
    // `DataFixUtils.consumerToFunction`: invoke the callback and return the
    // message unchanged.
    or_else_get_map_error(
        codec,
        Arc::new(move |e: String| {
            on_error(&e);
            e
        }),
        Arc::new(move || v.clone()),
    )
}

/// `Codec.orElseGet(Consumer<String>, Supplier<A>)`.
pub fn or_else_get<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    on_error: Arc<dyn Fn(&str) + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync + Send + Sync,
{
    or_else_get_map_error(
        codec,
        Arc::new(move |e: String| {
            on_error(&e);
            e
        }),
        value,
    )
}

/// `Codec.orElseGet(UnaryOperator<String>, Supplier<A>)`.
pub fn or_else_get_map_error<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    on_error: Arc<dyn Fn(String) -> String + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync + Send + Sync,
{
    map_result(
        codec,
        Arc::new(OrElseResultFunctionCodec { on_error, value }),
    )
}

/// `Codec.orElse(A)`.
pub fn or_else_value<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    value: A,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync + Send + Sync,
{
    let v = value.clone();
    or_else_get_value(codec, Arc::new(move || v.clone()))
}

/// `Codec.orElseGet(Supplier<A>)`.
pub fn or_else_get_value<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static + Clone + Send + Sync + Send + Sync,
{
    map_result(codec, Arc::new(OrElseValueResultFunctionCodec { value }))
}

/// `Codec.promotePartial(Consumer<String>)`.
pub fn promote_partial<A, Ops: DynamicOps + 'static>(
    codec: Arc<dyn Codec<A, Ops>>,
    on_error: Arc<dyn Fn(&str) + Send + Sync>,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
{
    of(
        encoder_of_codec(codec.clone()),
        crate::decoder::promote_partial(decoder_of_codec(codec.clone()), on_error),
        format!("{:?}[promotePartial]", codec),
    )
}

// ---------------------------------------------------------------------------
// ResultFunction
// ---------------------------------------------------------------------------

/// `Codec.ResultFunction` — `apply`/`coApply`.
///
/// `Send + Sync` mirrors Paper: result functions are stored inside codecs that
/// are shared across netty threads.
pub trait ResultFunction<A, Ops: DynamicOps + 'static>: Debug + Send + Sync {
    fn apply(
        &self,
        ops: &Ops,
        input: &Ops::Output,
        a: DataResult<(A, Ops::Output)>,
    ) -> DataResult<(A, Ops::Output)>;

    fn co_apply(&self, ops: &Ops, input: &A, t: DataResult<Ops::Output>)
    -> DataResult<Ops::Output>;
}

/// `Codec.mapResult(ResultFunction)` result.
pub struct MapResultCodec<A, Ops: DynamicOps + 'static> {
    function: Arc<dyn ResultFunction<A, Ops>>,
    inner: Arc<dyn Codec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for MapResultCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapResultCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for MapResultCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let t = self.inner.encode(input, ops, prefix);
        self.function.co_apply(ops, input, t)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for MapResultCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        let a = self.inner.decode(ops, input);
        self.function.apply(ops, input, a)
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for MapResultCodec<A, Ops> {}

/// `Codec.orElseGet(UnaryOperator<String>, Supplier<A>)` result function.
pub struct OrElseResultFunctionCodec<A> {
    on_error: Arc<dyn Fn(String) -> String + Send + Sync>,
    value: Arc<dyn Fn() -> A + Send + Sync>,
}
impl<A> std::fmt::Debug for OrElseResultFunctionCodec<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrElseResultFunctionCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> ResultFunction<A, Ops> for OrElseResultFunctionCodec<A>
where
    A: Clone,
{
    fn apply(
        &self,
        _ops: &Ops,
        input: &Ops::Output,
        a: DataResult<(A, Ops::Output)>,
    ) -> DataResult<(A, Ops::Output)> {
        let mapped = a.map_error(move |e| (self.on_error)(e));
        match mapped.result() {
            Some(v) => DataResult::success(v.clone()),
            None => DataResult::success(((self.value)(), input.clone())),
        }
    }

    fn co_apply(
        &self,
        _ops: &Ops,
        _input: &A,
        t: DataResult<Ops::Output>,
    ) -> DataResult<Ops::Output> {
        let on_error = self.on_error.clone();
        t.map_error(move |e| on_error(e))
    }
}

/// `Codec.orElseGet(Supplier<A>)` result function.
pub struct OrElseValueResultFunctionCodec<A> {
    value: Arc<dyn Fn() -> A + Send + Sync>,
}
impl<A> std::fmt::Debug for OrElseValueResultFunctionCodec<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrElseValueResultFunctionCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> ResultFunction<A, Ops> for OrElseValueResultFunctionCodec<A>
where
    A: Clone,
{
    fn apply(
        &self,
        _ops: &Ops,
        input: &Ops::Output,
        a: DataResult<(A, Ops::Output)>,
    ) -> DataResult<(A, Ops::Output)> {
        match a.result() {
            Some(v) => DataResult::success(v.clone()),
            None => DataResult::success(((self.value)(), input.clone())),
        }
    }

    fn co_apply(
        &self,
        _ops: &Ops,
        _input: &A,
        t: DataResult<Ops::Output>,
    ) -> DataResult<Ops::Output> {
        t
    }
}

// ---------------------------------------------------------------------------
// Concrete codecs
// ---------------------------------------------------------------------------

/// `Codec.withLifecycle(Lifecycle)` result.
pub struct WithLifecycleCodec<A, Ops: DynamicOps + 'static> {
    lifecycle: Lifecycle,
    inner: Arc<dyn Codec<A, Ops>>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for WithLifecycleCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycleCodec")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for WithLifecycleCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.inner
            .encode(input, ops, prefix)
            .set_lifecycle(self.lifecycle)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for WithLifecycleCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.inner.decode(ops, input).set_lifecycle(self.lifecycle)
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for WithLifecycleCodec<A, Ops> {}

/// `Codec.of(Encoder, Decoder, String)` result.
pub struct OfCodec<A, Ops: DynamicOps + 'static> {
    encoder: Arc<dyn crate::Encoder<A, Ops>>,
    decoder: Arc<dyn crate::Decoder<A, Ops>>,
    name: Arc<dyn Fn() -> String + Send + Sync>,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for OfCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OfCodec[{}]", (self.name)())
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for OfCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.encoder.encode(input, ops, prefix)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for OfCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.decoder.decode(ops, input)
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for OfCodec<A, Ops> {}

/// `Codec.recursive(...)` result.
pub struct RecursiveCodec<A, Ops: DynamicOps + 'static> {
    name: String,
    wrapped: RecursiveFn<A, Ops>,
    cell: std::sync::OnceLock<Arc<dyn Codec<A, Ops>>>,
    weak: std::sync::Weak<RecursiveCodec<A, Ops>>,
}

impl<A, Ops: DynamicOps + 'static> Debug for RecursiveCodec<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecursiveCodec[{}]", self.name)
    }
}

impl<A: 'static, Ops: DynamicOps + 'static> RecursiveCodec<A, Ops> {
    fn get(&self) -> &Arc<dyn Codec<A, Ops>> {
        let parent = self
            .weak
            .upgrade()
            .expect("recursive codec's parent Arc must outlive the weak reference");
        self.cell
            .get_or_init(|| (self.wrapped)(Arc::new(RecursiveSelf { parent })))
    }
}

/// The self-referential codec handed to the recursive wrapper.
// Debug impl provided manually (deriving would add `A: Debug` bounds).
pub struct RecursiveSelf<A: 'static, Ops: DynamicOps + 'static> {
    parent: std::sync::Arc<RecursiveCodec<A, Ops>>,
}

impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for RecursiveSelf<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecursiveSelf")
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for RecursiveSelf<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.parent.get().encode(input, ops, prefix)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for RecursiveSelf<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.parent.get().decode(ops, input)
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for RecursiveSelf<A, Ops> {}

impl<A: 'static, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for RecursiveCodec<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.get().encode(input, ops, prefix)
    }
}

impl<A: 'static, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for RecursiveCodec<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        self.get().decode(ops, input)
    }
}

impl<A: 'static, Ops: DynamicOps + 'static> Codec<A, Ops> for RecursiveCodec<A, Ops> {}

/// `PrimitiveCodec<A>` — a codec with a `read`/`write` pair (the Java
/// interface default methods `decode`/`encode` are the shared impl).
pub struct PrimitiveCodecImpl<A, Ops: DynamicOps + 'static> {
    read: PrimitiveReadFn<A, Ops>,
    write: PrimitiveWriteFn<A, Ops>,
    name: &'static str,
}
impl<A, Ops: DynamicOps + 'static> std::fmt::Debug for PrimitiveCodecImpl<A, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Java `PrimitiveCodec.toString()` returns the name ("Bool", "Int", ...).
        write!(f, "{}", self.name)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Encoder<A, Ops> for PrimitiveCodecImpl<A, Ops> {
    fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        let written = (self.write)(ops, input);
        ops.merge_to_primitive(prefix, written)
    }
}

impl<A, Ops: DynamicOps + 'static> crate::Decoder<A, Ops> for PrimitiveCodecImpl<A, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
        // Java `PrimitiveCodec.decode`: `read(ops, input).map(r -> Pair.of(r,
        // ops.empty()))`.
        (self.read)(ops, input).map_owned(|r| (r, ops.empty()))
    }
}

impl<A, Ops: DynamicOps + 'static> Codec<A, Ops> for PrimitiveCodecImpl<A, Ops> {}

/// `Codec.PASSTHROUGH` — the value element is `Ops::Output` (Java's
/// `Dynamic<?>` is bound to the ops type at use).
pub struct PassthroughCodec<Ops: DynamicOps> {
    _ops: std::marker::PhantomData<Ops>,
}
impl<Ops: DynamicOps> std::fmt::Debug for PassthroughCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PassthroughCodec")
    }
}

impl<Ops: DynamicOps + 'static> crate::Decoder<crate::dynamic::Dynamic<Ops::Output>, Ops>
    for PassthroughCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &Ops::Output,
    ) -> DataResult<(crate::dynamic::Dynamic<Ops::Output>, Ops::Output)> {
        // Java: `Pair.of(new Dynamic<>(ops, input), ops.empty())`.
        DataResult::success((
            crate::dynamic::Dynamic {
                _ops: std::marker::PhantomData,
                value: input.clone(),
            },
            ops.empty(),
        ))
    }
}

impl<Ops: DynamicOps + 'static> crate::Encoder<crate::dynamic::Dynamic<Ops::Output>, Ops>
    for PassthroughCodec<Ops>
{
    fn encode(
        &self,
        input: &crate::dynamic::Dynamic<Ops::Output>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let value: &Ops::Output = &input.value;
        // Java uses reference identity (`input.getValue() == input.getOps().empty()`);
        // `Ops::Output` is value-semantic here, so this is a deliberate switch
        // to value equality (a structurally-equal empty is treated as empty).
        if value == &ops.empty() {
            // nothing to merge, return rest
            return DataResult::success_with_lifecycle(prefix.clone(), Lifecycle::experimental());
        }
        // Java: `input.convert(ops).getValue()`; the value is already in the
        // target ops (no stored ops to convert from), so `casted = value`.
        let casted = value.clone();
        if prefix == &ops.empty() {
            // no need to merge anything, return the old value
            return DataResult::success_with_lifecycle(casted, Lifecycle::experimental());
        }
        let to_map = ops
            .get_map(&casted)
            .flat_map(|m| ops.merge_to_map_like(prefix, m.as_ref()));
        match to_map.result() {
            Some(v) => DataResult::success_with_lifecycle(v.clone(), Lifecycle::experimental()),
            None => {
                let to_list = ops
                    .get_stream(&casted)
                    .flat_map(|s| ops.merge_to_list_many(prefix, s));
                match to_list.result() {
                    Some(v) => {
                        DataResult::success_with_lifecycle(v.clone(), Lifecycle::experimental())
                    }
                    None => DataResult::error_with_partial_lifecycle(
                        format!(
                            "Don't know how to merge {} and {}",
                            debug_str(prefix.clone()),
                            debug_str(casted.clone())
                        ),
                        Some(prefix.clone()),
                        Lifecycle::experimental(),
                    ),
                }
            }
        }
    }
}

impl<Ops: DynamicOps + 'static> Codec<crate::dynamic::Dynamic<Ops::Output>, Ops>
    for PassthroughCodec<Ops>
{
}

/// Debug formatting of an ops value for `PASSTHROUGH` error messages.
fn debug_str<T: Debug>(value: T) -> String {
    format!("{:?}", value)
}
