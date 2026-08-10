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
use rivet_registry::core::{BlockPos, Difficulty, GlobalPos, global_pos_map_codec};
use rivet_registry::registries::Level as LevelKey;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{MapCodec, codec_of};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use std::sync::Arc;

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
///
/// No `PartialEq`: the Java record's generated `equals` compares `float`
/// components with `Float.compare` (NaN equal, `+0.0 != -0.0`), which Rust's
/// derived `f32` `PartialEq` does not match. The seam needs only
/// [`RespawnData::position_equals`] (Paper's plain-`==` variant); whole-record
/// equality defers with the codec surface (issue #126).
#[derive(Clone, Debug)]
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
/// Level.OVERWORLD, BlockPos.ZERO), 0.0F, 0.0F)`. Java builds `DEFAULT` with
/// the record constructor (no angle normalization); only `RespawnData.of`
/// normalizes. Not `const` (the dimension key is a `LazyLock`-rooted
/// `ResourceKey`), so it is a function.
pub fn default_respawn_data() -> RespawnData {
    RespawnData::new(
        GlobalPos::of(super::super::level::overworld(), BlockPos::ZERO),
        0.0,
        0.0,
    )
}

/// `LevelData.RespawnData.MAP_CODEC` — `RecordCodecBuilder.mapCodec(i ->
/// i.group(GlobalPos.MAP_CODEC.forGetter(RespawnData::globalPos),
/// Codec.floatRange(-180, 180).fieldOf("yaw"), Codec.floatRange(-90,
/// 90).fieldOf("pitch")).apply(i, RespawnData::new))`.
///
/// Exposed as the ops-generic `respawn_data_map_codec::<Ops>()` factory
/// (Java's `static final` constant). The yaw/pitch bounds run on both decode
/// and encode, exactly like `Codec.floatRange`'s `flatXMap`.
pub fn respawn_data_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<RespawnData, Ops>>
where
    RespawnData: 'static,
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|r: &RespawnData| r.global_pos.clone()),
                global_pos_map_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|r: &RespawnData| r.yaw),
                "yaw".to_string(),
                codec::float_range::<Ops>(-180.0, 180.0),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|r: &RespawnData| r.pitch),
                "pitch".to_string(),
                codec::float_range::<Ops>(-90.0, 90.0),
            ))
            .apply(instance, Arc::new(RespawnData::new))
    })
}

/// `LevelData.RespawnData.CODEC` — `MAP_CODEC.codec()`.
pub fn respawn_data_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<RespawnData, Ops>>
where
    RespawnData: 'static,
{
    codec_of(respawn_data_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::level::overworld;
    use rivet_nbt::nbt_io;
    use rivet_nbt::nbt_ops::NbtOps;
    use rivet_serialization::Dynamic;

    /// Decode the real 26.2 `level.dat` fixture's `spawn` compound through
    /// `LevelData.RespawnData.CODEC` end-to-end.
    #[test]
    fn respawn_data_codec_decodes_real_fixture_spawn() {
        let Some(path) =
            workspace_root().map(|ws| ws.join("tools/rivet-oracle/fixtures/level.dat"))
        else {
            eprintln!("fixtures not present — skipping");
            return;
        };
        if !path.is_file() {
            eprintln!("fixtures not present — skipping");
            return;
        }
        let bytes = std::fs::read(&path).expect("level.dat readable");
        let tag = nbt_io::read_compressed(
            &bytes[..],
            &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
        )
        .expect("read_compressed must read Paper's gzip level.dat");
        let data = tag
            .get_compound("Data")
            .expect("level.dat must carry a Data compound");
        let ops = NbtOps::instance();
        let dynamic = Dynamic::new(&ops, rivet_nbt::tag::Tag::Compound(data.clone()));
        // The fixture's spawn: pos (0,-60,0), pitch 0.0, yaw 0.0, dimension minecraft:overworld.
        let spawn = dynamic
            .get(&ops, "spawn")
            .decode(&ops, &*respawn_data_codec::<NbtOps>())
            .result()
            .expect("spawn decode must succeed")
            .0
            .clone();
        assert_eq!(spawn.pos(), BlockPos::new(0, -60, 0));
        assert_eq!(spawn.pitch(), 0.0);
        assert_eq!(spawn.yaw(), 0.0);
        assert_eq!(spawn.dimension(), &overworld());
    }

    /// The `RespawnData` codec round-trips through `NbtOps`.
    #[test]
    fn respawn_data_codec_round_trips() {
        let ops = NbtOps::instance();
        let mut spawn = rivet_nbt::compound_tag::CompoundTag::new();
        spawn.put(
            "pos".to_string(),
            rivet_nbt::tag::Tag::IntArray(rivet_nbt::int_array_tag::IntArrayTag::new(vec![
                1, 2, 3,
            ])),
        );
        spawn.put(
            "pitch".to_string(),
            rivet_nbt::tag::Tag::Float(rivet_nbt::float_tag::FloatTag::new(10.0)),
        );
        spawn.put(
            "yaw".to_string(),
            rivet_nbt::tag::Tag::Float(rivet_nbt::float_tag::FloatTag::new(20.0)),
        );
        spawn.put(
            "dimension".to_string(),
            rivet_nbt::tag::Tag::String(rivet_nbt::string_tag::StringTag::value_of(
                "minecraft:overworld".to_string(),
            )),
        );
        let dynamic = Dynamic::new(&ops, rivet_nbt::tag::Tag::Compound(spawn));
        let decoded = dynamic
            .decode(&ops, &*respawn_data_codec::<NbtOps>())
            .result()
            .expect("decode must succeed")
            .0
            .clone();
        assert_eq!(decoded.pos(), BlockPos::new(1, 2, 3));
        assert_eq!(decoded.yaw(), 20.0);
        assert_eq!(decoded.pitch(), 10.0);
        assert_eq!(decoded.dimension(), &overworld());
        // Encode back.
        let encoded = respawn_data_codec::<NbtOps>()
            .encode_start(&ops, &decoded)
            .result()
            .expect("encode must succeed")
            .clone();
        assert!(
            matches!(encoded, rivet_nbt::tag::Tag::Compound(_)),
            "encode must produce a compound"
        );
    }

    fn workspace_root() -> Option<std::path::PathBuf> {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()
            .map(|p| p.to_path_buf())
    }

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
