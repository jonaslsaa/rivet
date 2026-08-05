//! Port of `net.minecraft.nbt.ShortTag` — `record ShortTag(short value)`.

pub const SELF_SIZE_IN_BYTES: i32 = 10;

/// `ShortTag` — value struct (Java record).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortTag {
    pub value: i16,
}

impl ShortTag {
    pub fn new(value: i16) -> Self {
        ShortTag { value }
    }

    /// `ShortTag.valueOf(short)`.
    pub fn value_of(i: i16) -> Self {
        ShortTag::new(i)
    }

    /// `ShortTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `ShortTag.byteValue()` — `(byte)(value & 0xFF)`.
    pub fn byte_value(&self) -> i8 {
        (self.value & 0xFF) as i8
    }
}
