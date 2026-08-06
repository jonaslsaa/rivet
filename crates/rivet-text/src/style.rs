//! Port of `net.minecraft.network.chat.Style`.
//!
//! `Style` is an immutable value holding nullable text-formatting fields.
//! `withX` builders return `this` when the value is unchanged, and
//! `checkEmptyAfterChange` collapses any all-null style back to the `EMPTY`
//! singleton. `applyTo` merges with `this`'s non-null fields winning (used for
//! style inheritance in `Component.visit`). Equality is structural over every
//! field; `hashCode` is `Objects.hash(color, shadowColor, bold, italic,
//! underlined, strikethrough, obfuscated, clickEvent, hoverEvent, insertion)`
//! (font deliberately excluded — Java bug-for-bug).

use crate::ChatFormatting;
use crate::click_event::ClickEvent;
use crate::font_description::FontDescription;
use crate::hover_event::HoverEvent;
use crate::text_color::TextColor;

/// Port of `net.minecraft.network.chat.Style`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    color: Option<TextColor>,
    shadow_color: Option<i32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underlined: Option<bool>,
    strikethrough: Option<bool>,
    obfuscated: Option<bool>,
    click_event: Option<ClickEvent>,
    hover_event: Option<HoverEvent>,
    insertion: Option<String>,
    font: Option<FontDescription>,
}

impl Style {
    /// `Style.EMPTY`.
    pub const EMPTY: Style = Style {
        color: None,
        shadow_color: None,
        bold: None,
        italic: None,
        underlined: None,
        strikethrough: None,
        obfuscated: None,
        click_event: None,
        hover_event: None,
        insertion: None,
        font: None,
    };

    /// `Style.NO_SHADOW`.
    pub const NO_SHADOW: i32 = 0;

    /// `Style.create(...)` — the private all-fields constructor that collapses
    /// an all-empty result to `EMPTY`.
    #[allow(clippy::too_many_arguments)] // mirrors Java's 11-field `Style.create`
    fn create(
        color: Option<TextColor>,
        shadow_color: Option<i32>,
        bold: Option<bool>,
        italic: Option<bool>,
        underlined: Option<bool>,
        strikethrough: Option<bool>,
        obfuscated: Option<bool>,
        click_event: Option<ClickEvent>,
        hover_event: Option<HoverEvent>,
        insertion: Option<String>,
        font: Option<FontDescription>,
    ) -> Style {
        let result = Style {
            color,
            shadow_color,
            bold,
            italic,
            underlined,
            strikethrough,
            obfuscated,
            click_event,
            hover_event,
            insertion,
            font,
        };
        if result == Style::EMPTY {
            Style::EMPTY
        } else {
            result
        }
    }

    /// `Style.getColor()`.
    pub fn get_color(&self) -> Option<&TextColor> {
        self.color.as_ref()
    }

    /// `Style.getShadowColor()`.
    pub fn get_shadow_color(&self) -> Option<i32> {
        self.shadow_color
    }

    /// `Style.isBold()` — false when absent.
    pub fn is_bold(&self) -> bool {
        self.bold == Some(true)
    }

    /// `Style.isItalic()`.
    pub fn is_italic(&self) -> bool {
        self.italic == Some(true)
    }

    /// `Style.isStrikethrough()`.
    pub fn is_strikethrough(&self) -> bool {
        self.strikethrough == Some(true)
    }

    /// `Style.isUnderlined()`.
    pub fn is_underlined(&self) -> bool {
        self.underlined == Some(true)
    }

    /// `Style.isObfuscated()`.
    pub fn is_obfuscated(&self) -> bool {
        self.obfuscated == Some(true)
    }

    /// `Style.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self == &Style::EMPTY
    }

    /// `Style.getClickEvent()`.
    pub fn get_click_event(&self) -> Option<&ClickEvent> {
        self.click_event.as_ref()
    }

    /// `Style.getHoverEvent()`.
    pub fn get_hover_event(&self) -> Option<&HoverEvent> {
        self.hover_event.as_ref()
    }

    /// `Style.getInsertion()`.
    pub fn get_insertion(&self) -> Option<&str> {
        self.insertion.as_deref()
    }

    /// `Style.getFont()` — `this.font != null ? this.font : DEFAULT`.
    pub fn get_font(&self) -> FontDescription {
        self.font
            .clone()
            .unwrap_or_else(FontDescription::default_font)
    }

    /// `Style.checkEmptyAfterChange(previous, next)` — collapse when the only
    /// field was cleared and nothing else remains.
    fn check_empty_after_change<T: PartialEq>(
        new_style: Style,
        previous: Option<&T>,
        next: Option<&T>,
    ) -> Style {
        if previous.is_some() && next.is_none() && new_style == Style::EMPTY {
            Style::EMPTY
        } else {
            new_style
        }
    }

    /// `Style.withColor(TextColor)`.
    pub fn with_color(&self, color: Option<TextColor>) -> Style {
        if self.color == color {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                color,
                ..self.clone()
            },
            self.color.as_ref(),
            color.as_ref(),
        )
    }

    /// `Style.withColor(ChatFormatting)`.
    pub fn with_color_format(&self, color: ChatFormatting) -> Style {
        self.with_color(TextColor::from_legacy_format(color))
    }

    /// `Style.withColor(int)`.
    pub fn with_color_rgb(&self, color: i32) -> Style {
        self.with_color(Some(TextColor::from_rgb(color)))
    }

    /// `Style.withShadowColor(int)`.
    pub fn with_shadow_color(&self, shadow_color: i32) -> Style {
        if self.shadow_color == Some(shadow_color) {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                shadow_color: Some(shadow_color),
                ..self.clone()
            },
            self.shadow_color.as_ref(),
            Some(&shadow_color),
        )
    }

    /// `Style.withoutShadow()` — `withShadowColor(0)`.
    pub fn without_shadow(&self) -> Style {
        self.with_shadow_color(Style::NO_SHADOW)
    }

    /// `Style.withBold(Boolean)`.
    pub fn with_bold(&self, bold: Option<bool>) -> Style {
        if self.bold == bold {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                bold,
                ..self.clone()
            },
            self.bold.as_ref(),
            bold.as_ref(),
        )
    }

    /// `Style.withItalic(Boolean)`.
    pub fn with_italic(&self, italic: Option<bool>) -> Style {
        if self.italic == italic {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                italic,
                ..self.clone()
            },
            self.italic.as_ref(),
            italic.as_ref(),
        )
    }

    /// `Style.withUnderlined(Boolean)`.
    pub fn with_underlined(&self, underlined: Option<bool>) -> Style {
        if self.underlined == underlined {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                underlined,
                ..self.clone()
            },
            self.underlined.as_ref(),
            underlined.as_ref(),
        )
    }

    /// `Style.withStrikethrough(Boolean)`.
    pub fn with_strikethrough(&self, strikethrough: Option<bool>) -> Style {
        if self.strikethrough == strikethrough {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                strikethrough,
                ..self.clone()
            },
            self.strikethrough.as_ref(),
            strikethrough.as_ref(),
        )
    }

    /// `Style.withObfuscated(Boolean)`.
    pub fn with_obfuscated(&self, obfuscated: Option<bool>) -> Style {
        if self.obfuscated == obfuscated {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                obfuscated,
                ..self.clone()
            },
            self.obfuscated.as_ref(),
            obfuscated.as_ref(),
        )
    }

    /// `Style.withClickEvent(ClickEvent)`.
    pub fn with_click_event(&self, click_event: Option<ClickEvent>) -> Style {
        if self.click_event == click_event {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                click_event: click_event.clone(),
                ..self.clone()
            },
            self.click_event.as_ref(),
            click_event.as_ref(),
        )
    }

    /// `Style.withHoverEvent(HoverEvent)`.
    pub fn with_hover_event(&self, hover_event: Option<HoverEvent>) -> Style {
        if self.hover_event == hover_event {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                hover_event: hover_event.clone(),
                ..self.clone()
            },
            self.hover_event.as_ref(),
            hover_event.as_ref(),
        )
    }

    /// `Style.withInsertion(String)`.
    pub fn with_insertion(&self, insertion: Option<String>) -> Style {
        if self.insertion == insertion {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                insertion: insertion.clone(),
                ..self.clone()
            },
            self.insertion.as_ref(),
            insertion.as_ref(),
        )
    }

    /// `Style.withFont(FontDescription)`.
    pub fn with_font(&self, font: Option<FontDescription>) -> Style {
        if self.font == font {
            return self.clone();
        }
        Self::check_empty_after_change(
            Style {
                font: font.clone(),
                ..self.clone()
            },
            self.font.as_ref(),
            font.as_ref(),
        )
    }

    /// `Style.applyFormat(ChatFormatting)` — a single format; RESET clears to
    /// `EMPTY` (all fields).
    pub fn apply_format(&self, format: ChatFormatting) -> Style {
        let mut color = self.color;
        let mut bold = self.bold;
        let mut italic = self.italic;
        let mut strikethrough = self.strikethrough;
        let mut underlined = self.underlined;
        let mut obfuscated = self.obfuscated;
        match format {
            ChatFormatting::Obfuscated => obfuscated = Some(true),
            ChatFormatting::Bold => bold = Some(true),
            ChatFormatting::Strikethrough => strikethrough = Some(true),
            ChatFormatting::Underline => underlined = Some(true),
            ChatFormatting::Italic => italic = Some(true),
            ChatFormatting::Reset => return Style::EMPTY,
            _ => color = TextColor::from_legacy_format(format),
        }
        Style::create(
            color,
            self.shadow_color,
            bold,
            italic,
            underlined,
            strikethrough,
            obfuscated,
            self.click_event.clone(),
            self.hover_event.clone(),
            self.insertion.clone(),
            self.font.clone(),
        )
    }

    /// `Style.applyLegacyFormat(ChatFormatting)` — like `applyFormat` but a
    /// color also clears the style flags.
    pub fn apply_legacy_format(&self, format: ChatFormatting) -> Style {
        let mut color = self.color;
        let mut bold = self.bold;
        let mut italic = self.italic;
        let mut strikethrough = self.strikethrough;
        let mut underlined = self.underlined;
        let mut obfuscated = self.obfuscated;
        match format {
            ChatFormatting::Obfuscated => obfuscated = Some(true),
            ChatFormatting::Bold => bold = Some(true),
            ChatFormatting::Strikethrough => strikethrough = Some(true),
            ChatFormatting::Underline => underlined = Some(true),
            ChatFormatting::Italic => italic = Some(true),
            ChatFormatting::Reset => return Style::EMPTY,
            _ => {
                obfuscated = Some(false);
                bold = Some(false);
                strikethrough = Some(false);
                underlined = Some(false);
                italic = Some(false);
                color = TextColor::from_legacy_format(format);
            }
        }
        Style::create(
            color,
            self.shadow_color,
            bold,
            italic,
            underlined,
            strikethrough,
            obfuscated,
            self.click_event.clone(),
            self.hover_event.clone(),
            self.insertion.clone(),
            self.font.clone(),
        )
    }

    /// `Style.applyFormats(ChatFormatting...)` — the vararg overload; RESET
    /// anywhere clears to `EMPTY`.
    pub fn apply_formats(&self, formats: &[ChatFormatting]) -> Style {
        let mut color = self.color;
        let mut bold = self.bold;
        let mut italic = self.italic;
        let mut strikethrough = self.strikethrough;
        let mut underlined = self.underlined;
        let mut obfuscated = self.obfuscated;
        for &format in formats {
            match format {
                ChatFormatting::Obfuscated => obfuscated = Some(true),
                ChatFormatting::Bold => bold = Some(true),
                ChatFormatting::Strikethrough => strikethrough = Some(true),
                ChatFormatting::Underline => underlined = Some(true),
                ChatFormatting::Italic => italic = Some(true),
                ChatFormatting::Reset => return Style::EMPTY,
                _ => color = TextColor::from_legacy_format(format),
            }
        }
        Style::create(
            color,
            self.shadow_color,
            bold,
            italic,
            underlined,
            strikethrough,
            obfuscated,
            self.click_event.clone(),
            self.hover_event.clone(),
            self.insertion.clone(),
            self.font.clone(),
        )
    }

    /// `Style.applyTo(Style other)` — this Style's non-null fields win.
    pub fn apply_to(&self, other: &Style) -> Style {
        if self == &Style::EMPTY {
            return other.clone();
        }
        if other == &Style::EMPTY {
            return self.clone();
        }
        Style::create(
            self.color.or(other.color),
            self.shadow_color.or(other.shadow_color),
            self.bold.or(other.bold),
            self.italic.or(other.italic),
            self.underlined.or(other.underlined),
            self.strikethrough.or(other.strikethrough),
            self.obfuscated.or(other.obfuscated),
            self.click_event
                .clone()
                .or_else(|| other.click_event.clone()),
            self.hover_event
                .clone()
                .or_else(|| other.hover_event.clone()),
            self.insertion.clone().or_else(|| other.insertion.clone()),
            self.font.clone().or_else(|| other.font.clone()),
        )
    }
}

impl std::fmt::Display for Style {
    /// `Style.toString()` — `{color=...,bold,!italic,...}` in the exact Java
    /// field order, with `!`-prefixed false flags.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut is_not_first = false;
        let mut prepend = |out: &mut std::fmt::Formatter<'_>| -> std::fmt::Result {
            if is_not_first {
                out.write_str(",")?;
            }
            is_not_first = true;
            Ok(())
        };
        f.write_str("{")?;
        if let Some(color) = &self.color {
            prepend(f)?;
            write!(f, "color={}", color)?;
        }
        if let Some(shadow) = self.shadow_color {
            prepend(f)?;
            write!(f, "shadowColor={}", shadow)?;
        }
        for (name, val) in [
            ("bold", self.bold),
            ("italic", self.italic),
            ("underlined", self.underlined),
            ("strikethrough", self.strikethrough),
            ("obfuscated", self.obfuscated),
        ] {
            if let Some(v) = val {
                prepend(f)?;
                if !v {
                    f.write_str("!")?;
                }
                f.write_str(name)?;
            }
        }
        if let Some(click) = &self.click_event {
            prepend(f)?;
            write!(f, "clickEvent={}", click)?;
        }
        if let Some(hover) = &self.hover_event {
            prepend(f)?;
            write!(f, "hoverEvent={}", hover)?;
        }
        if let Some(insertion) = &self.insertion {
            prepend(f)?;
            write!(f, "insertion={}", insertion)?;
        }
        if let Some(font) = &self.font {
            prepend(f)?;
            write!(f, "font={}", font)?;
        }
        f.write_str("}")
    }
}

/// `Style.Serializer` — the JSON `MapCodec`/`Codec` over the 11 optional
/// fields (Java's nested `static class Serializer`).
pub mod serializer {
    use super::*;
    use crate::click_event::ClickEvent;
    use crate::font_description::font_description_codec;
    use crate::hover_event::HoverEvent;
    use crate::text_color::text_color_codec;
    use rivet_serialization::codec::{self, Codec};
    use rivet_serialization::data_result::DataResult;
    use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
    use rivet_serialization::map_codec::MapCodec;
    use rivet_serialization::map_decoder::MapDecoder;
    use rivet_serialization::map_encoder::MapEncoder;
    use std::sync::Arc;

    /// `Style.Serializer.MAP_CODEC`.
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Style, Ops>> {
        Arc::new(StyleMapCodec {
            color: codec::optional_field("color".to_string(), text_color_codec(), false),
            // Java uses `ExtraCodecs.ARGB_COLOR_CODEC` (INT with a VECTOR4F
            // alternative); the vector alternative is deferred, INT is ported.
            shadow_color: codec::optional_field(
                "shadow_color".to_string(),
                codec::int_codec(),
                false,
            ),
            bold: codec::optional_field("bold".to_string(), codec::bool_codec(), false),
            italic: codec::optional_field("italic".to_string(), codec::bool_codec(), false),
            underlined: codec::optional_field("underlined".to_string(), codec::bool_codec(), false),
            strikethrough: codec::optional_field(
                "strikethrough".to_string(),
                codec::bool_codec(),
                false,
            ),
            obfuscated: codec::optional_field("obfuscated".to_string(), codec::bool_codec(), false),
            click_event: codec::optional_field(
                "click_event".to_string(),
                ClickEvent::codec(),
                false,
            ),
            hover_event: codec::optional_field(
                "hover_event".to_string(),
                HoverEvent::codec(),
                false,
            ),
            insertion: codec::optional_field("insertion".to_string(), codec::string_codec(), false),
            font: codec::optional_field("font".to_string(), font_description_codec(), false),
        })
    }

    /// `Style.Serializer.CODEC` — `MAP_CODEC.codec()`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Style, Ops>> {
        rivet_serialization::map_codec::codec_of(map_codec())
    }

    /// The 11-field record codec. `record_builder` only composes up to 4
    /// fields, so this is hand-rolled: each optional field decodes/encodes via
    /// its own `optional_field` `MapCodec`, and the results combine into
    /// `Style::create`. Java's `RecordCodecBuilder` accumulates decode errors
    /// via the applicative; here the first failing field aborts (identical on
    /// the valid/absent path, only the multi-error message differs).
    struct StyleMapCodec<Ops: DynamicOps + 'static> {
        color: Arc<dyn MapCodec<Option<TextColor>, Ops>>,
        shadow_color: Arc<dyn MapCodec<Option<i32>, Ops>>,
        bold: Arc<dyn MapCodec<Option<bool>, Ops>>,
        italic: Arc<dyn MapCodec<Option<bool>, Ops>>,
        underlined: Arc<dyn MapCodec<Option<bool>, Ops>>,
        strikethrough: Arc<dyn MapCodec<Option<bool>, Ops>>,
        obfuscated: Arc<dyn MapCodec<Option<bool>, Ops>>,
        click_event: Arc<dyn MapCodec<Option<ClickEvent>, Ops>>,
        hover_event: Arc<dyn MapCodec<Option<HoverEvent>, Ops>>,
        insertion: Arc<dyn MapCodec<Option<String>, Ops>>,
        font: Arc<dyn MapCodec<Option<FontDescription>, Ops>>,
    }

    impl<Ops: DynamicOps + 'static> std::fmt::Debug for StyleMapCodec<Ops> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "StyleMapCodec")
        }
    }

    impl<Ops: DynamicOps + 'static> Keyable<Ops> for StyleMapCodec<Ops> {
        fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
            let mut keys = self.color.keys(ops);
            keys.extend(self.shadow_color.keys(ops));
            keys.extend(self.bold.keys(ops));
            keys.extend(self.italic.keys(ops));
            keys.extend(self.underlined.keys(ops));
            keys.extend(self.strikethrough.keys(ops));
            keys.extend(self.obfuscated.keys(ops));
            keys.extend(self.click_event.keys(ops));
            keys.extend(self.hover_event.keys(ops));
            keys.extend(self.insertion.keys(ops));
            keys.extend(self.font.keys(ops));
            keys
        }
    }

    impl<Ops: DynamicOps + 'static> MapDecoder<Style, Ops> for StyleMapCodec<Ops> {
        fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Style> {
            let color = self.color.decode(ops, input);
            let shadow = self.shadow_color.decode(ops, input);
            let bold = self.bold.decode(ops, input);
            let italic = self.italic.decode(ops, input);
            let underlined = self.underlined.decode(ops, input);
            let strikethrough = self.strikethrough.decode(ops, input);
            let obfuscated = self.obfuscated.decode(ops, input);
            let click = self.click_event.decode(ops, input);
            let hover = self.hover_event.decode(ops, input);
            let insertion = self.insertion.decode(ops, input);
            let font = self.font.decode(ops, input);
            color.flat_map(move |color| {
                shadow.flat_map(move |shadow| {
                    bold.flat_map(move |bold| {
                        italic.flat_map(move |italic| {
                            underlined.flat_map(move |underlined| {
                                strikethrough.flat_map(move |strikethrough| {
                                    obfuscated.flat_map(move |obfuscated| {
                                        click.flat_map(move |click| {
                                            hover.flat_map(move |hover| {
                                                insertion.flat_map(move |insertion| {
                                                    font.map(move |font| {
                                                        Style::create(
                                                            color,
                                                            shadow,
                                                            bold,
                                                            italic,
                                                            underlined,
                                                            strikethrough,
                                                            obfuscated,
                                                            click,
                                                            hover,
                                                            insertion,
                                                            font.clone(),
                                                        )
                                                    })
                                                })
                                            })
                                        })
                                    })
                                })
                            })
                        })
                    })
                })
            })
        }
    }

    impl<Ops: DynamicOps + 'static> MapEncoder<Style, Ops> for StyleMapCodec<Ops> {
        fn encode(
            &self,
            input: &Style,
            ops: &Ops,
            prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
        ) {
            self.color.encode(&input.color, ops, prefix);
            self.shadow_color.encode(&input.shadow_color, ops, prefix);
            self.bold.encode(&input.bold, ops, prefix);
            self.italic.encode(&input.italic, ops, prefix);
            self.underlined.encode(&input.underlined, ops, prefix);
            self.strikethrough.encode(&input.strikethrough, ops, prefix);
            self.obfuscated.encode(&input.obfuscated, ops, prefix);
            self.click_event.encode(&input.click_event, ops, prefix);
            self.hover_event.encode(&input.hover_event, ops, prefix);
            self.insertion.encode(&input.insertion, ops, prefix);
            self.font.encode(&input.font, ops, prefix);
        }
    }

    impl<Ops: DynamicOps + 'static> MapCodec<Style, Ops> for StyleMapCodec<Ops> {
        fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Style> {
            MapDecoder::decode(self, ops, input)
        }

        fn encode(
            &self,
            input: &Style,
            ops: &Ops,
            prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
        ) {
            MapEncoder::encode(self, input, ops, prefix)
        }
    }
}
