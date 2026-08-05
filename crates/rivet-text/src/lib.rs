//! Port of Paper's Adventure usage: `net.minecraft.network.chat` components and
//! `net.minecraft.ChatFormatting`.
//!
//! STUB(mc.nbt.text) — currently only the surface `TextComponentTagVisitor`
//! needs. The real rivet-text port (owned by its own manifest unit) replaces
//! these stubs.

pub mod component;
pub mod style;
pub mod text_color;

pub use component::Component;
pub use rivet_core::ChatFormatting;
pub use style::Style;
pub use text_color::TextColor;
