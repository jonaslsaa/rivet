//! Port of `net.minecraft.network.protocol.login.LoginPacketTypes` (issue #99).
//!
//! Java: `LoginPacketTypes.java` in `working/Paper`. The login packet
//! discriminators — the values `LoginProtocols` registers in `addPacket`
//! order. Network ids are protocol-local (`ProtocolInfoBuilder.addPacket`
//! registration order, pinned by the generated table #50); this module defines
//! only the discriminator values, like `common::packet_types` /
//! `cookie::packet_types` for their packages.
//!
//! The RSA challenge pair — `CLIENTBOUND_HELLO`/`SERVERBOUND_KEY` — is the
//! online-auth path: M1 runs offline (`usesAuthentication()` false), so
//! `ServerLoginPacketListenerImpl.handleHello` never sends `ClientboundHello`
//! and the client never replies with `ServerboundKey`. The discriminators are
//! plain data and live here like every other `*PacketTypes` constant; the
//! packet bodies defer with #88.

use crate::protocol::packet_type::PacketType;

/// `LoginPacketTypes.CLIENTBOUND_CUSTOM_QUERY`.
pub fn clientbound_custom_query() -> PacketType {
    PacketType::clientbound("custom_query")
}
/// `LoginPacketTypes.CLIENTBOUND_LOGIN_FINISHED`.
pub fn clientbound_login_finished() -> PacketType {
    PacketType::clientbound("login_finished")
}
/// `LoginPacketTypes.CLIENTBOUND_HELLO`.
pub fn clientbound_hello() -> PacketType {
    PacketType::clientbound("hello")
}
/// `LoginPacketTypes.CLIENTBOUND_LOGIN_COMPRESSION`.
pub fn clientbound_login_compression() -> PacketType {
    PacketType::clientbound("login_compression")
}
/// `LoginPacketTypes.CLIENTBOUND_LOGIN_DISCONNECT`.
pub fn clientbound_login_disconnect() -> PacketType {
    PacketType::clientbound("login_disconnect")
}
/// `LoginPacketTypes.SERVERBOUND_CUSTOM_QUERY_ANSWER`.
pub fn serverbound_custom_query_answer() -> PacketType {
    PacketType::serverbound("custom_query_answer")
}
/// `LoginPacketTypes.SERVERBOUND_HELLO`.
pub fn serverbound_hello() -> PacketType {
    PacketType::serverbound("hello")
}
/// `LoginPacketTypes.SERVERBOUND_KEY`.
pub fn serverbound_key() -> PacketType {
    PacketType::serverbound("key")
}
/// `LoginPacketTypes.SERVERBOUND_LOGIN_ACKNOWLEDGED`.
pub fn serverbound_login_acknowledged() -> PacketType {
    PacketType::serverbound("login_acknowledged")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(serverbound_hello().flow(), PacketFlow::Serverbound);
        assert_eq!(serverbound_hello().id().to_string(), "minecraft:hello");
        assert_eq!(
            serverbound_login_acknowledged().id().to_string(),
            "minecraft:login_acknowledged"
        );
        assert_eq!(serverbound_key().id().to_string(), "minecraft:key");
        assert_eq!(clientbound_login_finished().flow(), PacketFlow::Clientbound);
        assert_eq!(
            clientbound_login_finished().id().to_string(),
            "minecraft:login_finished"
        );
        assert_eq!(
            clientbound_login_compression().id().to_string(),
            "minecraft:login_compression"
        );
        assert_eq!(clientbound_hello().id().to_string(), "minecraft:hello");
        assert_eq!(
            clientbound_login_disconnect().id().to_string(),
            "minecraft:login_disconnect"
        );
        assert_eq!(
            clientbound_custom_query().id().to_string(),
            "minecraft:custom_query"
        );
        assert_eq!(
            serverbound_custom_query_answer().id().to_string(),
            "minecraft:custom_query_answer"
        );
    }
}
