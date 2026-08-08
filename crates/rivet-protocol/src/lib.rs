//! Minecraft protocol layer: `net.minecraft.network`.
//!
//! Packet-ID tables are **generated, not hand-typed** (PORTING.md): run
//! `tools/rivet-codegen generate` and commit the output. The tables live in
//! `src/generated/`, are compile-time (`phf` maps + `&'static` arrays + enums),
//! and are gated behind the `packets` cargo feature so consumers only pay for
//! the protocol data they use.
//!
//! The hand-ported leaf modules cover the `mc.network.buf` and `mc.network.framing`
//! MANIFEST class-cluster units (one module per Java class, PORTING.md naming):
//!
//!   mc.network.buf     -> friendly_byte_buf, utf8_string, var_int, var_long
//!   mc.network.framing -> varint21_frame_decoder, varint21_length_field_prepender,
//!                         compression_encoder, compression_decoder
//!   mc.network.codec   -> codec (StreamCodec family, ByteBufCodecs, IdDispatchCodec)
//!   mc.network.protocol -> protocol (Packet/PacketType/ProtocolInfoBuilder/
//!                         ProtocolCodecBuilder/SimpleUnboundProtocol/UnboundProtocol/
//!                         BundlerInfo/BundlePacket/BundleDelimiterPacket), issue #84
//!   mc.network          -> protocol_info (ProtocolInfo/Details)
//!
//! `friendly_byte_buf` carries the registry-independent `FriendlyByteBuf`
//! surface (the registry/JOML/codec paths are blocked on later units);
//! `utf8_string` ports `Utf8String` with an exact WHATWG-style UTF-8 decode
//! (see its module docs). Later `mc.network.*` units append their own `mod`
//! declarations here (controller edit between waves).

/// Compile-time packet-ID tables (protocol state -> flow -> packet name -> id).
///
/// Gated behind the `packets` feature; empty when the feature is off.
/// Submodule wiring lives in the generated `generated/mod.rs`.
#[cfg(feature = "packets")]
pub mod generated;

pub mod chat;
pub mod codec;
pub mod compression_decoder;
pub mod compression_encoder;
pub mod friendly_byte_buf;
pub mod registry_friendly_byte_buf;
pub mod syncher;
pub mod utf8_string;
pub mod var_int;
pub mod var_long;
pub mod varint21_frame_decoder;
pub mod varint21_length_field_prepender;

/// `net.minecraft.network.protocol` — packet registration (issue #84):
/// `Packet`/`PacketType`, `ProtocolInfoBuilder`/`ProtocolCodecBuilder`,
/// `SimpleUnboundProtocol`/`UnboundProtocol`, and the bundle trio
/// (`BundlerInfo`/`BundlePacket`/`BundleDelimiterPacket`).
///
/// Gated behind `packets`: the builder assigns network ids in `addPacket`
/// registration order and validates `PacketType.flow` against the protocol's
/// direction, both mirroring `ProtocolInfoBuilder`/`ProtocolCodecBuilder` — and
/// both live on the generated `ConnectionProtocol`/`PacketFlow`/`PacketType`
/// tables that the feature gates.
#[cfg(feature = "packets")]
pub mod protocol;

/// `net.minecraft.network.protocol.game` — the game protocol packet bodies
/// (issue #97). Currently the join-critical serverbound play slice
/// (`mc.network.protocol.game.serverbound`): bodies, `STREAM_CODEC`s, and
/// `Packet::packet_type()`; `handle` (listener dispatch) is deferred with the
/// `ServerGamePacketListener` hierarchy. Lives behind `packets` like
/// `generated`/`protocol` (it depends on the `Packet`/`PacketType` surface).
#[cfg(feature = "packets")]
pub mod game;

/// `net.minecraft.network.ProtocolInfo` (issue #84) — the bound protocol value
/// produced by a template's `bind`: state, direction, the id-dispatch codec, and
/// the optional bundle info. See [`protocol`].
#[cfg(feature = "packets")]
pub mod protocol_info;
