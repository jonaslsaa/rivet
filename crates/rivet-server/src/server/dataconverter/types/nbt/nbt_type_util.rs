//! Port of `ca.spottedleaf.dataconverter.types.nbt.NBTTypeUtil`.

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::nbt::nbt_list_type::NBTListType;
use crate::server::dataconverter::types::nbt::nbt_map_type::NBTMapType;
use crate::server::dataconverter::types::type_util::{TypeUtil, TypeUtilBase};
use rivet_nbt::byte_array_tag::ByteArrayTag;
use rivet_nbt::byte_tag::ByteTag;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::end_tag::EndTag;
use rivet_nbt::int_array_tag::IntArrayTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::long_array_tag::LongArrayTag;
use rivet_nbt::long_tag::LongTag;
use rivet_nbt::short_tag::ShortTag;
use rivet_nbt::string_tag::StringTag;
use rivet_nbt::tag::Tag;

/// `NBTTypeUtil` — `TypeUtil<Tag>` implementation for Mojang NBT.
#[derive(Debug, Clone, Copy, Default)]
pub struct NBTTypeUtil;

impl NBTTypeUtil {
    /// `NBTTypeUtil.convertNBTToGeneric(TypeUtil<?>, Tag)`.
    pub fn convert_nbt_to_generic(to: &dyn TypeUtilBase, tag: &Tag) -> Option<Generic> {
        match tag {
            Tag::Compound(c) => Some(Generic::Map(Self::convert_nbt_map(to, c))),
            Tag::List(l) => Some(Generic::List(Self::convert_nbt_list(to, l))),
            Tag::End(_) => None,
            Tag::String(s) => Some(Generic::Str(s.value.clone())),
            Tag::Byte(b) => Some(Generic::Byte(b.value)),
            Tag::Short(s) => Some(Generic::Short(s.value)),
            Tag::Int(i) => Some(Generic::Int(i.value)),
            Tag::Long(l) => Some(Generic::Long(l.value)),
            Tag::Float(f) => Some(Generic::Float(f.value)),
            Tag::Double(d) => Some(Generic::Double(d.value)),
            Tag::ByteArray(b) => Some(Generic::Bytes(b.data.clone())),
            Tag::IntArray(i) => Some(Generic::Ints(i.data.clone())),
            Tag::LongArray(l) => Some(Generic::Longs(l.data.clone())),
        }
    }

    /// `NBTTypeUtil.convertNBT(TypeUtil<?>, CompoundTag)`.
    pub fn convert_nbt_map(to: &dyn TypeUtilBase, nbt: &CompoundTag) -> Box<dyn MapType> {
        let mut ret = to.create_empty_map();
        for (key, value) in &nbt.tags {
            let converted =
                Self::convert_nbt_to_generic(to, value).expect("Unrecognized type null");
            ret.set_generic(key, converted);
        }
        ret
    }

    /// `NBTTypeUtil.convertNBT(TypeUtil<?>, ListTag)`.
    pub fn convert_nbt_list(to: &dyn TypeUtilBase, nbt: &ListTag) -> Box<dyn ListType> {
        let mut ret = to.create_empty_list();
        for value in &nbt.list {
            let converted =
                Self::convert_nbt_to_generic(to, value).expect("Unrecognized type null");
            ret.add_generic(converted);
        }
        ret
    }
}

impl TypeUtilBase for NBTTypeUtil {
    fn create_empty_list(&self) -> Box<dyn ListType> {
        Box::new(NBTListType::new())
    }

    fn create_empty_map(&self) -> Box<dyn MapType> {
        Box::new(NBTMapType::new())
    }
}

impl TypeUtil<Tag> for NBTTypeUtil {
    fn convert_to(
        &self,
        value_generic: Option<&Generic>,
        to: &dyn TypeUtilBase,
    ) -> Option<Generic> {
        let value = value_generic?;
        match value {
            Generic::Str(s) => Some(Generic::Str(s.clone())),
            Generic::Bool(b) => Some(Generic::Bool(*b)),
            Generic::Byte(v) => Some(Generic::Byte(*v)),
            Generic::Short(v) => Some(Generic::Short(*v)),
            Generic::Int(v) => Some(Generic::Int(*v)),
            Generic::Long(v) => Some(Generic::Long(*v)),
            Generic::Float(v) => Some(Generic::Float(*v)),
            Generic::Double(v) => Some(Generic::Double(*v)),
            Generic::List(l) => {
                let nbt_list = l
                    .as_any()
                    .downcast_ref::<NBTListType>()
                    .expect("Unrecognized list type in NBTTypeUtil::convert_to");
                Some(Generic::List(Self::convert_nbt_list(
                    to,
                    &nbt_list.get_tag(),
                )))
            }
            Generic::Map(m) => {
                let nbt_map = m
                    .as_any()
                    .downcast_ref::<NBTMapType>()
                    .expect("Unrecognized map type in NBTTypeUtil::convert_to");
                Some(Generic::Map(Self::convert_nbt_map(to, &nbt_map.get_tag())))
            }
            _ => panic!("Unrecognized type in NBTTypeUtil::convert_to"),
        }
    }

    fn convert_from_base_to_generic(&self, input: &Tag, to: &dyn TypeUtilBase) -> Option<Generic> {
        Self::convert_nbt_to_generic(to, input)
    }

    fn base_to_generic(&self, input: &Tag) -> Option<Generic> {
        match input {
            Tag::Compound(ct) => Some(Generic::Map(Box::new(NBTMapType::from_tag(ct)))),
            Tag::List(lt) => Some(Generic::List(Box::new(NBTListType::from_tag(lt)))),
            Tag::End(_) => None,
            Tag::String(st) => Some(Generic::Str(st.value.clone())),
            Tag::ByteArray(bt) => Some(Generic::Bytes(bt.data.clone())),
            Tag::IntArray(it) => Some(Generic::Ints(it.data.clone())),
            Tag::LongArray(lt) => Some(Generic::Longs(lt.data.clone())),
            Tag::Byte(b) => Some(Generic::Byte(b.value)),
            Tag::Short(s) => Some(Generic::Short(s.value)),
            Tag::Int(i) => Some(Generic::Int(i.value)),
            Tag::Long(l) => Some(Generic::Long(l.value)),
            Tag::Float(f) => Some(Generic::Float(f.value)),
            Tag::Double(d) => Some(Generic::Double(d.value)),
        }
    }

    fn generic_to_base(&self, input: Option<&Generic>) -> Tag {
        let Some(value) = input else {
            return Tag::End(EndTag);
        };
        match value {
            Generic::Map(m) => {
                let nbt_map = m
                    .as_any()
                    .downcast_ref::<NBTMapType>()
                    .expect("Unrecognized map type in NBTTypeUtil::generic_to_base");
                Tag::Compound(nbt_map.get_tag())
            }
            Generic::List(l) => {
                let nbt_list = l
                    .as_any()
                    .downcast_ref::<NBTListType>()
                    .expect("Unrecognized list type in NBTTypeUtil::generic_to_base");
                Tag::List(nbt_list.get_tag())
            }
            Generic::Str(s) => Tag::String(StringTag::value_of(s.clone())),
            Generic::Bool(b) => Tag::Byte(ByteTag::value_of_bool(*b)),
            Generic::Byte(b) => Tag::Byte(ByteTag::value_of(*b)),
            Generic::Short(s) => Tag::Short(ShortTag::value_of(*s)),
            Generic::Int(i) => Tag::Int(IntTag::value_of(*i)),
            Generic::Long(l) => Tag::Long(LongTag::value_of(*l)),
            Generic::Bytes(b) => Tag::ByteArray(ByteArrayTag::new(b.clone())),
            Generic::Ints(i) => Tag::IntArray(IntArrayTag::new(i.clone())),
            Generic::Longs(l) => Tag::LongArray(LongArrayTag::new(l.clone())),
            // Fidelity: Paper's NBTTypeUtil.genericToBase omits Float/Double and Shorts.
            _ => panic!("Unrecognized type in NBTTypeUtil::generic_to_base"),
        }
    }
}
