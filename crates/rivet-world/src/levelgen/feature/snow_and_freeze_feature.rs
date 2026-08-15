//! Port of `net.minecraft.world.level.levelgen.feature.SnowAndFreezeFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.snowandfreeze`
//! manifest unit.
//!
//! Java: `Feature<NoneFeatureConfiguration>` that walks the 16x16 chunk column
//! at the `MOTION_BLOCKING` height, freezing the cell below (`minecraft:ice`,
//! `Block.UPDATE_CLIENTS`) when the biome freezes there and snowing the top
//! cell (`minecraft:snow`, `Block.UPDATE_CLIENTS`) when the biome snows, also
//! setting the `SNOWY` face property on the block below. Always returns `true`.
//! No random draws.
//!
//! `Biome.shouldFreeze(level, belowPos, false)` and `Biome.shouldSnow(level,
//! topPos)` are the dedicated `WorldGenLevel::should_freeze`/`should_snow`
//! seams (RivetTodo #232 — the `LevelReader` brightness/fluid surface they read
//! is not ported; production worlds override the seams, test doubles fix the
//! verdict).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::core::Vec3i;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `SnowAndFreezeFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.SnowAndFreezeFeature`.
#[derive(Debug)]
pub struct SnowAndFreezeFeature;

/// `Feature.FREEZE_TOP_LAYER` — the registered `minecraft:freeze_top_layer`
/// singleton (the Java field is `Feature.FREEZE_TOP_LAYER`, registered as
/// `minecraft:freeze_top_layer` in `Feature.java`, not a `SNOW_AND_FREEZE`
/// entry).
pub const FREEZE_TOP_LAYER: SnowAndFreezeFeature = SnowAndFreezeFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for SnowAndFreezeFeature {
    /// `SnowAndFreezeFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// for (int dx = 0; dx < 16; dx++) {
    ///     for (int dz = 0; dz < 16; dz++) {
    ///         int x = origin.getX() + dx;
    ///         int z = origin.getZ() + dz;
    ///         int y = level.getHeight(Heightmap.Types.MOTION_BLOCKING, x, z);
    ///         topPos.set(x, y, z);
    ///         belowPos.set(topPos).move(Direction.DOWN, 1);
    ///         Biome biome = level.getBiome(topPos).value();
    ///         if (biome.shouldFreeze(level, belowPos, false)) {
    ///             level.setBlock(belowPos, Blocks.ICE.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///         }
    ///
    ///         if (biome.shouldSnow(level, topPos)) {
    ///             level.setBlock(topPos, Blocks.SNOW.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///             BlockState belowState = level.getBlockState(belowPos);
    ///             if (belowState.hasProperty(SnowyBlock.SNOWY)) {
    ///                 level.setBlock(belowPos, belowState.setValue(SnowyBlock.SNOWY, true), Block.UPDATE_CLIENTS);
    ///             }
    ///         }
    ///     }
    /// }
    ///
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext { level, origin, .. } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let origin = **origin;
        let mut top_pos = BlockPos::ZERO.mutable();
        let mut below_pos = BlockPos::ZERO.mutable();
        for dx in 0..16 {
            for dz in 0..16 {
                let x = origin.get_x().wrapping_add(dx);
                let z = origin.get_z().wrapping_add(dz);
                let y = level.get_height_at(Types::MotionBlocking, x, z);
                top_pos.set(x, y, z);
                // `MutableBlockPos.set(BlockPos)` has no direct port; the copy
                // goes through a `Vec3i` like `set`-with-`BlockPos` would.
                let top = top_pos.immutable();
                below_pos.set_vec(&Vec3i::new(top.get_x(), top.get_y(), top.get_z()));
                below_pos.move_dir_steps(&Direction::Down, 1);
                if level.should_freeze(&below_pos.immutable(), false) {
                    level.set_block(
                        &below_pos.immutable(),
                        Blocks::ICE.default_block_state(),
                        UPDATE_CLIENTS,
                    );
                }
                if level.should_snow(&top_pos.immutable()) {
                    level.set_block(
                        &top_pos.immutable(),
                        Blocks::SNOW.default_block_state(),
                        UPDATE_CLIENTS,
                    );
                    let below_state = level.get_block_state(&below_pos.immutable());
                    if below_state.has_property(BlockStateProperties::SNOWY) {
                        let snowy = below_state
                            .set_value(BlockStateProperties::SNOWY, true)
                            .expect("snowy property present, so set_value succeeds");
                        level.set_block(&below_pos.immutable(), snowy, UPDATE_CLIENTS);
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    fn place_with<R: rivet_util::RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        FREEZE_TOP_LAYER.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration,
        ))
    }

    /// Neither biome verdict fires (the `TestLevel` defaults), so the feature
    /// walks all 256 cells, reads every column height and biome verdict, and
    /// writes nothing. `MOTION_BLOCKING` column height is the fixed `height`.
    #[test]
    fn no_freeze_no_snow_writes_nothing() {
        let mut level = TestLevel::over(access());
        level.height = 100;
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// Freeze fires everywhere: every cell writes `minecraft:ice` at the column
    /// height minus one (the `MOTION_BLOCKING` height `y`, moved down 1). The
    /// cell order is dx outer, dz inner over the 16x16 chunk column.
    #[test]
    fn freeze_writes_ice_below_the_motion_blocking_surface() {
        let mut level = TestLevel::over(access());
        level.height = 100;
        level.freeze = true;
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        let ice = Blocks::ICE.default_block_state();
        assert_eq!(level.writes.len(), 256);
        let mut seen = std::collections::BTreeSet::new();
        for (pos, state) in &level.writes {
            assert_eq!(pos.get_y(), 99);
            assert_eq!(*state, ice);
            assert!(seen.insert(*pos), "each cell written once");
        }
        for dx in 0..16 {
            for dz in 0..16 {
                assert!(seen.contains(&BlockPos::new(dx, 99, dz)));
            }
        }
    }

    /// Snow fires everywhere: the top cell (at the `MOTION_BLOCKING` height)
    /// gets `minecraft:snow`; the cell below it gets the `SNOWY` property set
    /// when its state carries it (the `TestLevel` map is pre-filled with grass,
    /// which has the property). The below-state is read after the ice write, so
    /// the freeze write is visible to the snowy update (ice has no `SNOWY`, so
    /// with freeze on, no snowy write happens).
    #[test]
    fn snow_writes_snow_top_and_snowy_below() {
        let mut level = TestLevel::over(access());
        level.height = 80;
        level.snow = true;
        let grass = BlockState::of(BlockId::from_name("minecraft:grass_block").unwrap());
        level.states.insert(BlockPos::new(0, 79, 0), grass);
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        let snow = Blocks::SNOW.default_block_state();
        // 256 snow writes at the surface height, plus one snowy update on the
        // grass cell below origin (the only pre-filled cell carrying SNOWY).
        assert_eq!(level.writes.len(), 257);
        assert!(
            level
                .writes
                .iter()
                .filter(|(pos, _)| pos.get_y() == 80)
                .all(|(_, state)| *state == snow)
        );
        assert!(
            level
                .writes
                .iter()
                .filter(|(pos, _)| *pos == BlockPos::new(0, 79, 0))
                .all(|(_, state)| state.block()
                    == BlockId::from_name("minecraft:grass_block").unwrap()
                    && state.get_value(BlockStateProperties::SNOWY)
                        == Some(PropertyValue::Bool(true)))
        );
    }

    /// Freeze writes to the below cell happen before the snow verdict reads
    /// it: with both fire, the below cell is `minecraft:ice` (no `SNOWY`
    /// property), so the snowy update is skipped for every cell.
    #[test]
    fn freeze_before_snow_means_ice_below_skips_snowy() {
        let mut level = TestLevel::over(access());
        level.height = 80;
        level.freeze = true;
        level.snow = true;
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        let ice = Blocks::ICE.default_block_state();
        let snow = Blocks::SNOW.default_block_state();
        assert_eq!(level.writes.len(), 512);
        let ice_writes = level
            .writes
            .iter()
            .filter(|(_, state)| *state == ice)
            .count();
        let snow_writes = level
            .writes
            .iter()
            .filter(|(_, state)| *state == snow)
            .count();
        assert_eq!(ice_writes, 256);
        assert_eq!(snow_writes, 256);
    }
}
