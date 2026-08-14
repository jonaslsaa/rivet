//! `net.minecraft.world.level` — the world/level layer (issue #232 value
//! slice, M2).
//!
//! This module ports the smallest non-ticking value/interface surface needed
//! for `SerializableChunkData.write()` to resolve `getGameTime`: the
//! `LevelHeightAccessor` → `BlockGetter` → `LevelReader` → `LevelAccessor` →
//! `Level` interface chain and the `LevelData` game-time seam. The concrete
//! world (`ServerLevel`, currently inherent methods in `rivet-server`) will
//! `impl` this chain with the concrete world port.
//!
//! `ChunkPos` is owned by `rivet-registry::core` (issue #125, per
//! OWNERSHIP.md §Chunks & blocks) and is re-exported here so the Java package
//! path `world.level.ChunkPos` resolves — never re-ported.

pub mod block_getter;
// `world.level.DataPackConfig` — the `WorldDataConfiguration` prerequisite
// (#387).
pub mod data_pack_config;
// The `mc.world.level.dimension` unit's `DimensionType` height constants are
// ported here (the minimal slice issue #388 needs); the full record/codec
// defers with the owning unit. Never re-ported.
pub mod dimension;
// `world.level.gamerules` — the game-rules unit: `GameRule` (+ its erased
// wildcard and value), `GameRuleMap`, `GameRules` (with the 59 built-in rules
// and the GAME_RULE registry), `GameRuleCategory`, `GameRuleType` and the
// `GameRuleTypeVisitor`.
pub mod gamerules;
pub mod height_accessor;
// `world.level.LevelSettings` — the level.dat settings record (+ nested
// `DifficultySettings`) and the `Dynamic` parse (#486).
pub mod level_settings;
// The Java package `world.level` + class `Level` mirrors to `level::level`;
// clippy's module_inception lint fires on the faithful PORTING.md name (same
// as `rivet-brigadier::suggestion::suggestion`).
#[allow(clippy::module_inception)]
pub mod level;
pub mod level_accessor;
pub mod level_reader;
pub mod storage;
pub mod validation;
// STUB(mc.world.level) — `WorldGenLevel`, the world
// surface feature placement runs against.
pub mod world_gen_level;
// `world.level.WorldDataConfiguration` — the datapack/feature-flag config of a
// world (#486).
pub mod world_data_configuration;

pub use block_getter::BlockGetter;
pub use data_pack_config::DataPackConfig;
pub use gamerules::{
    ArgumentErased, Builder as GameRuleMapBuilder, CHAT, DROPS, GAME_RULE, GameRuleCategory,
    GameRuleErased, GameRuleMap, GameRuleType, GameRuleTypeVisitor, GameRuleValue,
    GameRuleValueCodec, MISC, MOBS, PLAYER, SPAWNING, UPDATES,
    VisitorCaller as GameRuleVisitorCaller, built_in_registry as game_rule_registry,
    last_game_rule_index,
};
pub use height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
pub use level::{Level, end, nether, overworld, resource_key_codec};
pub use level_accessor::LevelAccessor;
pub use level_reader::LevelReader;
pub use level_settings::{DifficultySettings, LevelSettings};
pub use rivet_registry::core::ChunkPos;
pub use storage::{
    DerivedLevelData, LevelData, RespawnData, ServerLevelData, WorldData, WritableLevelData,
    default_respawn_data, format_location, respawn_data_codec, respawn_data_map_codec,
};
pub use world_data_configuration::WorldDataConfiguration;
pub use world_gen_level::WorldGenLevel;
