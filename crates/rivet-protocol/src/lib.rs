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
//!   mc.network.framing -> varint21_frame_decoder, varint21_length_field_prepender
//!   mc.network.codec   -> codec (StreamCodec family, ByteBufCodecs, IdDispatchCodec)
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

pub mod codec;
pub mod friendly_byte_buf;
pub mod utf8_string;
pub mod var_int;
pub mod var_long;
pub mod varint21_frame_decoder;
pub mod varint21_length_field_prepender;
