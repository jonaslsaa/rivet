//! Port of `net.minecraft.network.chat.TextColor`.
//!
//! Java `TextColor` is a value type holding an `int value` (masked to 24 bits)
//! and an optional `String name`. Equality is on the *value* only (`equals`
//! compares `this.value`). `serialize()` returns the name when present, else
//! `#RRGGBB` (uppercase hex via `String.format(Locale.ROOT, "#%06X")`).
//!
//! Note on hashing: Java's `hashCode` is `Objects.hash(value, name)`, which
//! violates the equals/hashCode contract for value-equal named/un-named pairs.
//! Rust requires `Hash` to agree with `PartialEq`, so `TextColor` hashes the
//! value only (e.g. `RED` and `from_rgb(0xFF5555)` are equal and hash equal).

use crate::ChatFormatting;

/// Port of `net.minecraft.network.chat.TextColor`.
#[derive(Clone, Copy, Debug)]
pub struct TextColor {
    value: i32,
    name: Option<&'static str>,
}

impl PartialEq for TextColor {
    /// Java `TextColor.equals` — compares `value` only, ignoring `name`.
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for TextColor {}

impl std::hash::Hash for TextColor {
    /// Java's `hashCode` is `Objects.hash(value, name)` and breaks the
    /// equals/hashCode contract; hash the value so equal colors hash equally.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl TextColor {
    /// `TextColor.CUSTOM_COLOR_PREFIX`.
    pub const CUSTOM_COLOR_PREFIX: &'static str = "#";

    // Named colors — exact RGB values from the Java `named(...)` statics (the
    // constructor masks to 24 bits, which is a no-op for these constants).
    pub const BLACK: TextColor = TextColor::named("black", 0);
    pub const DARK_BLUE: TextColor = TextColor::named("dark_blue", 170);
    pub const DARK_GREEN: TextColor = TextColor::named("dark_green", 43520);
    pub const DARK_AQUA: TextColor = TextColor::named("dark_aqua", 43690);
    pub const DARK_RED: TextColor = TextColor::named("dark_red", 11141120);
    pub const DARK_PURPLE: TextColor = TextColor::named("dark_purple", 11141290);
    pub const GOLD: TextColor = TextColor::named("gold", 16755200);
    pub const GRAY: TextColor = TextColor::named("gray", 11184810);
    pub const DARK_GRAY: TextColor = TextColor::named("dark_gray", 5592405);
    pub const BLUE: TextColor = TextColor::named("blue", 5592575);
    pub const GREEN: TextColor = TextColor::named("green", 5635925);
    pub const AQUA: TextColor = TextColor::named("aqua", 5636095);
    pub const RED: TextColor = TextColor::named("red", 16733525);
    pub const LIGHT_PURPLE: TextColor = TextColor::named("light_purple", 16733695);
    pub const YELLOW: TextColor = TextColor::named("yellow", 16777045);
    pub const WHITE: TextColor = TextColor::named("white", 16777215);

    /// `TextColor.named(name, rgb)` — `new TextColor(rgb, name)`.
    const fn named(name: &'static str, rgb: i32) -> TextColor {
        TextColor {
            value: rgb & 0xFF_FFFF,
            name: Some(name),
        }
    }

    /// `TextColor.fromRgb(rgb)` — `new TextColor(rgb)` (un-named custom color).
    pub fn from_rgb(rgb: i32) -> TextColor {
        TextColor {
            value: rgb & 0xFF_FFFF,
            name: None,
        }
    }

    /// `TextColor.fromLegacyFormat(ChatFormatting)` — `null` for non-color
    /// formats (RESET and the style flags).
    pub fn from_legacy_format(format: ChatFormatting) -> Option<TextColor> {
        match format {
            ChatFormatting::Black => Some(Self::BLACK),
            ChatFormatting::DarkBlue => Some(Self::DARK_BLUE),
            ChatFormatting::DarkGreen => Some(Self::DARK_GREEN),
            ChatFormatting::DarkAqua => Some(Self::DARK_AQUA),
            ChatFormatting::DarkRed => Some(Self::DARK_RED),
            ChatFormatting::DarkPurple => Some(Self::DARK_PURPLE),
            ChatFormatting::Gold => Some(Self::GOLD),
            ChatFormatting::Gray => Some(Self::GRAY),
            ChatFormatting::DarkGray => Some(Self::DARK_GRAY),
            ChatFormatting::Blue => Some(Self::BLUE),
            ChatFormatting::Green => Some(Self::GREEN),
            ChatFormatting::Aqua => Some(Self::AQUA),
            ChatFormatting::Red => Some(Self::RED),
            ChatFormatting::LightPurple => Some(Self::LIGHT_PURPLE),
            ChatFormatting::Yellow => Some(Self::YELLOW),
            ChatFormatting::White => Some(Self::WHITE),
            _ => None,
        }
    }

    /// `TextColor.getValue()`.
    pub fn get_value(&self) -> i32 {
        self.value
    }

    /// `TextColor.serialize()` — the name, or `#RRGGBB` for a custom color.
    pub fn serialize(&self) -> String {
        match self.name {
            Some(name) => name.to_owned(),
            None => format!("#{:06X}", self.value),
        }
    }

    /// `TextColor.parseColor(String)` — `#`-prefixed hex (in range) or a
    /// named color. Errors, matching Java:
    /// `"Color value out of range: {color}"`, `"Invalid color value:
    /// {color}"`, `"Invalid color name: {color}"`.
    pub fn parse_color(color: &str) -> Result<TextColor, String> {
        if let Some(rest) = color.strip_prefix(Self::CUSTOM_COLOR_PREFIX) {
            match i32::from_str_radix(rest, 16) {
                Ok(value) if (0..=0xFF_FFFF).contains(&value) => Ok(TextColor::from_rgb(value)),
                Ok(_) => Err(format!("Color value out of range: {color}")),
                Err(_) => Err(format!("Invalid color value: {color}")),
            }
        } else {
            named_color(color).ok_or_else(|| format!("Invalid color name: {color}"))
        }
    }
}

/// Named-color lookup (Java's `NAMED_COLORS` map; built lazily from the
/// `named(...)` constants).
fn named_color(name: &str) -> Option<TextColor> {
    const NAMED: [(&str, TextColor); 16] = [
        ("black", TextColor::BLACK),
        ("dark_blue", TextColor::DARK_BLUE),
        ("dark_green", TextColor::DARK_GREEN),
        ("dark_aqua", TextColor::DARK_AQUA),
        ("dark_red", TextColor::DARK_RED),
        ("dark_purple", TextColor::DARK_PURPLE),
        ("gold", TextColor::GOLD),
        ("gray", TextColor::GRAY),
        ("dark_gray", TextColor::DARK_GRAY),
        ("blue", TextColor::BLUE),
        ("green", TextColor::GREEN),
        ("aqua", TextColor::AQUA),
        ("red", TextColor::RED),
        ("light_purple", TextColor::LIGHT_PURPLE),
        ("yellow", TextColor::YELLOW),
        ("white", TextColor::WHITE),
    ];
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

impl std::fmt::Display for TextColor {
    /// `TextColor.toString()` = `serialize()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.serialize())
    }
}

/// `TextColor.CODEC` — `Codec.STRING.comapFlatMap(TextColor::parseColor,
/// TextColor::serialize)`: decode parses (failing with Java's exact messages),
/// encode serializes to the name or `#RRGGBB`.
pub fn text_color_codec<Ops: rivet_serialization::DynamicOps + 'static>()
-> std::sync::Arc<dyn rivet_serialization::Codec<TextColor, Ops>> {
    use rivet_serialization::codec;
    use rivet_serialization::data_result::DataResult;
    use std::sync::Arc;
    codec::flat_xmap(
        codec::string_codec(),
        Arc::new(|s: &String| match TextColor::parse_color(s) {
            Ok(color) => DataResult::success(color),
            Err(message) => DataResult::error(message),
        }),
        Arc::new(|c: &TextColor| DataResult::success(c.serialize())),
    )
}
