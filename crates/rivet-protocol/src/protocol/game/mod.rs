//! Port of `net.minecraft.network.protocol.game` — the game (play) packet
//! bodies and the value codecs they compose.
//!
//! Slices live here:
//!
//! * The spawn-info slice (#108) owns `CommonPlayerSpawnInfo` — the
//!   `(Holder<DimensionType>, ResourceKey<Level>, seed, GameType,
//!   previousGameType, isDebug, isFlat, lastDeathLocation, portalCooldown,
//!   seaLevel)` record embedded by `ClientboundLoginPacket` (field 4 of 12) and
//!   re-read by `ClientboundRespawnPacket`. It is a sub-record, not a `Packet`,
//!   so it has no `PacketType`/`packet_type()` and its codec is
//!   `StreamCodec<RegistryFriendlyByteBuf, CommonPlayerSpawnInfo>` (the
//!   registry-aware buffer, because `dimensionType` resolves through the
//!   `DIMENSION_TYPE` registry).
//!
//! * The chunk-send slice (#94) ports the chunk-send value layer: the heightmap
//!   key enum ([`heightmap_types`]), the `ClientboundLevelChunkPacketData` /
//!   light-data value types ([`level_chunk_packet_data`],
//!   [`light_update_packet_data`]) and the packet bodies that carry them, plus
//!   the chunk batch start/finished/received trio ([`packet_types`] for the
//!   discriminator constants).
//!
//! * The join slice (#87, `mc.network.protocol.game.join`, MANIFEST line 262)
//!   ports the join-critical clientbound bodies: `change_difficulty`,
//!   `game_event`, `initialize_border`, `player_abilities`,
//!   `set_default_spawn_position`, `set_held_slot`, `login`, `set_time`,
//!   `player_info_update`, `player_info_remove`, `player_position`, the
//!   `PositionMoveRotation` / `ClockNetworkState` value types, and (in
//!   [`clientbound_set_default_spawn`]) the `LevelData.RespawnData` wire shape.
//!   The plain-`FriendlyByteBuf` bodies compose the value types whose
//!   `StreamCodec`s live here; the registry-aware bodies (`clientbound_login`,
//!   `clientbound_set_time`, `clientbound_player_info_update`) resolve holders
//!   through the `RegistryFriendlyByteBuf` the connection carries.
//!
//! The sections buffer (`ClientboundLevelChunkPacketData.buffer`) and the biome
//! buffer (`ChunkBiomeData.buffer`) are **opaque bytes** to the protocol crate:
//! `LevelChunkSection.write`/`getBiomes()` run in `rivet-world` (issue #100) and
//! fill the `Vec<u8>` the packets carry. No `&LevelChunk`/`&ChunkMap`
//! back-reference crosses into `rivet-protocol`; every encode path takes plain
//! values (`x`/`z`, the heightmap map, the opaque buffer, the block-entity list,
//! the light layers, the `ChunkPos`).
//!
//! `ServerboundChunkBatchReceivedPacket` also has a port in `rivet_protocol::game`
//! (the #97 serverbound-play slice); the two are independent ports of the same
//! Java class and this slice's [`serverbound_chunk_batch_received`] is the one
//! that shares `packet_types` with the rest of the chunk-send trio.

pub mod clientbound_bundle;
pub mod clientbound_bundle_delimiter;
pub mod clientbound_change_difficulty;
pub mod clientbound_chunk_batch_finished;
pub mod clientbound_chunk_batch_start;
pub mod clientbound_chunks_biomes;
pub mod clientbound_game_event;
pub mod clientbound_initialize_border;
pub mod clientbound_level_chunk_with_light;
pub mod clientbound_light_update;
pub mod clientbound_login;
pub mod clientbound_player_abilities;
pub mod clientbound_player_info_remove;
pub mod clientbound_player_info_update;
pub mod clientbound_player_position;
pub mod clientbound_set_chunk_cache_center;
pub mod clientbound_set_chunk_cache_radius;
pub mod clientbound_set_default_spawn;
pub mod clientbound_set_held_slot;
pub mod clientbound_set_simulation_distance;
pub mod clientbound_set_time;
pub mod clientbound_update_recipes;
pub mod clock_network_state;
pub mod common_player_spawn_info;
pub mod heightmap_types;
pub mod level_chunk_packet_data;
pub mod light_update_packet_data;
pub mod packet_types;
pub mod position_move_rotation;
pub mod serverbound_chunk_batch_received;

pub use common_player_spawn_info::CommonPlayerSpawnInfo;
