//! Port of `net.minecraft.world.level.levelgen.feature.HugeFungusConfiguration` (26.2).
//!
//! Holds the six configuration fields for `HugeFungusFeature` and its DFU codec:
//! `valid_base_block`, `stem_state`, `hat_state`, `decor_state`,
//! `replaceable_blocks`, and optional `planted` (default `false`).

use crate::levelgen::blockpredicates::block_predicate::{BlockPredicate, block_predicate_codec};
use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.HugeFungusConfiguration`.
#[derive(Debug, Clone)]
pub struct HugeFungusConfiguration {
    /// `validBaseState` — JSON field `"valid_base_block"`.
    pub valid_base_state: BlockState,
    /// `stemState` — JSON field `"stem_state"`.
    pub stem_state: BlockState,
    /// `hatState` — JSON field `"hat_state"`.
    pub hat_state: BlockState,
    /// `decorState` — JSON field `"decor_state"`.
    pub decor_state: BlockState,
    /// `replaceableBlocks` — JSON field `"replaceable_blocks"`.
    pub replaceable_blocks: Arc<dyn BlockPredicate>,
    /// `planted` — optional JSON field `"planted"`, defaults to `false`.
    pub planted: bool,
}

impl HugeFungusConfiguration {
    /// `new HugeFungusConfiguration(BlockState, BlockState, BlockState, BlockState, BlockPredicate, boolean)`.
    pub fn new(
        valid_base_state: BlockState,
        stem_state: BlockState,
        hat_state: BlockState,
        decor_state: BlockState,
        replaceable_blocks: Arc<dyn BlockPredicate>,
        planted: bool,
    ) -> Self {
        Self {
            valid_base_state,
            stem_state,
            hat_state,
            decor_state,
            replaceable_blocks,
            planted,
        }
    }
}

impl FeatureConfiguration for HugeFungusConfiguration {}

/// `HugeFungusConfiguration.CODEC`.
pub fn huge_fungus_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HugeFungusConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|c: &HugeFungusConfiguration| c.valid_base_state),
                "valid_base_block".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &HugeFungusConfiguration| c.stem_state),
                "stem_state".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &HugeFungusConfiguration| c.hat_state),
                "hat_state".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &HugeFungusConfiguration| c.decor_state),
                "decor_state".to_string(),
                block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &HugeFungusConfiguration| c.replaceable_blocks.clone()),
                "replaceable_blocks".to_string(),
                block_predicate_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &HugeFungusConfiguration| c.planted),
                codec::optional_field_of::<bool, Ops>("planted", codec::bool_codec::<Ops>(), false),
            ))
            .apply(
                instance,
                Arc::new(
                    |valid_base_state: BlockState,
                     stem_state: BlockState,
                     hat_state: BlockState,
                     decor_state: BlockState,
                     replaceable_blocks: Arc<dyn BlockPredicate>,
                     planted: bool| {
                        HugeFungusConfiguration::new(
                            valid_base_state,
                            stem_state,
                            hat_state,
                            decor_state,
                            replaceable_blocks,
                            planted,
                        )
                    },
                ),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::always_true;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn block_id(name: &str) -> BlockId {
        BlockId::from_name(name).expect("generated block name resolves")
    }

    fn state(name: &str) -> BlockState {
        BlockState::of(block_id(name))
    }

    #[test]
    fn codec_defaults_planted_to_false_and_round_trips() {
        let codec = huge_fungus_configuration_codec::<TestOps>();
        let ops = ops();
        let input = json!({
            "valid_base_block": { "Name": "minecraft:crimson_nylium" },
            "stem_state": { "Name": "minecraft:crimson_stem" },
            "hat_state": { "Name": "minecraft:nether_wart_block" },
            "decor_state": { "Name": "minecraft:shroomlight" },
            "replaceable_blocks": { "type": "minecraft:true" }
        });

        let parse_result = codec.parse(&ops, &input);
        let parsed = parse_result.result().unwrap();
        assert_eq!(
            parsed.valid_base_state.block(),
            block_id("minecraft:crimson_nylium")
        );
        assert_eq!(
            parsed.stem_state.block(),
            block_id("minecraft:crimson_stem")
        );
        assert_eq!(
            parsed.hat_state.block(),
            block_id("minecraft:nether_wart_block")
        );
        assert_eq!(
            parsed.decor_state.block(),
            block_id("minecraft:shroomlight")
        );
        assert!(!parsed.planted);

        let planted_cfg = HugeFungusConfiguration::new(
            state("minecraft:warped_nylium"),
            state("minecraft:warped_stem"),
            state("minecraft:warped_wart_block"),
            state("minecraft:shroomlight"),
            always_true(),
            true,
        );
        let encode_result = codec.encode_start(&ops, &planted_cfg);
        let encoded = encode_result.result().unwrap();
        assert_eq!(encoded.get("planted"), Some(&json!(true)));
    }
}
