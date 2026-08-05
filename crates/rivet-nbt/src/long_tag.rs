//! Port of `net.minecraft.nbt.LongTag` — `record LongTag(long value)`.

pub const SELF_SIZE_IN_BYTES: i32 = 16;

/// `LongTag` — value struct (Java record).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LongTag {
    pub value: i64,
}

impl LongTag {
    pub fn new(value: i64) -> Self {
        LongTag { value }
    }

    /// `LongTag.valueOf(long)`.
    pub fn value_of(i: i64) -> Self {
        LongTag::new(i)
    }

    /// `LongTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `LongTag.intValue()` — `(int)(value & -1L)`.
    pub fn int_value(&self) -> i32 {
        (self.value & 0xFFFF_FFFF) as i32
    }

    /// `LongTag.shortValue()` — `(short)(value & 65535L)`.
    pub fn short_value(&self) -> i16 {
        (self.value & 0xFFFF) as i16
    }

    /// `LongTag.byteValue()` — `(byte)(value & 255L)`.
    pub fn byte_value(&self) -> i8 {
        (self.value & 0xFF) as i8
    }

    /// `LongTag.floatValue()` — `(float)value`.
    pub fn float_value(&self) -> f32 {
        self.value as f32
    }
}
