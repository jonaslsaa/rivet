//! Concrete JSON object implementation.
//!
//! Mirrors `ca.spottedleaf.dataconverter.types.json.JsonMapType` from Paper 26.2.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use rivet_serialization::number::Number;
use serde_json::{Map as JsonObject, Value as JsonValue};

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::json::{
    JsonListType, JsonNode, SharedJsonObject, convert_parsed_number_to_generic_number,
    from_json_object, parsed_number_to_number, to_json_object,
};
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use crate::server::dataconverter::types::types::Types;

/// JSON-backed [`MapType`].
#[derive(Debug, Clone)]
pub struct JsonMapType {
    pub(crate) map: SharedJsonObject,
    pub(crate) compressed: bool,
}

impl PartialEq for JsonMapType {
    fn eq(&self, other: &Self) -> bool {
        // Fidelity: Paper's JsonMapType.equals compares only `this.map.equals(other.map)`.
        self.map == other.map
    }
}

impl JsonMapType {
    pub fn new(compressed: bool) -> Self {
        Self {
            map: Rc::new(RefCell::new(IndexMap::new())),
            compressed,
        }
    }

    pub fn from_json(map: &JsonObject<String, JsonValue>, compressed: bool) -> Self {
        Self {
            map: from_json_object(map),
            compressed,
        }
    }

    pub(crate) fn from_shared(map: SharedJsonObject, compressed: bool) -> Self {
        Self { map, compressed }
    }

    pub fn get_json(&self) -> JsonObject<String, JsonValue> {
        to_json_object(&self.map)
    }

    pub fn is_compressed(&self) -> bool {
        self.compressed
    }

    pub fn get_forced_string(&self, key: &str) -> Option<String> {
        self.get_forced_string_or(key, None)
    }

    pub fn get_forced_string_or(&self, key: &str, dfl: Option<String>) -> Option<String> {
        let map = self.map.borrow();
        let Some(element) = map.get(key) else {
            return dfl;
        };
        match element {
            JsonNode::String(s) => Some(s.clone()),
            node @ (JsonNode::Number(_) | JsonNode::ParsedNumber(_)) => node.number_as_string(),
            JsonNode::Bool(b) => Some(b.to_string()),
            JsonNode::Null => Some("null".to_owned()),
            JsonNode::Array(_) | JsonNode::Object(_) => {
                Some(serde_json::to_string(&element.to_value()).unwrap_or_default())
            }
        }
    }
}

impl MapType for JsonMapType {
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
        let mut copy = IndexMap::with_capacity(self.map.borrow().len());
        for (k, v) in self.map.borrow().iter() {
            copy.insert(k.clone(), v.deep_copy());
        }
        Box::new(Self {
            map: Rc::new(RefCell::new(copy)),
            compressed: self.compressed,
        })
    }

    fn has_key(&self, key: &str) -> bool {
        self.map.borrow().contains_key(key)
    }

    fn has_key_of_type(&self, key: &str, ty: ObjectType) -> bool {
        let map = self.map.borrow();
        let Some(element) = map.get(key) else {
            return false;
        };

        if ty == ObjectType::Undefined {
            return true;
        }

        match element {
            JsonNode::Array(_) => ty == ObjectType::List,
            JsonNode::Object(_) => ty == ObjectType::Map,
            JsonNode::Null => false,
            JsonNode::String(_) => {
                ty == ObjectType::String || (self.compressed && ty == ObjectType::Number)
            }
            JsonNode::Bool(_) => ty.is_number(),
            JsonNode::Number(n) => {
                let exact = match n {
                    Number::Byte(_) => ObjectType::Byte,
                    Number::Short(_) => ObjectType::Short,
                    Number::Int(_) => ObjectType::Int,
                    Number::Long(_) => ObjectType::Long,
                    Number::Float(_) => ObjectType::Float,
                    Number::Double(_) => ObjectType::Double,
                };
                ty == exact || (self.compressed && ty == ObjectType::String)
            }
            JsonNode::ParsedNumber(_) => {
                // Fidelity: Gson's LazilyParsedNumber is not Byte/Short/Integer/Long/Float,
                // so Paper's JsonMapType.hasKey falls into the `else` branch (ObjectType.DOUBLE).
                ty == ObjectType::Double || (self.compressed && ty == ObjectType::String)
            }
        }
    }

    fn remove(&mut self, key: &str) {
        self.map.borrow_mut().shift_remove(key);
    }

    fn get_generic(&self, key: &str) -> Option<Generic> {
        let map = self.map.borrow();
        let element = map.get(key)?;
        match element {
            JsonNode::Null => None,
            JsonNode::Object(obj) => Some(Generic::Map(Box::new(JsonMapType::from_shared(
                Rc::clone(obj),
                self.compressed,
            )))),
            JsonNode::Array(arr) => Some(Generic::List(Box::new(JsonListType::from_shared(
                Rc::clone(arr),
                self.compressed,
            )))),
            JsonNode::Number(n) => {
                Some(crate::server::dataconverter::types::json::number_to_generic(*n))
            }
            JsonNode::ParsedNumber(n) => Some(
                crate::server::dataconverter::types::json::number_to_generic(
                    convert_parsed_number_to_generic_number(n),
                ),
            ),
            JsonNode::String(s) => Some(Generic::Str(s.clone())),
            JsonNode::Bool(b) => Some(Generic::Bool(*b)),
        }
    }

    fn get_number(&self, key: &str) -> Option<Number> {
        let map = self.map.borrow();
        let element = map.get(key)?;
        match element {
            JsonNode::Number(n) => Some(*n),
            JsonNode::ParsedNumber(n) => Some(parsed_number_to_number(n)),
            JsonNode::Bool(b) => Some(Number::Byte(if *b { 1 } else { 0 })),
            JsonNode::String(s) if self.compressed => s.parse::<i32>().ok().map(Number::Int),
            _ => None,
        }
    }

    fn get_number_or(&self, key: &str, dfl: Number) -> Number {
        self.get_number(key).unwrap_or(dfl)
    }

    fn get_boolean(&self, key: &str) -> bool {
        self.get_boolean_or(key, false)
    }

    fn get_boolean_or(&self, key: &str, dfl: bool) -> bool {
        let map = self.map.borrow();
        if let Some(element) = map.get(key) {
            match element {
                JsonNode::Number(n) => return n.byte_value() != 0,
                JsonNode::ParsedNumber(n) => {
                    return crate::server::dataconverter::types::json::parse_decimal_trunc_wrapping_i64(
                        n.as_str(),
                    ) as i8
                        != 0;
                }
                JsonNode::Bool(b) => return *b,
                _ => {}
            }
        }
        dfl
    }

    fn set_boolean(&mut self, key: &str, val: bool) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Bool(val));
    }

    fn get_byte(&self, key: &str) -> i8 {
        self.get_byte_or(key, 0)
    }

    fn get_byte_or(&self, key: &str, dfl: i8) -> i8 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_byte(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_byte(&mut self, key: &str, val: i8) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Byte(val)));
    }

    fn get_short(&self, key: &str) -> i16 {
        self.get_short_or(key, 0)
    }

    fn get_short_or(&self, key: &str, dfl: i16) -> i16 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_short(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_short(&mut self, key: &str, val: i16) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Short(val)));
    }

    fn get_int(&self, key: &str) -> i32 {
        self.get_int_or(key, 0)
    }

    fn get_int_or(&self, key: &str, dfl: i32) -> i32 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_int(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_int(&mut self, key: &str, val: i32) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Int(val)));
    }

    fn get_long(&self, key: &str) -> i64 {
        self.get_long_or(key, 0)
    }

    fn get_long_or(&self, key: &str, dfl: i64) -> i64 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_long(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_long(&mut self, key: &str, val: i64) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Long(val)));
    }

    fn get_float(&self, key: &str) -> f32 {
        self.get_float_or(key, 0.0)
    }

    fn get_float_or(&self, key: &str, dfl: f32) -> f32 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_float(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_float(&mut self, key: &str, val: f32) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Float(val)));
    }

    fn get_double(&self, key: &str) -> f64 {
        self.get_double_or(key, 0.0)
    }

    fn get_double_or(&self, key: &str, dfl: f64) -> f64 {
        self.map
            .borrow()
            .get(key)
            .and_then(|n| n.as_double(self.compressed))
            .unwrap_or(dfl)
    }

    fn set_double(&mut self, key: &str, val: f64) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Number(Number::Double(val)));
    }

    fn get_bytes(&self, _key: &str) -> Option<Vec<i8>> {
        None
    }

    fn get_bytes_or(&self, _key: &str, dfl: Vec<i8>) -> Vec<i8> {
        dfl
    }

    fn set_bytes(&mut self, _key: &str, _val: Vec<i8>) {
        panic!("JSON does not support raw primitive arrays")
    }

    fn get_shorts(&self, _key: &str) -> Option<Vec<i16>> {
        None
    }

    fn get_shorts_or(&self, _key: &str, dfl: Vec<i16>) -> Vec<i16> {
        dfl
    }

    fn set_shorts(&mut self, _key: &str, _val: Vec<i16>) {
        panic!("JSON does not support raw primitive arrays")
    }

    fn get_ints(&self, _key: &str) -> Option<Vec<i32>> {
        None
    }

    fn get_ints_or(&self, _key: &str, dfl: Vec<i32>) -> Vec<i32> {
        dfl
    }

    fn set_ints(&mut self, _key: &str, _val: Vec<i32>) {
        panic!("JSON does not support raw primitive arrays")
    }

    fn get_longs(&self, _key: &str) -> Option<Vec<i64>> {
        None
    }

    fn get_longs_or(&self, _key: &str, dfl: Vec<i64>) -> Vec<i64> {
        dfl
    }

    fn set_longs(&mut self, _key: &str, _val: Vec<i64>) {
        panic!("JSON does not support raw primitive arrays")
    }

    fn get_list_unchecked(&self, key: &str) -> Option<Box<dyn ListType>> {
        let map = self.map.borrow();
        if let Some(JsonNode::Array(arr)) = map.get(key) {
            return Some(Box::new(JsonListType::from_shared(
                Rc::clone(arr),
                self.compressed,
            )));
        }
        None
    }

    fn get_list_unchecked_or(&self, key: &str, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        self.get_list_unchecked(key).unwrap_or(dfl)
    }

    fn set_list(&mut self, key: &str, val: Box<dyn ListType>) {
        let json_list = val
            .as_any()
            .downcast_ref::<JsonListType>()
            .expect("Expected JsonListType in JsonMapType::set_list");
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Array(Rc::clone(&json_list.array)));
    }

    fn get_map(&self, key: &str) -> Option<Box<dyn MapType>> {
        let map = self.map.borrow();
        if let Some(JsonNode::Object(obj)) = map.get(key) {
            return Some(Box::new(JsonMapType::from_shared(
                Rc::clone(obj),
                self.compressed,
            )));
        }
        None
    }

    fn get_map_or(&self, key: &str, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        self.get_map(key).unwrap_or(dfl)
    }

    fn set_map(&mut self, key: &str, val: Box<dyn MapType>) {
        let json_map = val
            .as_any()
            .downcast_ref::<JsonMapType>()
            .expect("Expected JsonMapType in JsonMapType::set_map");
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::Object(Rc::clone(&json_map.map)));
    }

    fn get_string(&self, key: &str) -> Option<String> {
        let map = self.map.borrow();
        if let Some(element) = map.get(key) {
            match element {
                JsonNode::String(s) => return Some(s.clone()),
                node @ (JsonNode::Number(_) | JsonNode::ParsedNumber(_)) if self.compressed => {
                    return node.number_as_string();
                }
                _ => {}
            }
        }
        None
    }

    fn get_string_or(&self, key: &str, dfl: String) -> String {
        self.get_string(key).unwrap_or(dfl)
    }

    fn set_string(&mut self, key: &str, val: String) {
        self.map
            .borrow_mut()
            .insert(key.to_owned(), JsonNode::String(val));
    }
}
