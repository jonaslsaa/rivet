//! Port of `net.minecraft.nbt.CollectionTag` (interface over `ListTag`,
//! `ByteArrayTag`, `IntArrayTag`, `LongArrayTag`).
//!
//! The Rust enum `Tag` already dispatches over these leaf types, so this module
//! documents the shared `CollectionTag` surface (`size`, `isEmpty`,
//! `clear`, `get`, `setTag`, `addTag`, `remove`, iteration) without a common
//! supertrait.

use crate::byte_array_tag::ByteArrayTag;
use crate::int_array_tag::IntArrayTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::tag::Tag;

/// A `CollectionTag` — any of the four collection leaves.
#[derive(Debug, Clone, PartialEq)]
pub enum CollectionTag {
    List(ListTag),
    ByteArray(ByteArrayTag),
    IntArray(IntArrayTag),
    LongArray(LongArrayTag),
}

impl CollectionTag {
    /// `CollectionTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// `CollectionTag.size()`.
    pub fn size(&self) -> usize {
        match self {
            CollectionTag::List(t) => t.size(),
            CollectionTag::ByteArray(t) => t.size(),
            CollectionTag::IntArray(t) => t.size(),
            CollectionTag::LongArray(t) => t.size(),
        }
    }

    /// The tag id for the leaf (TAG_LIST / TAG_BYTE_ARRAY / ...).
    pub fn id(&self) -> i8 {
        match self {
            CollectionTag::List(_) => crate::tag::TAG_LIST,
            CollectionTag::ByteArray(_) => crate::tag::TAG_BYTE_ARRAY,
            CollectionTag::IntArray(_) => crate::tag::TAG_INT_ARRAY,
            CollectionTag::LongArray(_) => crate::tag::TAG_LONG_ARRAY,
        }
    }

    /// `CollectionTag.get(int)` — the boxed leaf.
    pub fn get(&self, index: usize) -> Tag {
        match self {
            CollectionTag::List(t) => t.get(index).clone(),
            CollectionTag::ByteArray(t) => Tag::Byte(t.get(index)),
            CollectionTag::IntArray(t) => Tag::Int(t.get(index)),
            CollectionTag::LongArray(t) => Tag::Long(t.get(index)),
        }
    }

    /// `CollectionTag.setTag(int, Tag)` — `true` for numeric tags, else `false`.
    pub fn set_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match self {
            CollectionTag::List(t) => t.set_tag(index, tag.clone()),
            CollectionTag::ByteArray(t) => t.set_tag(index, tag),
            CollectionTag::IntArray(t) => t.set_tag(index, tag),
            CollectionTag::LongArray(t) => t.set_tag(index, tag),
        }
    }

    /// `CollectionTag.addTag(int, Tag)` — `true` for numeric tags, else `false`.
    pub fn add_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match self {
            CollectionTag::List(t) => t.add_tag(index, tag.clone()),
            CollectionTag::ByteArray(t) => t.add_tag(index, tag),
            CollectionTag::IntArray(t) => t.add_tag(index, tag),
            CollectionTag::LongArray(t) => t.add_tag(index, tag),
        }
    }

    /// `CollectionTag.remove(int)` — the boxed previous value.
    pub fn remove(&mut self, index: usize) -> Tag {
        match self {
            CollectionTag::List(t) => t.remove(index),
            CollectionTag::ByteArray(t) => Tag::Byte(t.remove(index)),
            CollectionTag::IntArray(t) => Tag::Int(t.remove(index)),
            CollectionTag::LongArray(t) => Tag::Long(t.remove(index)),
        }
    }

    /// `CollectionTag.clear()`.
    pub fn clear(&mut self) {
        match self {
            CollectionTag::List(t) => t.clear(),
            CollectionTag::ByteArray(t) => t.clear(),
            CollectionTag::IntArray(t) => t.clear(),
            CollectionTag::LongArray(t) => t.clear(),
        }
    }
}

impl From<ListTag> for Tag {
    fn from(t: ListTag) -> Self {
        Tag::List(t)
    }
}
