//! Test-only reference backings for the container traits, used to exercise the
//! default dispatch methods (`set_generic`, `add_generic`, `get_or_create_*`,
//! `get_list`) and the view-aliasing contract without the out-of-scope NBT/JSON
//! concrete types. The semantics mirror what the `dataconverter-foundation` probe
//! captured against the real NBT backing.
//!
//! Both backings store their elements in `Rc<RefCell<...>>` so a `get_map`/
//! `get_list`/`get_generic`/`get_or_create_*` view shares the same storage as
//! the parent — exactly how the NBT backings wrap the same `CompoundTag`/
//! `ListTag`. Numeric getters coerce all six boxed numbers with `Number`'s
//! `*_value` narrowing (Java `NumericTag.*Value()`), booleans are stored as
//! bytes (`NBTMapType.setBoolean` -> `setByte`), and the `_or` overloads return
//! the supplied default when the value is present but the wrong type
//! (`NBTListType.getBytes(index, dfl)` returns `dfl` for a non-`ByteArrayTag`).
//! List access is strict, matching `ListType.java`'s contract and the NBT
//! throws: the no-default list `get*`/`get_generic`/`get_list`/`get_map`
//! accessors panic on an out-of-range index, a wrong-typed element, or a nested
//! container that is not this mock's backing.
//!
//! [`foundation_fixture`] embeds the same committed oracle golden that
//! `rivet-oracle verify` hash-checks, so the container tests are differentially
//! checked against Paper — a re-pin that changes a default-method semantic fails
//! the tests, not just the fixture hash.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use rivet_serialization::number::Number;
use serde_json::Value;

/// The committed `dataconverter-foundation` oracle golden
/// (`tools/rivet-oracle/fixtures/dataconverter/dataconverter-foundation.json`),
/// embedded so the container default-method tests are differentially checked
/// against the same fixture `rivet-oracle verify` hash-validates.
pub(crate) fn foundation_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../tools/rivet-oracle/fixtures/dataconverter/dataconverter-foundation.json"
    ))
    .expect("dataconverter-foundation.json parses")
}

/// Renders a boxed [`Generic`] the way the probe recorded the value fields of
/// `mapTypeDefaults`/`listTypeDefaults` — the NBT `Tag.toString` /
/// `Arrays.toString` forms (numbers and strings as their plain value, arrays as
/// `[a, b]`). Containers are asserted structurally by the callers, never here.
pub(crate) fn render_generic(value: &Generic) -> String {
    match value {
        Generic::Byte(v) => format!("{v}"),
        Generic::Short(v) => format!("{v}"),
        Generic::Int(v) => format!("{v}"),
        Generic::Long(v) => format!("{v}"),
        Generic::Float(v) => format!("{v}"),
        Generic::Double(v) => format!("{v}"),
        Generic::Str(s) => s.clone(),
        Generic::Bytes(v) => format!("{v:?}"),
        Generic::Shorts(v) => format!("{v:?}"),
        Generic::Ints(v) => format!("{v:?}"),
        Generic::Longs(v) => format!("{v:?}"),
        Generic::Bool(b) => format!("{b}"),
        Generic::Map(_) | Generic::List(_) => {
            unreachable!("containers are asserted structurally, not rendered")
        }
    }
}

/// Renders a [`Number`] as `String.valueOf` would for the probe's `getNumber`
/// sample (e.g. `getNumberFromByte`).
pub(crate) fn render_number(value: &Number) -> String {
    match value {
        Number::Byte(v) => format!("{v}"),
        Number::Short(v) => format!("{v}"),
        Number::Int(v) => format!("{v}"),
        Number::Long(v) => format!("{v}"),
        Number::Float(v) => format!("{v}"),
        Number::Double(v) => format!("{v}"),
    }
}

/// The factory for both reference backings (`TypeUtil.createEmptyList/Map`).
pub(crate) struct MockTypeUtil;

impl TypeUtilBase for MockTypeUtil {
    fn create_empty_list(&self) -> Box<dyn ListType> {
        Box::new(MockList::new())
    }

    fn create_empty_map(&self) -> Box<dyn MapType> {
        Box::new(MockMap::new())
    }
}

/// Deep-copies a [`Generic`] (the `MapType.copy`/`ListType.copy` behavior).
fn deep_copy(value: &Generic) -> Generic {
    match value {
        Generic::Bool(v) => Generic::Bool(*v),
        Generic::Byte(v) => Generic::Byte(*v),
        Generic::Short(v) => Generic::Short(*v),
        Generic::Int(v) => Generic::Int(*v),
        Generic::Long(v) => Generic::Long(*v),
        Generic::Float(v) => Generic::Float(*v),
        Generic::Double(v) => Generic::Double(*v),
        Generic::Str(v) => Generic::Str(v.clone()),
        Generic::Bytes(v) => Generic::Bytes(v.clone()),
        Generic::Shorts(v) => Generic::Shorts(v.clone()),
        Generic::Ints(v) => Generic::Ints(v.clone()),
        Generic::Longs(v) => Generic::Longs(v.clone()),
        Generic::Map(m) => Generic::Map(m.copy()),
        Generic::List(l) => Generic::List(l.copy()),
    }
}

/// `NumericTag.*Value()` narrowing for each primitive — `None` when the value
/// is not one of the six boxed numbers (Java `tag instanceof NumericTag`).
/// `Number` reproduces the JLS 5.1.3 casts exactly, including the float/double
/// `(byte)(int)` wrap on `byte_value`/`short_value`.
fn coerce_i8(value: &Generic) -> Option<i8> {
    value.as_number().map(|n| n.byte_value())
}

fn coerce_i16(value: &Generic) -> Option<i16> {
    value.as_number().map(|n| n.short_value())
}

fn coerce_i32(value: &Generic) -> Option<i32> {
    value.as_number().map(|n| n.int_value())
}

fn coerce_i64(value: &Generic) -> Option<i64> {
    value.as_number().map(|n| n.long_value())
}

fn coerce_f32(value: &Generic) -> Option<f32> {
    value.as_number().map(|n| n.float_value())
}

fn coerce_f64(value: &Generic) -> Option<f64> {
    value.as_number().map(|n| n.double_value())
}

/// The reference `MapType` backing.
pub(crate) struct MockMap {
    entries: Rc<RefCell<HashMap<String, Generic>>>,
}

impl Default for MockMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMap {
    pub(crate) fn new() -> Self {
        MockMap {
            entries: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    fn clone_view(&self) -> MockMap {
        MockMap {
            entries: Rc::clone(&self.entries),
        }
    }
}

impl MapType for MockMap {
    fn get_type_util(&self) -> &dyn TypeUtilBase {
        &MockTypeUtil
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn size(&self) -> usize {
        self.entries.borrow().len()
    }

    fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    fn clear(&mut self) {
        self.entries.borrow_mut().clear();
    }

    fn keys(&self) -> Vec<String> {
        self.entries.borrow().keys().cloned().collect()
    }

    fn copy(&self) -> Box<dyn MapType> {
        let entries = self
            .entries
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), deep_copy(v)))
            .collect();
        Box::new(MockMap {
            entries: Rc::new(RefCell::new(entries)),
        })
    }

    fn has_key(&self, key: &str) -> bool {
        self.entries.borrow().contains_key(key)
    }

    fn has_key_of_type(&self, key: &str, ty: ObjectType) -> bool {
        let borrowed = self.entries.borrow();
        let Some(value) = borrowed.get(key) else {
            return false;
        };
        let Some(value_type) = ObjectType::get_type(value) else {
            return false;
        };
        value_type == ty || (ty == ObjectType::Number && value_type.is_number())
    }

    fn remove(&mut self, key: &str) {
        self.entries.borrow_mut().remove(key);
    }

    fn get_generic(&self, key: &str) -> Option<Generic> {
        match self.entries.borrow().get(key)? {
            Generic::Map(map) => {
                let mock = map
                    .as_any()
                    .downcast_ref::<MockMap>()
                    .expect("MockMap.getGeneric: value is not a MockMap");
                Some(Generic::Map(Box::new(mock.clone_view())))
            }
            Generic::List(list) => {
                let mock = list
                    .as_any()
                    .downcast_ref::<MockList>()
                    .expect("MockMap.getGeneric: value is not a MockList");
                Some(Generic::List(Box::new(mock.clone_view())))
            }
            other => Some(deep_copy(other)),
        }
    }

    fn get_number(&self, key: &str) -> Option<Number> {
        self.entries.borrow().get(key).and_then(Generic::as_number)
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
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i8)
            .unwrap_or(0)
    }

    fn get_byte_or(&self, key: &str, dfl: i8) -> i8 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i8)
            .unwrap_or(dfl)
    }

    fn set_byte(&mut self, key: &str, val: i8) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Byte(val));
    }

    fn get_short(&self, key: &str) -> i16 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i16)
            .unwrap_or(0)
    }

    fn get_short_or(&self, key: &str, dfl: i16) -> i16 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i16)
            .unwrap_or(dfl)
    }

    fn set_short(&mut self, key: &str, val: i16) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Short(val));
    }

    fn get_int(&self, key: &str) -> i32 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i32)
            .unwrap_or(0)
    }

    fn get_int_or(&self, key: &str, dfl: i32) -> i32 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i32)
            .unwrap_or(dfl)
    }

    fn set_int(&mut self, key: &str, val: i32) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Int(val));
    }

    fn get_long(&self, key: &str) -> i64 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i64)
            .unwrap_or(0)
    }

    fn get_long_or(&self, key: &str, dfl: i64) -> i64 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_i64)
            .unwrap_or(dfl)
    }

    fn set_long(&mut self, key: &str, val: i64) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Long(val));
    }

    fn get_float(&self, key: &str) -> f32 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_f32)
            .unwrap_or(0.0)
    }

    fn get_float_or(&self, key: &str, dfl: f32) -> f32 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_f32)
            .unwrap_or(dfl)
    }

    fn set_float(&mut self, key: &str, val: f32) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Float(val));
    }

    fn get_double(&self, key: &str) -> f64 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_f64)
            .unwrap_or(0.0)
    }

    fn get_double_or(&self, key: &str, dfl: f64) -> f64 {
        self.entries
            .borrow()
            .get(key)
            .and_then(coerce_f64)
            .unwrap_or(dfl)
    }

    fn set_double(&mut self, key: &str, val: f64) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Double(val));
    }

    fn get_bytes(&self, key: &str) -> Option<Vec<i8>> {
        match self.entries.borrow().get(key) {
            Some(Generic::Bytes(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_bytes_or(&self, key: &str, dfl: Vec<i8>) -> Vec<i8> {
        self.get_bytes(key).unwrap_or(dfl)
    }

    fn set_bytes(&mut self, key: &str, val: Vec<i8>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Bytes(val));
    }

    fn get_shorts(&self, key: &str) -> Option<Vec<i16>> {
        match self.entries.borrow().get(key) {
            Some(Generic::Shorts(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_shorts_or(&self, key: &str, dfl: Vec<i16>) -> Vec<i16> {
        self.get_shorts(key).unwrap_or(dfl)
    }

    fn set_shorts(&mut self, key: &str, val: Vec<i16>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Shorts(val));
    }

    fn get_ints(&self, key: &str) -> Option<Vec<i32>> {
        match self.entries.borrow().get(key) {
            Some(Generic::Ints(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_ints_or(&self, key: &str, dfl: Vec<i32>) -> Vec<i32> {
        self.get_ints(key).unwrap_or(dfl)
    }

    fn set_ints(&mut self, key: &str, val: Vec<i32>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Ints(val));
    }

    fn get_longs(&self, key: &str) -> Option<Vec<i64>> {
        match self.entries.borrow().get(key) {
            Some(Generic::Longs(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_longs_or(&self, key: &str, dfl: Vec<i64>) -> Vec<i64> {
        self.get_longs(key).unwrap_or(dfl)
    }

    fn set_longs(&mut self, key: &str, val: Vec<i64>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Longs(val));
    }

    fn get_list_unchecked(&self, key: &str) -> Option<Box<dyn ListType>> {
        let borrowed = self.entries.borrow();
        let Generic::List(list) = borrowed.get(key)? else {
            return None;
        };
        let mock = list
            .as_any()
            .downcast_ref::<MockList>()
            .expect("MockMap.getList: value is not a MockList");
        Some(Box::new(mock.clone_view()))
    }

    fn get_list_unchecked_or(&self, key: &str, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        self.get_list_unchecked(key).unwrap_or(dfl)
    }

    fn set_list(&mut self, key: &str, val: Box<dyn ListType>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::List(val));
    }

    fn get_map(&self, key: &str) -> Option<Box<dyn MapType>> {
        let borrowed = self.entries.borrow();
        let Generic::Map(map) = borrowed.get(key)? else {
            return None;
        };
        let mock = map
            .as_any()
            .downcast_ref::<MockMap>()
            .expect("MockMap.getMap: value is not a MockMap");
        Some(Box::new(mock.clone_view()))
    }

    fn get_map_or(&self, key: &str, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        self.get_map(key).unwrap_or(dfl)
    }

    fn set_map(&mut self, key: &str, val: Box<dyn MapType>) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Map(val));
    }

    fn get_string(&self, key: &str) -> Option<String> {
        match self.entries.borrow().get(key) {
            Some(Generic::Str(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_string_or(&self, key: &str, dfl: String) -> String {
        self.get_string(key).unwrap_or(dfl)
    }

    fn set_string(&mut self, key: &str, val: String) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Str(val));
    }
}

/// The reference `ListType` backing.
pub(crate) struct MockList {
    elems: Rc<RefCell<Vec<Generic>>>,
}

impl Default for MockList {
    fn default() -> Self {
        Self::new()
    }
}

impl MockList {
    pub(crate) fn new() -> Self {
        MockList {
            elems: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn clone_view(&self) -> MockList {
        MockList {
            elems: Rc::clone(&self.elems),
        }
    }
}

impl ListType for MockList {
    fn get_type_util(&self) -> &dyn TypeUtilBase {
        &MockTypeUtil
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn copy(&self) -> Box<dyn ListType> {
        let elems = self.elems.borrow().iter().map(deep_copy).collect();
        Box::new(MockList {
            elems: Rc::new(RefCell::new(elems)),
        })
    }

    fn get_uniform_type(&self) -> ObjectType {
        let borrowed = self.elems.borrow();
        let mut iter = borrowed.iter().map(ObjectType::get_type);
        let first = match iter.next() {
            None => return ObjectType::None,
            Some(first) => first,
        };
        let first = match first {
            Some(first) => first,
            None => return ObjectType::Undefined,
        };
        for element in iter {
            match element {
                Some(element) if element == first => {}
                _ => return ObjectType::Mixed,
            }
        }
        first
    }

    fn size(&self) -> usize {
        self.elems.borrow().len()
    }

    fn remove(&mut self, index: usize) {
        self.elems.borrow_mut().remove(index);
    }

    fn get_generic(&self, index: usize) -> Option<Generic> {
        match self.elems.borrow().get(index)? {
            Generic::Map(map) => {
                let mock = map
                    .as_any()
                    .downcast_ref::<MockMap>()
                    .expect("MockList.getGeneric: element is not a MockMap");
                Some(Generic::Map(Box::new(mock.clone_view())))
            }
            Generic::List(list) => {
                let mock = list
                    .as_any()
                    .downcast_ref::<MockList>()
                    .expect("MockList.getGeneric: element is not a MockList");
                Some(Generic::List(Box::new(mock.clone_view())))
            }
            other => Some(deep_copy(other)),
        }
    }

    fn get_number(&self, index: usize) -> Option<Number> {
        self.elems.borrow().get(index).and_then(Generic::as_number)
    }

    fn get_number_or(&self, index: usize, dfl: Number) -> Number {
        self.get_number(index).unwrap_or(dfl)
    }

    fn get_byte(&self, index: usize) -> i8 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getByte: index out of bounds");
        coerce_i8(element).expect("NBTListType.getByte: element is not a NumericTag")
    }

    fn get_byte_or(&self, index: usize, dfl: i8) -> i8 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_i8)
            .unwrap_or(dfl)
    }

    fn set_byte(&mut self, index: usize, to: i8) {
        self.elems.borrow_mut()[index] = Generic::Byte(to);
    }

    fn get_short(&self, index: usize) -> i16 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getShort: index out of bounds");
        coerce_i16(element).expect("NBTListType.getShort: element is not a NumericTag")
    }

    fn get_short_or(&self, index: usize, dfl: i16) -> i16 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_i16)
            .unwrap_or(dfl)
    }

    fn set_short(&mut self, index: usize, to: i16) {
        self.elems.borrow_mut()[index] = Generic::Short(to);
    }

    fn get_int(&self, index: usize) -> i32 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getInt: index out of bounds");
        coerce_i32(element).expect("NBTListType.getInt: element is not a NumericTag")
    }

    fn get_int_or(&self, index: usize, dfl: i32) -> i32 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_i32)
            .unwrap_or(dfl)
    }

    fn set_int(&mut self, index: usize, to: i32) {
        self.elems.borrow_mut()[index] = Generic::Int(to);
    }

    fn get_long(&self, index: usize) -> i64 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getLong: index out of bounds");
        coerce_i64(element).expect("NBTListType.getLong: element is not a NumericTag")
    }

    fn get_long_or(&self, index: usize, dfl: i64) -> i64 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_i64)
            .unwrap_or(dfl)
    }

    fn set_long(&mut self, index: usize, to: i64) {
        self.elems.borrow_mut()[index] = Generic::Long(to);
    }

    fn get_float(&self, index: usize) -> f32 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getFloat: index out of bounds");
        coerce_f32(element).expect("NBTListType.getFloat: element is not a NumericTag")
    }

    fn get_float_or(&self, index: usize, dfl: f32) -> f32 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_f32)
            .unwrap_or(dfl)
    }

    fn set_float(&mut self, index: usize, to: f32) {
        self.elems.borrow_mut()[index] = Generic::Float(to);
    }

    fn get_double(&self, index: usize) -> f64 {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getDouble: index out of bounds");
        coerce_f64(element).expect("NBTListType.getDouble: element is not a NumericTag")
    }

    fn get_double_or(&self, index: usize, dfl: f64) -> f64 {
        self.elems
            .borrow()
            .get(index)
            .and_then(coerce_f64)
            .unwrap_or(dfl)
    }

    fn set_double(&mut self, index: usize, to: f64) {
        self.elems.borrow_mut()[index] = Generic::Double(to);
    }

    fn get_bytes(&self, index: usize) -> Vec<i8> {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getBytes: index out of bounds");
        match element {
            Generic::Bytes(v) => v.clone(),
            _ => panic!("NBTListType.getBytes: element is not a ByteArrayTag"),
        }
    }

    fn get_bytes_or(&self, index: usize, dfl: Vec<i8>) -> Vec<i8> {
        // Java `NBTListType.getBytes(index, dfl)` returns `dfl` for a
        // present-but-not-`ByteArrayTag` element (and for an out-of-range
        // index) — not an empty array.
        match self.elems.borrow().get(index) {
            Some(Generic::Bytes(v)) => v.clone(),
            _ => dfl,
        }
    }

    fn set_bytes(&mut self, index: usize, to: Vec<i8>) {
        self.elems.borrow_mut()[index] = Generic::Bytes(to);
    }

    fn get_shorts(&self, index: usize) -> Vec<i16> {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getShorts: index out of bounds");
        match element {
            Generic::Shorts(v) => v.clone(),
            _ => panic!("NBTListType.getShorts: element is not a ShortArrayTag"),
        }
    }

    fn get_shorts_or(&self, index: usize, dfl: Vec<i16>) -> Vec<i16> {
        match self.elems.borrow().get(index) {
            Some(Generic::Shorts(v)) => v.clone(),
            _ => dfl,
        }
    }

    fn set_shorts(&mut self, index: usize, to: Vec<i16>) {
        self.elems.borrow_mut()[index] = Generic::Shorts(to);
    }

    fn get_ints(&self, index: usize) -> Vec<i32> {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getInts: index out of bounds");
        match element {
            Generic::Ints(v) => v.clone(),
            _ => panic!("NBTListType.getInts: element is not an IntArrayTag"),
        }
    }

    fn get_ints_or(&self, index: usize, dfl: Vec<i32>) -> Vec<i32> {
        match self.elems.borrow().get(index) {
            Some(Generic::Ints(v)) => v.clone(),
            _ => dfl,
        }
    }

    fn set_ints(&mut self, index: usize, to: Vec<i32>) {
        self.elems.borrow_mut()[index] = Generic::Ints(to);
    }

    fn get_longs(&self, index: usize) -> Vec<i64> {
        let elems = self.elems.borrow();
        let element = elems
            .get(index)
            .expect("NBTListType.getLongs: index out of bounds");
        match element {
            Generic::Longs(v) => v.clone(),
            _ => panic!("NBTListType.getLongs: element is not a LongArrayTag"),
        }
    }

    fn get_longs_or(&self, index: usize, dfl: Vec<i64>) -> Vec<i64> {
        match self.elems.borrow().get(index) {
            Some(Generic::Longs(v)) => v.clone(),
            _ => dfl,
        }
    }

    fn set_longs(&mut self, index: usize, to: Vec<i64>) {
        self.elems.borrow_mut()[index] = Generic::Longs(to);
    }

    fn get_list(&self, index: usize) -> Option<Box<dyn ListType>> {
        let borrowed = self.elems.borrow();
        let Generic::List(list) = borrowed.get(index)? else {
            return None;
        };
        let mock = list
            .as_any()
            .downcast_ref::<MockList>()
            .expect("MockList.getList: element is not a MockList");
        Some(Box::new(mock.clone_view()))
    }

    fn get_list_or(&self, index: usize, dfl: Box<dyn ListType>) -> Box<dyn ListType> {
        self.get_list(index).unwrap_or(dfl)
    }

    fn set_list(&mut self, index: usize, list: Box<dyn ListType>) {
        self.elems.borrow_mut()[index] = Generic::List(list);
    }

    fn get_map(&self, index: usize) -> Option<Box<dyn MapType>> {
        let borrowed = self.elems.borrow();
        let Generic::Map(map) = borrowed.get(index)? else {
            return None;
        };
        let mock = map
            .as_any()
            .downcast_ref::<MockMap>()
            .expect("MockList.getMap: element is not a MockMap");
        Some(Box::new(mock.clone_view()))
    }

    fn get_map_or(&self, index: usize, dfl: Box<dyn MapType>) -> Box<dyn MapType> {
        self.get_map(index).unwrap_or(dfl)
    }

    fn set_map(&mut self, index: usize, to: Box<dyn MapType>) {
        self.elems.borrow_mut()[index] = Generic::Map(to);
    }

    fn get_string(&self, index: usize) -> Option<String> {
        match self.elems.borrow().get(index) {
            Some(Generic::Str(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn get_string_or(&self, index: usize, dfl: String) -> String {
        self.get_string(index).unwrap_or(dfl)
    }

    fn set_string(&mut self, index: usize, to: String) {
        self.elems.borrow_mut()[index] = Generic::Str(to);
    }

    fn add_byte(&mut self, b: i8) {
        self.elems.borrow_mut().push(Generic::Byte(b));
    }

    fn add_byte_at(&mut self, index: usize, b: i8) {
        self.elems.borrow_mut().insert(index, Generic::Byte(b));
    }

    fn add_short(&mut self, s: i16) {
        self.elems.borrow_mut().push(Generic::Short(s));
    }

    fn add_short_at(&mut self, index: usize, s: i16) {
        self.elems.borrow_mut().insert(index, Generic::Short(s));
    }

    fn add_int(&mut self, i: i32) {
        self.elems.borrow_mut().push(Generic::Int(i));
    }

    fn add_int_at(&mut self, index: usize, i: i32) {
        self.elems.borrow_mut().insert(index, Generic::Int(i));
    }

    fn add_long(&mut self, l: i64) {
        self.elems.borrow_mut().push(Generic::Long(l));
    }

    fn add_long_at(&mut self, index: usize, l: i64) {
        self.elems.borrow_mut().insert(index, Generic::Long(l));
    }

    fn add_float(&mut self, f: f32) {
        self.elems.borrow_mut().push(Generic::Float(f));
    }

    fn add_float_at(&mut self, index: usize, f: f32) {
        self.elems.borrow_mut().insert(index, Generic::Float(f));
    }

    fn add_double(&mut self, d: f64) {
        self.elems.borrow_mut().push(Generic::Double(d));
    }

    fn add_double_at(&mut self, index: usize, d: f64) {
        self.elems.borrow_mut().insert(index, Generic::Double(d));
    }

    fn add_byte_array(&mut self, arr: Vec<i8>) {
        self.elems.borrow_mut().push(Generic::Bytes(arr));
    }

    fn add_byte_array_at(&mut self, index: usize, arr: Vec<i8>) {
        self.elems.borrow_mut().insert(index, Generic::Bytes(arr));
    }

    fn add_short_array(&mut self, arr: Vec<i16>) {
        self.elems.borrow_mut().push(Generic::Shorts(arr));
    }

    fn add_short_array_at(&mut self, index: usize, arr: Vec<i16>) {
        self.elems.borrow_mut().insert(index, Generic::Shorts(arr));
    }

    fn add_int_array(&mut self, arr: Vec<i32>) {
        self.elems.borrow_mut().push(Generic::Ints(arr));
    }

    fn add_int_array_at(&mut self, index: usize, arr: Vec<i32>) {
        self.elems.borrow_mut().insert(index, Generic::Ints(arr));
    }

    fn add_long_array(&mut self, arr: Vec<i64>) {
        self.elems.borrow_mut().push(Generic::Longs(arr));
    }

    fn add_long_array_at(&mut self, index: usize, arr: Vec<i64>) {
        self.elems.borrow_mut().insert(index, Generic::Longs(arr));
    }

    fn add_list(&mut self, list: Box<dyn ListType>) {
        self.elems.borrow_mut().push(Generic::List(list));
    }

    fn add_list_at(&mut self, index: usize, list: Box<dyn ListType>) {
        self.elems.borrow_mut().insert(index, Generic::List(list));
    }

    fn add_map(&mut self, map: Box<dyn MapType>) {
        self.elems.borrow_mut().push(Generic::Map(map));
    }

    fn add_map_at(&mut self, index: usize, map: Box<dyn MapType>) {
        self.elems.borrow_mut().insert(index, Generic::Map(map));
    }

    fn add_string(&mut self, string: String) {
        self.elems.borrow_mut().push(Generic::Str(string));
    }

    fn add_string_at(&mut self, index: usize, string: String) {
        self.elems.borrow_mut().insert(index, Generic::Str(string));
    }
}
