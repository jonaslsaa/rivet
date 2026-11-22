//! Port of `ca.spottedleaf.dataconverter.types.nbt.NBTMapType`.

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::nbt::nbt_list_type::NBTListType;
use crate::server::dataconverter::types::nbt::{
    NbtValue, SharedNbtCompound, from_compound_tag, tag_id_to_object_type, to_compound_tag,
};
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use crate::server::dataconverter::types::types::Types;
use indexmap::IndexMap;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_serialization::number::Number;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

/// `NBTMapType` — NBT-backed implementation of [`MapType`].
#[derive(Debug, Clone, PartialEq)]
pub struct NBTMapType {
    pub(crate) map: SharedNbtCompound,
}

impl Default for NBTMapType {
    fn default() -> Self {
        Self::new()
    }
}

impl NBTMapType {
    /// `new NBTMapType()`.
    pub fn new() -> Self {
        Self {
            map: Rc::new(RefCell::new(IndexMap::new())),
        }
    }

    /// `new NBTMapType(CompoundTag)`.
    pub fn from_tag(tag: &CompoundTag) -> Self {
        Self {
            map: from_compound_tag(tag),
        }
    }

    pub(crate) fn from_shared(map: SharedNbtCompound) -> Self {
        Self { map }
    }

    /// `NBTMapType.getTag()`.
    pub fn get_tag(&self) -> CompoundTag {
        to_compound_tag(&self.map)
    }
}

impl MapType for NBTMapType {
    fn get_type_util(&self) -> &dyn TypeUtilBase {
        &Types::NBT
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn size(&self) -> usize {
        self.map.borrow().len()
    }

    fn is_empty(&self) -> bool {
        self.map.borrow().is_empty()
    }

    fn clear(&mut self) {
        self.map.borrow_mut().clear();
    }

    fn keys(&self) -> Vec<String> {
        self.map.borrow().keys().cloned().collect()
    }

    fn copy(&self) -> Box<dyn MapType> {
        let copied: IndexMap<String, NbtValue> = self
            .map
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.deep_copy()))
            .collect();
        Box::new(Self {
            map: Rc::new(RefCell::new(copied)),
        })
    }

    fn has_key(&self, key: &str) -> bool {
        self.map.borrow().contains_key(key)
    }

    fn has_key_of_type(&self, key: &str, ty: ObjectType) -> bool {
        let map = self.map.borrow();
        let Some(tag) = map.get(key) else {
            return false;
        };
        let value_type = tag_id_to_object_type(tag.tag_id());
        value_type == ty || (ty == ObjectType::Number && value_type.is_number())
    }

    fn remove(&mut self, key: &str) {
        self.map.borrow_mut().shift_remove(key);
    }

    fn get_generic(&self, key: &str) -> Option<Generic> {
        let map = self.map.borrow();
        let tag = map.get(key)?;
        match tag {
            NbtValue::Byte(v) => Some(Generic::Byte(*v)),
            NbtValue::Short(v) => Some(Generic::Short(*v)),
            NbtValue::Int(v) => Some(Generic::Int(*v)),
            NbtValue::Long(v) => Some(Generic::Long(*v)),
            NbtValue::Float(v) => Some(Generic::Float(*v)),
            NbtValue::Double(v) => Some(Generic::Double(*v)),
            NbtValue::Compound(m) => Some(Generic::Map(Box::new(NBTMapType::from_shared(
                Rc::clone(m),
            )))),
            NbtValue::List(l) => Some(Generic::List(Box::new(NBTListType::from_shared(
                Rc::clone(l),
            )))),
            NbtValue::String(s) => Some(Generic::Str(s.clone())),
            NbtValue::ByteArray(a) => Some(Generic::Bytes(a.clone())),
            NbtValue::IntArray(a) => Some(Generic::Ints(a.clone())),
            NbtValue::LongArray(a) => Some(Generic::Longs(a.clone())),
            NbtValue::End => panic!("Unrecognized type End"),
        }
    }

    fn get_number(&self, key: &str) -> Option<Number> {
        self.map.borrow().get(key).and_then(NbtValue::as_number)
    }

    fn get_number_or(&self, key: &str, dfl: Number) -> Number {
        self.get_number(key).unwrap_or(dfl)
    }

    fn get_boolean(&self, key: &str) -> bool {
        self.get_byte(key) != 0
    }

    fn get_boolean_or(&self, key: &str, dfl: bool) -> bool {
        self.get_byte_or(key, if dfl { 1 } else { 0 }) != 0
    }

    fn set_boolean(&mut self, key: &str, val: bool) {
        self.set_byte(key, if val { 1 } else { 0 });
    }

    fn get_byte(&self, key: &str) -> i8 {
        self.get_byte_or(key, 0)
    }

    fn get_byte_or(&self, key: &str, dfl: i8) -> i8 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.byte_value())
    }

    fn set_byte(&mut self, key: &str, val: i8) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Byte(val));
    }

    fn get_short(&self, key: &str) -> i16 {
        self.get_short_or(key, 0)
    }

    fn get_short_or(&self, key: &str, dfl: i16) -> i16 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.short_value())
    }

    fn set_short(&mut self, key: &str, val: i16) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Short(val));
    }

    fn get_int(&self, key: &str) -> i32 {
        self.get_int_or(key, 0)
    }

    fn get_int_or(&self, key: &str, dfl: i32) -> i32 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.int_value())
    }

    fn set_int(&mut self, key: &str, val: i32) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Int(val));
    }

    fn get_long(&self, key: &str) -> i64 {
        self.get_long_or(key, 0)
    }

    fn get_long_or(&self, key: &str, dfl: i64) -> i64 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.long_value())
    }

    fn set_long(&mut self, key: &str, val: i64) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Long(val));
    }

    fn get_float(&self, key: &str) -> f32 {
        self.get_float_or(key, 0.0)
    }

    fn get_float_or(&self, key: &str, dfl: f32) -> f32 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.float_value())
    }

    fn set_float(&mut self, key: &str, val: f32) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Float(val));
    }

    fn get_double(&self, key: &str) -> f64 {
        self.get_double_or(key, 0.0)
    }

    fn get_double_or(&self, key: &str, dfl: f64) -> f64 {
        self.map
            .borrow()
            .get(key)
            .and_then(NbtValue::as_numeric_tag)
            .map_or(dfl, |n| n.double_value())
    }

    fn set_double(&mut self, key: &str, val: f64) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Double(val));
    }

    fn get_bytes(&self, key: &str) -> Option<Vec<i8>> {
        match self.map.borrow().get(key) {
            Some(NbtValue::ByteArray(arr)) => Some(arr.clone()),
            _ => None,
        }
    }

    fn get_bytes_or(&self, key: &str, dfl: Vec<i8>) -> Vec<i8> {
        self.get_bytes(key).unwrap_or(dfl)
    }

    fn set_bytes(&mut self, key: &str, val: Vec<i8>) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::ByteArray(val));
    }

    fn get_shorts(&self, _key: &str) -> Option<Vec<i16>> {
        // NBT does not support short array
        None
    }

    fn get_shorts_or(&self, _key: &str, dfl: Vec<i16>) -> Vec<i16> {
        // NBT does not support short array
        dfl
    }

    fn set_shorts(&mut self, _key: &str, _val: Vec<i16>) {
        unimplemented!("NBT does not support short arrays")
    }

    fn get_ints(&self, key: &str) -> Option<Vec<i32>> {
        match self.map.borrow().get(key) {
            Some(NbtValue::IntArray(arr)) => Some(arr.clone()),
            _ => None,
        }
    }

    fn get_ints_or(&self, key: &str, dfl: Vec<i32>) -> Vec<i32> {
        self.get_ints(key).unwrap_or(dfl)
    }

    fn set_ints(&mut self, key: &str, val: Vec<i32>) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::IntArray(val));
    }

    fn get_longs(&self, key: &str) -> Option<Vec<i64>> {
        match self.map.borrow().get(key) {
            Some(NbtValue::LongArray(arr)) => Some(arr.clone()),
            _ => None,
        }
    }

    fn get_longs_or(&self, key: &str, dfl: Vec<i64>) -> Vec<i64> {
        self.get_longs(key).unwrap_or(dfl)
    }

    fn set_longs(&mut self, key: &str, val: Vec<i64>) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::LongArray(val));
    }

    fn get_list_unchecked(&self, key: &str) -> Option<Box<dyn ListType>> {
        match self.map.borrow().get(key) {
            Some(NbtValue::List(l)) => Some(Box::new(NBTListType::from_shared(Rc::clone(l)))),
            _ => None,
        }
    }

    fn get_list_unchecked_or(&self, key: &str, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        self.get_list_unchecked(key).unwrap_or(dfl)
    }

    fn set_list(&mut self, key: &str, val: Box<dyn ListType>) {
        let nbt_list = val
            .as_any()
            .downcast_ref::<NBTListType>()
            .expect("Expected NBTListType in NBTMapType::set_list");
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::List(Rc::clone(&nbt_list.list)));
    }

    fn get_map(&self, key: &str) -> Option<Box<dyn MapType>> {
        match self.map.borrow().get(key) {
            Some(NbtValue::Compound(m)) => Some(Box::new(NBTMapType::from_shared(Rc::clone(m)))),
            _ => None,
        }
    }

    fn get_map_or(&self, key: &str, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        self.get_map(key).unwrap_or(dfl)
    }

    fn set_map(&mut self, key: &str, val: Box<dyn MapType>) {
        let nbt_map = val
            .as_any()
            .downcast_ref::<NBTMapType>()
            .expect("Expected NBTMapType in NBTMapType::set_map");
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::Compound(Rc::clone(&nbt_map.map)));
    }

    fn get_string(&self, key: &str) -> Option<String> {
        match self.map.borrow().get(key) {
            Some(NbtValue::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn get_string_or(&self, key: &str, dfl: String) -> String {
        self.get_string(key).unwrap_or(dfl)
    }

    fn set_string(&mut self, key: &str, val: String) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), NbtValue::String(val));
    }
}
