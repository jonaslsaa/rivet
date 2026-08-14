//! Port of `net.minecraft.world.level.levelgen.feature.configurations.OreConfiguration`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.configurations.ore`
//! manifest unit.
//!
//! Java: a value class holding `List<TargetBlockState> targetStates`, `int
//! size` (`Codec.intRange(0, 64)`) and `float discardChanceOnAirExposure`
//! (`Codec.floatRange(0.0F, 1.0F)`), whose `CODEC` is a `RecordCodecBuilder`
//! over the required `"targets"`, `"size"` and `"discard_chance_on_air_exposure"`
//! fields. The nested `OreConfiguration.TargetBlockState` record pairs a
//! `RuleTest target` (`RuleTest.CODEC` — the `"predicate_type"` by-name
//! dispatch) with a `BlockState state` (`BlockState.CODEC`).
//!
//! This unit ports the value layer only. The placement behavior
//! (`OreFeature`/`ScatteredOreFeature`) writes blocks through
//! `WorldGenLevel.setBlock`/`getBlockState`, whose seams are not reachable on
//! the `WorldGenLevel` surface yet (RivetTodo #228/#399) — those defer. Of the
//! two pure `OreFeature` helpers (see `crate::levelgen::feature::ore_feature`),
//! `shouldSkipAirCheck` IS ported; `canPlaceOre` DEFERS because its first
//! conjunct evaluates the erased `RuleTest` (no object-safe `test` exists on
//! the `ErasedRuleTest` carrier).

use crate::levelgen::structure::templatesystem::rule_test::ErasedRuleTest;
use crate::levelgen::structure::templatesystem::rule_test::rule_test_codec;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.OreConfiguration`.
///
/// The `targetStates` list holds the erased `Arc<dyn ErasedRuleTest>` carrier
/// (rule tests are behavior, not values), so the configuration derives
/// `Clone`+`Debug` only — no `PartialEq`, the same shape the other
/// provider/predicate-carrying configuration value types take.
#[derive(Debug, Clone)]
pub struct OreConfiguration {
    /// `targetStates` — the per-target rule test + replacement state pairs.
    pub target_states: Vec<TargetBlockState>,
    /// `size` — `[0, 64]`.
    pub size: i32,
    /// `discardChanceOnAirExposure` — `[0.0F, 1.0F]`.
    pub discard_chance_on_air_exposure: f32,
}

impl OreConfiguration {
    /// `new OreConfiguration(List<TargetBlockState>, int, float)` — the
    /// three-arg constructor (the codec's `apply` function).
    pub fn new(
        target_states: Vec<TargetBlockState>,
        size: i32,
        discard_chance_on_air_exposure: f32,
    ) -> Self {
        OreConfiguration {
            target_states,
            size,
            discard_chance_on_air_exposure,
        }
    }

    /// `new OreConfiguration(List<TargetBlockState>, int)` — the two-arg
    /// constructor (`discardChanceOnAirExposure = 0.0F`).
    pub fn new_without_discard_chance(target_states: Vec<TargetBlockState>, size: i32) -> Self {
        Self::new(target_states, size, 0.0)
    }

    /// `new OreConfiguration(RuleTest, BlockState, int, float)`.
    pub fn new_single_target(
        target: Arc<dyn ErasedRuleTest>,
        state: BlockState,
        size: i32,
        discard_chance_on_air_exposure: f32,
    ) -> Self {
        Self::new(
            vec![TargetBlockState::new(target, state)],
            size,
            discard_chance_on_air_exposure,
        )
    }

    /// `new OreConfiguration(RuleTest, BlockState, int)`.
    pub fn new_single_target_without_discard_chance(
        target: Arc<dyn ErasedRuleTest>,
        state: BlockState,
        size: i32,
    ) -> Self {
        Self::new_single_target(target, state, size, 0.0)
    }

    /// `OreConfiguration.target(RuleTest, BlockState)` — the static helper
    /// wrapping a single target pair.
    pub fn target(rule: Arc<dyn ErasedRuleTest>, state: BlockState) -> TargetBlockState {
        TargetBlockState::new(rule, state)
    }
}

/// `OreConfiguration.TargetBlockState` — the nested record pairing a `RuleTest`
/// with the `BlockState` it replaces with.
#[derive(Debug, Clone)]
pub struct TargetBlockState {
    /// `target` — the rule test, erased.
    pub target: Arc<dyn ErasedRuleTest>,
    /// `state` — the replacement block state.
    pub state: BlockState,
}

impl TargetBlockState {
    /// `new TargetBlockState(RuleTest, BlockState)` — the private record
    /// constructor (the codec's `apply` function).
    pub fn new(target: Arc<dyn ErasedRuleTest>, state: BlockState) -> Self {
        TargetBlockState { target, state }
    }
}

/// `OreConfiguration.TargetBlockState.CODEC` — the ops-generic
/// `target_block_state_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     RuleTest.CODEC.fieldOf("target"),
///     BlockState.CODEC.fieldOf("state"))
///     .apply(i, TargetBlockState::new))
/// ```
pub fn target_block_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TargetBlockState, Ops>>
{
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &TargetBlockState| t.target.clone()),
                codec::field_of(rule_test_codec::<Ops>(), "target".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TargetBlockState| t.state),
                codec::field_of(block_state_codec::<Ops>(), "state".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|target: Arc<dyn ErasedRuleTest>, state: BlockState| {
                    TargetBlockState::new(target, state)
                }),
            )
    })
}

/// `OreConfiguration.CODEC` — the ops-generic `ore_configuration_codec::<Ops>()`
/// factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Codec.list(TargetBlockState.CODEC).fieldOf("targets"),
///     Codec.intRange(0, 64).fieldOf("size"),
///     Codec.floatRange(0.0F, 1.0F).fieldOf("discard_chance_on_air_exposure"))
///     .apply(i, OreConfiguration::new))
/// ```
pub fn ore_configuration_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<OreConfiguration, Ops>>
{
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &OreConfiguration| c.target_states.clone()),
                codec::field_of(
                    codec::list(target_block_state_codec::<Ops>()),
                    "targets".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &OreConfiguration| c.size),
                codec::field_of(codec::int_range::<Ops>(0, 64), "size".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &OreConfiguration| c.discard_chance_on_air_exposure),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "discard_chance_on_air_exposure".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(|targets: Vec<TargetBlockState>, size: i32, discard: f32| {
                    OreConfiguration::new(targets, size, discard)
                }),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for OreConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
    use crate::levelgen::structure::templatesystem::block_state_match_test::BlockStateMatchTest;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:air").unwrap())
    }

    fn always_true() -> Arc<dyn ErasedRuleTest> {
        Arc::new(AlwaysTrueTest)
    }

    /// A `TargetBlockState` whose `"predicate_type": "minecraft:blockstate_match"`
    /// dispatch matches an exact `BlockState`.
    fn blockstate_match_target() -> Arc<dyn ErasedRuleTest> {
        Arc::new(BlockStateMatchTest::new(stone()))
    }

    #[test]
    fn codec_round_trip() {
        let codec = ore_configuration_codec::<JsonOps>();
        let config =
            OreConfiguration::new(vec![TargetBlockState::new(always_true(), air())], 9, 0.0);
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
                }],
                "size": 9,
                "discard_chance_on_air_exposure": 0.0
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.target_states.len(), 1);
        assert_eq!(decoded.size, 9);
        assert_eq!(decoded.discard_chance_on_air_exposure, 0.0);
    }

    #[test]
    fn target_block_state_codec_round_trip() {
        let codec = target_block_state_codec::<JsonOps>();
        let target = TargetBlockState::new(blockstate_match_target(), stone());
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &target)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "target": {
                    "predicate_type": "minecraft:blockstate_match",
                    "block_state": {"Name": "minecraft:stone"}
                },
                "state": {"Name": "minecraft:stone"}
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.state, stone());
    }

    #[test]
    fn codec_rejects_out_of_bounds() {
        let codec = ore_configuration_codec::<JsonOps>();
        // size above 64.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "targets": [],
                        "size": 65,
                        "discard_chance_on_air_exposure": 0.0
                    })
                )
                .is_error()
        );
        // discard chance above 1.0.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "targets": [],
                        "size": 4,
                        "discard_chance_on_air_exposure": 1.5
                    })
                )
                .is_error()
        );
        // discard chance below 0.0.
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "targets": [],
                        "size": 4,
                        "discard_chance_on_air_exposure": -0.1
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = ore_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!({"targets": [], "size": 4}))
                .is_error()
        );
    }

    #[test]
    fn constructors_set_the_fields() {
        let always = always_true();
        let config = OreConfiguration::new_single_target(always.clone(), air(), 3, 0.25);
        assert_eq!(config.target_states.len(), 1);
        assert_eq!(config.size, 3);
        assert_eq!(config.discard_chance_on_air_exposure, 0.25);

        let config2 = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(always.clone(), air())],
            2,
        );
        assert_eq!(config2.discard_chance_on_air_exposure, 0.0);

        let config3 =
            OreConfiguration::new_single_target_without_discard_chance(always.clone(), air(), 2);
        assert_eq!(config3.size, 2);
        assert_eq!(config3.discard_chance_on_air_exposure, 0.0);

        let t = OreConfiguration::target(always, air());
        assert_eq!(t.state, air());
    }
}
