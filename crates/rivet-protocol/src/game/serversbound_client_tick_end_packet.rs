//! Port of `net.minecraft.network.protocol.game.ServerboundClientTickEndPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundClientTickEndPacket.java`. A fieldless record whose
//! `STREAM_CODEC = StreamCodec.unit(INSTANCE)` — encodes nothing, so the wire
//! body is zero bytes. `handle` is a documented STUB (the
//! `tickEndEvent`/`receivedMovementThisTick` handling is server-side, #158/M3).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundClientTickEndPacket.INSTANCE` — the single, fieldless instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundClientTickEndPacket;

impl std::fmt::Display for ServerboundClientTickEndPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Java class name, so the `StreamCodec.unit` mismatch message reads
        // like the JVM's `IllegalStateException`.
        f.write_str("ServerboundClientTickEndPacket")
    }
}

impl Packet for ServerboundClientTickEndPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("client_tick_end")
    }
}

/// `STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`. Encode writes nothing (a
/// mismatched encode panics with `unit`'s Java `IllegalStateException`
/// message); decode returns the instance.
pub fn client_tick_end_codec() -> StreamCodec<FriendlyByteBuf, ServerboundClientTickEndPacket> {
    crate::codec::unit(ServerboundClientTickEndPacket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    #[test]
    fn unit_codec_encodes_nothing_and_decodes_instance() {
        let codec = client_tick_end_codec();
        let mut out = buf();
        codec
            .encode(&mut out, &ServerboundClientTickEndPacket)
            .unwrap();
        assert!(out.into_inner().is_empty(), "0-byte body");

        let mut input = buf();
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            ServerboundClientTickEndPacket
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn packet_type_is_client_tick_end() {
        assert_eq!(
            ServerboundClientTickEndPacket.packet_type(),
            PacketType::serverbound("client_tick_end")
        );
    }

    #[test]
    fn display_is_the_java_class_name() {
        // `StreamCodec.unit`'s mismatch panic is `IllegalStateException("Can't
        // encode '{value}', expected '{instance}'")`, and both slots render via
        // `Display`; the class name must read like the JVM's. (A unit struct has
        // one value, so the mismatch branch is unreachable in practice.)
        assert_eq!(
            ServerboundClientTickEndPacket.to_string(),
            "ServerboundClientTickEndPacket"
        );
    }
}
