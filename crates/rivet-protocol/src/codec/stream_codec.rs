//! Port of `net.minecraft.network.codec.StreamCodec`.
//!
//! Java: `StreamCodec.java` in `working/Paper` (vanilla 26.2). `StreamCodec<B,
//! V>` is an interface (a shared reference) extending `StreamEncoder<B, V>` +
//! `StreamDecoder<B, V>`. Rust has no multi-trait `dyn` (E0225), so the port
//! splits the Java surface into two object-safe traits ([`StreamDecoder`],
//! [`StreamEncoder`]) plus a combined marker [`StreamCodecDyn`] that carries
//! `Send + Sync`; the erased codec value [`StreamCodec`] is a `Clone` struct
//! owning an `Arc<dyn StreamCodecDyn<B, V>>`. This mirrors the
//! `Arc<dyn Codec<A, Ops>>` precedent in `rivet-serialization::codec` and makes
//! every constructor ([`of`], [`of_member`], [`unit`], [`composite_1`]..[`composite_12`],
//! [`recursive`], plus the free combinators [`map`]/[`dispatch`]/[`apply`]) return
//! the same value type with no "codec-or-arc" ambiguity.
//!
//! Java's instance default methods that are generic in their argument/result
//! (`map`, `dispatch`, `apply`) are not object-safe on a `dyn`, so they are free
//! functions — the standard way to port a default method that must not live on
//! the vtable.
//!
//! Error model: every netty `DecoderException`/`EncoderException` thrown by a
//! codec maps to [`CodecError`] (see [`StreamDecoder`]). One deliberate
//! exception: [`unit`]'s `IllegalStateException` on a mismatched encode is a
//! programmer error (a statically-mismatched instance) and `panic!`s with Java's
//! exact message. `DontDecorateException` has no port: `IdDispatchCodec`
//! decorates by construction (see its module), so the catch-and-rethrow marker
//! disappears.

use crate::codec::stream_decoder::StreamDecoder;
use crate::codec::stream_encoder::StreamEncoder;
use crate::codec::stream_member_encoder::StreamMemberEncoder;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::OnceLock;

/// The erased `StreamCodec.of` encoder — `(output, value)`.
type EncodeFn<B, V> = Box<dyn Fn(&mut B, &V) -> Result<(), CodecError> + Send + Sync>;
/// The erased `StreamCodec.of` decoder — `(input) -> value`.
type DecodeFn<B, V> = Box<dyn Fn(&mut B) -> Result<V, CodecError> + Send + Sync>;
/// The shared `StreamCodec.dispatch` payload-codec selector — `(key) -> codec`.
type DispatchCodecFn<B, V, U> = Arc<dyn Fn(&V) -> StreamCodec<B, U> + Send + Sync>;
/// The erased `StreamCodec.CodecOperation` — `apply(StreamCodec)`.
type OperationFn<B, S, T> = Box<dyn Fn(StreamCodec<B, S>) -> StreamCodec<B, T> + Send + Sync>;
/// The erased `StreamCodec.recursive` factory — `(self-handle) -> codec`.
type RecursiveFactoryFn<B, V> = Box<dyn Fn(&StreamCodec<B, V>) -> StreamCodec<B, V> + Send + Sync>;

/// The checked error returned by [`StreamDecoder::decode`]/[`StreamEncoder::encode`].
///
/// Netty's `DecoderException`/`EncoderException` are `RuntimeException`s that
/// Paper does not catch per-codec: they surface at the frame boundary and kick
/// the connection. Per PORTING.md's checked-exception rule they map to
/// `Result<_, CodecError>` here so the frame boundary can decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    pub message: String,
}

impl CodecError {
    /// Constructs a `CodecError` from the netty exception message.
    pub fn new(message: impl Into<String>) -> Self {
        CodecError {
            message: message.into(),
        }
    }
}

impl Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodecError {}

/// Combined marker: `StreamEncoder<B, V> + StreamDecoder<B, V> + Send + Sync`.
///
/// `Send + Sync` is required because codecs are built on the tick thread and
/// shipped to a connection thread. A single marker trait makes
/// `Arc<dyn StreamCodecDyn<B, V>>` a legal trait object (Rust forbids
/// multi-trait `dyn`, E0225).
pub trait StreamCodecDyn<B, V>: StreamEncoder<B, V> + StreamDecoder<B, V> + Send + Sync {}

/// The erased codec value: `StreamCodec<B, V>` implements the two halves by
/// forwarding to its inner `Arc<dyn StreamCodecDyn<B, V>>`.
///
/// Java's `StreamCodec` is an interface (a reference), and the erased reference
/// *is* the `Arc<dyn>`; making the Rust `StreamCodec` a concrete value struct
/// that implements the two halves is the 1:1 port. `Clone` is cheap (one `Arc`
/// bump), so a codec can be captured and shipped onto a connection thread.
///
/// `Clone` is implemented manually (not derived) because the derived impl would
/// impose `B: Clone`/`V: Clone` on every use; the inner `Arc` is cloneable for
/// any `B`, `V` satisfying the trait object's own `Send + Sync` supertraits.
pub struct StreamCodec<B: 'static, V: 'static>(Arc<dyn StreamCodecDyn<B, V>>);

impl<B: 'static, V: 'static> Clone for StreamCodec<B, V> {
    fn clone(&self) -> Self {
        StreamCodec(self.0.clone())
    }
}

impl<B: 'static, V: 'static> StreamCodec<B, V> {
    /// Wraps a concrete [`StreamCodecDyn`] implementation, e.g. an
    /// [`IdDispatchCodec`](crate::codec::IdDispatchCodec) returned by its
    /// builder.
    pub fn new<C: StreamCodecDyn<B, V> + 'static>(codec: C) -> Self {
        StreamCodec(Arc::new(codec))
    }
}

impl<B: 'static, V: 'static> StreamDecoder<B, V> for StreamCodec<B, V> {
    fn decode(&self, input: &mut B) -> Result<V, CodecError> {
        self.0.decode(input)
    }
}

impl<B: 'static, V: 'static> StreamEncoder<B, V> for StreamCodec<B, V> {
    fn encode(&self, output: &mut B, value: &V) -> Result<(), CodecError> {
        self.0.encode(output, value)
    }
}

impl<B: 'static, V: 'static> StreamCodecDyn<B, V> for StreamCodec<B, V> {}

/// A codec built from two plain closures — `StreamCodec.of(encoder, decoder)`.
struct OfCodec<B, V> {
    encoder: EncodeFn<B, V>,
    decoder: DecodeFn<B, V>,
}

impl<B, V> StreamDecoder<B, V> for OfCodec<B, V> {
    fn decode(&self, input: &mut B) -> Result<V, CodecError> {
        (self.decoder)(input)
    }
}

impl<B, V> StreamEncoder<B, V> for OfCodec<B, V> {
    fn encode(&self, output: &mut B, value: &V) -> Result<(), CodecError> {
        (self.encoder)(output, value)
    }
}

impl<B, V> StreamCodecDyn<B, V> for OfCodec<B, V> {}

/// `StreamCodec.of(StreamEncoder, StreamDecoder)`.
pub fn of<B: 'static, V: 'static>(
    encoder: impl Fn(&mut B, &V) -> Result<(), CodecError> + Send + Sync + 'static,
    decoder: impl Fn(&mut B) -> Result<V, CodecError> + Send + Sync + 'static,
) -> StreamCodec<B, V> {
    StreamCodec::new(OfCodec {
        encoder: Box::new(encoder),
        decoder: Box::new(decoder),
    })
}

/// `StreamCodec.ofMember(StreamMemberEncoder, StreamDecoder)` — the encoder
/// takes `(value, output)`. `Packet.codec(writer, reader)` is exactly this.
///
/// A `(value, output)` closure implements [`StreamMemberEncoder`] via the
/// blanket impl, so call sites keep passing plain closures exactly like Java's
/// lambdas.
pub fn of_member<B: 'static, V: 'static>(
    encoder: impl StreamMemberEncoder<B, V> + Send + Sync + 'static,
    decoder: impl Fn(&mut B) -> Result<V, CodecError> + Send + Sync + 'static,
) -> StreamCodec<B, V> {
    of(
        move |output: &mut B, value: &V| encoder.encode(value, output),
        decoder,
    )
}

/// `Packet.codec(writer, reader)` — `StreamCodec.ofMember(writer, reader)`
/// (`working/Paper/.../network/protocol/Packet.java:35-36`).
pub fn codec<B: 'static, T: 'static>(
    writer: impl Fn(&T, &mut B) -> Result<(), CodecError> + Send + Sync + 'static,
    reader: impl Fn(&mut B) -> Result<T, CodecError> + Send + Sync + 'static,
) -> StreamCodec<B, T> {
    of_member(writer, reader)
}

/// `StreamCodec.unit(V instance)` — decodes the instance (encoding nothing),
/// and panics with Java's `IllegalStateException` message on a mismatched
/// encode (a programmer error, not a wire error).
pub fn unit<B: 'static, V>(instance: V) -> StreamCodec<B, V>
where
    V: Clone + PartialEq + Display + Send + Sync + 'static,
{
    let decoder_instance = instance.clone();
    of(
        move |_output: &mut B, value: &V| {
            if *value != instance {
                panic!("Can't encode '{value}', expected '{instance}'");
            }
            Ok(())
        },
        move |_input: &mut B| Ok(decoder_instance.clone()),
    )
}

/// `StreamCodec.map(Function<? super V, ? extends O> to, Function<? super O,
/// ? extends V> from)` — `to` runs on decode, `from` on encode.
pub fn map<B: 'static, V: 'static, O: 'static>(
    codec: StreamCodec<B, V>,
    to: impl Fn(&V) -> O + Send + Sync + 'static,
    from: impl Fn(&O) -> V + Send + Sync + 'static,
) -> StreamCodec<B, O> {
    let encoder_codec = codec.clone();
    of(
        move |output: &mut B, value: &O| {
            let inner = from(value);
            encoder_codec.encode(output, &inner)
        },
        move |input: &mut B| {
            let inner = codec.decode(input)?;
            Ok(to(&inner))
        },
    )
}

/// `StreamCodec.dispatch(Function<? super U, ? extends V> type, Function<?
/// super V, ? extends StreamCodec<...>> codec)` — decode reads the key via the
/// inner codec and delegates to `codec(key)`; encode writes the key first, then
/// the payload (Java evaluation order).
pub fn dispatch<B: 'static, V: 'static, U: 'static>(
    inner: StreamCodec<B, V>,
    type_getter: impl Fn(&U) -> V + Send + Sync + 'static,
    codec_fn: impl Fn(&V) -> StreamCodec<B, U> + Send + Sync + 'static,
) -> StreamCodec<B, U> {
    // `codec_fn` runs on both halves, so it is shared by `Arc` (a plain closure
    // cannot be copied into two `move` closures).
    let codec_fn: DispatchCodecFn<B, V, U> = Arc::new(codec_fn);
    let decoder_inner = inner.clone();
    let decoder_codec_fn = codec_fn.clone();
    of(
        move |output: &mut B, value: &U| {
            let key = type_getter(value);
            let value_codec = codec_fn(&key);
            inner.encode(output, &key)?;
            value_codec.encode(output, value)
        },
        move |input: &mut B| {
            let key = decoder_inner.decode(input)?;
            let value_codec = decoder_codec_fn(&key);
            value_codec.decode(input)
        },
    )
}

/// `StreamCodec.apply(CodecOperation)` — the default method is generic in its
/// result, so it is not object-safe on the `dyn` and becomes a free function.
pub fn apply<B: 'static, S: 'static, T: 'static>(
    codec: StreamCodec<B, S>,
    operation: CodecOperation<B, S, T>,
) -> StreamCodec<B, T> {
    (operation.0)(codec)
}

/// `StreamCodec.CodecOperation<B, S, T>` — `StreamCodec<B, T> apply(StreamCodec<B, S>)`.
pub struct CodecOperation<B: 'static, S: 'static, T: 'static>(OperationFn<B, S, T>);

impl<B: 'static, S: 'static, T: 'static> CodecOperation<B, S, T> {
    /// Wraps an operation closure.
    pub fn new(
        operation: impl Fn(StreamCodec<B, S>) -> StreamCodec<B, T> + Send + Sync + 'static,
    ) -> Self {
        CodecOperation(Box::new(operation))
    }
}

/// `StreamCodec.recursive(UnaryOperator<StreamCodec<B, T>> factory)`.
///
/// The factory result is memoized lazily (Java `Suppliers.memoize`): the first
/// decode/encode runs the factory with the self-handle already in place, and
/// every later call reuses the cached inner codec. The self-handle is a
/// `StreamCodec<B, V>` value, so a composite can freely embed it.
///
/// Cycle cost (deliberate, and the cost is permanent): the factory result is a
/// codec that embeds the self-handle, so `RecursiveCodec` -> inner ->
/// self-handle -> `Arc` -> `RecursiveCodec` is a strong `Arc` cycle. `Arc`
/// cannot collect cycles the way Java's GC collects the anonymous-class `this`
/// capture, so the whole graph (including the captured `factory` closure) is
/// never freed. This is bounded only by the number of distinct `recursive`
/// codec graphs ever built: `recursive` is a registration-time constructor
/// (like Java's `static final` codec fields) and must never be called per
/// connection, or the graph accumulates once per connection. Build it once per
/// process and reuse the returned `StreamCodec`.
pub fn recursive<B: 'static, V: 'static>(
    factory: impl Fn(&StreamCodec<B, V>) -> StreamCodec<B, V> + Send + Sync + 'static,
) -> StreamCodec<B, V> {
    let codec = Arc::new(RecursiveCodec {
        self_handle: OnceLock::new(),
        inner: OnceLock::new(),
        factory: Box::new(factory),
    });
    let handle = StreamCodec(codec.clone());
    if codec.self_handle.set(handle).is_err() {
        // Unreachable: the handle is set once here, before `recursive` returns.
        panic!("recursive self-handle set twice");
    }
    StreamCodec(codec)
}

/// The inner `recursive` codec: a lazy factory cache plus the stored
/// self-handle.
struct RecursiveCodec<B: 'static, V: 'static> {
    self_handle: OnceLock<StreamCodec<B, V>>,
    inner: OnceLock<StreamCodec<B, V>>,
    factory: RecursiveFactoryFn<B, V>,
}

impl<B: 'static, V: 'static> RecursiveCodec<B, V> {
    /// `Suppliers.memoize(() -> factory.apply(this))` — runs the factory once,
    /// caching the result, with the self-handle guaranteed set first.
    fn inner(&self) -> &StreamCodec<B, V> {
        self.inner.get_or_init(|| {
            let handle = self
                .self_handle
                .get()
                .expect("recursive self-handle set before first use")
                .clone();
            (self.factory)(&handle)
        })
    }
}

impl<B: 'static, V: 'static> StreamDecoder<B, V> for RecursiveCodec<B, V> {
    fn decode(&self, input: &mut B) -> Result<V, CodecError> {
        self.inner().decode(input)
    }
}

impl<B: 'static, V: 'static> StreamEncoder<B, V> for RecursiveCodec<B, V> {
    fn encode(&self, output: &mut B, value: &V) -> Result<(), CodecError> {
        self.inner().encode(output, value)
    }
}

impl<B: 'static, V: 'static> StreamCodecDyn<B, V> for RecursiveCodec<B, V> {}

/// Defines one `StreamCodec.composite` arity: a struct holding the field codecs
/// and getters plus the constructor, the two trait impls, and the
/// `composite_N` constructor function.
///
/// Each invocation has disjoint generic params (`T1..Tn`, `G1..Gn`), so there is
/// no "parameter type constrained by multiple impl paths" clash across arities.
/// Field names (`codec1..codec12`) and getter names (`getter1..getter12`) are
/// hard-coded per arity via the tuple literals. Decode binds each field decode
/// to the explicitly-named `$v` (`v1..v12`) and calls the constructor
/// positionally; encode calls the getters in order (Java evaluation order).
macro_rules! define_composite {
    ($s:ident, $fn_name:ident; $(($field:ident, $t:ident, $g:ident, $getter:ident, $v:ident),)+) => {
        /// A `StreamCodec.composite` struct: `n` field codecs + getters, then the
        /// constructor. See the Java overloads.
        pub struct $s<B: 'static, C: 'static, $($t: 'static, $g,)+ F> {
            $( $field: StreamCodec<B, $t>, )+
            $( $getter: $g, )+
            constructor: F,
            // `fn(C)` so the value type `C` does not force `Send`/`Sync` on the struct.
            marker: PhantomData<fn(C)>,
        }

        impl<B: 'static, C: 'static, $($t: 'static, $g,)+ F> StreamDecoder<B, C> for $s<B, C, $($t, $g,)+ F>
        where
            $($g: Fn(&C) -> $t + Send + Sync + 'static,)+
            F: Fn($($t,)+) -> C + Send + Sync + 'static,
        {
            fn decode(&self, input: &mut B) -> Result<C, CodecError> {
                $( let $v = self.$field.decode(input)?; )+
                Ok((self.constructor)($( $v, )+))
            }
        }

        impl<B: 'static, C: 'static, $($t: 'static, $g,)+ F> StreamEncoder<B, C> for $s<B, C, $($t, $g,)+ F>
        where
            $($g: Fn(&C) -> $t + Send + Sync + 'static,)+
            F: Fn($($t,)+) -> C + Send + Sync + 'static,
        {
            fn encode(&self, output: &mut B, value: &C) -> Result<(), CodecError> {
                $( self.$field.encode(output, &(self.$getter)(value))?; )+
                Ok(())
            }
        }

        impl<B: 'static, C: 'static, $($t: 'static, $g,)+ F> StreamCodecDyn<B, C> for $s<B, C, $($t, $g,)+ F>
        where
            $($g: Fn(&C) -> $t + Send + Sync + 'static,)+
            F: Fn($($t,)+) -> C + Send + Sync + 'static,
        {}

        /// `StreamCodec.composite` — decode reads each field in order then calls
        /// the constructor; encode calls each getter in order and encodes the
        /// fields (Java evaluation order).
        #[allow(clippy::too_many_arguments)]
        pub fn $fn_name<B: 'static, C: 'static, $($t: 'static, $g,)+ F>(
            $( $field: StreamCodec<B, $t>, $getter: $g, )+
            constructor: F,
        ) -> StreamCodec<B, C>
        where
            $($g: Fn(&C) -> $t + Send + Sync + 'static,)+
            F: Fn($($t,)+) -> C + Send + Sync + 'static,
        {
            StreamCodec::new($s {
                $( $field, $getter, )+
                constructor,
                marker: PhantomData,
            })
        }
    };
}

define_composite!(Composite1, composite_1; (codec1, T1, G1, getter1, v1),);
define_composite!(Composite2, composite_2; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2),);
define_composite!(Composite3, composite_3; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3),);
define_composite!(Composite4, composite_4; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4),);
define_composite!(Composite5, composite_5; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5),);
define_composite!(Composite6, composite_6; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6),);
define_composite!(Composite7, composite_7; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7),);
define_composite!(Composite8, composite_8; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7), (codec8, T8, G8, getter8, v8),);
define_composite!(Composite9, composite_9; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7), (codec8, T8, G8, getter8, v8), (codec9, T9, G9, getter9, v9),);
define_composite!(Composite10, composite_10; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7), (codec8, T8, G8, getter8, v8), (codec9, T9, G9, getter9, v9), (codec10, T10, G10, getter10, v10),);
define_composite!(Composite11, composite_11; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7), (codec8, T8, G8, getter8, v8), (codec9, T9, G9, getter9, v9), (codec10, T10, G10, getter10, v10), (codec11, T11, G11, getter11, v11),);
define_composite!(Composite12, composite_12; (codec1, T1, G1, getter1, v1), (codec2, T2, G2, getter2, v2), (codec3, T3, G3, getter3, v3), (codec4, T4, G4, getter4, v4), (codec5, T5, G5, getter5, v5), (codec6, T6, G6, getter6, v6), (codec7, T7, G7, getter7, v7), (codec8, T8, G8, getter8, v8), (codec9, T9, G9, getter9, v9), (codec10, T10, G10, getter10, v10), (codec11, T11, G11, getter11, v11), (codec12, T12, G12, getter12, v12),);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::byte_buf_codecs;
    use crate::friendly_byte_buf::FriendlyByteBuf;
    use bytes::BytesMut;
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    /// The bytes written since construction (the reader index starts at 0).
    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    /// A 4-byte big-endian codec, like `ByteBufCodecs.INT`.
    fn int_be() -> StreamCodec<FriendlyByteBuf, i32> {
        of(
            |output: &mut FriendlyByteBuf, value: &i32| {
                output.write_int(*value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_int()),
        )
    }

    // ---- of / of_member / codec -----------------------------------------

    #[test]
    fn of_round_trips_and_encodes_big_endian() {
        let codec = of(
            |output: &mut FriendlyByteBuf, value: &i32| {
                output.write_int(*value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_int()),
        );
        let mut out = buf();
        codec.encode(&mut out, &1234).unwrap();
        assert_eq!(written(out), 1234i32.to_be_bytes().to_vec());

        let mut input = FriendlyByteBuf::new(BytesMut::from(1234i32.to_be_bytes().as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), 1234);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn of_member_flips_argument_order() {
        // `of_member`'s encoder is `(value, output)`, like `StreamMemberEncoder`.
        let codec = of_member(
            |value: &i32, output: &mut FriendlyByteBuf| {
                output.write_int(*value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_int()),
        );
        let mut out = buf();
        codec.encode(&mut out, &7).unwrap();
        assert_eq!(written(out), 7i32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(7i32.to_be_bytes().as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), 7);
    }

    #[test]
    fn packet_codec_is_of_member() {
        let codec = codec(
            |value: &String, output: &mut FriendlyByteBuf| {
                output.write_utf(value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_utf()),
        );
        let mut out = buf();
        codec.encode(&mut out, &"hi".to_string()).unwrap();
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), "hi");
    }

    // ---- unit --------------------------------------------------------------

    #[test]
    fn unit_encodes_nothing_and_decodes_instance() {
        let codec: StreamCodec<FriendlyByteBuf, &'static str> = unit("hello");
        let mut out = buf();
        codec.encode(&mut out, &"hello").unwrap();
        assert!(written(out).is_empty());

        let mut input = buf();
        assert_eq!(codec.decode(&mut input).unwrap(), "hello");
    }

    #[test]
    fn unit_mismatched_encode_panics_with_java_message() {
        let codec: StreamCodec<FriendlyByteBuf, &'static str> = unit("hello");
        let mut out = buf();
        let msg = panic_message(|| {
            let _ = codec.encode(&mut out, &"world");
        });
        assert_eq!(msg, "Can't encode 'world', expected 'hello'");
    }

    // ---- map ----------------------------------------------------------------

    #[test]
    fn map_runs_to_on_decode_and_from_on_encode() {
        let doubled = map(int_be(), |v: &i32| v * 2, |v: &i32| v / 2);
        let mut out = buf();
        doubled.encode(&mut out, &6).unwrap(); // from(6) = 3 -> writes 3 BE
        assert_eq!(written(out), 3i32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(21i32.to_be_bytes().as_slice()));
        assert_eq!(doubled.decode(&mut input).unwrap(), 42); // to(21) = 42
    }

    // ---- dispatch -----------------------------------------------------------

    #[test]
    fn dispatch_selects_payload_codec_by_decoded_key() {
        // Key: bool. True -> int codec (value "1"), false -> string codec.
        let dispatcher: StreamCodec<FriendlyByteBuf, String> = dispatch(
            bool_codec(),
            |value: &String| value == "1",
            |key: &bool| {
                if *key {
                    map(int_be(), |_v: &i32| "1".to_string(), |_v: &String| 1)
                } else {
                    of(
                        |output: &mut FriendlyByteBuf, value: &String| {
                            output.write_utf(value);
                            Ok(())
                        },
                        |input: &mut FriendlyByteBuf| Ok(input.read_utf()),
                    )
                }
            },
        );
        // Encode "1": key byte `true` first, then the int payload 1 BE.
        let mut out = buf();
        dispatcher.encode(&mut out, &"1".to_string()).unwrap();
        let bytes = written(out);
        assert_eq!(bytes, vec![1u8, 0, 0, 0, 1]); // true + 1 BE
        // Decode the same bytes -> looks up by decoded key -> int path.
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(dispatcher.decode(&mut input).unwrap(), "1");
    }

    fn bool_codec() -> StreamCodec<FriendlyByteBuf, bool> {
        of(
            |output: &mut FriendlyByteBuf, value: &bool| {
                output.write_boolean(*value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_boolean()),
        )
    }

    // ---- composite ----------------------------------------------------------

    #[derive(Debug, PartialEq)]
    struct Pair {
        x: i32,
        y: i32,
    }

    #[test]
    fn composite_2_round_trips_field_order() {
        let codec = composite_2(
            int_be(),
            |p: &Pair| p.x,
            int_be(),
            |p: &Pair| p.y,
            |x, y| Pair { x, y },
        );
        let mut out = buf();
        codec.encode(&mut out, &Pair { x: 1, y: 2 }).unwrap();
        let bytes = written(out);
        // x (4 bytes) then y (4 bytes), both big-endian.
        assert_eq!(bytes, [0, 0, 0, 1, 0, 0, 0, 2]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), Pair { x: 1, y: 2 });
    }

    #[test]
    fn composite_1_round_trips() {
        #[derive(Debug, PartialEq)]
        struct Single(i32);
        let codec = composite_1(int_be(), |s: &Single| s.0, Single);
        let mut out = buf();
        codec.encode(&mut out, &Single(42)).unwrap();
        let bytes = written(out);
        assert_eq!(bytes, 42i32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), Single(42));
    }

    #[test]
    fn composite_3_round_trips_field_order() {
        // Smoke test through a higher arity to guard the macro's parameter
        // expansion beyond the two arities directly exercised elsewhere.
        #[derive(Debug, PartialEq)]
        struct Triple {
            a: i32,
            b: bool,
            c: String,
        }
        let str_codec = of(
            |output: &mut FriendlyByteBuf, value: &String| {
                output.write_utf(value);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(input.read_utf()),
        );
        let codec = composite_3(
            int_be(),
            |t: &Triple| t.a,
            bool_codec(),
            |t: &Triple| t.b,
            str_codec,
            |t: &Triple| t.c.clone(),
            |a, b, c| Triple { a, b, c },
        );
        let value = Triple {
            a: -7,
            b: true,
            c: "héllo".to_string(),
        };
        let mut out = buf();
        codec.encode(&mut out, &value).unwrap();
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), value);
    }

    /// One round-trip + exact-layout smoke test per `composite_N` arity: N
    /// big-endian int fields, getters in declaration order, constructor
    /// positionally. Guards the macro's parameter expansion for every arity the
    /// `define_composite!` block emits (arity 1-3 have dedicated tests above).
    macro_rules! composite_arity_smoke {
        ($name:ident, $fn_name:ident, $(($field:ident, $idx:expr)),+) => {
            #[test]
            fn $name() {
                #[derive(Debug, PartialEq)]
                struct S { $( $field: i32, )+ }
                let codec = $fn_name(
                    $( int_be(), |s: &S| s.$field, )+
                    |$( $field, )+| S { $( $field, )+ },
                );
                let value = S { $( $field: $idx, )+ };
                let mut out = buf();
                codec.encode(&mut out, &value).unwrap();
                let bytes = written(out);
                let mut expected = Vec::new();
                $( expected.extend_from_slice(&($idx as i32).to_be_bytes()); )+
                assert_eq!(bytes, expected);
                let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
                assert_eq!(codec.decode(&mut input).unwrap(), value);
            }
        };
    }

    composite_arity_smoke!(
        composite_4_round_trips,
        composite_4,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4)
    );
    composite_arity_smoke!(
        composite_5_round_trips,
        composite_5,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5)
    );
    composite_arity_smoke!(
        composite_6_round_trips,
        composite_6,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6)
    );
    composite_arity_smoke!(
        composite_7_round_trips,
        composite_7,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7)
    );
    composite_arity_smoke!(
        composite_8_round_trips,
        composite_8,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7),
        (f8, 8)
    );
    composite_arity_smoke!(
        composite_9_round_trips,
        composite_9,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7),
        (f8, 8),
        (f9, 9)
    );
    composite_arity_smoke!(
        composite_10_round_trips,
        composite_10,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7),
        (f8, 8),
        (f9, 9),
        (f10, 10)
    );
    composite_arity_smoke!(
        composite_11_round_trips,
        composite_11,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7),
        (f8, 8),
        (f9, 9),
        (f10, 10),
        (f11, 11)
    );
    composite_arity_smoke!(
        composite_12_round_trips,
        composite_12,
        (f1, 1),
        (f2, 2),
        (f3, 3),
        (f4, 4),
        (f5, 5),
        (f6, 6),
        (f7, 7),
        (f8, 8),
        (f9, 9),
        (f10, 10),
        (f11, 11),
        (f12, 12)
    );

    // ---- recursive ----------------------------------------------------------

    /// A recursive `Node` tree: value (int BE) + children (varint count, then
    /// recursively-encoded nodes). Built with `recursive` + `composite_2` +
    /// `ByteBufCodecs.collection`. Exercises Java's memoized factory: the
    /// factory runs exactly once and the recursion handle is the value it
    /// returns.
    #[derive(Debug, Clone, PartialEq)]
    struct Node {
        value: i32,
        children: Vec<Node>,
    }

    #[test]
    fn recursive_builds_self_referential_tree_codec() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_outer = Arc::clone(&runs);
        let tree_codec: StreamCodec<FriendlyByteBuf, Node> = recursive(move |self_handle| {
            runs_outer.fetch_add(1, Ordering::SeqCst);
            let children = byte_buf_codecs::collection(
                |capacity: i32| {
                    if capacity < 0 {
                        panic!("Illegal Capacity: {capacity}");
                    }
                    Vec::with_capacity(capacity as usize)
                },
                self_handle.clone(),
                i32::MAX,
            );
            composite_2(
                int_be(),
                |n: &Node| n.value,
                children,
                |n: &Node| n.children.clone(),
                |value, children| Node { value, children },
            )
        });

        let root = Node {
            value: 1,
            children: vec![
                Node {
                    value: 2,
                    children: vec![],
                },
                Node {
                    value: 3,
                    children: vec![Node {
                        value: 4,
                        children: vec![],
                    }],
                },
            ],
        };
        let mut out = buf();
        tree_codec.encode(&mut out, &root).unwrap();
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "factory runs once (memoized)"
        );
        let bytes = written(out);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(tree_codec.decode(&mut input).unwrap(), root);
    }

    // ---- apply ---------------------------------------------------------------

    #[test]
    fn apply_runs_codec_operation() {
        // `ByteBufCodecs.list()` applied over `int_be` via `apply`.
        let ints = int_be();
        let list_of_ints = apply(ints, byte_buf_codecs::list());
        let mut out = buf();
        list_of_ints.encode(&mut out, &vec![1i32, 2, 3]).unwrap();
        // varint count 3, then three big-endian ints.
        let bytes = written(out);
        assert_eq!(bytes, [3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(list_of_ints.decode(&mut input).unwrap(), vec![1i32, 2, 3]);
    }

    // ---- Send/Sync is a hard requirement -----------------------------------

    #[test]
    fn codecs_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StreamCodec<FriendlyByteBuf, i32>>();
        assert_send_sync::<StreamCodec<FriendlyByteBuf, Option<i32>>>();
        // The composite structs must be too (built on the tick thread, shipped
        // to a connection thread).
        assert_send_sync::<StreamCodec<FriendlyByteBuf, Pair>>();
    }

    #[test]
    fn clone_is_cheap_and_independent() {
        let a = int_be();
        let b = a.clone();
        let mut out_a = buf();
        let mut out_b = buf();
        a.encode(&mut out_a, &5).unwrap();
        b.encode(&mut out_b, &6).unwrap();
        assert_eq!(written(out_a), 5i32.to_be_bytes().to_vec());
        assert_eq!(written(out_b), 6i32.to_be_bytes().to_vec());
    }
}
