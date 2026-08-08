//! Port of `net.minecraft.network.chat.numbers.NumberFormat`.
//!
//! Java's `NumberFormat` is a sealed-style interface with `format(int)` and a
//! `type()`; the Rust port models it as an enum over the three registered
//! concrete formats. `type()` is `NumberFormatType`-keyed in Java; here each
//! variant maps to a name string via [`NumberFormatTypes::name`], which the
//! dispatch codec uses as the `"type"` discriminator.

use crate::Component;
use crate::numbers::number_format_type::NumberFormatType;

/// Port of `net.minecraft.network.chat.numbers.NumberFormat`.
///
/// `PartialEq` only (no `Eq`): `FixedFormat` wraps a `Component`, which derives
/// `PartialEq` only (Java's `equals` compares the component graph).
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    /// `BlankFormat.INSTANCE` — formats any value to `Component.empty()`.
    Blank,
    /// `StyledFormat(Style)` — formats the value as `Component.literal(Integer
    /// .toString(value)).withStyle(style)`.
    Styled(crate::Style),
    /// `FixedFormat(Component)` — formats any value to `value.copy()`.
    Fixed(Component),
}

impl NumberFormat {
    /// `NumberFormat.format(int)` — the `Component` produced for a score value.
    pub fn format(&self, value: i32) -> Component {
        match self {
            NumberFormat::Blank => Component::empty(),
            NumberFormat::Styled(style) => {
                Component::literal(&value.to_string()).with_style(style.clone())
            }
            NumberFormat::Fixed(component) => component.copy(),
        }
    }

    /// `NumberFormat.type()` — the concrete `NumberFormatType`.
    pub fn type_(&self) -> NumberFormatType {
        match self {
            NumberFormat::Blank => NumberFormatType::Blank,
            NumberFormat::Styled(_) => NumberFormatType::Styled,
            NumberFormat::Fixed(_) => NumberFormatType::Fixed,
        }
    }
}
