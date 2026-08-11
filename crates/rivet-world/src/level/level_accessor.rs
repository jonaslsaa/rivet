//! `net.minecraft.world.level.LevelAccessor` — the world's mutable-write
//! accessor interface.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! LevelAccessor.java`. The #232 value slice ports the game-time seam — the
//! read side that `SerializableChunkData.write()` resolves through `Level`
//! (`getGameTime` → `getLevelData().getGameTime()`) — plus the `getDifficulty`
//! value pass-through. The scheduled-tick, sound/particle/event, chunk-source,
//! random, server and CraftBukkit surfaces defer.

use rivet_registry::core::Difficulty;

use super::level_reader::LevelReader;
use super::storage::level_data::LevelData;

/// `LevelAccessor` — read/write access to a level.
///
/// Java `LevelAccessor extends CommonLevelAccessor, ScheduledTickAccess`.
/// `CommonLevelAccessor` (`LevelReader + LevelSimulatedRW + EntityGetter`) and
/// `ScheduledTickAccess` (the scheduled-tick surface) are not part of this
/// unit's file list; `LevelReader` is the ported ancestor. The scheduled-tick
/// `createTick`/`scheduleTick` defaults, the `ScheduledTick` type, and
/// `nextSubTickCount` (tick-loop mutable state) defer with the ticks unit.
pub trait LevelAccessor: LevelReader {
    /// `getLevelData()`.
    fn get_level_data(&self) -> &dyn LevelData;

    /// `getGameTime()` — `getLevelData().getGameTime()`. This is the seam
    /// `SerializableChunkData.write()` resolves (`level.getGameTime()`).
    fn get_game_time(&self) -> i64 {
        self.get_level_data().get_game_time()
    }

    /// `getDifficulty()` — `getLevelData().getDifficulty()`.
    fn get_difficulty(&self) -> Difficulty {
        self.get_level_data().get_difficulty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::block_getter::BlockGetter;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::level::level_reader::LevelReader;
    use crate::level::storage::level_data::{RespawnData, default_respawn_data};

    /// A fake `LevelData` backed by plain values.
    struct FakeLevelData {
        game_time: i64,
        difficulty: Difficulty,
        respawn: RespawnData,
    }

    impl LevelData for FakeLevelData {
        fn get_respawn_data(&self) -> &RespawnData {
            &self.respawn
        }

        fn get_game_time(&self) -> i64 {
            self.game_time
        }

        fn is_hardcore(&self) -> bool {
            false
        }

        fn get_difficulty(&self) -> Difficulty {
            self.difficulty
        }

        fn is_difficulty_locked(&self) -> bool {
            false
        }
    }

    /// A fake `LevelAccessor` — the seam `SerializableChunkData.write()`
    /// resolves: `level.getGameTime()` → `getLevelData().getGameTime()`.
    struct FakeLevel {
        data: FakeLevelData,
    }

    impl LevelHeightAccessor for FakeLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl BlockGetter for FakeLevel {}

    impl LevelReader for FakeLevel {
        fn has_chunk(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
            true
        }

        fn get_sky_darken(&self) -> i32 {
            0
        }

        fn is_client_side(&self) -> bool {
            false
        }

        fn get_sea_level(&self) -> i32 {
            -63
        }
    }

    impl LevelAccessor for FakeLevel {
        fn get_level_data(&self) -> &dyn LevelData {
            &self.data
        }
    }

    fn fake_level() -> FakeLevel {
        FakeLevel {
            data: FakeLevelData {
                game_time: 987_654,
                difficulty: Difficulty::Hard,
                respawn: default_respawn_data(),
            },
        }
    }

    #[test]
    fn get_game_time_resolves_through_level_data() {
        // Java `LevelAccessor.getGameTime()` returns `getLevelData().getGameTime()`
        // — the value `SerializableChunkData.write()` writes as the `LastUpdate`
        // tag (`this.lastUpdateTime`, set from `level.getGameTime()` in the
        // constructor).
        let level = fake_level();
        assert_eq!(level.get_game_time(), 987_654);
        assert_eq!(level.get_difficulty(), Difficulty::Hard);
    }

    /// Counterfactual: the game-time seam is a pass-through to `LevelData`, so
    /// even a zero or negative tick count must flow through unchanged (Java
    /// `getLevelData().getGameTime()` has no clamping).
    #[test]
    fn game_time_pass_through_is_unclamped() {
        let level = FakeLevel {
            data: FakeLevelData {
                game_time: -1,
                difficulty: Difficulty::Peaceful,
                respawn: default_respawn_data(),
            },
        };
        assert_eq!(level.get_game_time(), -1);
    }
}
