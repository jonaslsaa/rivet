//! Port of `net.minecraft.network.FriendlyByteBuf` — the registry-independent
//! surface.
//!
//! Java: `FriendlyByteBuf.java` in `working/Paper` (vanilla 26.2). This module
//! ports the subset that does not depend on registries, JOML, codecs, or crypto:
//! scalar big-endian primitives, `VarInt`/`VarLong`, bounded byte/int/long
//! arrays, bounded UTF strings (via [`crate::utf8_string`]), UUID, nullable /
//! optional / list / map / collection helpers, by-id helpers, container ids, and
//! the NBT bridge (via `rivet-nbt`'s `NbtIo` over `DataInput`/`DataOutput`
//! adapters for `BytesMut`).
//!
//! Deliberately absent (reported blocked by later work, not stubbed):
//! `ResourceKey`/registry paths, `BlockPos`/`ChunkPos`/`GlobalPos`,
//! `Vector3f`/`Quaternionf` (JOML), `PublicKey` (crypto), the JsonOps/codec
//! paths, `Instant`, and the `BitSet`-based set helpers.
//!
//! `readEnum`-by-class (`clazz.getEnumConstants()[readVarInt()]`) is ported as
//! [`FriendlyByteBuf::read_enum`] with an explicit value table — Java's
//! `Enum.values()` order (declaration order, ordinal == index) — mirroring how
//! the project's existing enums enumerate their own constants (e.g.
//! `rivet-registry::core::Direction`). `readIdentifier`/`writeIdentifier`
//! port `Identifier.read`/`write` (a `MAX_STRING_LENGTH`-bounded UTF string
//! parsed through `Identifier.parse`), which the common/cookie/ping packet
//! bodies in #86 need; `read_resource_key`/`read_registry_key` stay deferred
//! with the registry-wired units.
//!
//! Netty's `ByteBuf` big-endian scalar contract maps onto `bytes::Buf`/`BufMut`
//! exactly (24-bit medium, signed/unsigned variants, raw NaN-preserving float/
//! double on write via `floatToRawIntBits`). Error mapping follows PORTING.md line
//! 33: the unchecked `DecoderException`/`EncoderException` netty throws map to
//! `panic!` with the exact message. A Java `NegativeArraySizeException` from a
//! negative array length maps to `panic!("{size}")` (the JVM's message is the
//! offending size, verified empirically).

use std::collections::HashMap;
use std::io;

use bytes::{Buf, BufMut, BytesMut};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::end_tag::EndTag;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_util::data_io::{DataInput, DataOutput};
use rivet_util::mth::Uuid;

/// `FriendlyByteBuf.MAX_STRING_LENGTH`.
pub const MAX_STRING_LENGTH: i32 = 32767;
/// `FriendlyByteBuf.MAX_COMPONENT_STRING_LENGTH`.
pub const MAX_COMPONENT_STRING_LENGTH: i32 = 262144;

/// `FriendlyByteBuf` — a faithful wrapper over `bytes::BytesMut` mirroring the
/// Java `FriendlyByteBuf extends ByteBuf` read/write surface. Reads consume from
/// the front (like netty's reader index); writes append (like the writer index).
#[derive(Debug, Clone)]
pub struct FriendlyByteBuf {
    inner: BytesMut,
}

impl FriendlyByteBuf {
    /// `new FriendlyByteBuf(ByteBuf)`.
    pub fn new(inner: BytesMut) -> Self {
        FriendlyByteBuf { inner }
    }

    /// The underlying `BytesMut`.
    pub fn into_inner(self) -> BytesMut {
        self.inner
    }

    /// The readable bytes (netty `ByteBuf.readableBytes()`).
    pub fn readable_bytes(&self) -> usize {
        self.inner.remaining()
    }

    /// The unread prefix, for direct inspection.
    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_ref()
    }

    /// Reads exactly `len` bytes into a fresh `Vec`, advancing the cursor.
    /// Used by the codec layer's slice-based combinators (`length_prefixed`,
    /// bounded byte arrays).
    ///
    /// Guards the allocation up front: a negative or oversized `len` would
    /// otherwise become a huge/zeroed `usize` allocation. This is a Rust-only
    /// defensive guard (Java's `ByteBuf.readBytes` throws an
    /// `IndexOutOfBounds` on a short read); the message is this helper's own.
    /// Callers that must return `Err` instead (the codec boundary) pre-check
    /// against their own bound.
    pub fn read_slice(&mut self, len: i32) -> Vec<u8> {
        let len_usize = len as usize;
        if len < 0 || len_usize > self.readable_bytes() {
            panic!(
                "read_slice: {len} bytes requested, but only {} readable",
                self.readable_bytes()
            );
        }
        let mut bytes = vec![0u8; len_usize];
        self.inner.copy_to_slice(&mut bytes);
        bytes
    }

    /// Appends raw bytes, advancing the cursor.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.inner.put_slice(bytes);
        self
    }

    // ---- scalars (netty big-endian) ---------------------------------------

    /// `readBoolean()`.
    pub fn read_boolean(&mut self) -> bool {
        self.inner.get_u8() != 0
    }

    /// `writeBoolean(boolean)`.
    pub fn write_boolean(&mut self, value: bool) -> &mut Self {
        self.inner.put_u8(value as u8);
        self
    }

    /// `readByte()`.
    pub fn read_byte(&mut self) -> i8 {
        self.inner.get_i8()
    }

    /// `writeByte(int)`.
    pub fn write_byte(&mut self, value: i8) -> &mut Self {
        self.inner.put_i8(value);
        self
    }

    /// `readUnsignedByte()`.
    pub fn read_unsigned_byte(&mut self) -> u8 {
        self.inner.get_u8()
    }

    /// `readShort()`.
    pub fn read_short(&mut self) -> i16 {
        self.inner.get_i16()
    }

    /// `writeShort(int)`.
    pub fn write_short(&mut self, value: i16) -> &mut Self {
        self.inner.put_i16(value);
        self
    }

    /// `readUnsignedShort()`.
    pub fn read_unsigned_short(&mut self) -> u16 {
        self.inner.get_u16()
    }

    /// `readMedium()` — 24-bit signed (netty sign-extends the unsigned medium).
    pub fn read_medium(&mut self) -> i32 {
        let value = self.read_unsigned_medium();
        if value & 0x0080_0000 != 0 {
            (value | 0xFF00_0000) as i32
        } else {
            value as i32
        }
    }

    /// `writeMedium(int)` — big-endian low 24 bits.
    pub fn write_medium(&mut self, value: i32) -> &mut Self {
        self.inner.put_u8((value >> 16) as u8);
        self.inner.put_u8((value >> 8) as u8);
        self.inner.put_u8(value as u8);
        self
    }

    /// `readUnsignedMedium()` — 24-bit big-endian.
    pub fn read_unsigned_medium(&mut self) -> u32 {
        let b0 = self.inner.get_u8() as u32;
        let b1 = self.inner.get_u8() as u32;
        let b2 = self.inner.get_u8() as u32;
        (b0 << 16) | (b1 << 8) | b2
    }

    /// `readInt()`.
    pub fn read_int(&mut self) -> i32 {
        self.inner.get_i32()
    }

    /// `writeInt(int)`.
    pub fn write_int(&mut self, value: i32) -> &mut Self {
        self.inner.put_i32(value);
        self
    }

    /// `readUnsignedInt()`.
    pub fn read_unsigned_int(&mut self) -> u32 {
        self.inner.get_u32()
    }

    /// `readLong()`.
    pub fn read_long(&mut self) -> i64 {
        self.inner.get_i64()
    }

    /// `writeLong(long)`.
    pub fn write_long(&mut self, value: i64) -> &mut Self {
        self.inner.put_i64(value);
        self
    }

    /// `readChar()` — an unsigned 16-bit code unit.
    pub fn read_char(&mut self) -> u16 {
        self.inner.get_u16()
    }

    /// `writeChar(int)`.
    pub fn write_char(&mut self, value: u16) -> &mut Self {
        self.inner.put_u16(value);
        self
    }

    /// `readFloat()` — `Float.intBitsToFloat(readInt())`, raw bits.
    pub fn read_float(&mut self) -> f32 {
        f32::from_bits(self.inner.get_u32())
    }

    /// `writeFloat(float)` — netty `writeFloat` is `Float.floatToRawIntBits`,
    /// so a NaN payload passes through untouched (no canonicalization).
    pub fn write_float(&mut self, value: f32) -> &mut Self {
        self.inner.put_u32(value.to_bits());
        self
    }

    /// `readDouble()` — `Double.longBitsToDouble(readLong())`, raw bits.
    pub fn read_double(&mut self) -> f64 {
        f64::from_bits(self.inner.get_u64())
    }

    /// `writeDouble(double)` — netty `writeDouble` is `Double.doubleToRawLongBits`,
    /// so a NaN payload passes through untouched (no canonicalization).
    pub fn write_double(&mut self, value: f64) -> &mut Self {
        self.inner.put_u64(value.to_bits());
        self
    }

    // ---- varints ----------------------------------------------------------

    /// `readVarInt()`.
    pub fn read_var_int(&mut self) -> i32 {
        crate::var_int::read(&mut self.inner)
    }

    /// `writeVarInt(int)`.
    pub fn write_var_int(&mut self, value: i32) -> &mut Self {
        crate::var_int::write(&mut self.inner, value);
        self
    }

    /// `readVarLong()`.
    pub fn read_var_long(&mut self) -> i64 {
        crate::var_long::read(&mut self.inner)
    }

    /// `writeVarLong(long)`.
    pub fn write_var_long(&mut self, value: i64) -> &mut Self {
        crate::var_long::write(&mut self.inner, value);
        self
    }

    // ---- container ids (protocol VarInt aliases) --------------------------

    /// `readContainerId()` — `VarInt.read`.
    pub fn read_container_id(&mut self) -> i32 {
        crate::var_int::read(&mut self.inner)
    }

    /// `writeContainerId(int)` — `VarInt.write`.
    pub fn write_container_id(&mut self, id: i32) -> &mut Self {
        crate::var_int::write(&mut self.inner, id);
        self
    }

    // ---- bounded UTF strings ----------------------------------------------

    /// `readUtf()`.
    pub fn read_utf(&mut self) -> String {
        crate::utf8_string::read(&mut self.inner, MAX_STRING_LENGTH)
    }

    /// `readUtf(int maxLength)`.
    pub fn read_utf_max(&mut self, max_length: i32) -> String {
        crate::utf8_string::read(&mut self.inner, max_length)
    }

    /// `writeUtf(String)`.
    pub fn write_utf(&mut self, value: &str) -> &mut Self {
        crate::utf8_string::write(&mut self.inner, value, MAX_STRING_LENGTH);
        self
    }

    /// `writeUtf(String, int maxLength)`.
    pub fn write_utf_max(&mut self, value: &str, max_length: i32) -> &mut Self {
        crate::utf8_string::write(&mut self.inner, value, max_length);
        self
    }

    // ---- bounded byte arrays ----------------------------------------------

    /// `readByteArray()` — bounded by the current readable bytes.
    pub fn read_byte_array(&mut self) -> Vec<u8> {
        let max_size = self.readable_bytes() as i32;
        self.read_byte_array_max(max_size)
    }

    /// `readByteArray(int maxSize)`.
    pub fn read_byte_array_max(&mut self, max_size: i32) -> Vec<u8> {
        let size = self.read_var_int();
        if size > max_size {
            panic!("ByteArray with size {size} is bigger than allowed {max_size}");
        }
        if size < 0 {
            panic!("{size}");
        }
        let mut bytes = vec![0u8; size as usize];
        self.inner.copy_to_slice(&mut bytes);
        bytes
    }

    /// `writeByteArray(byte[])`.
    pub fn write_byte_array(&mut self, bytes: &[u8]) -> &mut Self {
        crate::var_int::write(&mut self.inner, bytes.len() as i32);
        self.inner.put_slice(bytes);
        self
    }

    // ---- bounded int arrays (varint elements) -----------------------------

    /// `readVarIntArray()` — bounded by the current readable bytes.
    pub fn read_var_int_array(&mut self) -> Vec<i32> {
        let max_size = self.readable_bytes() as i32;
        self.read_var_int_array_max(max_size)
    }

    /// `readVarIntArray(int maxSize)`.
    pub fn read_var_int_array_max(&mut self, max_size: i32) -> Vec<i32> {
        let size = self.read_var_int();
        if size > max_size {
            panic!("VarIntArray with size {size} is bigger than allowed {max_size}");
        }
        if size < 0 {
            panic!("{size}");
        }
        let mut out = Vec::with_capacity(size as usize);
        for _ in 0..size {
            out.push(self.read_var_int());
        }
        out
    }

    /// `writeVarIntArray(int[])`.
    pub fn write_var_int_array(&mut self, ints: &[i32]) -> &mut Self {
        crate::var_int::write(&mut self.inner, ints.len() as i32);
        for v in ints {
            crate::var_int::write(&mut self.inner, *v);
        }
        self
    }

    // ---- long arrays -------------------------------------------------------

    /// `readLongArray()` — length varint, then `length` big-endian longs; the
    /// size bound is computed from the readable bytes *after* the varint is
    /// consumed, exactly as Java reads it.
    pub fn read_long_array(&mut self) -> Vec<i64> {
        let size = self.read_var_int();
        let max_size = self.readable_bytes() as i32 / 8;
        if size > max_size {
            panic!("LongArray with size {size} is bigger than allowed {max_size}");
        }
        if size < 0 {
            panic!("{size}");
        }
        let mut out = vec![0i64; size as usize];
        self.read_fixed_size_long_array(&mut out);
        out
    }

    /// `readFixedSizeLongArray(long[])` — reads exactly `output.len()` longs.
    pub fn read_fixed_size_long_array(&mut self, output: &mut [i64]) {
        for slot in output.iter_mut() {
            *slot = self.read_long();
        }
    }

    /// `writeLongArray(long[])`.
    pub fn write_long_array(&mut self, longs: &[i64]) -> &mut Self {
        crate::var_int::write(&mut self.inner, longs.len() as i32);
        self.write_fixed_size_long_array(longs)
    }

    /// `writeFixedSizeLongArray(long[])`.
    pub fn write_fixed_size_long_array(&mut self, longs: &[i64]) -> &mut Self {
        for l in longs {
            self.write_long(*l);
        }
        self
    }

    // ---- UUID -------------------------------------------------------------

    /// `readUUID()` — `new UUID(readLong(), readLong())`.
    pub fn read_uuid(&mut self) -> Uuid {
        let most = self.read_long();
        let least = self.read_long();
        Uuid { most, least }
    }

    /// `writeUUID(UUID)`.
    pub fn write_uuid(&mut self, uuid: Uuid) -> &mut Self {
        self.write_long(uuid.most);
        self.write_long(uuid.least);
        self
    }

    // ---- optional / nullable ----------------------------------------------

    /// `readOptional(StreamDecoder)`.
    pub fn read_optional<T>(&mut self, mut value_reader: impl FnMut(&mut Self) -> T) -> Option<T> {
        if self.read_boolean() {
            Some(value_reader(self))
        } else {
            None
        }
    }

    /// `writeOptional(Optional, StreamEncoder)`.
    pub fn write_optional<T>(
        &mut self,
        value: Option<&T>,
        mut value_writer: impl FnMut(&mut Self, &T),
    ) -> &mut Self {
        match value {
            Some(v) => {
                self.write_boolean(true);
                value_writer(self, v);
            }
            None => {
                self.write_boolean(false);
            }
        }
        self
    }

    /// `readNullable(StreamDecoder)` — wire-identical to `readOptional`.
    pub fn read_nullable<T>(&mut self, mut value_reader: impl FnMut(&mut Self) -> T) -> Option<T> {
        if self.read_boolean() {
            Some(value_reader(self))
        } else {
            None
        }
    }

    /// `writeNullable(@Nullable T, StreamEncoder)`.
    pub fn write_nullable<T>(
        &mut self,
        value: Option<&T>,
        mut value_writer: impl FnMut(&mut Self, &T),
    ) -> &mut Self {
        match value {
            Some(v) => {
                self.write_boolean(true);
                value_writer(self, v);
            }
            None => {
                self.write_boolean(false);
            }
        }
        self
    }

    // ---- collections -------------------------------------------------------

    /// `readCollection(IntFunction<C> ctor, StreamDecoder)` — the ctor receives
    /// the raw varint count (Java `int`), so a negative count is rejected with
    /// the ctor's own exception, exactly as Java's guava-based ctors do.
    pub fn read_collection<T, C>(
        &mut self,
        ctor: impl Fn(i32) -> C,
        mut element_decoder: impl FnMut(&mut Self) -> T,
    ) -> C
    where
        C: Extend<T>,
    {
        let count = self.read_var_int();
        let mut result = ctor(count);
        result.extend((0..count).map(|_| element_decoder(self)));
        result
    }

    /// `writeCollection(Collection, StreamEncoder)`.
    pub fn write_collection<T>(
        &mut self,
        collection: &[T],
        mut encoder: impl FnMut(&mut Self, &T),
    ) -> &mut Self {
        crate::var_int::write(&mut self.inner, collection.len() as i32);
        for element in collection {
            encoder(self, element);
        }
        self
    }

    /// `readList(StreamDecoder)` — `readCollection(Lists::newArrayListWithCapacity, ...)`.
    pub fn read_list<T>(&mut self, element_decoder: impl FnMut(&mut Self) -> T) -> Vec<T> {
        self.read_collection(
            |count| {
                // `Lists.newArrayListWithCapacity` -> `checkNonnegative`.
                if count < 0 {
                    panic!("initialArraySize cannot be negative but was: {count}");
                }
                Vec::with_capacity(count as usize)
            },
            element_decoder,
        )
    }

    /// `readMap(IntFunction<M> ctor, StreamDecoder, StreamDecoder)` — the ctor
    /// receives the raw varint count (Java `int`), so a negative count is
    /// rejected with the ctor's own exception, exactly as Java's guava-based
    /// ctors do.
    pub fn read_map<K, V, M>(
        &mut self,
        ctor: impl Fn(i32) -> M,
        mut key_decoder: impl FnMut(&mut Self) -> K,
        mut value_decoder: impl FnMut(&mut Self) -> V,
    ) -> M
    where
        M: Extend<(K, V)>,
    {
        let count = self.read_var_int();
        let mut result = ctor(count);
        result.extend((0..count).map(|_| {
            let key = key_decoder(self);
            let value = value_decoder(self);
            (key, value)
        }));
        result
    }

    /// `readMap(StreamDecoder, StreamDecoder)` —
    /// `readMap(Maps::newHashMapWithExpectedSize, ...)`, returning a
    /// `std::collections::HashMap`.
    pub fn read_hash_map<K, V>(
        &mut self,
        key_decoder: impl FnMut(&mut Self) -> K,
        value_decoder: impl FnMut(&mut Self) -> V,
    ) -> HashMap<K, V>
    where
        K: Eq + std::hash::Hash,
    {
        self.read_map(
            |expected_size| {
                // `Maps.newHashMapWithExpectedSize` -> `capacity` -> `checkNonnegative`.
                if expected_size < 0 {
                    panic!("expectedSize cannot be negative but was: {expected_size}");
                }
                HashMap::with_capacity(expected_size as usize)
            },
            key_decoder,
            value_decoder,
        )
    }

    /// `writeMap(Map, StreamEncoder, StreamEncoder)` — iteration order is
    /// arbitrary, matching Java's `map.forEach`.
    pub fn write_map<K, V>(
        &mut self,
        map: &HashMap<K, V>,
        mut key_encoder: impl FnMut(&mut Self, &K),
        mut value_encoder: impl FnMut(&mut Self, &V),
    ) -> &mut Self {
        crate::var_int::write(&mut self.inner, map.len() as i32);
        for (k, v) in map {
            key_encoder(self, k);
            value_encoder(self, v);
        }
        self
    }

    // ---- counted readers ---------------------------------------------------

    /// `readWithCount(Consumer<FriendlyByteBuf>)`.
    pub fn read_with_count(&mut self, mut reader: impl FnMut(&mut Self)) {
        let count = self.read_var_int();
        for _ in 0..count {
            reader(self);
        }
    }

    // ---- by-id helpers -----------------------------------------------------

    /// `readById(IntFunction<T>)`.
    pub fn read_by_id<T>(&mut self, converter: impl Fn(i32) -> T) -> T {
        let id = self.read_var_int();
        converter(id)
    }

    /// `writeById(ToIntFunction<T>, T)`.
    pub fn write_by_id<T>(&mut self, converter: impl Fn(&T) -> i32, value: &T) -> &mut Self {
        let id = converter(value);
        self.write_var_int(id)
    }

    // ---- enum / identifier -------------------------------------------------

    /// `readEnum(Class<T>)` — `clazz.getEnumConstants()[readVarInt()]`.
    ///
    /// Java's `Enum.values()` returns the constants in declaration order, so the
    /// ordinal == the array index; `by_id` maps a varint id to the constant at
    /// that index. Out-of-range ids are the caller's Java-equivalent
    /// `ArrayIndexOutOfBounds` — the caller's codec decides how to surface it.
    pub fn read_enum<T>(&mut self, by_id: impl Fn(i32) -> T) -> T {
        let id = self.read_var_int();
        by_id(id)
    }

    /// `writeEnum(Enum<?>)` — `writeVarInt(value.ordinal())`. The ordinal is
    /// exactly `writeById(Enum::ordinal)`.
    pub fn write_enum<T>(&mut self, ordinal: impl Fn(&T) -> i32, value: &T) -> &mut Self {
        self.write_by_id(ordinal, value)
    }

    /// `readIdentifier()` — `Identifier.parse(readUtf(MAX_STRING_LENGTH))`.
    pub fn read_identifier(&mut self) -> Identifier {
        Identifier::parse(&self.read_utf())
    }

    /// `writeIdentifier(Identifier)` — `writeUtf(identifier.toString())`.
    pub fn write_identifier(&mut self, identifier: &Identifier) -> &mut Self {
        self.write_utf(&identifier.to_string())
    }

    // ---- NBT bridge --------------------------------------------------------

    /// `writeNbt(@Nullable Tag)` — `null` is written as `EndTag`; the tag is
    /// written unnamed (`NbtIo.writeAnyTag`). Errors map to the unchecked
    /// `EncoderException` (panic).
    pub fn write_nbt(&mut self, tag: Option<&Tag>) -> &mut Self {
        match tag {
            Some(t) => self.write_nbt_ref(t),
            None => {
                let end = Tag::End(EndTag);
                self.write_nbt_ref(&end)
            }
        }
    }

    fn write_nbt_ref(&mut self, tag: &Tag) -> &mut Self {
        let mut output = BufDataOutput {
            buf: &mut self.inner,
        };
        if let Err(e) = nbt_io::write_any_tag(tag, &mut output) {
            panic!("{e}");
        }
        self
    }

    /// `readNbt()` — `readNbt(input, NbtAccounter.defaultQuota())`, returning
    /// `None` for `EndTag` and panicking with `DecoderException("Not a compound
    /// tag: ...")` when the payload is a non-compound tag.
    pub fn read_nbt(&mut self) -> Option<CompoundTag> {
        let mut accounter = NbtAccounter::default_quota();
        match self.read_nbt_with_accounter(&mut accounter) {
            Some(Tag::Compound(compound)) => Some(compound),
            Some(other) => panic!("Not a compound tag: {other}"),
            None => None,
        }
    }

    /// `readNbt(NbtAccounter)` — reads any tag, `None` for `EndTag`. I/O errors
    /// map to the unchecked `EncoderException` (panic), mirroring Java.
    pub fn read_nbt_with_accounter(&mut self, accounter: &mut NbtAccounter) -> Option<Tag> {
        let mut input = BufDataInput {
            buf: &mut self.inner,
        };
        let tag = match nbt_io::read_any_tag(&mut input, accounter) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };
        if tag.id() == 0 { None } else { Some(tag) }
    }
}

/// `FriendlyByteBuf.limitValue(IntFunction, int)`.
pub fn limit_value<T>(original: impl Fn(i32) -> T, limit: i32) -> impl Fn(i32) -> T {
    move |value: i32| {
        if value > limit {
            panic!("Value {value} is larger than limit {limit}");
        } else {
            original(value)
        }
    }
}

/// `java.io.DataOutput` adapter over a `BufMut`, used by the NBT write bridge.
/// Modified UTF-8 (`writeUTF`) matches `DataOutputStream` exactly via `cesu8`.
struct BufDataOutput<'a> {
    buf: &'a mut dyn BufMut,
}

impl DataOutput for BufDataOutput<'_> {
    fn write(&mut self, b: i32) -> io::Result<()> {
        self.buf.put_u8(b as u8);
        Ok(())
    }

    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.buf.put_slice(b);
        Ok(())
    }

    fn write_utf(&mut self, s: &str) -> io::Result<()> {
        let encoded = cesu8::to_java_cesu8(s);
        if encoded.len() > u16::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("encoded string too long: {} bytes", encoded.len()),
            ));
        }
        self.buf.put_u16(encoded.len() as u16);
        self.buf.put_slice(&encoded);
        Ok(())
    }
}

/// `java.io.DataInput` adapter over a `Buf`, used by the NBT read bridge.
/// Short reads surface as `EOFException` with netty `ByteBufInputStream`'s exact
/// message (`"fieldSize is too long! Length is N, but maximum is M"`), matching
/// the `EncoderException(e)` message Java surfaces through `readNbt`.
struct BufDataInput<'a> {
    buf: &'a mut dyn Buf,
}

impl BufDataInput<'_> {
    /// netty `ByteBufInputStream.checkAvailable(fieldSize)`.
    fn check_available(&self, field_size: usize) -> io::Result<()> {
        let available = self.buf.remaining();
        if field_size > available {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "fieldSize is too long! Length is {field_size}, but maximum is {available}"
                ),
            ));
        }
        Ok(())
    }
}

impl DataInput for BufDataInput<'_> {
    fn read_unsigned_byte(&mut self) -> io::Result<i32> {
        self.check_available(1)?;
        Ok(self.buf.get_u8() as i32)
    }

    fn read_unsigned_short(&mut self) -> io::Result<i32> {
        self.check_available(2)?;
        Ok(self.buf.get_u16() as i32)
    }

    fn read_int(&mut self) -> io::Result<i32> {
        self.check_available(4)?;
        Ok(self.buf.get_i32())
    }

    fn read_long(&mut self) -> io::Result<i64> {
        self.check_available(8)?;
        Ok(self.buf.get_i64())
    }

    fn read_float(&mut self) -> io::Result<f32> {
        Ok(f32::from_bits(self.read_int()? as u32))
    }

    fn read_double(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.read_long()? as u64))
    }

    fn read_utf(&mut self) -> io::Result<String> {
        let len = self.read_unsigned_short()? as usize;
        self.check_available(len)?;
        let mut bytes = vec![0u8; len];
        self.buf.copy_to_slice(&mut bytes);
        cesu8::from_java_cesu8(&bytes)
            .map(|cow| cow.into_owned())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn read_fully(&mut self, n: usize) -> io::Result<Vec<u8>> {
        self.check_available(n)?;
        let mut bytes = vec![0u8; n];
        self.buf.copy_to_slice(&mut bytes);
        Ok(bytes)
    }

    fn skip_bytes(&mut self, n: usize) -> io::Result<usize> {
        let skip = n.min(self.buf.remaining());
        self.buf.advance(skip);
        Ok(skip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::catch_unwind;

    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::string_tag::StringTag;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    /// Re-reads the bytes written since construction (the reader index starts at
    /// 0, so `into_inner` yields everything written).
    fn written(b: FriendlyByteBuf) -> BytesMut {
        b.into_inner()
    }

    fn panic_message<F: FnOnce() -> R, R: std::fmt::Debug>(f: F) -> String {
        let err = catch_unwind(std::panic::AssertUnwindSafe(f))
            .expect_err("expected the closure to panic");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    // ---- scalars ----------------------------------------------------------

    #[test]
    fn scalar_round_trips() {
        let mut b = buf();
        b.write_boolean(true).write_boolean(false);
        b.write_byte(-5).write_byte(100);
        b.write_short(-1234).write_short(32000);
        b.write_medium(-1_000_000).write_medium(1_000_000);
        b.write_int(-2_000_000_000).write_int(2_000_000_000);
        b.write_long(-9_000_000_000_000_000_000)
            .write_long(9_000_000_000_000_000_000);
        b.write_char(0xFFFF).write_char(0x0041);

        let mut r = FriendlyByteBuf::new(written(b));
        assert!(r.read_boolean());
        assert!(!r.read_boolean());
        assert_eq!(r.read_byte(), -5);
        assert_eq!(r.read_byte(), 100);
        assert_eq!(r.read_short(), -1234);
        assert_eq!(r.read_short(), 32000);
        assert_eq!(r.read_medium(), -1_000_000);
        assert_eq!(r.read_medium(), 1_000_000);
        assert_eq!(r.read_int(), -2_000_000_000);
        assert_eq!(r.read_int(), 2_000_000_000);
        assert_eq!(r.read_long(), -9_000_000_000_000_000_000);
        assert_eq!(r.read_long(), 9_000_000_000_000_000_000);
        assert_eq!(r.read_char(), 0xFFFF);
        assert_eq!(r.read_char(), 0x0041);
        assert_eq!(r.readable_bytes(), 0);
    }

    #[test]
    fn unsigned_scalars() {
        let mut b = buf();
        b.write_byte(-2).write_short(-1).write_medium(-1);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_unsigned_byte(), 0xFE);
        assert_eq!(r.read_unsigned_short(), 0xFFFF);
        assert_eq!(r.read_unsigned_medium(), 0xFF_FFFF);
    }

    #[test]
    fn float_double_round_trip_and_raw_nan_payload() {
        let mut b = buf();
        b.write_float(3.25).write_float(f32::NAN);
        b.write_double(-2.5e100).write_double(f64::NAN);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_float(), 3.25);
        assert!(r.read_float().is_nan());
        assert_eq!(r.read_double(), -2.5e100);
        assert!(r.read_double().is_nan());
        // netty `writeFloat`/`writeDouble` use `floatToRawIntBits`/
        // `doubleToRawLongBits`, so a non-canonical NaN payload passes through
        // untouched (no canonicalization, matching the wire).
        let mut b2 = buf();
        b2.write_float(f32::from_bits(0x7fc0_0001)) // signaling NaN payload
            .write_double(f64::from_bits(0x7ff8_0000_0000_0001));
        let bytes = b2.into_inner();
        assert_eq!(&bytes[0..4], &0x7fc0_0001u32.to_be_bytes());
        assert_eq!(&bytes[4..12], &0x7ff8_0000_0000_0001u64.to_be_bytes());
        // `f32::NAN`/`f64::NAN` write their raw canonical bits, unchanged.
        let mut b3 = buf();
        b3.write_float(f32::NAN).write_double(f64::NAN);
        let bytes = b3.into_inner();
        assert_eq!(&bytes[0..4], &0x7fc0_0000u32.to_be_bytes());
        assert_eq!(&bytes[4..12], &0x7ff8_0000_0000_0000u64.to_be_bytes());
    }

    #[test]
    fn unsigned_int_round_trips() {
        let mut b = buf();
        b.write_int(-1).write_int(0x7F00_0000).write_int(i32::MIN);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_unsigned_int(), 0xFFFF_FFFF);
        assert_eq!(r.read_unsigned_int(), 0x7F00_0000);
        assert_eq!(r.read_unsigned_int(), 0x8000_0000);
    }

    #[test]
    fn scalar_big_endian_wire_format() {
        let mut b = buf();
        b.write_short(0x1234)
            .write_int(0x12345678)
            .write_long(0x123456789ABCDEF0);
        let bytes = b.into_inner();
        assert_eq!(&bytes[0..2], &[0x12, 0x34]);
        assert_eq!(&bytes[2..6], &[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(
            &bytes[6..14],
            &[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]
        );
    }

    // ---- varints ----------------------------------------------------------

    #[test]
    fn var_int_round_trips() {
        let mut b = buf();
        for v in [0, 1, 127, 128, 16_383, 16_384, i32::MAX, -1, i32::MIN] {
            b.write_var_int(v);
        }
        let mut r = FriendlyByteBuf::new(written(b));
        for v in [0, 1, 127, 128, 16_383, 16_384, i32::MAX, -1, i32::MIN] {
            assert_eq!(r.read_var_int(), v);
        }
    }

    #[test]
    fn var_long_round_trips() {
        let mut b = buf();
        for v in [0i64, 1, i64::MAX, -1, i64::MIN] {
            b.write_var_long(v);
        }
        let mut r = FriendlyByteBuf::new(written(b));
        for v in [0i64, 1, i64::MAX, -1, i64::MIN] {
            assert_eq!(r.read_var_long(), v);
        }
    }

    #[test]
    fn container_id_matches_var_int() {
        let mut b = buf();
        b.write_container_id(300);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_container_id(), 300);
        assert_eq!(r.readable_bytes(), 0);
    }

    // ---- utf --------------------------------------------------------------

    #[test]
    fn utf_round_trips() {
        let mut b = buf();
        b.write_utf("hello")
            .write_utf("héllo wörld 💩")
            .write_utf("");
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_utf(), "hello");
        assert_eq!(r.read_utf(), "héllo wörld 💩");
        assert_eq!(r.read_utf(), "");
    }

    #[test]
    fn utf_max_length_variants() {
        let mut b = buf();
        b.write_utf_max("abc", 3);
        b.write_utf_max("💩", 2);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_utf_max(3), "abc");
        assert_eq!(r.read_utf_max(2), "💩");
    }

    #[test]
    fn utf_rejects_over_max_length() {
        let msg = panic_message(|| {
            let mut b = buf();
            b.write_utf_max("hello", 4);
        });
        assert_eq!(msg, "String too big (was 5 characters, max 4)");
    }

    #[test]
    fn utf_read_rejects_over_decoded_length() {
        let mut b = buf();
        b.write_utf("hello");
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_utf_max(4));
        assert_eq!(
            msg,
            "The received string length is longer than maximum allowed (5 > 4)"
        );
        // The varint + payload were consumed (reader index advanced) before the check.
        assert_eq!(r.readable_bytes(), 0);
    }

    // ---- arrays -----------------------------------------------------------

    #[test]
    fn read_slice_guards_negative_and_oversized_lengths() {
        // A negative `len` must not become a huge `usize` allocation.
        let mut b = buf();
        b.write_var_int(5);
        let msg = panic_message(|| b.read_slice(-1));
        assert_eq!(msg, "read_slice: -1 bytes requested, but only 1 readable");
        // An oversized `len` (positive, but beyond the readable bytes) must not
        // allocate `len` bytes either.
        let mut b = buf();
        b.write_bytes(&[1, 2, 3]);
        let msg = panic_message(|| b.read_slice(100));
        assert_eq!(msg, "read_slice: 100 bytes requested, but only 3 readable");
        // The happy path still reads exactly `len` bytes.
        let mut b = buf();
        b.write_bytes(&[1, 2, 3]);
        assert_eq!(b.read_slice(2), vec![1, 2]);
        assert_eq!(b.readable_bytes(), 1);
    }

    #[test]
    fn byte_array_round_trips() {
        let mut b = buf();
        b.write_byte_array(&[])
            .write_byte_array(&[1, 2, 3])
            .write_byte_array(&[0u8; 300]);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_byte_array(), Vec::<u8>::new());
        assert_eq!(r.read_byte_array(), vec![1, 2, 3]);
        assert_eq!(r.read_byte_array(), vec![0u8; 300]);
    }

    #[test]
    fn byte_array_max_size_error() {
        let mut b = buf();
        b.write_byte_array(&[1, 2, 3, 4]);
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_byte_array_max(3));
        assert_eq!(msg, "ByteArray with size 4 is bigger than allowed 3");
    }

    #[test]
    fn byte_array_negative_size() {
        let mut b = buf();
        b.write_var_int(-1);
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_byte_array());
        assert_eq!(msg, "-1");
    }

    #[test]
    fn var_int_array_round_trips() {
        let mut b = buf();
        b.write_var_int_array(&[])
            .write_var_int_array(&[1, -1, 300, i32::MIN]);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_var_int_array(), Vec::<i32>::new());
        assert_eq!(r.read_var_int_array(), vec![1, -1, 300, i32::MIN]);
    }

    #[test]
    fn var_int_array_max_size_error() {
        let mut b = buf();
        b.write_var_int_array(&[1, 2, 3]);
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_var_int_array_max(2));
        assert_eq!(msg, "VarIntArray with size 3 is bigger than allowed 2");
    }

    #[test]
    fn long_array_round_trips() {
        let mut b = buf();
        b.write_long_array(&[]);
        b.write_long_array(&[i64::MIN, 0, i64::MAX]);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_long_array(), Vec::<i64>::new());
        assert_eq!(r.read_long_array(), vec![i64::MIN, 0, i64::MAX]);
    }

    #[test]
    fn long_array_max_size_uses_readable_after_varint() {
        // 4 longs present; a size of 5 must be rejected with maxSize computed
        // after the varint is read (readableBytes()/8 = 4).
        let mut b = buf();
        b.write_long_array(&[1, 2, 3, 4]);
        // Patch the length to 5.
        let mut bytes = written(b);
        bytes[0] = 5;
        let mut r = FriendlyByteBuf::new(bytes);
        let msg = panic_message(|| r.read_long_array());
        assert_eq!(msg, "LongArray with size 5 is bigger than allowed 4");
    }

    #[test]
    fn fixed_size_long_array_reads_exactly() {
        let mut b = buf();
        b.write_fixed_size_long_array(&[10, 20, 30]);
        let mut r = FriendlyByteBuf::new(written(b));
        let mut out = vec![0i64; 3];
        r.read_fixed_size_long_array(&mut out);
        assert_eq!(out, vec![10, 20, 30]);
        assert_eq!(r.readable_bytes(), 0);
    }

    // ---- UUID -------------------------------------------------------------

    #[test]
    fn uuid_round_trips() {
        let uuid = Uuid {
            most: 0x123456789ABCDEF0,
            least: -0x123456789ABCDEF0,
        };
        let mut b = buf();
        b.write_uuid(uuid);
        let mut r = FriendlyByteBuf::new(written(b));
        let got = r.read_uuid();
        assert_eq!(got.most, uuid.most);
        assert_eq!(got.least, uuid.least);
        // Wire is big-endian 16 bytes.
        let bytes = {
            let mut bb = buf();
            bb.write_uuid(uuid);
            bb.into_inner()
        };
        assert_eq!(bytes.len(), 16);
        assert_eq!(&bytes[0..8], &0x123456789ABCDEF0u64.to_be_bytes());
    }

    // ---- optional / nullable ----------------------------------------------

    #[test]
    fn optional_round_trips() {
        let mut b = buf();
        b.write_optional(Some(&42i32), |buf, v| {
            buf.write_int(*v);
        });
        b.write_optional(None::<&i32>, |buf, v: &i32| {
            buf.write_int(*v);
        });
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_optional(|buf| buf.read_int()), Some(42));
        assert_eq!(r.read_optional(|buf| buf.read_int()), None);
    }

    #[test]
    fn nullable_round_trips() {
        let mut b = buf();
        b.write_nullable(Some(&"x".to_string()), |buf, v| {
            buf.write_utf(v);
        });
        b.write_nullable(None::<&String>, |buf, v| {
            buf.write_utf(v);
        });
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_nullable(|buf| buf.read_utf()), Some("x".to_string()));
        assert_eq!(r.read_nullable(|buf| buf.read_utf()), None);
    }

    // ---- collections ------------------------------------------------------

    #[test]
    fn list_round_trips() {
        let mut b = buf();
        b.write_collection(&[1, 2, 3], |buf, v| {
            buf.write_var_int(*v);
        });
        b.write_collection(&[] as &[i32], |buf, v| {
            buf.write_var_int(*v);
        });
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_list(|buf| buf.read_var_int()), vec![1, 2, 3]);
        assert_eq!(r.read_list(|buf| buf.read_var_int()), Vec::<i32>::new());
    }

    #[test]
    fn list_negative_count_panics_like_guava() {
        // `Lists.newArrayListWithCapacity(-1)` -> `IllegalArgumentException:
        // initialArraySize cannot be negative but was: -1`, raised at the ctor
        // call (varint consumed, before any element read).
        let mut b = buf();
        b.write_var_int(-1);
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_list(|buf| buf.read_var_int()));
        assert_eq!(msg, "initialArraySize cannot be negative but was: -1");
        // Cursor consumed only the varint, as Java's ctor throws first.
        assert_eq!(r.readable_bytes(), 0);
    }

    #[test]
    fn map_negative_count_panics_like_guava() {
        // `Maps.newHashMapWithExpectedSize(-1)` -> `IllegalArgumentException:
        // expectedSize cannot be negative but was: -1`.
        let mut b = buf();
        b.write_var_int(-1);
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_hash_map(|buf| buf.read_var_int(), |buf| buf.read_utf()));
        assert_eq!(msg, "expectedSize cannot be negative but was: -1");
        assert_eq!(r.readable_bytes(), 0);
    }

    #[test]
    fn list_uses_utf_elements() {
        let mut b = buf();
        b.write_collection(&["a".to_string(), "bb".to_string()], |buf, v| {
            buf.write_utf(v);
        });
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(
            r.read_list(|buf| buf.read_utf()),
            vec!["a".to_string(), "bb".to_string()]
        );
    }

    #[test]
    fn map_round_trips() {
        let mut map: HashMap<i32, String> = HashMap::new();
        map.insert(1, "one".to_string());
        map.insert(2, "two".to_string());
        let mut b = buf();
        b.write_map(
            &map,
            |buf, k| {
                buf.write_var_int(*k);
            },
            |buf, v| {
                buf.write_utf(v);
            },
        );
        let mut r = FriendlyByteBuf::new(written(b));
        let got: HashMap<i32, String> =
            r.read_hash_map(|buf| buf.read_var_int(), |buf| buf.read_utf());
        assert_eq!(got, map);
    }

    #[test]
    fn read_with_count_invokes_reader_count_times() {
        let mut b = buf();
        b.write_var_int(3);
        b.write_utf("a").write_utf("b").write_utf("c");
        let mut r = FriendlyByteBuf::new(written(b));
        let mut seen = Vec::new();
        r.read_with_count(|buf| seen.push(buf.read_utf()));
        assert_eq!(
            seen,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    // ---- by-id helpers ----------------------------------------------------

    #[test]
    fn read_by_id_and_write_by_id() {
        let mut b = buf();
        b.write_by_id(|v: &i32| v % 10, &37); // writes 7
        let mut r = FriendlyByteBuf::new(written(b));
        let got = r.read_by_id(|id| id * 100);
        assert_eq!(got, 700);
    }

    // ---- limit_value ------------------------------------------------------

    #[test]
    fn limit_value_passes_within_limit() {
        let f = limit_value(|v: i32| v * 2, 10);
        assert_eq!(f(5), 10);
    }

    #[test]
    fn limit_value_rejects_over_limit() {
        let f = limit_value(|v: i32| v, 10);
        let msg = panic_message(|| f(11));
        assert_eq!(msg, "Value 11 is larger than limit 10");
    }

    // ---- NBT bridge -------------------------------------------------------

    #[test]
    fn nbt_compound_round_trip() {
        let mut tag = CompoundTag::new();
        tag.put_string("name", "héllo");
        tag.put_int("age", 42);
        tag.put_boolean("flag", true);
        let mut b = buf();
        b.write_nbt(Some(&Tag::Compound(tag.clone())));
        let mut r = FriendlyByteBuf::new(written(b));
        let got = r.read_nbt().expect("expected a compound tag");
        assert_eq!(got, tag);
    }

    #[test]
    fn nbt_null_writes_end_tag_and_reads_none() {
        let mut b = buf();
        b.write_nbt(None);
        // A lone 0 byte on the wire (EndTag id).
        assert_eq!(&b.into_inner()[..], &[0x00]);
        let mut b = buf();
        b.write_nbt(None);
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_nbt(), None);
    }

    #[test]
    fn nbt_non_compound_read_panics_with_exact_message() {
        let mut b = buf();
        b.write_nbt(Some(&Tag::Int(IntTag::new(5))));
        let mut r = FriendlyByteBuf::new(written(b));
        let msg = panic_message(|| r.read_nbt());
        assert_eq!(msg, "Not a compound tag: 5");
    }

    #[test]
    fn nbt_any_tag_returns_end_as_none_and_other_tags() {
        let mut b = buf();
        b.write_nbt(Some(&Tag::String(StringTag::value_of("abc".to_string()))));
        let mut r = FriendlyByteBuf::new(written(b));
        let mut accounter = NbtAccounter::default_quota();
        let got = r.read_nbt_with_accounter(&mut accounter);
        assert_eq!(
            got,
            Some(Tag::String(StringTag::value_of("abc".to_string())))
        );
    }

    #[test]
    fn nbt_truncated_read_panics_like_netty_eof() {
        // An empty buffer fails at `NbtIo.readAnyTag`'s very first byte:
        // `input.readByte()` on a 0-length stream -> netty
        // `ByteBufInputStream.checkAvailable(1)` ->
        // `EOFException("fieldSize is too long! Length is 1, but maximum is 0")`.
        // This read happens *before* `readTagSafe`'s `ReportedNbtException`
        // wrapping, so the raw detail message is what Java surfaces through
        // `EncoderException(e)` — and what the Rust panic reproduces.
        let mut r = FriendlyByteBuf::new(BytesMut::new());
        let msg = panic_message(|| {
            let _ = r.read_nbt_with_accounter(&mut NbtAccounter::default_quota());
        });
        assert_eq!(msg, "fieldSize is too long! Length is 1, but maximum is 0");
        // A payload short-read (after the type byte) is wrapped as
        // `ReportedNbtException` ("Loading NBT data") in Java, which rivet-nbt
        // reproduces — so the panic message there is the crash-report text, not
        // the netty EOF text.
        let mut b = buf();
        b.write_nbt(Some(&Tag::Int(IntTag::new(123))));
        let mut bytes = written(b);
        assert_eq!(bytes.len(), 5); // 0x03 id + 4 payload bytes
        bytes.truncate(3); // drop 2 payload bytes
        let mut r = FriendlyByteBuf::new(bytes);
        let msg = panic_message(|| {
            let _ = r.read_nbt_with_accounter(&mut NbtAccounter::default_quota());
        });
        assert!(msg.starts_with("Loading NBT data"), "got: {msg}");
    }

    #[test]
    fn nbt_nested_compound_round_trips_through_modified_utf8() {
        // Supplementary chars exercise the modified UTF-8 string path.
        let mut inner = CompoundTag::new();
        inner.put_string("key", "💩");
        let mut outer = CompoundTag::new();
        outer.put("inner".to_string(), Tag::Compound(inner));
        outer.put_long("big", i64::MIN);
        let mut b = buf();
        b.write_nbt(Some(&Tag::Compound(outer.clone())));
        let mut r = FriendlyByteBuf::new(written(b));
        assert_eq!(r.read_nbt(), Some(outer));
    }

    #[test]
    fn nbt_round_trip_cursor_consumes_exactly() {
        let mut tag = CompoundTag::new();
        tag.put_byte("b", -1);
        let mut b = buf();
        b.write_nbt(Some(&Tag::Compound(tag)));
        // Append a trailing sentinel to prove the reader stops at the tag.
        b.write_utf("sentinel");
        let mut r = FriendlyByteBuf::new(written(b));
        assert!(r.read_nbt().is_some());
        assert_eq!(r.read_utf(), "sentinel");
        assert_eq!(r.readable_bytes(), 0);
    }
}
