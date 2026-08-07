//! Port of `net.minecraft.network.protocol.game.GamePacketTypes` (issue #94,
//! chunk-send slice).
//!
//! Java: `GamePacketTypes.java` in `working/Paper`. Each constant is a
//! `PacketType` built by `createClientbound`/`createServerbound`. Network ids are
//! protocol-local (`ProtocolInfoBuilder.addPacket` registration order); only the
//! discriminator values live here.

use crate::protocol::packet_type::PacketType;

/// `GamePacketTypes.CLIENTBOUND_CHUNK_BATCH_FINISHED`.
pub fn clientbound_chunk_batch_finished() -> PacketType {
    PacketType::clientbound("chunk_batch_finished")
}
/// `GamePacketTypes.CLIENTBOUND_CHUNK_BATCH_START`.
pub fn clientbound_chunk_batch_start() -> PacketType {
    PacketType::clientbound("chunk_batch_start")
}
/// `GamePacketTypes.CLIENTBOUND_CHUNKS_BIOMES`.
pub fn clientbound_chunks_biomes() -> PacketType {
    PacketType::clientbound("chunks_biomes")
}
/// `GamePacketTypes.CLIENTBOUND_LEVEL_CHUNK_WITH_LIGHT`.
pub fn clientbound_level_chunk_with_light() -> PacketType {
    PacketType::clientbound("level_chunk_with_light")
}
/// `GamePacketTypes.CLIENTBOUND_LIGHT_UPDATE`.
pub fn clientbound_light_update() -> PacketType {
    PacketType::clientbound("light_update")
}
/// `GamePacketTypes.SERVERBOUND_CHUNK_BATCH_RECEIVED`.
pub fn serverbound_chunk_batch_received() -> PacketType {
    PacketType::serverbound("chunk_batch_received")
}
/// `GamePacketTypes.CLIENTBOUND_BUNDLE`.
pub fn clientbound_bundle() -> PacketType {
    PacketType::clientbound("bundle")
}
/// `GamePacketTypes.CLIENTBOUND_BUNDLE_DELIMITER`.
pub fn clientbound_bundle_delimiter() -> PacketType {
    PacketType::clientbound("bundle_delimiter")
}
/// `GamePacketTypes.CLIENTBOUND_CHANGE_DIFFICULTY`.
pub fn clientbound_change_difficulty() -> PacketType {
    PacketType::clientbound("change_difficulty")
}
/// `GamePacketTypes.CLIENTBOUND_GAME_EVENT`.
pub fn clientbound_game_event() -> PacketType {
    PacketType::clientbound("game_event")
}
/// `GamePacketTypes.CLIENTBOUND_INITIALIZE_BORDER`.
pub fn clientbound_initialize_border() -> PacketType {
    PacketType::clientbound("initialize_border")
}
/// `GamePacketTypes.CLIENTBOUND_LOGIN`.
pub fn clientbound_login() -> PacketType {
    PacketType::clientbound("login")
}
/// `GamePacketTypes.CLIENTBOUND_PLAYER_ABILITIES`.
pub fn clientbound_player_abilities() -> PacketType {
    PacketType::clientbound("player_abilities")
}
/// `GamePacketTypes.CLIENTBOUND_PLAYER_INFO_REMOVE`.
pub fn clientbound_player_info_remove() -> PacketType {
    PacketType::clientbound("player_info_remove")
}
/// `GamePacketTypes.CLIENTBOUND_PLAYER_INFO_UPDATE`.
pub fn clientbound_player_info_update() -> PacketType {
    PacketType::clientbound("player_info_update")
}
/// `GamePacketTypes.CLIENTBOUND_PLAYER_POSITION`.
pub fn clientbound_player_position() -> PacketType {
    PacketType::clientbound("player_position")
}
/// `GamePacketTypes.CLIENTBOUND_SET_CHUNK_CACHE_CENTER`.
pub fn clientbound_set_chunk_cache_center() -> PacketType {
    PacketType::clientbound("set_chunk_cache_center")
}
/// `GamePacketTypes.CLIENTBOUND_SET_CHUNK_CACHE_RADIUS`.
pub fn clientbound_set_chunk_cache_radius() -> PacketType {
    PacketType::clientbound("set_chunk_cache_radius")
}
/// `GamePacketTypes.CLIENTBOUND_SET_SIMULATION_DISTANCE`.
pub fn clientbound_set_simulation_distance() -> PacketType {
    PacketType::clientbound("set_simulation_distance")
}
/// `GamePacketTypes.CLIENTBOUND_SET_DEFAULT_SPAWN_POSITION`.
pub fn clientbound_set_default_spawn_position() -> PacketType {
    PacketType::clientbound("set_default_spawn_position")
}
/// `GamePacketTypes.CLIENTBOUND_SET_HELD_SLOT`.
pub fn clientbound_set_held_slot() -> PacketType {
    PacketType::clientbound("set_held_slot")
}
/// `GamePacketTypes.CLIENTBOUND_SET_TIME`.
pub fn clientbound_set_time() -> PacketType {
    PacketType::clientbound("set_time")
}
/// `GamePacketTypes.CLIENTBOUND_UPDATE_RECIPES`.
pub fn clientbound_update_recipes() -> PacketType {
    PacketType::clientbound("update_recipes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol::PacketFlow;

    #[test]
    fn constants_carry_flow_and_default_namespace() {
        assert_eq!(
            clientbound_level_chunk_with_light().flow(),
            PacketFlow::Clientbound
        );
        assert_eq!(
            clientbound_level_chunk_with_light().id().to_string(),
            "minecraft:level_chunk_with_light"
        );
        assert_eq!(
            clientbound_light_update().id().to_string(),
            "minecraft:light_update"
        );
        assert_eq!(
            clientbound_chunk_batch_start().id().to_string(),
            "minecraft:chunk_batch_start"
        );
        assert_eq!(
            clientbound_chunk_batch_finished().id().to_string(),
            "minecraft:chunk_batch_finished"
        );
        assert_eq!(
            clientbound_chunks_biomes().id().to_string(),
            "minecraft:chunks_biomes"
        );
        assert_eq!(
            serverbound_chunk_batch_received().flow(),
            PacketFlow::Serverbound
        );
        assert_eq!(
            serverbound_chunk_batch_received().id().to_string(),
            "minecraft:chunk_batch_received"
        );
        // The #87 join slice discriminators: all clientbound, canonical names.
        for (constant, name) in [
            (clientbound_bundle(), "minecraft:bundle"),
            (clientbound_bundle_delimiter(), "minecraft:bundle_delimiter"),
            (
                clientbound_change_difficulty(),
                "minecraft:change_difficulty",
            ),
            (clientbound_game_event(), "minecraft:game_event"),
            (
                clientbound_initialize_border(),
                "minecraft:initialize_border",
            ),
            (clientbound_login(), "minecraft:login"),
            (clientbound_player_abilities(), "minecraft:player_abilities"),
            (
                clientbound_player_info_remove(),
                "minecraft:player_info_remove",
            ),
            (
                clientbound_player_info_update(),
                "minecraft:player_info_update",
            ),
            (clientbound_player_position(), "minecraft:player_position"),
            (
                clientbound_set_chunk_cache_center(),
                "minecraft:set_chunk_cache_center",
            ),
            (
                clientbound_set_chunk_cache_radius(),
                "minecraft:set_chunk_cache_radius",
            ),
            (
                clientbound_set_simulation_distance(),
                "minecraft:set_simulation_distance",
            ),
            (
                clientbound_set_default_spawn_position(),
                "minecraft:set_default_spawn_position",
            ),
            (clientbound_set_held_slot(), "minecraft:set_held_slot"),
            (clientbound_set_time(), "minecraft:set_time"),
            (clientbound_update_recipes(), "minecraft:update_recipes"),
        ] {
            assert_eq!(constant.flow(), PacketFlow::Clientbound, "{name}");
            assert_eq!(constant.id().to_string(), name);
        }
    }
}
