//! Port of `net.minecraft.world.level.levelgen.feature.configurations.RandomBooleanFeatureConfiguration`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.selector` manifest unit.
//!
//! Java: a two-field class with public final fields `Holder<PlacedFeature>
//! featureTrue`/`featureFalse` and a constructor assigning both. Its `CODEC` is a
//! `RecordCodecBuilder` over the required `"feature_true"` and `"feature_false"`
//! fields, both `PlacedFeature.CODEC` (a `RegistryFileCodec` over
//! `Registries.PLACED_FEATURE`).
//!
//! `getSubFeatures` is `Stream.concat(this.featureTrue.value().getFeatures(),
//! this.featureFalse.value().getFeatures())`, flattened through
//! [`placed_sub_features`](crate::levelgen::feature::sub_features::placed_sub_features)
//! (see the `random_feature_configuration` module doc for the Direct/Reference split).

use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::configurations::vegetation_patch_configuration::placed_feature_codec;
use crate::levelgen::feature::sub_features::placed_sub_features;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.RandomBooleanFeatureConfiguration`.
#[derive(Debug, Clone)]
pub struct RandomBooleanFeatureConfiguration {
    /// `featureTrue` — the placed feature the `RandomBooleanSelectorFeature`
    /// picks on a `true` draw.
    pub feature_true: Holder<PlacedFeature>,
    /// `featureFalse` — the placed feature picked on a `false` draw.
    pub feature_false: Holder<PlacedFeature>,
}

impl RandomBooleanFeatureConfiguration {
    /// `new RandomBooleanFeatureConfiguration(Holder<PlacedFeature>,
    /// Holder<PlacedFeature>)` — the constructor assigning both final fields.
    pub fn new(feature_true: Holder<PlacedFeature>, feature_false: Holder<PlacedFeature>) -> Self {
        RandomBooleanFeatureConfiguration {
            feature_true,
            feature_false,
        }
    }
}

/// `RandomBooleanFeatureConfiguration.CODEC` — a record codec over the required
/// `"feature_true"` and `"feature_false"` fields (both `PlacedFeature.CODEC`), as
/// the ops-generic `random_boolean_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     PlacedFeature.CODEC.fieldOf("feature_true").forGetter(c -> c.featureTrue),
///     PlacedFeature.CODEC.fieldOf("feature_false").forGetter(c -> c.featureFalse))
///     .apply(i, RandomBooleanFeatureConfiguration::new))
/// ```
pub fn random_boolean_feature_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<RandomBooleanFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &RandomBooleanFeatureConfiguration| c.feature_true.clone()),
                codec::field_of(placed_feature_codec::<Ops>(), "feature_true".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &RandomBooleanFeatureConfiguration| c.feature_false.clone()),
                codec::field_of(placed_feature_codec::<Ops>(), "feature_false".to_string()),
            ))
            .apply(instance, Arc::new(RandomBooleanFeatureConfiguration::new))
    })
}

impl FeatureConfiguration for RandomBooleanFeatureConfiguration {
    /// `getSubFeatures()` — `Stream.concat(featureTrue.value().getFeatures(),
    /// featureFalse.value().getFeatures())`.
    fn get_sub_features(&self) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        Box::new(
            placed_sub_features(&self.feature_true).chain(placed_sub_features(&self.feature_false)),
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

    #[derive(Debug)]
    struct PlaceholderConfig;
    impl FeatureConfiguration for PlaceholderConfig {}

    fn configured_feature(access: &RegistryAccess, id: u32) -> Holder<ConfiguredFeatureErased> {
        let registry = RegistryAccess::lookup(access, &*CONFIGURED_FEATURE)
            .expect("configured feature registry");
        Holder::reference(registry.registry_id(), id)
    }

    fn inline_placed(configured: Holder<ConfiguredFeatureErased>) -> Holder<PlacedFeature> {
        Holder::direct(PlacedFeature::new(configured, Vec::new()))
    }

    fn ops(access: &RegistryAccess) -> TestOps {
        // `create_from_access` owns the access; cloning is a cheap `Arc` bump.
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone())
    }

    #[test]
    fn codec_round_trip() {
        let access = access();
        let config = RandomBooleanFeatureConfiguration::new(
            inline_placed(configured_feature(&access, 0)),
            inline_placed(configured_feature(&access, 1)),
        );
        let codec = random_boolean_feature_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops(&access), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "feature_true": {"feature": "minecraft:oak", "placement": []},
                "feature_false": {"feature": "minecraft:birch", "placement": []},
            })
        );
        let decoded = codec
            .parse(&ops(&access), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        match &decoded.feature_true {
            Holder::Direct(pf) => match &pf.feature {
                Holder::Reference { id, .. } => assert_eq!(*id, 0),
                other => panic!("expected a reference configured feature, got {other:?}"),
            },
            other => panic!("expected an inline placed feature, got {other:?}"),
        }
    }

    #[test]
    fn codec_requires_all_fields() {
        let access = access();
        let codec = random_boolean_feature_configuration_codec::<TestOps>();
        // `fieldOf` on both — each single-field omission errors.
        let missing_false = json!({
            "feature_true": {"feature": "minecraft:oak", "placement": []},
        });
        let result = codec.parse(&ops(&access), &missing_false);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key feature_false"), "got: {msg}");
    }

    #[test]
    fn get_sub_features_concatenates_true_then_false() {
        // `Stream.concat(featureTrue.value().getFeatures(), featureFalse.value()
        // .getFeatures())` — the true feature's holders, then the false feature's.
        // The configured holders are inline `Direct` (see `sub_features.rs` — a
        // `Reference` cannot be resolved through the trait surface), so the
        // assertions read the FeatureId.
        let config = RandomBooleanFeatureConfiguration::new(
            Holder::direct(PlacedFeature::new(
                Holder::direct(ConfiguredFeatureErased {
                    feature: FeatureId::new(0),
                    config: Arc::new(PlaceholderConfig),
                }),
                Vec::new(),
            )),
            Holder::direct(PlacedFeature::new(
                Holder::direct(ConfiguredFeatureErased {
                    feature: FeatureId::new(1),
                    config: Arc::new(PlaceholderConfig),
                }),
                Vec::new(),
            )),
        );
        let subs: Vec<_> = config.get_sub_features().collect();
        assert_eq!(subs.len(), 2);
        match &subs[0] {
            Holder::Direct(cf) => assert_eq!(cf.feature.id, 0),
            other => panic!("expected a direct configured feature, got {other:?}"),
        }
        match &subs[1] {
            Holder::Direct(cf) => assert_eq!(cf.feature.id, 1),
            other => panic!("expected a direct configured feature, got {other:?}"),
        }
    }
}
