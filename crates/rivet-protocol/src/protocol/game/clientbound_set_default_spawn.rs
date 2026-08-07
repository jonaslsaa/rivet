//! Port of `net.minecraft.network.protocol.game.ClientboundSetDefaultSpawnPositionPacket`
//! (issue #87) — `set_default_spawn_position` (play clientbound id 97).
//!
//! Java source: `.../network/protocol/game/ClientboundSetDefaultSpawnPositionPacket.java`.
//! Wire body: `LevelData.RespawnData.STREAM_CODEC` — a `GlobalPos` (identifier
//! string `ResourceKey` + packed `BlockPos` long) then `yaw` and `pitch`
//! floats. The captured golden body is
//! `136d696e6563726166743a6f766572776f726c640000000000000fc10000000000000000` —
//! dimension `minecraft:overworld`, `BlockPos(0, -63, 0)`, `yaw 0.0`,
//! `pitch 0.0`.
//!
//! `GlobalPos.STREAM_CODEC` is a plain `StreamCodec<ByteBuf, GlobalPos>` in
//! this MC version (`ResourceKey.streamCodec(Registries.DIMENSION)` + packed
//! long — no registry resolution), so the whole body ports over the plain
//! [`FriendlyByteBuf`]. `LevelData.RespawnData` is a pure value type whose only
//! wire surface this packet needs; it is ported here (the
//! `mc.world.level.storage.LevelData` unit owns the full record in
//! `rivet-world`).

use crate::codec::byte_buf_codecs::{float, string};
use crate::codec::{StreamCodec, composite_2, composite_3, map, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_set_default_spawn_position;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::{BlockPos, GlobalPos};
use rivet_registry::registries;
use rivet_registry::{Identifier, ResourceKey, registries::Level};

/// `LevelData.RespawnData` — the record `(GlobalPos globalPos, float yaw, float pitch)`.
#[derive(Debug, Clone, PartialEq)]
pub struct RespawnData {
    /// `globalPos`.
    global_pos: GlobalPos,
    /// `yaw`.
    yaw: f32,
    /// `pitch`.
    pitch: f32,
}

impl RespawnData {
    /// The record's canonical constructor.
    pub fn new(global_pos: GlobalPos, yaw: f32, pitch: f32) -> Self {
        RespawnData {
            global_pos,
            yaw,
            pitch,
        }
    }

    /// `LevelData.RespawnData.globalPos()`.
    pub fn global_pos(&self) -> &GlobalPos {
        &self.global_pos
    }

    /// `LevelData.RespawnData.yaw()`.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// `LevelData.RespawnData.pitch()`.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// `LevelData.RespawnData.STREAM_CODEC` — `GlobalPos.STREAM_CODEC`, `FLOAT`,
    /// `FLOAT`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, RespawnData> {
        composite_3(
            global_pos_stream_codec(),
            |data: &RespawnData| data.global_pos.clone(),
            float(),
            RespawnData::yaw,
            float(),
            RespawnData::pitch,
            RespawnData::new,
        )
    }
}

/// `GlobalPos.STREAM_CODEC` — the plain-`ByteBuf` variant: a `ResourceKey` over
/// `Registries.DIMENSION` (identifier string) then the packed `BlockPos` long.
/// Java declares this `StreamCodec<ByteBuf, GlobalPos>` (no registry access
/// needed — the registry name is baked into the codec), so it ports over
/// [`FriendlyByteBuf`].
fn global_pos_stream_codec() -> StreamCodec<FriendlyByteBuf, GlobalPos> {
    composite_2(
        // `ResourceKey.streamCodec(Registries.DIMENSION)` — the identifier
        // string; `Identifier.STREAM_CODEC.map(name -> create(registry, name))`.
        map(
            string(),
            |name: &String| ResourceKey::create(&*registries::DIMENSION, Identifier::parse(name)),
            |key: &ResourceKey<Level>| key.identifier().to_string(),
        ),
        |pos: &GlobalPos| pos.dimension().clone(),
        of(
            |output: &mut FriendlyByteBuf, pos: &BlockPos| {
                output.write_long(pos.as_long());
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(BlockPos::of_long(input.read_long())),
        ),
        |pos: &GlobalPos| pos.pos(),
        GlobalPos::of,
    )
}

/// `ClientboundSetDefaultSpawnPositionPacket` — the record `(RespawnData respawnData)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundSetDefaultSpawnPositionPacket {
    /// `respawnData`.
    respawn_data: RespawnData,
}

impl ClientboundSetDefaultSpawnPositionPacket {
    /// The record's canonical constructor.
    pub fn new(respawn_data: RespawnData) -> Self {
        ClientboundSetDefaultSpawnPositionPacket { respawn_data }
    }

    /// `ClientboundSetDefaultSpawnPositionPacket.respawnData()`.
    pub fn respawn_data(&self) -> &RespawnData {
        &self.respawn_data
    }

    /// `STREAM_CODEC` — the single `RespawnData.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetDefaultSpawnPositionPacket>
    {
        map(
            RespawnData::stream_codec(),
            |respawn_data: &RespawnData| {
                ClientboundSetDefaultSpawnPositionPacket::new(respawn_data.clone())
            },
            |packet: &ClientboundSetDefaultSpawnPositionPacket| packet.respawn_data.clone(),
        )
    }
}

impl Packet for ClientboundSetDefaultSpawnPositionPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_default_spawn_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    /// Hex string -> `Vec<u8>` (test helper).
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture (36 bytes): "minecraft:overworld" identifier, BlockPos(0,-63,0)
        // packed long (0xfc1), yaw 0.0, pitch 0.0.
        let bytes = hex("136d696e6563726166743a6f766572776f726c640000000000000fc10000000000000000");
        assert_eq!(bytes.len(), 36);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundSetDefaultSpawnPositionPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(input.readable_bytes(), 0);
        assert_eq!(
            decoded
                .respawn_data()
                .global_pos()
                .dimension()
                .identifier()
                .to_string(),
            "minecraft:overworld"
        );
        assert_eq!(
            decoded.respawn_data().global_pos().pos(),
            BlockPos::new(0, -63, 0)
        );
        assert_eq!(decoded.respawn_data().yaw(), 0.0);
        assert_eq!(decoded.respawn_data().pitch(), 0.0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetDefaultSpawnPositionPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), bytes);
    }
}
