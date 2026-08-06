//! STUB(mc.text.hover_event) — `net.minecraft.network.chat.HoverEvent`.
//!
//! The full model dispatches on `HoverEvent.Action` (`show_text`/`show_item`/
//! `show_entity`): `ShowText` needs only `ComponentSerialization.CODEC`, but
//! `ShowItem` needs `ItemStackTemplate` and `ShowEntity` needs `UUIDUtil` +
//! the `EntityType` registry. The action model is deferred as a unit to a
//! later epic #12 slice rather than ported piecemeal (`ShowText` alone would
//! still leave the dispatch `MapCodec` erroring on the other two actions).
//! This slice only needs the *value type* so `Style` can hold an event; `CODEC`
//! errors when actually exercised (a component whose JSON carries a
//! `hover_event` field fails to decode, which is the honest behaviour until
//! the real port lands).

use rivet_serialization::DynamicOps;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::decoder;
use rivet_serialization::encoder;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.HoverEvent` (value placeholder).
///
/// STUB: the concrete `Action` variants and their codecs are not ported. The
/// struct is opaque so `Style` can carry the field with the same shape as Java.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverEvent;

impl HoverEvent {
    /// `HoverEvent.CODEC` — STUB: a codec that errors on both halves. The
    /// `optionalFieldOf("hover_event")` in `Style.Serializer` therefore only
    /// surfaces an error when a component actually carries the field.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<HoverEvent, Ops>> {
        codec::of(
            encoder::error("STUB: HoverEvent codec not ported (epic #12)".to_string()),
            decoder::error("STUB: HoverEvent codec not ported (epic #12)".to_string()),
            "HoverEvent[STUB]".to_string(),
        )
    }
}

impl std::fmt::Display for HoverEvent {
    /// `HoverEvent.toString()`. The concrete `Action` model is deferred, so
    /// this is the placeholder `"HoverEvent"` (only `Style`'s `Display` needs
    /// the trait).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HoverEvent")
    }
}
