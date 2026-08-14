//! `net.minecraft.world.level.biome.BiomeGenerationSettings` — the biome's
//! carvers and features (issue #178, `mc.world.level.biome.core` unit).
//!
//! Faithful port of the 26.2 `BiomeGenerationSettings.java` value surface: the
//! `carvers`/`features` fields, the `CODEC`, the `Builder`/`PlainBuilder`,
//! `EMPTY`, and the memoized `boneMealFeatures` accessor
//! (`getBoneMealFeatures`). The memo is a pure function of the `features`
//! field, so Java's `Suppliers.memoize` is elided (a recompute is behaviorally
//! identical); the holder-resolution lookups thread in as parameters
//! (`feature.value()` needs the placed-feature `HolderLookup`, and
//! `PlacedFeature.getFeatures`/`Holder.is` the configured-feature one).
//! `hasFeature`'s `featureSet` memo defers — `PlacedFeature` has no value
//! equality (see [`BiomeGenerationSettings::has_feature`]).
//!
//! ## Codec notes
//!
//! - `carvers` — `ConfiguredWorldCarver.LIST_CODEC` is a `HolderSetCodec` over
//!   a new `CONFIGURED_CARVER` registry key (element = `RegistryFileCodec`),
//!   which resolves carvers by name. The element `DIRECT_CODEC` (an inline
//!   carver definition) defers with the `#126` dispatch-codec surface, so the
//!   element codec is a STUB: named references round-trip, inline definitions
//!   error honestly.
//! - `features` — `PlacedFeature.LIST_OF_LISTS_CODEC` is
//!   `RegistryCodecs.homogeneousList(PLACED_FEATURE, DIRECT_CODEC, true).listOf()`:
//!   a `RegistryFileCodec` element (named placed features resolve through the
//!   `PLACED_FEATURE` registry) wrapped in a `HolderSetCodec(alwaysUseList =
//!   true)` and a `listOf()`. The inline `DIRECT_CODEC` defers with the `#126`
//!   surface, so an inline feature definition errors honestly while a named
//!   reference round-trips through the registry.
//! - Both fields run `promotePartial(Util.prefix("Carver: "/"Features: ",
//!   logger))`. Java's `promotePartial` turns an error-with-partial into a
//!   success with the partial value (the error is logged), so a non-empty
//!   carver/feature list whose elements fail to resolve decodes to the empty
//!   list rather than failing the whole biome — the `#126` inline-definition
//!   errors are swallowed here exactly as unresolvable references would be in
//!   Java. The logger callback is a no-op (Rivet has no slf4j surface; only
//!   the promote-partial semantics matter).

use crate::levelgen::carver::ConfiguredWorldCarverErased;
use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::vegetation_patch_configuration::placed_feature_direct_codec;
use crate::levelgen::generation_step::Decoration;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_lookup::HolderLookup;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFileCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::{Identifier, Registry, ResourceKey, TagKey};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::string_representable::EnumOrdinal;
use std::sync::Arc;
use std::sync::LazyLock;

/// `Registries.CONFIGURED_CARVER` — `createRegistryKey("worldgen/configured_carver")`,
/// the typed registry key over the erased `ConfiguredWorldCarver<?>` value.
pub static CONFIGURED_CARVER: LazyLock<ResourceKey<Registry<ConfiguredWorldCarverErased>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/configured_carver",
        ))
    });

/// `Registries.PLACED_FEATURE` — `createRegistryKey("worldgen/placed_feature")`,
/// the typed registry key over the `PlacedFeature` value.
pub static PLACED_FEATURE: LazyLock<ResourceKey<Registry<PlacedFeature>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace(
        "worldgen/placed_feature",
    ))
});

/// `Registries.CONFIGURED_FEATURE` — `createRegistryKey("worldgen/configured_feature")`,
/// the typed registry key over the erased `ConfiguredFeature<?, ?>` value. The
/// `FeatureTags.CAN_SPAWN_FROM_BONE_MEAL` tag and `PlacedFeature.getFeatures`
/// resolution operate over it.
pub static CONFIGURED_FEATURE: LazyLock<ResourceKey<Registry<ConfiguredFeatureErased>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/configured_feature",
        ))
    });

/// `FeatureTags.CAN_SPAWN_FROM_BONE_MEAL` — `TagKey.create(
/// Registries.CONFIGURED_FEATURE, Identifier.withDefaultNamespace(
/// "can_spawn_from_bone_meal"))`, the named set `getBoneMealFeatures` filters
/// its configured features against.
pub static CAN_SPAWN_FROM_BONE_MEAL: LazyLock<TagKey<ConfiguredFeatureErased>> =
    LazyLock::new(|| {
        TagKey::create(
            &*CONFIGURED_FEATURE,
            Identifier::with_default_namespace("can_spawn_from_bone_meal"),
        )
    });

/// `net.minecraft.world.level.biome.BiomeGenerationSettings`.
#[derive(Debug, Clone)]
pub struct BiomeGenerationSettings {
    /// `this.carvers` — `HolderSet<ConfiguredWorldCarver<?>>`.
    carvers: HolderSet<ConfiguredWorldCarverErased>,
    /// `this.features` — `List<HolderSet<PlacedFeature>>`, one per
    /// `GenerationStep.Decoration` ordinal.
    features: Vec<HolderSet<PlacedFeature>>,
}

impl BiomeGenerationSettings {
    /// `new BiomeGenerationSettings(HolderSet<ConfiguredWorldCarver<?>>,
    /// List<HolderSet<PlacedFeature>>)` — the private constructor.
    pub fn new(
        carvers: HolderSet<ConfiguredWorldCarverErased>,
        features: Vec<HolderSet<PlacedFeature>>,
    ) -> Self {
        BiomeGenerationSettings { carvers, features }
    }

    /// `BiomeGenerationSettings.EMPTY`.
    pub const EMPTY: BiomeGenerationSettings = BiomeGenerationSettings {
        carvers: HolderSet::Direct(Vec::new()),
        features: Vec::new(),
    };

    /// `BiomeGenerationSettings.CODEC` — the ops-generic `MapCodec`.
    pub fn map_codec_of<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<BiomeGenerationSettings, Ops>> {
        record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|g: &BiomeGenerationSettings| g.carvers.clone()),
                    codec::field_of(
                        codec::promote_partial(
                            configured_world_carver_list_codec::<Ops>(),
                            Arc::new(|_| {}),
                        ),
                        "carvers".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|g: &BiomeGenerationSettings| g.features.clone()),
                    codec::field_of(
                        codec::promote_partial(
                            placed_feature_list_of_lists_codec::<Ops>(),
                            Arc::new(|_| {}),
                        ),
                        "features".to_string(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |carvers: HolderSet<ConfiguredWorldCarverErased>,
                         features: Vec<HolderSet<PlacedFeature>>| {
                            BiomeGenerationSettings::new(carvers, features)
                        },
                    ),
                )
        })
    }

    /// `BiomeGenerationSettings.getCarvers()`.
    pub fn get_carvers(&self) -> &HolderSet<ConfiguredWorldCarverErased> {
        &self.carvers
    }

    /// `BiomeGenerationSettings.features()`.
    pub fn features(&self) -> &[HolderSet<PlacedFeature>] {
        &self.features
    }

    /// `BiomeGenerationSettings.getBoneMealFeatures()` — the memoized list of
    /// configured features that can spawn from bone meal.
    ///
    /// Java's memo folds
    /// `features.stream().flatMap(HolderSet::stream).flatMap(feature ->
    /// feature.value().getFeatures()).filter(feature ->
    /// feature.is(FeatureTags.CAN_SPAWN_FROM_BONE_MEAL)).map(Holder::value)`
    /// into a list. The memo is a pure function of `features`, so it is
    /// recomputed per call (behaviorally identical); the holder resolutions
    /// thread in as the lookups (`feature.value()` resolves the placed-feature
    /// holder through the placed-feature lookup, `getFeatures`/`is`/the final
    /// `Holder::value` through the configured-feature one).
    pub fn get_bone_meal_features(
        &self,
        placed_lookup: &dyn HolderLookup<PlacedFeature>,
        configured_lookup: &dyn HolderLookup<ConfiguredFeatureErased>,
    ) -> Vec<ConfiguredFeatureErased> {
        let mut result = Vec::new();
        for set in &self.features {
            for feature in set.stream() {
                for configured in feature.value(placed_lookup).get_features(configured_lookup) {
                    if configured.is_tag(configured_lookup, &CAN_SPAWN_FROM_BONE_MEAL) {
                        result.push(configured.value(configured_lookup).clone());
                    }
                }
            }
        }
        result
    }

    /// `BiomeGenerationSettings.hasFeature(PlacedFeature)`.
    ///
    /// Java: `featureSet.get().contains(feature)` — the memoized
    /// `Set<PlacedFeature>` of *resolved* values compared with `PlacedFeature`'s
    /// record `equals` (feature + placement). The Rust `PlacedFeature` carries
    /// `placement: Vec<Arc<dyn ErasedPlacementModifier>>`, which has no
    /// equality, and `ConfiguredFeatureErased` likewise (its config is an
    /// erased `Arc<dyn FeatureConfiguration>`), so the value-equality compare
    /// is genuinely unavailable — RivetTodo(#181): needs `PlacedFeature` value
    /// equality once the placement/feature dispatch lands. Rather than
    /// fabricate a partial compare, this fails explicitly; the `featureSet`
    /// memo defers with it. Its only caller — `BiomeFilter.shouldPlace`, via
    /// `ChunkGenerator.getBiomeGenerationSettings(...).hasFeature` — is already
    /// a panic seam (`get_biome_generation_settings_has_feature`, #178): the
    /// `Holder<BiomeId>` handle cannot resolve a `BiomeGenerationSettings`.
    pub fn has_feature(&self, _feature: &PlacedFeature) -> bool {
        panic!(
            "BiomeGenerationSettings.hasFeature(PlacedFeature) needs PlacedFeature value equality (RivetTodo #181)"
        )
    }
}

/// `ConfiguredWorldCarver.LIST_CODEC` — `RegistryCodecs.homogeneousList(
/// Registries.CONFIGURED_CARVER, DIRECT_CODEC)`, as the ops-generic factory.
/// Named carver references resolve through the registry; the inline `DIRECT_CODEC`
/// element is a STUB (`#126`).
fn configured_world_carver_list_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HolderSet<ConfiguredWorldCarverErased>, Ops>> {
    let direct_stub: Arc<dyn Codec<ConfiguredWorldCarverErased, Ops>> = codec::of(
        encoder::error("ConfiguredWorldCarver.DIRECT_CODEC is a STUB (RivetTodo #126)".to_string()),
        decoder::error("ConfiguredWorldCarver.DIRECT_CODEC is a STUB (RivetTodo #126)".to_string()),
        "ConfiguredWorldCarver.DIRECT_CODEC[STUB]".to_string(),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<ConfiguredWorldCarverErased>, Ops>> =
        Arc::new(RegistryFileCodec::create(&CONFIGURED_CARVER, direct_stub));
    Arc::new(HolderSetCodec::create(&CONFIGURED_CARVER, element, false))
}

/// `PlacedFeature.LIST_OF_LISTS_CODEC` — `RegistryCodecs.homogeneousList(
/// Registries.PLACED_FEATURE, DIRECT_CODEC, true).listOf()`, as the
/// ops-generic factory.
///
/// Each element is a `HolderSetCodec` with `alwaysUseList = true` (the
/// `"features"` field is a list of holder sets); its element is the
/// `RegistryFileCodec` over `PLACED_FEATURE`, which resolves a named placed
/// feature reference through the registry and defers to `PlacedFeature.
/// DIRECT_CODEC` (`#126`) for an inline definition, which errors honestly. The
/// field's `promotePartial` swallows those errors into the empty list (see the
/// module doc).
fn placed_feature_list_of_lists_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Vec<HolderSet<PlacedFeature>>, Ops>> {
    let element: Arc<dyn Codec<Holder<PlacedFeature>, Ops>> = Arc::new(RegistryFileCodec::create(
        &PLACED_FEATURE,
        placed_feature_direct_codec::<Ops>(),
    ));
    let holder_set = Arc::new(HolderSetCodec::create(&PLACED_FEATURE, element, true));
    codec::list(holder_set)
}

/// `BiomeGenerationSettings.PlainBuilder`.
#[derive(Debug, Clone, Default)]
pub struct PlainBuilder {
    /// `PlainBuilder.carvers` — `List<Holder<ConfiguredWorldCarver<?>>>`.
    carvers: Vec<Holder<ConfiguredWorldCarverErased>>,
    /// `PlainBuilder.features` — `List<List<Holder<PlacedFeature>>>`.
    features: Vec<Vec<Holder<PlacedFeature>>>,
}

impl PlainBuilder {
    /// `PlainBuilder.addFeature(GenerationStep.Decoration, Holder<PlacedFeature>)`.
    pub fn add_feature(self, step: Decoration, feature: Holder<PlacedFeature>) -> Self {
        self.add_feature_index(step.ordinal() as i32, feature)
    }

    /// `PlainBuilder.addFeature(int index, Holder<PlacedFeature>)`.
    pub fn add_feature_index(mut self, index: i32, feature: Holder<PlacedFeature>) -> Self {
        self.add_feature_steps_up_to(index);
        self.features[index as usize].push(feature);
        self
    }

    /// `PlainBuilder.addCarver(Holder<ConfiguredWorldCarver<?>>)`.
    pub fn add_carver(mut self, carver: Holder<ConfiguredWorldCarverErased>) -> Self {
        self.carvers.push(carver);
        self
    }

    /// `PlainBuilder.addFeatureStepsUpTo(int index)`.
    fn add_feature_steps_up_to(&mut self, index: i32) {
        while (self.features.len() as i32) <= index {
            self.features.push(Vec::new());
        }
    }

    /// `PlainBuilder.build()`.
    pub fn build(self) -> BiomeGenerationSettings {
        let features = self.features.into_iter().map(HolderSet::direct).collect();
        BiomeGenerationSettings::new(HolderSet::direct(self.carvers), features)
    }
}

/// `BiomeGenerationSettings.Builder` — `PlainBuilder` over a
/// `HolderGetter<PlacedFeature>`/`HolderGetter<ConfiguredWorldCarver<?>>`.
pub struct Builder {
    /// The underlying `PlainBuilder` (Java's `Builder extends PlainBuilder`).
    inner: PlainBuilder,
    /// `Builder.placedFeatures` — the `HolderGetter<PlacedFeature>`.
    placed_features: Arc<dyn HolderGetter<PlacedFeature>>,
    /// `Builder.worldCarvers` — the `HolderGetter<ConfiguredWorldCarver<?>>`.
    world_carvers: Arc<dyn HolderGetter<ConfiguredWorldCarverErased>>,
}

impl Builder {
    /// `new Builder(HolderGetter<PlacedFeature>, HolderGetter<ConfiguredWorldCarver<?>>)`.
    pub fn new(
        placed_features: Arc<dyn HolderGetter<PlacedFeature>>,
        world_carvers: Arc<dyn HolderGetter<ConfiguredWorldCarverErased>>,
    ) -> Self {
        Builder {
            inner: PlainBuilder::default(),
            placed_features,
            world_carvers,
        }
    }

    /// `Builder.addFeature(GenerationStep.Decoration, ResourceKey<PlacedFeature>)`.
    pub fn add_feature(mut self, step: Decoration, feature: &ResourceKey<PlacedFeature>) -> Self {
        let holder = self.placed_features.get_or_throw(feature);
        self.inner = std::mem::take(&mut self.inner).add_feature(step, holder);
        self
    }

    /// `Builder.addCarver(ResourceKey<ConfiguredWorldCarver<?>>)`.
    pub fn add_carver(mut self, carver: &ResourceKey<ConfiguredWorldCarverErased>) -> Self {
        let holder = self.world_carvers.get_or_throw(carver);
        self.inner = std::mem::take(&mut self.inner).add_carver(holder);
        self
    }

    /// `Builder.build()` — delegates to the underlying `PlainBuilder`.
    pub fn build(self) -> BiomeGenerationSettings {
        self.inner.build()
    }
}

impl std::fmt::Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::carver_configuration::{
        CarverConfiguration, CarverConfigurationBase,
    };
    use crate::levelgen::carver::carver_debug_settings::CarverDebugSettings;
    use crate::levelgen::carver::world_carver::WorldCarverId;
    use crate::levelgen::feature::FeatureId;
    use crate::levelgen::heightproviders::constant_height::ConstantHeight;
    use crate::levelgen::heightproviders::height_provider::HeightProvider;
    use crate::levelgen::placement::PlacedFeature;
    use crate::levelgen::vertical_anchor::VerticalAnchor;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder_set::HolderSet;
    use rivet_registry::registries::BlockType;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use rivet_util::valueproviders::constant_float::ConstantFloat;
    use rivet_util::valueproviders::float_provider::FloatProvider;
    use serde_json::json;
    use std::any::Any;
    use std::sync::Arc;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A minimal `CarverConfiguration` for the erased-holder round-trip tests
    /// (the config value is never read — only its erased `Arc<dyn>` identity).
    #[derive(Debug)]
    struct TestCarverConfig {
        base: CarverConfigurationBase,
    }

    impl TestCarverConfig {
        fn new() -> Self {
            TestCarverConfig {
                base: CarverConfigurationBase::new(
                    1.0,
                    HeightProvider::Constant(ConstantHeight::of(VerticalAnchor::absolute(0))),
                    FloatProvider::Constant(ConstantFloat::of(1.0)),
                    VerticalAnchor::absolute(0),
                    CarverDebugSettings::default(),
                    HolderSet::Direct(Vec::new()),
                ),
            }
        }
    }

    impl CarverConfiguration for TestCarverConfig {
        fn probability(&self) -> f32 {
            self.base.probability()
        }
        fn y(&self) -> &HeightProvider {
            self.base.y()
        }
        fn y_scale(&self) -> &FloatProvider {
            self.base.y_scale()
        }
        fn lava_level(&self) -> &VerticalAnchor {
            self.base.lava_level()
        }
        fn debug_settings(&self) -> &CarverDebugSettings {
            self.base.debug_settings()
        }
        fn replaceable(&self) -> &HolderSet<BlockType> {
            self.base.replaceable()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn carver_holder() -> Holder<ConfiguredWorldCarverErased> {
        let erased = ConfiguredWorldCarverErased::new(
            WorldCarverId::new(0, "minecraft:cave"),
            Arc::new(TestCarverConfig::new()),
        );
        Holder::direct(erased)
    }

    /// The `(RegistryId, id)` pairs of a `HolderSet`'s reference members
    /// (comparison surface for the erased/`Arc<dyn>`-field holders, which are
    /// not `PartialEq`).
    fn ref_ids<T>(set: &HolderSet<T>) -> Vec<(rivet_registry::holder::RegistryId, u32)> {
        set.iter()
            .map(|h| match h {
                Holder::Reference { registry, id } => (*registry, *id),
                Holder::Direct(_) => panic!("expected a reference holder"),
            })
            .collect()
    }

    #[test]
    fn empty_is_empty() {
        let empty = BiomeGenerationSettings::EMPTY;
        assert_eq!(empty.get_carvers().size(), 0);
        assert!(empty.features().is_empty());
    }

    #[test]
    fn plain_builder_grows_feature_steps_and_builds() {
        let cave = carver_holder();
        let mut builder = PlainBuilder::default();
        builder = builder.add_carver(cave);
        // A feature added at step 3 (UndergroundStructures) grows the list up
        // to index 3 (4 steps).
        let feature = Holder::<PlacedFeature>::reference(rivet_registry::holder::RegistryId(0), 7);
        builder = builder.add_feature(Decoration::UndergroundStructures, feature);
        let settings = builder.build();
        assert_eq!(settings.features().len(), 4);
        assert_eq!(
            ref_ids(&settings.features()[3]),
            vec![(rivet_registry::holder::RegistryId(0), 7)]
        );
        // The carvers holder-set carries the direct carver.
        match settings.get_carvers() {
            HolderSet::Direct(holders) => assert_eq!(holders.len(), 1),
            HolderSet::Named { .. } => panic!("carvers should be direct"),
        }
    }

    #[test]
    fn builder_resolves_keys_through_getters() {
        // A minimal HolderGetter over a single key.
        let feature_key = ResourceKey::create(
            &*PLACED_FEATURE,
            Identifier::parse("minecraft:test_feature"),
        );
        let carver_key = ResourceKey::create(
            &*CONFIGURED_CARVER,
            Identifier::parse("minecraft:test_carver"),
        );

        #[derive(Clone)]
        struct Getter {
            key: ResourceKey<PlacedFeature>,
        }
        impl HolderGetter<PlacedFeature> for Getter {
            fn get(&self, key: &ResourceKey<PlacedFeature>) -> Option<Holder<PlacedFeature>> {
                (key == &self.key)
                    .then(|| Holder::reference(rivet_registry::holder::RegistryId(0), 3))
            }
            fn get_tag(
                &self,
                _tag: &rivet_registry::TagKey<PlacedFeature>,
            ) -> Option<HolderSet<PlacedFeature>> {
                None
            }
        }
        #[derive(Clone)]
        struct CarverGetter {
            key: ResourceKey<ConfiguredWorldCarverErased>,
        }
        impl HolderGetter<ConfiguredWorldCarverErased> for CarverGetter {
            fn get(
                &self,
                key: &ResourceKey<ConfiguredWorldCarverErased>,
            ) -> Option<Holder<ConfiguredWorldCarverErased>> {
                (key == &self.key).then(carver_holder)
            }
            fn get_tag(
                &self,
                _tag: &rivet_registry::TagKey<ConfiguredWorldCarverErased>,
            ) -> Option<HolderSet<ConfiguredWorldCarverErased>> {
                None
            }
        }

        let builder = Builder::new(
            Arc::new(Getter {
                key: feature_key.clone(),
            }),
            Arc::new(CarverGetter {
                key: carver_key.clone(),
            }),
        );
        let settings = builder
            .add_feature(Decoration::Lakes, &feature_key)
            .add_carver(&carver_key)
            .build();
        assert_eq!(settings.features().len(), 2);
        assert_eq!(
            ref_ids(&settings.features()[1]),
            vec![(rivet_registry::holder::RegistryId(0), 3)]
        );
        assert_eq!(settings.get_carvers().size(), 1);
    }

    /// A `RegistryAccess` carrying empty CONFIGURED_CARVER and PLACED_FEATURE
    /// registries (enough for the codec's empty-list round-trip).
    fn empty_access() -> RegistryAccess {
        let carver_registry = RegistryBuilder::new(&*CONFIGURED_CARVER).freeze();
        let feature_registry = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_carver",
                )),
                Box::new(carver_registry) as rivet_registry::root::AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(feature_registry) as rivet_registry::root::AnyBox,
            ),
        ])
    }

    #[test]
    fn codec_round_trips_empty_settings() {
        let access = empty_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = map_codec::codec_of(BiomeGenerationSettings::map_codec_of::<TestOps>());
        let settings = BiomeGenerationSettings::EMPTY;
        let encoded = codec
            .encode_start(&ops, &settings)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!({"carvers": [], "features": []}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.get_carvers().size(), 0);
        assert!(decoded.features().is_empty());
    }

    #[test]
    fn codec_unresolvable_feature_name_drops_to_empty_holder_set() {
        // A features list of *unresolvable names*: the element `RegistryFileCodec`
        // errors "Failed to get element" (no partial), but the holder-set's inner
        // list codec attaches `([], errors)` as its partial, and the outer
        // `listOf()` similarly partials the failed holder set through. The field's
        // `promotePartial` (Java-faithful: an error-with-partial becomes a success
        // with the partial value) recovers `[Direct([])]` — the unresolvable name
        // drops out, leaving a one-step feature list of empty holder sets, exactly
        // as Java's `homogeneousList(...).listOf()` + `promotePartial` produce.
        let access = empty_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = map_codec::codec_of(BiomeGenerationSettings::map_codec_of::<TestOps>());
        let decoded = codec
            .parse(
                &ops,
                &json!({"carvers": [], "features": [["minecraft:test_feature"]]}),
            )
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.get_carvers().size(), 0);
        // One feature step (the partial outer list), holding an empty holder set.
        assert_eq!(decoded.features().len(), 1);
        assert_eq!(decoded.features()[0].size(), 0);
    }

    /// A configured feature whose config is a do-nothing placeholder (no
    /// sub-features), for the registry fixtures.
    fn configured_feature(id: u32) -> ConfiguredFeatureErased {
        ConfiguredFeatureErased {
            feature: crate::levelgen::feature::FeatureId::new(id),
            config: Arc::new(NoSubFeatureConfig),
        }
    }

    /// A do-nothing `FeatureConfiguration` placeholder (default
    /// `get_sub_features`: empty).
    #[derive(Debug)]
    struct NoSubFeatureConfig;
    impl crate::levelgen::feature::configurations::FeatureConfiguration for NoSubFeatureConfig {}

    /// A `FeatureConfiguration` whose `get_sub_features` yields one holder —
    /// the sub-feature expansion exercised by `getBoneMealFeatures`.
    #[derive(Debug)]
    struct WithSubFeatureConfig(Holder<ConfiguredFeatureErased>);
    impl crate::levelgen::feature::configurations::FeatureConfiguration for WithSubFeatureConfig {
        fn get_sub_features(
            &self,
        ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
            Box::new(std::iter::once(self.0.clone()))
        }
    }

    /// A `RegistryAccess` carrying CONFIGURED_CARVER, CONFIGURED_FEATURE and
    /// PLACED_FEATURE registries.
    ///
    /// The configured-feature registry holds `minecraft:grass` (id 0, tagged
    /// `CAN_SPAWN_FROM_BONE_MEAL` via `bind_tags` pre-freeze),
    /// `minecraft:flower` (id 1, untagged, no sub-features) and `minecraft:tree`
    /// (id 2, untagged, whose config yields `grass` as a sub-feature). The
    /// placed-feature registry holds `minecraft:a` (→ grass), `minecraft:b`
    /// (→ flower) and `minecraft:c` (→ tree) at ids 0..2. The references are
    /// built in the same registries the ops resolve through (the back-reference
    /// rule), so `Holder::is_tag`/`Holder::value`/`getFeatures` resolve like
    /// Java's holder-stored values.
    fn access() -> RegistryAccess {
        let carvers = RegistryBuilder::new(&*CONFIGURED_CARVER).freeze();

        let mut features = RegistryBuilder::new(&*CONFIGURED_FEATURE);
        // The configured-feature registry id (the tree sub-holder references
        // the same registry the ops resolve through).
        let configured_registry_id = features.registry_id();
        features.register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse("minecraft:grass")),
            Arc::new(configured_feature(0)),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        features.register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse("minecraft:flower")),
            Arc::new(configured_feature(1)),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        features.register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse("minecraft:tree")),
            Arc::new(ConfiguredFeatureErased {
                feature: crate::levelgen::feature::FeatureId::new(2),
                config: Arc::new(WithSubFeatureConfig(Holder::reference(
                    configured_registry_id,
                    0,
                ))),
            }),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        features.bind_tags(vec![(
            (*CAN_SPAWN_FROM_BONE_MEAL).clone(),
            vec![rivet_registry::holder::HolderId(0)],
        )]);
        let features = features.freeze();

        let mut placed = RegistryBuilder::new(&*PLACED_FEATURE);
        // The placed features' configured-feature holders are references in the
        // same configured-feature registry (insertion order ids 0/1/2).
        placed.register(
            &ResourceKey::create(&*PLACED_FEATURE, Identifier::parse("minecraft:a")),
            Arc::new(PlacedFeature::new(
                Holder::reference(features.registry_id(), 0),
                Vec::new(),
            )),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        placed.register(
            &ResourceKey::create(&*PLACED_FEATURE, Identifier::parse("minecraft:b")),
            Arc::new(PlacedFeature::new(
                Holder::reference(features.registry_id(), 1),
                Vec::new(),
            )),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        placed.register(
            &ResourceKey::create(&*PLACED_FEATURE, Identifier::parse("minecraft:c")),
            Arc::new(PlacedFeature::new(
                Holder::reference(features.registry_id(), 2),
                Vec::new(),
            )),
            rivet_registry::registration_info::RegistrationInfo::BUILT_IN,
        );
        let placed = placed.freeze();

        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_carver",
                )),
                Box::new(carvers) as rivet_registry::root::AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_feature",
                )),
                Box::new(features) as rivet_registry::root::AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(placed) as rivet_registry::root::AnyBox,
            ),
        ])
    }

    /// A placed-feature reference through the access's placed-feature registry.
    fn placed_holder(access: &RegistryAccess, id: u32) -> Holder<PlacedFeature> {
        let registry =
            RegistryAccess::lookup(access, &*PLACED_FEATURE).expect("placed feature registry");
        Holder::reference(registry.registry_id(), id)
    }

    /// A named-holder feature set: `minecraft:a` in step 0, `minecraft:b` in
    /// step 1 — `features` as the `PlainBuilder` would produce it (each step's
    /// holders wrapped in a direct `HolderSet`).
    fn two_step_settings(access: &RegistryAccess) -> BiomeGenerationSettings {
        PlainBuilder::default()
            .add_feature_index(0, placed_holder(access, 0))
            .add_feature_index(1, placed_holder(access, 1))
            .build()
    }

    /// The `(registry, id)` pairs of a placed-feature step's reference members,
    /// for comparing against the settings the decode produced.
    fn step_ref_ids(settings: &BiomeGenerationSettings, step: usize) -> Vec<(u32, u32)> {
        settings.features()[step]
            .iter()
            .map(|h| match h {
                Holder::Reference { registry, id } => (registry.0, *id),
                Holder::Direct(_) => panic!("expected a reference holder"),
            })
            .collect()
    }

    #[test]
    fn codec_round_trips_named_feature_references() {
        // `PlacedFeature.LIST_OF_LISTS_CODEC` = `homogeneousList(PLACED_FEATURE,
        // DIRECT_CODEC, true).listOf()` — named references resolve through the
        // placed-feature registry, so a non-empty `features` field now round-trips
        // (it used to hit the full element STUB). The decode is a list of holder
        // sets (the `listOf` outer), always kept as the list form (`alwaysUseList
        // = true`).
        let access = access();
        let settings = two_step_settings(&access);
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = map_codec::codec_of(BiomeGenerationSettings::map_codec_of::<TestOps>());
        let encoded = codec
            .encode_start(&ops, &settings)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"carvers": [], "features": [["minecraft:a"], ["minecraft:b"]]})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.features().len(), 2);
        assert_eq!(step_ref_ids(&decoded, 0), step_ref_ids(&settings, 0));
        assert_eq!(step_ref_ids(&decoded, 1), step_ref_ids(&settings, 1));
    }

    #[test]
    fn get_bone_meal_features_filters_can_spawn_from_bone_meal_tag() {
        // `features.stream().flatMap(HolderSet::stream).flatMap(feature ->
        // feature.value().getFeatures()).filter(feature ->
        // feature.is(FeatureTags.CAN_SPAWN_FROM_BONE_MEAL)).map(Holder::value)`
        // — `minecraft:a`'s configured feature (`grass`) is tagged and survives;
        // `minecraft:b`'s direct feature (`flower`) is not.
        let access = access();
        let settings = two_step_settings(&access);
        let placed_lookup = RegistryAccess::lookup(&access, &*PLACED_FEATURE).expect("placed");
        let configured_lookup =
            RegistryAccess::lookup(&access, &*CONFIGURED_FEATURE).expect("configured");
        let bone_meal = settings.get_bone_meal_features(placed_lookup, configured_lookup);
        assert_eq!(bone_meal.len(), 1);
        assert_eq!(bone_meal[0].feature, FeatureId::new(0), "minecraft:grass");
    }

    #[test]
    fn get_bone_meal_features_expands_sub_features() {
        // `feature.value().getFeatures()` = the configured feature plus its
        // sub-features; `minecraft:c`'s direct feature (`tree`) is untagged,
        // but its sub-feature (`grass`) is in the tag and survives.
        let access = access();
        let settings = PlainBuilder::default()
            .add_feature_index(0, placed_holder(&access, 2))
            .build();
        let placed_lookup = RegistryAccess::lookup(&access, &*PLACED_FEATURE).expect("placed");
        let configured_lookup =
            RegistryAccess::lookup(&access, &*CONFIGURED_FEATURE).expect("configured");
        let bone_meal = settings.get_bone_meal_features(placed_lookup, configured_lookup);
        assert_eq!(bone_meal.len(), 1);
        assert_eq!(bone_meal[0].feature, FeatureId::new(0), "grass sub-feature");
    }

    #[test]
    fn has_feature_defers_honestly() {
        // `featureSet.get().contains(feature)` needs `PlacedFeature` value
        // equality (`#181`), which is unavailable — the accessor fails
        // explicitly rather than fabricating a membership result.
        let access = access();
        let settings = two_step_settings(&access);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            settings.has_feature(&PlacedFeature::new(
                Holder::reference(
                    RegistryAccess::lookup(&access, &*CONFIGURED_FEATURE)
                        .expect("configured")
                        .registry_id(),
                    0,
                ),
                Vec::new(),
            ))
        }))
        .expect_err("hasFeature must fail explicitly until PlacedFeature value equality lands");
        // `panic!("literal")` carries a `&'static str` payload; a formatted
        // message would be a `String`. Accept either.
        let msg = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&'static str>().map(|s| s.to_string()))
            .unwrap();
        assert!(
            msg.contains("RivetTodo #181"),
            "the deferred-featureSet seam must name its issue: {msg}"
        );
    }
}
