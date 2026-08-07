//! Port of `net.minecraft.network.protocol.configuration.ServerboundFinishConfigurationPacket`
//! (issue #109).
//!
//! Java: `ServerboundFinishConfigurationPacket.java` in `working/Paper`. The
//! fieldless packet the client sends when it has finished applying the
//! configuration phase's registry data; `STREAM_CODEC = StreamCodec.unit(INSTANCE)`
//! (a 0-byte body). `isTerminal()` is true (the outbound protocol switches to
//! `GameProtocols.SERVERBOUND`).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packet_types::serverbound_finish_configuration;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundFinishConfigurationPacket.INSTANCE` — the single, fieldless
/// instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundFinishConfigurationPacket;

impl std::fmt::Display for ServerboundFinishConfigurationPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Java class name, so the `StreamCodec.unit` mismatch message reads
        // like the JVM's `IllegalStateException`.
        f.write_str("ServerboundFinishConfigurationPacket")
    }
}

impl Packet for ServerboundFinishConfigurationPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_finish_configuration()
    }

    fn is_terminal(&self) -> bool {
        true
    }
}

/// `ServerboundFinishConfigurationPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
/// Encode writes nothing; decode returns the instance.
pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundFinishConfigurationPacket> {
    crate::codec::unit(ServerboundFinishConfigurationPacket)
}
