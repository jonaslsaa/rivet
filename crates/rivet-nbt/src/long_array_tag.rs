//! Port of `net.minecraft.nbt.LongArrayTag` — `final class` holding `long[] data`.

use crate::long_tag::LongTag;
use crate::tag::Tag;

pub const SELF_SIZE_IN_BYTES: i32 = 24;

/// `LongArrayTag`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LongArrayTag {
    pub data: Vec<i64>,
}

impl LongArrayTag {
    pub fn new(data: Vec<i64>) -> Self {
        LongArrayTag { data }
    }

    /// `LongArrayTag.sizeInBytes()` — `24 + 8 * data.length`.
    pub fn size_in_bytes(&self) -> i32 {
        24 + 8 * self.data.len() as i32
    }

    /// `LongArrayTag.copy()` — deep copy.
    pub fn copy_tag(&self) -> LongArrayTag {
        LongArrayTag {
            data: self.data.clone(),
        }
    }

    /// `LongArrayTag.getAsLongArray()`.
    pub fn get_as_long_array(&self) -> &Vec<i64> {
        &self.data
    }

    /// `LongArrayTag.size()`.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// `CollectionTag.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// `CollectionTag.get(int)` — `LongTag.valueOf(data[index])`.
    pub fn get(&self, index: usize) -> LongTag {
        LongTag::value_of(self.data[index])
    }

    /// `CollectionTag.setTag(int, Tag)` — stores `numeric.longValue()`.
    pub fn set_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data[index] = n.long_value();
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.addTag(int, Tag)` — `ArrayUtils.add(data, index, ...)`.
    pub fn add_tag(&mut self, index: usize, tag: &Tag) -> bool {
        match crate::numeric_tag::NumericTag::try_from(tag) {
            Ok(n) => {
                self.data.insert(index, n.long_value());
                true
            }
            Err(_) => false,
        }
    }

    /// `CollectionTag.remove(int)` — `LongTag.valueOf` of the previous value.
    pub fn remove(&mut self, index: usize) -> LongTag {
        let prev = self.data.remove(index);
        LongTag::value_of(prev)
    }

    /// `CollectionTag.clear()`.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// `CollectionTag.iterator()` — boxed leaf per element.
    pub fn iter(&self) -> impl Iterator<Item = LongTag> + '_ {
        self.data.iter().map(|v| LongTag::value_of(*v))
    }
}
