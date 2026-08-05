//! Port of `com.mojang.serialization.codecs` — the concrete codec
//! implementations (`ListCodec`, `PairCodec`, `EitherCodec`, `XorCodec`,
//! `CompoundListCodec`, `OptionalFieldCodec`, `SimpleMapCodec`,
//! `UnboundedMapCodec`).

pub mod compound_list_codec;
pub mod either_codec;
pub mod list_codec;
pub mod optional_field_codec;
pub mod pair_codec;
pub mod simple_map_codec;
pub mod unbounded_map_codec;
pub mod xor_codec;
