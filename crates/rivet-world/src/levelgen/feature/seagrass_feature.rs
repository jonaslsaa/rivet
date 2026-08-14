//! Port of `net.minecraft.world.level.levelgen.feature.SeagrassFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.seagrass`
//! manifest unit (issue #600).
//!
//! Java: `Feature<ProbabilityFeatureConfiguration>` whose `place` offsets one
//! cell from the origin by `random.nextInt(8) - random.nextInt(8)` on each of
//! x and z, reads the `OCEAN_FLOOR` height at that column, and — when the cell
//! is water — draws `nextDouble() < config.probability` to decide whether the
//! seagrass is tall. A short plant writes `SEAGRASS` at the cell; a tall one
//! writes `TALL_SEAGRASS` (bottom half) at the cell plus `TALL_SEAGRASS`
//! `HALF=UPPER` at the cell above, and the pair only lands when both cells are
//! water (the `setValue(HALF, DoubleBlockHalf.UPPER)` and the two writes are
//! inside the `above is water` guard). `placedAny` is set exactly when a plant
//! survives; the feature returns that flag.
//!
//! The block-state writes route through the `WorldGenLevel::set_block` seam
//! with `Block.UPDATE_CLIENTS` (2). The `is(Blocks.WATER)` identity checks read
//! `get_block_state(...).block()` against the `minecraft:water` id (the
//! registry id from the generated block table), and `state.canSurvive` is the
//! `WorldGenLevel::can_survive` seam (RivetTodo #232) — the test double
//! overrides it with a controlled verdict.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::ProbabilityFeatureConfiguration;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state_properties::{BlockStateProperties, DoubleBlockHalf};
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to (`level.setBlock(pos, state, Block.UPDATE_CLIENTS)`).
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.SeagrassFeature`.
#[derive(Debug)]
pub struct SeagrassFeature;

/// `Feature.SEAGRASS` — the registered `minecraft:seagrass` singleton.
pub const SEAGRASS: SeagrassFeature = SeagrassFeature;

/// `BlockStateBase.is(Blocks.WATER)` — the water identity check
/// `SeagrassFeature`/`KelpFeature`/`SeaPickleFeature`/`BlueIceFeature` gate
/// their writes on (Paper `is(Block)` compares the state's block).
#[inline]
fn is_water(state: rivet_registry::block_state::BlockState) -> bool {
    state.block() == BlockId::from_name("minecraft:water").expect("water is a generated block")
}

impl FeatureBehavior<ProbabilityFeatureConfiguration> for SeagrassFeature {
    /// `SeagrassFeature.place(FeaturePlaceContext<ProbabilityFeatureConfiguration>)`.
    ///
    /// ```java
    /// boolean placedAny = false;
    /// int x = random.nextInt(8) - random.nextInt(8);
    /// int z = random.nextInt(8) - random.nextInt(8);
    /// int y = level.getHeight(Heightmap.Types.OCEAN_FLOOR, origin.getX() + x, origin.getZ() + z);
    /// BlockPos grassPos = new BlockPos(origin.getX() + x, y, origin.getZ() + z);
    /// if (level.getBlockState(grassPos).is(Blocks.WATER)) {
    ///     boolean isTall = random.nextDouble() < config.probability;
    ///     BlockState state = isTall ? Blocks.TALL_SEAGRASS.defaultBlockState()
    ///                               : Blocks.SEAGRASS.defaultBlockState();
    ///     if (state.canSurvive(level, grassPos)) {
    ///         if (isTall) {
    ///             BlockState upperState = state.setValue(TallSeagrassBlock.HALF, DoubleBlockHalf.UPPER);
    ///             BlockPos above = grassPos.above();
    ///             if (level.getBlockState(above).is(Blocks.WATER)) {
    ///                 level.setBlock(grassPos, state, Block.UPDATE_CLIENTS);
    ///                 level.setBlock(above, upperState, Block.UPDATE_CLIENTS);
    ///             }
    ///         } else {
    ///             level.setBlock(grassPos, state, Block.UPDATE_CLIENTS);
    ///         }
    ///         placedAny = true;
    ///     }
    /// }
    /// return placedAny;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, ProbabilityFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = *origin;
        let config = *config;
        let mut placed_any = false;
        let x = random
            .next_int_bound(8)
            .wrapping_sub(random.next_int_bound(8));
        let z = random
            .next_int_bound(8)
            .wrapping_sub(random.next_int_bound(8));
        let y = level.get_height_at(
            Types::OceanFloor,
            origin.get_x().wrapping_add(x),
            origin.get_z().wrapping_add(z),
        );
        let grass_pos = BlockPos::new(
            origin.get_x().wrapping_add(x),
            y,
            origin.get_z().wrapping_add(z),
        );
        if is_water(level.get_block_state(&grass_pos)) {
            let is_tall = random.next_double() < config.probability as f64;
            let state = if is_tall {
                Blocks::TALL_SEAGRASS.default_block_state()
            } else {
                Blocks::SEAGRASS.default_block_state()
            };
            if level.can_survive(&state, &grass_pos) {
                if is_tall {
                    let upper_state = state
                        .set_value(
                            BlockStateProperties::DOUBLE_BLOCK_HALF,
                            DoubleBlockHalf::Upper,
                        )
                        .expect("tall_seagrass has the half property");
                    let above = grass_pos.above();
                    if is_water(level.get_block_state(&above)) {
                        level.set_block(&grass_pos, state, UPDATE_CLIENTS);
                        level.set_block(&above, upper_state, UPDATE_CLIENTS);
                    }
                } else {
                    level.set_block(&grass_pos, state, UPDATE_CLIENTS);
                }
                placed_any = true;
            }
        }
        placed_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;

    fn place_with_probability(
        level: &mut TestLevel,
        random: &mut RecordingRandom,
        probability: f32,
    ) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        SEAGRASS.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &ProbabilityFeatureConfiguration::new(probability),
        ))
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        place_with_probability(level, random, 0.5)
    }

    fn water() -> BlockState {
        Blocks::WATER.default_block_state()
    }

    /// The drawn cell is `nextInt(8) - nextInt(8)` per axis, anywhere in
    /// `-7..=7` around the origin at the fixed column height 0. Flood that
    /// whole range with water so the cell a given seed lands on is water. The
    /// tall path additionally needs the cell above (y=1) to be water.
    fn flood_offset_range(level: &mut TestLevel) {
        for x in -7..=7 {
            for z in -7..=7 {
                level.states.insert(BlockPos::new(x, 0, z), water());
            }
        }
    }

    /// The tall path requires both the drawn cell and the cell above it to be
    /// water — flood y=0 and y=1 over the offset range.
    fn flood_offset_range_with_upper(level: &mut TestLevel) {
        for x in -7..=7 {
            for z in -7..=7 {
                level.states.insert(BlockPos::new(x, 0, z), water());
                level.states.insert(BlockPos::new(x, 1, z), water());
            }
        }
    }

    /// Short (non-tall) path: the `nextDouble < probability` draw misses, the
    /// single `SEAGRASS` write lands. The draw order pins the exact Java
    /// sequence — `[IntBound(8), IntBound(8), IntBound(8), IntBound(8), Double]`.
    #[test]
    fn short_plant_draws_and_writes_one_state() {
        let mut level = TestLevel::over(access());
        flood_offset_range(&mut level);
        let mut random = RecordingRandom::new(7);
        // probability 0.0 — `nextDouble() < 0.0` is never true, so the plant
        // is always short (the `nextDouble` draw still happens).
        assert!(place_with_probability(&mut level, &mut random, 0.0));
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::Double,
            ]
        );
        assert_eq!(level.writes.len(), 1);
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:seagrass").unwrap()
        );
    }

    /// Tall path with the upper cell also water: the `TALL_SEAGRASS` bottom and
    /// the `HALF=UPPER` top both land (two writes, in Java's order).
    #[test]
    fn tall_plant_writes_both_halves_when_upper_is_water() {
        let mut level = TestLevel::over(access());
        flood_offset_range_with_upper(&mut level);
        // probability 1.0 — the `nextDouble < probability` draw always hits.
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let mut random = RecordingRandom::new(7);
        let placed = SEAGRASS.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &ProbabilityFeatureConfiguration::new(1.0),
        ));
        assert!(placed);
        assert_eq!(level.writes.len(), 2);
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:tall_seagrass").unwrap()
        );
        let upper = level.writes[1].1;
        assert_eq!(
            upper.block(),
            BlockId::from_name("minecraft:tall_seagrass").unwrap()
        );
        assert_eq!(
            upper
                .get_value(BlockStateProperties::DOUBLE_BLOCK_HALF)
                .map(|v| format!("{v:?}")),
            Some("Enum(\"upper\")".to_string())
        );
        // The bottom sits at the OCEAN_FLOOR column height (0); the upper is
        // exactly one cell above it (the drawn offset may be anywhere in the
        // flooded range, so the relationship is asserted, not a fixed cell).
        assert_eq!(level.writes[0].0.get_y(), 0);
        assert_eq!(level.writes[1].0, level.writes[0].0.above());
    }

    /// The tall path with a non-water upper cell writes nothing (both writes
    /// are inside the `above is water` guard) but still reports placed.
    #[test]
    fn tall_plant_skips_writes_when_upper_is_not_water() {
        let mut level = TestLevel::over(access());
        flood_offset_range(&mut level);
        let mut random = RecordingRandom::new(7);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = SEAGRASS.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &ProbabilityFeatureConfiguration::new(1.0),
        ));
        assert!(placed);
        assert!(level.writes.is_empty());
    }

    /// No water at the cell: the feature returns `false` after only the four
    /// offset draws (no `nextDouble`).
    #[test]
    fn non_water_cell_returns_false_after_offset_draws() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(7);
        assert!(!place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
            ]
        );
        assert!(level.writes.is_empty());
    }

    /// A `canSurvive` false verdict skips the write (and the tall upper write),
    /// and `placedAny` stays false — `placedAny = true` is set inside the
    /// survival gate, so the feature returns `false`.
    #[test]
    fn cannot_survive_returns_false_without_writing() {
        let mut level = TestLevel::over(access());
        level.survive = false;
        flood_offset_range_with_upper(&mut level);
        let mut random = RecordingRandom::new(7);
        assert!(!place(&mut level, &mut random));
        assert!(level.writes.is_empty());
    }
}
