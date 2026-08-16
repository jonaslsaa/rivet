//! Port of `net.minecraft.world.level.levelgen.feature.configurations.
//! HugeMushroomFeatureConfiguration` (record, 26.2).
//!
//! Java: a record `record HugeMushroomFeatureConfiguration(BlockStateProvider
//! capProvider, BlockStateProvider stemProvider, int foliageRadius,
//! BlockPredicate canPlaceOn)` whose `CODEC` is a `RecordCodecBuilder` over the
//! required `"cap_provider"` field (`BlockStateProvider.CODEC` — the `"type"`
//! by-name dispatch), the required `"stem_provider"` field (the same codec),
//! the `"foliage_radius"` field (`Codec.INT.optionalFieldOf("foliage_radius",
//! 2)` — the NON-lenient with-default optional), and the required
//! `"can_place_on"` field (`BlockPredicate.CODEC` — the `"type"` by-name
//! dispatch). DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java
//! constant is exposed as the ops-generic
//! `huge_mushroom_feature_configuration_codec::<Ops>()` factory.
//!
//! The `cap_provider`/`stem_provider` halves are the erased
//! `Arc<dyn ErasedBlockStateProvider>` carriers and `can_place_on` is the
//! erased `Arc<dyn BlockPredicate>` carrier — the traits do not extend
//! `PartialEq` (providers/predicates are behavior, not values), so the
//! configuration is `Clone`+`Debug` only — no `PartialEq` (the same shape
//! `BlockBlobConfiguration`/`DiskConfiguration` take). `foliage_radius` is a
//! value component.
//!
//! ## The `foliage_radius` optional field
//!
//! `Codec.optionalFieldOf(name, default)` (two-arg) is the NON-lenient
//! with-default form (DFU 10.0.21: `optionalField(name, this,
//! false).xmap(o -> o.orElse(default), a -> Objects.equals(a, default) ?
//! Optional.empty() : Optional.of(a))`). Unlike `lenientOptionalFieldOf`, a
//! present-but-malformed `"foliage_radius"` is a decode error; the field is
//! omitted on encode when value-equal to the default `2`. The shared
//! `rivet-serialization::codec::optional_field_of` helper implements this
//! non-lenient with-default form — the twin of the crate's lenient
//! `lenient_optional_field_of`, differing only in `lenient=false`.

use crate::levelgen::blockpredicates::{BlockPredicate, block_predicate_codec};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.HugeMushroomFeatureConfiguration`.
///
/// The `capProvider`/`stemProvider` halves are held as the erased
/// `Arc<dyn ErasedBlockStateProvider>` carriers and `canPlaceOn` as the erased
/// `Arc<dyn BlockPredicate>` carrier; the traits do not extend `PartialEq`, so
/// the configuration is `Clone`+`Debug` only (the same shape
/// `BlockBlobConfiguration`/`DiskConfiguration` take).
#[derive(Debug, Clone)]
pub struct HugeMushroomFeatureConfiguration {
    /// `capProvider` — the provider for the mushroom cap blocks.
    pub cap_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `stemProvider` — the provider for the mushroom stem blocks.
    pub stem_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `foliageRadius` — the cap's foliage radius, defaulting to `2`.
    pub foliage_radius: i32,
    /// `canPlaceOn` — the predicate for blocks the mushroom may grow on.
    pub can_place_on: Arc<dyn BlockPredicate>,
}

impl HugeMushroomFeatureConfiguration {
    /// `new HugeMushroomFeatureConfiguration(BlockStateProvider,
    /// BlockStateProvider, int, BlockPredicate)` — the record constructor (the
    /// codec's `apply` function).
    pub fn new(
        cap_provider: Arc<dyn ErasedBlockStateProvider>,
        stem_provider: Arc<dyn ErasedBlockStateProvider>,
        foliage_radius: i32,
        can_place_on: Arc<dyn BlockPredicate>,
    ) -> Self {
        HugeMushroomFeatureConfiguration {
            cap_provider,
            stem_provider,
            foliage_radius,
            can_place_on,
        }
    }

    /// `HugeMushroomFeatureConfiguration.capProvider()`.
    pub fn cap_provider(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.cap_provider
    }

    /// `HugeMushroomFeatureConfiguration.stemProvider()`.
    pub fn stem_provider(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.stem_provider
    }

    /// `HugeMushroomFeatureConfiguration.foliageRadius()`.
    pub fn foliage_radius(&self) -> i32 {
        self.foliage_radius
    }

    /// `HugeMushroomFeatureConfiguration.canPlaceOn()`.
    pub fn can_place_on(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_place_on
    }
}

/// `HugeMushroomFeatureConfiguration.CODEC` — a record codec over the required
/// `"cap_provider"`, `"stem_provider"` and `"can_place_on"` fields and the
/// non-lenient with-default `"foliage_radius"` field, as the ops-generic
/// `huge_mushroom_feature_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockStateProvider.CODEC.fieldOf("cap_provider"),
///     BlockStateProvider.CODEC.fieldOf("stem_provider"),
///     Codec.INT.optionalFieldOf("foliage_radius", 2),
///     BlockPredicate.CODEC.fieldOf("can_place_on"))
///     .apply(i, HugeMushroomFeatureConfiguration::new))
/// ```
pub fn huge_mushroom_feature_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HugeMushroomFeatureConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &HugeMushroomFeatureConfiguration| c.cap_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "cap_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &HugeMushroomFeatureConfiguration| c.stem_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "stem_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &HugeMushroomFeatureConfiguration| c.foliage_radius),
                codec::optional_field_of::<i32, Ops>(
                    "foliage_radius",
                    codec::int_codec::<Ops>(),
                    2,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &HugeMushroomFeatureConfiguration| c.can_place_on.clone()),
                codec::field_of(block_predicate_codec::<Ops>(), "can_place_on".to_string()),
            ))
            .apply(
                instance,
                Arc::new(
                    |cap_provider: Arc<dyn ErasedBlockStateProvider>,
                     stem_provider: Arc<dyn ErasedBlockStateProvider>,
                     foliage_radius: i32,
                     can_place_on: Arc<dyn BlockPredicate>| {
                        HugeMushroomFeatureConfiguration::new(
                            cap_provider,
                            stem_provider,
                            foliage_radius,
                            can_place_on,
                        )
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration
    for HugeMushroomFeatureConfiguration
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// `block_state_provider_codec`/`block_predicate_codec` dispatch over the
    /// registry-backed matching predicates, so the codec requires `RegistryOps`
    /// (the `RegistryOpsLookup` ops). An empty access is enough — the providers
    /// are `simple` and the predicate is `always_true`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn cap() -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(simple(BlockState::of(
            BlockId::from_name("minecraft:brown_mushroom_block").unwrap(),
        )))
    }

    fn stem() -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(simple(BlockState::of(
            BlockId::from_name("minecraft:mushroom_stem").unwrap(),
        )))
    }

    fn config(foliage_radius: i32) -> HugeMushroomFeatureConfiguration {
        HugeMushroomFeatureConfiguration::new(cap(), stem(), foliage_radius, always_true())
    }

    fn json_config(
        cap_block: &str,
        stem_block: &str,
        foliage_radius: serde_json::Value,
    ) -> serde_json::Value {
        json!({
            "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": cap_block}},
            "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": stem_block}},
            "foliage_radius": foliage_radius,
            "can_place_on": {"type": "minecraft:true"},
        })
    }

    #[test]
    fn codec_round_trip_with_default_foliage_radius_omitted() {
        let codec = huge_mushroom_feature_configuration_codec::<TestOps>();
        let ops = ops();
        // `foliage_radius == 2` equals the default, so the field is OMITTED on
        // encode (DFU `Objects.equals(a, default) ? Optional.empty() : ...`).
        // The `state` halves carry a `"Properties"` object: both
        // `minecraft:brown_mushroom_block` and `minecraft:mushroom_stem` are
        // non-singleton states (six boolean face properties, default all-true),
        // so Java's `StateHolder.codec` (`StateHolder.java:199`) always writes
        // the `"Properties"` field via its encode half `Optional::of(state)`.
        let config = config(2);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:brown_mushroom_block", "Properties": {"west": "true", "up": "true", "south": "true", "north": "true", "east": "true", "down": "true"}}},
                "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:mushroom_stem", "Properties": {"west": "true", "up": "true", "south": "true", "north": "true", "east": "true", "down": "true"}}},
                "can_place_on": {"type": "minecraft:true"},
            })
        );
        // Decoding the omitted field restores the default.
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.foliage_radius, 2);
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.cap_provider()),
            ErasedBlockStateProvider::type_id(&**config.cap_provider())
        );
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.stem_provider()),
            ErasedBlockStateProvider::type_id(&**config.stem_provider())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_place_on()),
            BlockPredicate::type_id(&**config.can_place_on())
        );
    }

    #[test]
    fn codec_round_trip_with_non_default_foliage_radius() {
        let codec = huge_mushroom_feature_configuration_codec::<TestOps>();
        let ops = ops();
        // A non-default `foliage_radius` is encoded and decoded back exactly.
        // As in the default-radius test, both block states are non-singleton
        // states, so the `"Properties"` object is always written (Java
        // `StateHolder.codec` encode half `Optional::of(state)`).
        let config = config(3);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:brown_mushroom_block", "Properties": {"west": "true", "up": "true", "south": "true", "north": "true", "east": "true", "down": "true"}}},
                "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:mushroom_stem", "Properties": {"west": "true", "up": "true", "south": "true", "north": "true", "east": "true", "down": "true"}}},
                "foliage_radius": 3,
                "can_place_on": {"type": "minecraft:true"},
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.foliage_radius, 3);
    }

    #[test]
    fn codec_decodes_absent_foliage_radius_to_default() {
        let codec = huge_mushroom_feature_configuration_codec::<TestOps>();
        let ops = ops();
        // A JSON without the optional field decodes to the default radius 2.
        let input = json!({
            "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:brown_mushroom_block"}},
            "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:mushroom_stem"}},
            "can_place_on": {"type": "minecraft:true"},
        });
        let decoded = codec
            .parse(&ops, &input)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.foliage_radius, 2);
    }

    #[test]
    fn codec_rejects_malformed_present_foliage_radius() {
        // The two-arg `optionalFieldOf` is NON-lenient: a present-but-malformed
        // `"foliage_radius"` is a decode error (NOT a fallback to the default).
        let codec = huge_mushroom_feature_configuration_codec::<TestOps>();
        let ops = ops();
        let input = json_config(
            "minecraft:brown_mushroom_block",
            "minecraft:mushroom_stem",
            json!("not-an-int"),
        );
        let result = codec.parse(&ops, &input);
        assert!(result.is_error(), "malformed foliage_radius must error");
    }

    #[test]
    fn codec_requires_the_three_required_fields() {
        let codec = huge_mushroom_feature_configuration_codec::<TestOps>();
        let ops = ops();
        // Empty map: every required field missing.
        assert!(codec.parse(&ops, &json!({})).is_error());
        // Missing `cap_provider`.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:mushroom_stem"}},
                        "can_place_on": {"type": "minecraft:true"},
                    })
                )
                .is_error()
        );
        // Missing `stem_provider`.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:brown_mushroom_block"}},
                        "can_place_on": {"type": "minecraft:true"},
                    })
                )
                .is_error()
        );
        // Missing `can_place_on`.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "cap_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:brown_mushroom_block"}},
                        "stem_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:mushroom_stem"}},
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn accessors_return_the_record_components() {
        let config = config(7);
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**config.cap_provider()),
            crate::levelgen::feature::stateproviders::block_state_provider::SIMPLE_STATE_PROVIDER
        );
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**config.stem_provider()),
            crate::levelgen::feature::stateproviders::block_state_provider::SIMPLE_STATE_PROVIDER
        );
        assert_eq!(config.foliage_radius(), 7);
        assert_eq!(
            BlockPredicate::type_id(&**config.can_place_on()),
            crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes::TRUE
        );
    }
}
