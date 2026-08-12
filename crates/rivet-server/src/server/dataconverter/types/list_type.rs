//! Port of `ca.spottedleaf.dataconverter.types.ListType` — the container
//! interface for ordered element lists, plus the generic-dispatch defaults.
//!
//! Strictness contract (Java `ListType.java` header): the no-default getters
//! throw when the value at `index` is not the requested type (and never return
//! null); the default-value overloads return the default instead. The trait
//! signatures model that strict, non-null shape: the no-default accessors
//! return the plain value and panic on a wrong-typed element or out-of-range
//! index, matching the `NBTListType` throws (`IllegalStateException` /
//! `IndexOutOfBoundsException`). The JSON backing deviates by returning
//! `null`/`0` for the strict accessors — a concrete-backing property of
//! `types.json` that these signatures cannot express, and not resolved here.
//!
//! `setGeneric`/`addGeneric` are *set/add* operations, not inserts: Java's
//! `ListTag.set` calls `ArrayList.set`, which throws `IndexOutOfBoundsException`
//! for an unpopulated index (probe `listTypeDefaults.setGeneric_empty_oob`).

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::map_type::MapType;
use crate::server::dataconverter::types::object_type::ObjectType;
use crate::server::dataconverter::types::type_util::TypeUtilBase;
use rivet_serialization::number::Number;
use std::any::Any;

#[cfg(test)]
use crate::server::dataconverter::types::test_support::{
    MockList, MockMap, foundation_fixture, render_generic,
};

/// `ListType` — a list of elements, backed by NBT or JSON.
pub trait ListType: Any {
    /// `ListType.getTypeUtil()` — the backing's `TypeUtil` (Java wildcard
    /// `TypeUtil<?>`; only the factory surface is exposed).
    fn get_type_util(&self) -> &dyn TypeUtilBase;

    /// Downcast support for the concrete backings (`as_any`), mirroring the
    /// Java `instanceof NBTListType` casts the backings perform on the generic
    /// `Object` arguments they receive.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// `ListType.copy()` — a deep copy.
    fn copy(&self) -> Box<dyn ListType>;

    /// `ListType.getUniformType()` — the element type, or `NONE` if empty,
    /// `UNDEFINED` if the backing cannot represent it, `MIXED` if mixed.
    fn get_uniform_type(&self) -> ObjectType;

    /// `ListType.size()`.
    fn size(&self) -> usize;

    /// `ListType.remove(int)`.
    fn remove(&mut self, index: usize);

    /// `ListType.getGeneric(int)` — panics on an out-of-range index (Java
    /// `list.get(index)` throws `IndexOutOfBoundsException`); `None` models the
    /// `EndTag`/null case, never an out-of-range index.
    fn get_generic(&self, index: usize) -> Option<Generic>;

    // --- strict numeric getters/setters ---

    /// `ListType.getNumber(int)` — strict: panics on an out-of-range index or
    /// a non-numeric element (Java `NBTListType.getNumber` throws
    /// `IndexOutOfBoundsException`/`IllegalStateException`).
    fn get_number(&self, index: usize) -> Number;
    /// `ListType.getNumber(int, Number)` — the default for a non-numeric
    /// element; an out-of-range index still throws.
    fn get_number_or(&self, index: usize, dfl: Number) -> Number;
    /// `ListType.getByte(int)`.
    fn get_byte(&self, index: usize) -> i8;
    /// `ListType.getByte(int, byte)` — the default for a non-numeric element;
    /// an out-of-range index still throws.
    fn get_byte_or(&self, index: usize, dfl: i8) -> i8;
    /// `ListType.setByte(int, byte)`.
    fn set_byte(&mut self, index: usize, to: i8);
    /// `ListType.getShort(int)`.
    fn get_short(&self, index: usize) -> i16;
    /// `ListType.getShort(int, short)` — the default for a non-numeric element;
    /// an out-of-range index still throws.
    fn get_short_or(&self, index: usize, dfl: i16) -> i16;
    /// `ListType.setShort(int, short)`.
    fn set_short(&mut self, index: usize, to: i16);
    /// `ListType.getInt(int)`.
    fn get_int(&self, index: usize) -> i32;
    /// `ListType.getInt(int, int)` — the default for a non-numeric element; an
    /// out-of-range index still throws.
    fn get_int_or(&self, index: usize, dfl: i32) -> i32;
    /// `ListType.setInt(int, int)`.
    fn set_int(&mut self, index: usize, to: i32);
    /// `ListType.getLong(int)`.
    fn get_long(&self, index: usize) -> i64;
    /// `ListType.getLong(int, long)` — the default for a non-numeric element;
    /// an out-of-range index still throws.
    fn get_long_or(&self, index: usize, dfl: i64) -> i64;
    /// `ListType.setLong(int, long)`.
    fn set_long(&mut self, index: usize, to: i64);
    /// `ListType.getFloat(int)`.
    fn get_float(&self, index: usize) -> f32;
    /// `ListType.getFloat(int, float)` — the default for a non-numeric element;
    /// an out-of-range index still throws.
    fn get_float_or(&self, index: usize, dfl: f32) -> f32;
    /// `ListType.setFloat(int, float)`.
    fn set_float(&mut self, index: usize, to: f32);
    /// `ListType.getDouble(int)`.
    fn get_double(&self, index: usize) -> f64;
    /// `ListType.getDouble(int, double)` — the default for a non-numeric
    /// element; an out-of-range index still throws.
    fn get_double_or(&self, index: usize, dfl: f64) -> f64;
    /// `ListType.setDouble(int, double)`.
    fn set_double(&mut self, index: usize, to: f64);

    // --- raw array getters/setters ---

    /// `ListType.getBytes(int)`.
    fn get_bytes(&self, index: usize) -> Vec<i8>;
    /// `ListType.getBytes(int, byte[])` — the default for a non-`ByteArrayTag`
    /// element; an out-of-range index still throws.
    fn get_bytes_or(&self, index: usize, dfl: Vec<i8>) -> Vec<i8>;
    /// `ListType.setBytes(int, byte[])`.
    fn set_bytes(&mut self, index: usize, to: Vec<i8>);
    /// `ListType.getShorts(int)` — NBT has no short-array tag.
    fn get_shorts(&self, index: usize) -> Vec<i16>;
    /// `ListType.getShorts(int, short[])` — the default for a non-array
    /// element; an out-of-range index still throws (NBT itself throws
    /// `UnsupportedOperationException`).
    fn get_shorts_or(&self, index: usize, dfl: Vec<i16>) -> Vec<i16>;
    /// `ListType.setShorts(int, short[])`.
    fn set_shorts(&mut self, index: usize, to: Vec<i16>);
    /// `ListType.getInts(int)`.
    fn get_ints(&self, index: usize) -> Vec<i32>;
    /// `ListType.getInts(int, int[])` — the default for a non-`IntArrayTag`
    /// element; an out-of-range index still throws.
    fn get_ints_or(&self, index: usize, dfl: Vec<i32>) -> Vec<i32>;
    /// `ListType.setInts(int, int[])`.
    fn set_ints(&mut self, index: usize, to: Vec<i32>);
    /// `ListType.getLongs(int)`.
    fn get_longs(&self, index: usize) -> Vec<i64>;
    /// `ListType.getLongs(int, long[])` — the default for a non-`LongArrayTag`
    /// element; an out-of-range index still throws.
    fn get_longs_or(&self, index: usize, dfl: Vec<i64>) -> Vec<i64>;
    /// `ListType.setLongs(int, long[])`.
    fn set_longs(&mut self, index: usize, to: Vec<i64>);

    // --- container getters/setters ---

    /// `ListType.getList(int)` — strict: panics on an out-of-range index or a
    /// non-list element (Java `NBTListType.getList` throws
    /// `IndexOutOfBoundsException`/`IllegalStateException`).
    fn get_list(&self, index: usize) -> Box<dyn ListType>;
    /// `ListType.getList(int, ListType)` — the default for a non-list element;
    /// an out-of-range index still throws.
    fn get_list_or(&self, index: usize, dfl: Box<dyn ListType>) -> Box<dyn ListType>;
    /// `ListType.setList(int, ListType)`.
    fn set_list(&mut self, index: usize, list: Box<dyn ListType>);
    /// `ListType.getMap(int)` — strict: panics on an out-of-range index or a
    /// non-map element (Java `NBTListType.getMap` throws
    /// `IndexOutOfBoundsException`/`IllegalStateException`).
    fn get_map(&self, index: usize) -> Box<dyn MapType>;
    /// `ListType.getMap(int, MapType)` — the default for a non-map element; an
    /// out-of-range index still throws.
    fn get_map_or(&self, index: usize, dfl: Box<dyn MapType>) -> Box<dyn MapType>;
    /// `ListType.setMap(int, MapType)`.
    fn set_map(&mut self, index: usize, to: Box<dyn MapType>);
    /// `ListType.getString(int)` — strict: panics on an out-of-range index or a
    /// non-string element (Java `NBTListType.getString` throws
    /// `IndexOutOfBoundsException`/`IllegalStateException`).
    fn get_string(&self, index: usize) -> String;
    /// `ListType.getString(int, String)` — the default for a non-string
    /// element; an out-of-range index still throws.
    fn get_string_or(&self, index: usize, dfl: String) -> String;
    /// `ListType.setString(int, String)`.
    fn set_string(&mut self, index: usize, to: String);

    /// `ListType.setGeneric(int, Object)` — dispatch on the boxed value type.
    /// A `set`, not an insert: the index must already be populated (Java
    /// `ArrayList.set`).
    fn set_generic(&mut self, index: usize, to: Generic) {
        match to {
            Generic::Byte(b) => self.set_byte(index, b),
            Generic::Short(s) => self.set_short(index, s),
            Generic::Int(i) => self.set_int(index, i),
            Generic::Long(l) => self.set_long(index, l),
            Generic::Float(f) => self.set_float(index, f),
            Generic::Double(d) => self.set_double(index, d),
            Generic::Map(m) => self.set_map(index, m),
            Generic::List(l) => self.set_list(index, l),
            Generic::Str(s) => self.set_string(index, s),
            Generic::Bytes(b) => self.set_bytes(index, b),
            Generic::Shorts(s) => self.set_shorts(index, s),
            Generic::Ints(i) => self.set_ints(index, i),
            Generic::Longs(l) => self.set_longs(index, l),
            // Java `setGeneric` has no Boolean arm: a `Boolean` falls through
            // to the `IllegalArgumentException`. The concrete backings cannot
            // store one either, so it is intentionally unreachable.
            Generic::Bool(_) => panic!(
                "Object {} is not a valid type! (ListType.setGeneric has no Boolean arm)",
                "Boolean"
            ),
        }
    }

    /// `ListType.addGeneric(Object)` — append, dispatching on the boxed value
    /// type (Java `add`-with-index arms throw `UnsupportedOperationException`
    /// in the JSON backing; the NBT backing supports them).
    fn add_generic(&mut self, to: Generic) {
        match to {
            Generic::Byte(b) => self.add_byte(b),
            Generic::Short(s) => self.add_short(s),
            Generic::Int(i) => self.add_int(i),
            Generic::Long(l) => self.add_long(l),
            Generic::Float(f) => self.add_float(f),
            Generic::Double(d) => self.add_double(d),
            Generic::Map(m) => self.add_map(m),
            Generic::List(l) => self.add_list(l),
            Generic::Str(s) => self.add_string(s),
            Generic::Bytes(b) => self.add_byte_array(b),
            Generic::Shorts(s) => self.add_short_array(s),
            Generic::Ints(i) => self.add_int_array(i),
            Generic::Longs(l) => self.add_long_array(l),
            Generic::Bool(_) => panic!(
                "Object {} is not a valid type! (ListType.addGeneric has no Boolean arm)",
                "Boolean"
            ),
        }
    }

    // --- append operations (Java `addX`/`addX(int, ...)`) ---

    /// `ListType.addByte(byte)`.
    fn add_byte(&mut self, b: i8);
    /// `ListType.addByte(int, byte)` — the index-add forms throw
    /// `UnsupportedOperationException` in the JSON backing.
    fn add_byte_at(&mut self, index: usize, b: i8);
    /// `ListType.addShort(short)`.
    fn add_short(&mut self, s: i16);
    /// `ListType.addShort(int, short)`.
    fn add_short_at(&mut self, index: usize, s: i16);
    /// `ListType.addInt(int)`.
    fn add_int(&mut self, i: i32);
    /// `ListType.addInt(int, int)`.
    fn add_int_at(&mut self, index: usize, i: i32);
    /// `ListType.addLong(long)`.
    fn add_long(&mut self, l: i64);
    /// `ListType.addLong(int, long)`.
    fn add_long_at(&mut self, index: usize, l: i64);
    /// `ListType.addFloat(float)`.
    fn add_float(&mut self, f: f32);
    /// `ListType.addFloat(int, float)`.
    fn add_float_at(&mut self, index: usize, f: f32);
    /// `ListType.addDouble(double)`.
    fn add_double(&mut self, d: f64);
    /// `ListType.addDouble(int, double)`.
    fn add_double_at(&mut self, index: usize, d: f64);
    /// `ListType.addByteArray(byte[])`.
    fn add_byte_array(&mut self, arr: Vec<i8>);
    /// `ListType.addByteArray(int, byte[])`.
    fn add_byte_array_at(&mut self, index: usize, arr: Vec<i8>);
    /// `ListType.addShortArray(short[])`.
    fn add_short_array(&mut self, arr: Vec<i16>);
    /// `ListType.addShortArray(int, short[])`.
    fn add_short_array_at(&mut self, index: usize, arr: Vec<i16>);
    /// `ListType.addIntArray(int[])`.
    fn add_int_array(&mut self, arr: Vec<i32>);
    /// `ListType.addIntArray(int, int[])`.
    fn add_int_array_at(&mut self, index: usize, arr: Vec<i32>);
    /// `ListType.addLongArray(long[])`.
    fn add_long_array(&mut self, arr: Vec<i64>);
    /// `ListType.addLongArray(int, long[])`.
    fn add_long_array_at(&mut self, index: usize, arr: Vec<i64>);
    /// `ListType.addList(ListType)`.
    fn add_list(&mut self, list: Box<dyn ListType>);
    /// `ListType.addList(int, ListType)`.
    fn add_list_at(&mut self, index: usize, list: Box<dyn ListType>);
    /// `ListType.addMap(MapType)`.
    fn add_map(&mut self, map: Box<dyn MapType>);
    /// `ListType.addMap(int, MapType)`.
    fn add_map_at(&mut self, index: usize, map: Box<dyn MapType>);
    /// `ListType.addString(String)`.
    fn add_string(&mut self, string: String);
    /// `ListType.addString(int, String)`.
    fn add_string_at(&mut self, index: usize, string: String);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `set_generic` is a *set*, not an insert: setting an index past the end
    /// panics (the concrete backings index `ArrayList.set`/`ListTag.set`,
    /// probe `listTypeDefaults.setGeneric_empty_oob`).
    #[test]
    #[should_panic]
    fn set_generic_on_unpopulated_index_panics() {
        let mut list = MockList::new();
        list.set_generic(0, Generic::Int(3));
    }

    /// `set_generic` on a populated index replaces the element and dispatches to
    /// the matching typed setter (probe `listTypeDefaults.setGeneric_int`).
    #[test]
    fn set_generic_replaces_populated_index() {
        let mut list = MockList::new();
        list.add_int(0);
        list.set_generic(0, Generic::Int(3));
        assert_eq!(list.get_int(0), 3);
    }

    /// `add_generic` appends, dispatching on the boxed value type across the
    /// number/string/map/array forms (probe `listTypeDefaults.addGeneric_*`).
    #[test]
    fn add_generic_dispatches_to_typed_adds() {
        let mut list = MockList::new();
        list.add_generic(Generic::Int(3));
        list.add_generic(Generic::Short(4));
        list.add_generic(Generic::Str("five".into()));
        let mut map = MockMap::new();
        map.set_int("k", 6);
        list.add_generic(Generic::Map(Box::new(map)));
        list.add_generic(Generic::Ints(vec![6, 7]));

        assert_eq!(list.get_int(0), 3);
        assert_eq!(list.get_short(1), 4);
        assert_eq!(list.get_string(2), "five");
        assert_eq!(list.get_map(3).get_int("k"), 6);
        assert_eq!(list.get_ints(4), vec![6, 7]);
    }

    /// `get_uniform_type` reports the element type, or `NONE` for empty,
    /// `MIXED` for mixed types (Java `ListType.getUniformType()`).
    #[test]
    fn uniform_type() {
        assert_eq!(MockList::new().get_uniform_type(), ObjectType::None);
        let mut ints = MockList::new();
        ints.add_int(1);
        ints.add_int(2);
        assert_eq!(ints.get_uniform_type(), ObjectType::Int);
        let mut mixed = MockList::new();
        mixed.add_int(1);
        mixed.add_string("x".into());
        assert_eq!(mixed.get_uniform_type(), ObjectType::Mixed);
    }

    /// A `get_list`/`get_map` element view shares the parent's storage:
    /// mutating a returned view is visible in the parent.
    #[test]
    fn container_element_aliases_parent_storage() {
        let mut list = MockList::new();
        let mut inner = MockMap::new();
        inner.set_int("k", 1);
        list.add_map(Box::new(inner));

        let mut view = list.get_map(0);
        view.set_int("k", 42);
        assert_eq!(list.get_map(0).get_int("k"), 42);
    }

    /// A `get_generic` container element view shares the parent's storage
    /// (`NBTListType.getGeneric` wraps the same `ListTag` element).
    #[test]
    fn get_generic_container_element_aliases_parent_storage() {
        let mut list = MockList::new();
        let mut inner = MockMap::new();
        inner.set_int("k", 1);
        list.add_map(Box::new(inner));
        list.add_list(Box::new(MockList::new()));

        let Generic::Map(mut map_view) = list.get_generic(0).unwrap() else {
            panic!("expected a map view");
        };
        map_view.set_int("k", 42);
        assert_eq!(list.get_map(0).get_int("k"), 42);

        let Generic::List(mut list_view) = list.get_generic(1).unwrap() else {
            panic!("expected a list view");
        };
        list_view.add_int(7);
        assert_eq!(list.get_list(1).get_int(0), 7);
    }

    /// `ListType.copy` is a deep copy: mutating the copy must not affect the
    /// source.
    #[test]
    fn copy_is_deep() {
        let mut list = MockList::new();
        list.add_int(3);
        let mut copy = list.copy();
        copy.set_int(0, 99);
        assert_eq!(list.get_int(0), 3);
        assert_eq!(copy.get_int(0), 99);
    }

    /// The array `_or` getters return the supplied default for a present-but-
    /// wrong-typed element, matching `NBTListType.getBytes(index, dfl)` etc.
    /// (which return `dfl` when the element is not the matching array tag) —
    /// not an empty array. An out-of-range index still throws (the `list.get`
    /// bound check happens before the tag check).
    #[test]
    fn array_getters_return_default_for_wrong_typed_element() {
        let mut list = MockList::new();
        list.add_string("x".into());

        assert_eq!(list.get_bytes_or(0, vec![9]), vec![9]);
        assert_eq!(list.get_shorts_or(0, vec![9]), vec![9]);
        assert_eq!(list.get_ints_or(0, vec![9]), vec![9]);
        assert_eq!(list.get_longs_or(0, vec![9]), vec![9]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_bytes_or(9, vec![9]);
            }))
            .is_err()
        );

        // The matching type still extracts the stored array.
        list.add_byte_array(vec![1, 2]);
        assert_eq!(list.get_bytes_or(1, vec![9]), vec![1, 2]);
    }

    /// The strict no-default `get_number`/`get_string`/`get_list`/`get_map`
    /// accessors panic on a present-but-wrong-typed element, matching the
    /// `NBTListType` throws (`getNumber`/`getString`/`getList`/`getMap` throw
    /// `IllegalStateException` for a non-matching tag) — they never silently
    /// yield `None`. Their `_or` overloads instead return the supplied default
    /// for the wrong-typed element.
    #[test]
    fn strict_accessors_panic_on_wrong_typed_element() {
        let mut list = MockList::new();
        list.add_string("s".into());
        list.add_int(7);

        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_number(0);
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_string(1);
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_list(0);
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_map(1);
            }))
            .is_err()
        );

        // The `_or` overloads return the default for the wrong-typed element.
        assert_eq!(list.get_number_or(0, Number::from(9)), Number::from(9));
        assert_eq!(list.get_string_or(1, "dfl".into()), "dfl");
        assert_eq!(list.get_list_or(0, Box::new(MockList::new())).size(), 0);
        let mut map_dfl = MockMap::new();
        map_dfl.set_int("k", 42);
        assert_eq!(list.get_map_or(1, Box::new(map_dfl)).get_int("k"), 42);

        // The matching type still extracts the stored value.
        assert_eq!(list.get_string(0), "s");
        assert_eq!(list.get_number(1), Number::from(7));
    }

    /// The six numeric `_or` getters return the supplied default for a present-
    /// but-wrong-typed element, while still coercing a present numeric element
    /// (Java `NBTListType.getByte(index, dfl)` etc. return `dfl` only for a
    /// non-`NumericTag`; a present `NumericTag` is always narrowed).
    #[test]
    fn numeric_or_getters_return_default_for_wrong_typed_element() {
        let mut list = MockList::new();
        list.add_string("x".into());
        list.add_int(7);

        assert_eq!(list.get_byte_or(0, 9), 9);
        assert_eq!(list.get_short_or(0, 9), 9);
        assert_eq!(list.get_int_or(0, 9), 9);
        assert_eq!(list.get_long_or(0, 9), 9);
        assert_eq!(list.get_float_or(0, 9.0), 9.0);
        assert_eq!(list.get_double_or(0, 9.0), 9.0);

        // A present numeric element is coerced, never the default.
        assert_eq!(list.get_byte_or(1, 9), 7);
        assert_eq!(list.get_short_or(1, 9), 7);
        assert_eq!(list.get_int_or(1, 9), 7);
        assert_eq!(list.get_long_or(1, 9), 7);
        assert_eq!(list.get_float_or(1, 9.0), 7.0);
        assert_eq!(list.get_double_or(1, 9.0), 7.0);
    }

    /// Every `_or` overload panics on an out-of-range index, matching
    /// `NBTListType`: `list.get(index)` throws `IndexOutOfBoundsException`
    /// before the tag/default decision. The default is only for a present-but-
    /// wrong-typed element.
    #[test]
    fn or_getters_panic_on_out_of_range() {
        let list = MockList::new();

        let panics =
            |f: &dyn Fn()| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err();
        assert!(panics(&|| {
            let _ = list.get_number_or(0, Number::from(9));
        }));
        assert!(panics(&|| {
            let _ = list.get_byte_or(0, 9);
        }));
        assert!(panics(&|| {
            let _ = list.get_short_or(0, 9);
        }));
        assert!(panics(&|| {
            let _ = list.get_int_or(0, 9);
        }));
        assert!(panics(&|| {
            let _ = list.get_long_or(0, 9);
        }));
        assert!(panics(&|| {
            let _ = list.get_float_or(0, 9.0);
        }));
        assert!(panics(&|| {
            let _ = list.get_double_or(0, 9.0);
        }));
        assert!(panics(&|| {
            let _ = list.get_bytes_or(0, vec![9]);
        }));
        assert!(panics(&|| {
            let _ = list.get_shorts_or(0, vec![9]);
        }));
        assert!(panics(&|| {
            let _ = list.get_ints_or(0, vec![9]);
        }));
        assert!(panics(&|| {
            let _ = list.get_longs_or(0, vec![9]);
        }));
        assert!(panics(&|| {
            let _ = list.get_list_or(0, Box::new(MockList::new()));
        }));
        assert!(panics(&|| {
            let _ = list.get_map_or(0, Box::new(MockMap::new()));
        }));
        assert!(panics(&|| {
            let _ = list.get_string_or(0, "dfl".into());
        }));
    }

    /// `get_generic` panics on an out-of-range index, matching
    /// `NBTListType.getGeneric` (`list.get(index)` throws); `None` is reserved
    /// for the `EndTag`/null element, never for an out-of-range index.
    #[test]
    fn get_generic_panics_on_out_of_range() {
        let list = MockList::new();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = list.get_generic(0);
            }))
            .is_err()
        );
    }

    /// Differential check of the default `setGeneric`/`addGeneric` dispatch
    /// against the committed `dataconverter-foundation` oracle golden
    /// (`listTypeDefaults`): rebuilding the probe's scenario over [`MockList`]
    /// must reproduce every recorded value.
    #[test]
    fn list_type_defaults_match_paper_golden() {
        let golden = &foundation_fixture()["listTypeDefaults"];

        // `setGeneric` is a set on a populated index; the rest are appends
        // (probe `listTypeDefaults.setGeneric_int`/`addGeneric_*`).
        let mut list = MockList::new();
        list.add_int(0);
        list.set_generic(0, Generic::Int(3));
        list.add_generic(Generic::Short(4));
        list.add_generic(Generic::Str("five".into()));
        list.add_generic(Generic::Map(Box::new(MockMap::new())));
        list.add_generic(Generic::Ints(vec![6, 7]));

        assert_eq!(
            list.get_int(0),
            golden["setGeneric_int"].as_i64().unwrap() as i32
        );
        assert_eq!(
            list.get_short(1),
            golden["addGeneric_short"].as_i64().unwrap() as i16
        );
        assert_eq!(
            render_generic(&list.get_generic(2).unwrap()),
            golden["addGeneric_string"].as_str().unwrap()
        );
        assert!(matches!(list.get_generic(3), Some(Generic::Map(_))));
        assert_eq!(
            render_generic(&list.get_generic(4).unwrap()),
            golden["addGeneric_ints"].as_str().unwrap()
        );

        // `setGeneric` on an unpopulated index throws in Java
        // (`IndexOutOfBoundsException`); the Rust mock panics.
        let result = std::panic::catch_unwind(|| {
            let mut empty = MockList::new();
            empty.set_generic(0, Generic::Int(1));
        });
        assert_eq!(
            result.is_err(),
            golden["setGeneric_empty_oob"].as_bool().unwrap()
        );
    }
}
