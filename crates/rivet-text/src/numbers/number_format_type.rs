//! Port of `net.minecraft.network.chat.numbers.NumberFormatType` — the
//! registered format-type discriminator.
//!
//! Java models each concrete format as an anonymous `NumberFormatType` instance
//! holding its `MapCodec`; `NumberFormatTypes.MAP_CODEC` dispatches on the
//! *type object* via the `NUMBER_FORMAT_TYPE` registry's `byNameCodec`. The
//! port collapses the three registered types into an enum and uses its
//! registry name string as the `"type"` discriminator (mirroring how
//! `ComponentSerialization` threads the contents type name).

/// Port of the three `net.minecraft.network.chat.numbers` format types
/// registered in `NumberFormatTypes.bootstrap` (`blank`, `styled`, `fixed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NumberFormatType {
    Blank,
    Styled,
    Fixed,
}

impl NumberFormatType {
    /// The registry id string registered in `NumberFormatTypes.bootstrap`.
    pub fn name(self) -> &'static str {
        match self {
            NumberFormatType::Blank => "blank",
            NumberFormatType::Styled => "styled",
            NumberFormatType::Fixed => "fixed",
        }
    }
}
