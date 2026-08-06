//! Port of `net.minecraft.network.protocol.common.ServerboundPongPacket`
//! (issue #86).
//!
//! Java: `ServerboundPongPacket.java` in `working/Paper`. An `int` id — the
//! client's reply to `ClientboundPingPacket`. Registered in play and
//! configuration serverbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::serverbound_pong;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ServerboundPongPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundPongPacket {
    id: i32,
}

impl ServerboundPongPacket {
    /// `new ServerboundPongPacket(int id)`.
    pub fn new(id: i32) -> Self {
        ServerboundPongPacket { id }
    }

    /// `ServerboundPongPacket.getId()`.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// `ServerboundPongPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundPongPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ServerboundPongPacket| {
                output.write_int(value.id);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ServerboundPongPacket::new(input.read_int())),
        )
    }
}

impl Packet for ServerboundPongPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_pong()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn golden_wire_bytes() {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundPongPacket::stream_codec()
            .encode(&mut out, &ServerboundPongPacket::new(305419896))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn round_trips() {
        let packet = ServerboundPongPacket::new(7);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundPongPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundPongPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
