//! `net.minecraft.world.level.storage.LevelData` — the world's persistent
//! settings.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/LevelData.java`. The #232 value slice ports the `getGameTime` read
//! (the `LevelAccessor` seam) plus the `RespawnData` value record.
//! `RespawnData.MAP_CODEC`/`CODEC` land here (wired on the
//! `GlobalPos`/`BlockPos` map codecs); `RespawnData.STREAM_CODEC` defers with
//! the protocol codec surface (issue #126, `rivet-protocol`), and the
//! `fillCrashReportCategory` default defers with the crash-report surface.

use rivet_registry::ResourceKey;
use rivet_registry::core::{BlockPos, Difficulty, GlobalPos, SectionPos, global_pos_map_codec};
use rivet_registry::registries::Level as LevelKey;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{MapCodec, codec_of};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use std::sync::Arc;

use super::super::height_accessor::LevelHeightAccessor;

/// `LevelData` — the read side of the world's persistent data.
///
/// RivetTodo(#232): `WritableLevelData` (`setSpawn`) and `ServerLevelData`
/// (`setGameTime`, game type, allow-commands) are separate `storage` files
/// outside this unit's list and defer with the concrete world data; `Level`
/// holds a `WritableLevelData` in Java, which the concrete world port will
/// type against. The `fillCrashReportCategory` default lands here (#398),
/// grounded in `CrashReportCategory.formatLocation` (the crash-report surface
/// stays a stub in `rivet-core`, which records entries).
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

    /// `LevelData.fillCrashReportCategory(CrashReportCategory, LevelHeightAccessor)`
    /// — the default that `ServerLevelData`/`PrimaryLevelData` build on.
    ///
    /// Java:
    /// ```java
    /// default void fillCrashReportCategory(final CrashReportCategory category,
    ///     final LevelHeightAccessor levelHeightAccessor) {
    ///     category.setDetail("Level spawn location",
    ///         () -> CrashReportCategory.formatLocation(levelHeightAccessor, this.getRespawnData().pos()));
    /// }
    /// ```
    ///
    /// The `CrashReportCategory` stub records the detail key and the rendered
    /// location string; the level-data defaults are otherwise stub-owned.
    fn fill_crash_report_category(
        &self,
        category: &mut rivet_core::CrashReportCategory,
        level_height_accessor: &dyn LevelHeightAccessor,
    ) {
        let pos = self.get_respawn_data().pos();
        category.set_detail(
            "Level spawn location",
            format_location(level_height_accessor, &pos),
        );
    }
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
/// equality is not ported — nothing consumes it yet.
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

/// `CrashReportCategory.formatLocation(LevelHeightAccessor, BlockPos)` — the
/// crash-report location string.
///
/// Java formats three sections: `World: (x,y,z)`, `Section: (at rx,ry,rz in
/// sx,sy,sz; chunk contains blocks ...)`, and `Region: (rx,rz; contains chunks
/// ... , blocks ...)`. The arithmetic (`blockToSectionCoord` `>> 4`, `x & 15`,
/// `sectionToBlockCoord` `<< 4`, `>> 9` / `<< 9` region math) mirrors the
/// Java exactly with the `SectionPos`/`BlockPos` value surfaces.
///
/// Java wraps each section in a defensive try/catch that prints
/// `(Error finding world loc)` / `(Error finding chunk loc)` on a throwable.
/// Those branches are unreachable in practice (the format calls and integer
/// arithmetic never throw in a release JVM), so the port omits the guards —
/// identical behavior on every reachable input.
///
/// The Java `%.2f` double overload is not ported — the level-data defaults
/// only ever call the `BlockPos` overload (`LevelData.fillCrashReportCategory`
/// passes `this.getRespawnData().pos()`). The two int/BlockPos overloads are
/// unified here (Java's `BlockPos` overload delegates to the int one).
pub fn format_location(level_height_accessor: &dyn LevelHeightAccessor, pos: &BlockPos) -> String {
    let x = pos.get_x();
    let y = pos.get_y();
    let z = pos.get_z();

    let mut result = String::new();
    result.push_str(&format!("World: ({x},{y},{z})"));
    result.push_str(", ");

    let section_x = SectionPos::block_to_section_coord(x);
    let section_y = SectionPos::block_to_section_coord(y);
    let section_z = SectionPos::block_to_section_coord(z);
    let relative_x = x & 15;
    let relative_y = y & 15;
    let relative_z = z & 15;
    let min_block_x = SectionPos::section_to_block_coord(section_x);
    let min_block_y = level_height_accessor.get_min_y();
    let min_block_z = SectionPos::section_to_block_coord(section_z);
    let max_block_x = SectionPos::section_to_block_coord(section_x + 1) - 1;
    let max_block_y = level_height_accessor.get_max_y();
    let max_block_z = SectionPos::section_to_block_coord(section_z + 1) - 1;
    result.push_str(&format!(
        "Section: (at {relative_x},{relative_y},{relative_z} in {section_x},{section_y},{section_z}; chunk contains blocks {min_block_x},{min_block_y},{min_block_z} to {max_block_x},{max_block_y},{max_block_z})"
    ));
    result.push_str(", ");

    let region_x = x >> 9;
    let region_z = z >> 9;
    let min_chunk_x = region_x << 5;
    let min_chunk_z = region_z << 5;
    let max_chunk_x = ((region_x + 1) << 5) - 1;
    let max_chunk_z = ((region_z + 1) << 5) - 1;
    let min_block_x = region_x << 9;
    let min_block_y = level_height_accessor.get_min_y();
    let min_block_z = region_z << 9;
    let max_block_x = ((region_x + 1) << 9) - 1;
    let max_block_y = level_height_accessor.get_max_y();
    let max_block_z = ((region_z + 1) << 9) - 1;
    result.push_str(&format!(
        "Region: ({region_x},{region_z}; contains chunks {min_chunk_x},{min_chunk_z} to {max_chunk_x},{max_chunk_z}, blocks {min_block_x},{min_block_y},{min_block_z} to {max_block_x},{max_block_y},{max_block_z})"
    ));

    result
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
        let path = workspace_root().join("tools/rivet-oracle/fixtures/level.dat");
        assert!(
            path.is_file(),
            "fixture {path:?} is missing — the committed 26.2 level.dat is git-tracked, so a missing fixture means this end-to-end codec test silently stopped exercising the codec"
        );
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
        // Encode back to the Java wire shape. `RespawnData.MAP_CODEC` writes
        // the `GlobalPos` fields first ("dimension", "pos"), then "yaw", then
        // "pitch" — exactly the group order of `LevelData.RespawnData.MAP_CODEC`.
        let encoded = respawn_data_codec::<NbtOps>()
            .encode_start(&ops, &decoded)
            .result()
            .expect("encode must succeed")
            .clone();
        let compound = match &encoded {
            rivet_nbt::tag::Tag::Compound(c) => c,
            other => panic!("encode must produce a compound, got {other:?}"),
        };
        assert_eq!(
            compound.tags.keys().cloned().collect::<Vec<_>>(),
            vec!["dimension", "pos", "yaw", "pitch"]
        );
        assert_eq!(
            compound.get("dimension"),
            Some(&rivet_nbt::tag::Tag::String(
                rivet_nbt::string_tag::StringTag::value_of("minecraft:overworld".to_string())
            ))
        );
        assert_eq!(
            compound.get("pos"),
            Some(&rivet_nbt::tag::Tag::IntArray(
                rivet_nbt::int_array_tag::IntArrayTag::new(vec![1, 2, 3])
            ))
        );
        assert_eq!(
            compound.get("yaw"),
            Some(&rivet_nbt::tag::Tag::Float(
                rivet_nbt::float_tag::FloatTag::new(20.0)
            ))
        );
        assert_eq!(
            compound.get("pitch"),
            Some(&rivet_nbt::tag::Tag::Float(
                rivet_nbt::float_tag::FloatTag::new(10.0)
            ))
        );
    }

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
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

    /// `CrashReportCategory.formatLocation(LevelHeightAccessor, BlockPos)` for
    /// the overworld height accessor (minY -64, height 384) at the origin.
    ///
    /// Hand-computed from the Java:
    /// - `World: (0,0,0)`.
    /// - Section: `blockToSectionCoord(0) = 0`, `0 & 15 = 0`;
    ///   `sectionToBlockCoord(0) = 0`, `sectionToBlockCoord(1) - 1 = 15`;
    ///   min/max y = -64/319.
    /// - Region: `0 >> 9 = 0`; `0 << 5 = 0`, `(0+1) << 5 - 1 = 31`;
    ///   blocks `0<<9=0` .. `(0+1)<<9-1=511`.
    #[test]
    fn format_location_origin_overworld() {
        let h = crate::level::height_accessor::create(-64, 384);
        let s = format_location(&h, &BlockPos::ZERO);
        assert_eq!(
            s,
            "World: (0,0,0), Section: (at 0,0,0 in 0,0,0; chunk contains blocks 0,-64,0 to 15,319,15), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)"
        );
    }

    /// A position in a positive region/section: (x,y,z) = (321, 80, 300).
    ///
    /// `321 >> 4 = 20` (section x), `321 & 15 = 1` (relative x);
    /// `80 >> 4 = 5` (section y), `80 & 15 = 0`; `300 >> 4 = 18`, `300 & 15 = 12`.
    /// `sectionToBlockCoord(20) = 320`, `(20+1) << 4 - 1 = 335`; section y
    /// bounds `5<<4=80` .. `6<<4-1=95`.
    /// Region: `321 >> 9 = 0`? No — `321 / 512 = 0`. So region x = 0.
    #[test]
    fn format_location_mid_world() {
        let h = crate::level::height_accessor::create(-64, 384);
        let s = format_location(&h, &BlockPos::new(321, 80, 300));
        assert_eq!(
            s,
            "World: (321,80,300), Section: (at 1,0,12 in 20,5,18; chunk contains blocks 320,-64,288 to 335,319,303), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)"
        );
    }

    /// Negative coordinates: `blockToSectionCoord(-1) = -1 >> 4 = -1` (Java
    /// arithmetic shift), `-1 & 15 = 15` (Java `&` on a negative int is
    /// two's-complement, `-1 & 15 = 15`). `sectionToBlockCoord(-1) = -16`,
    /// `(-1+1) << 4 - 1 = -1`.
    #[test]
    fn format_location_negative() {
        let h = crate::level::height_accessor::create(-64, 384);
        let s = format_location(&h, &BlockPos::new(-1, -1, -1));
        assert_eq!(
            s,
            "World: (-1,-1,-1), Section: (at 15,15,15 in -1,-1,-1; chunk contains blocks -16,-64,-16 to -1,319,-1), Region: (-1,-1; contains chunks -32,-32 to -1,-1, blocks -512,-64,-512 to -1,319,-1)"
        );
    }

    /// The `LevelData.fillCrashReportCategory` default records the
    /// `"Level spawn location"` detail with the formatted position.
    #[test]
    fn level_data_fill_crash_report_category_records_spawn_location() {
        let h = crate::level::height_accessor::create(-64, 384);
        let data = FakeLevelData {
            game_time: 0,
            respawn: RespawnData::new(
                GlobalPos::of(overworld(), BlockPos::new(0, 64, 0)),
                0.0,
                0.0,
            ),
            hardcore: false,
            difficulty: Difficulty::Normal,
            locked: false,
        };
        let mut category = rivet_core::CrashReportCategory::new("test");
        LevelData::fill_crash_report_category(&data, &mut category, &h);
        assert_eq!(
            category.entries(),
            &[(
                "Level spawn location".to_string(),
                "World: (0,64,0), Section: (at 0,0,0 in 0,4,0; chunk contains blocks 0,-64,0 to 15,319,15), Region: (0,0; contains chunks 0,0 to 31,31, blocks 0,-64,0 to 511,319,511)".to_string()
            )]
        );
    }
}
