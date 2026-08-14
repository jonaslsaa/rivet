//! Port of `net.minecraft.world.level.levelgen.feature.SeaPickleFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.vegetation-family`
//! manifest unit (issue #600).
//!
//! Java: `Feature<CountConfiguration>` whose `place` samples
//! `config.count().sample(random)` attempts; each attempt offsets one cell by
//! `random.nextInt(8) - random.nextInt(8)` per axis, reads the `OCEAN_FLOOR`
//! height, draws `random.nextInt(4) + 1` for the `PICKLES` property, and —
//! when the cell is water and the state survives — writes the sea pickle with
//! `Block.UPDATE_CLIENTS` (2). Returns `true` iff at least one attempt wrote.
//!
//! The `IntProvider` count draw happens first (before any offset draw), the
//! `nextInt(4) + 1` pickle draw per attempt happens after the offset draws
//! (the `SEA_PICKLE` state is built before the water/survival gate), so a
//! failed attempt still consumes its `nextInt(4)`.
//!
//! `state.canSurvive` is the `WorldGenLevel::can_survive` seam (RivetTodo
//! #399); the test double overrides it with a controlled verdict.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::CountConfiguration;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `BlockStateBase.is(Blocks.WATER)` — the water identity check the feature
/// gates its writes on.
#[inline]
fn is_water(state: rivet_registry::block_state::BlockState) -> bool {
    state.block() == BlockId::from_name("minecraft:water").expect("water is a generated block")
}

/// `net.minecraft.world.level.levelgen.feature.SeaPickleFeature`.
#[derive(Debug)]
pub struct SeaPickleFeature;

/// `Feature.SEA_PICKLE` — the registered `minecraft:sea_pickle` singleton.
pub const SEA_PICKLE: SeaPickleFeature = SeaPickleFeature;

impl FeatureBehavior<CountConfiguration> for SeaPickleFeature {
    /// `SeaPickleFeature.place(FeaturePlaceContext<CountConfiguration>)`.
    ///
    /// ```java
    /// int placed = 0;
    /// int count = context.config().count().sample(random);
    /// for (int i = 0; i < count; i++) {
    ///     int x = random.nextInt(8) - random.nextInt(8);
    ///     int z = random.nextInt(8) - random.nextInt(8);
    ///     int y = level.getHeight(Heightmap.Types.OCEAN_FLOOR, origin.getX() + x, origin.getZ() + z);
    ///     BlockPos picklePos = new BlockPos(origin.getX() + x, y, origin.getZ() + z);
    ///     BlockState pickleState = Blocks.SEA_PICKLE.defaultBlockState()
    ///         .setValue(SeaPickleBlock.PICKLES, random.nextInt(4) + 1);
    ///     if (level.getBlockState(picklePos).is(Blocks.WATER) && pickleState.canSurvive(level, picklePos)) {
    ///         level.setBlock(picklePos, pickleState, Block.UPDATE_CLIENTS);
    ///         placed++;
    ///     }
    /// }
    /// return placed > 0;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, CountConfiguration, R>,
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
        let mut placed: i32 = 0;
        let count = config.count().sample(random);
        for _ in 0..count {
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
            let pickle_pos = BlockPos::new(
                origin.get_x().wrapping_add(x),
                y,
                origin.get_z().wrapping_add(z),
            );
            let pickle_state = Blocks::SEA_PICKLE
                .default_block_state()
                .set_value(
                    BlockStateProperties::PICKLES,
                    random.next_int_bound(4).wrapping_add(1),
                )
                .expect("sea_pickle has the pickles property");
            if is_water(level.get_block_state(&pickle_pos))
                && level.can_survive(&pickle_state, &pickle_pos)
            {
                level.set_block(&pickle_pos, pickle_state, UPDATE_CLIENTS);
                placed = placed.wrapping_add(1);
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
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;

    /// `config.count().sample(random)` — a `ConstantInt` count draws nothing,
    /// so the recorded draws are the per-attempt offsets + pickle bound.
    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        SEA_PICKLE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &CountConfiguration::new(IntProvider::Constant(ConstantInt::of(1))),
        ))
    }

    fn water() -> BlockState {
        Blocks::WATER.default_block_state()
    }

    /// The per-attempt offsets are `nextInt(8) - nextInt(8)` per axis, so the
    /// drawn cell is anywhere in `-7..=7` around the origin (at the fixed
    /// column height 0). Flood that whole range with water so the cell a given
    /// seed lands on is water.
    fn flood_offset_range(level: &mut TestLevel) {
        for x in -7..=7 {
            for z in -7..=7 {
                level.states.insert(BlockPos::new(x, 0, z), water());
            }
        }
    }

    /// One successful attempt: one `nextInt(4)` pickle draw after the two
    /// offset pairs, and the `SEA_PICKLE` write with `PICKLES` in 1..=4.
    #[test]
    fn single_attempt_writes_one_pickle() {
        let mut level = TestLevel::over(access());
        flood_offset_range(&mut level);
        let mut random = RecordingRandom::new(3);
        assert!(place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(4),
            ]
        );
        assert_eq!(level.writes.len(), 1);
        let state = level.writes[0].1;
        assert_eq!(
            state.block(),
            BlockId::from_name("minecraft:sea_pickle").unwrap()
        );
        let pickles = state
            .get_value(BlockStateProperties::PICKLES)
            .expect("pickles property present");
        assert!(
            matches!(pickles, rivet_registry::block_state_property::PropertyValue::Int(v) if (1..=4).contains(&v))
        );
    }

    /// A non-water cell still consumes the `nextInt(4)` pickle draw (the state
    /// is built before the water gate) but writes nothing and returns false.
    #[test]
    fn failed_attempt_still_draws_pickles_and_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(3);
        assert!(!place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(4),
            ]
        );
        assert!(level.writes.is_empty());
    }

    /// A constant count of two with both cells water draws per-attempt
    /// `[8,8,8,8,4]` twice.
    #[test]
    fn two_attempts_draw_twice() {
        let mut level = TestLevel::over(access());
        flood_offset_range(&mut level);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let mut random = RecordingRandom::new(3);
        let placed = SEA_PICKLE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &CountConfiguration::new(IntProvider::Constant(ConstantInt::of(2))),
        ));
        assert!(placed);
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(4),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(4),
            ]
        );
    }

    /// A `canSurvive` false verdict skips the write for an otherwise water
    /// cell; with no successful attempt the feature returns false.
    #[test]
    fn cannot_survive_skips_the_write() {
        let mut level = TestLevel::over(access());
        level.survive = false;
        level.states.insert(BlockPos::new(0, 0, 0), water());
        let mut random = RecordingRandom::new(3);
        assert!(!place(&mut level, &mut random));
        assert!(level.writes.is_empty());
    }
}
