//! Port of `net.minecraft.network.protocol.status.StatusPacketTypes` (issue #86).
//!
//! Java: `StatusPacketTypes.java` in `working/Paper`. The two status-packet
//! discriminators.

use crate::protocol::packet_type::PacketType;

/// `StatusPacketTypes.CLIENTBOUND_STATUS_RESPONSE`.
pub fn clientbound_status_response() -> PacketType {
    PacketType::clientbound("status_response")
}

/// `StatusPacketTypes.SERVERBOUND_STATUS_REQUEST`.
pub fn serverbound_status_request() -> PacketType {
    PacketType::serverbound("status_request")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(
            clientbound_status_response().flow(),
            PacketFlow::Clientbound
        );
        assert_eq!(
            clientbound_status_response().id().to_string(),
            "minecraft:status_response"
        );
        assert_eq!(serverbound_status_request().flow(), PacketFlow::Serverbound);
        assert_eq!(
            serverbound_status_request().id().to_string(),
            "minecraft:status_request"
        );
    }
}
