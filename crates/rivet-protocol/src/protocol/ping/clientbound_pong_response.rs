//! Port of `net.minecraft.network.protocol.ping.ClientboundPongResponsePacket`
//! (issue #86).
//!
//! Java: `ClientboundPongResponsePacket.java` in `working/Paper`. A `long` time.
//! Registered in play and status clientbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::ping::packet_types::clientbound_pong_response;

/// `net.minecraft.network.protocol.ping.ClientboundPongResponsePacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundPongResponsePacket {
    time: i64,
}

impl ClientboundPongResponsePacket {
    /// `new ClientboundPongResponsePacket(long time)`.
    pub fn new(time: i64) -> Self {
        ClientboundPongResponsePacket { time }
    }

    /// `ClientboundPongResponsePacket.time()`.
    pub fn time(&self) -> i64 {
        self.time
    }

    /// `ClientboundPongResponsePacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPongResponsePacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundPongResponsePacket| {
                output.write_long(value.time);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ClientboundPongResponsePacket::new(input.read_long())),
        )
    }
}

impl Packet for ClientboundPongResponsePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_pong_response()
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
        ClientboundPongResponsePacket::stream_codec()
            .encode(&mut out, &ClientboundPongResponsePacket::new(1))
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]
        );
    }

    #[test]
    fn round_trips() {
        let packet = ClientboundPongResponsePacket::new(-99);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundPongResponsePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundPongResponsePacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
