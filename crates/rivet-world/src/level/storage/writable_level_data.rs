//! `net.minecraft.world.level.storage.WritableLevelData` — the write side of
//! the world's persistent data.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/WritableLevelData.java`. A one-method interface extending
//! `LevelData` with the spawn setter.
//!
//! The Java method takes the `RespawnData` record by value; `setSpawn` is
//! `&mut self` — the concrete `PrimaryLevelData` mutates its spawn field.

use super::level_data::{LevelData, RespawnData};

/// `WritableLevelData` — `LevelData` plus the spawn setter.
///
/// Java `Level` holds a `WritableLevelData` (`getLevelData()`); the concrete
/// world port types against this.
pub trait WritableLevelData: LevelData {
    /// `setSpawn(LevelData.RespawnData)`.
    fn set_spawn(&mut self, respawn_data: RespawnData);
}
