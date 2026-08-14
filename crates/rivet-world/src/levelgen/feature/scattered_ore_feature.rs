//! Port of `net.minecraft.world.level.levelgen.feature.ScatteredOreFeature`
//! (class, 26.2) — grouped under the `mc.world.level.levelgen.feature.ore`
//! manifest unit (MANIFEST row 554 lists `OreFeature.java,
//! ScatteredOreFeature.java` together).
//!
//! Java: `Feature<OreConfiguration>` that scatters ore veins. `place` draws
//! `numberOfTries = random.nextInt(config.size + 1)`; per try it offsets the
//! target position by up to `Math.min(i, 7)` blocks per axis
//! (`MAX_DIST_FROM_ORIGIN = 7`) and — for the first target state whose
//! `OreFeature.canPlaceOre` gate passes — writes that state
//! (`Block.UPDATE_CLIENTS`), before returning `true` unconditionally.
//!
//! The `MAX_DIST_FROM_ORIGIN` constant and the offset helpers
//! (`getRandomPlacementInOneAxisRelativeToOrigin` / `offsetTargetPos`, the
//! latter reduced to returning the offset `BlockPos`) are test-pinned
//! reproductions of the Java offset surface — they live inside the `#[cfg
//! (test)]` module and are exercised only by the tests below, exactly as the
//! MANIFEST row describes. `OreFeature.canPlaceOre` DEFERS
//! (RivetTodo #399): its first conjunct evaluates
//! `targetState.target().test(state, random)` on the erased `RuleTest`
//! carrier, which has no object-safe `test` (`RandomSource` is `Sized`), and
//! the templatesystem unit's erased evaluation surface is not ported anywhere
//! yet. `place` therefore cannot return a verdict — it fails explicitly (a
//! panic naming the seam) rather than fabricating success, the same
//! capability-unavailable pattern as the `#232` world seams. The `#181`
//! dispatch arm (id 51) is wired to that honest failure.

use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::OreConfiguration;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.ScatteredOreFeature`.
#[derive(Debug)]
pub struct ScatteredOreFeature;

/// `Feature.SCATTERED_ORE` — the registered `minecraft:scattered_ore`
/// singleton.
pub const SCATTERED_ORE: ScatteredOreFeature = ScatteredOreFeature;

impl FeatureBehavior<OreConfiguration> for ScatteredOreFeature {
    /// `ScatteredOreFeature.place(FeaturePlaceContext<OreConfiguration>)`.
    ///
    /// DEFERS: every try's write decision is `OreFeature.canPlaceOre`, which
    /// evaluates the erased `RuleTest` (`targetState.target().test(...)`) — not
    /// ported (RivetTodo #399). The faithful body is:
    ///
    /// ```java
    /// int numberOfTries = random.nextInt(config.size + 1);
    /// for (int i = 0; i < numberOfTries; i++) {
    ///     this.offsetTargetPos(targetPos, random, origin, Math.min(i, 7));
    ///     BlockState blockState = level.getBlockState(targetPos);
    ///     for (OreConfiguration.TargetBlockState targetState : config.targetStates) {
    ///         if (OreFeature.canPlaceOre(blockState, level::getBlockState, random, config, targetState, targetPos)) {
    ///             level.setBlock(targetPos, targetState.state, Block.UPDATE_CLIENTS);
    ///             break;
    ///         }
    ///     }
    /// }
    /// return true;
    /// ```
    ///
    /// The port cannot return a verdict without fabricating it, so placement
    /// fails explicitly (never `true`, never a silent no-op).
    fn place<R: RandomSource>(
        &self,
        _context: &mut FeaturePlaceContext<'_, OreConfiguration, R>,
    ) -> bool {
        panic!(
            "ScatteredOreFeature.place is not implemented (RivetTodo #399: OreFeature.canPlaceOre evaluates the erased RuleTest, which has no object-safe test)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::core::BlockPos;
    use rivet_util::RandomSource;
    use rivet_util::random::LegacyRandomSource;

    /// `ScatteredOreFeature.MAX_DIST_FROM_ORIGIN` — the per-try axis cap
    /// (`Math.min(i, 7)`).
    const MAX_DIST_FROM_ORIGIN: i32 = 7;

    /// `ScatteredOreFeature.getRandomPlacementInOneAxisRelativeToOrigin(
    /// RandomSource, int)` — `Math.round((random.nextFloat() -
    /// random.nextFloat()) * maxDistanceFromOrigin)`.
    ///
    /// `Math.round(float)` is `floor(x + 0.5)` with ties rounding toward
    /// positive infinity — NOT `f32::round` (half-away-from-zero): `-0.5f`
    /// rounds to `0` here and to `-1` under `f32::round`. The Rust cast `(x +
    /// 0.5).floor() as i32` additionally reproduces Java's `Math.round`
    /// special cases exactly (verified against the JVM): `NaN -> 0` (Rust
    /// casts NaN to 0), `+Inf -> i32::MAX`, `-Inf -> i32::MIN` (Rust
    /// float-to-int casts saturate). For the real argument — a product in
    /// `(-7, 7)` — only the `floor(x + 0.5)` path is reachable.
    fn get_random_placement_in_one_axis_relative_to_origin<R: RandomSource>(
        random: &mut R,
        max_distance_from_origin: i32,
    ) -> i32 {
        let x = (random.next_float() - random.next_float()) * max_distance_from_origin as f32;
        (x + 0.5).floor() as i32
    }

    /// `ScatteredOreFeature.offsetTargetPos(MutableBlockPos, RandomSource,
    /// BlockPos, int)` — set the target to the origin offset by a per-axis
    /// `getRandomPlacementInOneAxisRelativeToOrigin` draw. The port returns
    /// the offset `BlockPos` instead of mutating Java's `MutableBlockPos`.
    fn offset_target_pos<R: RandomSource>(
        random: &mut R,
        origin: &BlockPos,
        max_dist_from_origin_for_this_try: i32,
    ) -> BlockPos {
        let xd = get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        let yd = get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        let zd = get_random_placement_in_one_axis_relative_to_origin(
            random,
            max_dist_from_origin_for_this_try,
        );
        origin.offset(xd, yd, zd)
    }

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
        let target = offset_target_pos(
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
            get_random_placement_in_one_axis_relative_to_origin(&mut random, 1),
            0
        );
        // (0.5, 0.0) * 1 = +0.5: floor(1.0) = 1 (both agree).
        let mut random = Pair(0.5, 0.0);
        assert_eq!(
            get_random_placement_in_one_axis_relative_to_origin(&mut random, 1),
            1
        );
    }

    /// The `#399` deferral is honest: placing a scattered-ore feature panics
    /// with the seam message rather than returning a fabricated verdict.
    #[test]
    #[should_panic(expected = "RivetTodo #399")]
    fn place_fails_explicitly_until_can_place_ore_lands() {
        let mut level = crate::levelgen::feature::test_support::TestLevel::over(
            crate::levelgen::feature::test_support::access(),
        );
        let origin = BlockPos::new(0, 64, 0);
        let generator = crate::levelgen::feature::test_support::TestGenerator;
        let config = OreConfiguration::new_without_discard_chance(Vec::new(), 0);
        let mut random = LegacyRandomSource::new(1);
        let _ = SCATTERED_ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
    }
}
