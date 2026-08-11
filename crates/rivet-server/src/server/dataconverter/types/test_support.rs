//! Test-only reference backings for the container traits, used to exercise the
//! default dispatch methods (`set_generic`, `add_generic`, `get_or_create_*`,
//! `get_list`) and the view-aliasing contract without the out-of-scope NBT/JSON
//! concrete types. The semantics mirror what the `dataconverter-foundation` probe
//! captured against the real NBT backing.
//!
//! Both backings store their elements in `Rc<RefCell<...>>` so a `get_map`/
//! `get_list`/`get_or_create_*` view shares the same storage as the parent —
//! exactly how the NBT backings wrap the same `CompoundTag`/`ListTag`.

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

fn numeric_cast_i8(value: &Generic) -> Option<i8> {
    match value {
        Generic::Byte(v) => Some(*v),
        Generic::Short(v) => Some(*v as i8),
        Generic::Int(v) => Some(*v as i8),
        Generic::Long(v) => Some(*v as i8),
        Generic::Float(v) => Some(*v as i8),
        Generic::Double(v) => Some(*v as i8),
        _ => None,
    }
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
        self.entries.borrow().get(key).map(deep_copy)
    }

    fn get_number(&self, key: &str) -> Option<Number> {
        self.entries.borrow().get(key).and_then(Generic::as_number)
    }

    fn get_number_or(&self, key: &str, dfl: Number) -> Number {
        self.get_number(key).unwrap_or(dfl)
    }

    fn get_boolean(&self, key: &str) -> bool {
        match self.entries.borrow().get(key) {
            Some(Generic::Bool(b)) => *b,
            Some(Generic::Byte(b)) => *b != 0,
            _ => false,
        }
    }

    fn get_boolean_or(&self, key: &str, dfl: bool) -> bool {
        if self.has_key(key) {
            self.get_boolean(key)
        } else {
            dfl
        }
    }

    fn set_boolean(&mut self, key: &str, val: bool) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Bool(val));
    }

    fn get_byte(&self, key: &str) -> i8 {
        self.entries
            .borrow()
            .get(key)
            .and_then(numeric_cast_i8)
            .unwrap_or(0)
    }

    fn get_byte_or(&self, key: &str, dfl: i8) -> i8 {
        if self.has_key(key) {
            self.get_byte(key)
        } else {
            dfl
        }
    }

    fn set_byte(&mut self, key: &str, val: i8) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Byte(val));
    }

    fn get_short(&self, key: &str) -> i16 {
        match self.entries.borrow().get(key) {
            Some(Generic::Short(v)) => *v,
            Some(Generic::Byte(v)) => *v as i16,
            Some(Generic::Int(v)) => *v as i16,
            _ => 0,
        }
    }

    fn get_short_or(&self, key: &str, dfl: i16) -> i16 {
        if self.has_key(key) {
            self.get_short(key)
        } else {
            dfl
        }
    }

    fn set_short(&mut self, key: &str, val: i16) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Short(val));
    }

    fn get_int(&self, key: &str) -> i32 {
        match self.entries.borrow().get(key) {
            Some(Generic::Int(v)) => *v,
            Some(Generic::Byte(v)) => *v as i32,
            Some(Generic::Short(v)) => *v as i32,
            Some(Generic::Long(v)) => *v as i32,
            _ => 0,
        }
    }

    fn get_int_or(&self, key: &str, dfl: i32) -> i32 {
        if self.has_key(key) {
            self.get_int(key)
        } else {
            dfl
        }
    }

    fn set_int(&mut self, key: &str, val: i32) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Int(val));
    }

    fn get_long(&self, key: &str) -> i64 {
        match self.entries.borrow().get(key) {
            Some(Generic::Long(v)) => *v,
            Some(Generic::Byte(v)) => *v as i64,
            Some(Generic::Short(v)) => *v as i64,
            Some(Generic::Int(v)) => *v as i64,
            _ => 0,
        }
    }

    fn get_long_or(&self, key: &str, dfl: i64) -> i64 {
        if self.has_key(key) {
            self.get_long(key)
        } else {
            dfl
        }
    }

    fn set_long(&mut self, key: &str, val: i64) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Long(val));
    }

    fn get_float(&self, key: &str) -> f32 {
        match self.entries.borrow().get(key) {
            Some(Generic::Float(v)) => *v,
            Some(Generic::Int(v)) => *v as f32,
            Some(Generic::Double(v)) => *v as f32,
            _ => 0.0,
        }
    }

    fn get_float_or(&self, key: &str, dfl: f32) -> f32 {
        if self.has_key(key) {
            self.get_float(key)
        } else {
            dfl
        }
    }

    fn set_float(&mut self, key: &str, val: f32) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), Generic::Float(val));
    }

    fn get_double(&self, key: &str) -> f64 {
        match self.entries.borrow().get(key) {
            Some(Generic::Double(v)) => *v,
            Some(Generic::Float(v)) => *v as f64,
            Some(Generic::Int(v)) => *v as f64,
            _ => 0.0,
        }
    }

    fn get_double_or(&self, key: &str, dfl: f64) -> f64 {
        if self.has_key(key) {
            self.get_double(key)
        } else {
            dfl
        }
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
        let mock = list.as_any().downcast_ref::<MockList>()?;
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
        let mock = map.as_any().downcast_ref::<MockMap>()?;
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
        self.elems.borrow().get(index).map(deep_copy)
    }

    fn get_number(&self, index: usize) -> Option<Number> {
        self.elems.borrow().get(index).and_then(Generic::as_number)
    }

    fn get_number_or(&self, index: usize, dfl: Number) -> Number {
        self.get_number(index).unwrap_or(dfl)
    }

    fn get_byte(&self, index: usize) -> i8 {
        self.elems
            .borrow()
            .get(index)
            .and_then(numeric_cast_i8)
            .unwrap_or(0)
    }

    fn get_byte_or(&self, index: usize, dfl: i8) -> i8 {
        if self.size() > index {
            self.get_byte(index)
        } else {
            dfl
        }
    }

    fn set_byte(&mut self, index: usize, to: i8) {
        self.elems.borrow_mut()[index] = Generic::Byte(to);
    }

    fn get_short(&self, index: usize) -> i16 {
        match self.elems.borrow().get(index) {
            Some(Generic::Short(v)) => *v,
            Some(Generic::Byte(v)) => *v as i16,
            Some(Generic::Int(v)) => *v as i16,
            _ => 0,
        }
    }

    fn get_short_or(&self, index: usize, dfl: i16) -> i16 {
        match self.get_generic(index) {
            Some(Generic::Short(v)) => v,
            Some(Generic::Byte(v)) => v as i16,
            Some(Generic::Int(v)) => v as i16,
            _ => dfl,
        }
    }

    fn set_short(&mut self, index: usize, to: i16) {
        self.elems.borrow_mut()[index] = Generic::Short(to);
    }

    fn get_int(&self, index: usize) -> i32 {
        match self.elems.borrow().get(index) {
            Some(Generic::Int(v)) => *v,
            Some(Generic::Byte(v)) => *v as i32,
            Some(Generic::Short(v)) => *v as i32,
            Some(Generic::Long(v)) => *v as i32,
            _ => 0,
        }
    }

    fn get_int_or(&self, index: usize, dfl: i32) -> i32 {
        if self.size() > index {
            self.get_int(index)
        } else {
            dfl
        }
    }

    fn set_int(&mut self, index: usize, to: i32) {
        self.elems.borrow_mut()[index] = Generic::Int(to);
    }

    fn get_long(&self, index: usize) -> i64 {
        match self.elems.borrow().get(index) {
            Some(Generic::Long(v)) => *v,
            Some(Generic::Byte(v)) => *v as i64,
            Some(Generic::Short(v)) => *v as i64,
            Some(Generic::Int(v)) => *v as i64,
            _ => 0,
        }
    }

    fn get_long_or(&self, index: usize, dfl: i64) -> i64 {
        if self.size() > index {
            self.get_long(index)
        } else {
            dfl
        }
    }

    fn set_long(&mut self, index: usize, to: i64) {
        self.elems.borrow_mut()[index] = Generic::Long(to);
    }

    fn get_float(&self, index: usize) -> f32 {
        match self.elems.borrow().get(index) {
            Some(Generic::Float(v)) => *v,
            Some(Generic::Int(v)) => *v as f32,
            Some(Generic::Double(v)) => *v as f32,
            _ => 0.0,
        }
    }

    fn get_float_or(&self, index: usize, dfl: f32) -> f32 {
        if self.size() > index {
            self.get_float(index)
        } else {
            dfl
        }
    }

    fn set_float(&mut self, index: usize, to: f32) {
        self.elems.borrow_mut()[index] = Generic::Float(to);
    }

    fn get_double(&self, index: usize) -> f64 {
        match self.elems.borrow().get(index) {
            Some(Generic::Double(v)) => *v,
            Some(Generic::Float(v)) => *v as f64,
            Some(Generic::Int(v)) => *v as f64,
            _ => 0.0,
        }
    }

    fn get_double_or(&self, index: usize, dfl: f64) -> f64 {
        if self.size() > index {
            self.get_double(index)
        } else {
            dfl
        }
    }

    fn set_double(&mut self, index: usize, to: f64) {
        self.elems.borrow_mut()[index] = Generic::Double(to);
    }

    fn get_bytes(&self, index: usize) -> Vec<i8> {
        match self.elems.borrow().get(index) {
            Some(Generic::Bytes(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    fn get_bytes_or(&self, index: usize, dfl: Vec<i8>) -> Vec<i8> {
        if self.size() > index {
            self.get_bytes(index)
        } else {
            dfl
        }
    }

    fn set_bytes(&mut self, index: usize, to: Vec<i8>) {
        self.elems.borrow_mut()[index] = Generic::Bytes(to);
    }

    fn get_shorts(&self, index: usize) -> Vec<i16> {
        match self.elems.borrow().get(index) {
            Some(Generic::Shorts(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    fn get_shorts_or(&self, index: usize, dfl: Vec<i16>) -> Vec<i16> {
        if self.size() > index {
            self.get_shorts(index)
        } else {
            dfl
        }
    }

    fn set_shorts(&mut self, index: usize, to: Vec<i16>) {
        self.elems.borrow_mut()[index] = Generic::Shorts(to);
    }

    fn get_ints(&self, index: usize) -> Vec<i32> {
        match self.elems.borrow().get(index) {
            Some(Generic::Ints(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    fn get_ints_or(&self, index: usize, dfl: Vec<i32>) -> Vec<i32> {
        if self.size() > index {
            self.get_ints(index)
        } else {
            dfl
        }
    }

    fn set_ints(&mut self, index: usize, to: Vec<i32>) {
        self.elems.borrow_mut()[index] = Generic::Ints(to);
    }

    fn get_longs(&self, index: usize) -> Vec<i64> {
        match self.elems.borrow().get(index) {
            Some(Generic::Longs(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    fn get_longs_or(&self, index: usize, dfl: Vec<i64>) -> Vec<i64> {
        if self.size() > index {
            self.get_longs(index)
        } else {
            dfl
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
        let mock = list.as_any().downcast_ref::<MockList>()?;
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
        let mock = map.as_any().downcast_ref::<MockMap>()?;
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
