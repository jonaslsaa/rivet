//! Port of `net.minecraft.network.protocol.common.CommonPacketTypes` (issue #86).
//!
//! Java: `CommonPacketTypes.java` in `working/Paper`. Each constant is a
//! `PacketType` built by `createClientbound`/`createServerbound` — a flow and a
//! `minecraft:`-namespaced identifier. The network id is protocol-local
//! (`ProtocolInfoBuilder.addPacket` registration order), so the same `PacketType`
//! value is shared across play and configuration; this module only defines the
//! discriminator values.
//!
//! `Identifier` has no const constructor, so each constant is a `fn` returning a
//! fresh `PacketType` (matching `PacketType::serverbound`'s call-time
//! construction — the value is always the same identifier).

use crate::protocol::packet_type::PacketType;

/// `CommonPacketTypes.CLIENTBOUND_CLEAR_DIALOG`.
pub fn clientbound_clear_dialog() -> PacketType {
    PacketType::clientbound("clear_dialog")
}
/// `CommonPacketTypes.CLIENTBOUND_CUSTOM_PAYLOAD`.
pub fn clientbound_custom_payload() -> PacketType {
    PacketType::clientbound("custom_payload")
}
/// `CommonPacketTypes.CLIENTBOUND_CUSTOM_REPORT_DETAILS`.
pub fn clientbound_custom_report_details() -> PacketType {
    PacketType::clientbound("custom_report_details")
}
/// `CommonPacketTypes.CLIENTBOUND_DISCONNECT`.
pub fn clientbound_disconnect() -> PacketType {
    PacketType::clientbound("disconnect")
}
/// `CommonPacketTypes.CLIENTBOUND_KEEP_ALIVE`.
pub fn clientbound_keep_alive() -> PacketType {
    PacketType::clientbound("keep_alive")
}
/// `CommonPacketTypes.CLIENTBOUND_PING`.
pub fn clientbound_ping() -> PacketType {
    PacketType::clientbound("ping")
}
/// `CommonPacketTypes.CLIENTBOUND_RESOURCE_PACK_POP`.
pub fn clientbound_resource_pack_pop() -> PacketType {
    PacketType::clientbound("resource_pack_pop")
}
/// `CommonPacketTypes.CLIENTBOUND_RESOURCE_PACK_PUSH`.
pub fn clientbound_resource_pack_push() -> PacketType {
    PacketType::clientbound("resource_pack_push")
}
/// `CommonPacketTypes.CLIENTBOUND_SERVER_LINKS`.
pub fn clientbound_server_links() -> PacketType {
    PacketType::clientbound("server_links")
}
/// `CommonPacketTypes.CLIENTBOUND_SHOW_DIALOG`.
pub fn clientbound_show_dialog() -> PacketType {
    PacketType::clientbound("show_dialog")
}
/// `CommonPacketTypes.CLIENTBOUND_STORE_COOKIE`.
pub fn clientbound_store_cookie() -> PacketType {
    PacketType::clientbound("store_cookie")
}
/// `CommonPacketTypes.CLIENTBOUND_TRANSFER`.
pub fn clientbound_transfer() -> PacketType {
    PacketType::clientbound("transfer")
}
/// `CommonPacketTypes.CLIENTBOUND_UPDATE_TAGS`.
pub fn clientbound_update_tags() -> PacketType {
    PacketType::clientbound("update_tags")
}
/// `CommonPacketTypes.SERVERBOUND_CLIENT_INFORMATION`.
pub fn serverbound_client_information() -> PacketType {
    PacketType::serverbound("client_information")
}
/// `CommonPacketTypes.SERVERBOUND_CUSTOM_PAYLOAD`.
pub fn serverbound_custom_payload() -> PacketType {
    PacketType::serverbound("custom_payload")
}
/// `CommonPacketTypes.SERVERBOUND_KEEP_ALIVE`.
pub fn serverbound_keep_alive() -> PacketType {
    PacketType::serverbound("keep_alive")
}
/// `CommonPacketTypes.SERVERBOUND_PONG`.
pub fn serverbound_pong() -> PacketType {
    PacketType::serverbound("pong")
}
/// `CommonPacketTypes.SERVERBOUND_RESOURCE_PACK`.
pub fn serverbound_resource_pack() -> PacketType {
    PacketType::serverbound("resource_pack")
}
/// `CommonPacketTypes.SERVERBOUND_CUSTOM_CLICK_ACTION`.
pub fn serverbound_custom_click_action() -> PacketType {
    PacketType::serverbound("custom_click_action")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(clientbound_keep_alive().flow(), PacketFlow::Clientbound);
        assert_eq!(
            clientbound_keep_alive().id().to_string(),
            "minecraft:keep_alive"
        );
        assert_eq!(serverbound_keep_alive().flow(), PacketFlow::Serverbound);
        assert_eq!(
            serverbound_keep_alive().id().to_string(),
            "minecraft:keep_alive"
        );
        // The serverbound constants never collide with the clientbound ones.
        assert_ne!(serverbound_pong(), clientbound_ping());
        assert_eq!(
            serverbound_custom_click_action().id().to_string(),
            "minecraft:custom_click_action"
        );
        assert_eq!(
            clientbound_custom_report_details().id().to_string(),
            "minecraft:custom_report_details"
        );
    }
}
