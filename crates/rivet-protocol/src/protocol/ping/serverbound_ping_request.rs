//! Port of `net.minecraft.network.protocol.ping.ServerboundPingRequestPacket`
//! (issue #86).
//!
//! Java: `ServerboundPingRequestPacket.java` in `working/Paper`. A `long` time.
//! Registered in play and status serverbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::ping::packet_types::serverbound_ping_request;

/// `net.minecraft.network.protocol.ping.ServerboundPingRequestPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundPingRequestPacket {
    time: i64,
}

impl ServerboundPingRequestPacket {
    /// `new ServerboundPingRequestPacket(long time)`.
    pub fn new(time: i64) -> Self {
        ServerboundPingRequestPacket { time }
    }

    /// `ServerboundPingRequestPacket.time()`.
    pub fn time(&self) -> i64 {
        self.time
    }

    /// `ServerboundPingRequestPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundPingRequestPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ServerboundPingRequestPacket| {
                output.write_long(value.time);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ServerboundPingRequestPacket::new(input.read_long())),
        )
    }
}

impl Packet for ServerboundPingRequestPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_ping_request()
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
        ServerboundPingRequestPacket::stream_codec()
            .encode(&mut out, &ServerboundPingRequestPacket::new(1))
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]
        );
    }

    #[test]
    fn round_trips() {
        let packet = ServerboundPingRequestPacket::new(-99);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundPingRequestPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundPingRequestPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
