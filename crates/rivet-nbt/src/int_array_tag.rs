//! Port of `net.minecraft.nbt.IntArrayTag` — `final class` holding `int[] data`.

use crate::int_tag::IntTag;
use crate::tag::Tag;

pub const SELF_SIZE_IN_BYTES: i32 = 24;

/// `IntArrayTag`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IntArrayTag {
    pub data: Vec<i32>,
}

impl IntArrayTag {
    pub fn new(data: Vec<i32>) -> Self {
        IntArrayTag { data }
    }

    /// `IntArrayTag.sizeInBytes()` — `24 + 4 * data.length`.
    pub fn size_in_bytes(&self) -> i32 {
        24 + 4 * self.data.len() as i32
    }

    /// `IntArrayTag.copy()` — deep copy.
    pub fn copy_tag(&self) -> IntArrayTag {
        IntArrayTag {
            data: self.data.clone(),
        }
    }

    /// `IntArrayTag.getAsIntArray()`.
    pub fn get_as_int_array(&self) -> &Vec<i32> {
        &self.data
    }

    /// `IntArrayTag.size()`.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// `CollectionTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// `CollectionTag.get(int)` — `IntTag.valueOf(data[index])`.
    pub fn get(&self, index: usize) -> IntTag {
        IntTag::value_of(self.data[index])
    }

    /// `CollectionTag.setTag(int, Tag)` — stores `numeric.intValue()`.
    pub fn set_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data[index] = n.int_value();
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.addTag(int, Tag)` — `ArrayUtils.add(data, index, ...)`.
    pub fn add_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data.insert(index, n.int_value());
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.remove(int)` — `IntTag.valueOf` of the previous value.
    pub fn remove(&mut self, index: usize) -> IntTag {
        let prev = self.data.remove(index);
        IntTag::value_of(prev)
    }

    /// `CollectionTag.clear()`.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// `CollectionTag.iterator()` — boxed leaf per element.
    pub fn iter(&self) -> impl Iterator<Item = IntTag> + '_ {
        self.data.iter().map(|v| IntTag::value_of(*v))
    }
}
