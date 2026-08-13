//! `net.minecraft.world.level.biome.BiomeGenerationSettings` — the biome's
//! carvers and features (issue #178, `mc.world.level.biome.core` unit).
//!
//! Faithful port of the 26.2 `BiomeGenerationSettings.java` value surface: the
//! `carvers`/`features` fields, the `CODEC`, the `Builder`/`PlainBuilder`, and
//! `EMPTY`. The memoized `boneMealFeatures`/`featureSet` and their accessors
//! (`getBoneMealFeatures`/`hasFeature`) defer — they need `PlacedFeature.
//! getFeatures` and `FeatureTags.CAN_SPAWN_FROM_BONE_MEAL` from the
//! feature/placement units.
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
//!   `RegistryCodecs.homogeneousList(PLACED_FEATURE, DIRECT_CODEC, true).listOf()`.
//!   `PlacedFeature`'s `DIRECT_CODEC` defers with the `#126` surface, so the
//!   element codec is a STUB.
//! - Both fields run `promotePartial(Util.prefix("Carver: "/"Features: ",
//!   logger))`. Java's `promotePartial` turns an error-with-partial into a
//!   success with the partial value (the error is logged), so a non-empty
//!   carver/feature list whose elements fail to resolve decodes to the empty
//!   list rather than failing the whole biome — the STUB errors are swallowed
//!   here exactly as unresolvable references would be in Java. The logger
//!   callback is a no-op (Rivet has no slf4j surface; only the promote-partial
//!   semantics matter).

use crate::levelgen::carver::ConfiguredWorldCarverErased;
use crate::levelgen::generation_step::Decoration;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFileCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::{Identifier, Registry, ResourceKey};
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

    // `getBoneMealFeatures()`/`hasFeature(PlacedFeature)` defer with the
    // memoized `boneMealFeatures`/`featureSet` (RivetTodo(#126/#181): needs
    // `PlacedFeature.getFeatures()` and `FeatureTags.CAN_SPAWN_FROM_BONE_MEAL`
    // from the feature/placement units).
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
/// ops-generic factory. The element `DIRECT_CODEC` defers with `#126`, so the
/// holder-set element codec is a STUB: an empty list round-trips, a non-empty
/// list errors honestly.
fn placed_feature_list_of_lists_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Vec<HolderSet<PlacedFeature>>, Ops>> {
    let stub: Arc<dyn Codec<HolderSet<PlacedFeature>, Ops>> = codec::of(
        encoder::error("PlacedFeature.LIST_OF_LISTS_CODEC is a STUB (RivetTodo #126)".to_string()),
        decoder::error("PlacedFeature.LIST_OF_LISTS_CODEC is a STUB (RivetTodo #126)".to_string()),
        "PlacedFeature.LIST_OF_LISTS[STUB]".to_string(),
    );
    codec::list(stub)
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
    use crate::levelgen::carver::CarverConfiguration;
    use crate::levelgen::carver::world_carver::WorldCarverId;
    use crate::levelgen::placement::PlacedFeature;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use serde_json::json;
    use std::sync::Arc;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[derive(Debug)]
    struct TestCarverConfig;

    impl CarverConfiguration for TestCarverConfig {}

    fn carver_holder() -> Holder<ConfiguredWorldCarverErased> {
        let erased = ConfiguredWorldCarverErased::new(
            WorldCarverId::new(0, "minecraft:cave"),
            Arc::new(TestCarverConfig),
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
    fn codec_non_empty_features_fall_back_to_empty() {
        let access = empty_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = map_codec::codec_of(BiomeGenerationSettings::map_codec_of::<TestOps>());
        // A non-empty features list hits the element STUB, but the field's
        // `promotePartial` (Java-faithful: an error-with-partial becomes a
        // success with the partial value) swallows it into empty features.
        let decoded = codec
            .parse(
                &ops,
                &json!({"carvers": [], "features": [["minecraft:test_feature"]]}),
            )
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.get_carvers().size(), 0);
        assert!(decoded.features().is_empty());
    }
}
