//! Port of `net.minecraft.nbt.NumericTag` — `sealed interface permits ByteTag,
//! ShortTag, IntTag, LongTag, FloatTag, DoubleTag`.
//!
//! The `byteValue`..`doubleValue` conversion surface lives on the leaf structs
//! and on `Tag::as_*`. `box()` (Java returns the boxed `Number`) maps to
//! `boxed` — `box` is a Rust keyword — and `Tag::as_number` delegates to it.

/// A `NumericTag` — one of the six numeric leaves.
#[derive(Debug, Clone, PartialEq)]
pub enum NumericTag {
    Byte(crate::byte_tag::ByteTag),
    Short(crate::short_tag::ShortTag),
    Int(crate::int_tag::IntTag),
    Long(crate::long_tag::LongTag),
    Float(crate::float_tag::FloatTag),
    Double(crate::double_tag::DoubleTag),
}

impl NumericTag {
    /// `NumericTag.byteValue()`.
    pub fn byte_value(&self) -> i8 {
        match self {
            NumericTag::Byte(t) => t.value,
            NumericTag::Short(t) => t.byte_value(),
            NumericTag::Int(t) => t.byte_value(),
            NumericTag::Long(t) => t.byte_value(),
            NumericTag::Float(t) => t.byte_value(),
            NumericTag::Double(t) => t.byte_value(),
        }
    }

    /// `NumericTag.shortValue()`.
    pub fn short_value(&self) -> i16 {
        match self {
            NumericTag::Byte(t) => t.value as i16,
            NumericTag::Short(t) => t.value,
            NumericTag::Int(t) => t.short_value(),
            NumericTag::Long(t) => t.short_value(),
            NumericTag::Float(t) => t.short_value(),
            NumericTag::Double(t) => t.short_value(),
        }
    }

    /// `NumericTag.intValue()`.
    pub fn int_value(&self) -> i32 {
        match self {
            NumericTag::Byte(t) => t.value as i32,
            NumericTag::Short(t) => t.value as i32,
            NumericTag::Int(t) => t.value,
            NumericTag::Long(t) => t.int_value(),
            NumericTag::Float(t) => t.int_value(),
            NumericTag::Double(t) => t.int_value(),
        }
    }

    /// `NumericTag.longValue()`.
    pub fn long_value(&self) -> i64 {
        match self {
            NumericTag::Byte(t) => t.value as i64,
            NumericTag::Short(t) => t.value as i64,
            NumericTag::Int(t) => t.value as i64,
            NumericTag::Long(t) => t.value,
            NumericTag::Float(t) => t.long_value(),
            NumericTag::Double(t) => t.long_value(),
        }
    }

    /// `NumericTag.floatValue()`.
    pub fn float_value(&self) -> f32 {
        match self {
            NumericTag::Byte(t) => t.value as f32,
            NumericTag::Short(t) => t.value as f32,
            NumericTag::Int(t) => t.value as f32,
            NumericTag::Long(t) => t.float_value(),
            NumericTag::Float(t) => t.value,
            NumericTag::Double(t) => t.float_value(),
        }
    }

    /// `NumericTag.doubleValue()`.
    pub fn double_value(&self) -> f64 {
        match self {
            NumericTag::Byte(t) => t.value as f64,
            NumericTag::Short(t) => t.value as f64,
            NumericTag::Int(t) => t.value as f64,
            NumericTag::Long(t) => t.value as f64,
            NumericTag::Float(t) => t.value as f64,
            NumericTag::Double(t) => t.value,
        }
    }

    /// `NumericTag.box()` — the boxed `Number` (the `box` name is a Rust
    /// keyword, hence `boxed`).
    pub fn boxed(&self) -> rivet_serialization::number::Number {
        match self {
            NumericTag::Byte(t) => rivet_serialization::number::Number::Byte(t.value),
            NumericTag::Short(t) => rivet_serialization::number::Number::Short(t.value),
            NumericTag::Int(t) => rivet_serialization::number::Number::Int(t.value),
            NumericTag::Long(t) => rivet_serialization::number::Number::Long(t.value),
            NumericTag::Float(t) => rivet_serialization::number::Number::Float(t.value),
            NumericTag::Double(t) => rivet_serialization::number::Number::Double(t.value),
        }
    }
}

impl TryFrom<&crate::tag::Tag> for NumericTag {
    type Error = ();

    fn try_from(tag: &crate::tag::Tag) -> Result<Self, Self::Error> {
        match tag {
            crate::tag::Tag::Byte(t) => Ok(NumericTag::Byte(*t)),
            crate::tag::Tag::Short(t) => Ok(NumericTag::Short(*t)),
            crate::tag::Tag::Int(t) => Ok(NumericTag::Int(*t)),
            crate::tag::Tag::Long(t) => Ok(NumericTag::Long(*t)),
            crate::tag::Tag::Float(t) => Ok(NumericTag::Float(*t)),
            crate::tag::Tag::Double(t) => Ok(NumericTag::Double(*t)),
            _ => Err(()),
        }
    }
}
