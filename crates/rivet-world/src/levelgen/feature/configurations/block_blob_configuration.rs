//! Port of `net.minecraft.world.level.levelgen.feature.configurations.BlockBlobConfiguration`
//! (record, 26.2).
//!
//! Java: a record `record BlockBlobConfiguration(BlockState state,
//! BlockPredicate canPlaceOn)` whose `CODEC` is a `RecordCodecBuilder` over the
//! required `"state"` (`BlockState.CODEC`) and `"can_place_on"`
//! (`BlockPredicate.CODEC`) fields. The predicate is the erased
//! `Arc<dyn BlockPredicate>` carrier (the `#399` dispatch surface); DFU
//! `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is
//! exposed as the ops-generic `block_blob_configuration_codec::<Ops>()`
//! factory. The `state` half is value-semantic; the predicate half is compared
//! by behavior (its `type_id`), matching the erased predicate carrier.

use crate::levelgen::blockpredicates::{BlockPredicate, block_predicate_codec};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.BlockBlobConfiguration`.
///
/// The `BlockPredicate` is held as the erased `Arc<dyn BlockPredicate>` carrier
/// and the trait does not extend `PartialEq` (predicates are behavior, not
/// values), so the configuration is `Clone`+`Debug` only — no `PartialEq`.
#[derive(Debug, Clone)]
pub struct BlockBlobConfiguration {
    /// `state` — the blob's block state.
    pub state: BlockState,
    /// `canPlaceOn` — the predicate for blocks the blob may rest on.
    pub can_place_on: Arc<dyn BlockPredicate>,
}

impl BlockBlobConfiguration {
    /// `new BlockBlobConfiguration(BlockState, BlockPredicate)` — the record
    /// constructor (the codec's `apply` function).
    pub fn new(state: BlockState, can_place_on: Arc<dyn BlockPredicate>) -> Self {
        BlockBlobConfiguration {
            state,
            can_place_on,
        }
    }

    /// `BlockBlobConfiguration.state()`.
    pub fn state(&self) -> BlockState {
        self.state
    }

    /// `BlockBlobConfiguration.canPlaceOn()`.
    pub fn can_place_on(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_place_on
    }
}

/// `BlockBlobConfiguration.CODEC` — a record codec over the required `"state"`
/// and `"can_place_on"` fields, as the ops-generic
/// `block_blob_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockState.CODEC.fieldOf("state"),
///     BlockPredicate.CODEC.fieldOf("can_place_on"))
///     .apply(i, BlockBlobConfiguration::new))
/// ```
pub fn block_blob_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::codec::Codec<BlockBlobConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &BlockBlobConfiguration| c.state),
                rivet_serialization::codec::field_of(
                    block_state_codec::<Ops>(),
                    "state".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &BlockBlobConfiguration| c.can_place_on.clone()),
                rivet_serialization::codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_place_on".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(|state: BlockState, can_place_on: Arc<dyn BlockPredicate>| {
                    BlockBlobConfiguration::new(state, can_place_on)
                }),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for BlockBlobConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// `block_predicate_codec` dispatches over the registry-backed matching
    /// predicates, so the codec requires `RegistryOps` (the `RegistryOpsLookup`
    /// ops). An empty access is enough — the predicates here are `always_true`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    #[test]
    fn codec_round_trip() {
        let codec = block_blob_configuration_codec::<TestOps>();
        let ops = ops();
        let config = BlockBlobConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            crate::levelgen::blockpredicates::always_true(),
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state": {"Name": "minecraft:stone"},
                "can_place_on": {"type": "minecraft:true"},
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.state(), config.state());
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_place_on()),
            BlockPredicate::type_id(&**config.can_place_on())
        );
    }

    #[test]
    fn codec_requires_both_fields() {
        let codec = block_blob_configuration_codec::<TestOps>();
        let ops = ops();
        assert!(codec.parse(&ops, &json!({})).is_error());
        assert!(
            codec
                .parse(&ops, &json!({"state": {"Name": "minecraft:stone"}}))
                .is_error()
        );
    }
}
