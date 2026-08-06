//! Port of `net.minecraft.network.protocol.common.ClientboundPingPacket`
//! (issue #86).
//!
//! Java: `ClientboundPingPacket.java` in `working/Paper`. An `int` id; the
//! client replies with `ServerboundPongPacket`. Registered in play and
//! configuration clientbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_ping;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ClientboundPingPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundPingPacket {
    id: i32,
}

impl ClientboundPingPacket {
    /// `new ClientboundPingPacket(int id)`.
    pub fn new(id: i32) -> Self {
        ClientboundPingPacket { id }
    }

    /// `ClientboundPingPacket.getId()`.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// `ClientboundPingPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPingPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundPingPacket| {
                output.write_int(value.id);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ClientboundPingPacket::new(input.read_int())),
        )
    }
}

impl Packet for ClientboundPingPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_ping()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn golden_wire_bytes() {
        // Java `writeInt(305419896)` -> 4 bytes BE.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundPingPacket::stream_codec()
            .encode(&mut out, &ClientboundPingPacket::new(305419896))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn round_trips() {
        let packet = ClientboundPingPacket::new(-1);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundPingPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundPingPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
