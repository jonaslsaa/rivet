//! `net.minecraft.world.level.storage` — world persistent data.
//!
//! #232 value slice: only `LevelData` (and its `RespawnData` record) is in
//! this unit's file list. `WritableLevelData` and `ServerLevelData` are
//! separate `storage` files deferred with the concrete world data (issue #232
//! tracks the full `mc.world.level` unit).

pub mod level_data;

pub use level_data::{LevelData, RespawnData, default_respawn_data};
