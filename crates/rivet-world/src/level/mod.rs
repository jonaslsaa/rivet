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
pub mod height_accessor;
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

pub use block_getter::BlockGetter;
pub use height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
pub use level::{Level, end, nether, overworld};
pub use level_accessor::LevelAccessor;
pub use level_reader::LevelReader;
pub use rivet_registry::core::ChunkPos;
pub use storage::{
    DerivedLevelData, LevelData, RespawnData, ServerLevelData, WorldData, WritableLevelData,
    default_respawn_data, format_location,
};
pub use world_gen_level::WorldGenLevel;
