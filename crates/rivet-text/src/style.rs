// STUB(mc.nbt.text) — minimal port of `net.minecraft.network.chat.Style` for
// `TextComponentTagVisitor`. Owned by the net.minecraft.network.chat package
// (rivet-text); replaced by the real port. Only the color surface the visitor
// uses is modeled.

use crate::text_color::TextColor;
use rivet_core::ChatFormatting;

/// Port of `net.minecraft.network.chat.Style` (color-only stub).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    color: Option<TextColor>,
}

impl Style {
    /// `Style.EMPTY`.
    pub const EMPTY: Style = Style { color: None };

    /// `Style.withColor(ChatFormatting)`. A non-color format resolves to
    /// `TextColor.fromLegacyFormat` → null, which (in the color-only stub)
    /// clears to EMPTY, matching `checkEmptyAfterChange`.
    pub fn with_color(&self, color: ChatFormatting) -> Style {
        match TextColor::from_legacy_format(color) {
            Some(color) => Style { color: Some(color) },
            None => Style::EMPTY,
        }
    }

    /// `Style.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        *self == Style::EMPTY
    }

    /// `Style.getColor()`.
    pub fn get_color(&self) -> Option<&TextColor> {
        self.color.as_ref()
    }

    /// `Style.applyTo(other)` — this Style's non-null fields win.
    pub fn apply_to(&self, other: &Style) -> Style {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Style {
            color: self.color.or(other.color),
        }
    }
}
