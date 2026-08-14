//! Port of `net.minecraft.world.level.levelgen.feature.configurations.RandomFeatureConfiguration`
//! (record, 26.2) — owned by the `mc.world.level.levelgen.feature.selector` manifest unit.
//!
//! Java: a two-field record `record RandomFeatureConfiguration(List<WeightedPlacedFeature>
//! features, Holder<PlacedFeature> defaultFeature)`. Its `CODEC` is a
//! `RecordCodecBuilder` over the required `"features"` field
//! (`WeightedPlacedFeature.CODEC.listOf()` — the list-element codec is the record
//! over `"feature"`/`"chance"`) and the required `"default"` field
//! (`PlacedFeature.CODEC` — a `RegistryFileCodec` over `Registries.PLACED_FEATURE`).
//! Java's `apply2` is exactly the two-field group the port's `record_builder`
//! reproduces.
//!
//! `@Deprecated`: the vanilla registrations were superseded by
//! `WeightedRandomFeatureConfiguration`, but the codec and the deprecated selector
//! behavior remain reachable, so the port keeps the type (no compatibility layer —
//! it is the Java surface, faithfully).
//!
//! `getSubFeatures` is `Stream.concat(this.features.stream().flatMap(weighted ->
//! weighted.feature().value().getFeatures()), this.defaultFeature.value().getFeatures())`.
//! The per-placed-feature sub-streams come from
//! [`placed_sub_features`](crate::levelgen::feature::sub_features::placed_sub_features):
//! a `Direct` holder yields its contained configured-feature holder and that
//! feature's own sub-features (the recursion terminates at leaf configurations);
//! a `Reference` holder needs the placed-feature `HolderLookup`, which the
//! `FeatureConfiguration::get_sub_features` trait surface cannot thread — that STUB
//! fails explicitly, never fabricating a stream (see `sub_features.rs`).
//!
//! The `Holder<PlacedFeature>` field carries no `PartialEq` (`PlacedFeature`
//! derives none — its placement list is erased), so the record derives
//! `Clone`+`Debug` only, the same shape `VegetationPatchConfiguration` takes.

use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::configurations::vegetation_patch_configuration::placed_feature_codec;
use crate::levelgen::feature::sub_features::placed_sub_features;
use crate::levelgen::feature::weighted_placed_feature::{
    WeightedPlacedFeature, weighted_placed_feature_codec,
};
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.RandomFeatureConfiguration`.
#[derive(Debug, Clone)]
pub struct RandomFeatureConfiguration {
    /// `features` — the ordered `List<WeightedPlacedFeature>`.
    pub features: Vec<WeightedPlacedFeature>,
    /// `defaultFeature` — the fallback placed feature.
    pub default_feature: Holder<PlacedFeature>,
}

impl RandomFeatureConfiguration {
    /// `new RandomFeatureConfiguration(List<WeightedPlacedFeature>,
    /// Holder<PlacedFeature>)` — the record constructor (the codec's `apply`).
    pub fn new(
        features: Vec<WeightedPlacedFeature>,
        default_feature: Holder<PlacedFeature>,
    ) -> Self {
        RandomFeatureConfiguration {
            features,
            default_feature,
        }
    }

    /// `features()` — the record accessor.
    pub fn features(&self) -> &[WeightedPlacedFeature] {
        &self.features
    }

    /// `defaultFeature()` — the record accessor.
    pub fn default_feature(&self) -> &Holder<PlacedFeature> {
        &self.default_feature
    }
}

/// `RandomFeatureConfiguration.CODEC` — a record codec over the required
/// `"features"` field (`WeightedPlacedFeature.CODEC.listOf()`) and the required
/// `"default"` field (`PlacedFeature.CODEC`), as the ops-generic
/// `random_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.apply2(
///     RandomFeatureConfiguration::new,
///     WeightedPlacedFeature.CODEC.listOf().fieldOf("features").forGetter(c -> c.features),
///     PlacedFeature.CODEC.fieldOf("default").forGetter(c -> c.defaultFeature)))
/// ```
pub fn random_feature_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<RandomFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &RandomFeatureConfiguration| c.features.clone()),
                codec::field_of(
                    codec::list(weighted_placed_feature_codec::<Ops>()),
                    "features".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &RandomFeatureConfiguration| c.default_feature.clone()),
                codec::field_of(placed_feature_codec::<Ops>(), "default".to_string()),
            ))
            .apply(instance, Arc::new(RandomFeatureConfiguration::new))
    })
}

impl FeatureConfiguration for RandomFeatureConfiguration {
    /// `getSubFeatures()` — `Stream.concat(features.stream().flatMap(w ->
    /// w.feature().value().getFeatures()), defaultFeature.value().getFeatures())`.
    fn get_sub_features(&self) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        Box::new(
            self.features
                .iter()
                .flat_map(|weighted| placed_sub_features(&weighted.feature))
                .chain(placed_sub_features(&self.default_feature)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
    use crate::levelgen::feature::{ConfiguredFeatureErased, FeatureId};
    use crate::levelgen::placement::PlacedFeature;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_registry::{Identifier, ResourceKey};
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `RegistryAccess` over the two feature registries the selector codecs
    /// resolve through: a configured-feature registry holding
    /// `minecraft:oak`/`minecraft:birch` (the inline placed features reference
    /// them by id) and an empty placed-feature registry (the inline `DIRECT_CODEC`
    /// form never references a placed feature by name).
    fn access() -> RegistryAccess {
        let mut configured = RegistryBuilder::new(&*CONFIGURED_FEATURE);
        configured.register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse("minecraft:oak")),
            Arc::new(ConfiguredFeatureErased {
                feature: FeatureId::new(0),
                config: Arc::new(PlaceholderConfig),
            }),
            RegistrationInfo::BUILT_IN,
        );
        configured.register(
            &ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse("minecraft:birch")),
            Arc::new(ConfiguredFeatureErased {
                feature: FeatureId::new(1),
                config: Arc::new(PlaceholderConfig),
            }),
            RegistrationInfo::BUILT_IN,
        );
        let configured = configured.freeze();
        let placed = RegistryBuilder::new(&*PLACED_FEATURE);
        let placed = placed.freeze();
        // `from_pairs` stores erased `RegistryKey<()>` keys; `lookup` erases the
        // typed key by re-reading its identifier, so the pair keys must use the
        // same identifier form the registry keys carry.
        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_feature",
                )),
                Box::new(configured) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(placed) as AnyBox,
            ),
        ])
    }

    /// A do-nothing `FeatureConfiguration` placeholder for the configured-feature
    /// registry values (never decoded/encoded — the holders are references).
    #[derive(Debug)]
    struct PlaceholderConfig;
    impl FeatureConfiguration for PlaceholderConfig {}

    /// `Holder<ConfiguredFeatureErased>` by insertion index through the same
    /// access the ops use.
    fn configured_feature(access: &RegistryAccess, id: u32) -> Holder<ConfiguredFeatureErased> {
        let registry = RegistryAccess::lookup(access, &*CONFIGURED_FEATURE)
            .expect("configured feature registry");
        Holder::reference(registry.registry_id(), id)
    }

    /// An inline placed feature wrapping a configured-feature reference — the
    /// `DIRECT_CODEC` form the fixture JSON and the round-trip use.
    fn inline_placed(configured: Holder<ConfiguredFeatureErased>) -> Holder<PlacedFeature> {
        Holder::direct(PlacedFeature::new(configured, Vec::new()))
    }

    fn oak(access: &RegistryAccess) -> Holder<PlacedFeature> {
        inline_placed(configured_feature(access, 0))
    }

    fn birch(access: &RegistryAccess) -> Holder<PlacedFeature> {
        inline_placed(configured_feature(access, 1))
    }

    fn sample_config(access: &RegistryAccess) -> RandomFeatureConfiguration {
        RandomFeatureConfiguration::new(
            vec![
                WeightedPlacedFeature::new(oak(access), 0.8),
                WeightedPlacedFeature::new(birch(access), 0.2),
            ],
            birch(access),
        )
    }

    /// An inline `Direct` configured feature — the lookup-free form the
    /// `get_sub_features` tests must use (`placed_sub_features` cannot resolve
    /// a `Reference` through the trait surface; see `sub_features.rs`).
    fn inline_configured(id: u32) -> Holder<ConfiguredFeatureErased> {
        Holder::direct(ConfiguredFeatureErased {
            feature: FeatureId::new(id),
            config: Arc::new(PlaceholderConfig),
        })
    }

    fn ops(access: &RegistryAccess) -> TestOps {
        // `create_from_access` owns the access; cloning is a cheap `Arc` bump.
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone())
    }

    #[test]
    fn codec_round_trip() {
        let access = access();
        let config = sample_config(&access);
        let codec = random_feature_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops(&access), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "features": [
                    {"feature": {"feature": "minecraft:oak", "placement": []}, "chance": 0.8},
                    {"feature": {"feature": "minecraft:birch", "placement": []}, "chance": 0.2},
                ],
                "default": {"feature": "minecraft:birch", "placement": []},
            })
        );
        let decoded = codec
            .parse(&ops(&access), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.features.len(), 2);
        assert_eq!(decoded.features[0].chance(), 0.8);
        assert_eq!(decoded.features[1].chance(), 0.2);
        // The default is the inline placed feature wrapping a birch reference.
        match &decoded.default_feature {
            Holder::Direct(pf) => match &pf.feature {
                Holder::Reference { id, .. } => assert_eq!(*id, 1),
                other => panic!("expected a reference configured feature, got {other:?}"),
            },
            other => panic!("expected an inline placed feature, got {other:?}"),
        }
    }

    #[test]
    fn codec_requires_all_fields() {
        let access = access();
        let codec = random_feature_configuration_codec::<TestOps>();
        // `fieldOf` on both — an empty map and each single-field omission error.
        assert!(codec.parse(&ops(&access), &json!({})).is_error());
        let missing_default = json!({
            "features": [
                {"feature": {"feature": "minecraft:oak", "placement": []}, "chance": 0.8},
            ],
        });
        let result = codec.parse(&ops(&access), &missing_default);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key default"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_out_of_range_chance() {
        // `Codec.floatRange(0.0F, 1.0F)` on the "chance" field — a value below
        // the inclusive lower bound errors with Java's exact message on decode.
        let access = access();
        let codec = random_feature_configuration_codec::<TestOps>();
        let bad = json!({
            "features": [
                {"feature": {"feature": "minecraft:oak", "placement": []}, "chance": -0.1},
            ],
            "default": {"feature": "minecraft:birch", "placement": []},
        });
        let result = codec.parse(&ops(&access), &bad);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value -0.1 outside of range [0.0:1.0]");
    }

    #[test]
    fn get_sub_features_concatenates_weighted_and_default() {
        // `Stream.concat(features.stream().flatMap(w -> w.feature().value().getFeatures()),
        // defaultFeature.value().getFeatures())` — each placed feature yields
        // its contained configured-feature holder plus (for a nested config) its
        // own sub-features. Here the configured features are leaf placeholders,
        // so each placed feature contributes exactly one holder. The configured
        // holders are inline `Direct` (see `sub_features.rs` — a `Reference`
        // cannot be resolved through the trait surface), so the assertion reads
        // the FeatureId.
        let oak = inline_configured(0);
        let birch = inline_configured(1);
        let config = RandomFeatureConfiguration::new(
            vec![
                WeightedPlacedFeature::new(
                    Holder::direct(PlacedFeature::new(oak, Vec::new())),
                    0.8,
                ),
                WeightedPlacedFeature::new(
                    Holder::direct(PlacedFeature::new(birch, Vec::new())),
                    0.2,
                ),
            ],
            Holder::direct(PlacedFeature::new(inline_configured(1), Vec::new())),
        );
        let subs: Vec<_> = config.get_sub_features().collect();
        // oak + birch (the weighted list) + birch (the default).
        assert_eq!(subs.len(), 3);
        for (i, sub) in subs.iter().enumerate() {
            match sub {
                Holder::Direct(cf) => assert_eq!(cf.feature.id, if i == 0 { 0 } else { 1 }),
                other => panic!("expected a direct configured feature, got {other:?}"),
            }
        }
    }
}
