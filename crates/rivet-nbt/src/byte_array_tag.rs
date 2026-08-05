//! Port of `net.minecraft.nbt.ByteArrayTag` — `final class` holding `byte[] data`.

use crate::byte_tag::ByteTag;
use crate::tag::Tag;

pub const SELF_SIZE_IN_BYTES: i32 = 24;

/// `ByteArrayTag`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByteArrayTag {
    pub data: Vec<i8>,
}

impl ByteArrayTag {
    pub fn new(data: Vec<i8>) -> Self {
        ByteArrayTag { data }
    }

    /// `ByteArrayTag.sizeInBytes()` — `24 + 1 * data.length`.
    pub fn size_in_bytes(&self) -> i32 {
        24 + self.data.len() as i32
    }

    /// `ByteArrayTag.copy()` — deep copy.
    pub fn copy_tag(&self) -> ByteArrayTag {
        ByteArrayTag {
            data: self.data.clone(),
        }
    }

    /// `ByteArrayTag.getAsByteArray()`.
    pub fn get_as_byte_array(&self) -> &Vec<i8> {
        &self.data
    }

    /// `ByteArrayTag.size()`.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// `CollectionTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// `CollectionTag.get(int)` — `ByteTag.valueOf(data[index])`.
    pub fn get(&self, index: usize) -> ByteTag {
        ByteTag::value_of(self.data[index])
    }

    /// `CollectionTag.setTag(int, Tag)` — stores `numeric.byteValue()`.
    pub fn set_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data[index] = n.byte_value();
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.addTag(int, Tag)` — `ArrayUtils.add(data, index, ...)`.
    pub fn add_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data.insert(index, n.byte_value());
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.remove(int)` — `ByteTag.valueOf` of the previous value.
    pub fn remove(&mut self, index: usize) -> ByteTag {
        let prev = self.data.remove(index);
        ByteTag::value_of(prev)
    }

    /// `CollectionTag.clear()`.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// `CollectionTag.iterator()` — boxed leaf per element.
    pub fn iter(&self) -> impl Iterator<Item = ByteTag> + '_ {
        self.data.iter().map(|v| ByteTag::value_of(*v))
    }
}
