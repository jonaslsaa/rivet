//! Port of `net.minecraft.world.level.levelgen.feature.configurations.BlockPileConfiguration`
//! (class, 26.2).
//!
//! Java: a single-field value class wrapping the `BlockStateProvider`; its
//! `CODEC` is `BlockStateProvider.CODEC.fieldOf("state_provider").xmap(
//! BlockPileConfiguration::new, c -> c.stateProvider).codec()` — the
//! `"state_provider"` field (the required `BlockStateProvider` dispatch codec,
//! the `#181` by-name dispatch) mapped onto the wrapper value type. The
//! provider half is the erased `Arc<dyn ErasedBlockStateProvider>` carrier (the
//! `#181` dispatch surface), so the configuration is `Clone`+`Debug` only — no
//! `PartialEq`, matching `BlockBlobConfiguration`. DFU `Codec<T>` is
//! `Codec<E, Ops>` in the port, so the static Java constant is exposed as the
//! ops-generic `block_pile_configuration_codec::<Ops>()` factory, which
//! inherits the wrapped dispatch's `RegistryOpsLookup` ops requirement.

use crate::levelgen::feature::stateproviders::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.BlockPileConfiguration`.
#[derive(Debug, Clone)]
pub struct BlockPileConfiguration {
    /// `stateProvider` — the provider of the pile's block states.
    pub state_provider: Arc<dyn ErasedBlockStateProvider>,
}

impl BlockPileConfiguration {
    /// `new BlockPileConfiguration(BlockStateProvider)`.
    pub fn new(state_provider: Arc<dyn ErasedBlockStateProvider>) -> Self {
        BlockPileConfiguration { state_provider }
    }
}

/// `BlockPileConfiguration.CODEC` — `BlockStateProvider.CODEC` as the required
/// `"state_provider"` field, mapped onto the wrapper, as the ops-generic
/// `block_pile_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// BlockStateProvider.CODEC
///     .fieldOf("state_provider")
///     .xmap(BlockPileConfiguration::new, c -> c.stateProvider)
///     .codec()
/// ```
pub fn block_pile_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<BlockPileConfiguration, Ops>> {
    let state_provider_field: Arc<dyn MapCodec<Arc<dyn ErasedBlockStateProvider>, Ops>> =
        codec::field_of(
            block_state_provider_codec::<Ops>(),
            "state_provider".to_string(),
        );
    map_codec::codec_of(map_codec::xmap(
        state_provider_field,
        Arc::new(|sp: &Arc<dyn ErasedBlockStateProvider>| BlockPileConfiguration::new(sp.clone())),
        Arc::new(|c: &BlockPileConfiguration| c.state_provider.clone()),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for BlockPileConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::{
        BlockStateProviderTypes, ErasedBlockStateProvider,
    };
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// `block_state_provider_codec` dispatches over the by-name provider
    /// registry, so the codec requires `RegistryOps` (the `RegistryOpsLookup`
    /// ops). An empty access is enough — `SimpleStateProvider` only embeds a
    /// `BlockState`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn stone_provider() -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(crate::levelgen::feature::stateproviders::simple(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        ))
    }

    #[test]
    fn codec_round_trip() {
        let codec = block_pile_configuration_codec::<TestOps>();
        let ops = ops();
        let config = BlockPileConfiguration::new(stone_provider());
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state_provider": {
                    "state": {"Name": "minecraft:stone"},
                    "type": "minecraft:simple_state_provider",
                }
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            ErasedBlockStateProvider::type_id(&*decoded.state_provider),
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER
        );
        // The erased provider is behavior, not value — pin the round trip by
        // re-encoding rather than equality on the carrier.
        let reencoded = codec
            .encode_start(&ops, &decoded)
            .result()
            .expect("re-encode should succeed")
            .clone();
        assert_eq!(reencoded, encoded);
    }

    #[test]
    fn codec_requires_the_state_provider_field() {
        let codec = block_pile_configuration_codec::<TestOps>();
        let ops = ops();
        assert!(codec.parse(&ops, &json!({})).is_error());
        // A nested non-dispatch value is also rejected by the provider codec.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({"state_provider": {"state": "minecraft:stone"}})
                )
                .is_error()
        );
    }
}
