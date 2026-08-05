// Port of `net.minecraft.nbt.TagVisitor` (interface) — the visit surface
// `TextComponentTagVisitor` implements. Owned by manifest unit mc.nbt (base),
// not mc.nbt.text. All tags arrive as references to the newtype structs in the
// per-class modules (`Tag::Byte(ByteTag)` etc.).

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::end_tag::EndTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::short_tag::ShortTag;
use crate::string_tag::StringTag;

/// Port of `net.minecraft.nbt.TagVisitor`.
pub trait TagVisitor {
    fn visit_string(&mut self, tag: &StringTag);
    fn visit_byte(&mut self, tag: &ByteTag);
    fn visit_short(&mut self, tag: &ShortTag);
    fn visit_int(&mut self, tag: &IntTag);
    fn visit_long(&mut self, tag: &LongTag);
    fn visit_float(&mut self, tag: &FloatTag);
    fn visit_double(&mut self, tag: &DoubleTag);
    fn visit_byte_array(&mut self, tag: &ByteArrayTag);
    fn visit_int_array(&mut self, tag: &IntArrayTag);
    fn visit_long_array(&mut self, tag: &LongArrayTag);
    fn visit_list(&mut self, tag: &ListTag);
    fn visit_compound(&mut self, tag: &CompoundTag);
    fn visit_end(&mut self, tag: &EndTag);
}
