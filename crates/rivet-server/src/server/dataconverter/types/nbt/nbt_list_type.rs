//! Port of `ca.spottedleaf.dataconverter.types.nbt.NBTListType`.

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::nbt::nbt_map_type::NBTMapType;
use crate::server::dataconverter::types::nbt::{
    NbtValue, SharedNbtList, from_list_tag, tag_id_to_object_type, to_list_tag,
};
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use crate::server::dataconverter::types::types::Types;
use rivet_nbt::list_tag::ListTag;
use rivet_serialization::number::Number;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

/// `NBTListType` — NBT-backed implementation of [`ListType`].
#[derive(Debug, Clone, PartialEq)]
pub struct NBTListType {
    pub(crate) list: SharedNbtList,
}

impl Default for NBTListType {
    fn default() -> Self {
        Self::new()
    }
}

impl NBTListType {
    /// `new NBTListType()`.
    pub fn new() -> Self {
        Self {
            list: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// `new NBTListType(ListTag)`.
    pub fn from_tag(tag: &ListTag) -> Self {
        Self {
            list: from_list_tag(tag),
        }
    }

    pub(crate) fn from_shared(list: SharedNbtList) -> Self {
        Self { list }
    }

    /// `NBTListType.getTag()`.
    pub fn get_tag(&self) -> ListTag {
        to_list_tag(&self.list)
    }

    /// Package-private `NBTListType.getType(byte)`.
    pub fn get_type(id: i8) -> ObjectType {
        tag_id_to_object_type(id)
    }
}

impl ListType for NBTListType {
    fn get_type_util(&self) -> &dyn TypeUtilBase {
        &Types::NBT
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn copy(&self) -> Box<dyn ListType> {
        let copied: Vec<NbtValue> = self.list.borrow().iter().map(NbtValue::deep_copy).collect();
        Box::new(Self {
            list: Rc::new(RefCell::new(copied)),
        })
    }

    fn get_uniform_type(&self) -> ObjectType {
        let mut curr: Option<ObjectType> = None;
        for tag in self.list.borrow().iter() {
            let tag_type = tag_id_to_object_type(tag.tag_id());
            match curr {
                None => curr = Some(tag_type),
                Some(prev) if prev != tag_type => return ObjectType::Mixed,
                Some(_) => {}
            }
        }
        curr.unwrap_or(ObjectType::None)
    }

    fn size(&self) -> usize {
        self.list.borrow().len()
    }

    fn remove(&mut self, index: usize) {
        self.list.borrow_mut().remove(index);
    }

    fn get_generic(&self, index: usize) -> Option<Generic> {
        let list = self.list.borrow();
        let tag = &list[index];
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

    fn get_number(&self, index: usize) -> Number {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_number()
            .expect("Expected NumericTag in NBTListType::get_number")
    }

    fn get_number_or(&self, index: usize, dfl: Number) -> Number {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_number().unwrap_or(dfl)
    }

    fn get_byte(&self, index: usize) -> i8 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_byte")
            .byte_value()
    }

    fn get_byte_or(&self, index: usize, dfl: i8) -> i8 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.byte_value())
    }

    fn set_byte(&mut self, index: usize, to: i8) {
        self.list.borrow_mut()[index] = NbtValue::Byte(to);
    }

    fn get_short(&self, index: usize) -> i16 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_short")
            .short_value()
    }

    fn get_short_or(&self, index: usize, dfl: i16) -> i16 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.short_value())
    }

    fn set_short(&mut self, index: usize, to: i16) {
        self.list.borrow_mut()[index] = NbtValue::Short(to);
    }

    fn get_int(&self, index: usize) -> i32 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_int")
            .int_value()
    }

    fn get_int_or(&self, index: usize, dfl: i32) -> i32 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.int_value())
    }

    fn set_int(&mut self, index: usize, to: i32) {
        self.list.borrow_mut()[index] = NbtValue::Int(to);
    }

    fn get_long(&self, index: usize) -> i64 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_long")
            .long_value()
    }

    fn get_long_or(&self, index: usize, dfl: i64) -> i64 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.long_value())
    }

    fn set_long(&mut self, index: usize, to: i64) {
        self.list.borrow_mut()[index] = NbtValue::Long(to);
    }

    fn get_float(&self, index: usize) -> f32 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_float")
            .float_value()
    }

    fn get_float_or(&self, index: usize, dfl: f32) -> f32 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.float_value())
    }

    fn set_float(&mut self, index: usize, to: f32) {
        self.list.borrow_mut()[index] = NbtValue::Float(to);
    }

    fn get_double(&self, index: usize) -> f64 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag()
            .expect("Expected NumericTag in NBTListType::get_double")
            .double_value()
    }

    fn get_double_or(&self, index: usize, dfl: f64) -> f64 {
        let list = self.list.borrow();
        let tag = &list[index];
        tag.as_numeric_tag().map_or(dfl, |n| n.double_value())
    }

    fn set_double(&mut self, index: usize, to: f64) {
        self.list.borrow_mut()[index] = NbtValue::Double(to);
    }

    fn get_bytes(&self, index: usize) -> Vec<i8> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::ByteArray(arr) => arr.clone(),
            _ => panic!("Expected ByteArrayTag in NBTListType::get_bytes"),
        }
    }

    fn get_bytes_or(&self, index: usize, dfl: Vec<i8>) -> Vec<i8> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::ByteArray(arr) => arr.clone(),
            _ => dfl,
        }
    }

    fn set_bytes(&mut self, index: usize, to: Vec<i8>) {
        self.list.borrow_mut()[index] = NbtValue::ByteArray(to);
    }

    fn get_shorts(&self, _index: usize) -> Vec<i16> {
        unimplemented!("NBT does not support short arrays")
    }

    fn get_shorts_or(&self, _index: usize, _dfl: Vec<i16>) -> Vec<i16> {
        unimplemented!("NBT does not support short arrays")
    }

    fn set_shorts(&mut self, _index: usize, _to: Vec<i16>) {
        unimplemented!("NBT does not support short arrays")
    }

    fn get_ints(&self, index: usize) -> Vec<i32> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::IntArray(arr) => arr.clone(),
            _ => panic!("Expected IntArrayTag in NBTListType::get_ints"),
        }
    }

    fn get_ints_or(&self, index: usize, dfl: Vec<i32>) -> Vec<i32> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::IntArray(arr) => arr.clone(),
            _ => dfl,
        }
    }

    fn set_ints(&mut self, index: usize, to: Vec<i32>) {
        self.list.borrow_mut()[index] = NbtValue::IntArray(to);
    }

    fn get_longs(&self, index: usize) -> Vec<i64> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::LongArray(arr) => arr.clone(),
            _ => panic!("Expected LongArrayTag in NBTListType::get_longs"),
        }
    }

    fn get_longs_or(&self, index: usize, dfl: Vec<i64>) -> Vec<i64> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::LongArray(arr) => arr.clone(),
            _ => dfl,
        }
    }

    fn set_longs(&mut self, index: usize, to: Vec<i64>) {
        self.list.borrow_mut()[index] = NbtValue::LongArray(to);
    }

    fn get_list(&self, index: usize) -> Box<dyn ListType> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::List(l) => Box::new(NBTListType::from_shared(Rc::clone(l))),
            _ => panic!("Expected ListTag in NBTListType::get_list"),
        }
    }

    fn get_list_or(&self, index: usize, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::List(l) => Box::new(NBTListType::from_shared(Rc::clone(l))),
            _ => dfl,
        }
    }

    fn set_list(&mut self, index: usize, list: Box<dyn ListType>) {
        let nbt_list = list
            .as_any()
            .downcast_ref::<NBTListType>()
            .expect("Expected NBTListType in NBTListType::set_list");
        self.list.borrow_mut()[index] = NbtValue::List(Rc::clone(&nbt_list.list));
    }

    fn get_map(&self, index: usize) -> Box<dyn MapType> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::Compound(m) => Box::new(NBTMapType::from_shared(Rc::clone(m))),
            _ => panic!("Expected CompoundTag in NBTListType::get_map"),
        }
    }

    fn get_map_or(&self, index: usize, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::Compound(m) => Box::new(NBTMapType::from_shared(Rc::clone(m))),
            _ => dfl,
        }
    }

    fn set_map(&mut self, index: usize, to: Box<dyn MapType>) {
        let nbt_map = to
            .as_any()
            .downcast_ref::<NBTMapType>()
            .expect("Expected NBTMapType in NBTListType::set_map");
        self.list.borrow_mut()[index] = NbtValue::Compound(Rc::clone(&nbt_map.map));
    }

    fn get_string(&self, index: usize) -> String {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::String(s) => s.clone(),
            _ => panic!("Expected StringTag in NBTListType::get_string"),
        }
    }

    fn get_string_or(&self, index: usize, dfl: String) -> String {
        let list = self.list.borrow();
        match &list[index] {
            NbtValue::String(s) => s.clone(),
            _ => dfl,
        }
    }

    fn set_string(&mut self, index: usize, to: String) {
        self.list.borrow_mut()[index] = NbtValue::String(to);
    }

    fn add_byte(&mut self, b: i8) {
        self.list.borrow_mut().push(NbtValue::Byte(b));
    }

    fn add_byte_at(&mut self, index: usize, b: i8) {
        self.list.borrow_mut().insert(index, NbtValue::Byte(b));
    }

    fn add_short(&mut self, s: i16) {
        self.list.borrow_mut().push(NbtValue::Short(s));
    }

    fn add_short_at(&mut self, index: usize, s: i16) {
        self.list.borrow_mut().insert(index, NbtValue::Short(s));
    }

    fn add_int(&mut self, i: i32) {
        self.list.borrow_mut().push(NbtValue::Int(i));
    }

    fn add_int_at(&mut self, index: usize, i: i32) {
        self.list.borrow_mut().insert(index, NbtValue::Int(i));
    }

    fn add_long(&mut self, l: i64) {
        self.list.borrow_mut().push(NbtValue::Long(l));
    }

    fn add_long_at(&mut self, index: usize, l: i64) {
        self.list.borrow_mut().insert(index, NbtValue::Long(l));
    }

    fn add_float(&mut self, f: f32) {
        self.list.borrow_mut().push(NbtValue::Float(f));
    }

    fn add_float_at(&mut self, index: usize, f: f32) {
        self.list.borrow_mut().insert(index, NbtValue::Float(f));
    }

    fn add_double(&mut self, d: f64) {
        self.list.borrow_mut().push(NbtValue::Double(d));
    }

    fn add_double_at(&mut self, index: usize, d: f64) {
        self.list.borrow_mut().insert(index, NbtValue::Double(d));
    }

    fn add_byte_array(&mut self, arr: Vec<i8>) {
        self.list.borrow_mut().push(NbtValue::ByteArray(arr));
    }

    fn add_byte_array_at(&mut self, index: usize, arr: Vec<i8>) {
        self.list
            .borrow_mut()
            .insert(index, NbtValue::ByteArray(arr));
    }

    fn add_short_array(&mut self, _arr: Vec<i16>) {
        unimplemented!("NBT does not support short arrays")
    }

    fn add_short_array_at(&mut self, _index: usize, _arr: Vec<i16>) {
        unimplemented!("NBT does not support short arrays")
    }

    fn add_int_array(&mut self, arr: Vec<i32>) {
        self.list.borrow_mut().push(NbtValue::IntArray(arr));
    }

    fn add_int_array_at(&mut self, index: usize, arr: Vec<i32>) {
        self.list
            .borrow_mut()
            .insert(index, NbtValue::IntArray(arr));
    }

    fn add_long_array(&mut self, arr: Vec<i64>) {
        self.list.borrow_mut().push(NbtValue::LongArray(arr));
    }

    fn add_long_array_at(&mut self, index: usize, arr: Vec<i64>) {
        self.list
            .borrow_mut()
            .insert(index, NbtValue::LongArray(arr));
    }

    fn add_list(&mut self, list: Box<dyn ListType>) {
        let nbt_list = list
            .as_any()
            .downcast_ref::<NBTListType>()
            .expect("Expected NBTListType in NBTListType::add_list");
        self.list
            .borrow_mut()
            .push(NbtValue::List(Rc::clone(&nbt_list.list)));
    }

    fn add_list_at(&mut self, index: usize, list: Box<dyn ListType>) {
        let nbt_list = list
            .as_any()
            .downcast_ref::<NBTListType>()
            .expect("Expected NBTListType in NBTListType::add_list_at");
        self.list
            .borrow_mut()
            .insert(index, NbtValue::List(Rc::clone(&nbt_list.list)));
    }

    fn add_map(&mut self, map: Box<dyn MapType>) {
        let nbt_map = map
            .as_any()
            .downcast_ref::<NBTMapType>()
            .expect("Expected NBTMapType in NBTListType::add_map");
        self.list
            .borrow_mut()
            .push(NbtValue::Compound(Rc::clone(&nbt_map.map)));
    }

    fn add_map_at(&mut self, index: usize, map: Box<dyn MapType>) {
        let nbt_map = map
            .as_any()
            .downcast_ref::<NBTMapType>()
            .expect("Expected NBTMapType in NBTListType::add_map_at");
        self.list
            .borrow_mut()
            .insert(index, NbtValue::Compound(Rc::clone(&nbt_map.map)));
    }

    fn add_string(&mut self, string: String) {
        self.list.borrow_mut().push(NbtValue::String(string));
    }

    fn add_string_at(&mut self, index: usize, string: String) {
        self.list
            .borrow_mut()
            .insert(index, NbtValue::String(string));
    }
}
