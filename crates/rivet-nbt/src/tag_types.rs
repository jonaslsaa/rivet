//! Port of `net.minecraft.nbt.TagTypes` — the `TYPES` array and `getType`.

use crate::tag_type::TagType;

/// `TagTypes.getType(typeId)`.
pub fn get_type(type_id: i8) -> TagType {
    TagType::from_id(type_id)
}
