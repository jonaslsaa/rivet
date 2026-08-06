//! Port of `net.minecraft.network.protocol.ping.PingPacketTypes` (issue #86).
//!
//! Java: `PingPacketTypes.java` in `working/Paper`. The two ping-packet
//! discriminators. Network ids are protocol-local (play/status).

use crate::protocol::packet_type::PacketType;

/// `PingPacketTypes.CLIENTBOUND_PONG_RESPONSE`.
pub fn clientbound_pong_response() -> PacketType {
    PacketType::clientbound("pong_response")
}
/// `PingPacketTypes.SERVERBOUND_PING_REQUEST`.
pub fn serverbound_ping_request() -> PacketType {
    PacketType::serverbound("ping_request")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(clientbound_pong_response().flow(), PacketFlow::Clientbound);
        assert_eq!(
            clientbound_pong_response().id().to_string(),
            "minecraft:pong_response"
        );
        assert_eq!(serverbound_ping_request().flow(), PacketFlow::Serverbound);
        assert_eq!(
            serverbound_ping_request().id().to_string(),
            "minecraft:ping_request"
        );
    }
}
