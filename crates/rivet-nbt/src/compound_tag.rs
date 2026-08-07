//! Port of `net.minecraft.nbt.CompoundTag` — `final class` wrapping a map.
//!
//! Java uses an `Object2ObjectOpenHashMap<String, Tag>` (fastutil). Per
//! PORTING.md, HashMap iteration order is not preserved; NBT serialization
//! writes the map in iteration order. Java's fastutil map iterates in hash
//! order — matching that order would require a Java-identical hasher. We use
//! `std::collections::HashMap`; its randomized SipHash seed makes iteration
//! order non-deterministic across processes, so field order is a known drift
//! that golden tests cannot pin down (see PORTING.md drift notes). When the
//! binary write path (`NbtIo`) lands, switch to an insertion-ordered map or
//! port fastutil's hash order if byte-for-byte output parity requires it.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::double_tag::DoubleTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::short_tag::ShortTag;
use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::string_tag::StringTag;
use crate::tag::Tag;
use crate::tag_visitor::TagVisitor;
use std::collections::HashMap;

pub const SELF_SIZE_IN_BYTES: i32 = 48;
pub const MAP_ENTRY_SIZE_IN_BYTES: i32 = 32;

/// `CompoundTag`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompoundTag {
    pub tags: HashMap<String, Tag>,
}

impl CompoundTag {
    /// `new CompoundTag()`.
    pub fn new() -> Self {
        CompoundTag {
            tags: HashMap::new(),
        }
    }

    /// `new CompoundTag(Map)`.
    pub fn with_map(tags: HashMap<String, Tag>) -> Self {
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
    pub fn remove(&mut self, name: &str) -> Option<Tag> {
        self.tags.remove(name)
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

    /// `CompoundTag.copy()` — deep copy.
    pub fn copy_tag(&self) -> CompoundTag {
        let mut ret = HashMap::with_capacity(self.tags.len());
        for (k, v) in self.tags.iter() {
            ret.insert(k.clone(), v.copy_tag());
        }
        CompoundTag { tags: ret }
    }

    /// RivetTodo(#204): Java's `CompoundTag.store(String, Codec, ...)`,
    /// `storeNullable`, `read(String, Codec, ...)`, `readQuiet` (incl. the
    /// `DynamicOps<Tag>` and `MapCodec` overloads) are not ported — the DFU
    /// `Codec`/`MapCodec` surface (`comapFlatMap`, `PASSTHROUGH`,
    /// `MapCodec.decode` over `MapLike`) now exists in `rivet-serialization`,
    /// so the overloads are a plain omission with no consumer forcing them.
    ///
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
    use rivet_serialization::{DataResult, Dynamic};

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
            other => DataResult::error(format!("Not a compound tag: {other:?}")),
        }
    }
}
