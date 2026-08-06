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
use crate::protocol::stream_codecs::identifier_codec;
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
        let key_codec = identifier_codec();
        let key_codec_decode = key_codec.clone();
        let payload_codec = ClientboundStoreCookiePacket::payload_stream_codec();
        let payload_codec_decode = payload_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ServerboundCookieResponsePacket| {
                key_codec.encode(output, &value.key)?;
                output.write_boolean(value.payload.is_some());
                if let Some(payload) = value.payload.as_ref() {
                    payload_codec.encode(output, payload)?;
                }
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                // The key is read through `identifier_codec` (server-reachable
                // hostile wire): a malformed identifier is `Err`, not a panic.
                let key = key_codec_decode.decode(input)?;
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

    #[test]
    fn malformed_key_errors_not_panics() {
        // A hostile `minecraft:aA` key is a Java `IdentifierException`; the
        // packet codec surfaces it as `Err` (the server closes the connection),
        // not a panic.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:aA");
        out.write_boolean(true);
        out.write_var_int(1);
        out.write_bytes(&[1u8]);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ServerboundCookieResponsePacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            "Non [a-z0-9/._-] character in path of location: minecraft:aA"
        );
    }

    #[test]
    fn oversize_payload_encode_errors_not_panics() {
        // A 5121-byte payload is over `byteArray(5120)`: encode returns `Err`
        // (Java `EncoderException`) instead of panicking through `unwrap`.
        let packet = ServerboundCookieResponsePacket::new(
            Identifier::with_default_namespace("brand"),
            Some(vec![0u8; 5121]),
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        let err = ServerboundCookieResponsePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap_err();
        assert_eq!(
            err.message,
            "ByteArray with size 5121 is bigger than allowed 5120"
        );
    }
}
