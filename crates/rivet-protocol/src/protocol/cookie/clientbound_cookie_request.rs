//! Port of `net.minecraft.network.protocol.cookie.ClientboundCookieRequestPacket`
//! (issue #86).
//!
//! Java: `ClientboundCookieRequestPacket.java` in `working/Paper`. An
//! `Identifier` key; the client replies with `ServerboundCookieResponsePacket`.
//! Registered in play, configuration, and login clientbound.

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::cookie::packet_types::clientbound_cookie_request;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::identifier_codec;
use rivet_registry::Identifier;

/// `net.minecraft.network.protocol.cookie.ClientboundCookieRequestPacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundCookieRequestPacket {
    key: Identifier,
}

impl ClientboundCookieRequestPacket {
    /// `new ClientboundCookieRequestPacket(Identifier key)`.
    pub fn new(key: Identifier) -> Self {
        ClientboundCookieRequestPacket { key }
    }

    /// `ClientboundCookieRequestPacket.key()`.
    pub fn key(&self) -> &Identifier {
        &self.key
    }

    /// `ClientboundCookieRequestPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundCookieRequestPacket> {
        let key_codec = identifier_codec();
        let key_codec_decode = key_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientboundCookieRequestPacket| {
                key_codec.encode(output, &value.key)
            },
            move |input: &mut FriendlyByteBuf| {
                Ok(ClientboundCookieRequestPacket::new(
                    key_codec_decode.decode(input)?,
                ))
            },
        )
    }
}

impl Packet for ClientboundCookieRequestPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_cookie_request()
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
        ClientboundCookieRequestPacket::stream_codec()
            .encode(
                &mut out,
                &ClientboundCookieRequestPacket::new(Identifier::with_default_namespace("brand")),
            )
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), b"\x0Fminecraft:brand".to_vec());
    }

    #[test]
    fn round_trips() {
        let packet =
            ClientboundCookieRequestPacket::new(Identifier::with_default_namespace("brand"));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundCookieRequestPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundCookieRequestPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
