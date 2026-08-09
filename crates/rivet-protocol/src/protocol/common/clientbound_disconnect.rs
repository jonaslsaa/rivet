//! Port of `net.minecraft.network.protocol.common.ClientboundDisconnectPacket`
//! (issue #86, join-path slice).
//!
//! Java: `ClientboundDisconnectPacket.java` in `working/Paper`. Carries a single
//! `Component` (the kick reason) over `ComponentSerialization
//! .TRUSTED_CONTEXT_FREE_STREAM_CODEC` — ported as
//! [`crate::chat::trusted_context_free_component`] (issue #89/#207). Registered
//! in both play (`minecraft:disconnect`, protocol id 32) and configuration
//! (`minecraft:disconnect`, protocol id 2) clientbound — the config+play
//! `disconnect0` join-path kick.

use crate::chat::trusted_context_free_component;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_disconnect;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_text::Component;

/// `net.minecraft.network.protocol.common.ClientboundDisconnectPacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundDisconnectPacket {
    reason: Component,
}

impl ClientboundDisconnectPacket {
    /// `new ClientboundDisconnectPacket(Component reason)`.
    pub fn new(reason: Component) -> Self {
        ClientboundDisconnectPacket { reason }
    }

    /// `ClientboundDisconnectPacket.reason()`.
    pub fn reason(&self) -> &Component {
        &self.reason
    }

    /// `ClientboundDisconnectPacket.STREAM_CODEC` —
    /// `ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC.map(new,
    /// reason)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundDisconnectPacket> {
        let component_codec = trusted_context_free_component();
        let component_codec_decode = component_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientboundDisconnectPacket| {
                component_codec.encode(output, &value.reason)
            },
            move |input: &mut FriendlyByteBuf| {
                Ok(ClientboundDisconnectPacket::new(
                    component_codec_decode.decode(input)?,
                ))
            },
        )
    }
}

impl Packet for ClientboundDisconnectPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_disconnect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn golden_literal_reason_wire_bytes() {
        // `Component.literal("Hi")` collapses to the codec's string branch, so
        // the trusted tag is a bare StringTag (type byte 8), then u16-BE length
        // 2 and the modified-UTF-8 "Hi" payload.
        let packet = ClientboundDisconnectPacket::new(Component::literal("Hi"));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundDisconnectPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), hex("0800024869"));

        let mut input = FriendlyByteBuf::new(BytesMut::from(hex("0800024869").as_slice()));
        assert_eq!(
            ClientboundDisconnectPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn translatable_reason_round_trips_through_full_codec() {
        // The real join-path kick reason (Paper `ServerGamePacketListenerImpl`
        // `multiplayer.disconnect.invalid_player_movement`). A translatable does
        // not collapse to the string branch, so it exercises the full recursive
        // Component codec: a CompoundTag (type byte 10) whose "type" key
        // dispatches back to TranslatableContents on decode.
        let packet = ClientboundDisconnectPacket::new(Component::translatable(
            "multiplayer.disconnect.invalid_player_movement",
        ));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundDisconnectPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 10); // CompoundTag type byte (not StringTag 8)

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            ClientboundDisconnectPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn empty_reason_round_trips() {
        // `Component.empty()` is a PlainTextContents with empty text, which is
        // still plain text: it collapses to the string branch with an empty
        // payload.
        let packet = ClientboundDisconnectPacket::new(Component::empty());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundDisconnectPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 8); // StringTag type byte
        assert_eq!(&bytes[1..3], &[0x00, 0x00]); // u16-BE length 0

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            ClientboundDisconnectPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }

    #[test]
    fn hostile_truncated_component_errors() {
        // A truncated tag body hits the raw buffer's EOF panic (netty
        // IndexOutOfBounds), the established raw-buf contract — not a codec Err.
        let mut input = FriendlyByteBuf::new(BytesMut::from(hex("08").as_slice()));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = ClientboundDisconnectPacket::stream_codec().decode(&mut input);
            }))
            .is_err()
        );
    }

    #[test]
    fn hostile_end_tag_is_codec_error() {
        // `EndTag` (type byte 0) decodes to `None` at the NBT boundary and the
        // trusted tag codec returns `Err("Expected non-null compound tag")` —
        // Java `DecoderException`, not a panic.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0u8].as_slice()));
        let err = ClientboundDisconnectPacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(err.message, "Expected non-null compound tag");
    }

    #[test]
    fn packet_type_matches_vanilla_ids() {
        use crate::generated::packets::configuration::clientbound::PacketType as Config;
        use crate::generated::packets::play::clientbound::PacketType as Play;

        let packet = ClientboundDisconnectPacket::new(Component::empty());
        assert_eq!(packet.packet_type(), clientbound_disconnect());
        assert_eq!(
            clientbound_disconnect().id().to_string(),
            "minecraft:disconnect"
        );

        // Play and configuration both register `minecraft:disconnect` at their
        // own protocol-local id (the `addPacket` index).
        assert_eq!(Play::Disconnect.id(), 32);
        assert_eq!(Config::Disconnect.id(), 2);
    }
}
