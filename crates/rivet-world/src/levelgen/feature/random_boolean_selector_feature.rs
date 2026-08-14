//! Port of `net.minecraft.world.level.levelgen.feature.RandomBooleanSelectorFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.selector`
//! manifest unit.
//!
//! Java: `Feature<RandomBooleanFeatureConfiguration>` whose `place` draws a
//! single `random.nextBoolean()` and places `featureTrue` on `true` /
//! `featureFalse` on `false`, resolving the chosen `Holder<PlacedFeature>` and
//! delegating. Exactly one `nextBoolean` draw, and exactly one placed feature
//! — never both.
//!
//! The chosen placed feature resolves through the placed/configured-feature
//! lookups threaded from `WorldGenLevel::registry_access` (the STUB seam in
//! `crate::level::world_gen_level`; see the `random_selector_feature` module
//! doc for the back-reference-rule rationale).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::RandomBooleanFeatureConfiguration;
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.RandomBooleanSelectorFeature`.
#[derive(Debug)]
pub struct RandomBooleanSelectorFeature;

/// `Feature.RANDOM_BOOLEAN_SELECTOR` — the registered
/// `minecraft:random_boolean_selector` singleton.
pub const RANDOM_BOOLEAN_SELECTOR: RandomBooleanSelectorFeature = RandomBooleanSelectorFeature;

impl FeatureBehavior<RandomBooleanFeatureConfiguration> for RandomBooleanSelectorFeature {
    /// `RandomBooleanSelectorFeature.place(FeaturePlaceContext<RandomBooleanFeatureConfiguration>)`.
    ///
    /// ```java
    /// boolean result = random.nextBoolean();
    /// return (result ? config.featureTrue : config.featureFalse).value()
    ///     .place(level, chunkGenerator, random, origin);
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, RandomBooleanFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            chunk_generator,
            random,
            origin,
            config,
            ..
        } = context;
        // The destructured bindings are `&mut` into the context's fields (the
        // `&mut context` reborrow), so each is deref'd to the concrete reference
        // the nested `place` calls take (`&mut dyn WorldGenLevel` etc. — the
        // generic `random` cannot deref-coerce through `&mut R`).
        let level: &mut dyn WorldGenLevel = &mut **level;
        let chunk_generator: &dyn ChunkGenerator = *chunk_generator;
        let random: &mut R = random;
        let origin = *origin;
        let config = *config;
        // Java never touches a RegistryAccess here — `Holder.value()` resolves
        // through the value stored in the holder. The Rust `Reference` resolves
        // by id (the back-reference rule), so a placement resolves its lookups
        // from the level's access (see the module doc) — but only at the point
        // a holder actually needs resolving (after the boolean draw), keeping
        // the `.expect` panic surface off the paths Java short-circuits.
        let result = random.next_boolean();
        let chosen = if result {
            &config.feature_true
        } else {
            &config.feature_false
        };
        let access = level.registry_access();
        chosen
            .value(
                access
                    .lookup(&*PLACED_FEATURE)
                    .expect("the placed-feature registry is present in the level access"),
            )
            .place(
                access
                    .lookup(&*CONFIGURED_FEATURE)
                    .expect("the configured-feature registry is present in the level access"),
                level,
                chunk_generator,
                random,
                origin,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access, failing_placed, no_op_placed,
    };
    use rivet_registry::core::BlockPos;

    /// The differentiated-branch config: `featureTrue` is the
    /// `minecraft:sea_pickle` leaf, which never writes on a default `TestLevel`
    /// (no water cell, no survival — see `test_support::failing_placed`) and
    /// returns `false`; `featureFalse` is the `minecraft:no_op` leaf, which
    /// always returns `true`. The placed verdict — and the recorded RNG calls
    /// (the failing leaf draws its per-attempt offsets, the no-op leaf draws
    /// nothing) — reveal which branch the single boolean draw routed, so a swap
    /// of the two branches would flip both observables.
    fn config() -> RandomBooleanFeatureConfiguration {
        RandomBooleanFeatureConfiguration::new(failing_placed(), no_op_placed())
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        RANDOM_BOOLEAN_SELECTOR.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// Seed 3's first `nextBoolean()` draws `true`, routing the failing
    /// `featureTrue` branch: the draws are the single boolean then the sea-pickle
    /// leaf's per-attempt `nextInt(8)`×4 + `nextInt(4)` (the count is a constant
    /// 1), and the verdict is the leaf's `false` — pinning that the *chosen*
    /// branch (not the unchosen no-op) placed.
    #[test]
    fn draws_exactly_one_boolean_and_routes_the_true_branch() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(3);
        assert!(!place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            vec![
                RngCall::Boolean,
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(4),
            ]
        );
    }

    /// Seed 4096's first `nextBoolean()` draws `false`, routing the no-op
    /// `featureFalse` branch: exactly one draw (the boolean; the no-op leaf draws
    /// nothing) and a `true` verdict — pinning the other routing direction.
    #[test]
    fn draws_exactly_one_boolean_and_routes_the_false_branch() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(4096);
        assert!(place(&mut level, &mut random));
        assert_eq!(random.calls, vec![RngCall::Boolean]);
    }

    /// The `ensureCanWrite` gate short-circuits before the boolean draw (see
    /// the `random_selector_feature` write-gate test for the reasoning).
    #[test]
    fn write_gate_short_circuits_before_any_draw() {
        let mut level = TestLevel::over(access());
        level.can_write = false;
        let mut random = RecordingRandom::new(3);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = RANDOM_BOOLEAN_SELECTOR.place_with_config(
            &config(),
            &mut level,
            &generator,
            &mut random,
            &origin,
        );
        assert!(!placed);
        assert!(random.calls.is_empty());
    }

    /// Hostile: a missing placed-feature registry fails explicitly (never
    /// fabricating a chosen placement).
    #[test]
    #[should_panic(expected = "the placed-feature registry is present in the level access")]
    fn missing_placed_registry_fails_explicitly() {
        let mut level =
            TestLevel::over(crate::levelgen::feature::test_support::configured_only_access());
        let mut random = RecordingRandom::new(3);
        let _ = place(&mut level, &mut random);
    }
}
