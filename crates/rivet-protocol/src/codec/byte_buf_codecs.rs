//! Port of `net.minecraft.network.codec.ByteBufCodecs` — the registry-independent
//! slice.
//!
//! Java: `ByteBufCodecs.java` in `working/Paper` (vanilla 26.2). Every method
//! here returns a fresh `StreamCodec<FriendlyByteBuf, T>` (codecs are stateless,
//! so a fresh `Arc` is observationally identical to Java's `static final`
//! fields). The buffer is `FriendlyByteBuf`, the only registry-independent
//! buffer this crate has; Java's generic `B extends ByteBuf` is instantiated
//! here as required by the port.
//!
//! Error model follows the [`crate::codec::StreamDecoder`]
//! boundary: netty `DecoderException`/`EncoderException` map to
//! `Err(CodecError)`. Where the underlying `FriendlyByteBuf`/`utf8_string`
//! helpers still panic (they model the raw netty layer, which Paper also does
//! not catch per-codec), this module performs the same bounds check first so the
//! codec boundary returns `Err` — see `string_utf8`, `byte_array`,
//! `byte_array_max`, and `long_array`. Negative sizes are programmer-unreachable
//! and keep the buffer layer's `panic!("{size}")` (Java `NegativeArraySizeException`).
//!
//! The NBT codecs (`tag_codec`, `compound_tag_codec`, `optional_tag_codec`,
//! `optional_compound_tag`) are the one deliberate exception: they delegate to
//! the raw `read_nbt_with_accounter` bridge, which panics on a truncated or
//! malformed payload (the bridge's I/O error maps to a panic, mirroring Java's
//! `readNbt` catch of `IOException` -> unchecked `EncoderException`). There is no
//! structure-independent pre-check a codec could apply, so this failure class
//! keeps the panic rather than returning `Err` — exactly the raw-buffer-layer
//! behavior those helpers document. `DecoderException`s that are structurally
//! detectable (a non-compound tag, a null/`EndTag`) still return `Err`.
//!
//! Blocked (STUB, landing with later units, not silently omitted):
//! - `fromCodec`/`fromCodecTrusted`/`fromCodecWithRegistries*` — need a DFU
//!   `Codec<T>` wired through `StreamCodec` via `NbtOps` (rivet-serialization
//!   port slice, epic #6/#10).
//! - `registryFriendlyLengthPrefixed` — needs the buffer-preserving decorator
//!   form of `lengthPrefixed` (the `RegistryFriendlyByteBuf` slice keeps the
//!   `RegistryAccess`; #126). `registry`/`holderRegistry`/`holder`/`holderSet`
//!   are ported in [`crate::codec::registry_byte_buf_codecs`] (they live there
//!   because their buffer is `RegistryFriendlyByteBuf`, not `FriendlyByteBuf`).
//! - `GAME_PROFILE`/`GAME_PROFILE_PROPERTIES` — need authlib `GameProfile`/
//!   `PropertyMap` (`PLAYER_NAME` is ported: it is just `stringUtf8(16)`).
//! - `VECTOR3F`/`QUATERNIONF` — need JOML.
//! - `lenientJson` — `JsonElement` has no port (`rivet-serialization` has no Gson
//!   value type; `json_ops` uses `ops::Output`).
//! - `RGB_COLOR` — needs `ARGB` in `rivet-util`.
//! - `trackDepth`/`increaseDepth` (Paper depth-tracking) — connection-level
//!   anti-DoS on the registry buffer, out of scope for the registry-independent
//!   slice.

use crate::codec::stream_codec::{CodecError, CodecOperation, StreamCodec, of};
use crate::codec::stream_decoder::StreamDecoder;
use crate::codec::stream_encoder::StreamEncoder;
use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use bytes::BytesMut;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::tag::Tag;
use rivet_registry::core::GameType;
use rivet_serialization::Either;
use rivet_util::mth::{pack_degrees, unpack_degrees};
use std::sync::Arc;

/// `ByteBufCodecs.MAX_INITIAL_COLLECTION_SIZE`.
pub const MAX_INITIAL_COLLECTION_SIZE: i32 = 65536;

/// `ByteBufCodecs.BOOL`.
pub fn bool() -> StreamCodec<FriendlyByteBuf, bool> {
    of(
        |output: &mut FriendlyByteBuf, value: &bool| {
            output.write_boolean(*value);
            Ok(())
        },
        |input| Ok(input.read_boolean()),
    )
}

/// `ByteBufCodecs.BYTE`.
pub fn byte() -> StreamCodec<FriendlyByteBuf, i8> {
    of(
        |output: &mut FriendlyByteBuf, value: &i8| {
            output.write_byte(*value);
            Ok(())
        },
        |input| Ok(input.read_byte()),
    )
}

/// `ByteBufCodecs.ROTATION_BYTE` — `BYTE.map(Mth::unpackDegrees, Mth::packDegrees)`.
pub fn rotation_byte() -> StreamCodec<FriendlyByteBuf, f32> {
    crate::codec::stream_codec::map(
        byte(),
        |b: &i8| unpack_degrees(*b),
        |f: &f32| pack_degrees(*f),
    )
}

/// `ByteBufCodecs.SHORT`.
pub fn short() -> StreamCodec<FriendlyByteBuf, i16> {
    of(
        |output: &mut FriendlyByteBuf, value: &i16| {
            output.write_short(*value);
            Ok(())
        },
        |input| Ok(input.read_short()),
    )
}

/// `ByteBufCodecs.UNSIGNED_SHORT` — decodes to a Java `Integer` in `0..=65535`.
pub fn unsigned_short() -> StreamCodec<FriendlyByteBuf, i32> {
    of(
        |output: &mut FriendlyByteBuf, value: &i32| {
            output.write_short(*value as i16);
            Ok(())
        },
        |input| Ok(input.read_unsigned_short() as i32),
    )
}

/// `ByteBufCodecs.INT`.
pub fn int() -> StreamCodec<FriendlyByteBuf, i32> {
    of(
        |output: &mut FriendlyByteBuf, value: &i32| {
            output.write_int(*value);
            Ok(())
        },
        |input| Ok(input.read_int()),
    )
}

/// `ByteBufCodecs.VAR_INT`.
pub fn var_int() -> StreamCodec<FriendlyByteBuf, i32> {
    of(
        |output: &mut FriendlyByteBuf, value: &i32| {
            output.write_var_int(*value);
            Ok(())
        },
        |input| Ok(input.read_var_int()),
    )
}

/// `ByteBufCodecs.OPTIONAL_VAR_INT` — `VAR_INT.map(i -> i == 0 ? empty : of(i -
/// 1), o -> o.isPresent() ? getAsInt() + 1 : 0)`. Java `int` arithmetic wraps,
/// so `Some(i32::MAX)` and `Some(i32::MIN)` round-trip through the full range.
pub fn optional_var_int() -> StreamCodec<FriendlyByteBuf, Option<i32>> {
    crate::codec::stream_codec::map(
        var_int(),
        |i: &i32| {
            if *i == 0 {
                None
            } else {
                Some(i.wrapping_sub(1))
            }
        },
        |o: &Option<i32>| match o {
            Some(n) => n.wrapping_add(1),
            None => 0,
        },
    )
}

/// `ByteBufCodecs.LONG`.
pub fn long() -> StreamCodec<FriendlyByteBuf, i64> {
    of(
        |output: &mut FriendlyByteBuf, value: &i64| {
            output.write_long(*value);
            Ok(())
        },
        |input| Ok(input.read_long()),
    )
}

/// `ByteBufCodecs.VAR_LONG`.
pub fn var_long() -> StreamCodec<FriendlyByteBuf, i64> {
    of(
        |output: &mut FriendlyByteBuf, value: &i64| {
            output.write_var_long(*value);
            Ok(())
        },
        |input| Ok(input.read_var_long()),
    )
}

/// `ByteBufCodecs.FLOAT` — raw-bits round-trip (`floatToRawIntBits`), so a NaN
/// payload passes through untouched.
pub fn float() -> StreamCodec<FriendlyByteBuf, f32> {
    of(
        |output: &mut FriendlyByteBuf, value: &f32| {
            output.write_float(*value);
            Ok(())
        },
        |input| Ok(input.read_float()),
    )
}

/// `ByteBufCodecs.DOUBLE` — raw-bits round-trip.
pub fn double() -> StreamCodec<FriendlyByteBuf, f64> {
    of(
        |output: &mut FriendlyByteBuf, value: &f64| {
            output.write_double(*value);
            Ok(())
        },
        |input| Ok(input.read_double()),
    )
}

/// `ByteBufCodecs.BYTE_ARRAY` — length varint, then raw bytes; the read bound
/// is the readable bytes after the varint (netty `DecoderException`), surfaced
/// as `Err` at this boundary.
pub fn byte_array() -> StreamCodec<FriendlyByteBuf, Vec<u8>> {
    of(
        |output: &mut FriendlyByteBuf, value: &Vec<u8>| {
            output.write_var_int(value.len() as i32);
            output.write_bytes(value);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let size = input.read_var_int();
            let max_size = input.readable_bytes() as i32;
            if size > max_size {
                return Err(CodecError::new(format!(
                    "ByteArray with size {size} is bigger than allowed {max_size}"
                )));
            }
            if size < 0 {
                panic!("{size}");
            }
            Ok(input.read_slice(size))
        },
    )
}

/// `ByteBufCodecs.byteArray(int maxSize)`.
///
/// Decode reads the varint length, then the raw bytes. Like `length_prefixed`,
/// a declared length that passes `maxSize` but exceeds the readable bytes
/// returns `Err` (Java: netty `IndexOutOfBoundsException` from `readBytes`);
/// the raw-buffer `read_slice` helper would otherwise allocate `size` bytes or
/// panic. Negative lengths stay a panic (Java `NegativeArraySizeException`),
/// matching the raw-buffer layer.
pub fn byte_array_max(max_size: i32) -> StreamCodec<FriendlyByteBuf, Vec<u8>> {
    of(
        move |output: &mut FriendlyByteBuf, value: &Vec<u8>| {
            if value.len() as i32 > max_size {
                return Err(CodecError::new(format!(
                    "ByteArray with size {} is bigger than allowed {max_size}",
                    value.len()
                )));
            }
            output.write_var_int(value.len() as i32);
            output.write_bytes(value);
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let size = input.read_var_int();
            if size > max_size {
                return Err(CodecError::new(format!(
                    "ByteArray with size {size} is bigger than allowed {max_size}"
                )));
            }
            if size < 0 {
                panic!("{size}");
            }
            let readable = input.readable_bytes() as i32;
            if size > readable {
                return Err(CodecError::new(format!(
                    "ByteArray with size {size} exceeds {readable} readable bytes"
                )));
            }
            Ok(input.read_slice(size))
        },
    )
}

/// `ByteBufCodecs.LONG_ARRAY` — length varint, then big-endian longs; the read
/// bound is `readableBytes() / 8` after the varint.
pub fn long_array() -> StreamCodec<FriendlyByteBuf, Vec<i64>> {
    of(
        |output: &mut FriendlyByteBuf, value: &Vec<i64>| {
            output.write_var_int(value.len() as i32);
            for v in value {
                output.write_long(*v);
            }
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let size = input.read_var_int();
            let max_size = input.readable_bytes() as i32 / 8;
            if size > max_size {
                return Err(CodecError::new(format!(
                    "LongArray with size {size} is bigger than allowed {max_size}"
                )));
            }
            if size < 0 {
                panic!("{size}");
            }
            let mut out = vec![0i64; size as usize];
            for slot in &mut out {
                *slot = input.read_long();
            }
            Ok(out)
        },
    )
}

/// `ByteBufCodecs.STRING_UTF8` — `stringUtf8(32767)`.
pub fn string() -> StreamCodec<FriendlyByteBuf, String> {
    string_utf8(MAX_STRING_LENGTH)
}

/// `ByteBufCodecs.stringUtf8(int maxStringLength)`.
///
/// Both sides perform `Utf8String`'s bounds checks at this boundary and return
/// `Err` (the checks live here because the underlying `utf8_string` helpers
/// still panic for the raw netty layer). UTF-16 code units are the count in
/// every check and message, matching Java's `CharSequence.length()`.
pub fn string_utf8(max_string_length: i32) -> StreamCodec<FriendlyByteBuf, String> {
    of(
        move |output: &mut FriendlyByteBuf, value: &String| {
            let value_length = value.encode_utf16().count() as i32;
            if value_length > max_string_length {
                return Err(CodecError::new(format!(
                    "String too big (was {value_length} characters, max {max_string_length})"
                )));
            }
            output.write_var_int(value.len() as i32);
            output.write_bytes(value.as_bytes());
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let max_encoded_length = crate::utf8_string::utf8_max_bytes(max_string_length);
            let buffer_length = input.read_var_int();
            if buffer_length > max_encoded_length {
                return Err(CodecError::new(format!(
                    "The received encoded string buffer length is longer than maximum allowed ({buffer_length} > {max_encoded_length})"
                )));
            }
            if buffer_length < 0 {
                return Err(CodecError::new(
                    "The received encoded string buffer length is less than zero! Weird string!",
                ));
            }
            let available_bytes = input.readable_bytes() as i32;
            if buffer_length > available_bytes {
                return Err(CodecError::new(format!(
                    "Not enough bytes in buffer, expected {buffer_length}, but got {available_bytes}"
                )));
            }
            let bytes = input.read_slice(buffer_length);
            let result = crate::utf8_string::decode_utf8(&bytes);
            let result_length = result.encode_utf16().count() as i32;
            if result_length > max_string_length {
                return Err(CodecError::new(format!(
                    "The received string length is longer than maximum allowed ({result_length} > {max_string_length})"
                )));
            }
            Ok(result)
        },
    )
}

/// `ByteBufCodecs.PLAYER_NAME` — `stringUtf8(16)`. A standalone static field
/// (used by the deferred `GAME_PROFILE` composite); the authlib `GameProfile`
/// value type is not ported, but the name codec itself is dependency-free.
pub fn player_name() -> StreamCodec<FriendlyByteBuf, String> {
    string_utf8(16)
}

/// `ByteBufCodecs.CONTAINER_ID` — `FriendlyByteBuf.readContainerId`/`writeContainerId`.
pub fn container_id() -> StreamCodec<FriendlyByteBuf, i32> {
    of(
        |output: &mut FriendlyByteBuf, value: &i32| {
            output.write_container_id(*value);
            Ok(())
        },
        |input| Ok(input.read_container_id()),
    )
}

/// `ByteBufCodecs.TAG` — `tagCodec(NbtAccounter::defaultQuota)`.
pub fn tag() -> StreamCodec<FriendlyByteBuf, Tag> {
    tag_codec(NbtAccounter::default_quota)
}

/// `ByteBufCodecs.TRUSTED_TAG` — `tagCodec(NbtAccounter::unlimitedHeap)`.
pub fn trusted_tag() -> StreamCodec<FriendlyByteBuf, Tag> {
    tag_codec(NbtAccounter::unlimited_heap)
}

/// `ByteBufCodecs.COMPOUND_TAG` — `compoundTagCodec(NbtAccounter::defaultQuota)`.
pub fn compound_tag() -> StreamCodec<FriendlyByteBuf, CompoundTag> {
    compound_tag_codec(NbtAccounter::default_quota)
}

/// `ByteBufCodecs.TRUSTED_COMPOUND_TAG` — `compoundTagCodec(NbtAccounter::unlimitedHeap)`.
pub fn trusted_compound_tag() -> StreamCodec<FriendlyByteBuf, CompoundTag> {
    compound_tag_codec(NbtAccounter::unlimited_heap)
}

/// `ByteBufCodecs.OPTIONAL_COMPOUND_TAG` — `null`/`EndTag` wire-encodes to
/// `Optional.empty()`, and a non-compound payload is `Err` (Java
/// `DecoderException("Not a compound tag: ...")`).
pub fn optional_compound_tag() -> StreamCodec<FriendlyByteBuf, Option<CompoundTag>> {
    of(
        |output: &mut FriendlyByteBuf, value: &Option<CompoundTag>| {
            match value {
                Some(c) => output.write_nbt(Some(&Tag::Compound(c.clone()))),
                None => output.write_nbt(None),
            };
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let mut accounter = NbtAccounter::default_quota();
            match input.read_nbt_with_accounter(&mut accounter) {
                Some(Tag::Compound(c)) => Ok(Some(c)),
                Some(other) => Err(CodecError::new(format!("Not a compound tag: {other}"))),
                None => Ok(None),
            }
        },
    )
}

/// `ByteBufCodecs.optionalTagCodec(Supplier<NbtAccounter>)`.
pub fn optional_tag_codec(
    accounter: impl Fn() -> NbtAccounter + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, Option<Tag>> {
    of(
        move |output: &mut FriendlyByteBuf, value: &Option<Tag>| {
            output.write_nbt(value.as_ref());
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let mut accounter = accounter();
            Ok(input.read_nbt_with_accounter(&mut accounter))
        },
    )
}

/// `ByteBufCodecs.tagCodec(Supplier<NbtAccounter>)` — `EndTag` is `Err` on both
/// sides with netty's `"Expected non-null compound tag"`.
pub fn tag_codec(
    accounter: impl Fn() -> NbtAccounter + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, Tag> {
    of(
        move |output: &mut FriendlyByteBuf, value: &Tag| {
            if matches!(value, Tag::End(_)) {
                return Err(CodecError::new("Expected non-null compound tag"));
            }
            output.write_nbt(Some(value));
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let mut accounter = accounter();
            match input.read_nbt_with_accounter(&mut accounter) {
                Some(tag) => Ok(tag),
                None => Err(CodecError::new("Expected non-null compound tag")),
            }
        },
    )
}

/// `ByteBufCodecs.compoundTagCodec(Supplier<NbtAccounter>)` — a non-compound
/// payload is `Err` (Java `DecoderException("Not a compound tag: ...")`).
pub fn compound_tag_codec(
    accounter: impl Fn() -> NbtAccounter + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, CompoundTag> {
    of(
        move |output: &mut FriendlyByteBuf, value: &CompoundTag| {
            output.write_nbt(Some(&Tag::Compound(value.clone())));
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let mut accounter = accounter();
            match input.read_nbt_with_accounter(&mut accounter) {
                Some(Tag::Compound(c)) => Ok(c),
                Some(other) => Err(CodecError::new(format!("Not a compound tag: {other}"))),
                None => Err(CodecError::new("Expected non-null compound tag")),
            }
        },
    )
}

/// `ByteBufCodecs.optional(StreamCodec)` — a boolean presence prefix, then the
/// value.
pub fn optional<V: 'static>(
    original: StreamCodec<FriendlyByteBuf, V>,
) -> StreamCodec<FriendlyByteBuf, Option<V>> {
    let encoder_codec = original.clone();
    of(
        move |output: &mut FriendlyByteBuf, value: &Option<V>| {
            match value {
                Some(v) => {
                    output.write_boolean(true);
                    encoder_codec.encode(output, v)?;
                }
                None => {
                    output.write_boolean(false);
                }
            }
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            if input.read_boolean() {
                Ok(Some(original.decode(input)?))
            } else {
                Ok(None)
            }
        },
    )
}

/// `ByteBufCodecs.readCount(ByteBuf, int maxSize)` — a varint count that is
/// `Err` when it exceeds `maxSize`.
pub fn read_count(input: &mut FriendlyByteBuf, max_size: i32) -> Result<i32, CodecError> {
    let count = input.read_var_int();
    if count > max_size {
        Err(CodecError::new(format!(
            "{count} elements exceeded max size of: {max_size}"
        )))
    } else {
        Ok(count)
    }
}

/// `ByteBufCodecs.writeCount(ByteBuf, int count, int maxSize)`.
pub fn write_count(
    output: &mut FriendlyByteBuf,
    count: i32,
    max_size: i32,
) -> Result<(), CodecError> {
    if count > max_size {
        Err(CodecError::new(format!(
            "{count} elements exceeded max size of: {max_size}"
        )))
    } else {
        output.write_var_int(count);
        Ok(())
    }
}

/// `ByteBufCodecs.collection(IntFunction<C> constructor, StreamCodec elementCodec, int maxSize)`.
///
/// Decode reads the count, calls the constructor with `count.min(65536)`
/// (Java's `MAX_INITIAL_COLLECTION_SIZE`), then decodes each element. Encode
/// writes the count first, then each element.
pub fn collection<V, C>(
    constructor: impl Fn(i32) -> C + Send + Sync + 'static,
    element_codec: StreamCodec<FriendlyByteBuf, V>,
    max_size: i32,
) -> StreamCodec<FriendlyByteBuf, C>
where
    V: 'static,
    C: Extend<V> + 'static,
    for<'a> &'a C: IntoIterator<Item = &'a V>,
{
    let encoder_codec = element_codec.clone();
    of(
        move |output: &mut FriendlyByteBuf, value: &C| {
            // Java writes the count first, then iterates the collection in its
            // own order. Reference iteration (no `Clone` of the elements) keeps
            // that order while the count is still known before any element is
            // written (Java `writeCount` before the loop).
            let elements: Vec<&V> = value.into_iter().collect();
            write_count(output, elements.len() as i32, max_size)?;
            for element in &elements {
                encoder_codec.encode(output, element)?;
            }
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let count = read_count(input, max_size)?;
            let mut result = constructor(count.min(MAX_INITIAL_COLLECTION_SIZE));
            let elements: Result<Vec<V>, CodecError> =
                (0..count).map(|_| element_codec.decode(input)).collect();
            result.extend(elements?);
            Ok(result)
        },
    )
}

/// `ByteBufCodecs.collection(IntFunction<C> constructor, StreamCodec elementCodec)` —
/// the unbounded overload (`Integer.MAX_VALUE`).
pub fn collection_unbounded<V, C>(
    constructor: impl Fn(i32) -> C + Send + Sync + 'static,
    element_codec: StreamCodec<FriendlyByteBuf, V>,
) -> StreamCodec<FriendlyByteBuf, C>
where
    V: 'static,
    C: Extend<V> + 'static,
    for<'a> &'a C: IntoIterator<Item = &'a V>,
{
    collection(constructor, element_codec, i32::MAX)
}

/// `ByteBufCodecs.collection(IntFunction<C> constructor)` — the `CodecOperation`
/// form.
pub fn collection_op<V, C>(
    constructor: impl Fn(i32) -> C + Send + Sync + 'static,
) -> CodecOperation<FriendlyByteBuf, V, C>
where
    V: 'static,
    C: Extend<V> + 'static,
    for<'a> &'a C: IntoIterator<Item = &'a V>,
{
    let constructor: Arc<dyn Fn(i32) -> C + Send + Sync> = Arc::new(constructor);
    CodecOperation::new(move |original| {
        let constructor = constructor.clone();
        collection(move |capacity| constructor(capacity), original, i32::MAX)
    })
}

/// `ByteBufCodecs.collection(IntFunction<C> constructor, int maxSize)` — the
/// `CodecOperation` form.
pub fn collection_op_max<V, C>(
    constructor: impl Fn(i32) -> C + Send + Sync + 'static,
    max_size: i32,
) -> CodecOperation<FriendlyByteBuf, V, C>
where
    V: 'static,
    C: Extend<V> + 'static,
    for<'a> &'a C: IntoIterator<Item = &'a V>,
{
    let constructor: Arc<dyn Fn(i32) -> C + Send + Sync> = Arc::new(constructor);
    CodecOperation::new(move |original| {
        let constructor = constructor.clone();
        collection(move |capacity| constructor(capacity), original, max_size)
    })
}

/// `ByteBufCodecs.list()` — `collection(ArrayList::new, original)` as a
/// `CodecOperation`. The constructor replicates `ArrayList(int)`'s negative
/// capacity `IllegalArgumentException`.
pub fn list<V: 'static>() -> CodecOperation<FriendlyByteBuf, V, Vec<V>> {
    CodecOperation::new(|original| {
        collection(
            |capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                Vec::with_capacity(capacity as usize)
            },
            original,
            i32::MAX,
        )
    })
}

/// `ByteBufCodecs.list(int maxSize)`.
pub fn list_max<V: 'static>(max_size: i32) -> CodecOperation<FriendlyByteBuf, V, Vec<V>> {
    CodecOperation::new(move |original| {
        collection(
            |capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                Vec::with_capacity(capacity as usize)
            },
            original,
            max_size,
        )
    })
}

/// `ByteBufCodecs.map(IntFunction<M> constructor, StreamCodec keyCodec,
/// StreamCodec valueCodec, int maxSize)`.
///
/// Decode reads the count, calls the constructor with `count.min(65536)`, then
/// decodes key/value pairs; encode writes the count first, then the pairs.
pub fn map<K, V, M>(
    constructor: impl Fn(i32) -> M + Send + Sync + 'static,
    key_codec: StreamCodec<FriendlyByteBuf, K>,
    value_codec: StreamCodec<FriendlyByteBuf, V>,
    max_size: i32,
) -> StreamCodec<FriendlyByteBuf, M>
where
    K: 'static,
    V: 'static,
    M: Extend<(K, V)> + 'static,
    for<'a> &'a M: IntoIterator<Item = (&'a K, &'a V)>,
{
    let key_encoder = key_codec.clone();
    let value_encoder = value_codec.clone();
    of(
        move |output, map: &M| {
            // Java `map.forEach` writes the count first, then key/value pairs in
            // the map's own iteration order (arbitrary for `HashMap`). The pairs
            // are collected as references so the count can precede the payload
            // without cloning the map or its entries.
            let pairs: Vec<(&K, &V)> = map.into_iter().collect();
            write_count(output, pairs.len() as i32, max_size)?;
            for (key, value) in &pairs {
                key_encoder.encode(output, key)?;
                value_encoder.encode(output, value)?;
            }
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let count = read_count(input, max_size)?;
            let mut result = constructor(count.min(MAX_INITIAL_COLLECTION_SIZE));
            let pairs: Result<Vec<(K, V)>, CodecError> = (0..count)
                .map(|_| {
                    let key = key_codec.decode(input)?;
                    let value = value_codec.decode(input)?;
                    Ok((key, value))
                })
                .collect();
            result.extend(pairs?);
            Ok(result)
        },
    )
}

/// `ByteBufCodecs.map(IntFunction<M> constructor, StreamCodec keyCodec,
/// StreamCodec valueCodec)` — the unbounded overload.
pub fn map_unbounded<K, V, M>(
    constructor: impl Fn(i32) -> M + Send + Sync + 'static,
    key_codec: StreamCodec<FriendlyByteBuf, K>,
    value_codec: StreamCodec<FriendlyByteBuf, V>,
) -> StreamCodec<FriendlyByteBuf, M>
where
    K: 'static,
    V: 'static,
    M: Extend<(K, V)> + 'static,
    for<'a> &'a M: IntoIterator<Item = (&'a K, &'a V)>,
{
    map(constructor, key_codec, value_codec, i32::MAX)
}

/// `ByteBufCodecs.either(StreamCodec leftCodec, StreamCodec rightCodec)` —
/// boolean prefix `true` selects the left codec, `false` the right.
pub fn either<L: 'static, R: 'static>(
    left_codec: StreamCodec<FriendlyByteBuf, L>,
    right_codec: StreamCodec<FriendlyByteBuf, R>,
) -> StreamCodec<FriendlyByteBuf, Either<L, R>> {
    let left_encoder = left_codec.clone();
    let right_encoder = right_codec.clone();
    of(
        move |output: &mut FriendlyByteBuf, value: &Either<L, R>| match value {
            Either::Left(l) => {
                output.write_boolean(true);
                left_encoder.encode(output, l)
            }
            Either::Right(r) => {
                output.write_boolean(false);
                right_encoder.encode(output, r)
            }
        },
        move |input: &mut FriendlyByteBuf| {
            if input.read_boolean() {
                Ok(Either::left(left_codec.decode(input)?))
            } else {
                Ok(Either::right(right_codec.decode(input)?))
            }
        },
    )
}

/// `ByteBufCodecs.lengthPrefixed(int maxSize)` — a varint payload length, then
/// the payload encoded/decoded through a scratch buffer. The decode message is
/// netty's `"Buffer size N is larger than allowed limit of M"` and the encode
/// message keeps Java's typo `"Buffer size N is  larger than allowed limit of M"`.
pub fn length_prefixed<V: 'static>(max_size: i32) -> CodecOperation<FriendlyByteBuf, V, V> {
    CodecOperation::new(move |original| {
        let encoder_codec = original.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &V| {
                let mut scratch = FriendlyByteBuf::new(BytesMut::new());
                encoder_codec.encode(&mut scratch, value)?;
                let size = scratch.readable_bytes() as i32;
                if size > max_size {
                    return Err(CodecError::new(format!(
                        "Buffer size {size} is  larger than allowed limit of {max_size}"
                    )));
                }
                output.write_var_int(size);
                output.write_bytes(scratch.as_slice());
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let size = input.read_var_int();
                if size > max_size {
                    return Err(CodecError::new(format!(
                        "Buffer size {size} is larger than allowed limit of {max_size}"
                    )));
                }
                // Defensive: `read_slice` would allocate `size` bytes, so bound
                // the allocation by what is actually readable before allocating
                // (a crafted length can pass `max_size` but exceed the buffer,
                // and a negative varint would wrap to a huge `usize`). Java
                // crashes with an `IndexOutOfBounds`/`NegativeArraySize` here;
                // the codec boundary surfaces `Err` instead.
                if size < 0 || size > input.readable_bytes() as i32 {
                    return Err(CodecError::new(format!(
                        "Length-prefixed payload of {size} bytes exceeds {readable} readable bytes",
                        readable = input.readable_bytes()
                    )));
                }
                let payload = input.read_slice(size);
                let mut limited = FriendlyByteBuf::new(BytesMut::from(payload.as_slice()));
                original.decode(&mut limited)
            },
        )
    })
}

/// `GameType.STREAM_CODEC` — `ByteBufCodecs.idMapper(BY_ID, GameType::getId)`.
///
/// A varint id through the `ByIdMap.continuous` `BY_ID` (ZERO fallback for any
/// id outside `[0, 4)`), encoding the value's `getId`. Java's declaration is
/// `StreamCodec<ByteBuf, GameType>`; Rust monomorphizes the buffer to
/// `FriendlyByteBuf`. Not used by `CommonPlayerSpawnInfo` (which reads raw
/// signed bytes), but it is the class's declared `STREAM_CODEC` (#108).
pub fn game_type_stream_codec() -> StreamCodec<FriendlyByteBuf, GameType> {
    id_mapper(GameType::by_id, GameType::get_id)
}

/// `ByteBufCodecs.idMapper(IntFunction<T> byId, ToIntFunction<T> toId)` — a
/// varint id in, the mapped value out.
pub fn id_mapper<T: 'static>(
    by_id: impl Fn(i32) -> T + Send + Sync + 'static,
    to_id: impl Fn(&T) -> i32 + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, T> {
    of(
        move |output: &mut FriendlyByteBuf, value: &T| {
            let id = to_id(value);
            output.write_var_int(id);
            Ok(())
        },
        move |input: &mut FriendlyByteBuf| {
            let id = input.read_var_int();
            Ok(by_id(id))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::stream_codec::apply;
    use rivet_nbt::end_tag::EndTag;
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::string_tag::StringTag;
    use std::collections::HashMap;
    use std::panic::catch_unwind;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    fn round_trip<T: PartialEq + std::fmt::Debug>(
        codec: &StreamCodec<FriendlyByteBuf, T>,
        value: &T,
    ) {
        let mut out = buf();
        codec.encode(&mut out, value).unwrap();
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        assert_eq!(&codec.decode(&mut input).unwrap(), value);
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

    // ---- primitives ---------------------------------------------------------

    #[test]
    fn scalar_round_trips() {
        round_trip(&bool(), &true);
        round_trip(&bool(), &false);
        round_trip(&byte(), &(-42i8));
        round_trip(&int(), &1234i32);
        round_trip(&var_int(), &(-1i32));
        round_trip(&var_int(), &i32::MIN);
        round_trip(&long(), &(i64::MAX - 1));
        round_trip(&var_long(), &(-(i64::MIN >> 1)));
        round_trip(&short(), &(-12345i16));
        round_trip(&unsigned_short(), &65535i32);
        round_trip(&float(), &3.5f32);
        round_trip(&double(), &-2.25f64);
    }

    #[test]
    fn float_preserves_nan_payload_bits() {
        let nan_bits = f32::from_bits(0x7fc0_1234);
        let mut out = buf();
        float().encode(&mut out, &nan_bits).unwrap();
        let bytes = written(out);
        // raw bits on the wire, no canonicalization
        assert_eq!(bytes, 0x7fc0_1234u32.to_be_bytes().to_vec());
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = float().decode(&mut input).unwrap();
        assert_eq!(decoded.to_bits(), 0x7fc0_1234u32);
    }

    #[test]
    fn rotation_byte_uses_degree_packing() {
        round_trip(&rotation_byte(), &90.0f32);
        // packDegrees(45) = floor(45 * 256 / 360) = 32 -> unpack = 32 * 360 / 256
        let mut out = buf();
        rotation_byte().encode(&mut out, &45.0f32).unwrap();
        assert_eq!(written(out), vec![32]);
    }

    #[test]
    fn container_id_is_var_int() {
        round_trip(&container_id(), &1234i32);
    }

    #[test]
    fn optional_var_int_shifts() {
        // 0 <-> None, n <-> Some(n-1); full i32 range wraps (Java int arithmetic).
        round_trip(&optional_var_int(), &None);
        round_trip(&optional_var_int(), &Some(0));
        round_trip(&optional_var_int(), &Some(i32::MAX));
        round_trip(&optional_var_int(), &Some(i32::MIN));

        let mut out = buf();
        optional_var_int().encode(&mut out, &Some(1)).unwrap();
        assert_eq!(written(out), vec![2]); // Some(1) -> 2
        let mut out = buf();
        optional_var_int().encode(&mut out, &None).unwrap();
        assert_eq!(written(out), vec![0]);
    }

    #[test]
    fn strings_round_trip_utf16_length() {
        round_trip(&string(), &"hello".to_string());
        round_trip(&string_utf8(16), &"héllo wörld".to_string());
        round_trip(&player_name(), &"Jonas".to_string());
    }

    #[test]
    fn string_utf8_oversize_encode_errors() {
        let codec = string_utf8(4);
        let mut out = buf();
        let err = codec.encode(&mut out, &"hello".to_string()).unwrap_err();
        assert_eq!(err.message, "String too big (was 5 characters, max 4)");
    }

    #[test]
    fn byte_array_round_trips() {
        round_trip(&byte_array(), &vec![1u8, 2, 3]);
        round_trip(&byte_array_max(10), &vec![9u8; 9]);
        let mut out = buf();
        byte_array().encode(&mut out, &vec![1u8, 2, 3]).unwrap();
        assert_eq!(written(out), vec![3, 1, 2, 3]);
    }

    #[test]
    fn byte_array_max_oversize_errors() {
        let codec = byte_array_max(2);
        let mut out = buf();
        let err = codec.encode(&mut out, &vec![1u8, 2, 3]).unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 3 is bigger than allowed 2"
        );
    }

    #[test]
    fn byte_array_decode_oversize_readable_errors() {
        // A length varint claiming 5 bytes when only 2 are readable.
        let mut input = buf();
        input.write_var_int(5);
        input.write_bytes(&[1u8, 2]);
        let err = byte_array().decode(&mut input).unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 5 is bigger than allowed 2"
        );
    }

    #[test]
    fn byte_array_max_decode_length_exceeding_readable_errors() {
        // A hostile length within max_size but larger than the readable payload
        // must return Err, not panic (defensive OOM/IOOB guard, mirroring
        // `length_prefixed`; Java: netty IndexOutOfBounds from readBytes).
        let codec = byte_array_max(100);
        let mut input = buf();
        input.write_var_int(50); // claims 50 bytes, only 0 readable
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 50 exceeds 0 readable bytes"
        );
    }

    #[test]
    fn long_array_round_trips() {
        round_trip(&long_array(), &vec![1i64, i64::MAX, -3]);
        let mut out = buf();
        long_array().encode(&mut out, &vec![5i64, 6]).unwrap();
        let bytes = written(out);
        let mut expected = vec![2];
        expected.extend_from_slice(&5i64.to_be_bytes());
        expected.extend_from_slice(&6i64.to_be_bytes());
        assert_eq!(bytes, expected);
    }

    // ---- NBT codecs ---------------------------------------------------------

    fn int_tag(v: i32) -> Tag {
        Tag::Int(IntTag::value_of(v))
    }

    fn compound(name: &str, v: i32) -> CompoundTag {
        let mut c = CompoundTag::new();
        c.put(name.to_string(), int_tag(v));
        c
    }

    #[test]
    fn tag_codec_round_trips() {
        round_trip(&tag(), &int_tag(5));
        round_trip(&tag(), &Tag::Compound(compound("k", 7)));
        round_trip(
            &trusted_tag(),
            &Tag::String(StringTag::value_of("hi".to_string())),
        );
    }

    #[test]
    fn tag_codec_rejects_end_on_encode() {
        let mut out = buf();
        let err = tag_codec(NbtAccounter::default_quota)
            .encode(&mut out, &Tag::End(EndTag))
            .unwrap_err();
        assert_eq!(err.message, "Expected non-null compound tag");
    }

    #[test]
    fn compound_tag_round_trips() {
        round_trip(&compound_tag(), &compound("x", 3));
        round_trip(&trusted_compound_tag(), &compound("y", -1));
    }

    #[test]
    fn compound_tag_rejects_non_compound_decode() {
        let mut out = buf();
        out.write_nbt(Some(&int_tag(5)));
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        let err = compound_tag().decode(&mut input).unwrap_err();
        // Java DecoderException("Not a compound tag: 5")
        assert_eq!(err.message, "Not a compound tag: 5");
    }

    #[test]
    fn optional_compound_tag_round_trips() {
        round_trip(&optional_compound_tag(), &Some(compound("a", 1)));
        round_trip(&optional_compound_tag(), &None);
    }

    #[test]
    fn optional_compound_tag_rejects_non_compound() {
        let mut out = buf();
        out.write_nbt(Some(&int_tag(5)));
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        let err = optional_compound_tag().decode(&mut input).unwrap_err();
        assert_eq!(err.message, "Not a compound tag: 5");
    }

    #[test]
    fn optional_tag_codec_round_trips() {
        round_trip(
            &optional_tag_codec(NbtAccounter::default_quota),
            &Some(int_tag(1)),
        );
        round_trip(&optional_tag_codec(NbtAccounter::default_quota), &None);
    }

    // ---- combinators --------------------------------------------------------

    #[test]
    fn optional_uses_bool_prefix_and_payload() {
        round_trip(&optional(int()), &Some(3));
        round_trip(&optional(int()), &None);
        let mut out = buf();
        optional(int()).encode(&mut out, &Some(3)).unwrap();
        assert_eq!(written(out), vec![1u8, 0, 0, 0, 3]);
        let mut out = buf();
        optional(int()).encode(&mut out, &None).unwrap();
        assert_eq!(written(out), vec![0u8]);
    }

    #[test]
    fn collection_round_trips_and_writes_count_first() {
        let codec = collection(
            |capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                Vec::with_capacity(capacity as usize)
            },
            int(),
            i32::MAX,
        );
        round_trip(&codec, &vec![1i32, 2, 3]);
        let mut out = buf();
        codec.encode(&mut out, &vec![7i32, 8]).unwrap();
        // varint count 2, then two big-endian ints.
        assert_eq!(written(out), vec![2, 0, 0, 0, 7, 0, 0, 0, 8]);
    }

    #[test]
    fn collection_oversize_count_errors() {
        let codec = collection(|_capacity: i32| Vec::new(), int(), 2);
        let mut out = buf();
        let err = codec.encode(&mut out, &vec![1i32, 2, 3]).unwrap_err();
        assert_eq!(err.message, "3 elements exceeded max size of: 2");

        let mut input = buf();
        input.write_var_int(3);
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(err.message, "3 elements exceeded max size of: 2");
    }

    #[test]
    fn collection_unbounded_round_trips() {
        round_trip(
            &collection_unbounded(
                |capacity: i32| {
                    if capacity < 0 {
                        panic!("Illegal Capacity: {capacity}");
                    }
                    Vec::with_capacity(capacity as usize)
                },
                int(),
            ),
            &vec![1i32, -2, 3],
        );
    }

    #[test]
    fn collection_op_and_op_max_apply_over_codec() {
        // The `CodecOperation` forms return a `CodecOperation`, used through
        // `apply` like `ByteBufCodecs.list()`.
        let codec = apply(
            int(),
            collection_op(|capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                Vec::with_capacity(capacity as usize)
            }),
        );
        round_trip(&codec, &vec![1i32, 2, 3]);

        let bounded = apply(
            int(),
            collection_op_max(
                |capacity: i32| {
                    if capacity < 0 {
                        panic!("Illegal Capacity: {capacity}");
                    }
                    Vec::with_capacity(capacity as usize)
                },
                2,
            ),
        );
        round_trip(&bounded, &vec![1i32, 2]);
        let mut out = buf();
        let err = bounded.encode(&mut out, &vec![1i32, 2, 3]).unwrap_err();
        assert_eq!(err.message, "3 elements exceeded max size of: 2");
    }

    #[test]
    fn list_max_bounds_the_count() {
        let codec = apply(int(), list_max(2));
        round_trip(&codec, &vec![1i32, 2]);
        let mut out = buf();
        let err = codec.encode(&mut out, &vec![1i32, 2, 3]).unwrap_err();
        assert_eq!(err.message, "3 elements exceeded max size of: 2");
    }

    #[test]
    fn map_unbounded_round_trips() {
        let codec = map_unbounded(
            |capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                HashMap::new()
            },
            string_utf8(16),
            int(),
        );
        let mut m = HashMap::new();
        m.insert("a".to_string(), 1);
        m.insert("b".to_string(), 2);
        round_trip(&codec, &m);
    }

    #[test]
    fn collection_constructor_capacity_capped_at_initial_size() {
        // decode passes count.min(65536) to the constructor (Java
        // MAX_INITIAL_COLLECTION_SIZE). The element codec is a no-op so the
        // decode can iterate a large count without consuming input bytes.
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_cap = Arc::clone(&calls);
        let codec = collection(
            move |capacity: i32| {
                calls_cap.lock().unwrap().push(capacity);
                Vec::new()
            },
            of(
                |_output: &mut FriendlyByteBuf, _value: &i32| Ok(()),
                |_input: &mut FriendlyByteBuf| Ok(0i32),
            ),
            70_000,
        );
        let mut input = buf();
        input.write_var_int(70_000);
        let _ = codec.decode(&mut input).unwrap();
        assert_eq!(*calls.lock().unwrap(), vec![65_536]);
    }

    #[test]
    fn list_applies_over_codec_via_apply() {
        let codec = apply(int(), list());
        round_trip(&codec, &vec![1i32, 2, 3]);
    }

    #[test]
    fn list_negative_capacity_panics_like_java() {
        // collection(ArrayList::new) with a count that is negative but passes
        // the max check (e.g. -1 <= Integer.MAX_VALUE) -> ArrayList(-1) panics.
        let codec = apply(int(), list());
        let mut input = buf();
        input.write_var_int(-1);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "Illegal Capacity: -1");
    }

    #[test]
    fn map_round_trips() {
        let codec = map(
            |capacity: i32| {
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                HashMap::new()
            },
            string_utf8(16),
            int(),
            i32::MAX,
        );
        let mut m = HashMap::new();
        m.insert("a".to_string(), 1);
        m.insert("b".to_string(), 2);
        round_trip(&codec, &m);
    }

    #[test]
    fn map_oversize_count_errors() {
        let codec = map(|_capacity: i32| HashMap::new(), string_utf8(16), int(), 1);
        let mut out = buf();
        let mut m = HashMap::new();
        m.insert("a".to_string(), 1);
        m.insert("b".to_string(), 2);
        let err = codec.encode(&mut out, &m).unwrap_err();
        assert_eq!(err.message, "2 elements exceeded max size of: 1");
    }

    #[test]
    fn either_uses_bool_prefix_left_true_right_false() {
        round_trip(&either(int(), var_int()), &Either::<i32, i32>::left(5));
        round_trip(&either(int(), var_int()), &Either::<i32, i32>::right(6));
        let mut out = buf();
        either(int(), var_int())
            .encode(&mut out, &Either::<i32, i32>::left(3))
            .unwrap();
        assert_eq!(written(out), vec![1u8, 0, 0, 0, 3]);
        let mut out = buf();
        either(int(), var_int())
            .encode(&mut out, &Either::<i32, i32>::right(3))
            .unwrap();
        assert_eq!(written(out), vec![0u8, 3]);
    }

    #[test]
    fn length_prefixed_round_trips_and_layout() {
        let codec = apply(int(), length_prefixed(1024));
        round_trip(&codec, &1234i32);
        let mut out = buf();
        codec.encode(&mut out, &1234i32).unwrap();
        // varint length 4, then the 4-byte BE int.
        assert_eq!(written(out), vec![4, 0, 0, 4, 210]);
    }

    #[test]
    fn length_prefixed_oversize_encode_errors() {
        let codec = apply(int(), length_prefixed(2));
        let mut out = buf();
        let err = codec.encode(&mut out, &1234i32).unwrap_err();
        // Java keeps the double space in the encode message ("is  larger than").
        assert_eq!(
            err.message,
            "Buffer size 4 is  larger than allowed limit of 2"
        );
    }

    #[test]
    fn length_prefixed_oversize_decode_errors() {
        let codec = apply(int(), length_prefixed(2));
        let mut input = buf();
        input.write_var_int(4);
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(
            err.message,
            "Buffer size 4 is larger than allowed limit of 2"
        );
    }

    #[test]
    fn length_prefixed_decode_length_exceeding_readable_errors() {
        // A length that passes max_size but exceeds the readable bytes must not
        // allocate `size` bytes (defensive OOM guard).
        let codec = apply(int(), length_prefixed(100));
        let mut input = buf();
        input.write_var_int(50); // claims 50 bytes, only 0 readable
        let err = codec.decode(&mut input).unwrap_err();
        assert_eq!(
            err.message,
            "Length-prefixed payload of 50 bytes exceeds 0 readable bytes"
        );
    }

    #[test]
    fn game_type_stream_codec_round_trips_and_wire_form() {
        // `idMapper(BY_ID, getId)`: a varint id, ZERO fallback on decode.
        let codec = game_type_stream_codec();
        for (game_type, id) in [
            (rivet_registry::core::GameType::Survival, 0),
            (rivet_registry::core::GameType::Creative, 1),
            (rivet_registry::core::GameType::Adventure, 2),
            (rivet_registry::core::GameType::Spectator, 3),
        ] {
            let mut out = buf();
            codec.encode(&mut out, &game_type).unwrap();
            assert_eq!(written(out), vec![id]);
            let mut input = FriendlyByteBuf::new(BytesMut::from(vec![id].as_slice()));
            assert_eq!(codec.decode(&mut input).unwrap(), game_type);
        }
        // Out-of-range id -> the ZERO fallback SURVIVAL (Java `BY_ID.apply`).
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![99u8].as_slice()));
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            rivet_registry::core::GameType::Survival
        );
        // A negative varint id -> SURVIVAL too (Java `BY_ID.apply(-1)`).
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        input.write_var_int(-1);
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            rivet_registry::core::GameType::Survival
        );
    }

    #[test]
    fn id_mapper_round_trips() {
        let codec = id_mapper(
            |id: i32| format!("v{id}"),
            |value: &String| value.trim_start_matches('v').parse::<i32>().unwrap(),
        );
        round_trip(&codec, &"v3".to_string());
        let mut out = buf();
        codec.encode(&mut out, &"v7".to_string()).unwrap();
        assert_eq!(written(out), vec![7]);
    }

    #[test]
    fn read_write_count_errors() {
        let mut out = buf();
        let err = write_count(&mut out, 5, 4).unwrap_err();
        assert_eq!(err.message, "5 elements exceeded max size of: 4");
        let mut input = buf();
        input.write_var_int(5);
        let err = read_count(&mut input, 4).unwrap_err();
        assert_eq!(err.message, "5 elements exceeded max size of: 4");
    }
}
