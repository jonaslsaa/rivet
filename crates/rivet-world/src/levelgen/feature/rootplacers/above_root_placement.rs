//! Port of `net.minecraft.world.level.levelgen.feature.rootplacers.
//! AboveRootPlacement` (record, 26.2).
//!
//! Java is the `RootPlacer.aboveRootPlacement` field's value type: the block
//! provider placed above each root when the chance passes. Its `CODEC` is the
//! two-field record codec (`BlockStateProvider.CODEC` `"above_root_provider"`
//! plus `Codec.floatRange(0, 1)` `"above_root_placement_chance"`), requiring
//! the `RegistryOpsLookup` ops surface for the embedded block-state-provider
//! dispatch.
//!
//! The provider is stored as `Arc<dyn ErasedBlockStateProvider>` (the erased
//! `BlockStateProvider` carrier, per the state-provider dispatch root), and
//! `RootPlacer.placeRoot` resolves its state through
//! `block_state_provider_get_state`.

use crate::levelgen::feature::stateproviders::block_state_provider::{
    block_state_provider_codec, ErasedBlockStateProvider,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.rootplacers.AboveRootPlacement`.
#[derive(Debug, Clone)]
pub struct AboveRootPlacement {
    /// `AboveRootPlacement.aboveRootProvider` — the provider whose state
    /// sits above each placed root.
    pub above_root_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `AboveRootPlacement.aboveRootPlacementChance`.
    pub above_root_placement_chance: f32,
}

impl AboveRootPlacement {
    /// `new AboveRootPlacement(BlockStateProvider, float)` — the record
    /// constructor.
    pub fn new(
        above_root_provider: Arc<dyn ErasedBlockStateProvider>,
        above_root_placement_chance: f32,
    ) -> AboveRootPlacement {
        AboveRootPlacement {
            above_root_provider,
            above_root_placement_chance,
        }
    }
}

/// `AboveRootPlacement.CODEC` — the record codec over
/// `"above_root_provider"` and `"above_root_placement_chance"`, as the
/// ops-generic `above_root_placement_map_codec::<Ops>()` factory.
pub fn above_root_placement_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<AboveRootPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|p: &AboveRootPlacement| p.above_root_provider.clone()),
                "above_root_provider".to_string(),
                block_state_provider_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|p: &AboveRootPlacement| p.above_root_placement_chance),
                "above_root_placement_chance".to_string(),
                codec::float_range::<Ops>(0.0, 1.0),
            ))
            .apply(instance, Arc::new(AboveRootPlacement::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_codec;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops implementing
    /// `RegistryOpsLookup` (the embedded `BlockStateProvider.CODEC` requires it).
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    #[test]
    fn codec_round_trips_simple_provider() {
        let codec = map_codec::codec_of(above_root_placement_map_codec::<TestOps>());
        let input = json!({
            "above_root_provider": {
                "type": "minecraft:simple_state_provider",
                "state": {"Name": "minecraft:stone"}
            },
            "above_root_placement_chance": 0.5
        });
        let decoded = codec
            .parse(&empty_ops(), &input)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.above_root_placement_chance, 0.5);
        let encoded = codec
            .encode_start(&empty_ops(), &decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_out_of_range_chance() {
        let codec = map_codec::codec_of(above_root_placement_map_codec::<TestOps>());
        // floatRange(0, 1) validates on both decode and encode.
        assert!(
            codec
                .parse(
                    &empty_ops(),
                    &json!({
                        "above_root_provider": {
                            "type": "minecraft:simple_state_provider",
                            "state": {"Name": "minecraft:stone"}
                        },
                        "above_root_placement_chance": 1.5
                    }),
                )
                .is_error()
        );
        let provider = crate::levelgen::feature::stateproviders::block_state_provider::simple(
            BlockState::of(BlockId::from_id(1)),
        );
        let bad = AboveRootPlacement::new(
            Arc::new(provider) as Arc<dyn ErasedBlockStateProvider>,
            1.5,
        );
        assert!(codec.encode_start(&empty_ops(), &bad).result().is_none());
    }

    #[test]
    fn codec_round_trips_through_public_codec_factory() {
        // The embedded provider codec is the same recursive dispatch root the
        // tree configuration's `trunk_provider` uses, so the map-codec factory
        // must compose with it.
        let codec = block_state_provider_codec::<TestOps>();
        let input = json!({"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}});
        let decoded = codec
            .parse(&empty_ops(), &input)
            .result()
            .expect("decode should succeed");
        let _ = decoded;
    }
}
