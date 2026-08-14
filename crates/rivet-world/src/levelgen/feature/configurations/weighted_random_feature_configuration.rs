//! Port of `net.minecraft.world.level.levelgen.feature.configurations.WeightedRandomFeatureConfiguration`
//! (record, 26.2) — owned by the `mc.world.level.levelgen.feature.selector` manifest unit.
//!
//! Java: a single-field record `record WeightedRandomFeatureConfiguration(WeightedList<Holder<PlacedFeature>>
//! features)`. Its `CODEC` is a `RecordCodecBuilder` over the required `"features"`
//! field (`WeightedList.codec(PlacedFeature.CODEC)`) — the list-element codec is
//! `Weighted.codec(PlacedFeature.CODEC)`, a record over the required `"data"`
//! field (the `PlacedFeature.CODEC` holder codec) and the required `"weight"`
//! field (`NON_NEGATIVE_INT` — `"Value must be non-negative: {n}"`).
//!
//! `getSubFeatures` is `this.features.unwrap().stream().flatMap(weighted ->
//! weighted.value().value().getFeatures())` — each entry's `value` is a
//! `Holder<PlacedFeature>`, whose `.value()` resolves the placed feature and
//! yields its sub-features, flattened through
//! [`placed_sub_features`](crate::levelgen::feature::sub_features::placed_sub_features)
//! (see the `random_feature_configuration` module doc for the Direct/Reference split).
//!
//! The port materializes the flattened holder sequence eagerly. Java's stream is
//! lazy, but `WeightedList.unwrap()` clones its entries (the port's `WeightedList`
//! hides the backing list behind `unwrap`, issue #353), so a lazy iterator
//! borrowing a clone cannot outlive the `unwrap()` temporary — a self-referential
//! borrow the trait surface cannot express (see the `FeatureConfiguration`
//! `getSubFeatures` note). `getSubFeatures` is a pure read of the holders (no
//! side effects), so the eagerly materialized sequence is observably identical
//! to Java's lazy stream; the weighted lists are small (a handful of entries).
//!
//! `WeightedList` compares by `totalWeight` + entries order (no Java `toString`),
//! so the record derives `Clone`+`Debug` only (the entries' values are
//! `Holder<PlacedFeature>`, which carry no `PartialEq`).

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
use rivet_util::weighted::{Weighted, WeightedList, weighted_list_codec};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.WeightedRandomFeatureConfiguration`.
#[derive(Debug, Clone)]
pub struct WeightedRandomFeatureConfiguration {
    /// `features` — the `WeightedList<Holder<PlacedFeature>>`.
    pub features: WeightedList<Holder<PlacedFeature>>,
}

impl WeightedRandomFeatureConfiguration {
    /// `new WeightedRandomFeatureConfiguration(WeightedList<Holder<PlacedFeature>>)`
    /// — the record constructor (the codec's `apply`).
    pub fn new(features: WeightedList<Holder<PlacedFeature>>) -> Self {
        WeightedRandomFeatureConfiguration { features }
    }

    /// `features()` — the record accessor.
    pub fn features(&self) -> &WeightedList<Holder<PlacedFeature>> {
        &self.features
    }
}

/// `WeightedRandomFeatureConfiguration.CODEC` — a record codec over the required
/// `"features"` field (`WeightedList.codec(PlacedFeature.CODEC)`), as the
/// ops-generic `weighted_random_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     WeightedList.codec(PlacedFeature.CODEC).fieldOf("features")
///         .forGetter(WeightedRandomFeatureConfiguration::features))
///     .apply(i, WeightedRandomFeatureConfiguration::new))
/// ```
pub fn weighted_random_feature_configuration_codec<
    Ops: DynamicOps + 'static + RegistryOpsLookup,
>() -> Arc<dyn Codec<WeightedRandomFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &WeightedRandomFeatureConfiguration| c.features.clone()),
                codec::field_of(
                    weighted_list_codec(placed_feature_codec::<Ops>()),
                    "features".to_string(),
                ),
            ))
            .apply(instance, Arc::new(WeightedRandomFeatureConfiguration::new))
    })
}

impl FeatureConfiguration for WeightedRandomFeatureConfiguration {
    /// `getSubFeatures()` — `features.unwrap().stream().flatMap(w ->
    /// w.value().value().getFeatures())`. Eagerly materialized — the
    /// `WeightedList.unwrap()` clone cannot be borrowed lazily (a
    /// self-referential borrow, see the module doc and the trait note).
    fn get_sub_features(&self) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        let sub_features: Vec<Holder<ConfiguredFeatureErased>> = self
            .features
            .unwrap()
            .into_iter()
            .flat_map(|weighted: Weighted<Holder<PlacedFeature>>| {
                placed_sub_features(weighted.value()).collect::<Vec<_>>()
            })
            .collect();
        Box::new(sub_features.into_iter())
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

    fn sample_features(access: &RegistryAccess) -> WeightedList<Holder<PlacedFeature>> {
        WeightedList::new(&[
            Weighted::new(inline_placed(configured_feature(access, 0)), 9),
            Weighted::new(inline_placed(configured_feature(access, 1)), 1),
        ])
    }

    fn ops(access: &RegistryAccess) -> TestOps {
        // `create_from_access` owns the access; cloning is a cheap `Arc` bump.
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone())
    }

    #[test]
    fn codec_round_trip() {
        let access = access();
        let config = WeightedRandomFeatureConfiguration::new(sample_features(&access));
        let codec = weighted_random_feature_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops(&access), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "features": [
                    {"data": {"feature": "minecraft:oak", "placement": []}, "weight": 9},
                    {"data": {"feature": "minecraft:birch", "placement": []}, "weight": 1},
                ],
            })
        );
        let decoded = codec
            .parse(&ops(&access), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        let entries = decoded.features.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].weight(), 9);
        match entries[0].value() {
            Holder::Direct(pf) => match &pf.feature {
                Holder::Reference { id, .. } => assert_eq!(*id, 0),
                other => panic!("expected a reference configured feature, got {other:?}"),
            },
            other => panic!("expected an inline placed feature, got {other:?}"),
        }
    }

    #[test]
    fn codec_rejects_negative_weight_with_java_message() {
        // `NON_NEGATIVE_INT` on "weight" — `"Value must be non-negative: N"`.
        let access = access();
        let codec = weighted_random_feature_configuration_codec::<TestOps>();
        let bad = json!({
            "features": [
                {"data": {"feature": "minecraft:oak", "placement": []}, "weight": -1},
            ],
        });
        let result = codec.parse(&ops(&access), &bad);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value must be non-negative: -1");
    }

    #[test]
    fn codec_requires_the_features_field() {
        let access = access();
        let codec = weighted_random_feature_configuration_codec::<TestOps>();
        let result = codec.parse(&ops(&access), &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key features"), "got: {msg}");
    }

    #[test]
    fn get_sub_features_flattens_each_entry_in_weight_order() {
        // `features.unwrap().stream().flatMap(w -> w.value().value().getFeatures())`
        // — each entry's holder, in list order. The configured holders are inline
        // `Direct` (see `sub_features.rs` — a `Reference` cannot be resolved
        // through the trait surface), so the assertions read the FeatureId.
        let features = WeightedList::new(&[
            Weighted::new(
                Holder::direct(PlacedFeature::new(
                    Holder::direct(ConfiguredFeatureErased {
                        feature: FeatureId::new(0),
                        config: Arc::new(PlaceholderConfig),
                    }),
                    Vec::new(),
                )),
                9,
            ),
            Weighted::new(
                Holder::direct(PlacedFeature::new(
                    Holder::direct(ConfiguredFeatureErased {
                        feature: FeatureId::new(1),
                        config: Arc::new(PlaceholderConfig),
                    }),
                    Vec::new(),
                )),
                1,
            ),
        ]);
        let config = WeightedRandomFeatureConfiguration::new(features);
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
