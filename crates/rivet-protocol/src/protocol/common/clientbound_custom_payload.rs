//! Port of `net.minecraft.network.protocol.common.ClientboundCustomPayloadPacket`
//! (issue #86).
//!
//! Java: `ClientboundCustomPayloadPacket.java` in `working/Paper`. A
//! [`CustomPacketPayload`] dispatched by its type id.
//!
//! Java has TWO codecs: `GAMEPLAY_STREAM_CODEC` (over `RegistryFriendlyByteBuf`,
//! which carries registry context the gameplay payload types may need) and
//! `CONFIG_STREAM_CODEC` (over `FriendlyByteBuf`). Only the config variant is
//! portable now — it needs nothing registry-wired, and the vanilla payload types
//! (`BrandPayload`) serialize identically over either buffer. The gameplay
//! variant is deferred with the registry-wired units (#126/#109) that its
//! `RegistryFriendlyByteBuf` requires.
//!
//! Both variants share `MAX_PAYLOAD_SIZE = 1048576` (the fallback
//! `DiscardedPayload` cap).
//!
//! [`CustomPacketPayload`]: crate::protocol::common::custom::CustomPacketPayload

use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::custom::CustomPacketPayload;
use crate::protocol::common::packet_types::clientbound_custom_payload;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundCustomPayloadPacket.MAX_PAYLOAD_SIZE`.
pub const MAX_PAYLOAD_SIZE: i32 = 1048576;

/// `net.minecraft.network.protocol.common.ClientboundCustomPayloadPacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundCustomPayloadPacket {
    payload: CustomPacketPayload,
}

impl ClientboundCustomPayloadPacket {
    /// `new ClientboundCustomPayloadPacket(CustomPacketPayload payload)`.
    pub fn new(payload: CustomPacketPayload) -> Self {
        ClientboundCustomPayloadPacket { payload }
    }

    /// `ClientboundCustomPayloadPacket.payload()`.
    pub fn payload(&self) -> &CustomPacketPayload {
        &self.payload
    }

    /// `ClientboundCustomPayloadPacket.CONFIG_STREAM_CODEC` — the
    /// `FriendlyByteBuf` variant, with the vanilla known types (`BrandPayload`)
    /// and the 1 MiB `DiscardedPayload` fallback.
    pub fn config_stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundCustomPayloadPacket> {
        map(
            CustomPacketPayload::codec(MAX_PAYLOAD_SIZE),
            |p: &CustomPacketPayload| ClientboundCustomPayloadPacket::new(p.clone()),
            |p: &ClientboundCustomPayloadPacket| p.payload.clone(),
        )
    }
}

impl Packet for ClientboundCustomPayloadPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_custom_payload()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use crate::protocol::common::custom::BrandPayload;
    use bytes::BytesMut;

    #[test]
    fn config_codec_round_trips_brand() {
        let packet = ClientboundCustomPayloadPacket::new(CustomPacketPayload::Brand(
            BrandPayload::new("Paper".to_string()),
        ));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundCustomPayloadPacket::config_stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        // Id `minecraft:brand`, then utf "Paper".
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x0Fminecraft:brand\x05Paper".to_vec()
        );
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            b"\x0Fminecraft:brand\x05Paper".to_vec().as_slice(),
        ));
        assert_eq!(
            ClientboundCustomPayloadPacket::config_stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
