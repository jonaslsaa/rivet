//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! SimpleStateProvider` (class, 26.2).
//!
//! Java: a provider that always returns a fixed `BlockState`. `type()` is
//! `BlockStateProviderType.SIMPLE_STATE_PROVIDER`.
//!
//! `CODEC` is a record codec over the `"state"` field,
//! `BlockState.CODEC.fieldOf("state").xmap(SimpleStateProvider::new, p ->
//! p.state)`. The constructor is `protected` in Java (the `BlockStateProvider
//! .simple(BlockState)` static factory is the public entry), mirrored by the
//! `pub(crate)` `new`.

use rivet_registry::block_state::BlockState;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::BlockStateProvider;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.SimpleStateProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleStateProvider {
    /// `this.state`.
    state: BlockState,
}

impl SimpleStateProvider {
    /// `SimpleStateProvider(BlockState)` — the protected constructor, exposed
    /// for `BlockStateProvider.simple(BlockState)`.
    pub(crate) fn new(state: BlockState) -> SimpleStateProvider {
        SimpleStateProvider { state }
    }

    /// `this.state`.
    pub fn state(&self) -> BlockState {
        self.state
    }
}

impl BlockStateProvider for SimpleStateProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        _random: &mut R,
        _pos: &BlockPos,
    ) -> BlockState {
        self.state
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::SIMPLE_STATE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `SimpleStateProvider.CODEC` — `BlockState.CODEC.fieldOf("state").xmap(...)`,
/// as the ops-generic `simple_state_provider_map_codec::<Ops>()` factory.
pub fn simple_state_provider_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SimpleStateProvider, Ops>> {
    map_codec::xmap(
        codec::field_of(
            rivet_registry::block_state_codec::block_state_codec::<Ops>(),
            "state".to_string(),
        ),
        Arc::new(|s: &BlockState| SimpleStateProvider::new(*s)),
        Arc::new(|p: &SimpleStateProvider| p.state),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trips_the_state() {
        let codec = map_codec::codec_of(simple_state_provider_map_codec::<JsonOps>());
        // `BlockState.CODEC` is the `"Name"` dispatch codec; stone is a
        // singleton state (no properties), so it serializes as name-only.
        let input = json!({"state": {"Name": "minecraft:stone"}});
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            decoded.state(),
            BlockState::of(rivet_registry::generated::blocks::BlockId::from_id(1))
        );
        assert_eq!(
            decoded.type_id(),
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_missing_state_field() {
        let codec = map_codec::codec_of(simple_state_provider_map_codec::<JsonOps>());
        let input = json!({});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("No key state"), "got: {msg}");
    }
}
