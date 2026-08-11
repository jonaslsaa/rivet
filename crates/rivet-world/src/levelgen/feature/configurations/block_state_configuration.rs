//! Port of `net.minecraft.world.level.levelgen.feature.configurations.BlockStateConfiguration`
//! (class, 26.2).
//!
//! Java: a single-field value class wrapping the `BlockState`; its `CODEC` is
//! `BlockState.CODEC.fieldOf("state").xmap(BlockStateConfiguration::new,
//! c -> c.state).codec()` — the `"state"` field (a required `BlockState`, the
//! #391 `"Name"`-dispatch codec) mapped onto the wrapper value type. DFU
//! `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is
//! exposed as the ops-generic `block_state_configuration_codec::<Ops>()`
//! factory. Equality is value-semantic (`PartialEq` on the wrapped state),
//! mirroring the leaf's single-field shape.

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.BlockStateConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStateConfiguration {
    /// `state` — the configured block state.
    pub state: BlockState,
}

impl BlockStateConfiguration {
    /// `new BlockStateConfiguration(BlockState)`.
    pub fn new(state: BlockState) -> Self {
        BlockStateConfiguration { state }
    }
}

/// `BlockStateConfiguration.CODEC` — `BlockState.CODEC` as the required
/// `"state"` field, mapped onto the wrapper, as the ops-generic
/// `block_state_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// BlockState.CODEC.fieldOf("state").xmap(BlockStateConfiguration::new, c -> c.state).codec()
/// ```
pub fn block_state_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BlockStateConfiguration, Ops>> {
    let state_field: Arc<dyn MapCodec<BlockState, Ops>> =
        codec::field_of(block_state_codec::<Ops>(), "state".to_string());
    map_codec::codec_of(map_codec::xmap(
        state_field,
        Arc::new(|state: &BlockState| BlockStateConfiguration::new(*state)),
        Arc::new(|c: &BlockStateConfiguration| c.state),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for BlockStateConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn oak_log() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap())
    }

    #[test]
    fn codec_round_trip_singleton_state() {
        // A singleton state (no properties) encodes as just the name.
        let codec = block_state_configuration_codec::<JsonOps>();
        let config = BlockStateConfiguration::new(BlockState::of(
            BlockId::from_name("minecraft:stone").unwrap(),
        ));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"state": {"Name": "minecraft:stone"}}));
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_round_trip_state_with_properties() {
        // oak_log's default state carries its axis property (the default axis
        // is "y" in the generated table); the "state" field is required.
        let codec = block_state_configuration_codec::<JsonOps>();
        let config = BlockStateConfiguration::new(oak_log());
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"state": {"Name": "minecraft:oak_log", "Properties": {"axis": "y"}}})
        );
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_the_state_field() {
        let codec = block_state_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
    }

    #[test]
    fn codec_rejects_unknown_block_name() {
        // The "Name" dispatch's unknown-key error propagates through the field.
        let codec = block_state_configuration_codec::<JsonOps>();
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({"state": {"Name": "minecraft:no_such_block"}})
                )
                .is_error()
        );
    }
}
