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
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundClientTickEndPacket.INSTANCE` — the single, fieldless instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundClientTickEndPacket;

impl Packet for ServerboundClientTickEndPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("client_tick_end")
    }
}

/// `STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`. Encode writes nothing (a
/// mismatched encode panics with `unit`'s Java `IllegalStateException`
/// message); decode returns the instance.
pub fn client_tick_end_codec() -> StreamCodec<FriendlyByteBuf, ServerboundClientTickEndPacket> {
    codec(
        |_value: &ServerboundClientTickEndPacket, _output: &mut FriendlyByteBuf| Ok(()),
        |_input: &mut FriendlyByteBuf| Ok(ServerboundClientTickEndPacket),
    )
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
}
