//! Port of `net.minecraft.world.level.levelgen.feature.configurations.NetherForestVegetationConfig`
//! (class, 26.2).
//!
//! Java: `NetherForestVegetationConfig extends BlockPileConfiguration` — the
//! superclass contributes the `stateProvider` field, the subclass adds
//! `public final int spreadWidth` / `spreadHeight`. Its `CODEC` is a
//! `RecordCodecBuilder` over the required `"state_provider"` field
//! (`BlockStateProvider.CODEC` — the `"type"` by-name dispatch), the required
//! `"spread_width"` field (`ExtraCodecs.POSITIVE_INT` — validated to `[1,
//! MAX]`, error `"Value must be positive: {n}"`), and the required
//! `"spread_height"` field (also `POSITIVE_INT`). The constructor calls
//! `super(stateProvider)`. DFU `Codec<T>` is `Codec<E, Ops>` in the port, so
//! the static Java constant is exposed as the ops-generic
//! `nether_forest_vegetation_config_codec::<Ops>()` factory.
//!
//! Class hierarchy → struct embedding (PORTING.md): the superclass fields
//! become the embedded [`BlockPileConfiguration`] field named after the parent
//! (`block_pile_configuration`), and the inherited public `stateProvider` field
//! (accessed as `c.stateProvider` in the codec's `forGetter`) is read through
//! it — `c.block_pile_configuration.state_provider`. Java does not override
//! `equals` (identity semantics) and the provider is behavior, not a value, so
//! the config derives `Clone`+`Debug` only — no `PartialEq` (the same shape
//! the `DiskConfiguration` unit takes for its erased provider field).
//!
//! `BlockPileConfiguration` is owned by the
//! `mc.world.level.levelgen.feature.configurations.blockpile` manifest unit and
//! is fully ported in this wave (issue #391), so the embedded superclass half
//! is the real port in `block_pile_configuration.rs`, not a cross-unit stub.

use crate::levelgen::feature::configurations::BlockPileConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::positive_int;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.NetherForestVegetationConfig`.
#[derive(Debug, Clone)]
pub struct NetherForestVegetationConfig {
    /// The superclass window — Java `super(stateProvider)`; the embedded
    /// `BlockPileConfiguration`, whose `stateProvider` field is inherited.
    pub block_pile_configuration: BlockPileConfiguration,
    /// `spreadWidth` — `[1, MAX]`.
    pub spread_width: i32,
    /// `spreadHeight` — `[1, MAX]`.
    pub spread_height: i32,
}

impl NetherForestVegetationConfig {
    /// `new NetherForestVegetationConfig(BlockStateProvider, int, int)` — the
    /// public constructor (the codec's `apply` function); `super(stateProvider)`
    /// becomes the embedded `BlockPileConfiguration::new(state_provider)`.
    pub fn new(
        state_provider: Arc<dyn ErasedBlockStateProvider>,
        spread_width: i32,
        spread_height: i32,
    ) -> Self {
        NetherForestVegetationConfig {
            block_pile_configuration: BlockPileConfiguration::new(state_provider),
            spread_width,
            spread_height,
        }
    }

    /// `stateProvider` — the inherited `BlockPileConfiguration` field, read
    /// through the embedded superclass (Java accesses it directly on `this`).
    pub fn state_provider(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.block_pile_configuration.state_provider
    }
}

/// `NetherForestVegetationConfig.CODEC` — a record codec over the required
/// `"state_provider"` field and the two required `POSITIVE_INT`-validated
/// `"spread_width"`/`"spread_height"` fields, as the ops-generic
/// `nether_forest_vegetation_config_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockStateProvider.CODEC.fieldOf("state_provider").forGetter(c -> c.stateProvider),
///     ExtraCodecs.POSITIVE_INT.fieldOf("spread_width").forGetter(c -> c.spreadWidth),
///     ExtraCodecs.POSITIVE_INT.fieldOf("spread_height").forGetter(c -> c.spreadHeight))
///     .apply(i, NetherForestVegetationConfig::new))
/// ```
pub fn nether_forest_vegetation_config_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<NetherForestVegetationConfig, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &NetherForestVegetationConfig| {
                    c.block_pile_configuration.state_provider.clone()
                }),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "state_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &NetherForestVegetationConfig| c.spread_width),
                "spread_width".to_string(),
                positive_int::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &NetherForestVegetationConfig| c.spread_height),
                "spread_height".to_string(),
                positive_int::<Ops>(),
            ))
            .apply(instance, Arc::new(NetherForestVegetationConfig::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration
    for NetherForestVegetationConfig
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// `block_state_provider_codec` dispatches over the registry-backed
    /// provider types, so the codec requires `RegistryOps` (the
    /// `RegistryOpsLookup` ops). An empty access is enough — the provider here
    /// is `simple`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn nether_forest_vegetation_config() -> NetherForestVegetationConfig {
        NetherForestVegetationConfig::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:warped_roots").unwrap(),
            ))),
            8,
            4,
        )
    }

    #[test]
    fn codec_round_trip() {
        let codec = nether_forest_vegetation_config_codec::<TestOps>();
        let ops = ops();
        let config = nether_forest_vegetation_config();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state_provider": {
                    "type": "minecraft:simple_state_provider",
                    "state": {"Name": "minecraft:warped_roots"},
                },
                "spread_width": 8,
                "spread_height": 4,
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.spread_width, 8);
        assert_eq!(decoded.spread_height, 4);
        assert_eq!(
            decoded
                .state_provider()
                .as_any()
                .downcast_ref::<crate::levelgen::feature::stateproviders::SimpleStateProvider>()
                .unwrap()
                .state(),
            BlockState::of(BlockId::from_name("minecraft:warped_roots").unwrap())
        );
    }

    #[test]
    fn constructor_wires_the_superclass() {
        // `super(stateProvider)` — the embedded BlockPileConfiguration carries
        // the provider; the subclass fields are set alongside.
        let config = nether_forest_vegetation_config();
        assert_eq!(config.spread_width, 8);
        assert_eq!(config.spread_height, 4);
        assert_eq!(
            config
                .block_pile_configuration
                .state_provider
                .type_id()
                .location,
            "minecraft:simple_state_provider"
        );
        // The inherited-field accessor reads the same carrier.
        assert_eq!(
            config.state_provider().type_id().location,
            config
                .block_pile_configuration
                .state_provider
                .type_id()
                .location
        );
    }

    #[test]
    fn codec_rejects_non_positive_on_decode() {
        // POSITIVE_INT is `[1, MAX]`: 0 and negative values are errors, with
        // the Java-exact message `"Value must be positive: {n}"`.
        let codec = nether_forest_vegetation_config_codec::<TestOps>();
        let ops = ops();
        let zero = json!({
            "state_provider": {
                "type": "minecraft:simple_state_provider",
                "state": {"Name": "minecraft:warped_roots"},
            },
            "spread_width": 0,
            "spread_height": 4,
        });
        assert!(codec.parse(&ops, &zero).is_error());
        let negative = json!({
            "state_provider": {
                "type": "minecraft:simple_state_provider",
                "state": {"Name": "minecraft:warped_roots"},
            },
            "spread_width": 8,
            "spread_height": -1,
        });
        assert!(codec.parse(&ops, &negative).is_error());
    }

    #[test]
    fn codec_rejects_non_positive_on_encode() {
        let codec = nether_forest_vegetation_config_codec::<TestOps>();
        let ops = ops();
        let bad = NetherForestVegetationConfig::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:warped_roots").unwrap(),
            ))),
            0,
            4,
        );
        assert!(codec.encode_start(&ops, &bad).result().is_none());
    }

    #[test]
    fn codec_requires_all_fields() {
        // Every field is required in Java (`fieldOf` on all three): an empty
        // map and each single-field omission must error. Mirrors the
        // `disk_configuration::tests::codec_requires_all_fields` precedent.
        let codec = nether_forest_vegetation_config_codec::<TestOps>();
        let ops = ops();
        // Empty map.
        assert!(codec.parse(&ops, &json!({})).is_error());
        // Missing `state_provider` (the first field).
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "spread_width": 8,
                        "spread_height": 4,
                    })
                )
                .is_error()
        );
        // Missing `spread_width` (the middle field).
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "state_provider": {
                            "type": "minecraft:simple_state_provider",
                            "state": {"Name": "minecraft:warped_roots"},
                        },
                        "spread_height": 4,
                    })
                )
                .is_error()
        );
        // Missing `spread_height` (the last field).
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "state_provider": {
                            "type": "minecraft:simple_state_provider",
                            "state": {"Name": "minecraft:warped_roots"},
                        },
                        "spread_width": 8,
                    })
                )
                .is_error()
        );
    }
}
