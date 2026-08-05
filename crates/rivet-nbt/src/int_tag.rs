//! Port of `net.minecraft.nbt.IntTag` — `record IntTag(int value)`.

pub const SELF_SIZE_IN_BYTES: i32 = 12;

/// `IntTag` — value struct (Java record).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntTag {
    pub value: i32,
}

impl IntTag {
    pub fn new(value: i32) -> Self {
        IntTag { value }
    }

    /// `IntTag.valueOf(int)`.
    pub fn value_of(i: i32) -> Self {
        IntTag::new(i)
    }

    /// `IntTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `IntTag.shortValue()` — `(short)(value & 65535)`.
    pub fn short_value(&self) -> i16 {
        (self.value & 0xFFFF) as i16
    }

    /// `IntTag.byteValue()` — `(byte)(value & 0xFF)`.
    pub fn byte_value(&self) -> i8 {
        (self.value & 0xFF) as i8
    }
}
