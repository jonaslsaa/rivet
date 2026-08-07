//! Port of `net.minecraft.network.protocol.game` — the play/join packet bodies
//! and the spawn-info value codecs they compose (#108, then the #87 join wave).
//!
//! This slice owns `CommonPlayerSpawnInfo` — the `(Holder<DimensionType>,
//! ResourceKey<Level>, seed, GameType, previousGameType, isDebug, isFlat,
//! lastDeathLocation, portalCooldown, seaLevel)` record embedded by
//! `ClientboundLoginPacket` (field 4 of 12) and re-read by
//! `ClientboundRespawnPacket`. It is a sub-record, not a `Packet`, so it has no
//! `PacketType`/`packet_type()` and its codec is
//! `StreamCodec<RegistryFriendlyByteBuf, CommonPlayerSpawnInfo>` (the
//! registry-aware buffer, because `dimensionType` resolves through the
//! `DIMENSION_TYPE` registry).
//!
//! The #87 join wave appends its packet modules (`ClientboundLoginPacket` etc.)
//! to this same `mod` file; only the first add of `pub mod game;` in
//! `protocol/mod.rs` is novel, so the two tracks merge cleanly.
//!
//! Coordination with the in-flight #87 join wave (manifest `mc.network.
//! protocol.game.join`, which lists `CommonPlayerSpawnInfo.java` in its file
//! set): the join wave must NOT re-port `CommonPlayerSpawnInfo` — this slice
//! already owns it, and consumes it via the `game::common_player_spawn_info`
//! module.

pub mod common_player_spawn_info;

pub use common_player_spawn_info::CommonPlayerSpawnInfo;
