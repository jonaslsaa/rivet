//! Port of `ca.spottedleaf.dataconverter.types.MapType` — the container
//! interface for keyed maps, plus the generic-dispatch and get-or-create
//! defaults.
//!
//! Non-strict getters (Java `MapType.java` header): "types here are not strict.
//! if the key maps to a different type, default is always returned; if default
//! is not a parameter, then default is always null."
//!
//! The `getMap`/`getList`/`getListUnchecked` methods return a *view* wrapping
//! the same backing as the receiver, so mutating a returned view is visible in
//! the parent (Java `new NBTMapType(compoundTag)` etc.) — the concrete backings
//! use shared single-threaded storage to make that aliasing exact.

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use rivet_serialization::number::Number;
use std::any::Any;

#[cfg(test)]
use crate::server::dataconverter::types::test_support::{MockList, MockMap};

/// `MapType` — a keyed map, backed by NBT or JSON.
pub trait MapType: Any {
    /// `MapType.getTypeUtil()` — the backing's `TypeUtil` (Java wildcard).
    fn get_type_util(&self) -> &dyn TypeUtilBase;

    /// Downcast support for the concrete backings.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// `MapType.size()`.
    fn size(&self) -> usize;

    /// `MapType.isEmpty()`.
    fn is_empty(&self) -> bool;

    /// `MapType.clear()`.
    fn clear(&mut self);

    /// `MapType.keys()` — the key set, in the backing's iteration order
    /// (insertion order for the JSON `LinkedHashSet`, arbitrary for NBT).
    fn keys(&self) -> Vec<String>;

    /// `MapType.copy()` — a deep copy.
    fn copy(&self) -> Box<dyn MapType>;

    /// `MapType.hasKey(String)`.
    fn has_key(&self, key: &str) -> bool;

    /// `MapType.hasKey(String, ObjectType)`.
    fn has_key_of_type(&self, key: &str, ty: ObjectType) -> bool;

    /// `MapType.remove(String)`.
    fn remove(&mut self, key: &str);

    /// `MapType.getGeneric(String)` — `None` when the key is absent.
    fn get_generic(&self, key: &str) -> Option<Generic>;

    // --- non-strict numeric getters/setters (default is returned on mismatch) ---

    /// `MapType.getNumber(String)`.
    fn get_number(&self, key: &str) -> Option<Number>;
    /// `MapType.getNumber(String, Number)`.
    fn get_number_or(&self, key: &str, dfl: Number) -> Number;
    /// `MapType.getBoolean(String)`.
    fn get_boolean(&self, key: &str) -> bool;
    /// `MapType.getBoolean(String, boolean)`.
    fn get_boolean_or(&self, key: &str, dfl: bool) -> bool;
    /// `MapType.setBoolean(String, boolean)`.
    fn set_boolean(&mut self, key: &str, val: bool);
    /// `MapType.getByte(String)` — `0` on absence/non-number.
    fn get_byte(&self, key: &str) -> i8;
    /// `MapType.getByte(String, byte)`.
    fn get_byte_or(&self, key: &str, dfl: i8) -> i8;
    /// `MapType.setByte(String, byte)`.
    fn set_byte(&mut self, key: &str, val: i8);
    /// `MapType.getShort(String)`.
    fn get_short(&self, key: &str) -> i16;
    /// `MapType.getShort(String, short)`.
    fn get_short_or(&self, key: &str, dfl: i16) -> i16;
    /// `MapType.setShort(String, short)`.
    fn set_short(&mut self, key: &str, val: i16);
    /// `MapType.getInt(String)`.
    fn get_int(&self, key: &str) -> i32;
    /// `MapType.getInt(String, int)`.
    fn get_int_or(&self, key: &str, dfl: i32) -> i32;
    /// `MapType.setInt(String, int)`.
    fn set_int(&mut self, key: &str, val: i32);
    /// `MapType.getLong(String)`.
    fn get_long(&self, key: &str) -> i64;
    /// `MapType.getLong(String, long)`.
    fn get_long_or(&self, key: &str, dfl: i64) -> i64;
    /// `MapType.setLong(String, long)`.
    fn set_long(&mut self, key: &str, val: i64);
    /// `MapType.getFloat(String)`.
    fn get_float(&self, key: &str) -> f32;
    /// `MapType.getFloat(String, float)`.
    fn get_float_or(&self, key: &str, dfl: f32) -> f32;
    /// `MapType.setFloat(String, float)`.
    fn set_float(&mut self, key: &str, val: f32);
    /// `MapType.getDouble(String)`.
    fn get_double(&self, key: &str) -> f64;
    /// `MapType.getDouble(String, double)`.
    fn get_double_or(&self, key: &str, dfl: f64) -> f64;
    /// `MapType.setDouble(String, double)`.
    fn set_double(&mut self, key: &str, val: f64);

    // --- raw array getters/setters (NBT supports byte/int/long; short is inert) ---

    /// `MapType.getBytes(String)`.
    fn get_bytes(&self, key: &str) -> Option<Vec<i8>>;
    /// `MapType.getBytes(String, byte[])`.
    fn get_bytes_or(&self, key: &str, dfl: Vec<i8>) -> Vec<i8>;
    /// `MapType.setBytes(String, byte[])`.
    fn set_bytes(&mut self, key: &str, val: Vec<i8>);
    /// `MapType.getShorts(String)` — no backing stores a short array.
    fn get_shorts(&self, key: &str) -> Option<Vec<i16>>;
    /// `MapType.getShorts(String, short[])`.
    fn get_shorts_or(&self, key: &str, dfl: Vec<i16>) -> Vec<i16>;
    /// `MapType.setShorts(String, short[])`.
    fn set_shorts(&mut self, key: &str, val: Vec<i16>);
    /// `MapType.getInts(String)`.
    fn get_ints(&self, key: &str) -> Option<Vec<i32>>;
    /// `MapType.getInts(String, int[])`.
    fn get_ints_or(&self, key: &str, dfl: Vec<i32>) -> Vec<i32>;
    /// `MapType.setInts(String, int[])`.
    fn set_ints(&mut self, key: &str, val: Vec<i32>);
    /// `MapType.getLongs(String)`.
    fn get_longs(&self, key: &str) -> Option<Vec<i64>>;
    /// `MapType.getLongs(String, long[])`.
    fn get_longs_or(&self, key: &str, dfl: Vec<i64>) -> Vec<i64>;
    /// `MapType.setLongs(String, long[])`.
    fn set_longs(&mut self, key: &str, val: Vec<i64>);

    // --- container getters/setters ---

    /// `MapType.getListUnchecked(String)` — the list view without a type check.
    fn get_list_unchecked(&self, key: &str) -> Option<Box<dyn ListType>>;
    /// `MapType.getListUnchecked(String, ListType)`.
    fn get_list_unchecked_or(&self, key: &str, dfl: Box<dyn ListType>) -> Box<dyn ListType>;

    /// `MapType.getList(String, ObjectType)` — the list only when its uniform
    /// element type matches `ty` (or is `UNDEFINED`/`NONE`), else `None`.
    fn get_list(&self, key: &str, ty: ObjectType) -> Option<Box<dyn ListType>> {
        let ret = self.get_list_unchecked(key);
        match ret {
            Some(list) => {
                let uniform = list.get_uniform_type();
                if uniform == ty || uniform == ObjectType::Undefined || uniform == ObjectType::None
                {
                    Some(list)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// `MapType.getOrCreateList(String, ObjectType)` — the list, or a fresh
    /// empty list of this backing's type, inserted and returned.
    fn get_or_create_list(&mut self, key: &str, ty: ObjectType) -> Box<dyn ListType> {
        if let Some(ret) = self.get_list(key, ty) {
            return ret;
        }
        let empty = self.get_type_util().create_empty_list();
        self.set_list(key, empty);
        self.get_list_unchecked(key)
            .expect("getOrCreateList just inserted an empty list")
    }

    /// `MapType.setList(String, ListType)`.
    fn set_list(&mut self, key: &str, val: Box<dyn ListType>);

    /// `MapType.getMap(String)`.
    fn get_map(&self, key: &str) -> Option<Box<dyn MapType>>;
    /// `MapType.getMap(String, MapType)`.
    fn get_map_or(&self, key: &str, dfl: Box<dyn MapType>) -> Box<dyn MapType>;

    /// `MapType.getOrCreateMap(String)` — the map, or a fresh empty map of
    /// this backing's type, inserted and returned.
    fn get_or_create_map(&mut self, key: &str) -> Box<dyn MapType> {
        if let Some(ret) = self.get_map(key) {
            return ret;
        }
        let empty = self.get_type_util().create_empty_map();
        self.set_map(key, empty);
        self.get_map(key)
            .expect("getOrCreateMap just inserted an empty map")
    }

    /// `MapType.setMap(String, MapType)`.
    fn set_map(&mut self, key: &str, val: Box<dyn MapType>);

    /// `MapType.getString(String)`.
    fn get_string(&self, key: &str) -> Option<String>;
    /// `MapType.getString(String, String)`.
    fn get_string_or(&self, key: &str, dfl: String) -> String;
    /// `MapType.setString(String, String)`.
    fn set_string(&mut self, key: &str, val: String);

    /// `MapType.setGeneric(String, Object)` — dispatch on the boxed value type.
    fn set_generic(&mut self, key: &str, value: Generic) {
        match value {
            Generic::Bool(b) => self.set_boolean(key, b),
            Generic::Byte(b) => self.set_byte(key, b),
            Generic::Short(s) => self.set_short(key, s),
            Generic::Int(i) => self.set_int(key, i),
            Generic::Long(l) => self.set_long(key, l),
            Generic::Float(f) => self.set_float(key, f),
            Generic::Double(d) => self.set_double(key, d),
            Generic::Map(m) => self.set_map(key, m),
            Generic::List(l) => self.set_list(key, l),
            Generic::Str(s) => self.set_string(key, s),
            Generic::Bytes(b) => self.set_bytes(key, b),
            Generic::Shorts(s) => self.set_shorts(key, s),
            Generic::Ints(i) => self.set_ints(key, i),
            Generic::Longs(l) => self.set_longs(key, l),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MapType.setGeneric` dispatch: each boxed `Generic` lands on the matching
    /// typed setter and round-trips through the typed getter (probe
    /// `mapTypeDefaults`).
    #[test]
    fn set_generic_dispatches_to_typed_setters() {
        let mut map = MockMap::new();
        map.set_generic("b", Generic::Byte(1));
        map.set_generic("s", Generic::Short(2));
        map.set_generic("i", Generic::Int(3));
        map.set_generic("l", Generic::Long(4));
        map.set_generic("f", Generic::Float(5.5));
        map.set_generic("d", Generic::Double(6.5));
        map.set_generic("bool", Generic::Bool(true));
        map.set_generic("str", Generic::Str("seven".into()));
        map.set_generic("bytes", Generic::Bytes(vec![8, 9]));
        map.set_generic("ints", Generic::Ints(vec![10, 11]));
        map.set_generic("longs", Generic::Longs(vec![12, 13]));

        assert_eq!(map.get_byte("b"), 1);
        assert_eq!(map.get_short("s"), 2);
        assert_eq!(map.get_int("i"), 3);
        assert_eq!(map.get_long("l"), 4);
        assert_eq!(map.get_float("f"), 5.5);
        assert_eq!(map.get_double("d"), 6.5);
        assert!(map.get_boolean("bool"));
        assert_eq!(map.get_string("str").as_deref(), Some("seven"));
        assert_eq!(map.get_bytes("bytes"), Some(vec![8, 9]));
        assert_eq!(map.get_ints("ints"), Some(vec![10, 11]));
        assert_eq!(map.get_longs("longs"), Some(vec![12, 13]));
    }

    /// `set_generic` with a map/list boxed value stores the container itself
    /// (the `Generic::Map`/`Generic::List` arms).
    #[test]
    fn set_generic_containers() {
        let mut map = MockMap::new();
        let mut inner = MockMap::new();
        inner.set_int("k", 7);
        map.set_generic("map", Generic::Map(Box::new(inner)));
        map.set_generic("list", Generic::List(Box::new(MockList::new())));
        assert!(map.get_map("map").is_some());
        assert!(map.get_list_unchecked("list").is_some());
    }

    /// `get_list` type gate: only a matching (or empty/undefined) uniform type
    /// is returned (probe `mapTypeDefaults.getList_*`).
    #[test]
    fn get_list_type_gate() {
        let mut map = MockMap::new();
        let mut ints = MockList::new();
        ints.add_int(1);
        map.set_list("i", Box::new(ints));

        assert!(map.get_list("i", ObjectType::Int).is_some());
        assert!(map.get_list("i", ObjectType::String).is_none());
        assert!(map.get_list("missing", ObjectType::Int).is_none());

        let mut strings = MockList::new();
        strings.add_string("x".into());
        map.set_list("s", Box::new(strings));
        assert!(map.get_list("s", ObjectType::String).is_some());
        assert!(map.get_list("s", ObjectType::Int).is_none());
    }

    /// `get_or_create_list` reuses an existing typed list; `get_or_create_map`
    /// reuses an existing map. A missing key inserts the backing's empty
    /// container (probe `mapTypeDefaults.getOrCreate_*`).
    #[test]
    fn get_or_create_reuses_existing() {
        let mut map = MockMap::new();
        let mut ints = MockList::new();
        ints.add_int(1);
        map.set_list("i", Box::new(ints));

        let reused = map.get_or_create_list("i", ObjectType::Int);
        assert_eq!(reused.size(), 1);

        let created = map.get_or_create_list("fresh", ObjectType::Int);
        assert_eq!(created.size(), 0);
        assert!(map.has_key("fresh"));

        let mut inner = MockMap::new();
        inner.set_int("k", 7);
        map.set_map("m", Box::new(inner));
        let reused_map = map.get_or_create_map("m");
        assert_eq!(reused_map.get_int("k"), 7);
        assert!(map.get_or_create_map("missing").is_empty());
        assert!(map.has_key("missing"));
    }

    /// `MapType.copy` is a deep copy: mutating the copy must not affect the
    /// source (Java `MapType.copy()`).
    #[test]
    fn copy_is_deep() {
        let mut map = MockMap::new();
        map.set_int("i", 3);
        let mut copy = map.copy();
        copy.set_int("i", 99);
        assert_eq!(map.get_int("i"), 3);
        assert_eq!(copy.get_int("i"), 99);
    }

    /// A `get_map` view shares the parent's storage: mutating a returned view
    /// is visible in the parent (the Java `getMap` wrapping of the same
    /// backing).
    #[test]
    fn get_map_aliases_parent_storage() {
        let mut map = MockMap::new();
        let mut inner = MockMap::new();
        inner.set_int("k", 1);
        map.set_map("m", Box::new(inner));

        let mut view = map.get_map("m").unwrap();
        view.set_int("k", 42);
        assert_eq!(map.get_map("m").unwrap().get_int("k"), 42);
    }

    /// A `get_generic` map/list view shares the parent's storage, mirroring
    /// `NBTMapType.getGeneric` wrapping the same `CompoundTag`/`ListTag`: a
    /// mutation through the returned container is visible in the parent.
    #[test]
    fn get_generic_container_view_aliases_parent_storage() {
        let mut map = MockMap::new();
        let mut inner = MockMap::new();
        inner.set_int("k", 1);
        map.set_map("m", Box::new(inner));
        map.set_list("l", Box::new(MockList::new()));

        let Generic::Map(mut map_view) = map.get_generic("m").unwrap() else {
            panic!("expected a map view");
        };
        map_view.set_int("k", 42);
        assert_eq!(map.get_map("m").unwrap().get_int("k"), 42);

        let Generic::List(mut list_view) = map.get_generic("l").unwrap() else {
            panic!("expected a list view");
        };
        list_view.add_int(7);
        assert_eq!(map.get_list_unchecked("l").unwrap().get_int(0), 7);
    }

    /// Non-strict numeric/boolean coercion mirrors the NBT backing: `getBoolean`
    /// is `getByte != 0` over any number, and the `_or` overloads return the
    /// default when the value is present but not a number (Java
    /// `NBTMapType.getBoolean`/`getInt(key, dfl)`).
    #[test]
    fn numeric_coercion_matches_nbt_backing() {
        let mut map = MockMap::new();
        map.set_int("i", 5);
        map.set_string("s", "x".into());
        map.set_long("l", 300);
        map.set_double("d", 300.0);

        assert!(map.get_boolean("i"));
        // Java `Double.byteValue()` = `(byte)(int)`: 300 wraps to 44, not 0.
        assert_eq!(map.get_byte("l"), 44);
        assert_eq!(map.get_byte("d"), 44);
        assert_eq!(map.get_int("d"), 300);
        assert_eq!(map.get_int_or("s", 42), 42);
    }

    /// `setBoolean` stores a byte, so `getGeneric("bool")` reads back the byte
    /// `1`, matching the probe golden `mapTypeDefaults.bool = "1"` (NBT stores
    /// booleans as `ByteTag`).
    #[test]
    fn set_boolean_round_trips_as_byte() {
        let mut map = MockMap::new();
        map.set_boolean("bool", true);
        assert!(matches!(map.get_generic("bool"), Some(Generic::Byte(1))));
        assert!(map.get_boolean("bool"));
    }

    /// `has_key(key, type)` checks the value's ObjectType (probe
    /// `mapTypeDefaults.hasKey_*`): `INT` matches only an int, `NUMBER` matches
    /// any number, `STRING` matches a string.
    #[test]
    fn has_key_of_type() {
        let mut map = MockMap::new();
        map.set_int("i", 1);
        map.set_string("s", "x".into());
        assert!(map.has_key_of_type("i", ObjectType::Int));
        assert!(!map.has_key_of_type("i", ObjectType::Byte));
        assert!(map.has_key_of_type("i", ObjectType::Number));
        assert!(map.has_key_of_type("s", ObjectType::String));
        assert!(!map.has_key_of_type("unknown", ObjectType::Int));
    }
}
