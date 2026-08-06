//! Port of `net.minecraft.nbt.TextComponentTagVisitor` (see working/Paper).
//! Owned by manifest unit mc.nbt.text.
//!
//! Builds a `net.minecraft.network.chat` text component (and, via
//! `Component.getString()`, the pretty SNBT string) for an NBT tag, optionally
//! with RichStyling color highlights. Mirrors the Java class method-for-method.
//!
//! Stub notes (replaced by later units):
//! - `String.valueOf(float/double)` parity is provided by the shared
//!   `float_to_string` module (`java_float_to_string` / `java_double_to_string`,
//!   also used by `StringTagVisitor`): shortest round-tripping digits via `ryu`
//!   (the JDK 19+ `FloatingDecimal` algorithm) with Java's plain-vs-scientific
//!   formatting rule and the subnormal tie-break overrides; verified against
//!   JDK 25 output.
//! - `sortKeys` defaults to `false`: Java reads `LOGGER.isDebugEnabled()`
//!   (SLF4J default is INFO, so `false`). The logger is not ported yet.
//! - Compound field order is non-deterministic vs Paper: with `sortKeys=false`
//!   (the default) keys render in `std::collections::HashMap` iteration order,
//!   which differs from Java's fastutil `Object2ObjectOpenHashMap` hash order
//!   and is randomized per-process (SipHash). This is an accepted drift
//!   (tracked in the manifest; documented in `compound_tag.rs`). Sorting would
//!   be a behavioral change vs Java, which does not sort at INFO level.
//!   Exclude pretty-SNBT compound field order from oracle byte-for-byte
//!   fixtures.

use std::collections::HashMap;

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
use rivet_text::{ChatFormatting, Component, Style};

/// `INLINE_LIST_THRESHOLD`.
pub const INLINE_LIST_THRESHOLD: i32 = 8;
/// `MAX_DEPTH`.
pub const MAX_DEPTH: i32 = 64;
/// `MAX_LENGTH`.
pub const MAX_LENGTH: i32 = 128;
/// `SIMPLE_VALUE` — `[A-Za-z0-9._+-]+` (fully anchored; no regex crate needed).
fn is_simple_value(input: &str) -> bool {
    !input.is_empty()
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

/// `Component.literal("\n")` — `NEWLINE`.
fn newline() -> Component {
    Component::literal("\n")
}

/// `Component.literal(" ")` — `ELEMENT_SPACING`.
fn element_spacing() -> Component {
    Component::literal(" ")
}

/// Port of `TextComponentTagVisitor`.
pub struct TextComponentTagVisitor {
    indentation: String,
    styling: Box<dyn Styling>,
    sort_keys: bool,
    indent_depth: i32,
    depth: i32,
    result: Component,
}

impl TextComponentTagVisitor {
    /// `new TextComponentTagVisitor(String indentation)` — RichStyling.
    pub fn new(indentation: &str) -> Self {
        Self::new_with_styling(indentation, Box::new(RichStyling::new()))
    }

    /// `new TextComponentTagVisitor(String, Styling)`.
    pub fn new_with_styling(indentation: &str, styling: Box<dyn Styling>) -> Self {
        // Java default: LOGGER.isDebugEnabled() == false at INFO level.
        Self::new_with_styling_sort_keys(indentation, styling, false)
    }

    /// `new TextComponentTagVisitor(String, Styling, boolean sortKeys)`.
    pub fn new_with_styling_sort_keys(
        indentation: &str,
        styling: Box<dyn Styling>,
        sort_keys: bool,
    ) -> Self {
        TextComponentTagVisitor {
            indentation: indentation.to_owned(),
            styling,
            sort_keys,
            indent_depth: 0,
            depth: 0,
            result: Component::empty(),
        }
    }

    /// `visit(Tag)` — run the visitor and return the built component.
    pub fn visit(&mut self, tag: &Tag) -> Component {
        tag.accept(self);
        self.result.clone()
    }

    /// `append(String, Style)`.
    fn append_styled(&mut self, string: &str, style: Style) -> &mut Self {
        self.result
            .append_component(Component::literal(string).with_style(style));
        self
    }

    /// `append(Component)`.
    fn append_component(&mut self, component: Component) -> &mut Self {
        self.result.append_component(component);
        self
    }

    /// `append(Token)` — via the active styling.
    fn append_token(&mut self, token: Token) -> &mut Self {
        self.result.append_component(self.styling.token(token));
        self
    }

    /// `appendSubTag(Tag, boolean indent)`.
    fn append_sub_tag(&mut self, tag: &Tag, indent: bool) {
        if indent {
            self.indent_depth += 1;
        }
        self.depth += 1;

        tag.accept(self);

        if indent {
            self.indent_depth -= 1;
        }
        self.depth -= 1;
    }

    /// `handleEscapePretty(String)`.
    fn handle_escape_pretty(&self, input: &str) -> Component {
        if is_simple_value(input) {
            return Component::literal(input).with_style(self.styling.key_style());
        }
        let quoted = quote_and_escape(input);
        // First and last chars are ASCII quotes; slice is safe.
        let quote = &quoted[..1];
        let inner = &quoted[1..quoted.len() - 1];
        let inner_component = Component::literal(inner).with_style(self.styling.key_style());
        let mut component = Component::literal(quote);
        component.append_component(inner_component);
        component.append_component(Component::literal(quote));
        component
    }
}

impl TagVisitor for TextComponentTagVisitor {
    fn visit_string(&mut self, tag: &StringTag) {
        let quoted = quote_and_escape(&tag.value);
        let quote = Component::literal(&quoted[..1]);
        let inner = &quoted[1..quoted.len() - 1];
        self.append_component(quote);
        self.append_styled(inner, self.styling.string_style());
        self.append_component(Component::literal(&quoted[..1]));
    }

    fn visit_byte(&mut self, tag: &ByteTag) {
        self.append_styled(&tag.value.to_string(), self.styling.number_style());
        self.append_token(Token::ByteSuffix);
    }

    fn visit_short(&mut self, tag: &ShortTag) {
        self.append_styled(&tag.value.to_string(), self.styling.number_style());
        self.append_token(Token::ShortSuffix);
    }

    fn visit_int(&mut self, tag: &IntTag) {
        self.append_styled(&tag.value.to_string(), self.styling.number_style());
    }

    fn visit_long(&mut self, tag: &LongTag) {
        self.append_styled(&tag.value.to_string(), self.styling.number_style());
        self.append_token(Token::LongSuffix);
    }

    fn visit_float(&mut self, tag: &FloatTag) {
        self.append_styled(
            &java_float_to_string(tag.value),
            self.styling.number_style(),
        );
        self.append_token(Token::FloatSuffix);
    }

    fn visit_double(&mut self, tag: &DoubleTag) {
        self.append_styled(
            &java_double_to_string(tag.value),
            self.styling.number_style(),
        );
        self.append_token(Token::DoubleSuffix);
    }

    fn visit_byte_array(&mut self, tag: &ByteArrayTag) {
        self.append_token(Token::ListOpen);
        self.append_token(Token::ByteArrayPrefix);
        self.append_token(Token::ListTypeSeparator);
        let data = &tag.data;

        for i in 0..data.len().min(MAX_LENGTH as usize) {
            self.append_component(element_spacing());
            self.append_styled(&data[i].to_string(), self.styling.number_style());
            self.append_token(Token::ByteSuffix);
            if i != data.len() - 1 {
                self.append_token(Token::ElementSeparator);
            }
        }

        if data.len() > MAX_LENGTH as usize {
            self.append_token(Token::Folded);
        }

        self.append_token(Token::ListClose);
    }

    fn visit_int_array(&mut self, tag: &IntArrayTag) {
        self.append_token(Token::ListOpen);
        self.append_token(Token::IntArrayPrefix);
        self.append_token(Token::ListTypeSeparator);
        let data = &tag.data;

        for i in 0..data.len().min(MAX_LENGTH as usize) {
            self.append_component(element_spacing());
            self.append_styled(&data[i].to_string(), self.styling.number_style());
            if i != data.len() - 1 {
                self.append_token(Token::ElementSeparator);
            }
        }

        if data.len() > MAX_LENGTH as usize {
            self.append_token(Token::Folded);
        }

        self.append_token(Token::ListClose);
    }

    fn visit_long_array(&mut self, tag: &LongArrayTag) {
        self.append_token(Token::ListOpen);
        self.append_token(Token::LongArrayPrefix);
        self.append_token(Token::ListTypeSeparator);
        let data = &tag.data;

        for i in 0..data.len().min(MAX_LENGTH as usize) {
            self.append_component(element_spacing());
            self.append_styled(&data[i].to_string(), self.styling.number_style());
            self.append_token(Token::LongSuffix);
            if i != data.len() - 1 {
                self.append_token(Token::ElementSeparator);
            }
        }

        if data.len() > MAX_LENGTH as usize {
            self.append_token(Token::Folded);
        }

        self.append_token(Token::ListClose);
    }

    fn visit_list(&mut self, tag: &ListTag) {
        if tag.is_empty() {
            self.append_token(Token::ListOpen);
            self.append_token(Token::ListClose);
        } else if self.depth >= MAX_DEPTH {
            self.append_token(Token::ListOpen);
            self.append_token(Token::Folded);
            self.append_token(Token::ListClose);
        } else if !should_wrap_list_elements(tag) {
            self.append_token(Token::ListOpen);

            for i in 0..tag.size() {
                if i != 0 {
                    self.append_token(Token::ElementSeparator);
                    self.append_component(element_spacing());
                }
                self.append_sub_tag(tag.get(i), false);
            }

            self.append_token(Token::ListClose);
        } else {
            self.append_token(Token::ListOpen);
            if !self.indentation.is_empty() {
                self.append_component(newline());
            }

            let indent_text =
                Component::literal(&self.indentation.repeat((self.indent_depth + 1) as usize));
            let entry_spacing = if self.indentation.is_empty() {
                Component::literal(" ")
            } else {
                newline()
            };

            for i in 0..tag.size().min(MAX_LENGTH as usize) {
                self.append_component(indent_text.clone());
                self.append_sub_tag(tag.get(i), true);
                if i != tag.size() - 1 {
                    self.append_token(Token::ElementSeparator);
                    self.append_component(entry_spacing.clone());
                }
            }

            if tag.size() > MAX_LENGTH as usize {
                self.append_component(indent_text);
                self.append_token(Token::Folded);
            }

            if !self.indentation.is_empty() {
                self.append_component(newline());
                self.append_component(Component::literal(
                    &self.indentation.repeat(self.indent_depth as usize),
                ));
            }

            self.append_token(Token::ListClose);
        }
    }

    fn visit_compound(&mut self, tag: &CompoundTag) {
        if tag.is_empty() {
            self.append_token(Token::StructOpen);
            self.append_token(Token::StructClose);
        } else if self.depth >= MAX_DEPTH {
            self.append_token(Token::StructOpen);
            self.append_token(Token::Folded);
            self.append_token(Token::StructClose);
        } else {
            self.append_token(Token::StructOpen);

            let keys: Vec<String> = if self.sort_keys {
                let mut key_copy: Vec<String> = tag.key_set().cloned().collect();
                key_copy.sort();
                key_copy
            } else {
                tag.key_set().cloned().collect()
            };

            if !self.indentation.is_empty() {
                self.append_component(newline());
            }

            let indent_text =
                Component::literal(&self.indentation.repeat((self.indent_depth + 1) as usize));
            let entry_spacing = if self.indentation.is_empty() {
                Component::literal(" ")
            } else {
                newline()
            };

            for (i, key) in keys.iter().enumerate() {
                self.append_component(indent_text.clone());
                self.append_component(self.handle_escape_pretty(key));
                self.append_token(Token::NameValueSeparator);
                self.append_component(Component::literal(" "));
                // Java `tag.get(key)` is non-null here (keys come from keySet).
                if let Some(sub) = tag.get(key) {
                    self.append_sub_tag(sub, true);
                }
                if i + 1 != keys.len() {
                    self.append_token(Token::ElementSeparator);
                    self.append_component(entry_spacing.clone());
                }
            }

            if !self.indentation.is_empty() {
                self.append_component(newline());
                self.append_component(Component::literal(
                    &self.indentation.repeat(self.indent_depth as usize),
                ));
            }

            self.append_token(Token::StructClose);
        }
    }

    fn visit_end(&mut self, _tag: &EndTag) {
        // Java: empty method.
    }
}

/// `shouldWrapListElements(ListTag)` — static, per the Java private method.
fn should_wrap_list_elements(list: &ListTag) -> bool {
    if list.size() >= INLINE_LIST_THRESHOLD as usize {
        return false;
    }
    list.iter().any(|element| !is_numeric_tag(element))
}

/// `element instanceof NumericTag` — the six numeric leaves.
fn is_numeric_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}

/// Port of `TextComponentTagVisitor.Styling` (interface).
pub trait Styling {
    fn key_style(&self) -> Style;
    fn string_style(&self) -> Style;
    fn number_style(&self) -> Style;
    fn token(&self, token: Token) -> Component;
}

/// Port of `TextComponentTagVisitor.PlainStyling`.
pub struct PlainStyling {
    tokens: HashMap<Token, Component>,
}

impl PlainStyling {
    /// `PlainStyling.INSTANCE`.
    pub fn instance() -> Self {
        let mut tokens = HashMap::new();
        for value in Token::VALUES {
            tokens.insert(value, Component::literal(value.text()));
        }
        PlainStyling { tokens }
    }
}

impl Styling for PlainStyling {
    fn key_style(&self) -> Style {
        Style::EMPTY
    }

    fn string_style(&self) -> Style {
        Style::EMPTY
    }

    fn number_style(&self) -> Style {
        Style::EMPTY
    }

    fn token(&self, token: Token) -> Component {
        self.tokens
            .get(&token)
            .cloned()
            .expect("PlainStyling must contain every Token")
    }
}

/// Port of `TextComponentTagVisitor.RichStyling`.
pub struct RichStyling {
    tokens: HashMap<Token, Component>,
}

impl RichStyling {
    /// `RichStyling.INSTANCE`.
    pub fn instance() -> Self {
        Self::new()
    }

    fn new() -> Self {
        let number_type = Style::EMPTY.with_color_format(ChatFormatting::Red);
        let mut tokens = HashMap::new();

        Self::override_token(
            &mut tokens,
            Token::Folded,
            Style::EMPTY.with_color_format(ChatFormatting::Gray),
        );
        Self::override_token(&mut tokens, Token::ByteSuffix, number_type.clone());
        Self::override_token(&mut tokens, Token::ByteArrayPrefix, number_type.clone());
        Self::override_token(&mut tokens, Token::ShortSuffix, number_type.clone());
        Self::override_token(&mut tokens, Token::IntArrayPrefix, number_type.clone());
        Self::override_token(&mut tokens, Token::LongSuffix, number_type.clone());
        Self::override_token(&mut tokens, Token::LongArrayPrefix, number_type.clone());
        Self::override_token(&mut tokens, Token::FloatSuffix, number_type.clone());
        Self::override_token(&mut tokens, Token::DoubleSuffix, number_type);

        for value in Token::VALUES {
            tokens
                .entry(value)
                .or_insert_with(|| Component::literal(value.text()));
        }

        RichStyling { tokens }
    }

    /// `overrideToken(Token, Style)`.
    fn override_token(tokens: &mut HashMap<Token, Component>, token: Token, style: Style) {
        tokens.insert(token, Component::literal(token.text()).with_style(style));
    }
}

impl Styling for RichStyling {
    fn key_style(&self) -> Style {
        Style::EMPTY.with_color_format(ChatFormatting::Aqua)
    }

    fn string_style(&self) -> Style {
        Style::EMPTY.with_color_format(ChatFormatting::Green)
    }

    fn number_style(&self) -> Style {
        Style::EMPTY.with_color_format(ChatFormatting::Gold)
    }

    fn token(&self, token: Token) -> Component {
        self.tokens
            .get(&token)
            .cloned()
            .expect("RichStyling must contain every Token")
    }
}

/// Port of `TextComponentTagVisitor.Token` (enum with a `text` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Folded,
    ElementSeparator,
    ListClose,
    ListOpen,
    ListTypeSeparator,
    StructClose,
    StructOpen,
    NameValueSeparator,
    ByteSuffix,
    ByteArrayPrefix,
    ShortSuffix,
    IntArrayPrefix,
    LongSuffix,
    LongArrayPrefix,
    FloatSuffix,
    DoubleSuffix,
}

impl Token {
    /// All variants, in declaration order (`Token.values()`).
    pub const VALUES: [Token; 16] = [
        Token::Folded,
        Token::ElementSeparator,
        Token::ListClose,
        Token::ListOpen,
        Token::ListTypeSeparator,
        Token::StructClose,
        Token::StructOpen,
        Token::NameValueSeparator,
        Token::ByteSuffix,
        Token::ByteArrayPrefix,
        Token::ShortSuffix,
        Token::IntArrayPrefix,
        Token::LongSuffix,
        Token::LongArrayPrefix,
        Token::FloatSuffix,
        Token::DoubleSuffix,
    ];

    /// `token.text`.
    pub fn text(self) -> &'static str {
        match self {
            Token::Folded => "<...>",
            Token::ElementSeparator => ",",
            Token::ListClose => "]",
            Token::ListOpen => "[",
            Token::ListTypeSeparator => ";",
            Token::StructClose => "}",
            Token::StructOpen => "{",
            Token::NameValueSeparator => ":",
            Token::ByteSuffix => "b",
            Token::ByteArrayPrefix => "B",
            Token::ShortSuffix => "s",
            Token::IntArrayPrefix => "I",
            Token::LongSuffix => "L",
            Token::LongArrayPrefix => "L",
            Token::FloatSuffix => "f",
            Token::DoubleSuffix => "d",
        }
    }
}

/// `String.valueOf(float)` parity — Java `Float.toString`. Delegates to the
/// shared `float_to_string` module (also used by `StringTagVisitor`).
pub fn java_float_to_string(value: f32) -> String {
    crate::float_to_string::java_float_to_string(value)
}

/// `String.valueOf(double)` parity — Java `Double.toString`. Delegates to the
/// shared `float_to_string` module (also used by `StringTagVisitor`).
pub fn java_double_to_string(value: f64) -> String {
    crate::float_to_string::java_double_to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byte_array_tag::ByteArrayTag;
    use crate::byte_tag::ByteTag;
    use crate::double_tag::DoubleTag;
    use crate::float_tag::FloatTag;
    use crate::int_array_tag::IntArrayTag;
    use crate::long_array_tag::LongArrayTag;
    use crate::string_tag::StringTag;

    /// Flatten to the plain-text string a rich component renders to
    /// (`Component.getString()` on the built component).
    fn build_string(tag: &Tag) -> String {
        let mut visitor = TextComponentTagVisitor::new("");
        visitor.visit(tag).get_string()
    }

    /// The style color (RichStyling) of the token text, if any.
    fn token_color(token: Token) -> Option<rivet_text::TextColor> {
        RichStyling::instance()
            .token(token)
            .get_style()
            .get_color()
            .copied()
    }

    #[test]
    fn plain_empty_compound() {
        let tag = Tag::Compound(CompoundTag::new());
        assert_eq!(build_string(&tag), "{}");
    }

    #[test]
    fn plain_primitives() {
        let mut compound = CompoundTag::new();
        compound.put_int("i", 5);
        compound.put_long("l", 9007199254740993_i64);
        compound.put_string("s", "he\"llo");
        compound.put_byte("b", -5);

        let mut visitor = TextComponentTagVisitor::new("");
        let component = visitor.visit(&Tag::Compound(compound));

        // No indentation, so a non-empty compound is one inline run.
        let text = component.get_string();
        assert!(text.starts_with("{"));
        assert!(text.ends_with("}"));
        // Java: keys appear in HashMap iteration order; we can't assert order.
        // Assert the components the visitor built directly.
        // `he"llo` contains `"`, so quoteAndEscape chooses `'` as the delimiter
        // and does not escape the `"`.
        assert!(text.contains("'he\"llo'"));
        assert!(text.contains("5"));
        assert!(text.contains("9007199254740993L"));
        assert!(text.contains("-5b"));
    }

    #[test]
    fn plain_string_quotes() {
        // `"` inside selects `'` as the quote delimiter, like Java.
        let tag = Tag::String(StringTag::value_of("he\"llo".to_owned()));
        assert_eq!(build_string(&tag), "'he\"llo'");

        let tag = Tag::String(StringTag::value_of("plain".to_owned()));
        assert_eq!(build_string(&tag), "\"plain\"");
    }

    #[test]
    fn plain_empty_list_and_wrapped_list() {
        let empty = Tag::List(ListTag::new());
        assert_eq!(build_string(&empty), "[]");

        // A list of compounds is wrapped (non-numeric, size < 8) with
        // indentation "" => entry indent is "" (empty), element spacing " ".
        let mut inner = ListTag::new();
        inner.add(Tag::Compound(CompoundTag::new()));
        inner.add(Tag::Compound(CompoundTag::new()));
        let text = build_string(&Tag::List(inner));
        assert_eq!(text, "[{}, {}]");
    }

    #[test]
    fn plain_inline_numeric_list() {
        // 2 numeric elements < 8 => inline, separated by ", ".
        let mut list = ListTag::new();
        list.add(Tag::Byte(ByteTag::value_of(1)));
        list.add(Tag::Byte(ByteTag::value_of(2)));
        let text = build_string(&Tag::List(list));
        assert_eq!(text, "[1b, 2b]");
    }

    #[test]
    fn plain_large_numeric_list_inline() {
        // 9 numeric elements >= INLINE_LIST_THRESHOLD => shouldWrapListElements
        // is false, so the list renders inline (no wrapping, no fold).
        let mut list = ListTag::new();
        for _ in 0..9 {
            list.add(Tag::Byte(ByteTag::value_of(1)));
        }
        let text = build_string(&Tag::List(list));
        assert_eq!(text, "[1b, 1b, 1b, 1b, 1b, 1b, 1b, 1b, 1b]");
    }

    #[test]
    fn arrays_prefix_and_fold() {
        let tag = Tag::ByteArray(ByteArrayTag::new(vec![1, 2]));
        assert_eq!(build_string(&tag), "[B; 1b, 2b]");

        let ints = Tag::IntArray(IntArrayTag::new(vec![1, 2]));
        assert_eq!(build_string(&ints), "[I; 1, 2]");

        let longs = Tag::LongArray(LongArrayTag::new(vec![1, 2]));
        assert_eq!(build_string(&longs), "[L; 1L, 2L]");

        // 129 elements => folded marker after the first 128.
        let big = ByteArrayTag::new((0..129).map(|v| v as i8).collect());
        let text = build_string(&Tag::ByteArray(big));
        assert!(text.contains("<...>"));
    }

    #[test]
    fn pretty_indented_compound() {
        let mut compound = CompoundTag::new();
        compound.put_int("x", 1);
        let mut nested = CompoundTag::new();
        nested.put_string("name", "value");
        compound.put("nested".to_string(), Tag::Compound(nested));

        let mut visitor = TextComponentTagVisitor::new("  ");
        let text = visitor.visit(&Tag::Compound(compound)).get_string();
        assert!(text.contains("\n  x: 1"));
        assert!(text.contains("\n  nested: {\n    name: \"value\"\n  }"));
    }

    #[test]
    fn rich_styling_colors() {
        // RichStyling colors: RED number type, AQUA key, GREEN string, GOLD number.
        let red = token_color(Token::ByteSuffix).expect("byte suffix colored");
        assert_eq!(red, rivet_text::TextColor::RED);
        assert_eq!(
            token_color(Token::Folded),
            Some(rivet_text::TextColor::GRAY)
        );
        assert_eq!(
            token_color(Token::ByteArrayPrefix),
            Some(rivet_text::TextColor::RED)
        );

        let rich = RichStyling::instance();
        assert_eq!(
            rich.key_style().get_color(),
            Some(&rivet_text::TextColor::AQUA)
        );
        assert_eq!(
            rich.string_style().get_color(),
            Some(&rivet_text::TextColor::GREEN)
        );
        assert_eq!(
            rich.number_style().get_color(),
            Some(&rivet_text::TextColor::GOLD)
        );
    }

    #[test]
    fn rich_number_style_applied() {
        let mut compound = CompoundTag::new();
        compound.put_int("a", 42);
        let mut visitor =
            TextComponentTagVisitor::new_with_styling("", Box::new(RichStyling::instance()));
        let component = visitor.visit(&Tag::Compound(compound));
        // The "42" leaf carries the GOLD number style.
        let styled: Vec<_> = component
            .flatten()
            .into_iter()
            .filter(|(text, _)| text == "42")
            .collect();
        assert_eq!(styled.len(), 1);
        assert_eq!(styled[0].1.get_color(), Some(&rivet_text::TextColor::GOLD));
    }

    #[test]
    fn float_double_formatting() {
        // Java String.valueOf(float): integral floats get ".0".
        assert_eq!(build_string(&Tag::Float(FloatTag::value_of(1.0))), "1.0f");
        assert_eq!(build_string(&Tag::Float(FloatTag::value_of(0.5))), "0.5f");
        assert_eq!(build_string(&Tag::Double(DoubleTag::value_of(1.5))), "1.5d");
        // A value that forces scientific notation in Java Double.toString.
        assert_eq!(
            build_string(&Tag::Double(DoubleTag::value_of(1.0e7))),
            "1.0E7d"
        );
    }

    #[test]
    fn depth_fold() {
        // A deeply nested compound beyond MAX_DEPTH renders {<...>} at the fold.
        // Build innermost-first so no mutable re-descend is needed.
        let mut inner = CompoundTag::new();
        for _ in 0..70 {
            let mut outer = CompoundTag::new();
            outer.put("n".to_string(), Tag::Compound(inner));
            inner = outer;
        }
        let text = build_string(&Tag::Compound(inner));
        assert!(text.contains("<...>"));
    }

    #[test]
    fn plain_styling_emits_empty_styles() {
        let mut visitor =
            TextComponentTagVisitor::new_with_styling("", Box::new(PlainStyling::instance()));
        let mut compound = CompoundTag::new();
        compound.put_int("a", 42);
        let component = visitor.visit(&Tag::Compound(compound));
        for (_, style) in component.flatten() {
            assert!(style.is_empty());
        }
    }
}
