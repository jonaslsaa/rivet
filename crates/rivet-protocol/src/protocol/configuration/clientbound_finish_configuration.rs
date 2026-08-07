//! Port of `net.minecraft.network.protocol.configuration.ClientboundFinishConfigurationPacket`
//! (issue #109).
//!
//! Java: `ClientboundFinishConfigurationPacket.java` in `working/Paper`. The
//! fieldless packet the server sends when the configuration phase is done;
//! `STREAM_CODEC = StreamCodec.unit(INSTANCE)` (a 0-byte body). `isTerminal()`
//! is true (the inbound protocol switches to `GameProtocols.CLIENTBOUND`).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packet_types::clientbound_finish_configuration;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundFinishConfigurationPacket.INSTANCE` — the single, fieldless
/// instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundFinishConfigurationPacket;

impl std::fmt::Display for ClientboundFinishConfigurationPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Java class name, so the `StreamCodec.unit` mismatch message reads
        // like the JVM's `IllegalStateException`.
        f.write_str("ClientboundFinishConfigurationPacket")
    }
}

impl Packet for ClientboundFinishConfigurationPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_finish_configuration()
    }

    fn is_terminal(&self) -> bool {
        true
    }
}

/// `ClientboundFinishConfigurationPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
/// Encode writes nothing; decode returns the instance.
pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundFinishConfigurationPacket> {
    crate::codec::unit(ClientboundFinishConfigurationPacket)
}
