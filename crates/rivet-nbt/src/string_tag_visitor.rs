//! Port of `net.minecraft.nbt.StringTagVisitor` (SNBT string builder).
//!
//! Owned by unit mc.nbt.snbt; implemented here because `Tag`'s `toString` and
//! the compound/list visitors depend on it.

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
use crate::string_tag::{StringTag, quote_and_escape, quote_and_escape_into};
use crate::tag::Tag;
use crate::tag_visitor::TagVisitor;

/// `StringTagVisitor` — builds the SNBT representation.
#[derive(Debug, Default)]
pub struct StringTagVisitor {
    builder: String,
}

impl StringTagVisitor {
    pub fn new() -> Self {
        StringTagVisitor {
            builder: String::new(),
        }
    }

    /// `StringTagVisitor.build()`.
    pub fn build(&self) -> &str {
        &self.builder
    }

    /// Convenience: `new StringTagVisitor()` + `tag.accept(this)` + `build()`.
    pub fn to_string(tag: &Tag) -> String {
        let mut visitor = StringTagVisitor::new();
        tag.accept(&mut visitor);
        visitor.builder
    }
}

impl TagVisitor for StringTagVisitor {
    fn visit_string(&mut self, tag: &StringTag) {
        self.builder.push_str(&quote_and_escape(&tag.value));
    }

    fn visit_byte(&mut self, tag: &ByteTag) {
        self.builder.push_str(&format!("{}b", tag.value));
    }

    fn visit_short(&mut self, tag: &ShortTag) {
        self.builder.push_str(&format!("{}s", tag.value));
    }

    fn visit_int(&mut self, tag: &IntTag) {
        self.builder.push_str(&format!("{}", tag.value));
    }

    fn visit_long(&mut self, tag: &LongTag) {
        self.builder.push_str(&format!("{}L", tag.value));
    }

    fn visit_float(&mut self, tag: &FloatTag) {
        // Java `StringBuilder.append(float)` = `Float.toString`.
        self.builder
            .push_str(&crate::float_to_string::java_float_to_string(tag.value));
        self.builder.push('f');
    }

    fn visit_double(&mut self, tag: &DoubleTag) {
        // Java `StringBuilder.append(double)` = `Double.toString`.
        self.builder
            .push_str(&crate::float_to_string::java_double_to_string(tag.value));
        self.builder.push('d');
    }

    fn visit_byte_array(&mut self, tag: &ByteArrayTag) {
        self.builder.push_str("[B;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            if i != 0 {
                self.builder.push(',');
            }
            self.builder.push_str(&format!("{v}B"));
        }
        self.builder.push(']');
    }

    fn visit_int_array(&mut self, tag: &IntArrayTag) {
        self.builder.push_str("[I;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            if i != 0 {
                self.builder.push(',');
            }
            self.builder.push_str(&format!("{v}"));
        }
        self.builder.push(']');
    }

    fn visit_long_array(&mut self, tag: &LongArrayTag) {
        self.builder.push_str("[L;");
        let data = &tag.data;
        for (i, v) in data.iter().enumerate() {
            if i != 0 {
                self.builder.push(',');
            }
            self.builder.push_str(&format!("{v}L"));
        }
        self.builder.push(']');
    }

    fn visit_list(&mut self, tag: &ListTag) {
        self.builder.push('[');
        for (i, child) in tag.iter().enumerate() {
            if i != 0 {
                self.builder.push(',');
            }
            child.accept(self);
        }
        self.builder.push(']');
    }

    fn visit_compound(&mut self, tag: &CompoundTag) {
        self.builder.push('{');
        let mut entries: Vec<(&String, &Tag)> = tag.entry_set().collect();
        // Java `Entry.comparingByKey()` = `String.compareTo` = UTF-16 code-unit
        // lexicographic order (NOT UTF-8/codepoint order).
        entries.sort_by(|a, b| utf16_cmp(a.0, b.0));
        for (i, (key, value)) in entries.iter().enumerate() {
            if i != 0 {
                self.builder.push(',');
            }
            self.handle_key_escape(key);
            self.builder.push(':');
            value.accept(self);
        }
        self.builder.push('}');
    }

    fn visit_end(&mut self, _tag: &EndTag) {
        self.builder.push_str("END");
    }
}

impl StringTagVisitor {
    /// `StringTagVisitor.handleKeyEscape`.
    fn handle_key_escape(&mut self, input: &str) {
        if !input.eq_ignore_ascii_case("true")
            && !input.eq_ignore_ascii_case("false")
            && unquoted_key_matches(input)
        {
            self.builder.push_str(input);
        } else {
            quote_and_escape_into(input, &mut self.builder);
        }
    }
}

/// `String.compareTo` order — compare UTF-16 code units (code units are
/// unsigned; a prefix sorts before a longer string).
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

/// `UNQUOTED_KEY_MATCH` = `[A-Za-z._]+[A-Za-z0-9._+-]*`, anchored full match.
fn unquoted_key_matches(input: &str) -> bool {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let mut i = 0;
    // First class `[A-Za-z._]+` — at least one.
    let mut first = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '.' || c == '_' {
            first += 1;
            i += 1;
        } else {
            break;
        }
    }
    if first == 0 {
        return false;
    }
    // Second class `[A-Za-z0-9._+-]*`.
    for c in &chars[i..] {
        if c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '+' || *c == '-' {
            continue;
        }
        return false;
    }
    true
}
