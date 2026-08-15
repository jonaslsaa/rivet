//! Port of `net.minecraft.world.level.levelgen.feature.CoralClawFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.coral`
//! manifest unit.
//!
//! Java: the `CoralFeature` subclass whose `placeFeature` first anchors the
//! claw at the origin (`placeCoralBlock`; `false` when the anchor fails), then
//! picks a random horizontal claw direction (`getRandomDirection`), samples
//! `2..=3` branches, shuffles `{claw, claw.clockwise, claw.counterClockwise}`
//! and takes the first `nBranches`. Each branch walks a sideways leg (the claw
//! direction with an `inway` of `nextInt(3) + 2`, or — for the two
//! off-direction branches — a `segment` drawn from `{branch, UP}` with an
//! `inway` of `nextInt(3) + 3`), then an inway leg that steps along the claw
//! direction, climbing a cell on each `nextFloat < 0.25`. Every `placeCoralBlock`
//! gate that fails mid-way stops that leg; a failed anchor is the only `false`.
//!
//! The RNG order is load-bearing: the base `place` draws the `#coral_blocks`
//! block first, then `placeFeature` draws the claw direction, `nBranches`, the
//! three shuffle draws, and per branch the leg samples interleaved with
//! `placeCoralBlock`'s own draws (see [`coral_feature::place_coral_block`]).
//! The port keeps that exactly, including the `&&`/`||` short-circuiting and
//! the wrapping coordinate arithmetic.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::coral_feature::{place_coral, place_coral_block};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction, Plane};
use rivet_util::RandomSource;
use rivet_util::get_random;
use rivet_util::shuffled_copy;

/// `net.minecraft.world.level.levelgen.feature.CoralClawFeature`.
#[derive(Debug)]
pub struct CoralClawFeature;

/// `Feature.CORAL_CLAW` — the registered `minecraft:coral_claw` singleton.
pub const CORAL_CLAW: CoralClawFeature = CoralClawFeature;

/// `CoralClawFeature.placeFeature(LevelAccessor, RandomSource, BlockPos,
/// BlockState)` — the claw walk.
fn place_feature<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    state: BlockState,
) -> bool {
    if !place_coral_block(level, random, origin, state) {
        return false;
    }

    let claw_direction = get_random(Plane::Horizontal.faces(), random);
    let n_branches = random.next_int_bound(2).wrapping_add(2);
    let possible_directions = shuffled_copy(
        &[
            claw_direction,
            claw_direction.get_clock_wise(),
            claw_direction.get_counter_clock_wise(),
        ],
        random,
    );

    for &branch_direction in possible_directions.iter().take(n_branches as usize) {
        let mut mut_pos = origin.mutable();
        let sideway_length = random.next_int_bound(2).wrapping_add(1);
        mut_pos.move_dir(&branch_direction);
        let (segment_direction, inway_length) = if branch_direction == claw_direction {
            (claw_direction, random.next_int_bound(3).wrapping_add(2))
        } else {
            mut_pos.move_dir(&Direction::Up);
            let segment_direction = get_random(&[branch_direction, Direction::Up], random);
            (segment_direction, random.next_int_bound(3).wrapping_add(3))
        };

        let mut i = 0;
        while i < sideway_length && place_coral_block(level, random, &mut_pos.immutable(), state) {
            mut_pos.move_dir(&segment_direction);
            i += 1;
        }

        mut_pos.move_dir(&segment_direction.get_opposite());
        mut_pos.move_dir(&Direction::Up);

        for _ in 0..inway_length {
            mut_pos.move_dir(&claw_direction);
            if !place_coral_block(level, random, &mut_pos.immutable(), state) {
                break;
            }
            if random.next_float() < 0.25 {
                mut_pos.move_dir(&Direction::Up);
            }
        }
    }

    true
}

impl FeatureBehavior<NoneFeatureConfiguration> for CoralClawFeature {
    /// `CoralClawFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`
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

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
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
        CORAL_CLAW.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &crate::levelgen::feature::test_support::TestGenerator,
            random,
            &origin,
            &crate::levelgen::feature::configurations::NoneFeatureConfiguration,
        ))
    }

    /// In water the claw anchors at the origin and writes the coral family:
    /// `place` draws the `#coral_blocks` block (`nextInt(5)`), then the claw
    /// walk writes at the origin and around it.
    #[test]
    fn water_claw_writes_coral_family_and_returns_true() {
        let mut level = TestLevel::over(access());
        fill_water(&mut level);
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert_eq!(random.calls[0], RngCall::IntBound(5));
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
        // The anchor cell itself holds the coral block.
        let origin_state = level.states.get(&BlockPos::new(0, 0, 0)).copied().unwrap();
        assert!(origin_state.is_in_tag("minecraft:coral_blocks"));
    }

    /// A solid origin fails the anchor: `place` returns `false` (after the
    /// `#coral_blocks` draw) with no writes.
    #[test]
    fn blocked_origin_returns_false_without_writes() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), stone());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert_eq!(random.calls.len(), 1);
        assert_eq!(random.calls[0], RngCall::IntBound(5));
        assert!(level.writes.is_empty());
    }
}
