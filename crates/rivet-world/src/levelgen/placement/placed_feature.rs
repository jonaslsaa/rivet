//! Port of `net.minecraft.world.level.levelgen.placement.PlacedFeature`
//! (record, 26.2).
//!
//! Java: `PlacedFeature(Holder<ConfiguredFeature<?, ?>> feature, List<PlacementModifier> placement)`.
//! The Rust port keeps the value record (the feature holder + the ordered
//! modifier list) and ports the placement pipeline: `place` builds a
//! `PlacementContext` with no top feature, `placeWithBiomeCheck` with `this`,
//! and `placeWithContext` walks `Stream.of(origin)` depth-first through each
//! modifier's `getPositions`, placing the configured feature at each resulting
//! position. `getFeatures` concatenates the holder with the configured
//! feature's sub-features.
//!
//! The modifier list is stored erased (`Arc<dyn ErasedPlacementModifier>`), the
//! object-safe carrier every `PlacementModifier` implements; the per-modifier
//! dispatch is the `#181` codegen hub `placement_get_positions`, mirroring how
//! `ConfiguredFeature` dispatches features through `feature_place`.
//!
//! Two Java surfaces defer with their unported dependencies:
//! - `feature.value()` resolves a registry-backed holder through a
//!   `HolderLookup`; the configured-feature registry lives in `rivet-registry`
//!   (#126) and its lookup isn't threaded through placement yet, so this port
//!   resolves the holder's inline `Direct` value and panics on any registry
//!   reference. Java's `Holder.Reference.value()` only throws on a genuinely
//!   unbound reference ("Trying to access unbound value") and resolves a bound
//!   one; the port has no lookup to resolve through, so the panic is broader —
//!   deferred until #126 threads the configured-feature `HolderLookup`.
//! - `SharedConstants.DEBUG_FEATURE_COUNT` + `FeatureCountTracker.featurePlaced`
//!   (keyed on `ServerLevel`, `rivet-server`) defer; the tracker is STUB'd in
//!   `feature.core`.
//!
//! See `place_walk` for the depth-first interleaving that reproduces Java's
//! lazy `flatMap` + `forEach` (the single authoritative parity account; the
//! module doc in `mod.rs` references it).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::placement::{
    ErasedPlacementModifier, PlacementContext, placement_get_positions,
};
use rivet_registry::Holder;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.PlacedFeature` — a configured
/// feature paired with the placement modifiers that scatter its positions.
#[derive(Debug, Clone)]
pub struct PlacedFeature {
    /// `PlacedFeature.feature` — `Holder<ConfiguredFeature<?, ?>>`.
    pub feature: Holder<ConfiguredFeatureErased>,
    /// `PlacedFeature.placement` — the ordered `List<PlacementModifier>`,
    /// stored erased for the `dyn`-held list.
    pub placement: Vec<Arc<dyn ErasedPlacementModifier>>,
}

impl PlacedFeature {
    /// `new PlacedFeature(Holder<ConfiguredFeature<?, ?>>, List<PlacementModifier>)`
    /// — the record constructor.
    pub fn new(
        feature: Holder<ConfiguredFeatureErased>,
        placement: Vec<Arc<dyn ErasedPlacementModifier>>,
    ) -> Self {
        PlacedFeature { feature, placement }
    }

    /// `PlacedFeature.feature()` — the accessor.
    pub fn feature(&self) -> &Holder<ConfiguredFeatureErased> {
        &self.feature
    }

    /// `PlacedFeature.placement()` — the accessor.
    pub fn placement(&self) -> &[Arc<dyn ErasedPlacementModifier>] {
        &self.placement
    }

    /// `PlacedFeature.place(WorldGenLevel, ChunkGenerator, RandomSource,
    /// BlockPos)` — `placeWithContext(new PlacementContext(level, generator,
    /// Optional.empty()), random, origin)`.
    pub fn place<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        self.place_with_context(level, generator, random, origin, None)
    }

    /// `PlacedFeature.placeWithBiomeCheck(...)` — same with the top feature set
    /// to `this` (used by the biome decoration pass).
    pub fn place_with_biome_check<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        self.place_with_context(level, generator, random, origin, Some(self))
    }

    /// `placeWithContext` — walk `Stream.of(origin)` depth-first through each
    /// modifier's `getPositions` and place the configured feature at every
    /// resulting position. The `MutableBoolean placedAny` is a plain `bool`; the
    /// `FeatureCountTracker` debug branch (gated by
    /// `SharedConstants.DEBUG_FEATURE_COUNT`, off in production) defers with
    /// the tracker STUB.
    ///
    /// See `place_walk` for the lazy-interleaving parity rationale (the single
    /// authoritative account).
    fn place_with_context<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
        top_feature: Option<&PlacedFeature>,
    ) -> bool {
        // `this.feature.value()` is resolved once, before the `forEach`
        // (Java: `ConfiguredFeature<?, ?> feature = this.feature.value();`).
        let feature = self.resolved_feature();
        let mut placed_any = false;
        self.place_walk(
            0,
            *origin,
            feature,
            level,
            generator,
            random,
            top_feature,
            &mut placed_any,
        );
        placed_any
    }

    /// The lazy-interleaving walk behind `placeWithContext`.
    ///
    /// Java's `placeWithContext` flatMaps `Stream.of(origin)` through each
    /// modifier's lazy `Stream<BlockPos>` and consumes the chain with a terminal
    /// `forEach`, so each modifier's `getPositions` runs per upstream position,
    /// depth-first, interleaved with `feature.place` writes. Every Java modifier
    /// draws eagerly *inside* `getPositions` and returns a pure stream
    /// (`RepeatingPlacement`'s `count(random, origin)` then
    /// `IntStream.range(0, count)`, `InSquarePlacement`'s two
    /// `random.nextInt(16)`, `HeightRangePlacement`'s `height.sample(...)`, …),
    /// so the port draws eagerly too (returning `Vec<BlockPos>`) and reproduces
    /// the interleaving with this walk: expand a position through the current
    /// modifier, recurse into the next, and only at the last stage place the
    /// feature — pulling one position through the whole chain before the next.
    ///
    /// That ordering is the parity-critical bit. The eager two-phase fold this
    /// walk replaces reordered the RNG draws for every chain: e.g. `count` ->
    /// `in_square` drew every position's `in_square` offsets before any
    /// placement, where Java interleaves placement draws between successive
    /// positions — so the same seed yields different coordinates/counts the
    /// moment the #181 dispatch becomes reachable. It also made a stateful
    /// modifier reading level state through `PlacementContext`
    /// (`getBlockState`/`getHeight`/`getCarvingMask`) observe pre-placement
    /// state for later positions where Java sees earlier placements' writes.
    /// This walk interleaves exactly like Java's `flatMap` + `forEach`.
    ///
    /// The `PlacementContext` is reconstructed per expansion rather than once
    /// (as Java constructs it): the context borrows the level mutably and
    /// placement writes through the same `level` reference, and the context is a
    /// pure read-only window over `(level, generator, top_feature)`, so every
    /// reconstruction is behaviorally identical to Java's single context.
    #[allow(clippy::too_many_arguments)] // the resolved-feature `&` + the `place`-signature params
    fn place_walk<R: RandomSource>(
        &self,
        index: usize,
        pos: BlockPos,
        feature: &ConfiguredFeatureErased,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        top_feature: Option<&PlacedFeature>,
        placed_any: &mut bool,
    ) {
        if index == self.placement.len() {
            if feature.place(level, generator, random, &pos) {
                *placed_any = true;
                // STUB(mc.world.level.levelgen.feature.core) — the debug-only
                // `SharedConstants.DEBUG_FEATURE_COUNT` +
                // `FeatureCountTracker.featurePlaced(level.getLevel(), feature,
                // topFeature)` branch; the tracker keys on `ServerLevel`
                // (rivet-server).
            }
            return;
        }
        let modifier = &self.placement[index];
        let positions = {
            let context = PlacementContext::new(level, generator, top_feature);
            placement_get_positions(modifier.as_ref(), &context, random, &pos)
        };
        for child in positions {
            self.place_walk(
                index + 1,
                child,
                feature,
                level,
                generator,
                random,
                top_feature,
                placed_any,
            );
        }
    }

    /// `PlacedFeature.getFeatures()` — `Stream.concat(Stream.of(this.feature),
    /// this.feature.value().getSubFeatures())` — the lazy concat iterator
    /// (Java's `Stream.concat` is lazy; sub-features are produced on demand).
    pub fn get_features(&self) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        Box::new(
            std::iter::once(self.feature.clone()).chain(self.resolved_feature().get_sub_features()),
        )
    }

    /// `this.feature.value()` — the holder's configured feature.
    ///
    /// RivetTodo(#126): a registry `Reference` panics on resolution — the
    /// configured-feature `HolderLookup` is not threaded through placement yet;
    /// the prose below details Java's narrower unbound-only throw.
    ///
    /// A `Direct` holder yields its inline value; a registry `Reference` needs
    /// the configured-feature `HolderLookup`, which is not threaded through
    /// placement yet. Java's `Reference.value()` throws only for a genuinely
    /// unbound reference and resolves a bound one; without the lookup the port
    /// cannot resolve either, so it panics on every reference — deferred until
    /// #126 threads the lookup (then route through `Holder::value`).
    fn resolved_feature(&self) -> &ConfiguredFeatureErased {
        match &self.feature {
            Holder::Direct(feature) => feature,
            Holder::Reference { registry, id } => panic!(
                "Trying to resolve configured feature id {} from registry {}: the \
                 configured-feature registry lookup is not threaded through placement \
                 (deferred with #126)",
                id, registry.0
            ),
        }
    }
}

/// `toString()` — `"Placed " + this.feature`, via the `Holder`'s `Display`
/// (`Direct{...}` / `Reference{...}`, Java's `Holder.toString()` shape).
impl fmt::Display for PlacedFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Placed {}", self.feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::configurations::FeatureConfiguration;
    use crate::levelgen::feature::{ConfiguredFeatureErased, FeatureId};
    use rivet_registry::RegistryId;

    /// A configured feature with the `None` configuration (no sub-features).
    fn no_op_feature() -> ConfiguredFeatureErased {
        ConfiguredFeatureErased {
            feature: FeatureId::new(0),
            config: Arc::new(crate::levelgen::feature::configurations::NoneFeatureConfiguration),
        }
    }

    /// A config that reports one sub-feature — `getSubFeatures` overridden.
    #[derive(Debug)]
    struct SubFeatureConfig(ConfiguredFeatureErased);

    impl FeatureConfiguration for SubFeatureConfig {
        fn get_sub_features(
            &self,
        ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
            Box::new(std::iter::once(Holder::direct(self.0.clone())))
        }
    }

    /// The `count` modifier the fixture reuses (identity-only; never dispatched).
    /// `PlacementModifierType.COUNT` is insertion index 5 in
    /// `PlacementModifierType.java`'s registration order.
    fn count_modifier() -> Arc<dyn ErasedPlacementModifier> {
        Arc::new(IdentityModifier(PlacementModifierTypeId::new(
            5,
            "minecraft:count",
        )))
    }

    use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId;
    use crate::levelgen::placement::{PlacementContext, PlacementModifier};

    #[derive(Debug)]
    struct IdentityModifier(PlacementModifierTypeId);

    impl PlacementModifier for IdentityModifier {
        fn get_positions<R: RandomSource>(
            &self,
            _context: &PlacementContext,
            _random: &mut R,
            _origin: &BlockPos,
        ) -> Vec<BlockPos> {
            Vec::new()
        }

        fn type_id(&self) -> PlacementModifierTypeId {
            self.0.clone()
        }
    }

    #[test]
    fn display_is_java_to_string() {
        // Java `toString()` — `"Placed " + this.feature`; the `Holder`
        // `Display` renders `Direct{<value>}` like Java's `Holder.toString()`.
        // The value part of a `Direct` holder renders the erased config via the
        // `dyn FeatureConfiguration` Debug, whose shape is not Java's value
        // string, so the stable `"Placed Direct{"` prefix is what carries the
        // `toString` shape.
        let placed = PlacedFeature::new(Holder::direct(no_op_feature()), vec![count_modifier()]);
        let s = placed.to_string();
        assert!(s.starts_with("Placed Direct{"), "got: {s}");
    }

    #[test]
    fn get_features_is_feature_plus_sub_features() {
        // `Stream.concat(Stream.of(this.feature), this.feature.value().getSubFeatures())`.
        let sub = no_op_feature();
        let top = ConfiguredFeatureErased {
            feature: FeatureId::new(1),
            config: Arc::new(SubFeatureConfig(sub.clone())),
        };
        let placed = PlacedFeature::new(
            Holder::direct(top),
            vec![count_modifier(), count_modifier()],
        );
        let features: Vec<_> = placed.get_features().collect();
        // The holder carries the inline top feature; its config reports one
        // sub-feature.
        assert_eq!(features.len(), 2);
        let top_holder = &features[0];
        let sub_holder = &features[1];
        if let Holder::Direct(f) = top_holder {
            assert_eq!(f.feature, FeatureId::new(1));
        } else {
            panic!("top feature holder must be Direct");
        }
        if let Holder::Direct(f) = sub_holder {
            assert_eq!(f.feature, FeatureId::new(0));
        } else {
            panic!("sub feature holder must be Direct");
        }
    }

    #[test]
    fn record_accessors_expose_the_fields() {
        // The record accessors expose the constructor fields; `placement()`
        // keeps the ordered modifier list.
        let feature = no_op_feature();
        let a = PlacedFeature::new(Holder::direct(feature.clone()), vec![count_modifier()]);
        let b = PlacedFeature::new(Holder::direct(feature), vec![count_modifier()]);
        if let Holder::Direct(f) = a.feature() {
            assert_eq!(f.feature, FeatureId::new(0));
        } else {
            panic!("feature holder must be Direct");
        }
        assert_eq!(a.placement().len(), 1);
        assert_eq!(
            a.placement()[0].type_id(),
            PlacementModifierTypeId::new(5, "minecraft:count")
        );
        // `Debug` on the record: `PlacedFeature` derives it from the fields.
        let debug_a = format!("{:?}", a);
        let debug_b = format!("{:?}", b);
        assert_eq!(debug_a, debug_b);
    }

    /// A minimal `WorldGenLevel`/`ChunkGenerator` double over the overworld
    /// window, used by the `place`-reachability tests.
    struct TestLevel;
    struct NoopGenerator;

    impl crate::level::height_accessor::LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl crate::level::WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }
    }

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }
        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// The panic payload, as `&str` if it was a format-string `panic!`.
    fn panic_message(result: std::thread::Result<bool>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| format!("{:?}", payload)),
        }
    }

    #[test]
    fn place_resolves_the_feature_before_walking_the_modifiers() {
        // Java's `placeWithContext` resolves `this.feature.value()` before the
        // `forEach` walks the pipeline. With a `Reference` holder the port's
        // `resolved_feature` panics (broader than Java's unbound-only throw;
        // deferred with #126), and it must panic before the #181 modifier
        // dispatch runs — pinning Java's resolution-before-walk ordering.
        let placed = PlacedFeature::new(
            Holder::Reference {
                registry: RegistryId(0),
                id: 1,
            },
            vec![count_modifier()],
        );
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            placed.place(&mut level, &generator, &mut random, &BlockPos::new(0, 0, 0))
        }));
        let msg = panic_message(result);
        assert!(
            msg.contains("configured feature"),
            "expected the holder-resolution panic, got: {msg}"
        );
    }

    #[test]
    fn place_reaches_the_modifier_dispatch_stub() {
        // The walk expands the origin through the first modifier and stops at
        // the #181 dispatch STUB, which panics (as documented there). This pins
        // that the pipeline is wired end-to-end (`place` -> `place_with_context`
        // -> `place_walk` -> `placement_get_positions`); the depth-first
        // interleaving the walk performs is not observable until #181 wires
        // both dispatch points (feature placement panics too).
        let placed = PlacedFeature::new(Holder::direct(no_op_feature()), vec![count_modifier()]);
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            placed.place(&mut level, &generator, &mut random, &BlockPos::new(0, 0, 0))
        }));
        let msg = panic_message(result);
        assert!(
            msg.contains("placement modifier"),
            "expected the #181 dispatch panic, got: {msg}"
        );
    }

    #[test]
    fn resolved_feature_panics_on_registry_reference() {
        // A `Reference` holder needs the configured-feature registry lookup
        // (deferred with #126); `resolved_feature` panics on every reference.
        // The panic is broader than Java's `Holder.Reference.value()`, which
        // throws ("Trying to access unbound value") only for a genuinely
        // unbound reference and resolves a bound one; with no lookup threaded
        // through the port resolves neither, and the message is non-Java.
        let placed = PlacedFeature::new(
            Holder::Reference {
                registry: RegistryId(0),
                id: 1,
            },
            vec![],
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // `get_features` builds the concat chain eagerly, resolving the
            // holder immediately, so constructing it panics on a Reference
            // holder; collect is not reached.
            let _: Vec<_> = placed.get_features().collect();
        }));
        assert!(result.is_err());
    }
}
