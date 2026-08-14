//! Port of `net.minecraft.world.level.levelgen.feature.configurations.ReplaceBlockConfiguration`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.configurations.replace_block`
//! manifest unit.
//!
//! Java: a value class holding `List<OreConfiguration.TargetBlockState> targetStates`
//! whose `CODEC` is a `RecordCodecBuilder` over the required `"targets"` field
//! (`Codec.list(OreConfiguration.TargetBlockState.CODEC)`). The two-arg
//! convenience constructor wraps a single `OreConfiguration.target(new
//! BlockStateMatchTest(targetState), state)` pair — ported here as
//! [`ReplaceBlockConfiguration::new_target_state`]. (NOT
//! `OreConfiguration::new_single_target`, which ports the different
//! `OreConfiguration(RuleTest, BlockState, int, float)` constructor.)

use crate::levelgen::feature::configurations::ore_configuration::OreConfiguration;
use crate::levelgen::feature::configurations::ore_configuration::TargetBlockState;
use crate::levelgen::feature::configurations::ore_configuration::target_block_state_codec;
use crate::levelgen::structure::templatesystem::block_state_match_test::BlockStateMatchTest;
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.ReplaceBlockConfiguration`.
///
/// The `targetStates` list holds the erased `Arc<dyn ErasedRuleTest>` carrier
/// (rule tests are behavior, not values), so the configuration derives
/// `Clone`+`Debug` only — the same shape as `OreConfiguration`.
#[derive(Debug, Clone)]
pub struct ReplaceBlockConfiguration {
    /// `targetStates` — the per-target rule test + replacement state pairs.
    pub target_states: Vec<TargetBlockState>,
}

impl ReplaceBlockConfiguration {
    /// `new ReplaceBlockConfiguration(List<TargetBlockState>)` — the
    /// list constructor (the codec's `apply` function).
    pub fn new(target_states: Vec<TargetBlockState>) -> Self {
        ReplaceBlockConfiguration { target_states }
    }

    /// `new ReplaceBlockConfiguration(BlockState targetState, BlockState state)` —
    /// the two-arg convenience constructor wrapping a single
    /// `OreConfiguration.target(new BlockStateMatchTest(targetState), state)` pair
    /// (`this(ImmutableList.of(...))`).
    pub fn new_target_state(target_state: BlockState, state: BlockState) -> Self {
        ReplaceBlockConfiguration::new(vec![OreConfiguration::target(
            Arc::new(BlockStateMatchTest::new(target_state)),
            state,
        )])
    }
}

/// `ReplaceBlockConfiguration.CODEC` — the ops-generic
/// `replace_block_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Codec.list(OreConfiguration.TargetBlockState.CODEC).fieldOf("targets"))
///     .apply(i, ReplaceBlockConfiguration::new))
/// ```
pub fn replace_block_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ReplaceBlockConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ReplaceBlockConfiguration| c.target_states.clone()),
                codec::field_of(
                    codec::list(target_block_state_codec::<Ops>()),
                    "targets".to_string(),
                ),
            ))
            .apply(instance, Arc::new(ReplaceBlockConfiguration::new))
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for ReplaceBlockConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::configurations::ore_configuration::OreConfiguration;
    use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
    use crate::levelgen::structure::templatesystem::rule_test::ErasedRuleTest;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;
    use std::sync::Arc;

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:air").unwrap())
    }

    fn always_true() -> Arc<dyn ErasedRuleTest> {
        Arc::new(AlwaysTrueTest)
    }

    #[test]
    fn codec_round_trip() {
        let codec = replace_block_configuration_codec::<JsonOps>();
        let config =
            ReplaceBlockConfiguration::new(vec![OreConfiguration::target(always_true(), air())]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "targets": [{
                    "target": {"predicate_type": "minecraft:always_true"},
                    "state": {"Name": "minecraft:air"}
                }]
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.target_states.len(), 1);
        assert_eq!(decoded.target_states[0].state, air());
    }

    #[test]
    fn codec_requires_targets_field() {
        let codec = replace_block_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
    }

    #[test]
    fn codec_accepts_empty_targets() {
        // Java has no `nonEmptyList` on `targets` — an empty list decodes fine.
        let codec = replace_block_configuration_codec::<JsonOps>();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!({"targets": []}))
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(decoded.target_states.is_empty());
    }

    #[test]
    fn single_target_constructor_uses_block_state_match_test() {
        // The two-arg `(BlockState, BlockState)` convenience constructor wraps a
        // `BlockStateMatchTest(targetState)` — pin the JSON dispatch.
        let target_state = stone();
        let state = air();
        let config = ReplaceBlockConfiguration::new_target_state(target_state, state);
        let encoded = replace_block_configuration_codec::<JsonOps>()
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "targets": [{
                    "target": {
                        "predicate_type": "minecraft:blockstate_match",
                        "block_state": {"Name": "minecraft:stone"}
                    },
                    "state": {"Name": "minecraft:air"}
                }]
            })
        );
    }
}
