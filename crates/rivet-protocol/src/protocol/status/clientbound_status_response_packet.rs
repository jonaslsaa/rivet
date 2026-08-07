//! Port of `net.minecraft.network.protocol.status.ClientboundStatusResponsePacket`
//! (issue #86).
//!
//! Java: `ClientboundStatusResponsePacket.java` in `working/Paper`. The status
//! JSON response: `STREAM_CODEC = StreamCodec.composite(
//! lenientJson(32767).apply(fromCodec(OPS, ServerStatus.CODEC)), ...)` where
//! `OPS = RegistryAccess.EMPTY.createSerializationContext(JsonOps.INSTANCE)`.
//! The wire payload is one UTF-8 string (VarInt-prefixed, 32767-char cap)
//! holding the compact JSON of `ServerStatus`.
//!
//! Registered in status clientbound.

use crate::codec::byte_buf_codecs::{self, lenient_json};
use crate::codec::{StreamCodec, apply, composite_1};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::status::packet_types::clientbound_status_response;
use crate::protocol::status::server_status::ServerStatus;
use rivet_serialization::json_ops::JsonOps;

/// `net.minecraft.network.protocol.status.ClientboundStatusResponsePacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundStatusResponsePacket {
    status: ServerStatus,
}

impl ClientboundStatusResponsePacket {
    /// `new ClientboundStatusResponsePacket(ServerStatus status)`.
    pub fn new(status: ServerStatus) -> Self {
        ClientboundStatusResponsePacket { status }
    }

    /// `ClientboundStatusResponsePacket.status()`.
    pub fn status(&self) -> &ServerStatus {
        &self.status
    }

    /// `ClientboundStatusResponsePacket.STREAM_CODEC` — `StreamCodec.composite(
    /// lenientJson(32767).apply(fromCodec(OPS, ServerStatus.CODEC)), ...)`.
    /// `JsonOps::INSTANCE` is the empty-registry serialization context
    /// (Java's `RegistryAccess.EMPTY.createSerializationContext(JsonOps.INSTANCE)`).
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundStatusResponsePacket> {
        let json_codec = apply(
            lenient_json(32767),
            byte_buf_codecs::from_codec(JsonOps::INSTANCE, ServerStatus::codec()),
        );
        composite_1(
            json_codec,
            |p: &ClientboundStatusResponsePacket| p.status.clone(),
            |status: ServerStatus| ClientboundStatusResponsePacket::new(status),
        )
    }
}

impl Packet for ClientboundStatusResponsePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_status_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use crate::protocol::status::{Favicon, Players, ServerStatus, Version};
    use bytes::BytesMut;
    use rivet_text::Component;

    #[test]
    fn round_trips_through_lenient_json() {
        let status = ServerStatus::new(
            Component::literal("A Rivet Server"),
            Some(Players::new(20, 1, Vec::new())),
            Some(Version::new("1.21.4".to_string(), 769)),
            Some(Favicon::new(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a])),
            true,
        );
        let packet = ClientboundStatusResponsePacket::new(status.clone());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundStatusResponsePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let wire = out.into_inner().to_vec();
        // The wire is one UTF-8 string: a VarInt byte length, then the JSON.
        let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
        let decoded = ClientboundStatusResponsePacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
    }

    #[test]
    fn wire_is_a_utf8_string_of_compact_json() {
        let status = ServerStatus::new(Component::literal("Hi"), None, None, None, false);
        let packet = ClientboundStatusResponsePacket::new(status);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundStatusResponsePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let wire = out.into_inner().to_vec();
        // VarInt(0x14) = 20, then the 20-byte compact JSON
        // `{"description":"Hi"}` (a defaulted `enforcesSecureChat` is omitted,
        // as in Java's `lenientOptionalFieldOf("enforcesSecureChat", false)`).
        let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
        let len = input.read_var_int();
        assert_eq!(len, 20);
        let json = input.read_slice(len);
        assert_eq!(
            String::from_utf8(json.to_vec()).unwrap(),
            r#"{"description":"Hi"}"#
        );
    }
}
