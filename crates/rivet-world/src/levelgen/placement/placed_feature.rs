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
//! `feature.value()` resolves through the threaded
//! `&dyn HolderLookup<ConfiguredFeatureErased>` (the back-reference rule from
//! `holder.rs`: `Holder::value` takes the lookup, since the Rust `Reference` is
//! a pure `(RegistryId, id)` pair). `Direct` yields the inline value; a
//! `Reference` resolves by id through the owning lookup, panicking with Java's
//! "Trying to access unbound value ..." message only when the id genuinely
//! cannot resolve (Java `Reference.value()` throws for an unbound reference and
//! resolves a bound one). Every value-resolving surface threads the lookup:
//! `place`/`place_with_biome_check`/`get_features` pass it to `resolved_feature`.
//!
//! `SharedConstants.DEBUG_FEATURE_COUNT` + `FeatureCountTracker.featurePlaced`
//! (keyed on `ServerLevel`, `rivet-server`) defer; the tracker is STUB'd in
//! `feature.core`.
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
use rivet_registry::holder_lookup::HolderLookup;
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
    ///
    /// `lookup` is the configured-feature `HolderLookup` the feature holder
    /// resolves through (Java's holder stores its value; the Rust `Reference`
    /// resolves by id — the back-reference rule).
    pub fn place<R: RandomSource>(
        &self,
        lookup: &dyn HolderLookup<ConfiguredFeatureErased>,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        self.place_with_context(lookup, level, generator, random, origin, None)
    }

    /// `PlacedFeature.placeWithBiomeCheck(...)` — same with the top feature set
    /// to `this` (used by the biome decoration pass).
    pub fn place_with_biome_check<R: RandomSource>(
        &self,
        lookup: &dyn HolderLookup<ConfiguredFeatureErased>,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        self.place_with_context(lookup, level, generator, random, origin, Some(self))
    }

    /// Walk the complete placement chain without invoking the configured
    /// feature. This is the selection half of `placeWithContext`: modifiers
    /// still run lazily, depth-first, with the same top-feature and world-state
    /// context, while the caller can stop at the first selected feature whose
    /// leaf implementation is outside its current slice.
    pub fn has_placement_positions<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        let mut selected = false;
        self.select_walk(0, *origin, level, generator, random, &mut selected);
        selected
    }

    fn select_walk<R: RandomSource>(
        &self,
        index: usize,
        pos: BlockPos,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        selected: &mut bool,
    ) {
        if *selected {
            return;
        }
        if index == self.placement.len() {
            *selected = true;
            return;
        }
        let modifier = &self.placement[index];
        let positions = {
            let context = PlacementContext::new(level, generator, Some(self));
            placement_get_positions(modifier.as_ref(), &context, random, &pos)
        };
        for child in positions {
            self.select_walk(index + 1, child, level, generator, random, selected);
            if *selected {
                break;
            }
        }
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
        lookup: &dyn HolderLookup<ConfiguredFeatureErased>,
        level: &mut dyn WorldGenLevel,
        generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
        top_feature: Option<&PlacedFeature>,
    ) -> bool {
        // `this.feature.value()` is resolved once, before the `forEach`
        // (Java: `ConfiguredFeature<?, ?> feature = this.feature.value();`).
        let feature = self.resolved_feature(lookup);
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
    /// so the port draws eagerly too (returning a lazy `Box<dyn Iterator>`) and
    /// reproduces the interleaving with this walk: expand a position through the
    /// current modifier, recurse into the next, and only at the last stage place
    /// the feature — pulling one position through the whole chain before the
    /// next.
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
    ///
    /// `lookup` is the configured-feature `HolderLookup` the feature holder
    /// resolves through (`this.feature.value()`), as in `place`; the returned
    /// iterator borrows both `self` and `lookup` (the resolved feature's
    /// sub-features), so both share one lifetime.
    pub fn get_features<'a>(
        &'a self,
        lookup: &'a dyn HolderLookup<ConfiguredFeatureErased>,
    ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + 'a> {
        Box::new(
            std::iter::once(self.feature.clone())
                .chain(self.resolved_feature(lookup).get_sub_features()),
        )
    }

    /// `this.feature.value()` — the holder's configured feature, resolved
    /// through the owning configured-feature `HolderLookup`.
    ///
    /// `Holder::value(lookup)` is the back-reference-rule resolution (OWNERSHIP
    /// §Registries): a `Direct` holder yields its inline value; a `Reference`
    /// resolves by id through the lookup, panicking with Java's
    /// "Trying to access unbound value '<key>' from registry <id>" message shape
    /// only when the id genuinely cannot resolve — Java's `Reference.value()`
    /// throws only for an unbound reference and resolves a bound one. The
    /// message is shape-faithful, not byte-identical: the pure-ID `Reference`
    /// stores no key, so an unresolvable id renders the key as "null" and the
    /// registry as its numeric id, where Java interpolates the reference's key
    /// and owner strings (see `render_holder` in `holder.rs`).
    fn resolved_feature<'a>(
        &'a self,
        lookup: &'a dyn HolderLookup<ConfiguredFeatureErased>,
    ) -> &'a ConfiguredFeatureErased {
        self.feature.value(lookup)
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
    use rivet_registry::RegistryBuilder;

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

    /// The `count` modifier the fixture reuses — a real `CountPlacement`
    /// (`PlacementModifierType.COUNT`, insertion index 5 in
    /// `PlacementModifierType.java`'s registration order), so any pipeline path
    /// that dispatches it reaches the wired `#181` leaf.
    fn count_modifier() -> Arc<dyn ErasedPlacementModifier> {
        Arc::new(CountPlacement::of_value(1))
    }

    use crate::levelgen::feature::configurations::RandomBooleanFeatureConfiguration;
    use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
    use crate::levelgen::placement::{
        CountPlacement, InSquarePlacement, PlacementModifierTypeId, RarityFilter,
    };
    use rivet_registry::Identifier;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::root::AnyBox;
    use rivet_util::random::{LegacyPositionalRandomFactory, LegacyRandomSource};

    /// The `worldgen/configured_feature` registry key — the configured-feature
    /// registry a `Holder::Reference` resolves through.
    fn configured_feature_registry_key()
    -> rivet_registry::ResourceKey<rivet_registry::Registry<ConfiguredFeatureErased>> {
        rivet_registry::ResourceKey::create_registry_key(
            rivet_registry::Identifier::with_default_namespace("worldgen/configured_feature"),
        )
    }

    /// A frozen configured-feature registry holding `values` — the test double
    /// for the `HolderLookup` `resolved_feature` resolves references through.
    /// `Registry` implements `HolderLookup<ConfiguredFeatureErased>`, so the
    /// returned registry is used directly as the lookup.
    fn configured_feature_lookup(
        values: Vec<ConfiguredFeatureErased>,
    ) -> rivet_registry::Registry<ConfiguredFeatureErased> {
        let mut builder = RegistryBuilder::new(&configured_feature_registry_key());
        for (i, value) in values.into_iter().enumerate() {
            builder.register(
                &rivet_registry::ResourceKey::create(
                    &configured_feature_registry_key(),
                    rivet_registry::Identifier::with_default_namespace(&format!("feature_{i}")),
                ),
                Arc::new(value),
                rivet_registry::RegistrationInfo::BUILT_IN,
            );
        }
        builder.freeze()
    }

    /// A config whose `getSubFeatures` yields a registry `Reference` — nesting
    /// the holder-resolution seam inside the sub-feature stream (the yielded
    /// holder is itself unresolved until `.value(lookup)`).
    #[derive(Debug)]
    struct NestedSubFeatureConfig(rivet_registry::RegistryId, u32);

    impl FeatureConfiguration for NestedSubFeatureConfig {
        fn get_sub_features(
            &self,
        ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
            Box::new(std::iter::once(Holder::reference(self.0, self.1)))
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
        // A Direct holder never resolves through the lookup, but the lookup is
        // still threaded through `get_features` (Java's `this.feature.value()`).
        let lookup = configured_feature_lookup(Vec::new());
        let features: Vec<_> = placed.get_features(&lookup).collect();
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
    fn get_features_resolves_a_reference_holder_through_the_lookup() {
        // The threaded-lookup seam: `this.feature.value()` resolves the
        // `Reference` by id through the owning lookup to reach the value whose
        // sub-features are concatenated. `getFeatures` is
        // `Stream.concat(Stream.of(this.feature), value.getSubFeatures())` — the
        // first element is the ORIGINAL holder (still a `Reference`), only the
        // sub-features are materialized fresh.
        let sub = no_op_feature();
        let top = ConfiguredFeatureErased {
            feature: FeatureId::new(1),
            config: Arc::new(SubFeatureConfig(sub.clone())),
        };
        let lookup = configured_feature_lookup(vec![top]);
        let registry_id = lookup.registry_id();
        // The reference points at the registry's sole element (id 0) — the same
        // holder shape `RegistryLookup.get(key)` constructs.
        let placed = PlacedFeature::new(Holder::reference(registry_id, 0), Vec::new());
        let features: Vec<_> = placed.get_features(&lookup).collect();
        assert_eq!(features.len(), 2);
        // Element 0 is the original holder — a `Reference` to id 0.
        match &features[0] {
            Holder::Reference { registry: r, id } => {
                assert_eq!(*r, registry_id);
                assert_eq!(*id, 0);
            }
            Holder::Direct(_) => panic!("top feature holder must stay a Reference"),
        }
        // Element 1 is the resolved top feature's sub-feature.
        if let Holder::Direct(f) = &features[1] {
            assert_eq!(f.feature, FeatureId::new(0));
        } else {
            panic!("sub feature holder must be Direct");
        }
    }

    #[test]
    fn get_features_panics_on_missing_key_with_java_message() {
        // A `Reference` whose id the lookup cannot resolve is Java's unbound
        // reference: `Holder::value` panics with Java's
        // "Trying to access unbound value ..." message shape (`Reference.value()`),
        // the key rendering as "null" — the pure-ID `Reference` stores no key,
        // so an unresolvable id cannot recover it (see `render_holder`).
        let lookup = configured_feature_lookup(Vec::new());
        let registry_id = lookup.registry_id();
        // id 42 is out of range — the lookup cannot resolve it.
        let placed = PlacedFeature::new(Holder::reference(registry_id, 42), Vec::new());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Vec<_> = placed.get_features(&lookup).collect();
        }));
        let msg = panic_message(result);
        assert_eq!(
            msg,
            format!(
                "Trying to access unbound value 'null' from registry {}",
                registry_id.0
            ),
        );
    }

    #[test]
    fn nested_sub_feature_reference_resolves_through_the_lookup() {
        // `this.feature.value().getSubFeatures()` may yield a *Reference* sub
        // feature holder; it is resolved against the same threaded lookup. The
        // top feature (id 0) is registered with a config whose `getSubFeatures`
        // yields a `Reference` to id 1 (a nested sub-feature). `RegistryId` is
        // assigned at builder construction, so the config can reference it
        // before freeze.
        let mut builder = RegistryBuilder::new(&configured_feature_registry_key());
        let registry_id = builder.registry_id();
        builder.register(
            &rivet_registry::ResourceKey::create(
                &configured_feature_registry_key(),
                rivet_registry::Identifier::with_default_namespace("top"),
            ),
            Arc::new(ConfiguredFeatureErased {
                feature: FeatureId::new(2),
                config: Arc::new(NestedSubFeatureConfig(registry_id, 1)),
            }),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &rivet_registry::ResourceKey::create(
                &configured_feature_registry_key(),
                rivet_registry::Identifier::with_default_namespace("nested"),
            ),
            Arc::new(no_op_feature()),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        let lookup = builder.freeze();

        // `this.feature.value()` resolves the top feature; the concatenated
        // `getSubFeatures()` yields the nested `Reference` holder. Java returns
        // holders (resolved lazily downstream), so the yielded holder is still a
        // `Reference` — resolved here through the same lookup.
        let placed = PlacedFeature::new(Holder::reference(registry_id, 0), Vec::new());
        let features: Vec<_> = placed.get_features(&lookup).collect();
        assert_eq!(features.len(), 2);
        match &features[1] {
            Holder::Reference { registry: r, id } => {
                assert_eq!(*r, registry_id);
                assert_eq!(*id, 1);
                // The nested holder resolves to the registry's second element.
                assert_eq!(features[1].value(&lookup).feature, FeatureId::new(0),);
            }
            Holder::Direct(_) => panic!("nested sub feature holder must be a Reference"),
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

    /// The two-registry access the selector feature resolves its placed/
    /// configured-feature holders through — frozen empty configured/placed
    /// registries (the composed chain's placed branches are inline `Direct`
    /// holders, so nothing resolves by id). The same access shape
    /// `feature::test_support::access` builds (that module is `#[cfg(test)]`-
    /// private to the `feature` unit).
    fn two_registry_access() -> RegistryAccess {
        let configured = RegistryBuilder::new(&*CONFIGURED_FEATURE).freeze();
        let placed = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
        RegistryAccess::from_pairs(vec![
            (
                rivet_registry::ResourceKey::create_registry_key(
                    Identifier::with_default_namespace("worldgen/configured_feature"),
                ),
                Box::new(configured) as AnyBox,
            ),
            (
                rivet_registry::ResourceKey::create_registry_key(
                    Identifier::with_default_namespace("worldgen/placed_feature"),
                ),
                Box::new(placed) as AnyBox,
            ),
        ])
    }

    /// An inline placed feature wrapping the `minecraft:no_op` configured leaf
    /// (id 0) — returns `true` without drawing, so the selector's own
    /// `nextBoolean` is the only RNG the branch contributes.
    fn no_op_placed() -> Holder<PlacedFeature> {
        Holder::direct(PlacedFeature::new(
            Holder::direct(no_op_feature()),
            Vec::new(),
        ))
    }

    /// The placed feature the composed-chain test drives: a
    /// `minecraft:random_boolean_selector` configured feature (id 55, both
    /// branches the no-op placed leaf) scattered by
    /// `[minecraft:count, minecraft:in_square]`.
    fn chain_placed(count: i32) -> PlacedFeature {
        let selector = ConfiguredFeatureErased {
            feature: FeatureId::new(55),
            config: Arc::new(RandomBooleanFeatureConfiguration::new(
                no_op_placed(),
                no_op_placed(),
            )),
        };
        PlacedFeature::new(
            Holder::direct(selector),
            vec![
                Arc::new(CountPlacement::of_value(count)) as Arc<dyn ErasedPlacementModifier>,
                Arc::new(InSquarePlacement::spread()),
            ],
        )
    }

    /// The RNG draws the composed-chain tests pin: `InSquarePlacement`'s two
    /// `nextInt(16)` and the selector's `nextBoolean`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ChainCall {
        IntBound(i32),
        Boolean,
    }

    /// A `RandomSource` wrapper recording the chain's draws in order — pins the
    /// exact depth-first interleaving (per-position in_square draws then the
    /// selector's boolean) rather than an eager two-phase expansion.
    struct ChainRandom {
        inner: LegacyRandomSource,
        calls: Vec<ChainCall>,
    }

    impl ChainRandom {
        fn new(seed: i64) -> ChainRandom {
            ChainRandom {
                inner: LegacyRandomSource::new(seed),
                calls: Vec::new(),
            }
        }
    }

    impl rivet_util::RandomSource for ChainRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            ChainRandom {
                inner: self.inner.fork(),
                calls: self.calls.clone(),
            }
        }

        fn fork_positional(&mut self) -> Self::Positional {
            self.inner.fork_positional()
        }

        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }

        fn next_int(&mut self) -> i32 {
            self.inner.next_int()
        }

        fn next_int_bound(&mut self, bound: i32) -> i32 {
            self.calls.push(ChainCall::IntBound(bound));
            self.inner.next_int_bound(bound)
        }

        fn next_long(&mut self) -> i64 {
            self.inner.next_long()
        }

        fn next_boolean(&mut self) -> bool {
            self.calls.push(ChainCall::Boolean);
            self.inner.next_boolean()
        }

        fn next_float(&mut self) -> f32 {
            self.inner.next_float()
        }

        fn next_double(&mut self) -> f64 {
            self.inner.next_double()
        }

        fn next_gaussian(&mut self) -> f64 {
            self.inner.next_gaussian()
        }
    }

    #[test]
    fn composed_chain_interleaves_draws_per_position() {
        // The depth-first walk behind `placeWithContext` interleaves the RNG
        // draws of successive chain stages per upstream position, exactly like
        // Java's lazy `flatMap` + `forEach`:
        //
        //   [count(2), in_square] -> random_boolean_selector (id 55)
        //
        // For each of the two count positions the walk draws the in_square
        // `nextInt(16)` pair, then places — the selector draws its
        // `nextBoolean` and (resolving its inline Direct no-op branches) returns
        // true. The recorded order is therefore
        //
        //   IntBound(16), IntBound(16), Boolean,   // position 1
        //   IntBound(16), IntBound(16), Boolean,   // position 2
        //
        // An eager two-phase expansion (all in_square offsets first, then all
        // placements) would record IntBound x4 then Boolean x2 — this pins the
        // per-position interleaving and the absence of eager collection drift in
        // one exact sequence.
        let placed = chain_placed(2);
        let lookup = configured_feature_lookup(Vec::new());
        let mut level = AccessLevel(two_registry_access());
        let generator = NoopGenerator;
        let mut random = ChainRandom::new(42);
        let placed_any = placed.place(
            &lookup,
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        );
        assert!(
            placed_any,
            "the no-op selector branches should have been placed"
        );
        assert_eq!(
            random.calls,
            vec![
                ChainCall::IntBound(16),
                ChainCall::IntBound(16),
                ChainCall::Boolean,
                ChainCall::IntBound(16),
                ChainCall::IntBound(16),
                ChainCall::Boolean,
            ]
        );
    }

    #[test]
    fn composed_chain_multiplicity_matches_the_count() {
        // `count(3)` multiplies the whole downstream chain: three positions,
        // each pulling its own in_square pair then the selector's boolean —
        // Java's `IntStream.range(0, count)` flatMap, not a single shared draw
        // phase. Pins the count multiplicity end-to-end through the full walk.
        let placed = chain_placed(3);
        let lookup = configured_feature_lookup(Vec::new());
        let mut level = AccessLevel(two_registry_access());
        let generator = NoopGenerator;
        let mut random = ChainRandom::new(0);
        let placed_any = placed.place(
            &lookup,
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        );
        assert!(placed_any);
        assert_eq!(random.calls.len(), 9);
        assert_eq!(
            random
                .calls
                .iter()
                .filter(|c| **c == ChainCall::Boolean)
                .count(),
            3
        );
        assert!(
            random.calls.chunks_exact(3).all(|triple| triple
                == [
                    ChainCall::IntBound(16),
                    ChainCall::IntBound(16),
                    ChainCall::Boolean
                ]),
            "each position must draw its in_square pair before its boolean, got {:?}",
            random.calls
        );
    }

    /// A `WorldGenLevel` double carrying the two-registry `RegistryAccess` the
    /// selector feature resolves its placed/configured holders through — the
    /// `registry_access` seam is the only world surface the composed chain
    /// touches (`get_block_state` is never reached: the no-op leaves return
    /// without reading).
    struct AccessLevel(RegistryAccess);

    impl crate::level::height_accessor::LevelHeightAccessor for AccessLevel {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl crate::level::WorldGenLevel for AccessLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(
            &self,
            _pos: &rivet_registry::core::BlockPos,
        ) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): no real world-access implementation is present —
            // the state-testing predicates surface the unavailable capability
            // explicitly (see `StateTestingPredicate::test`).
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }

        fn registry_access(&self) -> RegistryAccess {
            self.0.clone()
        }
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

        fn get_block_state(
            &self,
            _pos: &rivet_registry::core::BlockPos,
        ) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): no real world-access implementation is present —
            // the state-testing predicates surface the unavailable capability
            // explicitly (see `StateTestingPredicate::test`).
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
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

    /// The panic payload, as `&str` if it was a format-string `panic!`. A
    /// `panic!` with a bare literal yields a `&'static str` payload; one with
    /// format arguments yields a `String` — both are recovered.
    fn panic_message<T>(result: std::thread::Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&'static str>()
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| format!("{:?}", payload)),
        }
    }

    #[test]
    fn place_resolves_the_feature_before_walking_the_modifiers() {
        // Java's `placeWithContext` resolves `this.feature.value()` before the
        // `forEach` walks the pipeline. With an unresolvable `Reference` holder
        // the resolution panics with Java's "Trying to access unbound value"
        // message, and it must panic before the #181 modifier dispatch runs —
        // pinning Java's resolution-before-walk ordering.
        let lookup = configured_feature_lookup(Vec::new());
        let registry_id = lookup.registry_id();
        // id 0 is out of range — the empty lookup cannot resolve it.
        let placed = PlacedFeature::new(Holder::reference(registry_id, 0), vec![count_modifier()]);
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            placed.place(
                &lookup,
                &mut level,
                &generator,
                &mut random,
                &BlockPos::new(0, 0, 0),
            )
        }));
        let msg = panic_message(result);
        assert!(
            msg.contains("Trying to access unbound value"),
            "expected the holder-resolution panic, got: {msg}"
        );
    }

    #[test]
    fn selection_walk_reaches_later_modifier_acceptance() {
        let placed = PlacedFeature::new(
            Holder::direct(no_op_feature()),
            vec![
                count_modifier(),
                Arc::new(RarityFilter::on_average_once_every(1)),
            ],
        );
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        assert!(placed.has_placement_positions(
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        ));
    }

    #[test]
    fn selection_walk_rejects_at_a_later_modifier() {
        let placed = PlacedFeature::new(
            Holder::direct(no_op_feature()),
            vec![
                count_modifier(),
                Arc::new(RarityFilter::on_average_once_every(2_147_483_647)),
            ],
        );
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert!(!placed.has_placement_positions(
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        ));
    }

    #[test]
    fn place_walks_the_count_modifier_to_a_placed_feature() {
        // The full pipeline is wired end-to-end: `place` -> `place_with_context`
        // -> `place_walk` -> `placement_get_positions` (dispatching the real
        // `CountPlacement` leaf, id 5) -> `feature.place` (the no-op feature
        // id 0, which returns true without touching the level).
        // `CountPlacement::of_value(1)` yields the origin once, so the walk
        // places exactly one position and reports `true`. A `Direct` holder
        // resolves without touching the lookup, so an empty lookup suffices.
        let placed = PlacedFeature::new(Holder::direct(no_op_feature()), vec![count_modifier()]);
        let lookup = configured_feature_lookup(Vec::new());
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        let placed_any = placed.place(
            &lookup,
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        );
        assert!(placed_any, "the no-op feature should have been placed");
    }

    #[test]
    fn place_with_biome_check_resolves_a_bound_reference() {
        // `placeWithBiomeCheck` threads the same lookup: a `Reference` to a
        // bound id resolves through the lookup, then the walk dispatches the
        // `CountPlacement` leaf and places the resolved no-op feature (id 0).
        let lookup = configured_feature_lookup(vec![no_op_feature()]);
        let registry_id = lookup.registry_id();
        let placed = PlacedFeature::new(Holder::reference(registry_id, 0), vec![count_modifier()]);
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut random = rivet_util::random::LegacyRandomSource::new(42);
        let placed_any = placed.place_with_biome_check(
            &lookup,
            &mut level,
            &generator,
            &mut random,
            &BlockPos::new(0, 0, 0),
        );
        assert!(
            placed_any,
            "the resolved reference's no-op feature should have been placed"
        );
    }
}
