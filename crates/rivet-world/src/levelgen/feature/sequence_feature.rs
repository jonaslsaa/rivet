//! Port of `net.minecraft.world.level.levelgen.feature.SequenceFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.selector`
//! manifest unit.
//!
//! Java: `Feature<CompositeFeatureConfiguration>` whose `place` iterates
//! `context.config().features()` (a `HolderSet<PlacedFeature>`) in order,
//! placing each member and returning `false` the moment one fails; only when
//! every member places does it return `true` (Java's short-circuiting
//! `&&`-fold). An empty set trivially returns `true` without drawing.
//!
//! Each member resolves through the placed/configured-feature lookups threaded
//! from `WorldGenLevel::registry_access` (the STUB seam in
//! `crate::level::world_gen_level`; see the `random_selector_feature` module
//! doc for the back-reference-rule rationale).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::CompositeFeatureConfiguration;
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.SequenceFeature`.
#[derive(Debug)]
pub struct SequenceFeature;

/// `Feature.SEQUENCE` — the registered `minecraft:sequence` singleton.
pub const SEQUENCE: SequenceFeature = SequenceFeature;

impl FeatureBehavior<CompositeFeatureConfiguration> for SequenceFeature {
    /// `SequenceFeature.place(FeaturePlaceContext<CompositeFeatureConfiguration>)`.
    ///
    /// ```java
    /// for (Holder<PlacedFeature> feature : context.config().features()) {
    ///     if (!feature.value().place(context.level(), context.chunkGenerator(),
    ///             context.random(), context.origin())) {
    ///         return false;
    ///     }
    /// }
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, CompositeFeatureConfiguration, R>,
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
        // a holder actually needs resolving (inside the loop), keeping the
        // `.expect` panic surface off the empty-set `true` path Java
        // short-circuits.
        for feature in config.features().iter() {
            let access = level.registry_access();
            if !feature
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
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access, configured_only_access, failing_placed,
        no_op_placed,
    };
    use rivet_registry::core::BlockPos;
    use rivet_registry::holder_set::HolderSet;

    fn config() -> CompositeFeatureConfiguration {
        CompositeFeatureConfiguration::new(HolderSet::direct(vec![no_op_placed(), no_op_placed()]))
    }

    /// A sequence with a failing member first — the short-circuit-on-false
    /// fixture: the leading `minecraft:sea_pickle` leaf writes nothing on a
    /// default `TestLevel` and returns `false`, so the `&&`-fold must stop
    /// there and never place the trailing member.
    fn failing_first_config() -> CompositeFeatureConfiguration {
        CompositeFeatureConfiguration::new(HolderSet::direct(vec![
            failing_placed(),
            no_op_placed(),
        ]))
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        SEQUENCE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    #[test]
    fn places_every_member_in_order() {
        // Both members are the `minecraft:no_op` leaf (always `true`), so the
        // sequence returns `true` — and draws nothing itself (the leaves draw
        // nothing; the recording asserts the sequence adds no draws of its own).
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(5);
        assert!(place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// A member that fails placement short-circuits the `&&`-fold — the
    /// sequence returns `false` and never places the trailing member (the
    /// Java `return false` the `!feature.value().place(...)` guard takes).
    /// The leading `minecraft:sea_pickle` leaf fails on a default `TestLevel`
    /// (no water cell, no survival — see `test_support::failing_placed`), so
    /// `placed_any` never becomes true and the verdict is the leaf's `false`.
    #[test]
    fn failing_member_short_circuits_and_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(5);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = SEQUENCE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &failing_first_config(),
        ));
        assert!(!placed);
    }

    /// An empty set is vacuously placed — `true`, no draws.
    #[test]
    fn empty_sequence_returns_true() {
        let config = CompositeFeatureConfiguration::new(HolderSet::direct(Vec::new()));
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(5);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed = SEQUENCE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(placed);
        assert!(random.calls.is_empty());
    }

    /// The `ensureCanWrite` gate short-circuits before any member places (see
    /// the `random_selector_feature` write-gate test for the reasoning).
    #[test]
    fn write_gate_short_circuits_before_any_member() {
        let mut level = TestLevel::over(access());
        level.can_write = false;
        let mut random = RecordingRandom::new(5);
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let placed =
            SEQUENCE.place_with_config(&config(), &mut level, &generator, &mut random, &origin);
        assert!(!placed);
        assert!(random.calls.is_empty());
    }

    /// Hostile: a missing placed-feature registry fails explicitly (never
    /// fabricating a member placement).
    #[test]
    #[should_panic(expected = "the placed-feature registry is present in the level access")]
    fn missing_placed_registry_fails_explicitly() {
        let mut level = TestLevel::over(configured_only_access());
        let mut random = RecordingRandom::new(5);
        let _ = place(&mut level, &mut random);
    }
}
