//! Port of `net.minecraft.network.codec` — the protocol stream-codec family
//! (issue #83, registry-independent slice).
//!
//! Java classes, one module each:
//! - [`StreamCodec`] — `StreamCodec`/`CodecError`/`CodecOperation` and the
//!   combinators (`of`, `of_member`, `unit`, `map`, `dispatch`, `apply`,
//!   `recursive`, `composite_1`..`composite_12`). The `StreamEncoder`/
//!   `StreamDecoder`/`StreamMemberEncoder` traits live in their own thin
//!   modules below.
//! - [`StreamDecoder`], [`StreamEncoder`], [`StreamMemberEncoder`] — the three
//!   Java interfaces.
//! - [`byte_buf_codecs`] — `ByteBufCodecs` primitives and combinator functions
//!   (registry-independent).
//! - [`registry_byte_buf_codecs`] — the registry-aware `ByteBufCodecs` methods
//!   (`registry`, `holderRegistry`, `holder`, `holderSet`, #126 phase G) plus the
//!   key `StreamCodec`s they compose over (`Identifier`/`ResourceKey`/`TagKey`/
//!   `BlockPos`/`GlobalPos`), all over [`RegistryFriendlyByteBuf`].
//! - [`IdDispatchCodec`] — `IdDispatchCodec` + its builder.
//!
//! The remaining registry-dependent `ByteBufCodecs` methods
//! (`registryFriendlyLengthPrefixed`, `fromCodec*`) and the authlib/JOML/Gson
//! value types are blocked on later units and are not present here (documented
//! in the `byte_buf_codecs` module). RivetTodo(#126): the registry-wired
//! `registryFriendlyLengthPrefixed`/`fromCodec*` variants defer with the holder
//! codecs. `Packet.codec` is [`codec`].
//!
//! Name shadowing note: at this root, `map` is the `StreamCodec.map` value
//! mapper combinator (the explicit re-export below). The `ByteBufCodecs.map`
//! collection codec stays reachable class-qualified as
//! [`byte_buf_codecs::map`], exactly mirroring Java's `ByteBufCodecs.map`
//! static.

pub mod byte_buf_codecs;
mod id_dispatch_codec;
pub mod registry_byte_buf_codecs;
mod stream_codec;
mod stream_decoder;
mod stream_encoder;
mod stream_member_encoder;

pub use byte_buf_codecs::*;
pub use id_dispatch_codec::{Builder, IdDispatchCodec, builder};
pub use stream_codec::{
    CodecError, CodecOperation, Composite1, Composite2, Composite3, Composite4, Composite5,
    Composite6, Composite7, Composite8, Composite9, Composite10, Composite11, Composite12,
    StreamCodec, StreamCodecDyn, apply, codec, composite_1, composite_2, composite_3, composite_4,
    composite_5, composite_6, composite_7, composite_8, composite_9, composite_10, composite_11,
    composite_12, dispatch, map, of, of_member, recursive, unit,
};
pub use stream_decoder::StreamDecoder;
pub use stream_encoder::StreamEncoder;
pub use stream_member_encoder::StreamMemberEncoder;
