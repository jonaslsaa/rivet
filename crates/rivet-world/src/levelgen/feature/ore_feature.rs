//! Port of `net.minecraft.world.level.levelgen.feature.OreFeature` (26.2) — the
//! `mc.world.level.levelgen.feature.ore` manifest unit.
//!
//! This unit ports the pure static helper surface only. `shouldSkipAirCheck` is
//! fully ported: it reads only the RNG and the `discardChanceOnAirExposure`
//! float, so it is faithful and reachable today. `canPlaceOre` DEFERS: its
//! first conjunct evaluates `targetState.target().test(orePosState, random)` on
//! the erased `RuleTest` carrier, and the templatesystem unit's
//! `ErasedRuleTest` deliberately has no object-safe `test` (`RandomSource` is
//! `Sized`, so `RuleTest::test` is not dispatchable through `dyn`); the erased
//! evaluation surface is owned by that unit and is not ported anywhere yet.
//! (RivetTodo(#399): the `place`/`doPlace` bodies and `ScatteredOreFeature`
//! additionally write blocks through `WorldGenLevel.setBlock`/`getBlockState`,
//! which also defer — the `#399` seam covers those.)

use rivet_util::RandomSource;

/// `OreFeature.shouldSkipAirCheck(RandomSource, float discardChanceOnAirExposure)`
/// — `discardChanceOnAirExposure <= 0.0F || (!(discardChanceOnAirExposure >=
/// 1.0F) && random.nextFloat() >= discardChanceOnAirExposure)`. Note the Java
/// short-circuit ordering: a value in `(0.0F, 1.0F)` rolls `nextFloat() >=
/// discardChanceOnAirExposure`; a value `>= 1.0F` never skips.
///
/// `#[allow(clippy::neg_cmp_op_on_partial_ord)]`: the mechanical rewrite of
/// `!(x >= 1.0)` to `x < 1.0` is NOT behavior-preserving for f32 — for NaN the
/// original evaluates `!(NaN >= 1.0)` to `true` and rolls `nextFloat() >= NaN`
/// (consuming an RNG draw), while `NaN < 1.0` short-circuits without a draw.
/// Clippy still flags the faithful form, so the lint is suppressed on it.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn should_skip_air_check(
    random: &mut impl RandomSource,
    discard_chance_on_air_exposure: f32,
) -> bool {
    discard_chance_on_air_exposure <= 0.0
        || (!(discard_chance_on_air_exposure >= 1.0)
            && random.next_float() >= discard_chance_on_air_exposure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_util::random::LegacyRandomSource;

    #[test]
    fn should_skip_air_check_zero_discard_always_skips() {
        // `discardChanceOnAirExposure <= 0.0F` short-circuits true — no RNG draw.
        let mut random = LegacyRandomSource::new(1);
        assert!(should_skip_air_check(&mut random, 0.0));
        assert!(should_skip_air_check(&mut random, -1.0));
    }

    #[test]
    fn should_skip_air_check_full_discard_never_skips() {
        // `>= 1.0F` — the second branch is skipped entirely, so always false.
        let mut random = LegacyRandomSource::new(1);
        assert!(!should_skip_air_check(&mut random, 1.0));
        assert!(!should_skip_air_check(&mut random, 2.0));
    }

    #[test]
    fn should_skip_air_check_mid_discard_draws_next_float() {
        // For `discardChanceOnAirExposure = 0.5` the body is exactly
        // `random.nextFloat() >= 0.5`: the check consumes one draw, so an
        // identically-seeded source must produce the same bit as the check.
        // Pin a few seeds to make the draw path (and both outcomes) load-bearing.
        for seed in [1i64, 7, 42, 12345, -2] {
            let mut expected_source = LegacyRandomSource::new(seed);
            let expected = expected_source.next_float() >= 0.5;

            let mut checked_source = LegacyRandomSource::new(seed);
            assert_eq!(
                should_skip_air_check(&mut checked_source, 0.5),
                expected,
                "seed {seed}"
            );
        }
    }
}
