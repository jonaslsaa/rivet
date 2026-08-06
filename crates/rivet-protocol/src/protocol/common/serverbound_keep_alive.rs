//! Port of `net.minecraft.network.protocol.common.ServerboundKeepAlivePacket`
//! (issue #86).
//!
//! Java: `ServerboundKeepAlivePacket.java` in `working/Paper`. A `long` id — the
//! client's echo of `ClientboundKeepAlivePacket`. Registered in both play and
//! configuration serverbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::serverbound_keep_alive;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ServerboundKeepAlivePacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundKeepAlivePacket {
    id: i64,
}

impl ServerboundKeepAlivePacket {
    /// `new ServerboundKeepAlivePacket(long id)`.
    pub fn new(id: i64) -> Self {
        ServerboundKeepAlivePacket { id }
    }

    /// `ServerboundKeepAlivePacket.getId()`.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// `ServerboundKeepAlivePacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundKeepAlivePacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ServerboundKeepAlivePacket| {
                output.write_long(value.id);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(ServerboundKeepAlivePacket::new(input.read_long())),
        )
    }
}

impl Packet for ServerboundKeepAlivePacket {
    fn packet_type(&self) -> PacketType {
        serverbound_keep_alive()
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
        ServerboundKeepAlivePacket::stream_codec()
            .encode(
                &mut out,
                &ServerboundKeepAlivePacket::new(1234567890123456789),
            )
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![0x11, 0x22, 0x10, 0xf4, 0x7d, 0xe9, 0x81, 0x15]
        );
    }

    #[test]
    fn round_trips() {
        let packet = ServerboundKeepAlivePacket::new(-1);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundKeepAlivePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundKeepAlivePacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
