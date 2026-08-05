//! Port of `net.minecraft.nbt.TagType`.
//!
//! Java: `interface TagType<T extends Tag>`. The `load`/`parse`/`skip` data-IO
//! members belong to the NbtIo/streaming port (`nbt_io.rs` + friends) and are
//! not carried here. This module keeps the identity (`getName`, `getPrettyName`,
//! `createInvalid`) and the per-tag singleton constants, which the visitors and
//! `Tag.getType()` need.

/// Port of the `TagType<T>` identity for the 13 concrete NBT types, plus the
/// `TagType.createInvalid(id)` sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagType {
    End,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    ByteArray,
    String,
    List,
    Compound,
    IntArray,
    LongArray,
    /// `TagType.createInvalid(id)`.
    Invalid(i32),
}

impl TagType {
    /// `TagTypes.getType(typeId)` — index into the `TYPES` array, else invalid.
    pub fn from_id(type_id: i8) -> TagType {
        match type_id {
            crate::tag::TAG_END => TagType::End,
            crate::tag::TAG_BYTE => TagType::Byte,
            crate::tag::TAG_SHORT => TagType::Short,
            crate::tag::TAG_INT => TagType::Int,
            crate::tag::TAG_LONG => TagType::Long,
            crate::tag::TAG_FLOAT => TagType::Float,
            crate::tag::TAG_DOUBLE => TagType::Double,
            crate::tag::TAG_BYTE_ARRAY => TagType::ByteArray,
            crate::tag::TAG_STRING => TagType::String,
            crate::tag::TAG_LIST => TagType::List,
            crate::tag::TAG_COMPOUND => TagType::Compound,
            crate::tag::TAG_INT_ARRAY => TagType::IntArray,
            crate::tag::TAG_LONG_ARRAY => TagType::LongArray,
            other => TagType::Invalid(other as i32),
        }
    }

    /// `TagType.getName()`.
    pub fn name(&self) -> String {
        match self {
            TagType::End => "END".into(),
            TagType::Byte => "BYTE".into(),
            TagType::Short => "SHORT".into(),
            TagType::Int => "INT".into(),
            TagType::Long => "LONG".into(),
            TagType::Float => "FLOAT".into(),
            TagType::Double => "DOUBLE".into(),
            TagType::ByteArray => "BYTE[]".into(),
            TagType::String => "STRING".into(),
            TagType::List => "LIST".into(),
            TagType::Compound => "COMPOUND".into(),
            TagType::IntArray => "INT[]".into(),
            TagType::LongArray => "LONG[]".into(),
            TagType::Invalid(id) => format!("INVALID[{id}]"),
        }
    }

    /// `TagType.getPrettyName()`.
    pub fn pretty_name(&self) -> String {
        match self {
            TagType::End => "TAG_End".into(),
            TagType::Byte => "TAG_Byte".into(),
            TagType::Short => "TAG_Short".into(),
            TagType::Int => "TAG_Int".into(),
            TagType::Long => "TAG_Long".into(),
            TagType::Float => "TAG_Float".into(),
            TagType::Double => "TAG_Double".into(),
            TagType::ByteArray => "TAG_Byte_Array".into(),
            TagType::String => "TAG_String".into(),
            TagType::List => "TAG_List".into(),
            TagType::Compound => "TAG_Compound".into(),
            TagType::IntArray => "TAG_Int_Array".into(),
            TagType::LongArray => "TAG_Long_Array".into(),
            TagType::Invalid(id) => format!("UNKNOWN_{id}"),
        }
    }

    /// `TagType.StaticSize.size()` for the six fixed-size types; `None`
    /// otherwise. `EndTag.TYPE` is a plain `TagType` in Java (not a
    /// `StaticSize`, so it has no `size()`), hence `None` for `End`.
    pub fn static_size(&self) -> Option<i32> {
        match self {
            TagType::Byte => Some(1),
            TagType::Short => Some(2),
            TagType::Int => Some(4),
            TagType::Long => Some(8),
            TagType::Float => Some(4),
            TagType::Double => Some(8),
            _ => None,
        }
    }
}
