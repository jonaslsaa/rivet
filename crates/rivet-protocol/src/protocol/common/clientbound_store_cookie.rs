//! Port of `net.minecraft.network.protocol.common.ClientboundStoreCookiePacket`
//! (issue #86).
//!
//! Java: `ClientboundStoreCookiePacket.java` in `working/Paper`. An
//! `Identifier` key and a `byte[]` payload capped at 5120 bytes
//! (`ByteBufCodecs.byteArray(5120)` — a varint length, then the bytes).
//! Registered in play and configuration clientbound.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_store_cookie;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::identifier_codec;
use rivet_registry::Identifier;

/// `ClientboundStoreCookiePacket.MAX_PAYLOAD_SIZE`.
pub const MAX_PAYLOAD_SIZE: i32 = 5120;

/// `net.minecraft.network.protocol.common.ClientboundStoreCookiePacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundStoreCookiePacket {
    key: Identifier,
    payload: Vec<u8>,
}

impl ClientboundStoreCookiePacket {
    /// `new ClientboundStoreCookiePacket(Identifier key, byte[] payload)`.
    pub fn new(key: Identifier, payload: Vec<u8>) -> Self {
        ClientboundStoreCookiePacket { key, payload }
    }

    /// `ClientboundStoreCookiePacket.key()`.
    pub fn key(&self) -> &Identifier {
        &self.key
    }

    /// `ClientboundStoreCookiePacket.payload()`.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// `ClientboundStoreCookiePacket.PAYLOAD_STREAM_CODEC` —
    /// `ByteBufCodecs.byteArray(5120)`. Shared with `ServerboundCookieResponsePacket`.
    pub fn payload_stream_codec() -> StreamCodec<FriendlyByteBuf, Vec<u8>> {
        byte_buf_codecs::byte_array_max(MAX_PAYLOAD_SIZE)
    }

    /// `ClientboundStoreCookiePacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundStoreCookiePacket> {
        let key_codec = identifier_codec();
        let key_codec_decode = key_codec.clone();
        let payload_codec = Self::payload_stream_codec();
        let payload_codec_decode = payload_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientboundStoreCookiePacket| {
                key_codec.encode(output, &value.key)?;
                payload_codec.encode(output, &value.payload)?;
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let key = key_codec_decode.decode(input)?;
                let payload = payload_codec_decode.decode(input)?;
                Ok(ClientboundStoreCookiePacket::new(key, payload))
            },
        )
    }
}

impl Packet for ClientboundStoreCookiePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_store_cookie()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn key() -> Identifier {
        Identifier::with_default_namespace("brand")
    }

    #[test]
    fn golden_wire_bytes() {
        // Identifier "minecraft:brand" (varint 10 + string), then payload
        // varint 3 + [1,2,3].
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundStoreCookiePacket::stream_codec()
            .encode(
                &mut out,
                &ClientboundStoreCookiePacket::new(key(), vec![1u8, 2, 3]),
            )
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x0Fminecraft:brand\x03\x01\x02\x03".to_vec()
        );
    }

    #[test]
    fn round_trips() {
        let packet = ClientboundStoreCookiePacket::new(key(), vec![9u8; 100]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundStoreCookiePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundStoreCookiePacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }

    #[test]
    fn oversize_payload_errors() {
        let packet = ClientboundStoreCookiePacket::new(key(), vec![0u8; 5121]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        let err = ClientboundStoreCookiePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 5121 is bigger than allowed 5120"
        );
    }

    #[test]
    fn oversize_payload_decode_errors() {
        // A hostile length varint over 5120 on decode is `Err`, not a panic.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:brand");
        out.write_var_int(5121);
        out.write_bytes(&[0u8; 5121]);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ClientboundStoreCookiePacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 5121 is bigger than allowed 5120"
        );
    }

    #[test]
    fn malformed_key_errors_not_panics() {
        // A hostile `minecraft:aA` key is `Err` (Java `IdentifierException`).
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:aA");
        out.write_var_int(0);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ClientboundStoreCookiePacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            "Non [a-z0-9/._-] character in path of location: minecraft:aA"
        );
    }
}
