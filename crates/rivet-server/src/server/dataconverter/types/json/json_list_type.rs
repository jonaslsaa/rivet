//! Concrete JSON list implementation.
//!
//! Mirrors `ca.spottedleaf.dataconverter.types.json.JsonListType` from Paper 26.2.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use rivet_serialization::number::Number;
use serde_json::Value as JsonValue;

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::json::{
    JsonMapType, JsonNode, SharedJsonArray, from_json_array, to_json_array,
};
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use crate::server::dataconverter::types::types::Types;

/// JSON-backed [`ListType`].
#[derive(Debug, Clone)]
pub struct JsonListType {
    pub(crate) array: SharedJsonArray,
    pub(crate) compressed: bool,
}

impl PartialEq for JsonListType {
    fn eq(&self, other: &Self) -> bool {
        // Fidelity: Paper's JsonListType.equals compares only `this.array.equals(other.array)`.
        self.array == other.array
    }
}

impl JsonListType {
    pub fn new(compressed: bool) -> Self {
        Self {
            array: Rc::new(RefCell::new(Vec::new())),
            compressed,
        }
    }

    pub fn from_json(array: &[JsonValue], compressed: bool) -> Self {
        Self {
            array: from_json_array(array),
            compressed,
        }
    }

    pub(crate) fn from_shared(array: SharedJsonArray, compressed: bool) -> Self {
        Self { array, compressed }
    }

    pub fn get_json(&self) -> Vec<JsonValue> {
        to_json_array(&self.array)
    }

    pub fn is_compressed(&self) -> bool {
        self.compressed
    }

    fn try_get_number(&self, index: usize) -> Option<Number> {
        let arr = self.array.borrow();
        match &arr[index] {
            JsonNode::Number(n) => Some(*n),
            JsonNode::ParsedNumber(n) => {
                Some(crate::server::dataconverter::types::json::parsed_number_to_number(n))
            }
            JsonNode::Bool(b) => Some(Number::Byte(if *b { 1 } else { 0 })),
            JsonNode::String(s) if self.compressed => s.parse::<i32>().ok().map(Number::Int),
            _ => None,
        }
    }

    fn try_get_list(&self, index: usize) -> Option<Box<dyn ListType>> {
        let arr = self.array.borrow();
        match &arr[index] {
            JsonNode::Array(a) => Some(Box::new(JsonListType::from_shared(
                Rc::clone(a),
                self.compressed,
            ))),
            _ => None,
        }
    }

    fn try_get_map(&self, index: usize) -> Option<Box<dyn MapType>> {
        let arr = self.array.borrow();
        match &arr[index] {
            JsonNode::Object(obj) => Some(Box::new(JsonMapType::from_shared(
                Rc::clone(obj),
                self.compressed,
            ))),
            _ => None,
        }
    }

    fn try_get_string(&self, index: usize) -> Option<String> {
        let arr = self.array.borrow();
        match &arr[index] {
            JsonNode::String(s) => Some(s.clone()),
            node @ (JsonNode::Number(_) | JsonNode::ParsedNumber(_)) if self.compressed => {
                node.number_as_string()
            }
            _ => None,
        }
    }
}

impl ListType for JsonListType {
    fn get_type_util(&self) -> &'static dyn TypeUtilBase {
        if self.compressed {
            &Types::JSON_COMPRESSED
        } else {
            &Types::JSON
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn copy(&self) -> Box<dyn ListType> {
        let copy = self
            .array
            .borrow()
            .iter()
            .map(JsonNode::deep_copy)
            .collect();
        Box::new(Self {
            array: Rc::new(RefCell::new(copy)),
            compressed: self.compressed,
        })
    }

    fn get_uniform_type(&self) -> ObjectType {
        ObjectType::Undefined
    }

    fn size(&self) -> usize {
        self.array.borrow().len()
    }

    fn remove(&mut self, index: usize) {
        self.array.borrow_mut().remove(index);
    }

    fn get_generic(&self, index: usize) -> Option<Generic> {
        let arr = self.array.borrow();
        let elem = &arr[index];
        // Fidelity: Paper's JsonListType.getGeneric calls `Types.JSON.baseToGeneric(this.array.get(index))`.
        match elem {
            JsonNode::Null => None,
            JsonNode::Object(obj) => Some(Generic::Map(Box::new(JsonMapType::from_shared(
                Rc::clone(obj),
                false,
            )))),
            JsonNode::Array(a) => Some(Generic::List(Box::new(JsonListType::from_shared(
                Rc::clone(a),
                false,
            )))),
            JsonNode::Bool(b) => Some(Generic::Bool(*b)),
            JsonNode::Number(n) => {
                Some(crate::server::dataconverter::types::json::number_to_generic(*n))
            }
            JsonNode::ParsedNumber(n) => Some(
                crate::server::dataconverter::types::json::number_to_generic(
                    crate::server::dataconverter::types::json::convert_parsed_number_to_generic_number(
                        n,
                    ),
                ),
            ),
            JsonNode::String(s) => Some(Generic::Str(s.clone())),
        }
    }

    fn get_number(&self, index: usize) -> Number {
        self.try_get_number(index)
            .expect("Expected number at index in JsonListType::get_number")
    }

    fn get_number_or(&self, index: usize, dfl: Number) -> Number {
        self.try_get_number(index).unwrap_or(dfl)
    }

    fn get_byte(&self, index: usize) -> i8 {
        self.try_get_number(index).map_or(0, |n| n.byte_value())
    }

    fn get_byte_or(&self, index: usize, dfl: i8) -> i8 {
        self.try_get_number(index).map_or(dfl, |n| n.byte_value())
    }

    fn set_byte(&mut self, index: usize, to: i8) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Byte(to));
    }

    fn get_short(&self, index: usize) -> i16 {
        self.try_get_number(index).map_or(0, |n| n.short_value())
    }

    fn get_short_or(&self, index: usize, dfl: i16) -> i16 {
        self.try_get_number(index).map_or(dfl, |n| n.short_value())
    }

    fn set_short(&mut self, index: usize, to: i16) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Short(to));
    }

    fn get_int(&self, index: usize) -> i32 {
        self.try_get_number(index).map_or(0, |n| n.int_value())
    }

    fn get_int_or(&self, index: usize, dfl: i32) -> i32 {
        self.try_get_number(index).map_or(dfl, |n| n.int_value())
    }

    fn set_int(&mut self, index: usize, to: i32) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Int(to));
    }

    fn get_long(&self, index: usize) -> i64 {
        self.try_get_number(index).map_or(0, |n| n.long_value())
    }

    fn get_long_or(&self, index: usize, dfl: i64) -> i64 {
        self.try_get_number(index).map_or(dfl, |n| n.long_value())
    }

    fn set_long(&mut self, index: usize, to: i64) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Long(to));
    }

    fn get_float(&self, index: usize) -> f32 {
        self.try_get_number(index).map_or(0.0, |n| n.float_value())
    }

    fn get_float_or(&self, index: usize, dfl: f32) -> f32 {
        self.try_get_number(index).map_or(dfl, |n| n.float_value())
    }

    fn set_float(&mut self, index: usize, to: f32) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Float(to));
    }

    fn get_double(&self, index: usize) -> f64 {
        self.try_get_number(index).map_or(0.0, |n| n.double_value())
    }

    fn get_double_or(&self, index: usize, dfl: f64) -> f64 {
        self.try_get_number(index).map_or(dfl, |n| n.double_value())
    }

    fn set_double(&mut self, index: usize, to: f64) {
        self.array.borrow_mut()[index] = JsonNode::Number(Number::Double(to));
    }

    fn get_bytes(&self, _index: usize) -> Vec<i8> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_bytes_or(&self, _index: usize, _dfl: Vec<i8>) -> Vec<i8> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn set_bytes(&mut self, _index: usize, _to: Vec<i8>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_shorts(&self, _index: usize) -> Vec<i16> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_shorts_or(&self, _index: usize, _dfl: Vec<i16>) -> Vec<i16> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn set_shorts(&mut self, _index: usize, _to: Vec<i16>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_ints(&self, _index: usize) -> Vec<i32> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_ints_or(&self, _index: usize, _dfl: Vec<i32>) -> Vec<i32> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn set_ints(&mut self, _index: usize, _to: Vec<i32>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_longs(&self, _index: usize) -> Vec<i64> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_longs_or(&self, _index: usize, _dfl: Vec<i64>) -> Vec<i64> {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn set_longs(&mut self, _index: usize, _to: Vec<i64>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn get_list(&self, index: usize) -> Box<dyn ListType> {
        self.try_get_list(index)
            .expect("Expected list at index in JsonListType::get_list")
    }

    fn get_list_or(&self, index: usize, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        self.try_get_list(index).unwrap_or(dfl)
    }

    fn set_list(&mut self, index: usize, list: Box<dyn ListType>) {
        let json_list = list
            .as_any()
            .downcast_ref::<JsonListType>()
            .expect("Expected JsonListType in JsonListType::set_list");
        self.array.borrow_mut()[index] = JsonNode::Array(Rc::clone(&json_list.array));
    }

    fn get_map(&self, index: usize) -> Box<dyn MapType> {
        self.try_get_map(index)
            .expect("Expected map at index in JsonListType::get_map")
    }

    fn get_map_or(&self, index: usize, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        self.try_get_map(index).unwrap_or(dfl)
    }

    fn set_map(&mut self, index: usize, to: Box<dyn MapType>) {
        let json_map = to
            .as_any()
            .downcast_ref::<JsonMapType>()
            .expect("Expected JsonMapType in JsonListType::set_map");
        self.array.borrow_mut()[index] = JsonNode::Object(Rc::clone(&json_map.map));
    }

    fn get_string(&self, index: usize) -> String {
        self.try_get_string(index)
            .expect("Expected string at index in JsonListType::get_string")
    }

    fn get_string_or(&self, index: usize, dfl: String) -> String {
        self.try_get_string(index).unwrap_or(dfl)
    }

    fn set_string(&mut self, index: usize, to: String) {
        self.array.borrow_mut()[index] = JsonNode::String(to);
    }

    fn add_byte(&mut self, b: i8) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Byte(b)));
    }

    fn add_byte_at(&mut self, _index: usize, _b: i8) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_short(&mut self, s: i16) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Short(s)));
    }

    fn add_short_at(&mut self, _index: usize, _s: i16) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_int(&mut self, i: i32) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Int(i)));
    }

    fn add_int_at(&mut self, _index: usize, _i: i32) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_long(&mut self, l: i64) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Long(l)));
    }

    fn add_long_at(&mut self, _index: usize, _l: i64) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_float(&mut self, f: f32) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Float(f)));
    }

    fn add_float_at(&mut self, _index: usize, _f: f32) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_double(&mut self, d: f64) {
        self.array
            .borrow_mut()
            .push(JsonNode::Number(Number::Double(d)));
    }

    fn add_double_at(&mut self, _index: usize, _d: f64) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_byte_array(&mut self, _arr: Vec<i8>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_byte_array_at(&mut self, _index: usize, _arr: Vec<i8>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_short_array(&mut self, _arr: Vec<i16>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_short_array_at(&mut self, _index: usize, _arr: Vec<i16>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_int_array(&mut self, _arr: Vec<i32>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_int_array_at(&mut self, _index: usize, _arr: Vec<i32>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_long_array(&mut self, _arr: Vec<i64>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_long_array_at(&mut self, _index: usize, _arr: Vec<i64>) {
        unimplemented!("JSON does not support raw primitive arrays")
    }

    fn add_list(&mut self, list: Box<dyn ListType>) {
        let json_list = list
            .as_any()
            .downcast_ref::<JsonListType>()
            .expect("Expected JsonListType in JsonListType::add_list");
        self.array
            .borrow_mut()
            .push(JsonNode::Array(Rc::clone(&json_list.array)));
    }

    fn add_list_at(&mut self, _index: usize, _list: Box<dyn ListType>) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_map(&mut self, map: Box<dyn MapType>) {
        let json_map = map
            .as_any()
            .downcast_ref::<JsonMapType>()
            .expect("Expected JsonMapType in JsonListType::add_map");
        self.array
            .borrow_mut()
            .push(JsonNode::Object(Rc::clone(&json_map.map)));
    }

    fn add_map_at(&mut self, _index: usize, _map: Box<dyn MapType>) {
        unimplemented!("JsonListType does not support indexed insertion")
    }

    fn add_string(&mut self, string: String) {
        self.array.borrow_mut().push(JsonNode::String(string));
    }

    fn add_string_at(&mut self, _index: usize, _string: String) {
        unimplemented!("JsonListType does not support indexed insertion")
    }
}
