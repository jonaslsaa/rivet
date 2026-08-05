// STUB(mc.nbt.text) — minimal port of `net.minecraft.network.chat.TextColor` for
// `TextComponentTagVisitor`. Owned by the net.minecraft.network.chat package
// (rivet-text); replaced by the real port.

use rivet_core::ChatFormatting;

/// Port of `net.minecraft.network.chat.TextColor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextColor {
    value: i32,
    name: Option<&'static str>,
}

impl TextColor {
    // Named colors, exact ARGB values from the Java `named(...)` statics.
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

    /// `TextColor.named(name, rgb)` — the value is masked to 24 bits.
    const fn named(name: &'static str, rgb: i32) -> TextColor {
        TextColor {
            value: rgb & 0xFF_FFFF,
            name: Some(name),
        }
    }

    /// `TextColor.fromRgb(rgb)`.
    pub fn from_rgb(rgb: i32) -> TextColor {
        TextColor {
            value: rgb & 0xFF_FFFF,
            name: None,
        }
    }

    /// `TextColor.fromLegacyFormat(format)`.
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

    /// `TextColor.serialize()` — the name, or `#RRGGBB` when unnamed.
    pub fn serialize(&self) -> String {
        match self.name {
            Some(name) => name.to_owned(),
            None => format!("#{:06X}", self.value),
        }
    }
}
