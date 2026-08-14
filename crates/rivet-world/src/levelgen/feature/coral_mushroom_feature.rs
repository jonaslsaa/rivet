//! Port of `net.minecraft.world.level.levelgen.feature.CoralMushroomFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.coral`
//! manifest unit.
//!
//! Java: the `CoralFeature` subclass whose `placeFeature` samples a
//! `height`/`width`/`length` in `3..=5` (`nextInt(3) + 3`) and a `sinkValue` in
//! `1..=3`, then walks the whole `width`×`height`×`length` box one cell below
//! the origin's surface (`move(DOWN, sinkValue)`), attempting `placeCoralBlock`
//! on every non-interior cell (`(x != 0 && x != width || y != 0 && y != height)
//! && (z != 0 && z != length || y != 0 && y != height) && (x != 0 && x != width
//! || z != 0 && z != length) && (x == 0 || x == width || y == 0 || y == height
//! || z == 0 || z == length)`) that also survives a `nextFloat < 0.1F` skip.
//! The `placeCoralBlock` result is **discarded** (the `&& !...` sits in an
//! empty `if` body), so the shape samples consume RNG but the failures never
//! abort the walk, and `placeFeature` always returns `true`.
//!
//! The RNG order is load-bearing: the base `place` draws the `#coral_blocks`
//! block first, then the four shape samples, then per surviving cell the
//! `nextFloat < 0.1F` skip roll and — when it passes — `placeCoralBlock`'s own
//! draws (see [`coral_feature::place_coral_block`]). The port keeps that
//! exactly, including the `&&`/`||` short-circuiting (the skip roll is *not*
//! drawn for interior cells) and the wrapping coordinate arithmetic.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::coral_feature::{place_coral, place_coral_block};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.CoralMushroomFeature`.
#[derive(Debug)]
pub struct CoralMushroomFeature;

/// `Feature.CORAL_MUSHROOM` — the registered `minecraft:coral_mushroom`
/// singleton.
pub const CORAL_MUSHROOM: CoralMushroomFeature = CoralMushroomFeature;

/// `CoralMushroomFeature.placeFeature(LevelAccessor, RandomSource, BlockPos,
/// BlockState)` — the mushroom box walk.
fn place_feature<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    state: BlockState,
) -> bool {
    let height = random.next_int_bound(3).wrapping_add(3);
    let width = random.next_int_bound(3).wrapping_add(3);
    let length = random.next_int_bound(3).wrapping_add(3);
    let sink_value = random.next_int_bound(3).wrapping_add(1);
    let mut mut_pos = origin.mutable();

    for x in 0..=width {
        for y in 0..=height {
            for z in 0..=length {
                mut_pos.set(
                    x.wrapping_add(origin.get_x()),
                    y.wrapping_add(origin.get_y()),
                    z.wrapping_add(origin.get_z()),
                );
                mut_pos.move_dir_steps(&Direction::Down, sink_value);
                if (x != 0 && x != width || y != 0 && y != height)
                    && (z != 0 && z != length || y != 0 && y != height)
                    && (x != 0 && x != width || z != 0 && z != length)
                    && (x == 0 || x == width || y == 0 || y == height || z == 0 || z == length)
                    && !(random.next_float() < 0.1)
                    && !place_coral_block(level, random, &mut_pos.immutable(), state)
                {
                    // Java's empty `if` body — the `placeCoralBlock` result is
                    // discarded.
                }
            }
        }
    }

    true
}

impl FeatureBehavior<NoneFeatureConfiguration> for CoralMushroomFeature {
    /// `CoralMushroomFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`
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
        CORAL_MUSHROOM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &crate::levelgen::feature::test_support::TestGenerator,
            random,
            &origin,
            &crate::levelgen::feature::configurations::NoneFeatureConfiguration,
        ))
    }

    /// In water the mushroom box writes coral-family blocks: `place` draws the
    /// `#coral_blocks` block (`nextInt(5)`), then the four shape samples (three
    /// `nextInt(3)` + one `nextInt(3)`), then the per-cell draws.
    #[test]
    fn water_mushroom_writes_coral_family_and_returns_true() {
        let mut level = TestLevel::over(access());
        fill_water(&mut level);
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert_eq!(random.calls[0], RngCall::IntBound(5));
        assert_eq!(random.calls[1], RngCall::IntBound(3));
        assert_eq!(random.calls[2], RngCall::IntBound(3));
        assert_eq!(random.calls[3], RngCall::IntBound(3));
        assert_eq!(random.calls[4], RngCall::IntBound(3));
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
    }

    /// `placeFeature` always returns `true` even when every `placeCoralBlock`
    /// fails (a stone world): the walk still consumes the shape and skip draws,
    /// writes nothing, and reports success.
    #[test]
    fn hostile_world_still_returns_true() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert!(level.writes.is_empty());
    }
}
