//! `net.minecraft.world.level.storage.LevelData` — the world's persistent
//! settings.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/LevelData.java`. The #232 value slice ports the `getGameTime` read
//! (the `LevelAccessor` seam) plus the `RespawnData` value record. The
//! `MAP_CODEC`/`CODEC`/`STREAM_CODEC` wire surfaces defer with the
//! registry-wired codecs (issue #126, `rivet-protocol`), and the
//! `fillCrashReportCategory` default defers with the crash-report surface.

use rivet_registry::ResourceKey;
use rivet_registry::core::{BlockPos, Difficulty, GlobalPos};
use rivet_registry::registries::Level as LevelKey;
use rivet_util::mth;

/// `LevelData` — the read side of the world's persistent data.
///
/// RivetTodo(#232): `WritableLevelData` (`setSpawn`) and `ServerLevelData`
/// (`setGameTime`, game type, allow-commands) are separate `storage` files
/// outside this unit's list and defer with the concrete world data; `Level`
/// holds a `WritableLevelData` in Java, which the concrete world port will
/// type against. `fillCrashReportCategory` defers with the crash-report unit.
pub trait LevelData {
    /// `getRespawnData()`.
    fn get_respawn_data(&self) -> &RespawnData;

    /// `getGameTime()` — the world's game time in ticks. `LevelAccessor
    /// .getGameTime` (and hence `SerializableChunkData.write()`) resolves
    /// through this.
    fn get_game_time(&self) -> i64;

    /// `isHardcore()`.
    fn is_hardcore(&self) -> bool;

    /// `getDifficulty()`.
    fn get_difficulty(&self) -> Difficulty;

    /// `isDifficultyLocked()`.
    fn is_difficulty_locked(&self) -> bool;
}

/// `LevelData.RespawnData` — the `(GlobalPos, yaw, pitch)` record.
///
/// Java record `RespawnData(GlobalPos globalPos, float yaw, float pitch)`.
/// `LevelData.RespawnData.of` normalizes the angles (`Mth.wrapDegrees` yaw,
/// `Mth.clamp(pitch, -90, 90)`); the record constructor stores the raw values.
#[derive(Clone, Debug, PartialEq)]
pub struct RespawnData {
    global_pos: GlobalPos,
    yaw: f32,
    pitch: f32,
}

impl RespawnData {
    /// `new RespawnData(GlobalPos, yaw, pitch)` — the record constructor
    /// (stores the values as given).
    pub fn new(global_pos: GlobalPos, yaw: f32, pitch: f32) -> Self {
        RespawnData {
            global_pos,
            yaw,
            pitch,
        }
    }

    /// `LevelData.RespawnData.of(dimension, pos, yaw, pitch)` — normalizes the
    /// angles (`wrapDegrees` yaw, `clamp(pitch, -90, 90)`) and anchors the
    /// spawn at `GlobalPos.of(dimension, pos.immutable())`.
    pub fn of(dimension: ResourceKey<LevelKey>, pos: BlockPos, yaw: f32, pitch: f32) -> Self {
        RespawnData::new(
            GlobalPos::of(dimension, pos.immutable()),
            mth::wrap_degrees_f32(yaw),
            mth::clamp_f32(pitch, -90.0, 90.0),
        )
    }

    /// `RespawnData.globalPos()`.
    pub fn global_pos(&self) -> &GlobalPos {
        &self.global_pos
    }

    /// `RespawnData.dimension()` — `this.globalPos.dimension()`.
    pub fn dimension(&self) -> &ResourceKey<LevelKey> {
        self.global_pos.dimension()
    }

    /// `RespawnData.pos()` — `this.globalPos.pos()`.
    pub fn pos(&self) -> BlockPos {
        self.global_pos.pos()
    }

    /// `RespawnData.yaw()`.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// `RespawnData.pitch()`.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// Paper `positionEquals(Object)` — equality of position and rotation
    /// without checking the dimension.
    pub fn position_equals(&self, other: &RespawnData) -> bool {
        self.pos() == other.pos() && self.yaw == other.yaw && self.pitch == other.pitch
    }
}

/// `LevelData.RespawnData.DEFAULT` — `new RespawnData(GlobalPos.of(
/// Level.OVERWORLD, BlockPos.ZERO), 0.0F, 0.0F)`. Not `const` (the dimension
/// key is a `LazyLock`-rooted `ResourceKey`), so it is a function.
pub fn default_respawn_data() -> RespawnData {
    RespawnData::of(super::super::level::overworld(), BlockPos::ZERO, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::level::overworld;

    /// A fake `LevelData` exercising the value seam.
    struct FakeLevelData {
        game_time: i64,
        respawn: RespawnData,
        hardcore: bool,
        difficulty: Difficulty,
        locked: bool,
    }

    impl LevelData for FakeLevelData {
        fn get_respawn_data(&self) -> &RespawnData {
            &self.respawn
        }

        fn get_game_time(&self) -> i64 {
            self.game_time
        }

        fn is_hardcore(&self) -> bool {
            self.hardcore
        }

        fn get_difficulty(&self) -> Difficulty {
            self.difficulty
        }

        fn is_difficulty_locked(&self) -> bool {
            self.locked
        }
    }

    #[test]
    fn of_normalizes_yaw_and_clamps_pitch() {
        // `LevelData.RespawnData.of`: wrapDegrees(370) = 10, clamp(100, -90, 90) = 90.
        let r = RespawnData::of(overworld(), BlockPos::new(1, 2, 3), 370.0, 100.0);
        assert_eq!(r.yaw(), 10.0);
        assert_eq!(r.pitch(), 90.0);
        assert_eq!(r.pos(), BlockPos::new(1, 2, 3));
        assert_eq!(r.dimension(), &overworld());
        // clamp below -90.
        let r2 = RespawnData::of(overworld(), BlockPos::ZERO, 0.0, -100.0);
        assert_eq!(r2.pitch(), -90.0);
    }

    #[test]
    fn record_constructor_keeps_raw_values() {
        // The record constructor does not normalize; only `of` does.
        let r = RespawnData::new(GlobalPos::of(overworld(), BlockPos::ZERO), 370.0, 100.0);
        assert_eq!(r.yaw(), 370.0);
        assert_eq!(r.pitch(), 100.0);
    }

    #[test]
    fn wrap_degrees_wraps_180_to_minus_180() {
        // `Mth.wrapDegrees`: 180.0 % 360.0 == 180.0, then `>= 180.0` wraps to
        // -180.0 (not +180.0); -180.0 is already in range. This is the
        // boundary where a naive modulo would disagree with Java.
        let r = RespawnData::of(overworld(), BlockPos::ZERO, 180.0, 0.0);
        assert_eq!(r.yaw(), -180.0);
        let r = RespawnData::of(overworld(), BlockPos::ZERO, -180.0, 0.0);
        assert_eq!(r.yaw(), -180.0);
        // The raw record constructor preserves +180.0 as given.
        let raw = RespawnData::new(GlobalPos::of(overworld(), BlockPos::ZERO), 180.0, 0.0);
        assert_eq!(raw.yaw(), 180.0);
    }

    #[test]
    fn clamp_pitch_is_inclusive_and_propagates_nan() {
        // `Mth.clamp(pitch, -90, 90)` is inclusive at both ends.
        let r = RespawnData::of(overworld(), BlockPos::ZERO, 0.0, -90.0);
        assert_eq!(r.pitch(), -90.0);
        let r = RespawnData::of(overworld(), BlockPos::ZERO, 0.0, 90.0);
        assert_eq!(r.pitch(), 90.0);
        // Java `Mth.clamp` returns NaN for a NaN value (`Math.min` propagates a
        // NaN operand) — the Rust port mirrors that rather than falling to a
        // bound.
        let r = RespawnData::of(overworld(), BlockPos::ZERO, 0.0, f32::NAN);
        assert!(r.pitch().is_nan());
    }

    #[test]
    fn position_equals_ignores_dimension() {
        // Paper `positionEquals`: same pos + angles but a different dimension.
        let a = RespawnData::new(GlobalPos::of(overworld(), BlockPos::new(4, 5, 6)), 1.0, 2.0);
        let b = RespawnData::new(
            GlobalPos::of(crate::level::level::end(), BlockPos::new(4, 5, 6)),
            1.0,
            2.0,
        );
        let c = RespawnData::new(GlobalPos::of(overworld(), BlockPos::new(4, 5, 7)), 1.0, 2.0);
        assert!(a.position_equals(&b));
        assert!(!a.position_equals(&c));
    }

    #[test]
    fn default_respawn_is_overworld_zero() {
        let r = default_respawn_data();
        assert_eq!(r.dimension(), &overworld());
        assert_eq!(r.pos(), BlockPos::ZERO);
        assert_eq!(r.yaw(), 0.0);
        assert_eq!(r.pitch(), 0.0);
    }

    #[test]
    fn game_time_seam() {
        // `LevelAccessor.getGameTime()` returns `getLevelData().getGameTime()` —
        // the value the chunk serialization reads.
        let level_data = FakeLevelData {
            game_time: 123456789,
            respawn: default_respawn_data(),
            hardcore: false,
            difficulty: Difficulty::Normal,
            locked: false,
        };
        assert_eq!(level_data.get_game_time(), 123456789);
        assert_eq!(level_data.get_difficulty(), Difficulty::Normal);
    }
}
