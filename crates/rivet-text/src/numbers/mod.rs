//! Port of `net.minecraft.network.chat.numbers` — the `NumberFormat` value
//! model and its `NumberFormatTypes` codecs.
//!
//! `NumberFormat` turns a scoreboard value into a `Component` (epic #12
//! scoreboard wiring). The pinned MC 26.2 source registers exactly three
//! `NumberFormatType`s: `blank`, `styled`, and `fixed` (`NumberFormatTypes
//! .bootstrap`). The dispatch map matches Java:
//!
//! - `MAP_CODEC` = `NUMBER_FORMAT_TYPE.byNameCodec().dispatchMap(type,
//!   NumberFormatType::mapCodec)` — field `"type"` discriminates.
//! - `STREAM_CODEC` / `OPTIONAL_STREAM_CODEC` need a wire registry of format
//!   ids and `RegistryFriendlyByteBuf`; that surface lives in rivet-protocol
//!   and is deferred (epic #12), so the JSON codec only is ported here.
//!
//! The older `IntegerFormat`/`DelegatingFormat`/`SignedFixedFormat` classes
//! were removed from the Java source; `StyledFormat` (a style + `%s`) replaced
//! `IntegerFormat`, `DelegatingFormat` is gone, and `FixedFormat` carries an
//! explicit value `Component` instead of a signed-fixed string.

pub mod number_format;
pub mod number_format_type;
pub mod number_format_types;

pub use number_format::NumberFormat;
pub use number_format_type::NumberFormatType;
