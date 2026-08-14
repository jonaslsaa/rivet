//! Port of `net.minecraft.world.level.levelgen.feature.WeightedRandomSelectorFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.selector`
//! manifest unit.
//!
//! Java: `Feature<WeightedRandomFeatureConfiguration>` whose `place` picks via
//! `config.features().getRandom(random)` — one `nextInt(totalWeight)` draw over
//! the `WeightedList<Holder<PlacedFeature>>` — and places the selected entry,
//! or returns `false` when the list is empty. The `WeightedList::get_random`
//! port is the `#353` surface: `None` on an empty list *without* consuming RNG,
//! else exactly one `nextInt(totalWeight)` draw.
//!
//! The chosen placed feature resolves through the placed/configured-feature
//! lookups threaded from `WorldGenLevel::registry_access` (the STUB seam in
//! `crate::level::world_gen_level`; see the `random_selector_feature` module
//! doc for the back-reference-rule rationale).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::WeightedRandomFeatureConfiguration;
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.WeightedRandomSelectorFeature`.
#[derive(Debug)]
pub struct WeightedRandomSelectorFeature;

/// `Feature.WEIGHTED_RANDOM_SELECTOR` — the registered
/// `minecraft:weighted_random_selector` singleton.
pub const WEIGHTED_RANDOM_SELECTOR: WeightedRandomSelectorFeature = WeightedRandomSelectorFeature;

impl FeatureBehavior<WeightedRandomFeatureConfiguration> for WeightedRandomSelectorFeature {
    /// `WeightedRandomSelectorFeature.place(FeaturePlaceContext<WeightedRandomFeatureConfiguration>)`.
    ///
    /// ```java
    /// Optional<Holder<PlacedFeature>> featureToPlace = config.features().getRandom(random);
    /// return featureToPlace.map(holder -> holder.value().place(level, chunkGenerator, random, origin))
    ///     .orElse(false);
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, WeightedRandomFeatureConfiguration, R>,
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
        // a holder actually needs resolving, keeping the `.expect` panic surface
        // off the empty-list `orElse(false)` path Java short-circuits.
        // `WeightedList.getRandom` — `None` on an empty list (no draw), else one
        // `nextInt(totalWeight)` selection. The port's `get_random` returns the
        // holder by value (the `#353` `WeightedList` surface).
        let Some(chosen) = config.features().get_random(random) else {
            return false;
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
        RecordingRandom, RngCall, TestGenerator, TestLevel, access, configured_only_access,
        no_op_placed,
    };
    use rivet_registry::core::BlockPos;
    use rivet_util::weighted::{Weighted, WeightedList};

    /// A two-entry list with total weight 10 — a single `nextInt(10)` draw.
    fn weighted_config() -> WeightedRandomFeatureConfiguration {
        WeightedRandomFeatureConfiguration::new(WeightedList::new(&[
            Weighted::new(no_op_placed(), 9),
            Weighted::new(no_op_placed(), 1),
        ]))
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        WEIGHTED_RANDOM_SELECTOR.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &weighted_config(),
        ))
    }

    #[test]
    fn draws_one_int_bound_by_total_weight() {
        // `config.features().getRandom(random)` — one `nextInt(totalWeight)`
        // draw (10 here), then the selected member places.
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(17);
        assert!(place(&mut level, &mut random));
        assert_eq!(random.calls, vec![RngCall::IntBound(10)]);
    }

    /// An empty `WeightedList` yields `None` from `getRandom` without consuming
    /// RNG — `orElse(false)`.
    #[test]
    fn empty_list_returns_false_without_drawing() {
        let config = WeightedRandomFeatureConfiguration::new(WeightedList::new(&[]));
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(17);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = WEIGHTED_RANDOM_SELECTOR.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(!placed);
        assert!(random.calls.is_empty());
    }

    /// The `ensureCanWrite` gate short-circuits before the weighted draw (see
    /// the `random_selector_feature` write-gate test for the reasoning).
    #[test]
    fn write_gate_short_circuits_before_any_draw() {
        let mut level = TestLevel::over(access());
        level.can_write = false;
        let mut random = RecordingRandom::new(17);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = WEIGHTED_RANDOM_SELECTOR.place_with_config(
            &weighted_config(),
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
        let mut level = TestLevel::over(configured_only_access());
        let mut random = RecordingRandom::new(17);
        let _ = place(&mut level, &mut random);
    }
}
