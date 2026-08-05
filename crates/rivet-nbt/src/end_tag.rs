//! Port of `net.minecraft.nbt.EndTag` (singleton).

use crate::tag::Tag;

pub const SELF_SIZE_IN_BYTES: i32 = 8;

/// `EndTag` — there is exactly one instance (`INSTANCE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndTag;

impl EndTag {
    /// `EndTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `EndTag.copy()` — returns `this` (singleton).
    pub fn copy_tag(&self) -> EndTag {
        *self
    }

    /// `EndTag.INSTANCE` as a `Tag`.
    pub fn as_tag(&self) -> Tag {
        Tag::End(*self)
    }
}
