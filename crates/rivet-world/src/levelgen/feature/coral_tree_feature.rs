//! Port of `net.minecraft.world.level.levelgen.feature.CoralTreeFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.coral`
//! manifest unit.
//!
//! Java: the `CoralFeature` subclass whose `placeFeature` grows a trunk of
//! `nextInt(3) + 1` cells straight up from the origin (each via
//! `placeCoralBlock`, with a failure aborting the whole placement with `true`),
//! then `nextInt(3) + 2` branches off a shuffled `#HORIZONTAL` plane. Each
//! branch steps `nextInt(5) + 2` cells in a zig-zag: up one cell, then — on the
//! first step and on every `nextFloat < 0.25F` after two accumulated cells —
//! sideways along the branch direction. `placeCoralBlock`'s gate failing
//! mid-branch ends that branch; the feature always returns `true`.
//!
//! The RNG order is load-bearing: the base `place` draws the `#coral_blocks`
//! block first, then `placeFeature` draws the trunk height, the branch count,
//! the four shuffle draws, and per branch the height and the interleaved
//! `placeCoralBlock`/`nextFloat < 0.25F` draws. The port keeps that exactly,
//! including the `&&`/`||` short-circuiting and the wrapping coordinate
//! arithmetic.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::coral_feature::{place_coral, place_coral_block};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction, Plane};
use rivet_util::RandomSource;
use rivet_util::shuffled_copy;

/// `net.minecraft.world.level.levelgen.feature.CoralTreeFeature`.
#[derive(Debug)]
pub struct CoralTreeFeature;

/// `Feature.CORAL_TREE` — the registered `minecraft:coral_tree` singleton.
pub const CORAL_TREE: CoralTreeFeature = CoralTreeFeature;

/// `CoralTreeFeature.placeFeature(LevelAccessor, RandomSource, BlockPos,
/// BlockState)` — the trunk-and-branch walk.
fn place_feature<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    state: BlockState,
) -> bool {
    let mut mut_pos = origin.mutable();
    let trunk_height = random.next_int_bound(3).wrapping_add(1);

    for _ in 0..trunk_height {
        if !place_coral_block(level, random, &mut_pos.immutable(), state) {
            return true;
        }
        mut_pos.move_dir(&Direction::Up);
    }

    let trunk_top = mut_pos.immutable();
    let n_branches = random.next_int_bound(3).wrapping_add(2);
    let directions = shuffled_copy(Plane::Horizontal.faces(), random);

    for &branch_direction in directions.iter().take(n_branches as usize) {
        mut_pos.set_vec(&trunk_top_vec(&trunk_top));
        mut_pos.move_dir(&branch_direction);
        let branch_height = random.next_int_bound(5).wrapping_add(2);
        let mut segment_length = 0;

        let mut j = 0;
        while j < branch_height && place_coral_block(level, random, &mut_pos.immutable(), state) {
            segment_length += 1;
            mut_pos.move_dir(&Direction::Up);
            if j == 0 || segment_length >= 2 && random.next_float() < 0.25 {
                mut_pos.move_dir(&branch_direction);
                segment_length = 0;
            }
            j += 1;
        }
    }

    true
}

/// `MutableBlockPos::set(BlockPos)` — the Rust port has no `BlockPos`-taking
/// `set`, so the copy goes through a `Vec3i`.
fn trunk_top_vec(pos: &BlockPos) -> rivet_registry::core::Vec3i {
    rivet_registry::core::Vec3i::new(pos.get_x(), pos.get_y(), pos.get_z())
}

impl FeatureBehavior<NoneFeatureConfiguration> for CoralTreeFeature {
    /// `CoralTreeFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`
    /// — the shared `CoralFeature.place` walk with this subclass's
    /// `placeFeature`.
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
        let origin = **origin;
        place_coral(level, random, &origin, place_feature::<R>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{RecordingRandom, RngCall, TestLevel, access};
    use rivet_registry::generated::blocks::BlockId;

    fn water() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:water").unwrap())
    }

    /// A full water box so every `placeCoralBlock` gate passes.
    fn fill_water(level: &mut TestLevel) {
        for x in -20..=20 {
            for z in -20..=20 {
                for y in -10..=20 {
                    level.states.insert(BlockPos::new(x, y, z), water());
                }
            }
        }
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let origin = BlockPos::new(0, 0, 0);
        CORAL_TREE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &crate::levelgen::feature::test_support::TestGenerator,
            random,
            &origin,
            &crate::levelgen::feature::configurations::NoneFeatureConfiguration,
        ))
    }

    /// In water the tree writes coral-family blocks: `place` draws the
    /// `#coral_blocks` block (`nextInt(5)`), then the trunk height
    /// (`nextInt(3)`), then the first trunk cell's `placeCoralBlock` topping
    /// roll (`nextFloat < 0.25`). The later draws (branch count, the four
    /// shuffle draws, per-branch height/toppings) are data-dependent, so only
    /// this deterministic prefix is pinned.
    #[test]
    fn water_tree_writes_coral_family_and_returns_true() {
        let mut level = TestLevel::over(access());
        fill_water(&mut level);
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert_eq!(random.calls[0], RngCall::IntBound(5));
        assert_eq!(random.calls[1], RngCall::IntBound(3));
        assert_eq!(random.calls[2], RngCall::Float);
        assert!(!level.writes.is_empty());
        for (_, state) in &level.writes {
            let block = state.block();
            assert!(
                state.is_in_tag("minecraft:coral_blocks")
                    || state.is_in_tag("minecraft:corals")
                    || state.is_in_tag("minecraft:wall_corals")
                    || block == BlockId::from_name("minecraft:sea_pickle").unwrap(),
                "unexpected write block {block:?}"
            );
        }
        // The trunk cells above the origin carry coral blocks (the first cells
        // written after the origin's anchor).
        assert!(
            level
                .states
                .get(&BlockPos::new(0, 1, 0))
                .copied()
                .unwrap()
                .is_in_tag("minecraft:coral_blocks")
        );
    }

    /// A hostile world (everything stone) still reports `true` — the trunk
    /// gate fails on the first cell and the feature returns `true` immediately.
    #[test]
    fn hostile_world_returns_true_without_writes() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert!(level.writes.is_empty());
    }
}
