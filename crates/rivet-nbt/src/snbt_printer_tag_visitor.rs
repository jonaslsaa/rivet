//! Port of `net.minecraft.nbt.SnbtPrinterTagVisitor` — the pretty (indented,
//! `KEY_ORDER`-sorted) SNBT printer used by `NbtUtils.structureToSnbt`.
//!
//! Differs from `StringTagVisitor` (compact): 4-space indentation, `" : "`
//! separators, `[B; 1B, 2B]` array spacing, and `KEY_ORDER`-driven key ordering
//! for structure templates. `handleEscapePretty` uses `SIMPLE_VALUE`
//! (`[A-Za-z0-9._+-]+`, digits allowed to start) which is *not* the
//! `StringTagVisitor` key pattern — keys like `123` or `true` print unquoted
//! here but quoted compactly.

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
use crate::string_tag::{StringTag, quote_and_escape};
use crate::tag::Tag;
use crate::tag_visitor::TagVisitor;

/// `SnbtPrinterTagVisitor.NAME_VALUE_SEPARATOR` — `':'`.
const NAME_VALUE_SEPARATOR: char = ':';
/// `SnbtPrinterTagVisitor.ELEMENT_SEPARATOR` — `','`.
const ELEMENT_SEPARATOR: char = ',';
/// `SnbtPrinterTagVisitor.ELEMENT_SPACING` — `" "`.
const ELEMENT_SPACING: char = ' ';
/// `SnbtPrinterTagVisitor.NEWLINE` — `"\n"`.
const NEWLINE: char = '\n';

/// `SnbtPrinterTagVisitor.SIMPLE_VALUE` — `[A-Za-z0-9._+-]+` (full match).
/// A key matching this prints unquoted.
fn simple_value_matches(input: &str) -> bool {
    !input.is_empty()
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '+' || c == '-')
}

/// `SnbtPrinterTagVisitor.KEY_ORDER` — preferred key order per path.
fn key_order(path: &str) -> Option<&'static [&'static str]> {
    match path {
        "{}" => Some(&[
            "DataVersion",
            "author",
            "size",
            "data",
            "entities",
            "palette",
            "palettes",
        ]),
        "{}.data.[].{}" => Some(&["pos", "state", "nbt"]),
        "{}.entities.[].{}" => Some(&["blockPos", "pos"]),
        _ => None,
    }
}

/// `SnbtPrinterTagVisitor.NO_INDENTATION` — paths printed on a single line.
fn no_indentation(path: &str) -> bool {
    matches!(
        path,
        "{}.size.[]" | "{}.data.[].{}" | "{}.palette.[].{}" | "{}.entities.[].{}"
    )
}

/// `Strings.repeat(string, count)` — `count` copies of `string` (`0` → `""`).
fn repeat(string: &str, count: usize) -> String {
    string.repeat(count)
}

/// `String.compareTo` order — UTF-16 code units (code units are unsigned; a
/// prefix sorts before a longer string).
fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut au = a.encode_utf16();
    let mut bu = b.encode_utf16();
    loop {
        match (au.next(), bu.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                std::cmp::Ordering::Equal => continue,
                o => return o,
            },
        }
    }
}

/// `SnbtPrinterTagVisitor.handleEscapePretty(String)` — unquoted iff it fully
/// matches `SIMPLE_VALUE`, else `StringTag.quoteAndEscape`.
fn handle_escape_pretty(input: &str) -> String {
    if simple_value_matches(input) {
        input.to_string()
    } else {
        quote_and_escape(input)
    }
}

/// `SnbtPrinterTagVisitor` — pretty SNBT builder (4-space indent by default).
pub struct SnbtPrinterTagVisitor {
    indentation: String,
    depth: usize,
    path: Vec<String>,
    result: String,
}

impl SnbtPrinterTagVisitor {
    /// `new SnbtPrinterTagVisitor()` — indentation `"    "`, depth `0`, empty
    /// path.
    pub fn new() -> Self {
        SnbtPrinterTagVisitor::new_with("    ".to_string(), 0, Vec::new())
    }

    /// `new SnbtPrinterTagVisitor(String indentation, int depth, List<String> path)`.
    pub fn new_with(indentation: String, depth: usize, path: Vec<String>) -> Self {
        SnbtPrinterTagVisitor {
            indentation,
            depth,
            path,
            result: String::new(),
        }
    }

    /// `visit(Tag)` — `tag.accept(this); return this.result`.
    pub fn visit(mut self, tag: &Tag) -> String {
        tag.accept(&mut self);
        self.result
    }
}

impl Default for SnbtPrinterTagVisitor {
    fn default() -> Self {
        SnbtPrinterTagVisitor::new()
    }
}

impl TagVisitor for SnbtPrinterTagVisitor {
    fn visit_string(&mut self, tag: &StringTag) {
        self.result = quote_and_escape(&tag.value);
    }

    fn visit_byte(&mut self, tag: &ByteTag) {
        self.result = format!("{}b", tag.value);
    }

    fn visit_short(&mut self, tag: &ShortTag) {
        self.result = format!("{}s", tag.value);
    }

    fn visit_int(&mut self, tag: &IntTag) {
        self.result = format!("{}", tag.value);
    }

    fn visit_long(&mut self, tag: &LongTag) {
        self.result = format!("{}L", tag.value);
    }

    fn visit_float(&mut self, tag: &FloatTag) {
        // Java `tag.value() + "f"` = `Float.toString(value) + "f"`.
        self.result = format!(
            "{}f",
            crate::float_to_string::java_float_to_string(tag.value)
        );
    }

    fn visit_double(&mut self, tag: &DoubleTag) {
        // Java `tag.value() + "d"` = `Double.toString(value) + "d"`.
        self.result = format!(
            "{}d",
            crate::float_to_string::java_double_to_string(tag.value)
        );
    }

    fn visit_byte_array(&mut self, tag: &ByteArrayTag) {
        let mut builder = String::from("[B;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            builder.push(ELEMENT_SPACING);
            builder.push_str(&format!("{v}B"));
            if i != data.len() - 1 {
                builder.push(ELEMENT_SEPARATOR);
            }
        }
        builder.push(']');
        self.result = builder;
    }

    fn visit_int_array(&mut self, tag: &IntArrayTag) {
        let mut builder = String::from("[I;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            builder.push(ELEMENT_SPACING);
            builder.push_str(&format!("{v}"));
            if i != data.len() - 1 {
                builder.push(ELEMENT_SEPARATOR);
            }
        }
        builder.push(']');
        self.result = builder;
    }

    fn visit_long_array(&mut self, tag: &LongArrayTag) {
        let mut builder = String::from("[L;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            builder.push(ELEMENT_SPACING);
            builder.push_str(&format!("{v}L"));
            if i != data.len() - 1 {
                builder.push(ELEMENT_SEPARATOR);
            }
        }
        builder.push(']');
        self.result = builder;
    }

    fn visit_list(&mut self, tag: &ListTag) {
        if tag.is_empty() {
            self.result = "[]".to_string();
        } else {
            let mut builder = String::from("[");
            self.push_path("[]".to_string());
            // Clone the indentation so the borrow of `self` ends before the
            // loop mutates `self` (see `visit_compound`).
            let indentation = if no_indentation(&self.path_string()) {
                String::new()
            } else {
                self.indentation.clone()
            };
            if !indentation.is_empty() {
                builder.push(NEWLINE);
            }
            for i in 0..tag.size() {
                builder.push_str(&repeat(&indentation, self.depth + 1));
                builder.push_str(
                    &SnbtPrinterTagVisitor::new_with(
                        indentation.clone(),
                        self.depth + 1,
                        self.path.clone(),
                    )
                    .visit(tag.get(i)),
                );
                if i != tag.size() - 1 {
                    builder.push(ELEMENT_SEPARATOR);
                    if indentation.is_empty() {
                        builder.push(ELEMENT_SPACING);
                    } else {
                        builder.push(NEWLINE);
                    }
                }
            }
            if !indentation.is_empty() {
                builder.push(NEWLINE);
                builder.push_str(&repeat(&indentation, self.depth));
            }
            builder.push(']');
            self.result = builder;
            self.pop_path();
        }
    }

    fn visit_compound(&mut self, tag: &CompoundTag) {
        if tag.is_empty() {
            self.result = "{}".to_string();
        } else {
            let mut builder = String::from("{");
            self.push_path("{}".to_string());
            // Clone the indentation so the borrow of `self` ends before the
            // loop mutates `self` (Java copies the reference freely).
            let indentation = if no_indentation(&self.path_string()) {
                String::new()
            } else {
                self.indentation.clone()
            };
            if !indentation.is_empty() {
                builder.push(NEWLINE);
            }
            let keys = self.get_keys(tag);
            let key_count = keys.len();
            for (i, key) in keys.iter().enumerate() {
                self.push_path(key.clone());
                builder.push_str(&repeat(&indentation, self.depth + 1));
                builder.push_str(&handle_escape_pretty(key));
                builder.push(NAME_VALUE_SEPARATOR);
                builder.push(ELEMENT_SPACING);
                builder.push_str(
                    &SnbtPrinterTagVisitor::new_with(
                        indentation.clone(),
                        self.depth + 1,
                        self.path.clone(),
                    )
                    .visit(tag.get(key).expect("key came from keySet")),
                );
                self.pop_path();
                if i != key_count - 1 {
                    builder.push(ELEMENT_SEPARATOR);
                    if indentation.is_empty() {
                        builder.push(ELEMENT_SPACING);
                    } else {
                        builder.push(NEWLINE);
                    }
                }
            }
            if !indentation.is_empty() {
                builder.push(NEWLINE);
                builder.push_str(&repeat(&indentation, self.depth));
            }
            builder.push('}');
            self.result = builder;
            self.pop_path();
        }
    }

    fn visit_end(&mut self, _tag: &EndTag) {}
}

impl SnbtPrinterTagVisitor {
    fn pop_path(&mut self) {
        self.path.pop();
    }

    fn push_path(&mut self, e: String) {
        self.path.push(e);
    }

    /// `getKeys(CompoundTag)` — `KEY_ORDER`-prefixed, remainder sorted
    /// (`String.compareTo` / UTF-16 order). Keys iterate from `entry_set` so
    /// each entry carries its value: Java's `visitCompound` reads
    /// `tag.get(key)` after `getKeys`, and a CompoundTag key is never null, but
    /// this avoids the `expect("key came from keySet")` panic if an entry's key
    /// were ever dropped between the key set and the value lookup.
    fn get_keys(&self, tag: &CompoundTag) -> Vec<String> {
        // Java: `Set<String> keys = Sets.newHashSet(tag.keySet())`. Iteration
        // order is irrelevant: matched keys are removed, the rest is sorted.
        let mut keys: Vec<String> = tag.entry_set().map(|(k, _)| k.clone()).collect();
        let mut strings: Vec<String> = Vec::new();
        if let Some(order) = key_order(&self.path_string()) {
            for key in order {
                if let Some(pos) = keys.iter().position(|k| k == key) {
                    keys.remove(pos);
                    strings.push(key.to_string());
                }
            }
            if !keys.is_empty() {
                keys.sort_by(|a, b| utf16_cmp(a, b));
                strings.extend(keys);
            }
        } else {
            keys.sort_by(|a, b| utf16_cmp(a, b));
            strings = keys;
        }
        strings
    }

    /// `pathString()` — `String.join(".", path)`.
    fn path_string(&self) -> String {
        self.path.join(".")
    }
}

/// Convenience free function matching the existing `nbt_utils.rs` call site
/// (`crate::snbt_printer_tag_visitor::visit(&tag)`). Java equivalent:
/// `new SnbtPrinterTagVisitor().visit(tag)`.
pub fn visit(tag: &Tag) -> String {
    SnbtPrinterTagVisitor::new().visit(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: &[(&str, Tag)]) -> CompoundTag {
        let mut c = CompoundTag::new();
        for (k, v) in entries {
            c.put(k.to_string(), v.clone());
        }
        c
    }

    #[test]
    fn simple_value_matches_java_pattern() {
        // SIMPLE_VALUE = [A-Za-z0-9._+-]+ — digits allowed to start.
        assert!(simple_value_matches("abc"));
        assert!(simple_value_matches("DataVersion"));
        assert!(simple_value_matches("123"));
        assert!(simple_value_matches("a.b"));
        assert!(simple_value_matches("true"));
        assert!(simple_value_matches("-x"));
        assert!(!simple_value_matches(""));
        assert!(!simple_value_matches("a b"));
        assert!(!simple_value_matches("a:b"));
        assert!(!simple_value_matches("a{b"));
    }

    #[test]
    fn key_order_and_no_indentation_sets() {
        assert_eq!(
            key_order("{}"),
            Some(
                &[
                    "DataVersion",
                    "author",
                    "size",
                    "data",
                    "entities",
                    "palette",
                    "palettes"
                ][..]
            )
        );
        assert_eq!(
            key_order("{}.data.[].{}"),
            Some(&["pos", "state", "nbt"][..])
        );
        assert_eq!(
            key_order("{}.entities.[].{}"),
            Some(&["blockPos", "pos"][..])
        );
        assert_eq!(key_order("nope"), None);
        assert!(no_indentation("{}.size.[]"));
        assert!(no_indentation("{}.data.[].{}"));
        assert!(no_indentation("{}.palette.[].{}"));
        assert!(no_indentation("{}.entities.[].{}"));
        assert!(!no_indentation("{}"));
        assert!(!no_indentation("[]"));
    }

    #[test]
    fn primitive_round_trip_pretty() {
        assert_eq!(visit(&Tag::Byte(ByteTag::new(5))), "5b");
        assert_eq!(visit(&Tag::Short(ShortTag::new(-3))), "-3s");
        assert_eq!(visit(&Tag::Int(IntTag::new(1234))), "1234");
        assert_eq!(visit(&Tag::Long(LongTag::new(99))), "99L");
        assert_eq!(visit(&Tag::Float(FloatTag::new(1.5))), "1.5f");
        assert_eq!(visit(&Tag::Float(FloatTag::new(1.0))), "1.0f");
        assert_eq!(visit(&Tag::Double(DoubleTag::new(2.25))), "2.25d");
        assert_eq!(
            visit(&Tag::String(StringTag::value_of("hi".to_string()))),
            "\"hi\""
        );
        // SIMPLE_VALUE keys print unquoted, including true/false and digits.
        assert_eq!(
            visit(&Tag::String(StringTag::value_of("a b".to_string()))),
            "\"a b\""
        );
    }

    #[test]
    fn arrays_use_space_separated_pretty_form() {
        // Java: "[B;" + (" " + v + "B") joined by ",". The leading space is
        // prepended to *every* element, so a comma is followed by the next
        // element's leading space: "[B; 1B, -1B, 2B]".
        assert_eq!(
            visit(&Tag::ByteArray(ByteArrayTag::new(vec![1, -1, 2]))),
            "[B; 1B, -1B, 2B]"
        );
        assert_eq!(visit(&Tag::ByteArray(ByteArrayTag::new(vec![]))), "[B;]");
        assert_eq!(
            visit(&Tag::IntArray(IntArrayTag::new(vec![1, 2]))),
            "[I; 1, 2]"
        );
        assert_eq!(
            visit(&Tag::LongArray(LongArrayTag::new(vec![1, 2]))),
            "[L; 1L, 2L]"
        );
    }

    #[test]
    fn empty_containers() {
        assert_eq!(visit(&Tag::Compound(CompoundTag::new())), "{}");
        assert_eq!(visit(&Tag::List(ListTag::new())), "[]");
    }

    #[test]
    fn non_empty_list_indents() {
        let mut l = ListTag::new();
        l.add(Tag::Int(IntTag::new(1)));
        l.add(Tag::Int(IntTag::new(2)));
        assert_eq!(visit(&Tag::List(l)), "[\n    1,\n    2\n]");
    }

    #[test]
    fn compound_indents_and_sorts_keys() {
        let c = compound(&[
            (
                "name",
                Tag::String(StringTag::value_of("Rivet".to_string())),
            ),
            ("x", Tag::Int(IntTag::new(42))),
        ]);
        // Keys sort (UTF-16): "name" < "x".
        assert_eq!(
            visit(&Tag::Compound(c)),
            "{\n    name: \"Rivet\",\n    x: 42\n}"
        );
    }

    #[test]
    fn key_order_applies_at_root_path() {
        let c = compound(&[
            ("palettes", Tag::List(ListTag::new())),
            ("data", Tag::String(StringTag::value_of("x".to_string()))),
            ("author", Tag::String(StringTag::value_of("y".to_string()))),
            ("zzz", Tag::Int(IntTag::new(1))),
        ]);
        // KEY_ORDER at "{}" puts DataVersion, author, size, data, entities,
        // palette, palettes first; "zzz" is sorted after.
        assert_eq!(
            visit(&Tag::Compound(c)),
            "{\n    author: \"y\",\n    data: \"x\",\n    palettes: [],\n    zzz: 1\n}"
        );
    }

    #[test]
    fn nested_compound_path_orders_data_entries() {
        // "{}.data.[].{}" path → pos, state, nbt.
        let mut data = CompoundTag::new();
        data.put_int("pos", 1);
        data.put_string("state", "s");
        data.put_string("nbt", "n");
        let mut inner = ListTag::new();
        inner.add(Tag::Compound(data));
        let mut c = CompoundTag::new();
        c.put("data".to_string(), Tag::List(inner));
        let out = visit(&Tag::Compound(c));
        assert!(out.contains("pos: 1, state: \"s\", nbt: \"n\""));
    }

    #[test]
    fn key_order_applies_to_entities_blockpos() {
        // "{}.entities.[].{}" path → blockPos, pos.
        let mut e = CompoundTag::new();
        e.put_string("pos", "p");
        e.put_string("blockPos", "bp");
        let mut list = ListTag::new();
        list.add(Tag::Compound(e));
        let mut c = CompoundTag::new();
        c.put("entities".to_string(), Tag::List(list));
        let out = visit(&Tag::Compound(c));
        assert!(out.contains("blockPos: \"bp\", pos: \"p\""));
    }
}
