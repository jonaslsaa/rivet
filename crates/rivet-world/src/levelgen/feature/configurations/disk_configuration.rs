//! Port of `net.minecraft.world.level.levelgen.feature.configurations.DiskConfiguration`
//! (record, 26.2).
//!
//! Java: a four-field record `record DiskConfiguration(BlockStateProvider
//! stateProvider, BlockPredicate target, IntProvider radius, int halfHeight)`
//! whose `CODEC` is a `RecordCodecBuilder` over the required
//! `"state_provider"` field (`BlockStateProvider.CODEC` — the `"type"` by-name
//! dispatch), the required `"target"` field (`BlockPredicate.CODEC` — the
//! `"type"` by-name dispatch), the required `"radius"` field
//! (`IntProviders.codec(0, 8)` — the integer provider dispatch codec validated
//! to the inclusive `[0, 8]` range), and the required `"half_height"` field
//! (`Codec.intRange(0, 4)` — the inclusive `[0, 4]` int window). DFU `Codec<T>`
//! is `Codec<E, Ops>` in the port, so the static Java constant is exposed as
//! the ops-generic `disk_configuration_codec::<Ops>()` factory.
//!
//! All four components are public in Java (record accessors), mirrored as
//! public fields. The radius bounds validation runs on both decode and encode
//! (Java's `IntProviders.codec` is a `.validate(...)` wrapper around the
//! constant-or-dispatch `CODEC`, exactly like the `codec::validate` used for
//! the `[0, 8]` window here), with Paper's exact `"Value provider too low"` /
//! `"Value provider too high"` messages. The `state_provider`/`target` halves
//! are compared by behavior (their `type_id`), matching the erased
//! provider/predicate carriers; the `radius`/`half_height` halves are value
//! comparisons, so the record derives no `PartialEq` — consistent with the
//! other configuration value types that hold erased carriers
//! (`BlockBlobConfiguration`).

use crate::levelgen::blockpredicates::{BlockPredicate, block_predicate_codec};
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_codec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.DiskConfiguration`.
///
/// The `BlockStateProvider` is held as the erased `Arc<dyn BlockStateProvider>`
/// carrier and the trait does not extend `PartialEq` (providers are behavior,
/// not values); the `BlockPredicate` is held the same way. So the configuration
/// is `Clone`+`Debug` only — no `PartialEq` (the same shape
/// `BlockBlobConfiguration` takes).
#[derive(Debug, Clone)]
pub struct DiskConfiguration {
    /// `stateProvider` — the block state provider for the disk's blocks.
    pub state_provider: Arc<dyn crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider>,
    /// `target` — the predicate for blocks the disk may replace.
    pub target: Arc<dyn BlockPredicate>,
    /// `radius` — an `IntProvider` validated to the inclusive `[0, 8]` range.
    pub radius: IntProvider,
    /// `halfHeight` — `[0, 4]`.
    pub half_height: i32,
}

impl DiskConfiguration {
    /// `new DiskConfiguration(BlockStateProvider, BlockPredicate, IntProvider,
    /// int)` — the record constructor (the codec's `apply` function).
    pub fn new(
        state_provider: Arc<dyn crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider>,
        target: Arc<dyn BlockPredicate>,
        radius: IntProvider,
        half_height: i32,
    ) -> Self {
        DiskConfiguration {
            state_provider,
            target,
            radius,
            half_height,
        }
    }

    /// `DiskConfiguration.stateProvider()`.
    pub fn state_provider(
        &self,
    ) -> &Arc<dyn crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider>
{
        &self.state_provider
    }

    /// `DiskConfiguration.target()`.
    pub fn target(&self) -> &Arc<dyn BlockPredicate> {
        &self.target
    }

    /// `DiskConfiguration.radius()`.
    pub fn radius(&self) -> &IntProvider {
        &self.radius
    }

    /// `DiskConfiguration.halfHeight()`.
    pub fn half_height(&self) -> i32 {
        self.half_height
    }
}

/// `DiskConfiguration.CODEC` — a record codec over the required
/// `"state_provider"`, `"target"`, `"radius"` and `"half_height"` fields, as
/// the ops-generic `disk_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockStateProvider.CODEC.fieldOf("state_provider"),
///     BlockPredicate.CODEC.fieldOf("target"),
///     IntProviders.codec(0, 8).fieldOf("radius"),
///     Codec.intRange(0, 4).fieldOf("half_height"))
///     .apply(i, DiskConfiguration::new))
/// ```
pub fn disk_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<DiskConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &DiskConfiguration| c.state_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "state_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &DiskConfiguration| c.target.clone()),
                codec::field_of(block_predicate_codec::<Ops>(), "target".to_string()),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DiskConfiguration| c.radius.clone()),
                "radius".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 8),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &DiskConfiguration| c.half_height),
                "half_height".to_string(),
                codec::int_range::<Ops>(0, 4),
            ))
            .apply(instance, Arc::new(DiskConfiguration::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for DiskConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::{
        ErasedBlockStateProvider, simple,
    };
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    /// `block_state_provider_codec`/`block_predicate_codec` dispatch over the
    /// registry-backed matching predicates, so the codec requires `RegistryOps`
    /// (the `RegistryOpsLookup` ops). An empty access is enough — the provider
    /// here is `simple` and the predicate is `always_true`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn sand_config() -> DiskConfiguration {
        // Paper's MiscOverworldFeatures.DISK_SAND:
        // `new DiskConfiguration(BlockStateProvider.simple(Blocks.SAND),
        // BlockPredicate.matchesBlocks(Blocks.DIRT), ConstantInt.of(2), 1)` —
        // the predicate half is a bare `always_true` here.
        DiskConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:sand").unwrap(),
            ))),
            always_true(),
            IntProvider::Constant(ConstantInt::of(2)),
            1,
        )
    }

    #[test]
    fn codec_round_trip() {
        let codec = disk_configuration_codec::<TestOps>();
        let ops = ops();
        let config = sand_config();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        // `key_dispatch_codec`'s MapEncoder reproduces Java's KeyDispatchCodec
        // 'Encode key AFTER value' ordering, so the state fields emit before the
        // `"type"` key — the order Paper produces (JsonOps LinkedTreeMap). The
        // `radius`/`half_height` halves are plain values.
        assert_eq!(
            encoded,
            json!({
                "state_provider": {"state": {"Name": "minecraft:sand"}, "type": "minecraft:simple_state_provider"},
                "target": {"type": "minecraft:true"},
                "radius": 2,
                "half_height": 1,
            })
        );
        // Pin the byte order too — indexmap map equality is order-insensitive,
        // so the `json!` assertion alone cannot catch a regression that emits
        // the `"type"` key before the value fields (a parity break vs Paper's
        // LinkedTreeMap order). The compact string is the observable order.
        assert_eq!(
            serde_json::to_string(&encoded).expect("encode is json"),
            r#"{"state_provider":{"state":{"Name":"minecraft:sand"},"type":"minecraft:simple_state_provider"},"target":{"type":"minecraft:true"},"radius":2,"half_height":1}"#
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.radius, config.radius);
        assert_eq!(decoded.half_height, config.half_height);
        // The provider/predicate halves are behavior carriers; equality is by
        // dispatch identity (the `"type"` key), which is what the codec round-trips.
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.state_provider()),
            ErasedBlockStateProvider::type_id(&**config.state_provider())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.target()),
            BlockPredicate::type_id(&**config.target())
        );
    }

    #[test]
    fn codec_round_trips_a_non_constant_radius() {
        // The radius is a real `IntProviders.CODEC`: a non-constant dispatch
        // provider (here `uniform`) round-trips through the `"type"` key.
        let codec = disk_configuration_codec::<TestOps>();
        let ops = ops();
        let config = DiskConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:sand").unwrap(),
            ))),
            always_true(),
            IntProvider::Uniform(UniformInt::of(2, 6)),
            2,
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state_provider": {"state": {"Name": "minecraft:sand"}, "type": "minecraft:simple_state_provider"},
                "target": {"type": "minecraft:true"},
                "radius": {"min_inclusive": 2, "max_inclusive": 6, "type": "minecraft:uniform"},
                "half_height": 2,
            })
        );
        assert_eq!(
            serde_json::to_string(&encoded).expect("encode is json"),
            r#"{"state_provider":{"state":{"Name":"minecraft:sand"},"type":"minecraft:simple_state_provider"},"target":{"type":"minecraft:true"},"radius":{"min_inclusive":2,"max_inclusive":6,"type":"minecraft:uniform"},"half_height":2}"#
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.radius, config.radius);
        assert_eq!(decoded.half_height, config.half_height);
    }

    #[test]
    fn codec_rejects_out_of_bounds_radius_on_encode() {
        let codec = disk_configuration_codec::<TestOps>();
        let ops = ops();
        // radius above 8.
        let radius_too_high = DiskConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:sand").unwrap(),
            ))),
            always_true(),
            IntProvider::Constant(ConstantInt::of(9)),
            1,
        );
        assert!(
            codec
                .encode_start(&ops, &radius_too_high)
                .result()
                .is_none()
        );
        // radius below 0.
        let radius_too_low = DiskConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:sand").unwrap(),
            ))),
            always_true(),
            IntProvider::Constant(ConstantInt::of(-1)),
            1,
        );
        assert!(codec.encode_start(&ops, &radius_too_low).result().is_none());
    }

    #[test]
    fn codec_rejects_out_of_range_half_height() {
        let codec = disk_configuration_codec::<TestOps>();
        let ops = ops();
        // half_height above 4 on decode.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "state_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:sand"}},
                        "target": {"type": "minecraft:true"},
                        "radius": 2,
                        "half_height": 5,
                    })
                )
                .is_error()
        );
        // half_height negative on decode.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "state_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:sand"}},
                        "target": {"type": "minecraft:true"},
                        "radius": 2,
                        "half_height": -1,
                    })
                )
                .is_error()
        );
        // half_height above 4 on encode (intRange validates both directions).
        let half_too_high = DiskConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:sand").unwrap(),
            ))),
            always_true(),
            IntProvider::Constant(ConstantInt::of(2)),
            5,
        );
        assert!(codec.encode_start(&ops, &half_too_high).result().is_none());
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = disk_configuration_codec::<TestOps>();
        let ops = ops();
        assert!(codec.parse(&ops, &json!({})).is_error());
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "target": {"type": "minecraft:true"},
                        "radius": 2,
                        "half_height": 1,
                    })
                )
                .is_error()
        );
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "state_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:sand"}},
                        "radius": 2,
                        "half_height": 1,
                    })
                )
                .is_error()
        );
    }
}
