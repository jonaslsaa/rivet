//! JSON [`TypeUtil`] implementation.
//!
//! Mirrors `ca.spottedleaf.dataconverter.types.json.JsonTypeUtil` from Paper 26.2.

use serde_json::{Map as JsonObject, Value as JsonValue};

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::json::{
    JsonListType, JsonMapType, JsonNode, convert_parsed_number_to_generic_number,
    number_to_json_value,
};
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::type_util::{TypeUtil, TypeUtilBase};

/// Type utility for `serde_json::Value` (Paper's `JsonElement`).
#[derive(Debug, Clone, Copy)]
pub struct JsonTypeUtil {
    compressed: bool,
}

impl JsonTypeUtil {
    pub const fn new(compressed: bool) -> Self {
        Self { compressed }
    }

    pub const fn is_compressed(&self) -> bool {
        self.compressed
    }

    fn convert_json_node_to_generic(to: &dyn TypeUtilBase, node: &JsonNode) -> Option<Generic> {
        match node {
            JsonNode::Null => None,
            JsonNode::Bool(b) => Some(Generic::Bool(*b)),
            JsonNode::Number(n) => {
                Some(crate::server::dataconverter::types::json::number_to_generic(*n))
            }
            JsonNode::ParsedNumber(n) => Some(
                crate::server::dataconverter::types::json::number_to_generic(
                    convert_parsed_number_to_generic_number(n),
                ),
            ),
            JsonNode::String(s) => Some(Generic::Str(s.clone())),
            JsonNode::Array(arr) => {
                let mut ret = to.create_empty_list();
                for elem in arr.borrow().iter() {
                    let converted = Self::convert_json_node_to_generic(to, elem)
                        .expect("Cannot add null JSON element to ListType");
                    ret.add_generic(converted);
                }
                Some(Generic::List(ret))
            }
            JsonNode::Object(obj) => {
                let mut ret = to.create_empty_map();
                for (k, v) in obj.borrow().iter() {
                    if let Some(converted) = Self::convert_json_node_to_generic(to, v) {
                        ret.set_generic(k, converted);
                    }
                }
                Some(Generic::Map(ret))
            }
        }
    }

    fn convert_json_to_generic(to: &dyn TypeUtilBase, element: &JsonValue) -> Option<Generic> {
        match element {
            JsonValue::Null => None,
            JsonValue::Bool(b) => Some(Generic::Bool(*b)),
            JsonValue::Number(n) => Some(
                crate::server::dataconverter::types::json::number_to_generic(
                    convert_parsed_number_to_generic_number(n),
                ),
            ),
            JsonValue::String(s) => Some(Generic::Str(s.clone())),
            JsonValue::Array(arr) => Some(Generic::List(Self::convert_json_array(to, arr))),
            JsonValue::Object(obj) => Some(Generic::Map(Self::convert_json_object(to, obj))),
        }
    }

    fn convert_json_object(
        to: &dyn TypeUtilBase,
        json: &JsonObject<String, JsonValue>,
    ) -> Box<dyn MapType> {
        let mut ret = to.create_empty_map();
        for (key, value) in json {
            let Some(obj) = Self::convert_json_to_generic(to, value) else {
                continue;
            };
            ret.set_generic(key, obj);
        }
        ret
    }

    fn convert_json_array(to: &dyn TypeUtilBase, json: &[JsonValue]) -> Box<dyn ListType> {
        let mut ret = to.create_empty_list();
        for elem in json {
            let converted = Self::convert_json_to_generic(to, elem)
                .expect("Cannot add null JSON element to ListType");
            ret.add_generic(converted);
        }
        ret
    }
}

impl TypeUtilBase for JsonTypeUtil {
    fn create_empty_list(&self) -> Box<dyn ListType> {
        Box::new(JsonListType::new(self.compressed))
    }

    fn create_empty_map(&self) -> Box<dyn MapType> {
        Box::new(JsonMapType::new(self.compressed))
    }
}

impl TypeUtil<JsonValue> for JsonTypeUtil {
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
            Generic::List(list) => {
                let json_list = list
                    .as_any()
                    .downcast_ref::<JsonListType>()
                    .expect("Unknown list type in JsonTypeUtil::convert_to");
                let mut ret = to.create_empty_list();
                for elem in json_list.array.borrow().iter() {
                    let converted = Self::convert_json_node_to_generic(to, elem)
                        .expect("Cannot add null JSON element to ListType");
                    ret.add_generic(converted);
                }
                Some(Generic::List(ret))
            }
            Generic::Map(map) => {
                let json_map = map
                    .as_any()
                    .downcast_ref::<JsonMapType>()
                    .expect("Unknown map type in JsonTypeUtil::convert_to");
                let mut ret = to.create_empty_map();
                for (k, v) in json_map.map.borrow().iter() {
                    if let Some(converted) = Self::convert_json_node_to_generic(to, v) {
                        ret.set_generic(k, converted);
                    }
                }
                Some(Generic::Map(ret))
            }
            _ => panic!("Unknown type in JsonTypeUtil::convert_to"),
        }
    }

    fn convert_from_base_to_generic(
        &self,
        input: &JsonValue,
        to: &dyn TypeUtilBase,
    ) -> Option<Generic> {
        Self::convert_json_to_generic(to, input)
    }

    fn base_to_generic(&self, input: &JsonValue) -> Option<Generic> {
        match input {
            JsonValue::Null => None,
            JsonValue::Object(obj) => Some(Generic::Map(Box::new(JsonMapType::from_json(
                obj,
                self.compressed,
            )))),
            JsonValue::Array(arr) => Some(Generic::List(Box::new(JsonListType::from_json(
                arr,
                self.compressed,
            )))),
            JsonValue::Bool(b) => Some(Generic::Bool(*b)),
            JsonValue::Number(n) => Some(
                crate::server::dataconverter::types::json::number_to_generic(
                    convert_parsed_number_to_generic_number(n),
                ),
            ),
            JsonValue::String(s) => Some(Generic::Str(s.clone())),
        }
    }

    fn generic_to_base(&self, input: Option<&Generic>) -> JsonValue {
        let Some(input) = input else {
            return JsonValue::Null;
        };
        match input {
            Generic::Map(map) => {
                let json_map = map
                    .as_any()
                    .downcast_ref::<JsonMapType>()
                    .expect("Unrecognized map type in JsonTypeUtil::generic_to_base");
                JsonValue::Object(json_map.get_json())
            }
            Generic::List(list) => {
                let json_list = list
                    .as_any()
                    .downcast_ref::<JsonListType>()
                    .expect("Unrecognized list type in JsonTypeUtil::generic_to_base");
                JsonValue::Array(json_list.get_json())
            }
            Generic::Bool(b) => JsonValue::Bool(*b),
            Generic::Byte(v) => number_to_json_value(rivet_serialization::number::Number::Byte(*v)),
            Generic::Short(v) => {
                number_to_json_value(rivet_serialization::number::Number::Short(*v))
            }
            Generic::Int(v) => number_to_json_value(rivet_serialization::number::Number::Int(*v)),
            Generic::Long(v) => number_to_json_value(rivet_serialization::number::Number::Long(*v)),
            Generic::Float(v) => {
                number_to_json_value(rivet_serialization::number::Number::Float(*v))
            }
            Generic::Double(v) => {
                number_to_json_value(rivet_serialization::number::Number::Double(*v))
            }
            Generic::Str(s) => JsonValue::String(s.clone()),
            _ => panic!("Unrecognized type in JsonTypeUtil::generic_to_base"),
        }
    }
}
