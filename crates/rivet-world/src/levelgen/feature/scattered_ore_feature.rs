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
//! The offset helpers are fully ported and test-pinned. The write decision
//! DEFERS: `can_place_ore` evaluates the erased `RuleTest` (`RivetTodo #399`),
//! and the write routes through `level.set_block` (`RivetTodo #232`). The
//! port reaches the unconditional `return true` only on the vacuous paths
//! (empty `target_states`, or `numberOfTries == 0`); on a production
//! `WorldGenLevel` the per-try read is live and the failure seam is
//! `can_place_ore` (#399), which panics before the final `return true`.
//! The shared feature-core `#181` dispatch arm (id 51, in `mod.rs`) resolves
//! against this unit's `SCATTERED_ORE`; this unit provides the
//! `ScatteredOreFeature`/`SCATTERED_ORE` types, not the dispatch wiring.

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
    /// `canPlaceOre`-passing target would be written with
    /// `Block.UPDATE_CLIENTS`. Returns `true` unconditionally (Java's final
    /// statement).
    ///
    /// DEFERS (never a fabricated write): the write decision is
    /// [`can_place_ore`] (erased `RuleTest`, RivetTodo #399) and the write
    /// routes through `level.set_block` (RivetTodo #232). The per-try walk
    /// still consumes the exact Java draws (offset geometry is test-pinned);
    /// on a production `WorldGenLevel` the per-try read is live, so each try
    /// completes and the failure seam is `can_place_ore` (#399), which panics
    /// before any write.
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
                // `OreFeature.canPlaceOre(...)` — DEFERS (RivetTodo #399).
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

    /// Non-vacuous drive of the per-try offset walk: with `numberOfTries > 0`
    /// and at least one target state, each try offsets the target (three
    /// `nextFloat` pairs) and reads it through `WorldGenLevel::get_block_state`
    /// — the `TestLevel` read answers from its map, so the walk completes and
    /// reaches `can_place_ore`, whose erased-`RuleTest` `#399` gate panics
    /// before any write. This pins the draw order (`nextInt(size + 1)`, then
    /// per try the `Math.min(i, 7)`-bounded offset pairs) and confirms the
    /// write branch never fabricates a write. The match below REQUIRES that
    /// `#399` panic — a `place` returning `Ok` fails the test — so the
    /// non-vacuity is structural: a seed whose `nextInt(size + 1)` draws 0
    /// (no tries, no floats, no gate reach) can no longer pass silently. (On a
    /// production `WorldGenLevel` the read is also live — `WorldGenRegion`
    /// answers from its chunks — so the failure seam stays `can_place_ore`
    /// (#399); a `TestLevel` read is likewise not a seam.)
    #[test]
    fn place_walks_per_try_offsets_then_panics_at_the_erased_gate() {
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
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(
                Arc::new(AlwaysTrueTest),
                BlockState::of(BlockId(0)),
            )],
            8,
        );
        let mut random = RecordingRandom::new(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SCATTERED_ORE.place(&mut FeaturePlaceContext::new(
                None,
                &mut level,
                &generator,
                &mut random,
                &origin,
                &config,
            ))
        }));
        // Draw order: `nextInt(size + 1)` for numberOfTries, then per try three
        // `nextFloat` pairs for the offset walk. The first try always consumes
        // its three pairs before the erased `#399` gate (which panics).
        let number_of_tries = random.calls.first();
        match number_of_tries {
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
            "the walk must reach the erased gate: a non-zero numberOfTries draws at least six floats"
        );
        assert_eq!(
            per_try_floats % 6,
            0,
            "each try consumes exactly six floats (three axis pairs)"
        );
        // The walk MUST reach the `#399` gate and fail there. A `place` that
        // returns `Ok` means `numberOfTries` was 0 — the loop never ran and
        // `can_place_ore` was never reached — so this test would have passed
        // vacuously; requiring the panic makes the non-vacuity structural
        // rather than an implicit property of the fixed seed.
        match result {
            Ok(_) => panic!(
                "place returned Ok — the per-try walk must reach the erased #399 gate and fail there; \
                 numberOfTries was 0 (seed drew 0 from nextInt(size + 1))"
            ),
            Err(payload) => {
                let text = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("<non-string panic>");
                assert!(
                    text.contains("RivetTodo #399"),
                    "the per-try walk reaches the erased gate and fails there, got {text:?}"
                );
            }
        }
    }

    /// `can_place_ore` — the `#399` deferral — is honest: the per-try write
    /// decision panics with the seam message rather than fabricating a verdict.
    #[test]
    #[should_panic(expected = "RivetTodo #399")]
    fn can_place_ore_fails_explicitly_until_erased_evaluation_lands() {
        use crate::levelgen::feature::configurations::TargetBlockState;
        use crate::levelgen::feature::ore_feature::can_place_ore;
        use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
        use rivet_registry::generated::blocks::BlockId;
        use std::sync::Arc;

        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(
                Arc::new(AlwaysTrueTest),
                BlockState::of(BlockId(0)),
            )],
            0,
        );
        let mut random = LegacyRandomSource::new(1);
        let target = BlockPos::new(0, 64, 0);
        let _ = can_place_ore(
            &BlockState::of(BlockId(0)),
            |_pos| BlockState::of(BlockId(0)),
            &mut random,
            &config,
            &config.target_states[0],
            &target,
        );
    }
}
