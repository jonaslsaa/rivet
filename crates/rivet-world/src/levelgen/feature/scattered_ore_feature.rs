//! Port of `net.minecraft.world.level.levelgen.feature.ScatteredOreFeature`
//! (class, 26.2) — grouped under the `mc.world.level.levelgen.feature.ore`
//! manifest unit (MANIFEST row lists `OreFeature.java, ScatteredOreFeature.java`
//! together).
//!
//! Java: `Feature<OreConfiguration>` that scatters ore veins. `place` draws
//! `numberOfTries = random.nextInt(config.size + 1)`; per try it offsets the
//! target position by up to `Math.min(i, 7)` blocks per axis
//! (`MAX_DIST_FROM_ORIGIN = 7`, three `nextFloat` pairs for `xd/yd/zd`), and —
//! for the first target state whose `OreFeature.canPlaceOre` gate passes — writes
//! that state (`level.setBlock(targetPos, targetState.state,
//! Block.UPDATE_CLIENTS)`), before returning `true` unconditionally.
//!
//! The offset helpers are fully ported and test-pinned, and the write
//! decision is live: [`can_place_ore`] evaluates the erased `RuleTest` via the
//! templatesystem `erased_test` downcast dispatch, and the write routes
//! through `level.set_block(target_pos, target_state.state, UPDATE_CLIENTS)`
//! (`Block.UPDATE_CLIENTS = 2`, matching Java's `setBlockState` with the
//! update-flag). `place` returns `true` unconditionally (Java's final
//! statement) regardless of how many tries wrote. The shared feature-core
//! `#181` dispatch arm (id 51, in `mod.rs`) resolves against this unit's
//! `SCATTERED_ORE`; this unit provides the `ScatteredOreFeature`/`SCATTERED_ORE`
//! types, not the dispatch wiring.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::OreConfiguration;
use crate::levelgen::feature::ore_feature::can_place_ore;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `ScatteredOreFeature.MAX_DIST_FROM_ORIGIN` — the per-try axis cap.
const MAX_DIST_FROM_ORIGIN: i32 = 7;

/// `Block.UPDATE_CLIENTS` — the write-flag constant the scattered-ore writes
/// use.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.ScatteredOreFeature`.
#[derive(Debug)]
pub struct ScatteredOreFeature;

/// `Feature.SCATTERED_ORE` — the registered `minecraft:scattered_ore`
/// singleton.
pub const SCATTERED_ORE: ScatteredOreFeature = ScatteredOreFeature;

impl ScatteredOreFeature {
    /// `ScatteredOreFeature.getRandomPlacementInOneAxisRelativeToOrigin(
    /// RandomSource, int)` — `Math.round((random.nextFloat() -
    /// random.nextFloat()) * maxDistanceFromOrigin)`. `Math.round(float)` is
    /// `floor(x + 0.5f)` with ties toward positive infinity — NOT `f32::round`
    /// (half-away-from-zero): `-0.5f` → `0` here, `-1` there. The saturating
    /// cast also covers `Math.round`'s `NaN`/`±Inf` cases; with draws in
    /// `[0, 1)` and distance `[0, 7]` only the tie-to-`+inf` path is reached.
    /// The load-bearing property is that tie-to-`+inf` rounding.
    fn get_random_placement_in_one_axis_relative_to_origin<R: RandomSource>(
        random: &mut R,
        max_distance_from_origin: i32,
    ) -> i32 {
        let x = (random.next_float() - random.next_float()) * max_distance_from_origin as f32;
        (x + 0.5f32).floor() as i32
    }

    /// `ScatteredOreFeature.offsetTargetPos(MutableBlockPos, RandomSource,
    /// BlockPos, int)` — set the target to the origin offset by a per-axis
    /// `getRandomPlacementInOneAxisRelativeToOrigin` draw (three `nextFloat`
    /// pairs, `xd`/`yd`/`zd` in order). The port returns the offset `BlockPos`
    /// instead of mutating Java's `MutableBlockPos`.
    fn offset_target_pos<R: RandomSource>(
        random: &mut R,
        origin: &BlockPos,
        max_dist_from_origin_for_this_try: i32,
    ) -> BlockPos {
        let xd = Self::get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        let yd = Self::get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        let zd = Self::get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        origin.offset(xd, yd, zd)
    }
}

impl FeatureBehavior<OreConfiguration> for ScatteredOreFeature {
    /// `ScatteredOreFeature.place(FeaturePlaceContext<OreConfiguration>)`:
    /// `numberOfTries = nextInt(size + 1)`; per try the target is offset by
    /// `Math.min(i, 7)` per axis (three `nextFloat` pairs); the first
    /// `canPlaceOre`-passing target is written with `Block.UPDATE_CLIENTS`
    /// (2). Returns `true` unconditionally (Java's final statement), even when
    /// no target passed. `can_place_ore` evaluates the erased `RuleTest` via
    /// the [`erased_test`] dispatch and reads the level (the inlined
    /// `|pos| level.get_block_state(pos)` mirroring Java's
    /// `level::getBlockState` block getter).
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, OreConfiguration, R>,
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
        let origin: &BlockPos = origin;
        let config: &OreConfiguration = config;
        let number_of_tries = random.next_int_bound(config.size.wrapping_add(1));

        for i in 0..number_of_tries {
            let target_pos = Self::offset_target_pos(random, origin, i.min(MAX_DIST_FROM_ORIGIN));
            let block_state = level.get_block_state(&target_pos);

            for target_state in &config.target_states {
                if can_place_ore(
                    &block_state,
                    |pos| level.get_block_state(pos),
                    random,
                    config,
                    target_state,
                    &target_pos,
                ) {
                    level.set_block(&target_pos, target_state.state, UPDATE_CLIENTS);
                    break;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_util::RandomSource;
    use rivet_util::random::LegacyRandomSource;

    /// `Math.round((nextFloat - nextFloat) * maxDist)` — the exact draw order
    /// (two `nextFloat`s per axis, three axes) and the floor(x + 0.5)
    /// rounding, pinned against a real `LegacyRandomSource` draw stream.
    #[test]
    fn offset_target_pos_draws_three_axis_pairs_and_rounds_java_style() {
        // A custom random whose next_float sequence is scripted, so the Java
        // `(f1 - f2) * maxDist -> Math.round` mapping is pinned on exact
        // fractions (including the tie-to-+inf cases `-0.5`/`0.5`).
        #[derive(Clone, Copy)]
        struct Scripted(&'static [f32]);
        impl RandomSource for Scripted {
            type Positional = rivet_util::random::LegacyPositionalRandomFactory;
            fn fork(&mut self) -> Self {
                *self
            }
            fn fork_positional(&mut self) -> Self::Positional {
                rivet_util::random::LegacyPositionalRandomFactory::new(0)
            }
            fn set_seed(&mut self, _seed: i64) {}
            fn next_int(&mut self) -> i32 {
                0
            }
            fn next_int_bound(&mut self, _bound: i32) -> i32 {
                0
            }
            fn next_long(&mut self) -> i64 {
                0
            }
            fn next_boolean(&mut self) -> bool {
                false
            }
            fn next_float(&mut self) -> f32 {
                let v = self.0[0];
                self.0 = &self.0[1..];
                v
            }
            fn next_double(&mut self) -> f64 {
                0.0
            }
            fn next_gaussian(&mut self) -> f64 {
                0.0
            }
        }
        // Six draws: xd uses (1.0, 0.0) -> (1.0 - 0.0) * 7 = 7 -> 7; yd uses
        // (0.0, 0.5) -> (0.0 - 0.5) * 7 = -3.5 -> floor(-3.0) = -3; zd uses
        // (0.5, 0.5) -> 0 -> 0.
        let mut random = Scripted(&[1.0, 0.0, 0.0, 0.5, 0.5, 0.5]);
        let target = ScatteredOreFeature::offset_target_pos(
            &mut random,
            &BlockPos::new(10, 20, 30),
            MAX_DIST_FROM_ORIGIN,
        );
        assert_eq!(target, BlockPos::new(17, 17, 30));
        assert_eq!(random.0.len(), 0, "exactly six draws consumed");
    }

    /// The rounding helper distinguishes Java `Math.round` from `f32::round`:
    /// the tie at `-0.5` rounds toward positive infinity (`0`), not away from
    /// zero (`-1`). Exposed via the single-axis helper with `maxDist = 1`.
    #[test]
    fn round_ties_toward_positive_infinity() {
        /// A two-draw `RandomSource`: `next_float` yields `self.0` then
        /// `self.1` (consumed once), so the helper's two `nextFloat` draws are
        /// the pair in order.
        #[derive(Clone, Copy)]
        struct Pair(f32, f32);
        impl RandomSource for Pair {
            type Positional = rivet_util::random::LegacyPositionalRandomFactory;
            fn fork(&mut self) -> Self {
                *self
            }
            fn fork_positional(&mut self) -> Self::Positional {
                rivet_util::random::LegacyPositionalRandomFactory::new(0)
            }
            fn set_seed(&mut self, _seed: i64) {}
            fn next_int(&mut self) -> i32 {
                0
            }
            fn next_int_bound(&mut self, _bound: i32) -> i32 {
                0
            }
            fn next_long(&mut self) -> i64 {
                0
            }
            fn next_boolean(&mut self) -> bool {
                false
            }
            fn next_float(&mut self) -> f32 {
                let v = self.0;
                std::mem::swap(&mut self.0, &mut self.1);
                v
            }
            fn next_double(&mut self) -> f64 {
                0.0
            }
            fn next_gaussian(&mut self) -> f64 {
                0.0
            }
        }
        // (0.0, 0.5) * 1 = -0.5: Java -> floor(0.0) = 0 (f32::round would give -1).
        let mut random = Pair(0.0, 0.5);
        assert_eq!(
            ScatteredOreFeature::get_random_placement_in_one_axis_relative_to_origin(
                &mut random,
                1
            ),
            0
        );
        // (0.5, 0.0) * 1 = +0.5: floor(1.0) = 1 (both agree).
        let mut random = Pair(0.5, 0.0);
        assert_eq!(
            ScatteredOreFeature::get_random_placement_in_one_axis_relative_to_origin(
                &mut random,
                1
            ),
            1
        );
    }

    /// `place` returns `true` unconditionally (Java's final statement) even
    /// when the per-try write decision is unavailable — the draw/offset walk
    /// runs before the deferred write branch. Uses a non-zero `size` so the
    /// per-try offset walk actually runs.
    #[test]
    fn place_returns_true_unconditionally() {
        let mut level = crate::levelgen::feature::test_support::TestLevel::over(
            crate::levelgen::feature::test_support::access(),
        );
        let origin = BlockPos::new(0, 64, 0);
        let generator = crate::levelgen::feature::test_support::TestGenerator;
        let config = OreConfiguration::new_without_discard_chance(Vec::new(), 8);
        let mut random = LegacyRandomSource::new(1);
        let result = SCATTERED_ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(
            result,
            "ScatteredOreFeature.place returns true unconditionally"
        );
    }

    /// Non-vacuous drive of the per-try offset walk with a live write: with
    /// `numberOfTries > 0` and an always-true target, each try offsets the
    /// target (three `nextFloat` pairs), reads it through the `TestLevel`
    /// map, passes `can_place_ore`, and records the write with
    /// `Block.UPDATE_CLIENTS` (2). With discard chance 0 the gate's
    /// `should_skip_air_check` short-circuits without a draw, so each try
    /// consumes exactly six floats and writes exactly once: the write count
    /// equals the float count divided by six.
    #[test]
    fn place_walks_per_try_offsets_and_writes_with_update_clients() {
        use crate::levelgen::feature::configurations::TargetBlockState;
        use crate::levelgen::feature::test_support::{
            RecordingRandom, RngCall, TestGenerator, TestLevel, access,
        };
        use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
        use rivet_registry::generated::blocks::BlockId;
        use std::sync::Arc;

        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let ore_state = BlockState::of(BlockId(1));
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(Arc::new(AlwaysTrueTest), ore_state)],
            8,
        );
        let mut random = RecordingRandom::new(1);
        let result = SCATTERED_ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(
            result,
            "ScatteredOreFeature.place returns true unconditionally"
        );
        // Draw order: `nextInt(size + 1)` for numberOfTries, then per try the
        // six offset floats (three axis pairs). The always-true rule and the
        // discard-chance-0 short-circuit draw nothing.
        match random.calls.first() {
            Some(RngCall::IntBound(bound)) => assert_eq!(*bound, 9, "nextInt(size + 1)"),
            other => panic!("first draw must be nextInt(size + 1), got {other:?}"),
        }
        let per_try_floats = random
            .calls
            .iter()
            .skip(1)
            .filter(|call| matches!(call, RngCall::Float))
            .count();
        assert!(
            per_try_floats > 0,
            "a non-zero numberOfTries draws at least six floats"
        );
        assert_eq!(
            per_try_floats % 6,
            0,
            "each try consumes exactly six floats (three axis pairs)"
        );
        assert_eq!(
            level.writes.len(),
            per_try_floats / 6,
            "each try with an always-true target writes exactly once"
        );
        assert_eq!(
            level.writes_flags.len(),
            level.writes.len(),
            "every write records its update flag"
        );
        for (i, (pos, state)) in level.writes.iter().enumerate() {
            assert_eq!(*state, ore_state, "write carries the target state");
            assert_eq!(
                level.writes_flags[i], UPDATE_CLIENTS,
                "scattered-ore writes use Block.UPDATE_CLIENTS (2)"
            );
            // Every written position is within MAX_DIST_FROM_ORIGIN of origin
            // (the per-try offset cap `Math.min(i, 7)`).
            let dx = (pos.get_x() - origin.get_x()).unsigned_abs();
            let dy = (pos.get_y() - origin.get_y()).unsigned_abs();
            let dz = (pos.get_z() - origin.get_z()).unsigned_abs();
            assert!(dx <= 7 && dy <= 7 && dz <= 7);
        }
    }

    /// The erased-`RuleTest` dispatch resolution on the `can_place_ore` gate:
    /// an always-true target passes (drawing nothing — the discard-chance-0
    /// short-circuit means the air-check roll is never reached), while a
    /// `BlockStateMatchTest` that never matches fails and also consumes zero
    /// draws (Java's short-circuit). Both pins run through the same
    /// `RecordingRandom` draw stream so the zero-draw property is structural.
    #[test]
    fn can_place_ore_erased_dispatch_matches_and_skips_without_draws() {
        use crate::levelgen::feature::configurations::TargetBlockState;
        use crate::levelgen::feature::ore_feature::can_place_ore;
        use crate::levelgen::feature::test_support::RecordingRandom;
        use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
        use crate::levelgen::structure::templatesystem::block_state_match_test::BlockStateMatchTest;
        use rivet_registry::generated::blocks::BlockId;
        use std::sync::Arc;

        let air = BlockState::of(BlockId(0));
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let ore_state = BlockState::of(BlockId(1));
        let target_pos = BlockPos::new(0, 64, 0);

        // Always-true target: the erased dispatch resolves to AlwaysTrueTest,
        // which matches without drawing, and with discard chance 0.0
        // `should_skip_air_check` short-circuits true without a draw either.
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(Arc::new(AlwaysTrueTest), ore_state)],
            0,
        );
        let mut random = RecordingRandom::new(1);
        assert!(
            can_place_ore(
                &air,
                |_pos| air,
                &mut random,
                &config,
                &config.target_states[0],
                &target_pos,
            ),
            "an always-true rule test passes canPlaceOre"
        );
        assert_eq!(
            random.calls,
            vec![],
            "always-true match + discard-chance-0 short-circuit consume zero draws"
        );

        // Never-match target (BlockStateMatchTest for stone over an air cell):
        // the erased dispatch evaluates to false, consuming zero draws, so the
        // gate fails before the air-check roll.
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(
                Arc::new(BlockStateMatchTest::new(stone)),
                ore_state,
            )],
            0,
        );
        let mut random = RecordingRandom::new(1);
        assert!(
            !can_place_ore(
                &air,
                |_pos| air,
                &mut random,
                &config,
                &config.target_states[0],
                &target_pos,
            ),
            "a stone-match test over an air cell fails"
        );
        assert_eq!(
            random.calls,
            vec![],
            "a non-matching rule test consumes zero draws (Java short-circuit)"
        );
    }
}
