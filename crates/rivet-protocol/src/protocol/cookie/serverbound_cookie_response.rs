//! Port of `net.minecraft.network.protocol.cookie.ServerboundCookieResponsePacket`
//! (issue #86).
//!
//! Java: `ServerboundCookieResponsePacket.java` in `working/Paper`. An
//! `Identifier` key and a nullable `byte[]` payload capped at 5120 bytes
//! (`readNullable`/`writeNullable` over `ClientboundStoreCookiePacket.PAYLOAD_STREAM_CODEC`).
//! Registered in play, configuration, and login serverbound.

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::clientbound_store_cookie::ClientboundStoreCookiePacket;
use crate::protocol::cookie::packet_types::serverbound_cookie_response;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::Identifier;

/// `net.minecraft.network.protocol.cookie.ServerboundCookieResponsePacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerboundCookieResponsePacket {
    key: Identifier,
    payload: Option<Vec<u8>>,
}

impl ServerboundCookieResponsePacket {
    /// `new ServerboundCookieResponsePacket(Identifier key, byte @Nullable []
    /// payload)`.
    pub fn new(key: Identifier, payload: Option<Vec<u8>>) -> Self {
        ServerboundCookieResponsePacket { key, payload }
    }

    /// `ServerboundCookieResponsePacket.key()`.
    pub fn key(&self) -> &Identifier {
        &self.key
    }

    /// `ServerboundCookieResponsePacket.payload()`.
    pub fn payload(&self) -> Option<&[u8]> {
        self.payload.as_deref()
    }

    /// `ServerboundCookieResponsePacket.STREAM_CODEC`.
    ///
    /// The nullable payload uses the presence-prefix byte exactly like
    /// `readNullable`/`writeNullable`, but the decode propagates the payload
    /// codec's `Err` (an over-cap byte array is a hostile wire value, not a
    /// panic) — `FriendlyByteBuf::read_nullable` cannot return a `Result`, so
    /// the boolean + payload are read here directly.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundCookieResponsePacket> {
        let payload_codec = ClientboundStoreCookiePacket::payload_stream_codec();
        let payload_codec_decode = payload_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ServerboundCookieResponsePacket| {
                output.write_identifier(&value.key);
                output.write_nullable(value.payload.as_ref(), |out, payload| {
                    payload_codec.encode(out, payload).unwrap();
                });
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let key = input.read_identifier();
                let payload = if input.read_boolean() {
                    Some(payload_codec_decode.decode(input)?)
                } else {
                    None
                };
                Ok(ServerboundCookieResponsePacket::new(key, payload))
            },
        )
    }
}

impl Packet for ServerboundCookieResponsePacket {
    fn packet_type(&self) -> PacketType {
        serverbound_cookie_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn golden_wire_bytes_present() {
        // Identifier "minecraft:brand", then nullable present (true byte) +
        // payload varint 3 + [1,2,3].
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundCookieResponsePacket::stream_codec()
            .encode(
                &mut out,
                &ServerboundCookieResponsePacket::new(
                    Identifier::with_default_namespace("brand"),
                    Some(vec![1u8, 2, 3]),
                ),
            )
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x0Fminecraft:brand\x01\x03\x01\x02\x03".to_vec()
        );
    }

    #[test]
    fn golden_wire_bytes_null_payload() {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundCookieResponsePacket::stream_codec()
            .encode(
                &mut out,
                &ServerboundCookieResponsePacket::new(
                    Identifier::with_default_namespace("brand"),
                    None,
                ),
            )
            .unwrap();
        // Identifier, then nullable absent (false byte).
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x0Fminecraft:brand\x00".to_vec()
        );
    }

    #[test]
    fn round_trips() {
        for payload in [Some(vec![1u8, 2, 3]), None] {
            let packet = ServerboundCookieResponsePacket::new(
                Identifier::with_default_namespace("brand"),
                payload,
            );
            let mut out = FriendlyByteBuf::new(BytesMut::new());
            ServerboundCookieResponsePacket::stream_codec()
                .encode(&mut out, &packet)
                .unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            assert_eq!(
                ServerboundCookieResponsePacket::stream_codec()
                    .decode(&mut input)
                    .unwrap(),
                packet
            );
        }
    }
}
