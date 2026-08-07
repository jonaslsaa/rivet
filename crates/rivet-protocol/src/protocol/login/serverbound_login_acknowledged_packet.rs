//! Port of `net.minecraft.network.protocol.login.ServerboundLoginAcknowledgedPacket`
//! (issue #99).
//!
//! Java: `ServerboundLoginAcknowledgedPacket.java` in `working/Paper`. The
//! fieldless packet the client sends after `ClientboundLoginFinished` to swap
//! into configuration. `STREAM_CODEC = StreamCodec.unit(INSTANCE)` — a 0-byte
//! body. Registered at login serverbound id 3; `isTerminal()` is true (the
//! inbound protocol switches to `ConfigurationProtocols.SERVERBOUND`).
//!
//! The server-side swap (`handleLoginAcknowledgement` — `setupOutboundProtocol`/
//! `setupInboundProtocol` into `ConfigurationProtocols`) is consumed by the
//! `rivet-server` login listener (`ServerLoginPacketListener`).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::login::packet_types::serverbound_login_acknowledged;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ServerboundLoginAcknowledgedPacket.INSTANCE` — the single, fieldless
/// instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundLoginAcknowledgedPacket;

impl std::fmt::Display for ServerboundLoginAcknowledgedPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Java class name, so the `StreamCodec.unit` mismatch message reads
        // like the JVM's `IllegalStateException`.
        f.write_str("ServerboundLoginAcknowledgedPacket")
    }
}

impl Packet for ServerboundLoginAcknowledgedPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_login_acknowledged()
    }

    fn is_terminal(&self) -> bool {
        true
    }
}

/// `ServerboundLoginAcknowledgedPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
/// Encode writes nothing (a mismatched encode panics with `unit`'s Java
/// `IllegalStateException` message); decode returns the instance.
pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundLoginAcknowledgedPacket> {
    crate::codec::unit(ServerboundLoginAcknowledgedPacket)
}
