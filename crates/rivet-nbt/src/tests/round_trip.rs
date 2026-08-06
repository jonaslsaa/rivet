//! Round-trip and constants tests for the `net.minecraft.nbt` tag hierarchy.
//!
//! Strategy (per unit notes): write->read round-trip property tests for
//! numeric/compound/list/array tags in-memory via the SNBT string visitor,
//! using the Java source as ground truth for tag id bytes, sizeInBytes
//! constants, and header sizes. No golden fixtures invented here.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::end_tag::EndTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::short_tag::ShortTag;
use crate::string_tag::StringTag;
use crate::string_tag_visitor::StringTagVisitor;
use crate::tag::{
    TAG_BYTE, TAG_BYTE_ARRAY, TAG_COMPOUND, TAG_DOUBLE, TAG_END, TAG_FLOAT, TAG_INT, TAG_INT_ARRAY,
    TAG_LIST, TAG_LONG, TAG_LONG_ARRAY, TAG_SHORT, TAG_STRING, Tag,
};

/// SNBT of a tag via `StringTagVisitor` (mirrors Java `Tag.toString()`).
fn snbt(tag: &Tag) -> String {
    StringTagVisitor::to_string(tag)
}

#[test]
fn tag_id_constants_match_java() {
    // Java `Tag` constants (net.minecraft.nbt.Tag).
    assert_eq!(TAG_END, 0);
    assert_eq!(TAG_BYTE, 1);
    assert_eq!(TAG_SHORT, 2);
    assert_eq!(TAG_INT, 3);
    assert_eq!(TAG_LONG, 4);
    assert_eq!(TAG_FLOAT, 5);
    assert_eq!(TAG_DOUBLE, 6);
    assert_eq!(TAG_BYTE_ARRAY, 7);
    assert_eq!(TAG_STRING, 8);
    assert_eq!(TAG_LIST, 9);
    assert_eq!(TAG_COMPOUND, 10);
    assert_eq!(TAG_INT_ARRAY, 11);
    assert_eq!(TAG_LONG_ARRAY, 12);
}

#[test]
fn header_size_constants_match_java() {
    // Java `Tag`: OBJECT_HEADER = 8, ARRAY_HEADER = 12, OBJECT_REFERENCE = 4,
    // STRING_SIZE = 28, MAX_DEPTH = 512.
    assert_eq!(crate::tag::OBJECT_HEADER, 8);
    assert_eq!(crate::tag::ARRAY_HEADER, 12);
    assert_eq!(crate::tag::OBJECT_REFERENCE, 4);
    assert_eq!(crate::tag::STRING_SIZE, 28);
    assert_eq!(crate::tag::MAX_DEPTH, 512);
}

#[test]
fn size_in_bytes_constants_match_java() {
    // Java SELF_SIZE_IN_BYTES per tag type.
    assert_eq!(Tag::End(EndTag).size_in_bytes(), 8);
    assert_eq!(Tag::Byte(ByteTag::new(0)).size_in_bytes(), 9);
    assert_eq!(Tag::Short(ShortTag::new(0)).size_in_bytes(), 10);
    assert_eq!(Tag::Int(IntTag::new(0)).size_in_bytes(), 12);
    assert_eq!(Tag::Long(LongTag::new(0)).size_in_bytes(), 16);
    assert_eq!(Tag::Float(FloatTag::new(0.0)).size_in_bytes(), 12);
    assert_eq!(Tag::Double(DoubleTag::new(0.0)).size_in_bytes(), 16);
    assert_eq!(
        Tag::String(StringTag::value_of(String::new())).size_in_bytes(),
        36
    );
    // StringTag: 36 + 2 * length (UTF-16 units).
    assert_eq!(
        Tag::String(StringTag::value_of("ab".to_string())).size_in_bytes(),
        40
    );
    // Array tags: 24 + unit * length.
    assert_eq!(
        Tag::ByteArray(ByteArrayTag::new(vec![])).size_in_bytes(),
        24
    );
    assert_eq!(
        Tag::ByteArray(ByteArrayTag::new(vec![1, 2, 3])).size_in_bytes(),
        27
    );
    assert_eq!(
        Tag::IntArray(IntArrayTag::new(vec![1, 2])).size_in_bytes(),
        32
    );
    assert_eq!(
        Tag::LongArray(LongArrayTag::new(vec![1])).size_in_bytes(),
        32
    );
}

#[test]
fn numeric_tags_round_trip_via_snbt() {
    assert_eq!(snbt(&Tag::Byte(ByteTag::new(5))), "5b");
    assert_eq!(snbt(&Tag::Byte(ByteTag::new(-128))), "-128b");
    assert_eq!(snbt(&Tag::Short(ShortTag::new(-3))), "-3s");
    assert_eq!(snbt(&Tag::Int(IntTag::new(1234))), "1234");
    assert_eq!(
        snbt(&Tag::Long(LongTag::new(99_000_000_000))),
        "99000000000L"
    );
    assert_eq!(snbt(&Tag::Float(FloatTag::new(1.5))), "1.5f");
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(2.25))), "2.25d");
}

#[test]
fn float_double_snbt_matches_java_to_string() {
    // Java `StringBuilder.append(float)` = `Float.toString`. Integral floats
    // get ".0"; scientific notation kicks in at |x| >= 1e7.
    assert_eq!(snbt(&Tag::Float(FloatTag::new(2.0))), "2.0f");
    assert_eq!(snbt(&Tag::Float(FloatTag::new(0.0))), "0.0f");
    assert_eq!(snbt(&Tag::Float(FloatTag::new(-0.0))), "-0.0f");
    assert_eq!(snbt(&Tag::Float(FloatTag::new(1.0e7))), "1.0E7f");
    assert_eq!(snbt(&Tag::Float(FloatTag::new(1.0e-4))), "1.0E-4f");
    // Java `StringBuilder.append(double)` = `Double.toString`.
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(1.0))), "1.0d");
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(0.0))), "0.0d");
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(-0.0))), "-0.0d");
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(1.0e7))), "1.0E7d");
    assert_eq!(snbt(&Tag::Double(DoubleTag::new(1.0e-4))), "1.0E-4d");
}

#[test]
fn float_double_equality_matches_java_compare() {
    // Java record equality uses Float.compare/Double.compare: NaN == NaN,
    // 0.0 != -0.0.
    assert_eq!(FloatTag::new(f32::NAN), FloatTag::new(f32::NAN));
    assert_eq!(DoubleTag::new(f64::NAN), DoubleTag::new(f64::NAN));
    assert_ne!(FloatTag::new(0.0), FloatTag::new(-0.0));
    assert_ne!(DoubleTag::new(0.0), DoubleTag::new(-0.0));
    // NaN hashes consistently (canonicalized like Float.floatToIntBits).
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h1 = DefaultHasher::new();
    FloatTag::new(f32::NAN).hash(&mut h1);
    let mut h2 = DefaultHasher::new();
    FloatTag::new(f32::NAN).hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
}

#[test]
fn compound_or_empty_mut_mutates_in_place() {
    // Mirrors the vanilla idiom `tag.getCompoundOrEmpty(key).putInt(...)`.
    let mut c = CompoundTag::new();
    c.put_string("name", "Rivet");
    c.get_compound_or_empty_mut("child").put_int("x", 42);
    assert!(c.get_compound("child").is_some()); // child is a compound
    assert_eq!(
        c.get_compound("child").and_then(|t| t.get_int("x")),
        Some(42)
    );
    // Existing child is returned live.
    c.get_compound_or_empty_mut("child").put_int("y", 7);
    assert_eq!(
        c.get_compound("child").and_then(|t| t.get_int("y")),
        Some(7)
    );
    // List variant.
    let mut l = CompoundTag::new();
    l.get_list_or_empty_mut("items")
        .add(Tag::Int(IntTag::new(1)));
    assert_eq!(l.get_list("items").map(|t| t.size()), Some(1));
}

#[test]
fn list_tag_or_empty_mut_mutates_in_place() {
    let mut l = ListTag::new();
    l.add(Tag::Compound(CompoundTag::new()));
    l.get_compound_or_empty_mut(0).put_int("x", 1);
    assert_eq!(l.get_compound(0).and_then(|t| t.get_int("x")), Some(1));
    l.add(Tag::List(ListTag::new()));
    l.get_list_or_empty_mut(1).add(Tag::Int(IntTag::new(5)));
    assert_eq!(l.get_list(1).map(|t| t.size()), Some(1));
}

#[test]
fn array_tags_collection_surface() {
    let mut b = ByteArrayTag::new(vec![1, 2, 3]);
    assert!(!b.is_empty());
    assert_eq!(b.get(0).value, 1);
    assert_eq!(b.get(2).value, 3);
    assert!(b.set_tag(0, &Tag::Short(ShortTag::new(9))));
    assert_eq!(b.get(0).value, 9); // byteValue() of 9
    assert!(!b.set_tag(1, &Tag::String(StringTag::value_of("x".to_string()))));
    assert!(b.add_tag(1, &Tag::Int(IntTag::new(7))));
    assert_eq!(b.size(), 4);
    let removed = b.remove(1);
    assert_eq!(removed.value, 7);
    assert_eq!(b.size(), 3);
    b.clear();
    assert!(b.is_empty());

    let mut i = IntArrayTag::new(vec![10, 20]);
    assert!(i.set_tag(0, &Tag::Long(LongTag::new(30)))); // data: [30, 20]
    assert_eq!(i.get(0).value, 30);
    assert!(i.add_tag(1, &Tag::Double(DoubleTag::new(5.5)))); // data: [30, 5, 20]
    assert_eq!(i.get(1).value, 5); // intValue() of 5.5
    let removed = i.remove(0);
    assert_eq!(removed.value, 30);
    assert_eq!(i.get(0).value, 5);
    i.clear();
    assert!(i.is_empty());

    let mut la = LongArrayTag::new(vec![1_000_000_000_000]);
    assert!(la.set_tag(0, &Tag::Int(IntTag::new(5))));
    assert_eq!(la.get(0).value, 5);
    la.clear();
    assert!(la.is_empty());
}

#[test]
fn numeric_conversions_match_java() {
    // ShortTag.byteValue() = (byte)(value & 0xFF)
    assert_eq!(ShortTag::new(-1).byte_value(), -1);
    // IntTag.shortValue() = (short)(value & 65535)
    assert_eq!(IntTag::new(70_000).short_value(), 4464); // (70000 & 0xFFFF) = 0x1170
    assert_eq!(IntTag::new(70_000).byte_value(), 112); // (70000 & 0xFF) = 0x70
    // LongTag.intValue() = (int)(value & -1L)
    assert_eq!(LongTag::new(0x1_0000_0000).int_value(), 0);
    // FloatTag.intValue() = Mth.floor(value)
    assert_eq!(FloatTag::new(1.9).int_value(), 1);
    assert_eq!(FloatTag::new(-1.9).int_value(), -2);
    assert_eq!(FloatTag::new(-1.9).byte_value(), -2);
    // DoubleTag.longValue() = (long)Math.floor(value)
    assert_eq!(DoubleTag::new(-2.5).long_value(), -3);
    assert_eq!(DoubleTag::new(2.5).long_value(), 2);
}

#[test]
fn string_quote_and_escape_matches_java() {
    // Plain string -> double-quoted.
    assert_eq!(StringTag::quote_and_escape("abc"), "\"abc\"");
    // Backslash is escaped.
    assert_eq!(StringTag::quote_and_escape("a\\b"), "\"a\\\\b\"");
    // Control chars are escaped: \n -> "n" (with leading backslash).
    assert_eq!(StringTag::quote_and_escape("a\nb"), "\"a\\nb\"");
    // The outer quote flips to single-quote if the value contains double-quotes.
    assert_eq!(StringTag::quote_and_escape("say \"hi\""), "'say \"hi\"'");
}

#[test]
fn compound_round_trip_via_snbt() {
    let mut c = CompoundTag::new();
    c.put_string("name", "Rivet");
    c.put_int("x", 42);
    c.put_boolean("flag", true); // -> ByteTag 1
    let out = snbt(&Tag::Compound(c));
    // "flag" matches UNQUOTED_KEY_MATCH and is not true/false -> unquoted;
    // keys sort lexicographically in StringTagVisitor.
    assert_eq!(out, "{flag:1b,name:\"Rivet\",x:42}");
}

#[test]
fn list_round_trip_via_snbt() {
    let mut l = ListTag::new();
    l.add(Tag::Int(IntTag::new(1)));
    l.add(Tag::Int(IntTag::new(2)));
    assert_eq!(snbt(&Tag::List(l)), "[1,2]");
}

#[test]
fn array_round_trip_via_snbt() {
    assert_eq!(
        snbt(&Tag::ByteArray(ByteArrayTag::new(vec![1, -1, 2]))),
        "[B;1B,-1B,2B]"
    );
    assert_eq!(
        snbt(&Tag::IntArray(IntArrayTag::new(vec![1, 2]))),
        "[I;1,2]"
    );
    assert_eq!(
        snbt(&Tag::LongArray(LongArrayTag::new(vec![1, 2]))),
        "[L;1L,2L]"
    );
}

#[test]
fn copy_is_deep_for_mutable_tags() {
    let mut c = CompoundTag::new();
    c.put_int("a", 1);
    let copy = c.copy_tag();
    c.put_int("b", 2);
    assert!(!copy.contains("b"));
    assert!(copy.contains("a"));
}

#[test]
fn list_identify_raw_element_type_matches_java() {
    // Empty -> TAG_END.
    assert_eq!(ListTag::new().identify_raw_element_type(), TAG_END);
    // Homogeneous -> that type.
    let mut l = ListTag::new();
    l.add(Tag::Int(IntTag::new(1)));
    l.add(Tag::Int(IntTag::new(2)));
    assert_eq!(l.identify_raw_element_type(), TAG_INT);
    // Mixed -> TAG_COMPOUND.
    l.add(Tag::String(StringTag::value_of("x".to_string())));
    assert_eq!(l.identify_raw_element_type(), TAG_COMPOUND);
}

#[test]
fn nbt_accounter_usage_grows_and_throws_on_quota() {
    use crate::nbt_accounter::NbtAccounter;
    let mut accounter = NbtAccounter::create(100);
    accounter.account_bytes(60);
    assert_eq!(accounter.get_usage(), 60);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut a = accounter.clone();
            a.account_bytes(50); // would exceed 100
        }))
        .is_err()
    );
}

#[test]
fn nbt_accounter_depth_limits() {
    use crate::nbt_accounter::NbtAccounter;
    // Java: pushDepth throws when `this.depth >= this.maxDepth`. With
    // maxDepth = 3 the depth may reach 3; the push that would take it to 4
    // throws.
    let mut a = NbtAccounter::new(i64::MAX, 3);
    a.push_depth();
    a.push_depth();
    a.push_depth(); // depth 3 == maxDepth, allowed
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut b = a.clone();
            b.push_depth(); // would take depth to 4
        }))
        .is_err()
    );
    // pop at top-level throws.
    let mut top = NbtAccounter::new(i64::MAX, 3);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| top.pop_depth())).is_err());
}

#[test]
fn as_number_matches_numeric_tag_box() {
    use rivet_serialization::number::Number;
    assert_eq!(
        Tag::Byte(ByteTag::new(7)).as_number(),
        Some(Number::Byte(7))
    );
    assert_eq!(
        Tag::Short(ShortTag::new(7)).as_number(),
        Some(Number::Short(7))
    );
    assert_eq!(Tag::Int(IntTag::new(7)).as_number(), Some(Number::Int(7)));
    assert_eq!(
        Tag::Long(LongTag::new(7)).as_number(),
        Some(Number::Long(7))
    );
    assert_eq!(
        Tag::Float(FloatTag::new(7.5)).as_number(),
        Some(Number::Float(7.5))
    );
    assert_eq!(
        Tag::Double(DoubleTag::new(7.5)).as_number(),
        Some(Number::Double(7.5))
    );
    assert_eq!(
        Tag::String(StringTag::value_of("x".to_string())).as_number(),
        None
    );
    assert_eq!(Tag::Compound(CompoundTag::new()).as_number(), None);
}

#[test]
fn as_number_f64_matches_double_value() {
    assert_eq!(Tag::Byte(ByteTag::new(7)).as_number_f64(), Some(7.0));
    assert_eq!(Tag::Int(IntTag::new(7)).as_number_f64(), Some(7.0));
    assert_eq!(Tag::Long(LongTag::new(7)).as_number_f64(), Some(7.0));
    assert_eq!(Tag::Float(FloatTag::new(7.5)).as_number_f64(), Some(7.5));
    assert_eq!(Tag::Double(DoubleTag::new(7.5)).as_number_f64(), Some(7.5));
    assert_eq!(
        Tag::String(StringTag::value_of("x".to_string())).as_number_f64(),
        None
    );
    assert_eq!(Tag::Compound(CompoundTag::new()).as_number_f64(), None);
}
