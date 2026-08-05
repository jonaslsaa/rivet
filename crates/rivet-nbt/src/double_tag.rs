//! Port of `net.minecraft.nbt.DoubleTag` — `record DoubleTag(double value)`.

use crate::mth;

pub const SELF_SIZE_IN_BYTES: i32 = 16;

/// `DoubleTag` — value struct (Java record).
///
/// Equality follows Java record equality (`Double.compare(value, that.value) == 0`):
/// `NaN == NaN`, `0.0 != -0.0`, and equal by magnitude otherwise — NOT the IEEE
/// `==` of a derived `PartialEq`.
#[derive(Debug, Clone, Copy)]
pub struct DoubleTag {
    pub value: f64,
}

impl PartialEq for DoubleTag {
    fn eq(&self, other: &Self) -> bool {
        let a = self.value;
        let b = other.value;
        if a.is_nan() && b.is_nan() {
            // Java `Double.compare` treats every NaN bit pattern as equal.
            return true;
        }
        if a == 0.0 && b == 0.0 {
            // IEEE `-0.0 == 0.0`, but Java compares them unequal.
            return a.is_sign_negative() == b.is_sign_negative();
        }
        a == b
    }
}

impl Eq for DoubleTag {}

impl std::hash::Hash for DoubleTag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Java `Double.hashCode()` = `Double.doubleToLongBits(value)`, which
        // canonicalizes every NaN bit pattern to 0x7ff8000000000000 (consistent
        // with compare/equals treating NaNs as equal). `-0.0` keeps its sign
        // bit, so it hashes differently from `0.0`.
        let bits = if self.value.is_nan() {
            0x7ff8_0000_0000_0000u64
        } else {
            self.value.to_bits()
        };
        bits.hash(state)
    }
}

impl DoubleTag {
    pub fn new(value: f64) -> Self {
        DoubleTag { value }
    }

    /// `DoubleTag.valueOf(double)` — `data == 0.0` returns `ZERO`.
    pub fn value_of(data: f64) -> Self {
        if data == 0.0 {
            ZERO
        } else {
            DoubleTag::new(data)
        }
    }

    /// `DoubleTag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        SELF_SIZE_IN_BYTES
    }

    /// `DoubleTag.longValue()` — `(long)Math.floor(value)`.
    pub fn long_value(&self) -> i64 {
        self.value.floor() as i64
    }

    /// `DoubleTag.intValue()` — `Mth.floor(value)`.
    pub fn int_value(&self) -> i32 {
        mth::floor_d(self.value)
    }

    /// `DoubleTag.shortValue()` — `(short)(Mth.floor(value) & 65535)`.
    pub fn short_value(&self) -> i16 {
        (mth::floor_d(self.value) & 0xFFFF) as i16
    }

    /// `DoubleTag.byteValue()` — `(byte)(Mth.floor(value) & 0xFF)`.
    pub fn byte_value(&self) -> i8 {
        (mth::floor_d(self.value) & 0xFF) as i8
    }

    /// `DoubleTag.floatValue()` — `(float)value`.
    pub fn float_value(&self) -> f32 {
        self.value as f32
    }
}

pub const ZERO: DoubleTag = DoubleTag { value: 0.0 };
