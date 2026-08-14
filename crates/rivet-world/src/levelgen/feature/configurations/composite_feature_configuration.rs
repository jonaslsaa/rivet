//! Port of `net.minecraft.world.level.levelgen.feature.configurations.CompositeFeatureConfiguration`
//! (record, 26.2) — owned by the `mc.world.level.levelgen.feature.selector` manifest unit.
//!
//! Java: a single-field record `record CompositeFeatureConfiguration(HolderSet<PlacedFeature>
//! features)`. Its `CODEC` is a `RecordCodecBuilder` over the required `"features"`
//! field (`ExtraCodecs.nonEmptyHolderSet(PlacedFeature.LIST_CODEC)`):
//! `PlacedFeature.LIST_CODEC` is `RegistryCodecs.homogeneousList(Registries.PLACED_FEATURE,
//! DIRECT_CODEC)` — a `HolderSetCodec` whose element codec is the placed-feature
//! `RegistryFileCodec` — and `nonEmptyHolderSet` validates the *direct* form is
//! non-empty (`list.unwrap().right().filter(List::isEmpty)`) with the exact message
//! `"List must have contents"`; a named (tag) set is never rejected.
//!
//! `getSubFeatures` is `this.features.stream().flatMap(f -> f.value().getFeatures())`,
//! flattened through [`placed_sub_features`](crate::levelgen::feature::sub_features::placed_sub_features)
//! (see the `random_feature_configuration` module doc for the Direct/Reference split).

use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::configurations::vegetation_patch_configuration::placed_feature_codec;
use crate::levelgen::feature::registry_keys::PLACED_FEATURE;
use crate::levelgen::feature::sub_features::placed_sub_features;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::HolderSetCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.CompositeFeatureConfiguration`.
#[derive(Debug, Clone)]
pub struct CompositeFeatureConfiguration {
    /// `features` — the `HolderSet<PlacedFeature>` (a named tag or an explicit
    /// holder list).
    pub features: HolderSet<PlacedFeature>,
}

impl CompositeFeatureConfiguration {
    /// `new CompositeFeatureConfiguration(HolderSet<PlacedFeature>)` — the
    /// record constructor (the codec's `apply`).
    pub fn new(features: HolderSet<PlacedFeature>) -> Self {
        CompositeFeatureConfiguration { features }
    }

    /// `features()` — the record accessor.
    pub fn features(&self) -> &HolderSet<PlacedFeature> {
        &self.features
    }
}

/// `PlacedFeature.LIST_CODEC` wrapped by `ExtraCodecs.nonEmptyHolderSet` — the
/// `"features"` field codec.
///
/// `PlacedFeature.LIST_CODEC` is `RegistryCodecs.homogeneousList(Registries.PLACED_FEATURE,
/// DIRECT_CODEC)`, i.e. `HolderSetCodec.create(key, PlacedFeature.CODEC, false)` —
/// the element codec is exactly the `RegistryFileCodec` `placed_feature_codec`.
/// `nonEmptyHolderSet` then validates that a *direct* (list) form is not empty,
/// leaving the named (tag) form unchecked — Java:
/// `list.unwrap().right().filter(List::isEmpty).isPresent() ? error("List must
/// have contents") : success(list)`.
fn non_empty_placed_holder_set_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HolderSet<PlacedFeature>, Ops>> {
    let holder_set: Arc<dyn Codec<HolderSet<PlacedFeature>, Ops>> = Arc::new(
        HolderSetCodec::create(&*PLACED_FEATURE, placed_feature_codec::<Ops>(), false),
    );
    codec::validate(
        holder_set,
        Arc::new(|set: &HolderSet<PlacedFeature>| {
            if matches!(set.unwrap(), Either::Right(holders) if holders.is_empty()) {
                DataResult::error("List must have contents")
            } else {
                DataResult::success(set.clone())
            }
        }),
    )
}

/// `CompositeFeatureConfiguration.CODEC` — a record codec over the required
/// `"features"` field (`nonEmptyHolderSet(PlacedFeature.LIST_CODEC)`), as the
/// ops-generic `composite_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     ExtraCodecs.nonEmptyHolderSet(PlacedFeature.LIST_CODEC).fieldOf("features")
///         .forGetter(CompositeFeatureConfiguration::features))
///     .apply(i, CompositeFeatureConfiguration::new))
/// ```
pub fn composite_feature_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<CompositeFeatureConfiguration, Ops>> {
    let features_field: Arc<dyn MapCodec<HolderSet<PlacedFeature>, Ops>> = codec::field_of(
        non_empty_placed_holder_set_codec::<Ops>(),
        "features".to_string(),
    );
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CompositeFeatureConfiguration| c.features.clone()),
                features_field,
            ))
            .apply(instance, Arc::new(CompositeFeatureConfiguration::new))
    })
}

impl FeatureConfiguration for CompositeFeatureConfiguration {
    /// `getSubFeatures()` — `this.features.stream().flatMap(f ->
    /// f.value().getFeatures())`.
    fn get_sub_features(&self) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        Box::new(
            self.features
                .iter()
                .flat_map(|feature| placed_sub_features(feature)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::registry_keys::CONFIGURED_FEATURE;
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

    /// The same two-registry access the other selector-config tests build.
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

    fn ops(access: &RegistryAccess) -> TestOps {
        // `create_from_access` owns the access; cloning is a cheap `Arc` bump.
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone())
    }

    /// A two-member direct holder set over the configured-feature references,
    /// through the same access the ops use.
    fn two_placed_features(access: &RegistryAccess) -> HolderSet<PlacedFeature> {
        let configured_registry = RegistryAccess::lookup(access, &*CONFIGURED_FEATURE)
            .expect("configured feature registry");
        HolderSet::direct(vec![
            Holder::direct(PlacedFeature::new(
                Holder::reference(configured_registry.registry_id(), 0),
                Vec::new(),
            )),
            Holder::direct(PlacedFeature::new(
                Holder::reference(configured_registry.registry_id(), 1),
                Vec::new(),
            )),
        ])
    }

    #[test]
    fn codec_round_trip_direct_set() {
        let access = access();
        let config = CompositeFeatureConfiguration::new(two_placed_features(&access));
        let codec = composite_feature_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops(&access), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "features": [
                    {"feature": "minecraft:oak", "placement": []},
                    {"feature": "minecraft:birch", "placement": []},
                ],
            })
        );
        let decoded = codec
            .parse(&ops(&access), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.features.size(), 2);
        match decoded.features.get(1) {
            Holder::Direct(pf) => match &pf.feature {
                Holder::Reference { id, .. } => assert_eq!(*id, 1),
                other => panic!("expected a reference configured feature, got {other:?}"),
            },
            other => panic!("expected an inline placed feature, got {other:?}"),
        }
    }

    #[test]
    fn codec_rejects_an_empty_direct_list_with_java_message() {
        // `nonEmptyHolderSet` rejects an empty *direct* list with the exact
        // message, on decode and encode.
        let access = access();
        let codec = composite_feature_configuration_codec::<TestOps>();
        let result = codec.parse(&ops(&access), &json!({"features": []}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "List must have contents");
        let empty = CompositeFeatureConfiguration::new(HolderSet::direct(Vec::new()));
        let result = codec.encode_start(&ops(&access), &empty);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "List must have contents");
    }

    #[test]
    fn codec_requires_the_features_field() {
        let access = access();
        let codec = composite_feature_configuration_codec::<TestOps>();
        let result = codec.parse(&ops(&access), &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key features"), "got: {msg}");
    }

    #[test]
    fn get_sub_features_flattens_each_placed_feature() {
        // `features.stream().flatMap(f -> f.value().getFeatures())` — each
        // member yields its contained configured-feature holder. The configured
        // holders are inline `Direct` (see `sub_features.rs` — a `Reference`
        // cannot be resolved through the trait surface), so the assertions read
        // the FeatureId.
        let members = HolderSet::direct(vec![
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
        ]);
        let config = CompositeFeatureConfiguration::new(members);
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
