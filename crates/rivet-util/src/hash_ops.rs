//! Port of `net.minecraft.util.HashOps` — a `DynamicOps<HashCode>` DFU
//! serialization adapter (issue #205).
//!
//! HashOps is not a hash algorithm (it lives in `java_hash`), it is a
//! *serialization ops*: encoding a DFU value through HashOps produces a
//! `HashCode` of the value's canonical binary form, with every node prefixed by
//! a Guava-style tag byte (see the `TAG_*` constants). Paper uses
//! `HashOps.CRC32C_INSTANCE` (`ServerPlayer` registry hashing) wrapped in a
//! registry serialization context.
//!
//! Java semantics preserved exactly (verified against the pinned Java source
//! and golden values below):
//! - Guava `Hasher` writes primitives **little-endian** (`AbstractByteHasher`
//!   uses a little-endian scratch `ByteBuffer`); `HashCode.asBytes()` for a
//!   32-bit hash is the 4 little-endian bytes.
//! - `createString` writes `putInt(value.length())` — the **UTF-16 code-unit
//!   length** (`String.length()`), not the byte length; astral-plane
//!   characters count two code units.
//! - `createMap` sorts entries by key `padToLong()`, then value `padToLong()`
//!   (`Comparator.comparingLong(HashCode::padToLong)`); both Java overloads
//!   (`createMap(Map)` with `MAP_ENTRY_ORDER` and `createMap(Stream)` with
//!   `MAPLIKE_ENTRY_ORDER`) use the identical comparator, and the stream form
//!   keeps duplicate keys (stable sort).
//! - The unsupported decode-side methods return `DataResult.error("Unsupported
//!   operation")` (the shared `UNSUPPORTED_OPERATION_ERROR`); `convertTo`
//!   throws `UnsupportedOperationException` (a `panic!` here).
//! - `set`/`update`/`updateGeneric`/`remove` return the input unchanged.
//! - The builders are the DFU `AbstractUniversalBuilder` (map) and
//!   `AbstractListBuilder` (list) shapes with `DataResult`-accumulated state.
//!   Their `assert prefix.equals(empty)` is NOT ported as a runtime check:
//!   Rivet deliberately matches pinned Paper's normal runtime, which runs
//!   without `-ea` so the `assert` is never evaluated; a non-empty build prefix
//!   is silently ignored — verified against the Java run
//!   (`mapBuilder_nonempty_prefix`/`listBuilder_nonempty_prefix` hashes are the
//!   entry-only hashes).
//!
//! The `HashFunction` surface is modeled as the enum Guava's constructor takes;
//! only `CRC32C_INSTANCE` is ever constructed in Minecraft. `crc32c` (the
//! hardware-accelerated Rust crate) implements RFC 3720 CRC-32C, which
//! byte-matches both of Guava's `Hashing.crc32c()` implementations (the
//! `java.util.zip.CRC32C` hardware path and the software
//! `Crc32cHashFunction`) — golden values below are from the pinned Java.

use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, ListBuilder, MapLike, RecordBuilder};
use rivet_serialization::functions::Fn1;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::number::Number;
use rivet_serialization::pair::Pair;
use std::fmt;
use std::sync::Arc;

// Tag bytes (`HashOps.java`): each serialized value is prefixed with a tag
// that identifies the DFU shape, so the same value in a different shape hashes
// differently.
const TAG_EMPTY: u8 = 1;
const TAG_MAP_START: u8 = 2;
const TAG_MAP_END: u8 = 3;
const TAG_LIST_START: u8 = 4;
const TAG_LIST_END: u8 = 5;
const TAG_BYTE: u8 = 6;
const TAG_SHORT: u8 = 7;
const TAG_INT: u8 = 8;
const TAG_LONG: u8 = 9;
const TAG_FLOAT: u8 = 10;
const TAG_DOUBLE: u8 = 11;
const TAG_STRING: u8 = 12;
const TAG_BOOLEAN: u8 = 13;
const TAG_BYTE_ARRAY_START: u8 = 14;
const TAG_BYTE_ARRAY_END: u8 = 15;
const TAG_INT_ARRAY_START: u8 = 16;
const TAG_INT_ARRAY_END: u8 = 17;
const TAG_LONG_ARRAY_START: u8 = 18;
const TAG_LONG_ARRAY_END: u8 = 19;

const EMPTY_PAYLOAD: [u8; 1] = [TAG_EMPTY];
const FALSE_PAYLOAD: [u8; 2] = [TAG_BOOLEAN, 0];
const TRUE_PAYLOAD: [u8; 2] = [TAG_BOOLEAN, 1];

/// `HashOps.EMPTY_MAP_PAYLOAD` — the two-byte payload hashed for `emptyMap()`.
pub const EMPTY_MAP_PAYLOAD: [u8; 2] = [TAG_MAP_START, TAG_MAP_END];

/// `HashOps.EMPTY_LIST_PAYLOAD` — the two-byte payload hashed for `emptyList()`.
pub const EMPTY_LIST_PAYLOAD: [u8; 2] = [TAG_LIST_START, TAG_LIST_END];

/// Guava `com.google.common.hash.HashCode` — a 32-bit hash value (the width of
/// `Hashing.crc32c()`), the `DynamicOps` output type.
///
/// `HashCode.asBytes()` is the four little-endian bytes; `padToLong()`
/// zero-extends to `u64` (`UnsignedInts.toLong`); `asInt()` reinterprets the
/// same 32 bits as `i32`. Only the 32-bit form is needed — every `HashCode`
/// HashOps produces comes from CRC-32C. A future wider `HashFunction` variant
/// would require width-aware bytes here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HashCode(u32);

impl HashCode {
    /// `HashCode.asBytes()` — the four little-endian bytes.
    pub fn as_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// `HashCode.padToLong()` — zero-extends the 32-bit value to `u64`.
    pub fn pad_to_long(self) -> u64 {
        self.0 as u64
    }

    /// `HashCode.asInt()` — the raw 32-bit value as `i32`.
    pub fn as_int(self) -> i32 {
        self.0 as i32
    }
}

/// Guava `com.google.common.hash.HashFunction` — the functions `HashOps` can be
/// built over. Minecraft only ever constructs `Hashing.crc32c()`
/// (`HashOps.CRC32C_INSTANCE`), so `Crc32c` is the only variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashFunction {
    /// `Hashing.crc32c()` — the RFC 3720 CRC-32C checksum (32 hash bits).
    Crc32c,
}

impl HashFunction {
    /// `HashFunction.bits()`.
    pub fn bits(self) -> u32 {
        32
    }

    /// `HashFunction.newHasher()`.
    pub fn new_hasher(self) -> Hasher {
        Hasher::new(self)
    }

    /// `HashFunction.hashBytes(byte[])` — hash a raw byte payload.
    pub fn hash_bytes(self, bytes: &[u8]) -> HashCode {
        match self {
            HashFunction::Crc32c => HashCode(crc32c::crc32c(bytes)),
        }
    }
}

impl fmt::Display for HashFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Crc32cHashFunction.toString()` and the `ChecksumType.CRC_32C`
        // wrapper both render as this.
        match self {
            HashFunction::Crc32c => write!(f, "Hashing.crc32c()"),
        }
    }
}

/// Guava `com.google.common.hash.Hasher` — a byte accumulator that feeds
/// `HashFunction.hashBytes` on finalization. `putByte`/`putInt`/... append the
/// primitive's little-endian bytes (`AbstractByteHasher` scratch buffer);
/// `putUnencodedChars` appends each UTF-16 `char` little-endian.
///
/// The Java `Hasher` mutates in place and is tied to the `HashFunction` that
/// created it; the byte stream is materialized eagerly (the streaming checksum
/// is observationally identical to hashing the concatenation).
#[derive(Debug, Clone)]
pub struct Hasher {
    bytes: Vec<u8>,
    function: HashFunction,
}

impl Hasher {
    fn new(function: HashFunction) -> Self {
        Hasher {
            bytes: Vec::new(),
            function,
        }
    }

    /// `Hasher.putByte(byte)`.
    pub fn put_byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// `Hasher.putShort(short)` — little-endian.
    pub fn put_short(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// `Hasher.putInt(int)` — little-endian.
    pub fn put_int(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// `Hasher.putLong(long)` — little-endian.
    pub fn put_long(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// `Hasher.putFloat(float)` — `putInt(Float.floatToRawIntBits(f))`.
    pub fn put_float(&mut self, value: f32) {
        self.put_int(value.to_bits() as i32);
    }

    /// `Hasher.putDouble(double)` — `putLong(Double.doubleToRawLongBits(d))`.
    pub fn put_double(&mut self, value: f64) {
        self.put_long(value.to_bits() as i64);
    }

    /// `Hasher.putUnencodedChars(CharSequence)` — one little-endian `char`
    /// (UTF-16 code unit) per element.
    pub fn put_unencoded_chars(&mut self, value: &str) {
        for unit in value.encode_utf16() {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    /// `Hasher.putBytes(byte[])`.
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// `Hasher.hash()` — finalize the accumulated bytes.
    pub fn hash(&self) -> HashCode {
        self.function.hash_bytes(&self.bytes)
    }
}

/// `net.minecraft.util.HashOps` — `DynamicOps<HashCode>`.
#[derive(Debug, Clone, Copy)]
pub struct HashOps {
    /// `hashFunction`.
    hash_function: HashFunction,
    /// `hashFunction.hashBytes(EMPTY_PAYLOAD)`.
    empty: HashCode,
    /// `hashFunction.hashBytes(EMPTY_MAP_PAYLOAD)`.
    empty_map: HashCode,
    /// `hashFunction.hashBytes(EMPTY_LIST_PAYLOAD)`.
    empty_list: HashCode,
    /// `hashFunction.hashBytes(FALSE_PAYLOAD)`.
    false_hash: HashCode,
    /// `hashFunction.hashBytes(TRUE_PAYLOAD)`.
    true_hash: HashCode,
}

impl HashOps {
    /// `new HashOps(HashFunction)` — precomputes the empty/singleton hashes.
    pub fn new(hash_function: HashFunction) -> HashOps {
        HashOps {
            hash_function,
            empty: hash_function.hash_bytes(&EMPTY_PAYLOAD),
            empty_map: hash_function.hash_bytes(&EMPTY_MAP_PAYLOAD),
            empty_list: hash_function.hash_bytes(&EMPTY_LIST_PAYLOAD),
            false_hash: hash_function.hash_bytes(&FALSE_PAYLOAD),
            true_hash: hash_function.hash_bytes(&TRUE_PAYLOAD),
        }
    }

    /// `HashOps.CRC32C_INSTANCE`.
    pub fn crc32c_instance() -> HashOps {
        HashOps::new(HashFunction::Crc32c)
    }

    /// `isEmpty(HashCode)` — `value.equals(this.empty)`.
    fn is_empty(&self, value: &HashCode) -> bool {
        *value == self.empty
    }

    /// `HashOps.hashMap(Hasher, Stream<Pair>)` — tag MAP_START, then each entry
    /// sorted by key-then-value `padToLong`, then tag MAP_END.
    fn hash_map(&self, map: &[Pair<HashCode, HashCode>]) -> HashCode {
        let mut entries = map.to_vec();
        entries.sort_by(|a, b| {
            a.first
                .pad_to_long()
                .cmp(&b.first.pad_to_long())
                .then_with(|| a.second.pad_to_long().cmp(&b.second.pad_to_long()))
        });
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_MAP_START);
        for entry in entries {
            hasher.put_bytes(&entry.first.as_bytes());
            hasher.put_bytes(&entry.second.as_bytes());
        }
        hasher.put_byte(TAG_MAP_END);
        hasher.hash()
    }

    /// `ListHashBuilder.initBuilder()` — a fresh hasher primed with the list
    /// start tag.
    fn new_list_hasher(&self) -> Hasher {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_LIST_START);
        hasher
    }
}

/// `HashOps.UNSUPPORTED_OPERATION_ERROR` — the shared
/// `DataResult.error("Unsupported operation")` (no partial value).
fn unsupported<T>() -> DataResult<T> {
    DataResult::error("Unsupported operation")
}

impl DynamicOps for HashOps {
    type Output = HashCode;

    fn empty(&self) -> HashCode {
        self.empty
    }

    fn empty_map(&self) -> HashCode {
        self.empty_map
    }

    fn empty_list(&self) -> HashCode {
        self.empty_list
    }

    /// `HashOps.convertTo` — throws `UnsupportedOperationException`.
    fn convert_to<U: DynamicOps>(&self, _out_ops: &U, _input: &HashCode) -> U::Output {
        panic!("Can't convert from this type");
    }

    /// `HashOps.createNumeric(Number)` — dispatches to the primitive creators;
    /// the Java `default` branch (`createDouble(value.doubleValue())`) covers
    /// `BigInteger`/`BigDecimal`, which `rivet_serialization::Number` does not
    /// carry (see `number.rs`), so it is unreachable here.
    fn create_numeric(&self, value: Number) -> HashCode {
        match value {
            Number::Byte(v) => self.create_byte(v),
            Number::Short(v) => self.create_short(v),
            Number::Int(v) => self.create_int(v),
            Number::Long(v) => self.create_long(v),
            Number::Float(v) => self.create_float(v),
            Number::Double(v) => self.create_double(v),
        }
    }

    /// `HashOps.createByte` — `newHasher(2).putByte(6).putByte(value).hash()`.
    fn create_byte(&self, value: i8) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_BYTE);
        hasher.put_byte(value as u8);
        hasher.hash()
    }

    /// `HashOps.createShort` — `newHasher(3).putByte(7).putShort(value).hash()`.
    fn create_short(&self, value: i16) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_SHORT);
        hasher.put_short(value);
        hasher.hash()
    }

    /// `HashOps.createInt` — `newHasher(5).putByte(8).putInt(value).hash()`.
    fn create_int(&self, value: i32) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_INT);
        hasher.put_int(value);
        hasher.hash()
    }

    /// `HashOps.createLong` — `newHasher(9).putByte(9).putLong(value).hash()`.
    fn create_long(&self, value: i64) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_LONG);
        hasher.put_long(value);
        hasher.hash()
    }

    /// `HashOps.createFloat` — `newHasher(5).putByte(10).putFloat(value).hash()`.
    fn create_float(&self, value: f32) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_FLOAT);
        hasher.put_float(value);
        hasher.hash()
    }

    /// `HashOps.createDouble` — `newHasher(9).putByte(11).putDouble(value).hash()`.
    fn create_double(&self, value: f64) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_DOUBLE);
        hasher.put_double(value);
        hasher.hash()
    }

    /// `HashOps.createString` — `putByte(12).putInt(value.length())
    /// .putUnencodedChars(value)` — the prefix is the **UTF-16 code-unit
    /// length**.
    fn create_string(&self, value: String) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_STRING);
        hasher.put_int(value.encode_utf16().count() as i32);
        hasher.put_unencoded_chars(&value);
        hasher.hash()
    }

    /// `HashOps.createBoolean` — the precomputed `trueHash`/`falseHash`.
    fn create_boolean(&self, value: bool) -> HashCode {
        if value {
            self.true_hash
        } else {
            self.false_hash
        }
    }

    /// `HashOps.getNumberValue(HashCode)` — `unsupported()`.
    fn get_number_value(&self, _input: &HashCode) -> DataResult<Number> {
        unsupported()
    }

    /// `HashOps.getNumberValue(HashCode, Number defaultValue)` — returns the
    /// default unconditionally (the trait default derives the same result
    /// because `get_number_value` always errors, but this mirrors Java's
    /// override exactly).
    fn get_number_value_or(&self, _input: &HashCode, default_value: Number) -> Number {
        default_value
    }

    /// `HashOps.getBooleanValue` — `unsupported()`.
    fn get_boolean_value(&self, _input: &HashCode) -> DataResult<bool> {
        unsupported()
    }

    /// `HashOps.getStringValue` — `unsupported()`.
    fn get_string_value(&self, _input: &HashCode) -> DataResult<String> {
        unsupported()
    }

    /// `HashOps.mergeToList(HashCode prefix, HashCode value)` — succeeds only
    /// when the prefix is empty.
    fn merge_to_list(&self, list: &HashCode, value: HashCode) -> DataResult<HashCode> {
        if self.is_empty(list) {
            DataResult::success(self.create_list(vec![value]))
        } else {
            unsupported()
        }
    }

    /// `HashOps.mergeToList(HashCode prefix, List<HashCode> values)`.
    fn merge_to_list_many(&self, list: &HashCode, values: Vec<HashCode>) -> DataResult<HashCode> {
        if self.is_empty(list) {
            DataResult::success(self.create_list(values))
        } else {
            unsupported()
        }
    }

    /// `HashOps.mergeToMap(HashCode prefix, HashCode key, HashCode value)`.
    fn merge_to_map(&self, map: &HashCode, key: HashCode, value: HashCode) -> DataResult<HashCode> {
        if self.is_empty(map) {
            DataResult::success(self.create_map(vec![Pair::of(key, value)]))
        } else {
            unsupported()
        }
    }

    /// `HashOps.mergeToMap(HashCode prefix, MapLike<HashCode> values)`.
    fn merge_to_map_like(
        &self,
        map: &HashCode,
        values: &dyn MapLike<HashCode>,
    ) -> DataResult<HashCode> {
        if self.is_empty(map) {
            DataResult::success(self.create_map(values.entries()))
        } else {
            unsupported()
        }
    }

    /// `HashOps.mergeToMap(HashCode prefix, Map<HashCode, HashCode> values)`.
    fn merge_to_map_many(
        &self,
        map: &HashCode,
        values: Vec<Pair<HashCode, HashCode>>,
    ) -> DataResult<HashCode> {
        if self.is_empty(map) {
            DataResult::success(self.create_map(values))
        } else {
            unsupported()
        }
    }

    /// `HashOps.getMapValues` — `unsupported()`.
    fn get_map_values(&self, _input: &HashCode) -> DataResult<Vec<Pair<HashCode, HashCode>>> {
        unsupported()
    }

    /// `HashOps.getMapEntries` — `unsupported()`.
    fn get_map_entries(
        &self,
        _input: &HashCode,
    ) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&HashCode, &HashCode))>> {
        unsupported()
    }

    /// `HashOps.createMap(Stream<Pair<HashCode, HashCode>>)` — the sorted map
    /// hash. The Java `createMap(Map)` overload sorts with the identical
    /// comparator, so this is the single ported form.
    fn create_map(&self, map: Vec<Pair<HashCode, HashCode>>) -> HashCode {
        self.hash_map(&map)
    }

    /// `HashOps.getMap` — `unsupported()`.
    fn get_map(&self, _input: &HashCode) -> DataResult<Box<dyn MapLike<HashCode>>> {
        unsupported()
    }

    /// `HashOps.getStream` — `unsupported()`.
    fn get_stream(&self, _input: &HashCode) -> DataResult<Vec<HashCode>> {
        unsupported()
    }

    /// `HashOps.getList` — `unsupported()`.
    fn get_list(&self, _input: &HashCode) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&HashCode))>> {
        unsupported()
    }

    /// `HashOps.createList(Stream<HashCode>)` — tag LIST_START, then each value
    /// in stream order, then tag LIST_END.
    fn create_list(&self, input: Vec<HashCode>) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_LIST_START);
        for value in input {
            hasher.put_bytes(&value.as_bytes());
        }
        hasher.put_byte(TAG_LIST_END);
        hasher.hash()
    }

    /// `HashOps.getByteBuffer` — `unsupported()`.
    fn get_byte_buffer(&self, _input: &HashCode) -> DataResult<Vec<u8>> {
        unsupported()
    }

    /// `HashOps.createByteList(ByteBuffer)` — tag BYTE_ARRAY_START, raw bytes,
    /// tag BYTE_ARRAY_END.
    fn create_byte_list(&self, input: &[u8]) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_BYTE_ARRAY_START);
        hasher.put_bytes(input);
        hasher.put_byte(TAG_BYTE_ARRAY_END);
        hasher.hash()
    }

    /// `HashOps.getIntStream` — `unsupported()`.
    fn get_int_stream(&self, _input: &HashCode) -> DataResult<Vec<i32>> {
        unsupported()
    }

    /// `HashOps.createIntList(IntStream)` — tag INT_ARRAY_START, each int
    /// little-endian, tag INT_ARRAY_END.
    fn create_int_list(&self, input: Vec<i32>) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_INT_ARRAY_START);
        for value in input {
            hasher.put_int(value);
        }
        hasher.put_byte(TAG_INT_ARRAY_END);
        hasher.hash()
    }

    /// `HashOps.getLongStream` — `unsupported()`.
    fn get_long_stream(&self, _input: &HashCode) -> DataResult<Vec<i64>> {
        unsupported()
    }

    /// `HashOps.createLongList(LongStream)` — tag LONG_ARRAY_START, each long
    /// little-endian, tag LONG_ARRAY_END.
    fn create_long_list(&self, input: Vec<i64>) -> HashCode {
        let mut hasher = self.hash_function.new_hasher();
        hasher.put_byte(TAG_LONG_ARRAY_START);
        for value in input {
            hasher.put_long(value);
        }
        hasher.put_byte(TAG_LONG_ARRAY_END);
        hasher.hash()
    }

    /// `HashOps.remove(HashCode, String)` — returns the input unchanged.
    fn remove(&self, input: HashCode, _key: &str) -> HashCode {
        input
    }

    /// `HashOps.get(HashCode, String)` — `unsupported()`.
    fn get(&self, _input: &HashCode, _key: &str) -> DataResult<HashCode> {
        unsupported()
    }

    /// `HashOps.getGeneric(HashCode, HashCode)` — `unsupported()`.
    fn get_generic(&self, _input: &HashCode, _key: &HashCode) -> DataResult<HashCode> {
        unsupported()
    }

    /// `HashOps.set(HashCode, String, HashCode)` — returns the input unchanged
    /// (the trait default would merge into a map, which is NOT HashOps
    /// behavior).
    fn set(&self, input: &HashCode, _key: &str, _value: HashCode) -> HashCode {
        *input
    }

    /// `HashOps.update(HashCode, String, Function)` — returns the input
    /// unchanged.
    fn update(
        &self,
        input: &HashCode,
        _key: &str,
        _function: impl Fn(HashCode) -> HashCode,
    ) -> HashCode {
        *input
    }

    /// `HashOps.updateGeneric(HashCode, HashCode, Function)` — returns the
    /// input unchanged.
    fn update_generic(
        &self,
        input: &HashCode,
        _key: &HashCode,
        _function: impl Fn(HashCode) -> HashCode,
    ) -> HashCode {
        *input
    }

    /// `HashOps.mapBuilder()` — a `HashOps.MapHashBuilder`.
    fn map_builder(&self) -> Box<dyn RecordBuilder<Output = HashCode> + '_> {
        Box::new(MapHashBuilder::new(*self))
    }

    /// `HashOps.listBuilder()` — a `HashOps.ListHashBuilder`.
    fn list_builder(&self) -> Box<dyn ListBuilder<Output = HashCode> + '_> {
        Box::new(ListHashBuilder::new(*self))
    }
}

impl fmt::Display for HashOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `"Hash " + this.hashFunction`.
        write!(f, "Hash {}", self.hash_function)
    }
}

/// `HashOps.MapHashBuilder` — `AbstractUniversalBuilder<HashCode,
/// List<Pair<HashCode, HashCode>>>`: accumulates key/value pairs, then hashes
/// them as a sorted map.
#[derive(Debug)]
struct MapHashBuilder {
    ops: HashOps,
    /// `AbstractBuilder.builder` — `DataResult.success(initBuilder(),
    /// Lifecycle.stable())`.
    builder: DataResult<Vec<Pair<HashCode, HashCode>>>,
}

impl MapHashBuilder {
    /// `new MapHashBuilder(HashOps.this)` — `initBuilder()` returns a fresh
    /// `ArrayList`.
    fn new(ops: HashOps) -> Self {
        MapHashBuilder {
            ops,
            builder: DataResult::success_with_lifecycle(Vec::new(), Lifecycle::stable()),
        }
    }
}

impl RecordBuilder for MapHashBuilder {
    type Output = HashCode;

    /// `AbstractUniversalBuilder.add(T key, T value)` — `append` pushes the
    /// pair.
    fn add(&mut self, key: HashCode, value: HashCode) {
        let prev = self.builder.clone();
        self.builder = prev.map_owned(|mut b| {
            b.push(Pair::of(key, value));
            b
        });
    }

    /// `AbstractUniversalBuilder.add(String key, T value)` — the interface
    /// default `add(ops.createString(key), value)`.
    fn add_string(&mut self, key: &str, value: HashCode) {
        self.add(self.ops.create_string(key.to_string()), value);
    }

    /// `AbstractUniversalBuilder.add(T key, DataResult<T> value)` —
    /// `builder.apply2stable((b, v) -> append(key, v, b), value)`.
    fn add_result(&mut self, key: HashCode, value: DataResult<HashCode>) {
        let prev = self.builder.clone();
        self.builder = prev.apply2_stable(
            move |b: &Vec<Pair<HashCode, HashCode>>, v: &HashCode| {
                let mut b = b.clone();
                b.push(Pair::of(key, *v));
                b
            },
            value,
        );
    }

    /// `AbstractUniversalBuilder.add(DataResult<T> key, DataResult<T> value)` —
    /// `builder.ap(key.apply2stable((k, v) -> b -> append(k, v, b), value))`.
    #[allow(clippy::type_complexity)] // the curried Fn1 mirrors Java's `Function<M, M>` curry
    fn add_result_result(&mut self, key: DataResult<HashCode>, value: DataResult<HashCode>) {
        let fr = key.apply2_stable(
            |k: &HashCode, v: &HashCode| {
                let (k, v) = (*k, *v);
                let append: Fn1<Vec<Pair<HashCode, HashCode>>, Vec<Pair<HashCode, HashCode>>> =
                    Arc::new(move |b: &Vec<Pair<HashCode, HashCode>>| {
                        let mut b = b.clone();
                        b.push(Pair::of(k, v));
                        b
                    });
                append
            },
            value,
        );
        let prev = self.builder.clone();
        self.builder = prev.ap(fr);
    }

    /// `AbstractUniversalBuilder.add(String key, DataResult<T> value)` — the
    /// interface default `add(ops.createString(key), value)`.
    fn add_string_result(&mut self, key: &str, value: DataResult<HashCode>) {
        self.add_result(self.ops.create_string(key.to_string()), value);
    }

    /// `AbstractBuilder.withErrorsFrom(DataResult<?>)` —
    /// `builder.flatMap(v -> result.map(r -> v))`.
    fn with_errors_from(&mut self, result: &DataResult<()>) {
        let r = result.clone();
        self.builder = self.builder.clone().flat_map(|v| r.map(|_| v));
    }

    /// `AbstractBuilder.setLifecycle(Lifecycle)`.
    fn set_lifecycle(&mut self, lifecycle: Lifecycle) {
        self.builder = self.builder.clone().set_lifecycle(lifecycle);
    }

    /// `AbstractBuilder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        let builder = self.builder.clone();
        self.builder = builder.map_error(on_error);
    }

    /// `AbstractBuilder.build(T prefix)` — `builder.flatMap(b -> build(b,
    /// prefix))`, then reset. `build(List, prefix)` hashes the accumulated
    /// pairs; the Java `assert isEmpty(prefix)` is not evaluated in pinned
    /// Paper's runtime (no `-ea`), so a non-empty prefix is ignored (see the
    /// module doc and `mapBuilder_nonempty_prefix` golden).
    fn build(&mut self, prefix: Option<HashCode>) -> DataResult<HashCode> {
        let _ = prefix;
        let prev = self.builder.clone();
        let result = prev.flat_map(|b| DataResult::success(self.ops.hash_map(&b)));
        self.builder = DataResult::success_with_lifecycle(Vec::new(), Lifecycle::stable());
        result
    }
}

/// `HashOps.ListHashBuilder` — `AbstractListBuilder<HashCode, Hasher>`:
/// accumulates value bytes into a hasher, then finalizes with the list end tag.
#[derive(Debug)]
struct ListHashBuilder {
    ops: HashOps,
    /// `AbstractListBuilder.builder` — `DataResult.success(initBuilder(),
    /// Lifecycle.stable())`.
    builder: DataResult<Hasher>,
}

impl ListHashBuilder {
    /// `new ListHashBuilder(HashOps.this)` — `initBuilder()` returns
    /// `newHasher().putByte(4)`.
    fn new(ops: HashOps) -> Self {
        ListHashBuilder {
            ops,
            builder: DataResult::success_with_lifecycle(ops.new_list_hasher(), Lifecycle::stable()),
        }
    }
}

impl ListBuilder for ListHashBuilder {
    type Output = HashCode;

    /// `AbstractListBuilder.add(T value)` — `append(hasher, value)` appends the
    /// value's bytes.
    fn add(&mut self, value: HashCode) {
        let prev = self.builder.clone();
        self.builder = prev.map_owned(|mut h| {
            h.put_bytes(&value.as_bytes());
            h
        });
    }

    /// `AbstractListBuilder.add(DataResult<T> value)` —
    /// `builder.apply2stable(this::append, value)`.
    fn add_result(&mut self, value: DataResult<HashCode>) {
        let prev = self.builder.clone();
        self.builder = prev.apply2_stable(
            |h: &Hasher, v: &HashCode| {
                let mut h = h.clone();
                h.put_bytes(&v.as_bytes());
                h
            },
            value,
        );
    }

    /// `AbstractListBuilder.withErrorsFrom(DataResult<?>)`.
    fn with_errors_from(&mut self, result: &DataResult<()>) {
        let r = result.clone();
        self.builder = self.builder.clone().flat_map(|v| r.map(|_| v));
    }

    /// `AbstractListBuilder.mapError(UnaryOperator<String>)`.
    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        let builder = self.builder.clone();
        self.builder = builder.map_error(on_error);
    }

    /// `AbstractListBuilder.build(T prefix)` — `build(hasher, prefix)` appends
    /// the list end tag and hashes; the Java `assert prefix.equals(empty)` is
    /// not evaluated in pinned Paper's runtime (no `-ea`), so a non-empty
    /// prefix is ignored (see the module doc and `listBuilder_nonempty_prefix`
    /// golden).
    fn build(&mut self, _prefix: HashCode) -> DataResult<HashCode> {
        let prev = self.builder.clone();
        let result = prev.flat_map(|mut h| {
            h.put_byte(TAG_LIST_END);
            DataResult::success(h.hash())
        });
        self.builder =
            DataResult::success_with_lifecycle(self.ops.new_list_hasher(), Lifecycle::stable());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::number::Number;

    /// The `HashOps.CRC32C_INSTANCE` used by Paper.
    fn ops() -> HashOps {
        HashOps::crc32c_instance()
    }

    /// `HashCode.padToLong()` — compare against the unsigned golden values.
    fn pad(h: HashCode) -> u64 {
        h.pad_to_long()
    }

    /// CRC-32C of a raw payload, via the underlying `crc32c` crate — an
    /// independent check of HashOps's byte layout.
    fn crc(payload: &[u8]) -> u64 {
        crc32c::crc32c(payload) as u64
    }

    // Golden values were produced by running the pinned Java `HashOps`
    // (Paper 26.2, OpenJDK 25) and printing `HashCode.padToLong()` as unsigned;
    // see the issue #205 port notes.
    #[test]
    fn golden_empty_and_booleans_from_java() {
        assert_eq!(pad(ops().empty()), 2685849682);
        assert_eq!(pad(ops().empty_map()), 3312760008);
        assert_eq!(pad(ops().empty_list()), 2316960274);
        assert_eq!(pad(ops().create_boolean(true)), 3275148994);
        assert_eq!(pad(ops().create_boolean(false)), 828198337);
    }

    #[test]
    fn golden_numerics_from_java() {
        let o = ops();
        assert_eq!(pad(o.create_byte(1)), 1791337955);
        assert_eq!(pad(o.create_byte(-1)), 903050673);
        assert_eq!(pad(o.create_short(300)), 1256850806);
        assert_eq!(pad(o.create_short(-300)), 1041873889);
        assert_eq!(pad(o.create_int(0)), 2148892068);
        assert_eq!(pad(o.create_int(1)), 1565579036);
        assert_eq!(pad(o.create_int(-1)), 932039068);
        assert_eq!(pad(o.create_int(i32::MAX)), 3044799204);
        assert_eq!(pad(o.create_int(i32::MIN)), 48449244);
        assert_eq!(pad(o.create_long(0)), 4155001980);
        assert_eq!(pad(o.create_long(1)), 3197383003);
        assert_eq!(pad(o.create_long(-1)), 870823217);
        assert_eq!(pad(o.create_long(i64::MAX)), 2970717769);
        assert_eq!(pad(o.create_long(i64::MIN)), 1969125124);
        assert_eq!(pad(o.create_float(0.0)), 4030178044);
        assert_eq!(pad(o.create_float(1.0)), 1694772624);
        assert_eq!(pad(o.create_float(-0.5)), 473033745);
        assert_eq!(pad(o.create_float(f32::INFINITY)), 612315180);
        assert_eq!(pad(o.create_float(f32::NAN)), 3681089064);
        assert_eq!(pad(o.create_double(0.0)), 439981597);
        assert_eq!(pad(o.create_double(1.0)), 810798582);
        assert_eq!(pad(o.create_double(std::f64::consts::PI)), 1590633341);
        assert_eq!(pad(o.create_double(-1234567890.1234)), 2732521804);
        assert_eq!(pad(o.create_double(f64::INFINITY)), 1898500170);
        assert_eq!(pad(o.create_double(f64::NAN)), 3963360242);
    }

    #[test]
    fn golden_strings_from_java() {
        let o = ops();
        assert_eq!(pad(o.create_string(String::new())), 1615905556);
        assert_eq!(pad(o.create_string("hello".to_string())), 773640809);
        assert_eq!(pad(o.create_string("héllo".to_string())), 2033959610);
        assert_eq!(pad(o.create_string("😀".to_string())), 1515787531);
        assert_eq!(
            pad(o.create_string("minecraft:stone".to_string())),
            1992329975
        );
    }

    #[test]
    fn golden_create_numeric_dispatch_from_java() {
        let o = ops();
        // The primitive creators are reached through createNumeric's switch.
        assert_eq!(pad(o.create_numeric(Number::Byte(1))), 1791337955); // == createByte(1)
        assert_eq!(pad(o.create_numeric(Number::Short(300))), 1256850806); // == createShort(300)
        assert_eq!(pad(o.create_numeric(Number::Int(42))), 504572232);
        assert_eq!(pad(o.create_numeric(Number::Long(42))), 720099185);
        assert_eq!(pad(o.create_numeric(Number::Float(1.5))), 2584996756);
        assert_eq!(pad(o.create_numeric(Number::Double(1.5))), 2907115086);
    }

    #[test]
    fn golden_lists_from_java() {
        let o = ops();
        assert_eq!(pad(o.create_list(vec![])), 2316960274); // == emptyList
        assert_eq!(
            pad(o.create_list(vec![o.create_int(1), o.create_int(2), o.create_int(3)])),
            3115035888
        );
        // Nested lists hash each element's bytes in place.
        assert_eq!(
            pad(o.create_list(vec![
                o.create_list(vec![o.create_int(1)]),
                o.create_list(vec![o.create_int(2)]),
            ])),
            3082248887
        );
    }

    #[test]
    fn golden_byte_int_long_arrays_from_java() {
        let o = ops();
        assert_eq!(pad(o.create_byte_list(&[1, 2, 3])), 3903615044);
        assert_eq!(pad(o.create_byte_list(&[])), 1537857916);
        assert_eq!(pad(o.create_int_list(vec![1, 2, 3])), 1392957496);
        assert_eq!(pad(o.create_int_list(vec![])), 747749951);
        assert_eq!(pad(o.create_long_list(vec![1, 2, 3])), 3517396747);
        assert_eq!(pad(o.create_long_list(vec![])), 3941564966);
    }

    #[test]
    fn golden_maps_from_java() {
        let o = ops();
        let key = |s: &str| o.create_string(s.to_string());
        assert_eq!(pad(o.create_map(vec![])), 3312760008); // == emptyMap

        // {b:2, a:1, c:3} as ints — order-independent because HashOps sorts.
        let entries = |order: [&str; 3]| {
            order
                .into_iter()
                .map(|k| match k {
                    "a" => Pair::of(key("a"), o.create_int(1)),
                    "b" => Pair::of(key("b"), o.create_int(2)),
                    _ => Pair::of(key("c"), o.create_int(3)),
                })
                .collect::<Vec<_>>()
        };
        let abc = entries(["a", "b", "c"]);
        let bca = entries(["b", "c", "a"]);
        assert_eq!(pad(o.create_map(abc.clone())), 1411621057);
        assert_eq!(pad(o.create_map(bca)), 1411621057); // sorted, not insertion

        assert_eq!(
            pad(o.create_map(vec![
                Pair::of(key("x"), o.create_int(9)),
                Pair::of(key("y"), o.create_int(8)),
            ])),
            61798629
        );
        // A map value that is itself a list.
        assert_eq!(
            pad(o.create_map(vec![
                Pair::of(key("k"), o.create_list(vec![o.create_int(1)])),
                Pair::of(key("j"), o.create_list(vec![o.create_int(2)])),
            ])),
            2343643249
        );
    }

    #[test]
    fn golden_duplicate_key_stream_from_java() {
        let o = ops();
        // The stream overload keeps duplicate keys (stable sort); a Java Map
        // would collapse them, so this distinguishes the two createMap paths.
        assert_eq!(
            pad(o.create_map(vec![
                Pair::of(o.create_string("a".to_string()), o.create_int(1)),
                Pair::of(o.create_string("a".to_string()), o.create_int(2)),
            ])),
            4283546196
        );
    }

    #[test]
    fn golden_merge_ops_from_java() {
        let o = ops();
        let k = o.create_string("k".to_string());
        let a = o.create_string("a".to_string());
        let b = o.create_string("b".to_string());
        // mergeToList on an empty prefix.
        assert_eq!(
            pad(*o
                .merge_to_list(&o.empty(), o.create_int(5))
                .result()
                .unwrap()),
            447358666
        );
        // mergeToList(List) on an empty prefix.
        assert_eq!(
            pad(*o
                .merge_to_list_many(&o.empty(), vec![o.create_int(1), o.create_int(2)])
                .result()
                .unwrap()),
            2921006628 // == listBuilder([1,2])
        );
        // mergeToMap on an empty prefix.
        assert_eq!(
            pad(*o
                .merge_to_map(&o.empty(), k, o.create_int(1))
                .result()
                .unwrap()),
            2125678401
        );
        // mergeToMap(Map) on an empty prefix.
        assert_eq!(
            pad(*o
                .merge_to_map_many(
                    &o.empty(),
                    vec![Pair::of(a, o.create_int(1)), Pair::of(b, o.create_int(2)),]
                )
                .result()
                .unwrap()),
            837818764
        );
    }

    #[test]
    fn merge_errors_on_nonempty_prefix() {
        let o = ops();
        let k = o.create_string("k".to_string());
        for r in [
            o.merge_to_list(&o.create_int(5), o.create_int(6)),
            o.merge_to_list_many(&o.create_int(5), vec![o.create_int(6)]),
            o.merge_to_map(&o.create_int(5), k, o.create_int(1)),
            o.merge_to_map_many(&o.create_int(5), vec![Pair::of(k, o.create_int(1))]),
        ] {
            assert_eq!(r.error_ref().unwrap().message(), "Unsupported operation");
        }
        // MapLike form routes through merge_to_map_like, erroring too.
        let map_like = vec![Pair::of(k, o.create_int(1))];
        let r = o.merge_to_map_like(&o.create_int(5), &map_like);
        assert_eq!(r.error_ref().unwrap().message(), "Unsupported operation");
        // ... and succeeding on an empty prefix.
        let ok = o.merge_to_map_like(&o.empty(), &map_like);
        assert_eq!(pad(*ok.result().unwrap()), 2125678401);
    }

    #[test]
    fn golden_builders_from_java() {
        let o = ops();
        // mapBuilder with a->1, b->2, c->3 == createMap({a:1,b:2,c:3}).
        {
            let mut rb = o.map_builder();
            rb.add_string("a", o.create_int(1));
            rb.add_string("b", o.create_int(2));
            rb.add_string("c", o.create_int(3));
            let r = rb.build(Some(o.empty()));
            assert_eq!(pad(*r.result().unwrap()), 1411621057);
        }
        // listBuilder [1,2].
        {
            let mut lb = o.list_builder();
            lb.add(o.create_int(1));
            lb.add(o.create_int(2));
            let r = lb.build(o.empty());
            assert_eq!(pad(*r.result().unwrap()), 2921006628);
        }
    }

    #[test]
    fn builders_ignore_nonempty_prefix() {
        // Pinned Paper runs without `-ea`, so its `assert prefix.equals(empty)`
        // is not evaluated and the build prefix is silently ignored: the golden
        // hashes are the entry-only ones.
        let o = ops();
        let mut rb = o.map_builder();
        rb.add_string("a", o.create_int(1));
        let r = rb.build(Some(o.create_int(9)));
        assert_eq!(pad(*r.result().unwrap()), 4186094590);

        let mut lb = o.list_builder();
        lb.add(o.create_int(1));
        let r = lb.build(o.create_int(9));
        assert_eq!(pad(*r.result().unwrap()), 3295012089);
    }

    #[test]
    fn builder_add_result_accumulates_errors() {
        let o = ops();
        let mut rb = o.map_builder();
        rb.add_string("a", o.create_int(1));
        rb.add_string_result("bad", DataResult::error("field boom"));
        let r = rb.build(Some(o.empty()));
        assert!(r.result().is_none());
        assert_eq!(r.error_ref().unwrap().message(), "field boom");

        let mut lb = o.list_builder();
        lb.add(o.create_int(1));
        lb.add_result(DataResult::error("item boom"));
        let r = lb.build(o.empty());
        assert!(r.result().is_none());
        assert_eq!(r.error_ref().unwrap().message(), "item boom");
    }

    #[test]
    fn unsupported_decode_ops_error() {
        let o = ops();
        let e = o.empty();
        // Every decode-side op returns the shared "Unsupported operation" error.
        assert!(o.get(&e, "k").result().is_none());
        assert!(
            o.get_generic(&e, &o.create_string("k".to_string()))
                .result()
                .is_none()
        );
        assert!(o.get_number_value(&e).result().is_none());
        assert!(o.get_boolean_value(&e).result().is_none());
        assert!(o.get_string_value(&e).result().is_none());
        assert!(o.get_map_values(&e).result().is_none());
        assert!(o.get_map_entries(&e).result().is_none());
        assert!(o.get_stream(&e).result().is_none());
        assert!(o.get_list(&e).result().is_none());
        assert!(o.get_map(&e).result().is_none());
        assert!(o.get_byte_buffer(&e).result().is_none());
        assert!(o.get_int_stream(&e).result().is_none());
        assert!(o.get_long_stream(&e).result().is_none());
        // The error message is the shared "Unsupported operation".
        assert_eq!(
            o.get(&e, "k").error_ref().unwrap().message(),
            "Unsupported operation"
        );
    }

    #[test]
    fn get_number_value_or_returns_default() {
        let o = ops();
        match o.get_number_value_or(&o.empty(), Number::Double(42.0)) {
            Number::Double(v) => assert_eq!(v, 42.0),
            other => panic!("expected Double default, got {other:?}"),
        }
        match o.get_number_value_or(&o.empty(), Number::Int(42)) {
            Number::Int(v) => assert_eq!(v, 42),
            other => panic!("expected Int default, got {other:?}"),
        }
    }

    #[test]
    fn identity_ops_return_input() {
        let o = ops();
        let e = o.empty();
        assert_eq!(o.remove(e, "k"), e);
        assert_eq!(o.set(&e, "k", o.create_int(1)), e);
        assert_eq!(o.update(&e, "k", |h| h), e);
        assert_eq!(
            o.update_generic(&e, &o.create_string("k".to_string()), |h| h),
            e
        );
    }

    #[test]
    fn convert_to_panics() {
        let o = ops();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            o.convert_to(&o, &o.empty())
        }));
        assert!(
            result.is_err(),
            "convertTo must throw UnsupportedOperationException"
        );
    }

    #[test]
    fn to_string_matches_java() {
        assert_eq!(ops().to_string(), "Hash Hashing.crc32c()");
    }

    // ---- Counterfactual byte-layout tests (independent of the golden run) ----

    #[test]
    fn byte_layout_little_endian() {
        let o = ops();
        // createInt(1): TAG_INT(8) + i32 LE.
        assert_eq!(pad(o.create_int(1)), crc(&[8, 1, 0, 0, 0]));
        assert_eq!(pad(o.create_int(-1)), crc(&[8, 0xff, 0xff, 0xff, 0xff]));
        // createShort(300): TAG_SHORT(7) + i16 LE.
        assert_eq!(pad(o.create_short(300)), crc(&[7, 0x2c, 0x01]));
        // createLong(1): TAG_LONG(9) + i64 LE.
        assert_eq!(pad(o.create_long(1)), crc(&[9, 1, 0, 0, 0, 0, 0, 0, 0]));
        // createFloat(1.0): TAG_FLOAT(10) + raw bits LE.
        assert_eq!(pad(o.create_float(1.0)), crc(&[10, 0, 0, 0x80, 0x3f]));
        // createDouble(1.0): TAG_DOUBLE(11) + raw bits LE.
        assert_eq!(
            pad(o.create_double(1.0)),
            crc(&[11, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f])
        );
        // float NaN raw bits 0x7fc00000 -> LE bytes 00 00 c0 7f.
        assert_eq!(pad(o.create_float(f32::NAN)), crc(&[10, 0, 0, 0xc0, 0x7f]));
        // double NaN raw bits 0x7ff8000000000000 -> LE bytes.
        assert_eq!(
            pad(o.create_double(f64::NAN)),
            crc(&[11, 0, 0, 0, 0, 0, 0, 0xf8, 0x7f])
        );
        // createByte(-1): TAG_BYTE(6) + the byte.
        assert_eq!(pad(o.create_byte(-1)), crc(&[6, 0xff]));
    }

    #[test]
    fn string_uses_utf16_code_unit_length_not_bytes() {
        let o = ops();
        let s = "😀";
        // "😀" is U+1F600: 4 UTF-8 bytes but 2 UTF-16 code units (a surrogate
        // pair). Java prefixes putInt(String.length()) = 2 code units.
        let payload_code_units = [12, 2, 0, 0, 0, 0x3d, 0xd8, 0x00, 0xde];
        assert_eq!(
            pad(o.create_string(s.to_string())),
            crc(&payload_code_units)
        );

        // A byte-length implementation would prefix 4 and feed the UTF-8 bytes.
        let payload_bytes = [12, 4, 0, 0, 0, 0xf0, 0x9f, 0x98, 0x80];
        assert_ne!(pad(o.create_string(s.to_string())), crc(&payload_bytes));
        // And it must equal the Java golden.
        assert_eq!(pad(o.create_string(s.to_string())), 1515787531);
    }

    #[test]
    fn map_sorting_is_observable() {
        let o = ops();
        let a = o.create_string("a".to_string());
        let b = o.create_string("b".to_string());
        let c = o.create_string("c".to_string());
        // HashOps sorts entries by key-then-value padToLong. Feeding entries in
        // unsorted key order must produce the sorted payload.
        let key_cmp = |x: &Pair<HashCode, HashCode>, y: &Pair<HashCode, HashCode>| {
            x.first
                .pad_to_long()
                .cmp(&y.first.pad_to_long())
                .then_with(|| x.second.pad_to_long().cmp(&y.second.pad_to_long()))
        };
        let insert_order = vec![
            Pair::of(b, o.create_int(2)),
            Pair::of(a, o.create_int(1)),
            Pair::of(c, o.create_int(3)),
        ];
        let mut sorted = insert_order.clone();
        sorted.sort_by(key_cmp);
        let mut sorted_payload = vec![TAG_MAP_START];
        for e in &sorted {
            sorted_payload.extend_from_slice(&e.first.as_bytes());
            sorted_payload.extend_from_slice(&e.second.as_bytes());
        }
        sorted_payload.push(TAG_MAP_END);
        assert_eq!(
            pad(o.create_map(insert_order.clone())),
            crc(&sorted_payload)
        );

        // The insertion-ordered (unsorted) payload differs — proving the sort is
        // load-bearing, not incidental.
        let mut insertion_payload = vec![TAG_MAP_START];
        for e in &insert_order {
            insertion_payload.extend_from_slice(&e.first.as_bytes());
            insertion_payload.extend_from_slice(&e.second.as_bytes());
        }
        insertion_payload.push(TAG_MAP_END);
        assert_ne!(pad(o.create_map(insert_order)), crc(&insertion_payload));
    }

    #[test]
    fn array_layouts() {
        let o = ops();
        // byteList [1,2,3]: TAG_BYTE_ARRAY_START(14) + raw + END(15).
        assert_eq!(pad(o.create_byte_list(&[1, 2, 3])), crc(&[14, 1, 2, 3, 15]));
        // intList [1,2,3]: TAG_INT_ARRAY_START(16) + ints LE + END(17).
        assert_eq!(
            pad(o.create_int_list(vec![1, 2, 3])),
            crc(&[16, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 17])
        );
        // longList [1,2,3].
        let mut payload = vec![TAG_LONG_ARRAY_START];
        for v in [1i64, 2, 3] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        payload.push(TAG_LONG_ARRAY_END);
        assert_eq!(pad(o.create_long_list(vec![1, 2, 3])), crc(&payload));
    }

    #[test]
    fn empty_payload_constants() {
        // The public constants are the exact payloads Java exposes.
        assert_eq!(EMPTY_MAP_PAYLOAD, [TAG_MAP_START, TAG_MAP_END]);
        assert_eq!(EMPTY_LIST_PAYLOAD, [TAG_LIST_START, TAG_LIST_END]);
        assert_eq!(pad(ops().empty_map()), crc(&EMPTY_MAP_PAYLOAD));
        assert_eq!(pad(ops().empty_list()), crc(&EMPTY_LIST_PAYLOAD));
    }

    #[test]
    fn hash_code_value_semantics() {
        let a = HashCode(u32::from_le_bytes([1, 2, 3, 4]));
        let b = HashCode(u32::from_le_bytes([1, 2, 3, 4]));
        let c = HashCode(u32::from_le_bytes([1, 2, 3, 5]));
        assert_eq!(a, b);
        assert_ne!(a, c);
        // asBytes is little-endian, padToLong zero-extends, asInt reinterprets.
        assert_eq!(a.as_bytes(), [1, 2, 3, 4]);
        assert_eq!(a.pad_to_long(), 0x04030201);
        assert_eq!(a.as_int(), 0x04030201);
    }

    #[test]
    fn crc32c_instance_is_deterministic() {
        let o1 = HashOps::crc32c_instance();
        let o2 = HashOps::crc32c_instance();
        let v = o1.create_map(vec![Pair::of(
            o1.create_string("a".to_string()),
            o1.create_int(1),
        )]);
        assert_eq!(
            v,
            o2.create_map(vec![Pair::of(
                o2.create_string("a".to_string()),
                o2.create_int(1)
            ),])
        );
    }
}
