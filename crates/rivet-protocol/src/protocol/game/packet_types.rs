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
    }
}
