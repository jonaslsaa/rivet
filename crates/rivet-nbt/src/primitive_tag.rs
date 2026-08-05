//! Port of `net.minecraft.nbt.PrimitiveTag` — `sealed interface permits
//! NumericTag, StringTag`.
//!
//! Marker for the immutable leaves; the interesting surface (identity `copy`)
//! is folded into the `Tag` enum's `copy_tag`. Kept as a marker to preserve the
//! class name for greppability.

/// A `PrimitiveTag` — one of the numeric tags or `StringTag`.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimitiveTag {
    Byte(crate::byte_tag::ByteTag),
    Short(crate::short_tag::ShortTag),
    Int(crate::int_tag::IntTag),
    Long(crate::long_tag::LongTag),
    Float(crate::float_tag::FloatTag),
    Double(crate::double_tag::DoubleTag),
    String(crate::string_tag::StringTag),
}

impl PrimitiveTag {
    /// `PrimitiveTag.copy()` — returns `this`.
    pub fn copy_identity(&self) -> PrimitiveTag {
        self.clone()
    }
}
