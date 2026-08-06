//! Port of `net.minecraft.network.protocol.cookie.CookiePacketTypes` (issue #86).
//!
//! Java: `CookiePacketTypes.java` in `working/Paper`. The two cookie-packet
//! discriminators. Network ids are protocol-local (play/config/login); only the
//! discriminator values live here.

use crate::protocol::packet_type::PacketType;

/// `CookiePacketTypes.CLIENTBOUND_COOKIE_REQUEST`.
pub fn clientbound_cookie_request() -> PacketType {
    PacketType::clientbound("cookie_request")
}
/// `CookiePacketTypes.SERVERBOUND_COOKIE_RESPONSE`.
pub fn serverbound_cookie_response() -> PacketType {
    PacketType::serverbound("cookie_response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(clientbound_cookie_request().flow(), PacketFlow::Clientbound);
        assert_eq!(
            clientbound_cookie_request().id().to_string(),
            "minecraft:cookie_request"
        );
        assert_eq!(
            serverbound_cookie_response().flow(),
            PacketFlow::Serverbound
        );
        assert_eq!(
            serverbound_cookie_response().id().to_string(),
            "minecraft:cookie_response"
        );
    }
}
