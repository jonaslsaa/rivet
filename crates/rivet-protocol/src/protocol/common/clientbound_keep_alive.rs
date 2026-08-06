//! Port of `net.minecraft.network.protocol.common.ClientboundKeepAlivePacket`
//! (issue #86).
//!
//! Java: `ClientboundKeepAlivePacket.java` in `working/Paper`. A `long` id; the
//! client echoes it back in `ServerboundKeepAlivePacket`. Registered in both
//! play and configuration clientbound (`ProtocolInfoBuilder.addPacket` order
//! assigns the protocol-local id).

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_keep_alive;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ClientboundKeepAlivePacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundKeepAlivePacket {
    id: i64,
}

impl ClientboundKeepAlivePacket {
    /// `new ClientboundKeepAlivePacket(long id)`.
    pub fn new(id: i64) -> Self {
        ClientboundKeepAlivePacket { id }
    }

    /// `ClientboundKeepAlivePacket.getId()`.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// `ClientboundKeepAlivePacket.STREAM_CODEC` — `Packet.codec(write, new)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundKeepAlivePacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundKeepAlivePacket| {
                output.write_long(value.id);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ClientboundKeepAlivePacket::new(input.read_long())),
        )
    }
}

impl Packet for ClientboundKeepAlivePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_keep_alive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn golden_wire_bytes() {
        // Java `writeLong(1234567890123456789)` -> 8 bytes BE.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundKeepAlivePacket::stream_codec()
            .encode(
                &mut out,
                &ClientboundKeepAlivePacket::new(1234567890123456789),
            )
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x11, 0x22, 0x10, 0xf4, 0x7d, 0xe9, 0x81, 0x15]
        );
    }

    #[test]
    fn round_trips() {
        let packet = ClientboundKeepAlivePacket::new(-42);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundKeepAlivePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundKeepAlivePacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
