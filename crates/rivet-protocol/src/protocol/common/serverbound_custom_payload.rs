//! Port of `net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket`
//! (issue #86).
//!
//! Java: `ServerboundCustomPayloadPacket.java` in `working/Paper`. A
//! [`CustomPacketPayload`] dispatched by its type id.
//!
//! CraftBukkit overrides Java's known-types list with an empty one, so every
//! serverbound payload id falls back to `DiscardedPayload` — the payload body
//! is preserved raw up to `MAX_PAYLOAD_SIZE = 32767` (smaller than the
//! clientbound 1 MiB cap).
//!
//! [`CustomPacketPayload`]: crate::protocol::common::custom::CustomPacketPayload

use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::custom::CustomPacketPayload;
use crate::protocol::common::packet_types::serverbound_custom_payload;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundCustomPayloadPacket.MAX_PAYLOAD_SIZE`.
pub const MAX_PAYLOAD_SIZE: i32 = 32767;

/// `net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerboundCustomPayloadPacket {
    payload: CustomPacketPayload,
}

impl ServerboundCustomPayloadPacket {
    /// `new ServerboundCustomPayloadPacket(CustomPacketPayload payload)`.
    pub fn new(payload: CustomPacketPayload) -> Self {
        ServerboundCustomPayloadPacket { payload }
    }

    /// `ServerboundCustomPayloadPacket.payload()`.
    pub fn payload(&self) -> &CustomPacketPayload {
        &self.payload
    }

    /// `ServerboundCustomPayloadPacket.STREAM_CODEC` — the empty known-types
    /// list is `Collections.emptyList()` (CraftBukkit's "treat all serverbound
    /// payloads the same"), so every id falls back to `DiscardedPayload`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundCustomPayloadPacket> {
        map(
            CustomPacketPayload::codec(&[], MAX_PAYLOAD_SIZE),
            |p: &CustomPacketPayload| ServerboundCustomPayloadPacket::new(p.clone()),
            |p: &ServerboundCustomPayloadPacket| p.payload.clone(),
        )
    }
}

impl Packet for ServerboundCustomPayloadPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_custom_payload()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use crate::protocol::common::custom::DiscardedPayload;
    use bytes::BytesMut;
    use rivet_registry::Identifier;

    #[test]
    fn stream_codec_preserves_unknown_payload() {
        // With an empty known-types list, a serverbound payload like
        // `minecraft:register` is kept as DiscardedPayload.
        let packet = ServerboundCustomPayloadPacket::new(CustomPacketPayload::Discarded(
            DiscardedPayload::new(Identifier::with_default_namespace("register"), vec![1u8, 2]),
        ));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundCustomPayloadPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ServerboundCustomPayloadPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        match decoded.payload() {
            CustomPacketPayload::Discarded(d) => {
                assert_eq!(d.id(), &Identifier::with_default_namespace("register"));
                assert_eq!(d.data(), &[1u8, 2]);
            }
            other => panic!("expected Discarded, got {other:?}"),
        }
    }

    #[test]
    fn malformed_key_errors_not_panics() {
        // A hostile `minecraft:aA` key is `Err` (Java `IdentifierException`)
        // through the dispatch codec — not a panic.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:aA");
        out.write_bytes(&[1u8, 2]);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ServerboundCustomPayloadPacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(
            err.message,
            "Non [a-z0-9/._-] character in path of location: minecraft:aA"
        );
    }

    #[test]
    fn serverbound_brand_payload_decodes_as_discarded() {
        // With the empty known-types list, a serverbound `minecraft:brand` is
        // kept as DiscardedPayload (raw bytes: varint length + brand string),
        // exactly like every other id — CraftBukkit treats them all the same.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:brand");
        out.write_bytes(b"\x05Paper");
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = ServerboundCustomPayloadPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        match decoded.payload() {
            CustomPacketPayload::Discarded(d) => {
                assert_eq!(d.id(), &Identifier::with_default_namespace("brand"));
                assert_eq!(d.data(), &b"\x05Paper".to_vec());
            }
            other => panic!("expected Discarded, got {other:?}"),
        }
    }
}
