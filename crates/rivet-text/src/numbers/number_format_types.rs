//! Port of `net.minecraft.network.chat.numbers.NumberFormatTypes` — the
//! `NumberFormat` JSON codec.
//!
//! Java's `MAP_CODEC` is:
//!
//! ```text
//! BuiltInRegistries.NUMBER_FORMAT_TYPE.byNameCodec().dispatchMap(
//!     NumberFormat::type, NumberFormatType::mapCodec)
//! ```
//!
//! i.e. a `KeyDispatchCodec` whose discriminator is the format **type name**
//! (`"blank"` / `"styled"` / `"fixed"`), decoded from the `"type"` field, and
//! whose element codec is the concrete format's `MapCodec`. The port threads
//! the name string exactly as `ComponentSerialization` does for contents (the
//! Java `NUMBER_FORMAT_TYPE` registry's `byNameCodec` maps name -> type; the
//! concrete `MapCodec` is looked up by name here).
//!
//! The concrete codecs mirror Java:
//!
//! - `BlankFormat.TYPE` = `MapCodec.unit(INSTANCE)` — encodes nothing, decodes
//!   the singleton from an empty map.
//! - `StyledFormat.TYPE` = `Style.Serializer.MAP_CODEC.xmap(...)` — the style
//!   serialized inline (a bare `{}` style encodes to an empty object).
//! - `FixedFormat.TYPE` = `ComponentSerialization.CODEC.fieldOf("value")
//!   .xmap(...)` — the `"value"` field.
//!
//! `CODEC` is `MAP_CODEC.codec()`. `STREAM_CODEC` / `OPTIONAL_STREAM_CODEC`
//! (the wire registry + `RegistryFriendlyByteBuf`) live in rivet-protocol and
//! are deferred (epic #12). The `Component` codec `top` is threaded through so
//! `FixedFormat` reuses the single recursive Component graph instead of
//! building a fresh one per encode (Java's `FixedFormat` references the static
//! `ComponentSerialization.CODEC`).

use super::number_format::NumberFormat;
use crate::style::Style;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use std::sync::Arc;

/// `NumberFormatTypes.MAP_CODEC`.
pub fn map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<crate::Component, Ops>>,
) -> Arc<dyn MapCodec<NumberFormat, Ops>> {
    let type_fn = Arc::new(|n: &NumberFormat| DataResult::success(name(n).to_string()));
    let codec_fn = Arc::new(move |name: &String| match name.as_str() {
        "blank" => DataResult::success(blank_codec()),
        "styled" => DataResult::success(styled_codec()),
        "fixed" => DataResult::success(fixed_codec(top.clone())),
        _ => DataResult::error(format!("Unknown element id: {}", name)),
    });
    key_dispatch_codec::dispatch_map("type", codec::string_codec(), type_fn, codec_fn)
}

/// `NumberFormatTypes.CODEC` — `MAP_CODEC.codec()`.
pub fn codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<crate::Component, Ops>>,
) -> Arc<dyn Codec<NumberFormat, Ops>> {
    map_codec_mod::codec_of(map_codec(top))
}

/// The `"type"` discriminator for a format (its registry name). Java dispatches
/// on the `NumberFormatType` object itself; the port maps to the name string.
pub fn name(format: &NumberFormat) -> &'static str {
    format.type_().name()
}

/// `BlankFormat.TYPE.MAP_CODEC` — `MapCodec.unit(BlankFormat.INSTANCE)`.
fn blank_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<NumberFormat, Ops>> {
    map_codec_mod::unit(NumberFormat::Blank)
}

/// `StyledFormat.TYPE.MAP_CODEC` — `Style.Serializer.MAP_CODEC.xmap(
/// StyledFormat::new, StyledFormat::style)`.
fn styled_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<NumberFormat, Ops>> {
    map_codec_mod::xmap(
        crate::style::serializer::map_codec(),
        Arc::new(|style: &Style| NumberFormat::Styled(style.clone())),
        Arc::new(|format: &NumberFormat| match format {
            NumberFormat::Styled(style) => style.clone(),
            _ => panic!("styled codec applied to non-styled format"),
        }),
    )
}

/// `FixedFormat.TYPE.MAP_CODEC` — `ComponentSerialization.CODEC.fieldOf(
/// "value").xmap(FixedFormat::new, FixedFormat::value)`.
fn fixed_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<crate::Component, Ops>>,
) -> Arc<dyn MapCodec<NumberFormat, Ops>> {
    let value_field = codec::field_of(top, "value".to_string());
    map_codec_mod::xmap(
        value_field,
        Arc::new(|c: &crate::Component| NumberFormat::Fixed(c.clone())),
        Arc::new(|format: &NumberFormat| match format {
            NumberFormat::Fixed(component) => component.clone(),
            _ => panic!("fixed codec applied to non-fixed format"),
        }),
    )
}
