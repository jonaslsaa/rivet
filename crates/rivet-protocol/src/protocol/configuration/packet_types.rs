//! Port of `net.minecraft.network.protocol.configuration.ConfigurationPacketTypes`
//! (issue #109).
//!
//! Java: `ConfigurationPacketTypes.java` in `working/Paper`. The configuration
//! packet discriminators — the values `ConfigurationProtocols` registers in
//! `addPacket` order. Network ids are protocol-local (the generated table #50
//! pins the vanilla `addPacket` order); this module defines only the
//! discriminator values, like `common::packet_types` / `login::packet_types`
//! for their packages.
//!
//! Every constant is plain data (a `PacketType` is a `(flow, identifier)`
//! pair), so the full set is present; the bodies land with their own units
//! (`reset_chat`/`code_of_conduct`/`accept_code_of_conduct` are not on the M1
//! offline join path — the configuration listener decodes them and ignores, #236).

use crate::protocol::packet_type::PacketType;

/// `ConfigurationPacketTypes.CLIENTBOUND_CODE_OF_CONDUCT`.
pub fn clientbound_code_of_conduct() -> PacketType {
    PacketType::clientbound("code_of_conduct")
}
/// `ConfigurationPacketTypes.CLIENTBOUND_FINISH_CONFIGURATION`.
pub fn clientbound_finish_configuration() -> PacketType {
    PacketType::clientbound("finish_configuration")
}
/// `ConfigurationPacketTypes.CLIENTBOUND_REGISTRY_DATA`.
pub fn clientbound_registry_data() -> PacketType {
    PacketType::clientbound("registry_data")
}
/// `ConfigurationPacketTypes.CLIENTBOUND_RESET_CHAT`.
pub fn clientbound_reset_chat() -> PacketType {
    PacketType::clientbound("reset_chat")
}
/// `ConfigurationPacketTypes.CLIENTBOUND_SELECT_KNOWN_PACKS`.
pub fn clientbound_select_known_packs() -> PacketType {
    PacketType::clientbound("select_known_packs")
}
/// `ConfigurationPacketTypes.CLIENTBOUND_UPDATE_ENABLED_FEATURES`.
pub fn clientbound_update_enabled_features() -> PacketType {
    PacketType::clientbound("update_enabled_features")
}
/// `ConfigurationPacketTypes.SERVERBOUND_ACCEPT_CODE_OF_CONDUCT`.
pub fn serverbound_accept_code_of_conduct() -> PacketType {
    PacketType::serverbound("accept_code_of_conduct")
}
/// `ConfigurationPacketTypes.SERVERBOUND_FINISH_CONFIGURATION`.
pub fn serverbound_finish_configuration() -> PacketType {
    PacketType::serverbound("finish_configuration")
}
/// `ConfigurationPacketTypes.SERVERBOUND_SELECT_KNOWN_PACKS`.
pub fn serverbound_select_known_packs() -> PacketType {
    PacketType::serverbound("select_known_packs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(
            clientbound_finish_configuration().flow(),
            PacketFlow::Clientbound
        );
        assert_eq!(
            clientbound_finish_configuration().id().to_string(),
            "minecraft:finish_configuration"
        );
        assert_eq!(
            serverbound_select_known_packs().flow(),
            PacketFlow::Serverbound
        );
        assert_eq!(
            serverbound_select_known_packs().id().to_string(),
            "minecraft:select_known_packs"
        );
        assert_eq!(
            clientbound_registry_data().id().to_string(),
            "minecraft:registry_data"
        );
        assert_eq!(
            clientbound_update_enabled_features().id().to_string(),
            "minecraft:update_enabled_features"
        );
        // The clientbound/serverbound finish discriminators never collide.
        assert_ne!(
            clientbound_finish_configuration(),
            serverbound_finish_configuration()
        );
    }
}
