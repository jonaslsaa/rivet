//! Concrete JSON backing types for Spottedleaf's DataConverter.
//!
//! Mirrors `ca.spottedleaf.dataconverter.types.json.*` from Paper 26.2.

pub mod json_list_type;
pub mod json_map_type;
pub mod json_type_util;

pub use json_list_type::JsonListType;
pub use json_map_type::JsonMapType;
pub use json_type_util::JsonTypeUtil;

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use rivet_serialization::number::Number;
use serde_json::{Map as JsonObject, Number as JsonNumber, Value as JsonValue};

pub(crate) type SharedJsonArray = Rc<RefCell<Vec<JsonNode>>>;
pub(crate) type SharedJsonObject = Rc<RefCell<IndexMap<String, JsonNode>>>;

/// Internal reference-counted JSON representation that preserves Java/Gson
/// nested-view aliasing and exact boxed `Number` types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JsonNode {
    Null,
    Bool(bool),
    /// Typed numeric primitive (e.g. written via `set_byte`, `set_int`, etc.).
    Number(Number),
    /// Untyped parsed JSON number (analogous to Gson's `LazilyParsedNumber`).
    ParsedNumber(JsonNumber),
    String(String),
    Array(SharedJsonArray),
    Object(SharedJsonObject),
}

impl JsonNode {
    pub(crate) fn deep_copy(&self) -> Self {
        match self {
            Self::Null => Self::Null,
            Self::Bool(b) => Self::Bool(*b),
            Self::Number(n) => Self::Number(*n),
            Self::ParsedNumber(n) => Self::ParsedNumber(n.clone()),
            Self::String(s) => Self::String(s.clone()),
            Self::Array(arr) => {
                let copy = arr.borrow().iter().map(Self::deep_copy).collect();
                Self::Array(Rc::new(RefCell::new(copy)))
            }
            Self::Object(obj) => {
                let mut copy = IndexMap::with_capacity(obj.borrow().len());
                for (k, v) in obj.borrow().iter() {
                    copy.insert(k.clone(), v.deep_copy());
                }
                Self::Object(Rc::new(RefCell::new(copy)))
            }
        }
    }

    pub(crate) fn from_value(value: &JsonValue) -> Self {
        match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(b) => Self::Bool(*b),
            JsonValue::Number(n) => Self::ParsedNumber(n.clone()),
            JsonValue::String(s) => Self::String(s.clone()),
            JsonValue::Array(arr) => Self::Array(from_json_array(arr)),
            JsonValue::Object(obj) => Self::Object(from_json_object(obj)),
        }
    }

    pub(crate) fn to_value(&self) -> JsonValue {
        match self {
            Self::Null => JsonValue::Null,
            Self::Bool(b) => JsonValue::Bool(*b),
            Self::Number(n) => number_to_json_value(*n),
            Self::ParsedNumber(n) => JsonValue::Number(n.clone()),
            Self::String(s) => JsonValue::String(s.clone()),
            Self::Array(arr) => JsonValue::Array(to_json_array(arr)),
            Self::Object(obj) => JsonValue::Object(to_json_object(obj)),
        }
    }

    pub(crate) fn number_as_string(&self) -> Option<String> {
        match self {
            Self::Number(n) => Some(number_to_string(*n)),
            Self::ParsedNumber(n) => Some(n.to_string()),
            _ => None,
        }
    }
}

pub(crate) fn from_json_array(arr: &[JsonValue]) -> SharedJsonArray {
    Rc::new(RefCell::new(arr.iter().map(JsonNode::from_value).collect()))
}

pub(crate) fn to_json_array(arr: &SharedJsonArray) -> Vec<JsonValue> {
    arr.borrow().iter().map(JsonNode::to_value).collect()
}

pub(crate) fn from_json_object(obj: &JsonObject<String, JsonValue>) -> SharedJsonObject {
    let mut map = IndexMap::with_capacity(obj.len());
    for (k, v) in obj {
        map.insert(k.clone(), JsonNode::from_value(v));
    }
    Rc::new(RefCell::new(map))
}

pub(crate) fn to_json_object(obj: &SharedJsonObject) -> JsonObject<String, JsonValue> {
    let mut map = JsonObject::with_capacity(obj.borrow().len());
    for (k, v) in obj.borrow().iter() {
        map.insert(k.clone(), v.to_value());
    }
    map
}

pub(crate) fn number_to_json_value(n: Number) -> JsonValue {
    match n {
        Number::Byte(v) => JsonValue::Number(JsonNumber::from(v)),
        Number::Short(v) => JsonValue::Number(JsonNumber::from(v)),
        Number::Int(v) => JsonValue::Number(JsonNumber::from(v)),
        Number::Long(v) => JsonValue::Number(JsonNumber::from(v)),
        Number::Float(v) => {
            JsonNumber::from_f64(f64::from(v)).map_or(JsonValue::Null, JsonValue::Number)
        }
        Number::Double(v) => JsonNumber::from_f64(v).map_or(JsonValue::Null, JsonValue::Number),
    }
}

pub(crate) fn number_to_string(n: Number) -> String {
    match n {
        Number::Byte(v) => v.to_string(),
        Number::Short(v) => v.to_string(),
        Number::Int(v) => v.to_string(),
        Number::Long(v) => v.to_string(),
        Number::Float(v) => v.to_string(),
        Number::Double(v) => v.to_string(),
    }
}

/// Converts a parsed `serde_json::Number` to `Number`, matching Paper's
/// `JsonTypeUtil.convertBDToGeneric` (narrows exact integers to smallest of
/// `Byte`/`Short`/`Int`/`Long`; floats to `Float` if lossless, else `Double`).
pub(crate) fn convert_parsed_number_to_generic_number(n: &JsonNumber) -> Number {
    if let Some(l) = n.as_i64() {
        let b = l as i8;
        if i64::from(b) == l {
            return Number::Byte(b);
        }
        let s = l as i16;
        if i64::from(s) == l {
            return Number::Short(s);
        }
        let i = l as i32;
        if i64::from(i) == l {
            return Number::Int(i);
        }
        return Number::Long(l);
    }
    let d = n.as_f64().unwrap_or(0.0);
    // Check if float has an exact integer value representable in i64 (e.g. 1.0)
    // Note: in Java BigDecimal("1.0").longValueExact() succeeds and returns 1L.
    if d.is_finite()
        && d.fract() == 0.0
        && d >= (i64::MIN as f64)
        && d < 9_223_372_036_854_775_808.0_f64
    {
        let l = d as i64;
        let b = l as i8;
        if i64::from(b) == l {
            return Number::Byte(b);
        }
        let s = l as i16;
        if i64::from(s) == l {
            return Number::Short(s);
        }
        let i = l as i32;
        if i64::from(i) == l {
            return Number::Int(i);
        }
        return Number::Long(l);
    }
    let f = d as f32;
    if f64::from(f) == d {
        Number::Float(f)
    } else {
        Number::Double(d)
    }
}

/// Converts a `Number` to its corresponding `Generic` variant.
pub(crate) fn number_to_generic(
    n: Number,
) -> crate::server::dataconverter::types::generic::Generic {
    use crate::server::dataconverter::types::generic::Generic;
    match n {
        Number::Byte(v) => Generic::Byte(v),
        Number::Short(v) => Generic::Short(v),
        Number::Int(v) => Generic::Int(v),
        Number::Long(v) => Generic::Long(v),
        Number::Float(v) => Generic::Float(v),
        Number::Double(v) => Generic::Double(v),
    }
}

/// Converts a parsed `serde_json::Number` to `Number` for `get_number()` reads.
pub(crate) fn parsed_number_to_number(n: &JsonNumber) -> Number {
    if let Some(l) = n.as_i64() {
        Number::Long(l)
    } else {
        Number::Double(n.as_f64().unwrap_or(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dataconverter::types::Types;
    use crate::server::dataconverter::types::generic::Generic;
    use crate::server::dataconverter::types::list_type::ListType;
    use crate::server::dataconverter::types::map_type::MapType;
    use crate::server::dataconverter::types::object_type::ObjectType;
    use crate::server::dataconverter::types::type_util::TypeUtil;

    #[test]
    fn json_view_aliasing_and_deep_copy_independence() {
        let mut root = JsonMapType::new(false);
        {
            let mut sub = root.get_or_create_map("sub");
            sub.set_int("v", 1);
            let mut arr = sub.get_or_create_list("arr", ObjectType::Map);
            arr.add_map(Box::new(JsonMapType::new(false)));
            arr.get_map(0).set_string("k", "orig".to_owned());
        }

        // Aliasing through get_map / get_generic
        root.get_map("sub").unwrap().set_int("v", 2);
        assert_eq!(root.get_map("sub").unwrap().get_int("v"), 2);
        if let Some(Generic::Map(mut m)) = root.get_generic("sub") {
            m.set_int("v", 3);
        }
        assert_eq!(root.get_map("sub").unwrap().get_int("v"), 3);

        // Deep copy does not alias original
        let copy = root.copy();
        root.get_map("sub").unwrap().set_int("v", 100);
        assert_eq!(copy.get_map("sub").unwrap().get_int("v"), 3);
        assert_eq!(
            copy.get_map("sub")
                .unwrap()
                .get_list_unchecked("arr")
                .unwrap()
                .get_map(0)
                .get_string("k")
                .as_deref(),
            Some("orig")
        );
    }

    #[test]
    fn json_compressed_vs_uncompressed_string_numbers() {
        let mut uncompressed = JsonMapType::new(false);
        uncompressed.set_string("num", "123".to_owned());
        assert_eq!(uncompressed.get_number("num"), None);
        assert_eq!(uncompressed.get_int_or("num", -1), -1);
        assert!(!uncompressed.has_key_of_type("num", ObjectType::Number));

        let mut compressed = JsonMapType::new(true);
        compressed.set_string("num", "123".to_owned());
        compressed.set_string("nonnum", "abc".to_owned());
        assert_eq!(compressed.get_number("num"), Some(Number::Int(123)));
        assert_eq!(compressed.get_int("num"), 123);
        assert_eq!(compressed.get_number("nonnum"), None);

        // Deep copy preserves compressed flag.
        let copy = compressed.copy();
        assert_eq!(copy.get_int("num"), 123);

        // Compressed JsonListType also parses integer strings.
        let mut clist = JsonListType::new(true);
        clist.add_string("456".to_owned());
        assert_eq!(clist.get_int(0), 456);
        assert_eq!(clist.get_number(0), Number::Int(456));

        let mut ulist = JsonListType::new(false);
        ulist.add_string("456".to_owned());
        assert_eq!(ulist.get_int_or(0, -5), -5);
    }

    #[test]
    fn json_booleans_numbers_and_null_handling() {
        let mut map = JsonMapType::new(false);
        map.set_boolean("b_true", true);
        map.set_boolean("b_false", false);
        map.set_int("zero", 0);
        map.set_int("nonzero", 5);

        assert!(map.get_boolean("b_true"));
        assert!(!map.get_boolean("b_false"));
        assert!(!map.get_boolean("zero"));
        assert!(map.get_boolean("nonzero"));
        assert_eq!(map.get_number("b_true"), Some(Number::Byte(1)));
        assert_eq!(map.get_number("b_false"), Some(Number::Byte(0)));
        assert!(map.has_key_of_type("b_true", ObjectType::Byte));
        assert!(map.has_key_of_type("b_true", ObjectType::Number));
        assert!(matches!(
            map.get_generic("b_true"),
            Some(Generic::Bool(true))
        ));

        // JSON null handling via JsonTypeUtil
        assert!(Types::JSON.base_to_generic(&JsonValue::Null).is_none());
        assert_eq!(Types::JSON.generic_to_base(None), JsonValue::Null);

        let mut obj = JsonObject::new();
        obj.insert("null_key".to_owned(), JsonValue::Null);
        obj.insert("int_key".to_owned(), JsonValue::from(7));
        let wrapped = JsonMapType::from_json(&obj, false);
        assert!(!wrapped.has_key("missing"));
        assert!(wrapped.get_generic("null_key").is_none());
        assert_eq!(wrapped.get_int("int_key"), 7);
    }

    #[test]
    fn json_parsed_number_narrowing() {
        let b = serde_json::from_str::<JsonValue>("12").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&b),
            Some(Generic::Byte(12))
        ));
        let s = serde_json::from_str::<JsonValue>("1000").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&s),
            Some(Generic::Short(1000))
        ));
        let i = serde_json::from_str::<JsonValue>("100000").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&i),
            Some(Generic::Int(100_000))
        ));
        let l = serde_json::from_str::<JsonValue>("5000000000").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&l),
            Some(Generic::Long(5_000_000_000))
        ));
        let f = serde_json::from_str::<JsonValue>("1.5").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&f),
            Some(Generic::Float(v)) if (v - 1.5).abs() < f32::EPSILON
        ));
        let d = serde_json::from_str::<JsonValue>("1.123456789012345").unwrap();
        assert!(matches!(
            Types::JSON.base_to_generic(&d),
            Some(Generic::Double(_))
        ));
    }

    #[test]
    #[should_panic]
    fn json_list_strict_wrong_type_panics() {
        let mut list = JsonListType::new(false);
        list.add_string("abc".to_owned());
        let _ = list.get_number(0);
    }

    #[test]
    #[should_panic]
    fn json_list_primitive_array_unsupported() {
        let mut list = JsonListType::new(false);
        list.add_int_array(vec![1, 2]);
    }

    #[test]
    #[should_panic]
    fn json_list_indexed_insert_unsupported() {
        let mut list = JsonListType::new(false);
        list.add_int_at(0, 1);
    }

    #[test]
    #[should_panic]
    fn json_map_primitive_array_set_unsupported() {
        let mut map = JsonMapType::new(false);
        map.set_ints("a", vec![1, 2]);
    }
}
