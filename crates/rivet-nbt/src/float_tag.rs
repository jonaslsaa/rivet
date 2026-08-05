//! Port of `net.minecraft.nbt.FloatTag` — `record FloatTag(float value)`.

use crate::mth;

pub const SELF_SIZE_IN_BYTES: i32 = 12;

/// `FloatTag` — value struct (Java record).
///
/// Equality follows Java record equality (`Float.compare(value, that.value) == 0`):
/// `NaN == NaN`, `0.0 != -0.0`, and equal by magnitude otherwise — NOT the IEEE
/// `==` of a derived `PartialEq`.
#[derive(Debug, Clone, Copy)]
pub struct FloatTag {
    pub value: f32,
}

impl PartialEq for FloatTag {
    fn eq(&self, other: &Self) -> bool {
        let a = self.value;
        let b = other.value;
        if a.is_nan() && b.is_nan() {
            // Java `Float.compare` treats every NaN bit pattern as equal.
            return true;
        }
        if a == 0.0 && b == 0.0 {
            // IEEE `-0.0 == 0.0`, but Java compares them unequal.
            return a.is_sign_negative() == b.is_sign_negative();
        }
        a == b
    }
}

impl Eq for FloatTag {}

impl std::hash::Hash for FloatTag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Java `Float.hashCode()` = `Float.floatToIntBits(value)`, which
        // canonicalizes every NaN bit pattern to 0x7fc00000 (consistent with
        // compare/equals treating NaNs as equal). `-0.0` keeps its sign bit, so
        // it hashes differently from `0.0`.
        let bits = if self.value.is_nan() {
            0x7fc0_0000
        } else {
            self.value.to_bits()
        };
        bits.hash(state)
    }
}

impl FloatTag {
    pub fn new(value: f32) -> Self {
        FloatTag { value }
    }

    /// `FloatTag.valueOf(float)` — `data == 0.0F` returns `ZERO`.
    pub fn value_of(data: f32) -> Self {
        if data == 0.0 {
            ZERO
        } else {
            FloatTag::new(data)
        }
    }

    /// `FloatTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `FloatTag.longValue()` — `(long)value`.
    pub fn long_value(&self) -> i64 {
        self.value as i64
    }

    /// `FloatTag.intValue()` — `Mth.floor(value)`.
    pub fn int_value(&self) -> i32 {
        mth::floor(self.value)
    }

    /// `FloatTag.shortValue()` — `(short)(Mth.floor(value) & 65535)`.
    pub fn short_value(&self) -> i16 {
        (mth::floor(self.value) & 0xFFFF) as i16
    }

    /// `FloatTag.byteValue()` — `(byte)(Mth.floor(value) & 0xFF)`.
    pub fn byte_value(&self) -> i8 {
        (mth::floor(self.value) & 0xFF) as i8
    }
}

pub const ZERO: FloatTag = FloatTag { value: 0.0 };
