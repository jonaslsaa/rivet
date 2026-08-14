//! Port of `net.minecraft.world.level.levelgen.feature.KelpFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.kelp`
//! manifest unit (issue #600).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` reads the
//! `OCEAN_FLOOR` height at the origin column and, when the floor cell is
//! water, grows a kelp stalk upward: `height = 1 + nextInt(10)`, and for each
//! cell `h` from 0 through `height` the stalk advances while the cell and the
//! cell above are water and the `KELP_PLANT` state survives. At the top cell
//! (`h == height`) it writes `KELP` with `AGE = nextInt(4) + 20`; below that
//! it writes `KELP_PLANT`. When a cell stops being growable after the first
//! (`h > 0`), it falls back to writing a `KELP` top at the cell below when
//! that top survives and the cell below that is not already `KELP`, then
//! breaks. Returns `true` iff at least one write landed.
//!
//! `state.canSurvive` is the `WorldGenLevel::can_survive` seam (RivetTodo
//! #399); the test double overrides it with a controlled verdict.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `BlockStateBase.is(Blocks.WATER)` / `is(Blocks.KELP)` — the block identity
/// checks the feature gates its growth on.
#[inline]
fn is_block(state: rivet_registry::block_state::BlockState, name: &str) -> bool {
    state.block() == BlockId::from_name(name).expect("generated block name resolves")
}

/// `net.minecraft.world.level.levelgen.feature.KelpFeature`.
#[derive(Debug)]
pub struct KelpFeature;

/// `Feature.KELP` — the registered `minecraft:kelp` singleton.
pub const KELP: KelpFeature = KelpFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for KelpFeature {
    /// `KelpFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// int placed = 0;
    /// int y = level.getHeight(Heightmap.Types.OCEAN_FLOOR, origin.getX(), origin.getZ());
    /// BlockPos kelpPos = new BlockPos(origin.getX(), y, origin.getZ());
    /// if (level.getBlockState(kelpPos).is(Blocks.WATER)) {
    ///     BlockState stateTop = Blocks.KELP.defaultBlockState();
    ///     BlockState state = Blocks.KELP_PLANT.defaultBlockState();
    ///     int height = 1 + random.nextInt(10);
    ///     for (int h = 0; h <= height; h++) {
    ///         if (level.getBlockState(kelpPos).is(Blocks.WATER)
    ///                 && level.getBlockState(kelpPos.above()).is(Blocks.WATER)
    ///                 && state.canSurvive(level, kelpPos)) {
    ///             if (h == height) {
    ///                 level.setBlock(kelpPos, stateTop.setValue(KelpBlock.AGE, random.nextInt(4) + 20), Block.UPDATE_CLIENTS);
    ///                 placed++;
    ///             } else {
    ///                 level.setBlock(kelpPos, state, Block.UPDATE_CLIENTS);
    ///             }
    ///         } else if (h > 0) {
    ///             BlockPos below = kelpPos.below();
    ///             if (stateTop.canSurvive(level, below)
    ///                     && !level.getBlockState(below.below()).is(Blocks.KELP)) {
    ///                 level.setBlock(below, stateTop.setValue(KelpBlock.AGE, random.nextInt(4) + 20), Block.UPDATE_CLIENTS);
    ///                 placed++;
    ///             }
    ///             break;
    ///         }
    ///         kelpPos = kelpPos.above();
    ///     }
    /// }
    /// return placed > 0;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = *origin;
        let mut placed: i32 = 0;
        let y = level.get_height_at(Types::OceanFloor, origin.get_x(), origin.get_z());
        let mut kelp_pos = BlockPos::new(origin.get_x(), y, origin.get_z());
        if is_block(level.get_block_state(&kelp_pos), "minecraft:water") {
            let state_top = Blocks::KELP.default_block_state();
            let state = Blocks::KELP_PLANT.default_block_state();
            let height = 1i32.wrapping_add(random.next_int_bound(10));
            let mut h = 0;
            loop {
                if is_block(level.get_block_state(&kelp_pos), "minecraft:water")
                    && is_block(level.get_block_state(&kelp_pos.above()), "minecraft:water")
                    && level.can_survive(&state, &kelp_pos)
                {
                    if h == height {
                        let top = state_top
                            .set_value(
                                BlockStateProperties::AGE_25,
                                random.next_int_bound(4).wrapping_add(20),
                            )
                            .expect("kelp has the age property");
                        level.set_block(&kelp_pos, top, UPDATE_CLIENTS);
                        placed = placed.wrapping_add(1);
                    } else {
                        level.set_block(&kelp_pos, state, UPDATE_CLIENTS);
                    }
                } else if h > 0 {
                    let below = kelp_pos.below();
                    if level.can_survive(&state_top, &below)
                        && !is_block(level.get_block_state(&below.below()), "minecraft:kelp")
                    {
                        let top = state_top
                            .set_value(
                                BlockStateProperties::AGE_25,
                                random.next_int_bound(4).wrapping_add(20),
                            )
                            .expect("kelp has the age property");
                        level.set_block(&below, top, UPDATE_CLIENTS);
                        placed = placed.wrapping_add(1);
                    }
                    break;
                }
                kelp_pos = kelp_pos.above();
                h = h.wrapping_add(1);
                if h > height {
                    break;
                }
            }
        }
        placed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        KELP.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    fn water() -> BlockState {
        Blocks::WATER.default_block_state()
    }

    /// A full-growth stalk: height is drawn once (`nextInt(10)`), then at the
    /// top cell the `nextInt(4) + 20` AGE draw lands. The column above the
    /// origin is all water, so every intermediate cell is a `KELP_PLANT` write
    /// and the top is a `KELP` write. With seed 1 the height draw yields a
    /// small fixed height, so `placed > 0`.
    #[test]
    fn full_growth_writes_plants_and_top() {
        let mut level = TestLevel::over(access());
        // Fill the whole column above the floor cell with water.
        for y in 0..16 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        // Height draw first, then the top AGE draw.
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(10), RngCall::IntBound(4)]
        );
        // First write is a KELP_PLANT (h=0), last is the KELP top with AGE in 20..=23.
        assert!(!level.writes.is_empty());
        let first = level.writes[0].1;
        assert_eq!(
            first.block(),
            BlockId::from_name("minecraft:kelp_plant").unwrap()
        );
        let last = level.writes.last().unwrap().1;
        assert_eq!(last.block(), BlockId::from_name("minecraft:kelp").unwrap());
        let age = last
            .get_value(BlockStateProperties::AGE_25)
            .expect("age property present");
        assert!(
            matches!(age, rivet_registry::block_state_property::PropertyValue::Int(v) if (20..=23).contains(&v))
        );
    }

    /// A non-water floor cell returns `false` after only the height draw
    /// (no `nextInt(10)` growth-height draw, no writes).
    #[test]
    fn non_water_floor_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
        assert!(level.writes.is_empty());
    }

    /// A `canSurvive` false verdict on the `KELP_PLANT` state at `h == 0`
    /// skips the write (no fallback at h=0) — the feature returns false with
    /// no AGE draw.
    #[test]
    fn cannot_survive_at_base_writes_nothing() {
        let mut level = TestLevel::over(access());
        level.survive = false;
        level.states.insert(BlockPos::new(0, 0, 0), water());
        level.states.insert(BlockPos::new(0, 1, 0), water());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        // Only the growth-height draw happens; the top AGE draw never fires.
        assert_eq!(random.calls, vec![RngCall::IntBound(10)]);
        assert!(level.writes.is_empty());
    }
}
