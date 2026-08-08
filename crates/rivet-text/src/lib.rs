//! Port of `net.minecraft.network.chat` components and `net.minecraft.ChatFormatting`:
//! the `Component`/`MutableComponent` value model, `Style`, text colors, the
//! contents variants, the `ComponentSerialization` JSON codec, the
//! `network.chat.numbers` `NumberFormat` model, and the `locale` translation
//! surface (M1.1 slice of epic #12).

pub mod click_event;
pub mod component;
pub mod component_contents;
pub mod component_serialization;
pub mod contents;
pub mod font_description;
pub mod hover_event;
pub mod locale;
pub mod numbers;
pub mod style;
pub mod text_color;

pub use click_event::ClickEvent;
pub use component::Component;
pub use component_contents::ComponentContents;
pub use font_description::FontDescription;
pub use hover_event::HoverEvent;
pub use locale::Language;
pub use numbers::NumberFormat;
pub use rivet_core::ChatFormatting;
pub use style::Style;
pub use text_color::TextColor;

#[cfg(test)]
mod tests;
