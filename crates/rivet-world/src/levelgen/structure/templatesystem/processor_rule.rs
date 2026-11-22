//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.ProcessorRule`
//! (MC 26.2).
//!
//! A single conditional replacement rule evaluated by `RuleProcessor`:
//! - `input_predicate` (`RuleTest`) tests the incoming template block state;
//! - `location_predicate` (`RuleTest`) tests the existing world block state at `world_pos`;
//! - `position_predicate` (`PosRuleTest`, default `PosAlwaysTrueTest`) tests the position triple;
//! - `output_state` (`BlockState`) is the replacement block state;
//! - `block_entity_modifier` (`RuleBlockEntityModifier`, default `Passthrough`) transforms NBT.

use super::axis_aligned_linear_pos_test::AxisAlignedLinearPosTest;
use super::linear_pos_test::LinearPosTest;
use super::pos_always_true_test::PosAlwaysTrueTest;
use super::pos_rule_test::{ErasedPosRuleTest, PosRuleTest, pos_rule_test_codec};
use super::pos_rule_test_type::PosRuleTestTypes;
use super::rule_block_entity_modifier::{
    ErasedRuleBlockEntityModifier, Passthrough, RuleBlockEntityModifierTypes, erased_apply,
    rule_block_entity_modifier_codec,
};
use super::rule_test::{ErasedRuleTest, erased_test, rule_test_codec};
use crate::level::world_gen_level::WorldGenLevel;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.ProcessorRule`.
#[derive(Clone, Debug)]
pub struct ProcessorRule {
    pub input_predicate: Arc<dyn ErasedRuleTest>,
    pub loc_predicate: Arc<dyn ErasedRuleTest>,
    pub pos_predicate: Arc<dyn ErasedPosRuleTest>,
    pub output_state: BlockState,
    pub block_entity_modifier: Arc<dyn ErasedRuleBlockEntityModifier>,
}

impl ProcessorRule {
    /// `new ProcessorRule(inputPredicate, locPredicate, outputState)`.
    pub fn new(
        input_predicate: Arc<dyn ErasedRuleTest>,
        loc_predicate: Arc<dyn ErasedRuleTest>,
        output_state: BlockState,
    ) -> Self {
        Self::with_pos(
            input_predicate,
            loc_predicate,
            Arc::new(PosAlwaysTrueTest),
            output_state,
        )
    }

    /// `new ProcessorRule(inputPredicate, locPredicate, posPredicate, outputState)`.
    pub fn with_pos(
        input_predicate: Arc<dyn ErasedRuleTest>,
        loc_predicate: Arc<dyn ErasedRuleTest>,
        pos_predicate: Arc<dyn ErasedPosRuleTest>,
        output_state: BlockState,
    ) -> Self {
        Self::with_modifier(
            input_predicate,
            loc_predicate,
            pos_predicate,
            output_state,
            Arc::new(Passthrough),
        )
    }

    /// Full constructor matching Paper's 5-argument `ProcessorRule` constructor.
    pub fn with_modifier(
        input_predicate: Arc<dyn ErasedRuleTest>,
        loc_predicate: Arc<dyn ErasedRuleTest>,
        pos_predicate: Arc<dyn ErasedPosRuleTest>,
        output_state: BlockState,
        block_entity_modifier: Arc<dyn ErasedRuleBlockEntityModifier>,
    ) -> Self {
        Self {
            input_predicate,
            loc_predicate,
            pos_predicate,
            output_state,
            block_entity_modifier,
        }
    }

    /// `ProcessorRule.test(LevelReader, BlockState, BlockPos, BlockPos, BlockPos, RandomSource)`.
    ///
    /// Preserves Java short-circuit evaluation order:
    /// `inputPredicate.test` -> `locPredicate.testAgainstWorldState` -> `posPredicate.test`.
    pub fn test<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        input_state: &BlockState,
        in_template_pos: &BlockPos,
        world_pos: &BlockPos,
        reference: &BlockPos,
        random: &mut R,
    ) -> bool {
        let world_state = level.get_block_state(world_pos);
        erased_test(&self.input_predicate, input_state, random)
            && erased_test(&self.loc_predicate, &world_state, random)
            && erased_pos_test(
                &self.pos_predicate,
                in_template_pos,
                world_pos,
                reference,
                random,
            )
    }

    /// `ProcessorRule.getOutputState()`.
    pub fn get_output_state(&self) -> BlockState {
        self.output_state
    }

    /// `ProcessorRule.getOutputTag(RandomSource, @Nullable CompoundTag)`.
    pub fn get_output_tag<R: RandomSource>(
        &self,
        random: &mut R,
        existing_tag: Option<&CompoundTag>,
    ) -> Option<CompoundTag> {
        erased_apply(&self.block_entity_modifier, random, existing_tag)
    }
}

/// Dispatch `PosRuleTest::test` on an erased position rule test.
pub fn erased_pos_test<R: RandomSource>(
    test: &Arc<dyn ErasedPosRuleTest>,
    in_template_pos: &BlockPos,
    world_pos: &BlockPos,
    reference: &BlockPos,
    random: &mut R,
) -> bool {
    let any = test.as_any();
    if let Some(t) = any.downcast_ref::<PosAlwaysTrueTest>() {
        t.test(in_template_pos, world_pos, reference, random)
    } else if let Some(t) = any.downcast_ref::<LinearPosTest>() {
        t.test(in_template_pos, world_pos, reference, random)
    } else if let Some(t) = any.downcast_ref::<AxisAlignedLinearPosTest>() {
        t.test(in_template_pos, world_pos, reference, random)
    } else {
        panic!(
            "unregistered PosRuleTest implementation: {:?}",
            ErasedPosRuleTest::type_id(&**test)
        )
    }
}

/// `ProcessorRule.CODEC`.
pub fn processor_rule_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ProcessorRule, Ops>> {
    let pos_field = map_codec::xmap(
        codec::optional_field(
            "position_predicate".to_string(),
            pos_rule_test_codec::<Ops>(),
            true,
        ),
        Arc::new(|opt: &Option<Arc<dyn ErasedPosRuleTest>>| {
            opt.clone().unwrap_or_else(|| Arc::new(PosAlwaysTrueTest))
        }),
        Arc::new(|v: &Arc<dyn ErasedPosRuleTest>| {
            if ErasedPosRuleTest::type_id(&**v) == PosRuleTestTypes::ALWAYS_TRUE_TEST {
                None
            } else {
                Some(Arc::clone(v))
            }
        }),
    );

    let mod_field = map_codec::xmap(
        codec::optional_field(
            "block_entity_modifier".to_string(),
            rule_block_entity_modifier_codec::<Ops>(),
            true,
        ),
        Arc::new(|opt: &Option<Arc<dyn ErasedRuleBlockEntityModifier>>| {
            opt.clone().unwrap_or_else(|| Arc::new(Passthrough))
        }),
        Arc::new(|v: &Arc<dyn ErasedRuleBlockEntityModifier>| {
            if ErasedRuleBlockEntityModifier::type_id(&**v)
                == RuleBlockEntityModifierTypes::PASSTHROUGH
            {
                None
            } else {
                Some(Arc::clone(v))
            }
        }),
    );

    let map = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|r: &ProcessorRule| Arc::clone(&r.input_predicate)),
                codec::field_of(rule_test_codec::<Ops>(), "input_predicate".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &ProcessorRule| Arc::clone(&r.loc_predicate)),
                codec::field_of(rule_test_codec::<Ops>(), "location_predicate".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &ProcessorRule| Arc::clone(&r.pos_predicate)),
                pos_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &ProcessorRule| r.output_state),
                codec::field_of(block_state_codec::<Ops>(), "output_state".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &ProcessorRule| Arc::clone(&r.block_entity_modifier)),
                mod_field,
            ))
            .apply(
                instance,
                Arc::new(|input, loc, pos, output, modifier| {
                    ProcessorRule::with_modifier(input, loc, pos, output, modifier)
                }),
            )
    });
    map_codec::codec_of(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
    use crate::levelgen::structure::templatesystem::block_match_test::BlockMatchTest;
    use crate::levelgen::structure::templatesystem::pos_rule_test_type::PosRuleTestTypes;
    use crate::levelgen::structure::templatesystem::rule_block_entity_modifier::{
        Clear, RuleBlockEntityModifierTypes,
    };
    use crate::levelgen::structure::templatesystem::rule_test_type::RuleTestTypes;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn block_id(name: &str) -> BlockId {
        BlockId::from_name(name).unwrap_or_else(|| panic!("unknown block {name}"))
    }

    #[test]
    fn codec_defaults_optional_fields() {
        let codec = processor_rule_codec::<JsonOps>();
        let input = json!({
            "input_predicate": {
                "predicate_type": "minecraft:block_match",
                "block": "minecraft:stone"
            },
            "location_predicate": {
                "predicate_type": "minecraft:always_true"
            },
            "output_state": {
                "Name": "minecraft:cobblestone"
            }
        });
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            ErasedRuleTest::type_id(&*decoded.input_predicate),
            RuleTestTypes::BLOCK_TEST
        );
        assert_eq!(
            ErasedRuleTest::type_id(&*decoded.loc_predicate),
            RuleTestTypes::ALWAYS_TRUE_TEST
        );
        assert_eq!(
            ErasedPosRuleTest::type_id(&*decoded.pos_predicate),
            PosRuleTestTypes::ALWAYS_TRUE_TEST
        );
        assert_eq!(
            ErasedRuleBlockEntityModifier::type_id(&*decoded.block_entity_modifier),
            RuleBlockEntityModifierTypes::PASSTHROUGH
        );
        assert_eq!(
            decoded.get_output_state().block(),
            block_id("minecraft:cobblestone")
        );
    }

    #[test]
    fn codec_round_trips_explicit_modifier() {
        let codec = processor_rule_codec::<JsonOps>();
        let rule = ProcessorRule::with_modifier(
            Arc::new(BlockMatchTest::new(crate::block::Block::new(block_id(
                "minecraft:stone",
            )))),
            Arc::new(AlwaysTrueTest),
            Arc::new(PosAlwaysTrueTest),
            BlockState::of(block_id("minecraft:deepslate")),
            Arc::new(Clear),
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &rule)
            .result()
            .expect("encode should succeed")
            .clone();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            ErasedRuleBlockEntityModifier::type_id(&*decoded.block_entity_modifier),
            RuleBlockEntityModifierTypes::CLEAR
        );
        assert_eq!(
            decoded.get_output_state().block(),
            block_id("minecraft:deepslate")
        );
    }
}
