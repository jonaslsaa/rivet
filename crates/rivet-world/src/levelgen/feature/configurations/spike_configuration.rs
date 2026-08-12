//! Port of `net.minecraft.world.level.levelgen.feature.configurations.SpikeConfiguration`
//! (record, 26.2).
//!
//! Java: a record `record SpikeConfiguration(BlockState state,
//! BlockPredicate canPlaceOn, BlockPredicate canReplace)` whose `CODEC` is a
//! `RecordCodecBuilder` over the required `"state"` (`BlockState.CODEC`),
//! `"can_place_on"` and `"can_replace"` (`BlockPredicate.CODEC`) fields. The
//! predicates are the erased `Arc<dyn BlockPredicate>` carrier (the `#399`
//! dispatch surface); DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the
//! static Java constant is exposed as the ops-generic
//! `spike_configuration_codec::<Ops>()` factory. The `state` half is
//! value-semantic; the predicate halves are compared by behavior (their
//! `type_id`), matching the erased predicate carrier.

use crate::levelgen::blockpredicates::{BlockPredicate, block_predicate_codec};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SpikeConfiguration`.
///
/// The `BlockPredicate` fields are held as the erased `Arc<dyn BlockPredicate>`
/// carrier and the trait does not extend `PartialEq` (predicates are behavior,
/// not values), so the configuration is `Clone`+`Debug` only — no `PartialEq`.
#[derive(Debug, Clone)]
pub struct SpikeConfiguration {
    /// `state` — the spike's block state.
    pub state: BlockState,
    /// `canPlaceOn` — the predicate for blocks the spike may grow on.
    pub can_place_on: Arc<dyn BlockPredicate>,
    /// `canReplace` — the predicate for blocks the spike may replace.
    pub can_replace: Arc<dyn BlockPredicate>,
}

impl SpikeConfiguration {
    /// `new SpikeConfiguration(BlockState, BlockPredicate, BlockPredicate)` —
    /// the record constructor (the codec's `apply` function).
    pub fn new(
        state: BlockState,
        can_place_on: Arc<dyn BlockPredicate>,
        can_replace: Arc<dyn BlockPredicate>,
    ) -> Self {
        SpikeConfiguration {
            state,
            can_place_on,
            can_replace,
        }
    }

    /// `SpikeConfiguration.state()`.
    pub fn state(&self) -> BlockState {
        self.state
    }

    /// `SpikeConfiguration.canPlaceOn()`.
    pub fn can_place_on(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_place_on
    }

    /// `SpikeConfiguration.canReplace()`.
    pub fn can_replace(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_replace
    }
}

/// `SpikeConfiguration.CODEC` — a record codec over the required `"state"`,
/// `"can_place_on"`, and `"can_replace"` fields, as the ops-generic
/// `spike_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockState.CODEC.fieldOf("state"),
///     BlockPredicate.CODEC.fieldOf("can_place_on"),
///     BlockPredicate.CODEC.fieldOf("can_replace"))
///     .apply(i, SpikeConfiguration::new))
/// ```
pub fn spike_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SpikeConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &SpikeConfiguration| c.state),
                rivet_serialization::codec::field_of(
                    block_state_codec::<Ops>(),
                    "state".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpikeConfiguration| c.can_place_on.clone()),
                rivet_serialization::codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_place_on".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpikeConfiguration| c.can_replace.clone()),
                rivet_serialization::codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_replace".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |state: BlockState,
                     can_place_on: Arc<dyn BlockPredicate>,
                     can_replace: Arc<dyn BlockPredicate>| {
                        SpikeConfiguration::new(state, can_place_on, can_replace)
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for SpikeConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trip() {
        let codec = spike_configuration_codec::<JsonOps>();
        let config = SpikeConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            crate::levelgen::blockpredicates::always_true(),
            crate::levelgen::blockpredicates::always_true(),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state": {"Name": "minecraft:stone"},
                "can_place_on": {"type": "minecraft:true"},
                "can_replace": {"type": "minecraft:true"},
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.state(), config.state());
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_place_on()),
            BlockPredicate::type_id(&**config.can_place_on())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_replace()),
            BlockPredicate::type_id(&**config.can_replace())
        );
    }

    #[test]
    fn codec_requires_all_three_fields() {
        let codec = spike_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "state": {"Name": "minecraft:stone"},
                        "can_place_on": {"type": "minecraft:true"},
                    })
                )
                .is_error()
        );
    }
}
