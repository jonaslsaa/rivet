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
//! The complete pinned `StyledFormat` surface is ported: `NO_STYLE`,
//! `SIDEBAR_DEFAULT` (RED), and `PLAYER_LIST_DEFAULT` (YELLOW) are the
//! `NumberFormat::NO_STYLE` / `SIDEBAR_DEFAULT` / `PLAYER_LIST_DEFAULT`
//! constants. The pinned 26.2 package contains only `BlankFormat`,
//! `FixedFormat`, `StyledFormat`, `NumberFormat`, `NumberFormatType`, and
//! `NumberFormatTypes`; the historical `IntegerFormat` /
//! `DelegatingFormat` / `SignedFixedFormat` class names do not appear in the
//! pinned source (nor anywhere in Paper's tracked minecraft-source history),
//! so nothing from them is silently omitted.

pub mod number_format;
pub mod number_format_type;
pub mod number_format_types;

pub use number_format::NumberFormat;
pub use number_format_type::NumberFormatType;
