//! Port of `net.minecraft.world.level.levelgen.feature.RandomSelectorFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.selector`
//! manifest unit.
//!
//! Java: `Feature<RandomFeatureConfiguration>` whose `place` walks
//! `config.features()` in order, and for each `WeightedPlacedFeature` returns
//! `feature.place(...)` the moment `random.nextFloat() < feature.chance()`
//! holds — drawing one `nextFloat` per entry, in list order, short-circuiting
//! at the first hit. Only when *every* entry's draw misses does it fall back to
//! `config.defaultFeature().value().place(...)`.
//!
//! `WeightedPlacedFeature.place`/`PlacedFeature.place` resolve their holders
//! through the placed/configured-feature lookups (the back-reference rule), so
//! the feature reaches both from `WorldGenLevel::registry_access` — the STUB
//! seam in `crate::level::world_gen_level` (no production level provides it
//! yet; the selector placement tests override it with the two-registry test
//! double). The `@Deprecated` status is Java's (the vanilla registrations were
//! superseded by `WeightedRandomSelectorFeature`); the codec and behavior
//! remain reachable, so the port keeps them faithfully.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::RandomFeatureConfiguration;
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.RandomSelectorFeature`.
#[derive(Debug)]
pub struct RandomSelectorFeature;

/// `Feature.RANDOM_SELECTOR` — the registered `minecraft:random_selector`
/// singleton.
pub const RANDOM_SELECTOR: RandomSelectorFeature = RandomSelectorFeature;

impl FeatureBehavior<RandomFeatureConfiguration> for RandomSelectorFeature {
    /// `RandomSelectorFeature.place(FeaturePlaceContext<RandomFeatureConfiguration>)`.
    ///
    /// ```java
    /// for (WeightedPlacedFeature feature : config.features()) {
    ///     if (random.nextFloat() < feature.chance()) {
    ///         return feature.place(level, chunkGenerator, random, origin);
    ///     }
    /// }
    /// return config.defaultFeature().value().place(level, chunkGenerator, random, origin);
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, RandomFeatureConfiguration, R>,
    ) -> bool {
        // The context exposes its fields directly (Java's accessors), so the
        // level/chunk-generator/random/origin references coexist as in Java.
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
        // a holder actually needs resolving, keeping the `.expect` panic surface
        // off the selection walk: Java short-circuits successfully on paths the
        // eager lookups would fail on (a missing registry only ever fails a
        // placement that resolves a `Reference` holder).
        for weighted in config.features() {
            if random.next_float() < weighted.chance() {
                let access = level.registry_access();
                return weighted.place(
                    access
                        .lookup(&*PLACED_FEATURE)
                        .expect("the placed-feature registry is present in the level access"),
                    access
                        .lookup(&*CONFIGURED_FEATURE)
                        .expect("the configured-feature registry is present in the level access"),
                    level,
                    chunk_generator,
                    random,
                    origin,
                );
            }
        }
        let access = level.registry_access();
        config
            .default_feature()
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
    use crate::levelgen::feature::configurations::RandomFeatureConfiguration;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access, configured_only_access,
        no_op_placed,
    };
    use crate::levelgen::feature::weighted_placed_feature::WeightedPlacedFeature;
    use rivet_registry::core::BlockPos;

    fn weighted(chance: f32) -> WeightedPlacedFeature {
        WeightedPlacedFeature::new(no_op_placed(), chance)
    }

    /// A config whose every weighted draw misses (`chance` 0.0 — a `nextFloat`
    /// in `[0, 1)` is never `< 0.0`), forcing the `defaultFeature` fallback.
    fn miss_to_default() -> RandomFeatureConfiguration {
        RandomFeatureConfiguration::new(vec![weighted(0.0), weighted(0.0)], no_op_placed())
    }

    /// A config whose first entry's chance is `1.0` — every `nextFloat` is
    /// `< 1.0`, so the first entry wins (no draw reaches the second).
    fn first_wins() -> RandomFeatureConfiguration {
        RandomFeatureConfiguration::new(vec![weighted(1.0), weighted(1.0)], no_op_placed())
    }

    fn place(
        level: &mut TestLevel,
        random: &mut RecordingRandom,
        config: &RandomFeatureConfiguration,
    ) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(3, 64, -7);
        RANDOM_SELECTOR.place(&mut FeaturePlaceContext::new(
            None, level, &generator, random, &origin, config,
        ))
    }

    #[test]
    fn falls_back_to_default_when_all_draws_miss() {
        // `random.nextFloat() < feature.chance()` — with `chance` 0.0 every draw
        // misses, so the default feature is placed. `nextFloat` is drawn once
        // per entry, in list order.
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(42);
        assert!(place(&mut level, &mut random, &miss_to_default()));
        assert_eq!(
            random.calls,
            vec![RngCall::Float, RngCall::Float],
            "one nextFloat per weighted entry, in order"
        );
    }

    #[test]
    fn short_circuits_at_the_first_hit() {
        // `chance` 1.0 — the first entry's draw always hits, so exactly one
        // `nextFloat` is drawn and the second entry is never tested.
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(7);
        assert!(place(&mut level, &mut random, &first_wins()));
        assert_eq!(random.calls, vec![RngCall::Float]);
    }

    /// The `ensureCanWrite` gate (`Feature.place(FC, …)`'s
    /// `level.ensureCanWrite(origin)`) is applied by the dispatch before this
    /// feature's `place` runs, so a false gate short-circuits the whole
    /// selection: no `nextFloat` is drawn. The test asserts that ordering by
    /// tripping the gate on the double and checking the RNG stays untouched.
    #[test]
    fn write_gate_short_circuits_before_any_draw() {
        let mut level = TestLevel::over(access());
        level.can_write = false;
        let mut random = RecordingRandom::new(1);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = RANDOM_SELECTOR.place_with_config(
            &miss_to_default(),
            &mut level,
            &generator,
            &mut random,
            &origin,
        );
        assert!(!placed);
        assert!(random.calls.is_empty());
    }

    /// Hostile: the selector resolves its lookups from
    /// `WorldGenLevel::registry_access`; an access missing the placed-feature
    /// registry fails explicitly (never fabricating a default placement).
    #[test]
    #[should_panic(expected = "the placed-feature registry is present in the level access")]
    fn missing_placed_registry_fails_explicitly() {
        let mut level = TestLevel::over(configured_only_access());
        let mut random = RecordingRandom::new(1);
        let _ = place(&mut level, &mut random, &miss_to_default());
    }
}
