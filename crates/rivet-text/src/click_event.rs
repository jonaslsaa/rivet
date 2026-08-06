//! STUB(mc.text.click_event) — `net.minecraft.network.chat.ClickEvent`.
//!
//! The full model is an interface dispatcing on `ClickEvent.Action`
//! (`open_url`/`run_command`/`copy_to_clipboard`/...) with a per-action record
//! and `MapCodec`. A faithful port needs the `ExtraCodecs` URI/positive-int/
//! chat-string codecs, and the `Custom` action needs `Identifier` + NBT `Tag`
//! while `ShowDialog` needs `Holder<Dialog>` — the latter two are unreachable
//! from `rivet-text` without the Cargo-cycle-forbidden `rivet-registry`/NBT
//! deps. The whole action model is deferred as a unit to a later epic #12
//! slice rather than ported piecemeal. This slice only needs the *value type*
//! so `Style` can hold an event; `CODEC` errors when actually exercised (a
//! component whose JSON carries a `click_event` field fails to decode, which
//! is the honest behaviour until the real port lands).

use rivet_serialization::DynamicOps;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::decoder;
use rivet_serialization::encoder;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.ClickEvent` (value placeholder).
///
/// STUB: the concrete `Action` variants and their codecs are not ported. The
/// struct is opaque so `Style` can carry the field with the same shape as Java.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickEvent;

impl ClickEvent {
    /// `ClickEvent.CODEC` — STUB: a codec that errors on both halves. The
    /// `optionalFieldOf("click_event")` in `Style.Serializer` therefore only
    /// surfaces an error when a component actually carries the field.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ClickEvent, Ops>> {
        codec::of(
            encoder::error("STUB: ClickEvent codec not ported (epic #12)".to_string()),
            decoder::error("STUB: ClickEvent codec not ported (epic #12)".to_string()),
            "ClickEvent[STUB]".to_string(),
        )
    }
}

impl std::fmt::Display for ClickEvent {
    /// `ClickEvent.toString()`. The concrete `Action` model is deferred, so
    /// this is the placeholder `"ClickEvent"` (only `Style`'s `Display` needs
    /// the trait).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClickEvent")
    }
}
