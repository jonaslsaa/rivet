//! Port of `net.minecraft.network.protocol.configuration.ServerboundAcceptCodeOfConductPacket`
//! (issue #236).
//!
//! Java: `ServerboundAcceptCodeOfConductPacket.java` in `working/Paper`. The
//! fieldless packet the client sends to accept the server's code of conduct;
//! `STREAM_CODEC = StreamCodec.unit(INSTANCE)` (a 0-byte body). `handle` routes
//! to `ServerConfigurationPacketListenerImpl.handleAcceptCodeOfConduct`, which
//! `finishCurrentTask(ServerCodeOfConductConfigurationTask.TYPE)` — a task that
//! is never queued in this Paper version (`MinecraftServer.getCodeOfConducts()`
//! is `Map.of()`), so the listener closes on the mismatch exactly like Java's
//! `IllegalStateException`.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packet_types::serverbound_accept_code_of_conduct;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundAcceptCodeOfConductPacket.INSTANCE` — the single, fieldless
/// instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundAcceptCodeOfConductPacket;

impl std::fmt::Display for ServerboundAcceptCodeOfConductPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Java class name, so the `StreamCodec.unit` mismatch message reads
        // like the JVM's `IllegalStateException`.
        f.write_str("ServerboundAcceptCodeOfConductPacket")
    }
}

impl Packet for ServerboundAcceptCodeOfConductPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_accept_code_of_conduct()
    }
}

/// `ServerboundAcceptCodeOfConductPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
/// Encode writes nothing; decode returns the instance.
pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundAcceptCodeOfConductPacket> {
    crate::codec::unit(ServerboundAcceptCodeOfConductPacket)
}
