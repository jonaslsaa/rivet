//! Port of `net.minecraft.nbt.CompoundTag` — `final class` wrapping a map.
//!
//! Java uses an `Object2ObjectOpenHashMap<String, Tag>` (fastutil), which
//! iterates in fastutil hash order — matching that order byte-for-byte would
//! require a Java-identical hasher (DECISIONS.md D12, 2026-08-07). Rivet instead
//! stores tags in an insertion-ordered `IndexMap` (DECISIONS.md D12): the reader
//! preserves the on-disk field order and the writer emits that same order, so
//! any binary that is *read* by Rivet round-trips byte-for-byte — including
//! Paper's own chunk NBT. The divergence from Java is confined to hand-built
//! compounds (SNBT → binary) where Rust's put order differs from fastutil hash
//! order; that is the `compound_key_order` divergence counted in PARITY.md.
//! Deterministic across processes (no randomized seed), so golden tests pin it.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::double_tag::DoubleTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::nbt_ops::NbtOps;
use crate::short_tag::ShortTag;
use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::string_tag::StringTag;
use crate::tag::Tag;
use crate::tag_visitor::TagVisitor;
use indexmap::IndexMap;
use rivet_serialization::{Codec, MapCodec, map_codec, map_encoder};
use std::sync::Arc;

pub const SELF_SIZE_IN_BYTES: i32 = 48;
pub const MAP_ENTRY_SIZE_IN_BYTES: i32 = 32;

/// `CompoundTag`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompoundTag {
    pub tags: IndexMap<String, Tag>,
}

impl CompoundTag {
    /// `new CompoundTag()`.
    pub fn new() -> Self {
        CompoundTag {
            tags: IndexMap::new(),
        }
    }

    /// `new CompoundTag(Map)`.
    pub fn with_map(tags: IndexMap<String, Tag>) -> Self {
        CompoundTag { tags }
    }

    /// `CompoundTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        let mut size = 48;
        for (key, value) in self.tags.iter() {
            size += 28 + 2 * key.encode_utf16().count() as i32;
            size += 36;
            size += value.size_in_bytes();
        }
        size
    }

    /// `CompoundTag.keySet()`.
    pub fn key_set(&self) -> impl Iterator<Item = &String> {
        self.tags.keys()
    }

    /// `CompoundTag.entrySet()`.
    pub fn entry_set(&self) -> impl Iterator<Item = (&String, &Tag)> {
        self.tags.iter()
    }

    /// `CompoundTag.values()`.
    pub fn values(&self) -> impl Iterator<Item = &Tag> {
        self.tags.values()
    }

    /// `CompoundTag.forEach(BiConsumer<String, Tag>)`.
    ///
    /// Note: Java hands the consumer the live child tag, so mutations through
    /// it persist; mirrored here with `&mut self` and `&mut Tag`.
    pub fn for_each<F: FnMut(&str, &mut Tag)>(&mut self, mut consumer: F) {
        for (k, v) in self.tags.iter_mut() {
            consumer(k, v);
        }
    }

    /// `CompoundTag.size()`.
    pub fn size(&self) -> usize {
        self.tags.len()
    }

    /// `CompoundTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// `CompoundTag.put(String, Tag)`.
    pub fn put(&mut self, name: String, tag: Tag) -> Option<Tag> {
        self.tags.insert(name, tag)
    }

    /// `CompoundTag.putByte(String, byte)`.
    pub fn put_byte(&mut self, name: &str, value: i8) {
        self.tags
            .insert(name.to_string(), Tag::Byte(ByteTag::value_of(value)));
    }

    /// `CompoundTag.putShort(String, short)`.
    pub fn put_short(&mut self, name: &str, value: i16) {
        self.tags
            .insert(name.to_string(), Tag::Short(ShortTag::value_of(value)));
    }

    /// `CompoundTag.putInt(String, int)`.
    pub fn put_int(&mut self, name: &str, value: i32) {
        self.tags
            .insert(name.to_string(), Tag::Int(IntTag::value_of(value)));
    }

    /// `CompoundTag.putLong(String, long)`.
    pub fn put_long(&mut self, name: &str, value: i64) {
        self.tags
            .insert(name.to_string(), Tag::Long(LongTag::value_of(value)));
    }

    /// `CompoundTag.putFloat(String, float)`.
    pub fn put_float(&mut self, name: &str, value: f32) {
        self.tags
            .insert(name.to_string(), Tag::Float(FloatTag::value_of(value)));
    }

    /// `CompoundTag.putDouble(String, double)`.
    pub fn put_double(&mut self, name: &str, value: f64) {
        self.tags
            .insert(name.to_string(), Tag::Double(DoubleTag::value_of(value)));
    }

    /// `CompoundTag.putString(String, String)`.
    pub fn put_string(&mut self, name: &str, value: &str) {
        self.tags.insert(
            name.to_string(),
            Tag::String(StringTag::value_of(value.to_string())),
        );
    }

    /// `CompoundTag.putByteArray(String, byte[])`.
    pub fn put_byte_array(&mut self, name: &str, value: Vec<i8>) {
        self.tags
            .insert(name.to_string(), Tag::ByteArray(ByteArrayTag::new(value)));
    }

    /// `CompoundTag.putIntArray(String, int[])`.
    pub fn put_int_array(&mut self, name: &str, value: Vec<i32>) {
        self.tags
            .insert(name.to_string(), Tag::IntArray(IntArrayTag::new(value)));
    }

    /// `CompoundTag.putLongArray(String, long[])`.
    pub fn put_long_array(&mut self, name: &str, value: Vec<i64>) {
        self.tags
            .insert(name.to_string(), Tag::LongArray(LongArrayTag::new(value)));
    }

    /// `CompoundTag.putBoolean(String, boolean)`.
    pub fn put_boolean(&mut self, name: &str, value: bool) {
        self.tags
            .insert(name.to_string(), Tag::Byte(ByteTag::value_of_bool(value)));
    }

    /// `CompoundTag.get(String)`.
    pub fn get(&self, name: &str) -> Option<&Tag> {
        self.tags.get(name)
    }

    /// `CompoundTag.contains(String)`.
    pub fn contains(&self, name: &str) -> bool {
        self.tags.contains_key(name)
    }

    /// `CompoundTag.getByte(String)`.
    pub fn get_byte(&self, name: &str) -> Option<i8> {
        self.get_optional(name).and_then(|t| t.as_byte())
    }

    /// `CompoundTag.getByteOr(String, byte)`.
    pub fn get_byte_or(&self, name: &str, default_value: i8) -> i8 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.byte_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getShort(String)`.
    pub fn get_short(&self, name: &str) -> Option<i16> {
        self.get_optional(name).and_then(|t| t.as_short())
    }

    /// `CompoundTag.getShortOr(String, short)`.
    pub fn get_short_or(&self, name: &str, default_value: i16) -> i16 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.short_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getInt(String)`.
    pub fn get_int(&self, name: &str) -> Option<i32> {
        self.get_optional(name).and_then(|t| t.as_int())
    }

    /// `CompoundTag.getIntOr(String, int)`.
    pub fn get_int_or(&self, name: &str, default_value: i32) -> i32 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.int_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getLong(String)`.
    pub fn get_long(&self, name: &str) -> Option<i64> {
        self.get_optional(name).and_then(|t| t.as_long())
    }

    /// `CompoundTag.getLongOr(String, long)`.
    pub fn get_long_or(&self, name: &str, default_value: i64) -> i64 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.long_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getFloat(String)`.
    pub fn get_float(&self, name: &str) -> Option<f32> {
        self.get_optional(name).and_then(|t| t.as_float())
    }

    /// `CompoundTag.getFloatOr(String, float)`.
    pub fn get_float_or(&self, name: &str, default_value: f32) -> f32 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.float_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getDouble(String)`.
    pub fn get_double(&self, name: &str) -> Option<f64> {
        self.get_optional(name).and_then(|t| t.as_double())
    }

    /// `CompoundTag.getDoubleOr(String, double)`.
    pub fn get_double_or(&self, name: &str, default_value: f64) -> f64 {
        match self.tags.get(name) {
            Some(t) => match crate::numeric_tag::NumericTag::try_from(t) {
                Ok(n) => n.double_value(),
                Err(_) => default_value,
            },
            None => default_value,
        }
    }

    /// `CompoundTag.getString(String)`.
    pub fn get_string(&self, name: &str) -> Option<&String> {
        self.get_optional(name).and_then(|t| t.as_string())
    }

    /// `CompoundTag.getStringOr(String, String)`.
    pub fn get_string_or<'a>(&'a self, name: &str, default_value: &'a str) -> &'a str {
        match self.tags.get(name) {
            Some(Tag::String(t)) => &t.value,
            _ => default_value,
        }
    }

    /// `CompoundTag.getByteArray(String)`.
    pub fn get_byte_array(&self, name: &str) -> Option<&Vec<i8>> {
        match self.tags.get(name) {
            Some(Tag::ByteArray(t)) => Some(t.get_as_byte_array()),
            _ => None,
        }
    }

    /// `CompoundTag.getIntArray(String)`.
    pub fn get_int_array(&self, name: &str) -> Option<&Vec<i32>> {
        match self.tags.get(name) {
            Some(Tag::IntArray(t)) => Some(t.get_as_int_array()),
            _ => None,
        }
    }

    /// `CompoundTag.getLongArray(String)`.
    pub fn get_long_array(&self, name: &str) -> Option<&Vec<i64>> {
        match self.tags.get(name) {
            Some(Tag::LongArray(t)) => Some(t.get_as_long_array()),
            _ => None,
        }
    }

    /// `CompoundTag.getCompound(String)`.
    pub fn get_compound(&self, name: &str) -> Option<&CompoundTag> {
        match self.tags.get(name) {
            Some(Tag::Compound(t)) => Some(t),
            _ => None,
        }
    }

    /// `CompoundTag.getCompoundOrEmpty(String)`.
    ///
    /// Note: Java returns the live child; this value form clones it (Rust
    /// cannot return a reference from `&self`). Use
    /// [`get_compound_or_empty_mut`](Self::get_compound_or_empty_mut) when the
    /// returned tag will be mutated, so the mutation lands in the parent.
    pub fn get_compound_or_empty(&self, name: &str) -> CompoundTag {
        match self.get_compound(name) {
            Some(t) => t.clone(),
            None => CompoundTag::new(),
        }
    }

    /// `CompoundTag.getCompoundOrEmpty(String)` — mutation form. Returns the
    /// live child when present, else inserts and returns a fresh empty
    /// `CompoundTag`, so in-place mutation (`get_compound_or_empty_mut(k).put
    /// (...)` on the returned tag) persists in this parent.
    pub fn get_compound_or_empty_mut(&mut self, name: &str) -> &mut CompoundTag {
        if !matches!(self.tags.get(name), Some(Tag::Compound(_))) {
            self.tags
                .insert(name.to_string(), Tag::Compound(CompoundTag::new()));
        }
        match self.tags.get_mut(name) {
            Some(Tag::Compound(t)) => t,
            _ => unreachable!("compound just inserted or already present"),
        }
    }

    /// `CompoundTag.getList(String)`.
    pub fn get_list(&self, name: &str) -> Option<&ListTag> {
        match self.tags.get(name) {
            Some(Tag::List(t)) => Some(t),
            _ => None,
        }
    }

    /// `CompoundTag.getListOrEmpty(String)`.
    ///
    /// Note: Java returns the live child; this value form clones it (Rust
    /// cannot return a reference from `&self`). Use
    /// [`get_list_or_empty_mut`](Self::get_list_or_empty_mut) when the returned
    /// list will be mutated, so the mutation lands in the parent.
    pub fn get_list_or_empty(&self, name: &str) -> ListTag {
        match self.get_list(name) {
            Some(t) => t.clone(),
            None => ListTag::new(),
        }
    }

    /// `CompoundTag.getListOrEmpty(String)` — mutation form. Returns the live
    /// child when present, else inserts and returns a fresh empty `ListTag`, so
    /// in-place mutation (`get_list_or_empty_mut(k).add(...)` on the returned
    /// list) persists in this parent.
    pub fn get_list_or_empty_mut(&mut self, name: &str) -> &mut ListTag {
        if !matches!(self.tags.get(name), Some(Tag::List(_))) {
            self.tags
                .insert(name.to_string(), Tag::List(ListTag::new()));
        }
        match self.tags.get_mut(name) {
            Some(Tag::List(t)) => t,
            _ => unreachable!("list just inserted or already present"),
        }
    }

    /// `CompoundTag.getBoolean(String)`.
    pub fn get_boolean(&self, name: &str) -> Option<bool> {
        self.get_optional(name).and_then(|t| t.as_boolean())
    }

    /// `CompoundTag.getBooleanOr(String, boolean)`.
    pub fn get_boolean_or(&self, name: &str, default_value: bool) -> bool {
        self.get_byte_or(name, if default_value { 1 } else { 0 }) != 0
    }

    /// `CompoundTag.remove(String)`.
    ///
    /// `shift_remove` preserves the relative insertion order of the remaining
    /// keys (matching Java's removal, which never reorders the map).
    pub fn remove(&mut self, name: &str) -> Option<Tag> {
        self.tags.shift_remove(name)
    }

    /// `CompoundTag.shallowCopy()` (package-private).
    ///
    /// Divergence note: Java `new HashMap<>(this.tags)` shares the nested tag
    /// instances, so mutating a child through the shallow copy reflects back
    /// into this tag. Rust's `Tag` enum owns its leaves (no reference sharing),
    /// so `self.tags.clone()` deep-copies the entire tree. The name is kept for
    /// greppability against Java; the read-modify-write callers in `NbtOps`
    /// (`remove`, `mergeToMap`) only `put` top-level keys into the copy, so the
    /// deep copy is behaviorally equivalent there.
    pub fn shallow_copy(&self) -> CompoundTag {
        CompoundTag {
            tags: self.tags.clone(),
        }
    }

    /// `CompoundTag.copy()` — deep copy. Keeps insertion order.
    pub fn copy_tag(&self) -> CompoundTag {
        let mut ret = IndexMap::with_capacity(self.tags.len());
        for (k, v) in self.tags.iter() {
            ret.insert(k.clone(), v.copy_tag());
        }
        CompoundTag { tags: ret }
    }

    /// `CompoundTag.store(String, Codec<T>, T)` — encode `value` and put the
    /// result under `name`. `Codec.encodeStart(NbtOps.INSTANCE,
    /// value).getOrThrow()`; an encode error panics (Java's
    /// `IllegalStateException`).
    pub fn store<A>(&mut self, name: &str, codec: &Arc<dyn Codec<A, NbtOps>>, value: &A)
    where
        A: 'static,
    {
        let ops = NbtOps::instance();
        self.put(
            name.to_string(),
            codec
                .encode_start(&ops, value)
                .get_or_throw("encodeStart")
                .clone(),
        );
    }

    /// `CompoundTag.store(String, Codec<T>, DynamicOps<Tag>, T)`. `NbtOps` is
    /// the only `DynamicOps<Tag>`, so the ops argument is retained purely for
    /// Java signature parity and must be `NbtOps::instance()`.
    pub fn store_with_ops<A>(
        &mut self,
        name: &str,
        codec: &Arc<dyn Codec<A, NbtOps>>,
        _ops: NbtOps,
        value: &A,
    ) where
        A: 'static,
    {
        self.store(name, codec, value);
    }

    /// `CompoundTag.storeNullable(String, Codec<T>, @Nullable T)` — a no-op for
    /// `None`.
    pub fn store_nullable<A>(
        &mut self,
        name: &str,
        codec: &Arc<dyn Codec<A, NbtOps>>,
        value: Option<&A>,
    ) where
        A: 'static,
    {
        if let Some(v) = value {
            self.store(name, codec, v);
        }
    }

    /// `CompoundTag.storeNullable(String, Codec<T>, DynamicOps<Tag>, @Nullable T)`.
    pub fn store_nullable_with_ops<A>(
        &mut self,
        name: &str,
        codec: &Arc<dyn Codec<A, NbtOps>>,
        ops: NbtOps,
        value: Option<&A>,
    ) where
        A: 'static,
    {
        if let Some(v) = value {
            self.store_with_ops(name, codec, ops, v);
        }
    }

    /// `CompoundTag.store(MapCodec<T>, T)` — merge the encoded compound into
    /// this tag.
    pub fn store_map<A>(&mut self, codec: &Arc<dyn MapCodec<A, NbtOps>>, value: &A)
    where
        A: 'static,
    {
        self.store_map_with_ops(codec, NbtOps::instance(), value);
    }

    /// `CompoundTag.store(MapCodec<T>, DynamicOps<Tag>, T)` —
    /// `this.merge((CompoundTag)codec.encoder().encodeStart(ops,
    /// value).getOrThrow())`. The `MapEncoder.encoder()` half builds into a
    /// fresh compressed builder; an encode error panics (Java
    /// `IllegalStateException`) and a non-compound result panics on the
    /// unchecked `(CompoundTag)` cast.
    pub fn store_map_with_ops<A>(
        &mut self,
        codec: &Arc<dyn MapCodec<A, NbtOps>>,
        ops: NbtOps,
        value: &A,
    ) where
        A: 'static,
    {
        let encoder = map_encoder::encoder(Arc::new(map_codec::MapCodecEncoderHalf(codec.clone())));
        let encoded = encoder.encode_start(&ops, value);
        match encoded.get_or_throw("encodeStart") {
            Tag::Compound(c) => self.merge(c),
            other => panic!("CompoundTag.store(MapCodec): expected compound, got {other}"),
        };
    }

    /// `CompoundTag.read(String, Codec<T>)` — parse the tag under `name`,
    /// logging nothing on a partial result.
    ///
    /// Java logs `LOGGER.error("Failed to read field ({}={}): {}", name, tag,
    /// error)` via `resultOrPartial`; rivet-nbt has no logging infrastructure,
    /// so the callback is a no-op (documented divergence — the partial-value
    /// semantics are preserved).
    pub fn read<A>(&self, name: &str, codec: &Arc<dyn Codec<A, NbtOps>>) -> Option<A>
    where
        A: 'static,
    {
        self.read_with_ops(name, codec, NbtOps::instance())
    }

    /// `CompoundTag.read(String, Codec<T>, DynamicOps<Tag>)`.
    pub fn read_with_ops<A>(
        &self,
        name: &str,
        codec: &Arc<dyn Codec<A, NbtOps>>,
        ops: NbtOps,
    ) -> Option<A>
    where
        A: 'static,
    {
        match self.get(name) {
            Some(tag) => codec.parse(&ops, tag).result_or_partial(|_error| {
                // Java's `LOGGER.error("Failed to read field ({}={}): {}", name,
                // tag, error)` is dropped — no logging infra in rivet-nbt.
            }),
            None => None,
        }
    }

    /// `CompoundTag.readQuiet(String, Codec<T>)` — `read` without the (absent)
    /// error logging.
    pub fn read_quiet<A>(&self, name: &str, codec: &Arc<dyn Codec<A, NbtOps>>) -> Option<A>
    where
        A: 'static,
    {
        self.read_quiet_with_ops(name, codec, NbtOps::instance())
    }

    /// `CompoundTag.readQuiet(String, Codec<T>, DynamicOps<Tag>)`.
    pub fn read_quiet_with_ops<A>(
        &self,
        name: &str,
        codec: &Arc<dyn Codec<A, NbtOps>>,
        ops: NbtOps,
    ) -> Option<A>
    where
        A: 'static,
    {
        match self.get(name) {
            Some(tag) => codec.parse(&ops, tag).result_or_partial_silent(),
            None => None,
        }
    }

    /// `CompoundTag.read(MapCodec<T>)` — decode this whole compound as a map.
    pub fn read_map<A>(&self, codec: &Arc<dyn MapCodec<A, NbtOps>>) -> Option<A>
    where
        A: 'static,
    {
        self.read_map_with_ops(codec, NbtOps::instance())
    }

    /// `CompoundTag.read(MapCodec<T>, DynamicOps<Tag>)` —
    /// `codec.decode(ops, ops.getMap(this).getOrThrow()).resultOrPartial(...)`.
    pub fn read_map_with_ops<A>(
        &self,
        codec: &Arc<dyn MapCodec<A, NbtOps>>,
        ops: NbtOps,
    ) -> Option<A>
    where
        A: 'static,
    {
        let map = ops.map_like(self);
        codec
            .decode(&ops, map.as_ref())
            .result_or_partial(|_error| {
                // Java's `LOGGER.error("Failed to read value ({}): {}", this,
                // error)` is dropped — no logging infra in rivet-nbt.
            })
    }

    /// `CompoundTag.readQuiet(MapCodec<T>)` — `readMap` without the (absent)
    /// error logging.
    pub fn read_map_quiet<A>(&self, codec: &Arc<dyn MapCodec<A, NbtOps>>) -> Option<A>
    where
        A: 'static,
    {
        self.read_map_quiet_with_ops(codec, NbtOps::instance())
    }

    /// `CompoundTag.readQuiet(MapCodec<T>, DynamicOps<Tag>)`.
    pub fn read_map_quiet_with_ops<A>(
        &self,
        codec: &Arc<dyn MapCodec<A, NbtOps>>,
        ops: NbtOps,
    ) -> Option<A>
    where
        A: 'static,
    {
        let map = ops.map_like(self);
        codec.decode(&ops, map.as_ref()).result_or_partial_silent()
    }

    /// `CompoundTag.merge(CompoundTag)`.
    pub fn merge(&mut self, other: &CompoundTag) -> &mut CompoundTag {
        for (tag_name, other_tag) in other.tags.iter() {
            if let Some(Tag::Compound(other_compound)) = other.tags.get(tag_name)
                && let Some(Tag::Compound(self_compound)) = self.tags.get_mut(tag_name)
            {
                self_compound.merge(other_compound);
                continue;
            }
            self.put(tag_name.clone(), other_tag.copy_tag());
        }
        self
    }

    /// `CompoundTag.accept(TagVisitor)`.
    pub fn accept(&self, visitor: &mut dyn TagVisitor) {
        visitor.visit_compound(self);
    }

    /// `CompoundTag.accept(StreamTagVisitor)`.
    pub fn accept_stream(&self, visitor: &mut dyn StreamTagVisitor) -> ValueResult {
        for (key, value) in self.tags.iter() {
            let ty = value.get_type();
            match visitor.visit_entry(ty) {
                EntryResult::Halt => return ValueResult::Halt,
                EntryResult::Break => return visitor.visit_container_end(),
                EntryResult::Skip => continue,
                EntryResult::Enter => {}
            }
            match visitor.visit_entry_named(ty, key) {
                EntryResult::Halt => return ValueResult::Halt,
                EntryResult::Break => return visitor.visit_container_end(),
                EntryResult::Skip => continue,
                EntryResult::Enter => {}
            }
            match value.accept_stream(visitor) {
                ValueResult::Halt => return ValueResult::Halt,
                ValueResult::Break => return visitor.visit_container_end(),
                ValueResult::Continue => {}
            }
        }
        visitor.visit_container_end()
    }

    fn get_optional(&self, name: &str) -> Option<&Tag> {
        self.tags.get(name)
    }
}

pub mod compound_tag_codec {
    use super::CompoundTag;
    use crate::nbt_ops::NbtOps;
    use crate::tag::Tag;
    use rivet_serialization::{Codec, DataResult, Dynamic, codec};
    use std::sync::Arc;

    /// Port of `CompoundTag.CODEC`.
    ///
    /// Java:
    /// ```java
    /// Codec.PASSTHROUGH.comapFlatMap(
    ///   t -> { Tag tag = t.convert(NbtOps.INSTANCE).getValue();
    ///          return tag instanceof CompoundTag c
    ///            ? DataResult.success(c == t.getValue() ? c.copy() : c)
    ///            : DataResult.error(() -> "Not a compound tag: " + tag); },
    ///   t -> new Dynamic<>(NbtOps.INSTANCE, t.copy()))
    /// ```
    pub fn encode(value: &CompoundTag) -> DataResult<Dynamic<Tag>> {
        DataResult::success(Dynamic::new(
            &NbtOps::instance(),
            Tag::Compound(value.copy_tag()),
        ))
    }

    pub fn parse(input: &Dynamic<Tag>) -> DataResult<CompoundTag> {
        // t.convert(NbtOps.INSTANCE) is identity here (ops == NbtOps).
        match &input.value {
            Tag::Compound(c) => DataResult::success(c.copy_tag()),
            other => DataResult::error(format!("Not a compound tag: {other}")),
        }
    }

    /// `CompoundTag.CODEC` as a `Codec<CompoundTag, NbtOps>` — the
    /// `Codec.PASSTHROUGH.comapFlatMap(...)` above, wired through
    /// `codec::comap_flat_map`.
    pub fn codec() -> Arc<dyn Codec<CompoundTag, NbtOps>> {
        codec::comap_flat_map(
            codec::passthrough::<NbtOps>(),
            Arc::new(|d: &Dynamic<Tag>| parse(d)),
            Arc::new(|c: &CompoundTag| {
                Dynamic::new(&NbtOps::instance(), Tag::Compound(c.copy_tag()))
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::int_tag::IntTag;
    use crate::tag::Tag;
    use rivet_serialization::{DataResult, codec};

    fn ops() -> NbtOps {
        NbtOps::instance()
    }

    #[test]
    fn store_read_round_trip_with_string_codec() {
        let mut tag = CompoundTag::new();
        let codec = codec::string_codec::<NbtOps>();
        tag.store("name", &codec, &"steve".to_string());
        assert_eq!(
            tag.get_string("name"),
            Some(&"steve".to_string()),
            "store must put the encoded value"
        );
        assert_eq!(
            tag.read::<String>("name", &codec),
            Some("steve".to_string())
        );
        assert_eq!(
            tag.read_quiet::<String>("name", &codec),
            Some("steve".to_string())
        );
    }

    #[test]
    fn store_nullable_omits_none() {
        let mut tag = CompoundTag::new();
        let codec = codec::int_codec::<NbtOps>();
        tag.store_nullable("x", &codec, Some(&7));
        assert_eq!(tag.get_int("x"), Some(7));
        tag.store_nullable("y", &codec, None::<&i32>);
        assert!(!tag.contains("y"), "None must not be stored");
    }

    #[test]
    fn store_with_ops_matches_default() {
        let mut tag = CompoundTag::new();
        let codec = codec::long_codec::<NbtOps>();
        tag.store_with_ops("k", &codec, ops(), &42_i64);
        assert_eq!(tag.get_long("k"), Some(42));
    }

    #[test]
    fn read_missing_returns_none() {
        let tag = CompoundTag::new();
        let codec = codec::int_codec::<NbtOps>();
        assert_eq!(tag.read::<i32>("missing", &codec), None);
        assert_eq!(tag.read_quiet::<i32>("missing", &codec), None);
    }

    #[test]
    fn read_type_mismatch_returns_none() {
        let mut tag = CompoundTag::new();
        tag.put_string("not_an_int", "oops");
        let codec = codec::int_codec::<NbtOps>();
        // String under an int codec → error (no partial) → None.
        assert_eq!(tag.read::<i32>("not_an_int", &codec), None);
        assert_eq!(tag.read_quiet::<i32>("not_an_int", &codec), None);
    }

    #[test]
    fn read_error_with_partial_returns_partial() {
        // A codec whose decode returns an error carrying a partial value — the
        // `resultOrPartial` path of `read`/`readQuiet`.
        let codec: Arc<dyn Codec<i32, NbtOps>> = codec::flat_xmap(
            codec::int_codec::<NbtOps>(),
            Arc::new(|_v: &i32| DataResult::error_with_partial("decode failed".to_string(), 123)),
            Arc::new(|v: &i32| DataResult::success(*v)),
        );
        let mut tag = CompoundTag::new();
        tag.put_int("x", 1);
        assert_eq!(tag.read::<i32>("x", &codec), Some(123));
        assert_eq!(tag.read_quiet::<i32>("x", &codec), Some(123));
    }

    #[test]
    fn store_map_and_read_map_round_trip() {
        let mut tag = CompoundTag::new();
        // `Codec.fieldOf("level")` over an int codec → a `MapCodec<i32>` that
        // encodes/decodes the single key `level`.
        let map_codec = codec::field_of(codec::int_codec::<NbtOps>(), "level".to_string());
        tag.store_map(&map_codec, &7);
        assert_eq!(tag.get_int("level"), Some(7), "store_map merges level:7");
        assert_eq!(tag.read_map(&map_codec), Some(7));

        // read_quiet_map: same result.
        assert_eq!(tag.read_map_quiet(&map_codec), Some(7));
    }

    #[test]
    fn store_map_merges_and_preserves_existing_keys() {
        let mut tag = CompoundTag::new();
        tag.put_int("keep", 1);
        let map_codec = codec::field_of(codec::int_codec::<NbtOps>(), "level".to_string());
        tag.store_map(&map_codec, &2);
        assert_eq!(tag.get_int("level"), Some(2));
        assert!(tag.contains("keep"), "store_map merges, not replaces");
    }

    #[test]
    fn read_map_type_mismatch_returns_none() {
        // A field present with the wrong tag type → the int field codec fails
        // to decode → None.
        let mut tag = CompoundTag::new();
        tag.put_string("x", "not_an_int");
        let map_codec = codec::field_of(codec::int_codec::<NbtOps>(), "x".to_string());
        assert_eq!(tag.read_map(&map_codec), None);
        assert_eq!(tag.read_map_quiet(&map_codec), None);
    }

    #[test]
    fn compound_tag_codec_round_trip() {
        let mut original = CompoundTag::new();
        original.put_string("a", "b");
        original.put_int("n", 9);
        let codec = crate::compound_tag::compound_tag_codec::codec();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &original)
            .get_or_throw("encode")
            .clone();
        let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
        assert_eq!(decoded, original);

        // PASSTHROUGH rejects a non-compound.
        let result = codec.parse(&ops, &Tag::Int(IntTag::value_of(1)));
        assert!(result.error_ref().is_some());
    }

    #[test]
    fn codec_non_compound_error_uses_snbt_display() {
        // Java: `DataResult.error(() -> "Not a compound tag: " + tag)` — the
        // tag renders through `Tag.toString()` (SNBT Display), not Rust Debug.
        let codec = crate::compound_tag::compound_tag_codec::codec();
        let ops = ops();

        let int_result = codec.parse(&ops, &Tag::Int(IntTag::value_of(1)));
        assert_eq!(
            int_result.error_ref().expect("int tag must fail").message(),
            "Not a compound tag: 1"
        );

        // SNBT quotes a string; Rust Debug renders the inner `String` (with
        // escaping and the `StringTag` wrapper), so these exact texts are the
        // counterfactual to `{:?}`. Note `quote_and_escape` switches the
        // delimiter: a string containing `"` is single-quoted.
        let plain_result = codec.parse(&ops, &Tag::String(StringTag::value_of("hi".into())));
        assert_eq!(
            plain_result
                .error_ref()
                .expect("string tag must fail")
                .message(),
            r#"Not a compound tag: "hi""#
        );

        let quote_result = codec.parse(&ops, &Tag::String(StringTag::value_of("a\"b".into())));
        assert_eq!(
            quote_result
                .error_ref()
                .expect("string tag must fail")
                .message(),
            "Not a compound tag: 'a\"b'"
        );
    }
}
