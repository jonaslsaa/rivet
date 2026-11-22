//! Port of `ca.spottedleaf.dataconverter.types.nbt`.
//!
//! Provides the concrete NBT-backed [`NBTListType`], [`NBTMapType`], and
//! [`NBTTypeUtil`].
//!
//! Because `rivet_nbt::Tag` is an owned value enum whereas Java's
//! `CompoundTag`/`ListTag` are reference-aliased containers, `NBTMapType` and
//! `NBTListType` wrap shared `Rc<RefCell<...>>` nodes (`NbtValue`) so nested
//! views returned by `get_map`/`get_list`/`get_generic` alias their parent
//! exactly as in Java, while `copy()` performs a deep copy and `get_tag()`
//! materializes a `CompoundTag`/`ListTag`.

pub mod nbt_list_type;
pub mod nbt_map_type;
pub mod nbt_type_util;

pub use nbt_list_type::NBTListType;
pub use nbt_map_type::NBTMapType;
pub use nbt_type_util::NBTTypeUtil;

use crate::server::dataconverter::types::object_type::ObjectType;
use indexmap::IndexMap;
use rivet_nbt::byte_array_tag::ByteArrayTag;
use rivet_nbt::byte_tag::ByteTag;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::double_tag::DoubleTag;
use rivet_nbt::end_tag::EndTag;
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::int_array_tag::IntArrayTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::long_array_tag::LongArrayTag;
use rivet_nbt::long_tag::LongTag;
use rivet_nbt::numeric_tag::NumericTag;
use rivet_nbt::short_tag::ShortTag;
use rivet_nbt::string_tag::StringTag;
use rivet_nbt::tag::{
    TAG_BYTE, TAG_BYTE_ARRAY, TAG_COMPOUND, TAG_DOUBLE, TAG_END, TAG_FLOAT, TAG_INT, TAG_INT_ARRAY,
    TAG_LIST, TAG_LONG, TAG_LONG_ARRAY, TAG_SHORT, TAG_STRING, Tag,
};
use rivet_serialization::float_format::{java_double_equals, java_float_equals};
use rivet_serialization::number::Number;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) type SharedNbtList = Rc<RefCell<Vec<NbtValue>>>;
pub(crate) type SharedNbtCompound = Rc<RefCell<IndexMap<String, NbtValue>>>;

/// Reference-aliased internal NBT node backing `NBTListType` and `NBTMapType`.
#[derive(Debug, Clone)]
pub(crate) enum NbtValue {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(SharedNbtList),
    Compound(SharedNbtCompound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl PartialEq for NbtValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::End, Self::End) => true,
            (Self::Byte(a), Self::Byte(b)) => a == b,
            (Self::Short(a), Self::Short(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Long(a), Self::Long(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => java_float_equals(*a, *b),
            (Self::Double(a), Self::Double(b)) => java_double_equals(*a, *b),
            (Self::ByteArray(a), Self::ByteArray(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Compound(a), Self::Compound(b)) => a == b,
            (Self::IntArray(a), Self::IntArray(b)) => a == b,
            (Self::LongArray(a), Self::LongArray(b)) => a == b,
            _ => false,
        }
    }
}

impl NbtValue {
    pub(crate) fn tag_id(&self) -> i8 {
        match self {
            Self::End => TAG_END,
            Self::Byte(_) => TAG_BYTE,
            Self::Short(_) => TAG_SHORT,
            Self::Int(_) => TAG_INT,
            Self::Long(_) => TAG_LONG,
            Self::Float(_) => TAG_FLOAT,
            Self::Double(_) => TAG_DOUBLE,
            Self::ByteArray(_) => TAG_BYTE_ARRAY,
            Self::String(_) => TAG_STRING,
            Self::List(_) => TAG_LIST,
            Self::Compound(_) => TAG_COMPOUND,
            Self::IntArray(_) => TAG_INT_ARRAY,
            Self::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    pub(crate) fn as_numeric_tag(&self) -> Option<NumericTag> {
        match self {
            Self::Byte(v) => Some(NumericTag::Byte(ByteTag::value_of(*v))),
            Self::Short(v) => Some(NumericTag::Short(ShortTag::value_of(*v))),
            Self::Int(v) => Some(NumericTag::Int(IntTag::value_of(*v))),
            Self::Long(v) => Some(NumericTag::Long(LongTag::value_of(*v))),
            Self::Float(v) => Some(NumericTag::Float(FloatTag::value_of(*v))),
            Self::Double(v) => Some(NumericTag::Double(DoubleTag::value_of(*v))),
            _ => None,
        }
    }

    pub(crate) fn as_number(&self) -> Option<Number> {
        self.as_numeric_tag().map(|t| t.boxed())
    }

    pub(crate) fn deep_copy(&self) -> Self {
        match self {
            Self::End => Self::End,
            Self::Byte(v) => Self::Byte(*v),
            Self::Short(v) => Self::Short(*v),
            Self::Int(v) => Self::Int(*v),
            Self::Long(v) => Self::Long(*v),
            Self::Float(v) => Self::Float(*v),
            Self::Double(v) => Self::Double(*v),
            Self::ByteArray(v) => Self::ByteArray(v.clone()),
            Self::String(v) => Self::String(v.clone()),
            Self::List(list) => {
                let copied = list.borrow().iter().map(Self::deep_copy).collect();
                Self::List(Rc::new(RefCell::new(copied)))
            }
            Self::Compound(map) => {
                let copied = map
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.deep_copy()))
                    .collect();
                Self::Compound(Rc::new(RefCell::new(copied)))
            }
            Self::IntArray(v) => Self::IntArray(v.clone()),
            Self::LongArray(v) => Self::LongArray(v.clone()),
        }
    }

    pub(crate) fn from_tag(tag: &Tag) -> Self {
        match tag {
            Tag::End(_) => Self::End,
            Tag::Byte(t) => Self::Byte(t.value),
            Tag::Short(t) => Self::Short(t.value),
            Tag::Int(t) => Self::Int(t.value),
            Tag::Long(t) => Self::Long(t.value),
            Tag::Float(t) => Self::Float(t.value),
            Tag::Double(t) => Self::Double(t.value),
            Tag::ByteArray(t) => Self::ByteArray(t.data.clone()),
            Tag::String(t) => Self::String(t.value.clone()),
            Tag::List(t) => Self::List(from_list_tag(t)),
            Tag::Compound(t) => Self::Compound(from_compound_tag(t)),
            Tag::IntArray(t) => Self::IntArray(t.data.clone()),
            Tag::LongArray(t) => Self::LongArray(t.data.clone()),
        }
    }

    pub(crate) fn to_tag(&self) -> Tag {
        match self {
            Self::End => Tag::End(EndTag),
            Self::Byte(v) => Tag::Byte(ByteTag::value_of(*v)),
            Self::Short(v) => Tag::Short(ShortTag::value_of(*v)),
            Self::Int(v) => Tag::Int(IntTag::value_of(*v)),
            Self::Long(v) => Tag::Long(LongTag::value_of(*v)),
            Self::Float(v) => Tag::Float(FloatTag::value_of(*v)),
            Self::Double(v) => Tag::Double(DoubleTag::value_of(*v)),
            Self::ByteArray(v) => Tag::ByteArray(ByteArrayTag::new(v.clone())),
            Self::String(v) => Tag::String(StringTag::value_of(v.clone())),
            Self::List(list) => Tag::List(to_list_tag(list)),
            Self::Compound(map) => Tag::Compound(to_compound_tag(map)),
            Self::IntArray(v) => Tag::IntArray(IntArrayTag::new(v.clone())),
            Self::LongArray(v) => Tag::LongArray(LongArrayTag::new(v.clone())),
        }
    }
}

pub(crate) fn from_list_tag(tag: &ListTag) -> SharedNbtList {
    let items = tag.list.iter().map(NbtValue::from_tag).collect();
    Rc::new(RefCell::new(items))
}

pub(crate) fn to_list_tag(list: &SharedNbtList) -> ListTag {
    let items = list.borrow().iter().map(NbtValue::to_tag).collect();
    ListTag::with_list(items)
}

pub(crate) fn from_compound_tag(tag: &CompoundTag) -> SharedNbtCompound {
    let map = tag
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), NbtValue::from_tag(v)))
        .collect();
    Rc::new(RefCell::new(map))
}

pub(crate) fn to_compound_tag(map: &SharedNbtCompound) -> CompoundTag {
    let tags = map
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.to_tag()))
        .collect();
    CompoundTag::with_map(tags)
}

/// Port of package-private `NBTListType.getType(byte)`.
pub(crate) fn tag_id_to_object_type(id: i8) -> ObjectType {
    match id {
        TAG_END => ObjectType::None,
        TAG_BYTE => ObjectType::Byte,
        TAG_SHORT => ObjectType::Short,
        TAG_INT => ObjectType::Int,
        TAG_LONG => ObjectType::Long,
        TAG_FLOAT => ObjectType::Float,
        TAG_DOUBLE => ObjectType::Double,
        TAG_BYTE_ARRAY => ObjectType::ByteArray,
        TAG_STRING => ObjectType::String,
        TAG_LIST => ObjectType::List,
        TAG_COMPOUND => ObjectType::Map,
        TAG_INT_ARRAY => ObjectType::IntArray,
        TAG_LONG_ARRAY => ObjectType::LongArray,
        _ => panic!("Unknown type: {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dataconverter::types::Types;
    use crate::server::dataconverter::types::generic::Generic;
    use crate::server::dataconverter::types::list_type::ListType;
    use crate::server::dataconverter::types::map_type::MapType;
    use crate::server::dataconverter::types::test_support::{foundation_fixture, render_generic};
    use crate::server::dataconverter::types::type_util::TypeUtil;
    use rivet_serialization::number::Number;

    #[test]
    fn nbt_map_type_matches_paper_foundation_fixture() {
        let fixture = foundation_fixture();
        let expected = &fixture["mapTypeDefaults"];

        let mut map = NBTMapType::new();
        map.set_generic("b", Generic::Byte(1));
        map.set_generic("s", Generic::Short(2));
        map.set_generic("i", Generic::Int(3));
        map.set_generic("l", Generic::Long(4));
        map.set_generic("f", Generic::Float(5.5));
        map.set_generic("d", Generic::Double(6.5));
        map.set_generic("bool", Generic::Bool(true));
        map.set_generic("str", Generic::Str("seven".to_owned()));
        map.set_generic("map", Generic::Map(Box::new(NBTMapType::new())));
        map.set_generic("list", Generic::List(Box::new(NBTListType::new())));
        map.set_generic("bytes", Generic::Bytes(vec![8, 9]));
        map.set_generic("ints", Generic::Ints(vec![10, 11]));
        map.set_generic("longs", Generic::Longs(vec![12, 13]));

        for key in [
            "b", "s", "i", "l", "f", "d", "bool", "str", "bytes", "ints", "longs",
        ] {
            let val = map.get_generic(key).expect("present");
            assert_eq!(
                render_generic(&val),
                expected[key].as_str().unwrap(),
                "key={key}"
            );
        }
        assert!(matches!(map.get_generic("map"), Some(Generic::Map(_))));
        assert!(matches!(map.get_generic("list"), Some(Generic::List(_))));

        // Boolean stored as Byte(1) in NBT.
        assert!(matches!(map.get_generic("bool"), Some(Generic::Byte(1))));
        assert_eq!(
            map.get_boolean("bool"),
            expected["getBooleanFromByte"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_number("bool").unwrap().byte_value().to_string(),
            expected["getNumberFromByte"].as_str().unwrap()
        );
        assert_eq!(
            map.get_int("bool") as i64,
            expected["getIntCoerced"].as_i64().unwrap()
        );

        assert_eq!(
            map.has_key_of_type("i", ObjectType::Int),
            expected["hasKey_int_AS_INT"].as_bool().unwrap()
        );
        assert_eq!(
            map.has_key_of_type("i", ObjectType::Byte),
            expected["hasKey_int_AS_BYTE"].as_bool().unwrap()
        );
        assert_eq!(
            map.has_key_of_type("i", ObjectType::Number),
            expected["hasKey_int_AS_NUMBER"].as_bool().unwrap()
        );
        assert_eq!(
            map.has_key_of_type("str", ObjectType::String),
            expected["hasKey_str_AS_STRING"].as_bool().unwrap()
        );
        assert_eq!(
            map.has_key_of_type("unknown", ObjectType::Int),
            expected["hasKey_unknown"].as_bool().unwrap()
        );

        let mut int_list = NBTListType::new();
        int_list.add_int(42);
        map.set_list("intList", Box::new(int_list));
        assert_eq!(
            map.get_list("intList", ObjectType::Int).is_some(),
            expected["getList_int_as_INT_notNull"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_list("intList", ObjectType::String).is_none(),
            expected["getList_int_as_STRING_null"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_list("list", ObjectType::String).is_some(),
            expected["getList_empty_as_any_notNull"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_list("missing", ObjectType::Int).is_none(),
            expected["getList_missing_null"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_list_unchecked("intList").is_some(),
            expected["getListUnchecked_int_notNull"].as_bool().unwrap()
        );

        {
            let mut created_list = map.get_or_create_list("newList", ObjectType::Int);
            created_list.add_int(99);
        }
        assert_eq!(
            map.has_key("newList"),
            expected["getOrCreateList_created"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_list_unchecked("newList").unwrap().size() as i64,
            expected["getOrCreateList_size"].as_i64().unwrap()
        );

        {
            let mut created_map = map.get_or_create_map("newMap");
            created_map.set_int("inner", 7);
        }
        assert_eq!(
            map.has_key("newMap"),
            expected["getOrCreateMap_created"].as_bool().unwrap()
        );
        assert_eq!(
            map.get_map("newMap").unwrap().get_int("inner") as i64,
            expected["getOrCreateMap_inner"].as_i64().unwrap()
        );
    }

    #[test]
    fn nbt_list_type_matches_paper_foundation_fixture() {
        let fixture = foundation_fixture();
        let expected = &fixture["listTypeDefaults"];

        let mut list = NBTListType::new();
        list.add_int(1);
        list.set_generic(0, Generic::Int(3));
        assert_eq!(
            list.get_int(0) as i64,
            expected["setGeneric_int"].as_i64().unwrap()
        );

        let mut s_list = NBTListType::new();
        s_list.add_generic(Generic::Short(4));
        assert_eq!(
            s_list.get_short(0) as i64,
            expected["addGeneric_short"].as_i64().unwrap()
        );

        let mut str_list = NBTListType::new();
        str_list.add_generic(Generic::Str("five".to_owned()));
        assert_eq!(
            str_list.get_string(0),
            expected["addGeneric_string"].as_str().unwrap()
        );

        let mut map_list = NBTListType::new();
        map_list.add_generic(Generic::Map(Box::new(NBTMapType::new())));
        assert!(matches!(map_list.get_generic(0), Some(Generic::Map(_))));

        let mut ints_list = NBTListType::new();
        ints_list.add_generic(Generic::Ints(vec![6, 7]));
        assert_eq!(
            render_generic(&ints_list.get_generic(0).unwrap()),
            expected["addGeneric_ints"].as_str().unwrap()
        );

        let empty_oob = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut empty = NBTListType::new();
            empty.set_generic(0, Generic::Int(1));
        }))
        .is_err();
        assert_eq!(
            empty_oob,
            expected["setGeneric_empty_oob"].as_bool().unwrap()
        );
    }

    #[test]
    fn nbt_view_aliasing_and_deep_copy_independence() {
        let mut root = NBTMapType::new();
        {
            let mut sub = root.get_or_create_map("sub");
            sub.set_int("x", 10);
            let mut arr = sub.get_or_create_list("arr", ObjectType::Map);
            arr.add_map(Box::new(NBTMapType::new()));
            arr.get_map(0).set_string("name", "aliased".to_owned());
        }

        // Mutating via get_map / get_generic mutates root in place.
        root.get_map("sub").unwrap().set_int("x", 20);
        assert_eq!(root.get_map("sub").unwrap().get_int("x"), 20);
        if let Some(Generic::Map(mut m)) = root.get_generic("sub") {
            m.set_int("x", 30);
        }
        assert_eq!(root.get_map("sub").unwrap().get_int("x"), 30);

        // Deep copy does not alias original.
        let copy = root.copy();
        root.get_map("sub").unwrap().set_int("x", 999);
        assert_eq!(copy.get_map("sub").unwrap().get_int("x"), 30);
        assert_eq!(
            copy.get_map("sub")
                .unwrap()
                .get_list_unchecked("arr")
                .unwrap()
                .get_map(0)
                .get_string("name")
                .as_deref(),
            Some("aliased")
        );
    }

    #[test]
    fn nbt_numeric_coercion_and_boolean_as_byte() {
        let mut map = NBTMapType::new();
        map.set_int("big", 300);
        assert_eq!(map.get_byte("big"), 300_i32 as i8);
        assert_eq!(map.get_short("big"), 300);
        assert_eq!(map.get_long("big"), 300);
        assert_eq!(map.get_float("big"), 300.0);
        assert_eq!(map.get_double("big"), 300.0);

        map.set_boolean("t", true);
        map.set_boolean("f", false);
        assert_eq!(map.get_byte("t"), 1);
        assert_eq!(map.get_byte("f"), 0);
        assert!(map.get_boolean("t"));
        assert!(!map.get_boolean("f"));
        assert!(map.has_key_of_type("t", ObjectType::Byte));
        assert!(map.has_key_of_type("t", ObjectType::Number));
        // Non-zero byte/int reads as true.
        map.set_byte("neg", -1);
        assert!(map.get_boolean("neg"));
    }

    #[test]
    fn nbt_type_util_null_end_tag_and_round_trip() {
        assert!(Types::NBT.base_to_generic(&Tag::End(EndTag)).is_none());
        assert!(matches!(Types::NBT.generic_to_base(None), Tag::End(_)));

        let tag = Types::NBT.generic_to_base(Some(&Generic::Bool(true)));
        assert!(matches!(tag, Tag::Byte(b) if b.value == 1));

        let mut compound = CompoundTag::new();
        compound.put_int("k", 42);
        compound.put_string("s", "hello");
        let generic = Types::NBT
            .base_to_generic(&Tag::Compound(compound))
            .expect("compound");
        let round_trip = Types::NBT.generic_to_base(Some(&generic));
        let Tag::Compound(c) = round_trip else {
            panic!("expected compound");
        };
        assert_eq!(c.get_int("k"), Some(42));
        assert_eq!(c.get_string("s").map(String::as_str), Some("hello"));
    }

    #[test]
    #[should_panic]
    fn nbt_list_strict_wrong_type_panics() {
        let mut list = NBTListType::new();
        list.add_string("not_a_number".to_owned());
        let _ = list.get_number(0);
    }

    #[test]
    #[should_panic]
    fn nbt_list_or_getter_out_of_bounds_panics() {
        let list = NBTListType::new();
        let _ = list.get_int_or(0, 7);
    }

    #[test]
    fn nbt_list_or_getter_wrong_type_returns_default() {
        let mut list = NBTListType::new();
        list.add_string("hello".to_owned());
        assert_eq!(list.get_int_or(0, 77), 77);
        assert_eq!(list.get_number_or(0, Number::Int(9)), Number::Int(9));
        assert_eq!(list.get_bytes_or(0, vec![1, 2]), vec![1, 2]);
    }

    #[test]
    #[should_panic]
    fn nbt_map_shorts_unsupported() {
        let mut map = NBTMapType::new();
        map.set_shorts("s", vec![1, 2]);
    }

    #[test]
    fn nbt_negative_fractional_coercion_matches_numeric_tag() {
        let mut map = NBTMapType::new();
        map.set_float("f", -0.5);
        map.set_double("d", -0.5);
        map.set_float("f_neg", -1.2);
        map.set_double("d_neg", -1.2);

        // NumericTag floors float/double for byte/short/int; FloatTag::long_value truncates while DoubleTag::long_value floors.
        assert_eq!(map.get_byte_or("f", 0), -1);
        assert_eq!(map.get_short_or("f", 0), -1);
        assert_eq!(map.get_int_or("f", 0), -1);
        assert_eq!(map.get_long_or("f", 99), 0);
        assert!(map.get_boolean("f"));

        assert_eq!(map.get_byte_or("d", 0), -1);
        assert_eq!(map.get_short_or("d", 0), -1);
        assert_eq!(map.get_int_or("d", 0), -1);
        assert_eq!(map.get_long_or("d", 99), -1);
        assert!(map.get_boolean("d"));

        assert_eq!(map.get_int("f_neg"), -2);
        assert_eq!(map.get_long("f_neg"), -1);
        assert_eq!(map.get_int("d_neg"), -2);
        assert_eq!(map.get_long("d_neg"), -2);

        let mut list = NBTListType::new();
        list.add_float(-0.5);
        list.add_double(-0.5);
        assert_eq!(list.get_byte(0), -1);
        assert_eq!(list.get_byte_or(0, 0), -1);
        assert_eq!(list.get_short(0), -1);
        assert_eq!(list.get_short_or(0, 0), -1);
        assert_eq!(list.get_int(0), -1);
        assert_eq!(list.get_int_or(0, 0), -1);
        assert_eq!(list.get_long(0), 0);
        assert_eq!(list.get_long_or(0, 99), 0);

        assert_eq!(list.get_byte(1), -1);
        assert_eq!(list.get_byte_or(1, 0), -1);
        assert_eq!(list.get_short(1), -1);
        assert_eq!(list.get_short_or(1, 0), -1);
        assert_eq!(list.get_int(1), -1);
        assert_eq!(list.get_int_or(1, 0), -1);
        assert_eq!(list.get_long(1), -1);
        assert_eq!(list.get_long_or(1, 99), -1);
    }

    #[test]
    fn nbt_container_float_equality_matches_java_float_double_tag() {
        let mut m1 = NBTMapType::new();
        m1.set_float("nan_f", f32::NAN);
        m1.set_double("nan_d", f64::NAN);
        let mut m2 = NBTMapType::new();
        m2.set_float("nan_f", f32::NAN);
        m2.set_double("nan_d", f64::NAN);
        assert_eq!(m1, m2);

        let mut pos_zero = NBTMapType::new();
        pos_zero.set_float("z", 0.0_f32);
        let mut neg_zero = NBTMapType::new();
        neg_zero.set_float("z", -0.0_f32);
        assert_ne!(pos_zero, neg_zero);

        let mut l1 = NBTListType::new();
        l1.add_double(f64::NAN);
        l1.add_double(0.0_f64);
        let mut l2 = NBTListType::new();
        l2.add_double(f64::NAN);
        l2.add_double(0.0_f64);
        let mut l3 = NBTListType::new();
        l3.add_double(f64::NAN);
        l3.add_double(-0.0_f64);
        assert_eq!(l1, l2);
        assert_ne!(l1, l3);
    }
}
