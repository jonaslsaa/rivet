//! Port of `net.minecraft.network.protocol.status.ServerboundStatusRequestPacket`
//! (issue #86).
//!
//! Java: `ServerboundStatusRequestPacket.java` in `working/Paper`. A
//! parameterless request; `STREAM_CODEC` is `StreamCodec.unit(INSTANCE)` — the
//! decode produces the singleton, and an encode of any other value panics with
//! Java's `IllegalStateException("Can't encode ...")` (a programmer error).
//! Registered in status serverbound.

use crate::codec::{StreamCodec, unit};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::status::packet_types::serverbound_status_request;

/// `net.minecraft.network.protocol.status.ServerboundStatusRequestPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundStatusRequestPacket;

/// `ServerboundStatusRequestPacket.INSTANCE`.
pub const INSTANCE: ServerboundStatusRequestPacket = ServerboundStatusRequestPacket;

impl std::fmt::Display for ServerboundStatusRequestPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ServerboundStatusRequestPacket")
    }
}

impl ServerboundStatusRequestPacket {
    /// `ServerboundStatusRequestPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundStatusRequestPacket> {
        unit(INSTANCE)
    }
}

impl Packet for ServerboundStatusRequestPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_status_request()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn decodes_to_singleton_without_reading() {
        // `StreamCodec.unit` reads nothing and yields `INSTANCE` (a zero-byte
        // body on the wire).
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        assert_eq!(
            ServerboundStatusRequestPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            INSTANCE
        );
    }

    #[test]
    fn encode_is_a_noop_for_the_singleton() {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundStatusRequestPacket::stream_codec()
            .encode(&mut out, &INSTANCE)
            .unwrap();
        assert!(out.into_inner().is_empty());
    }
}
