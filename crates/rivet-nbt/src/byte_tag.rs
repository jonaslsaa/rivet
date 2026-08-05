//! Port of `net.minecraft.nbt.ByteTag` — `record ByteTag(byte value)`.

pub const SELF_SIZE_IN_BYTES: i32 = 9;

/// `ByteTag` — value struct (Java record).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteTag {
    pub value: i8,
}

impl ByteTag {
    pub fn new(value: i8) -> Self {
        ByteTag { value }
    }

    /// `ByteTag.valueOf(byte)` — the Java intern cache has no observable effect
    /// on value semantics, so this is `new`.
    pub fn value_of(data: i8) -> Self {
        ByteTag::new(data)
    }

    /// `ByteTag.valueOf(boolean)`.
    pub fn value_of_bool(data: bool) -> Self {
        if data {
            ByteTag::new(1)
        } else {
            ByteTag::new(0)
        }
    }

    /// `ByteTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }
}

pub const ZERO: ByteTag = ByteTag { value: 0 };
pub const ONE: ByteTag = ByteTag { value: 1 };
