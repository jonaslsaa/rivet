//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.RandomBlockMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching a block by id with a per-attempt probability.
//! Its `CODEC` is a `RecordCodecBuilder` over `block`
//! (`BuiltInRegistries.BLOCK.byNameCodec().fieldOf`) and `probability`
//! (`Codec.FLOAT.fieldOf`), and its `test` is `blockState.is(block) &&
//! random.nextFloat() < probability` (short-circuiting: the random draw
//! happens only when the block matches). The codec is ported here (as the
//! ops-generic `random_block_match_test_map_codec::<Ops>()` factory) and lifted
//! to the erased carrier in `rule_test`.

use crate::block::Block;
use crate::chunk::registry_codecs::block_by_name_codec;
use crate::levelgen::structure::templatesystem::rule_test::RuleTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{RuleTestTypeId, RuleTestTypes};
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.RandomBlockMatchTest`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RandomBlockMatchTest {
    /// `block` — the matched block.
    pub block: Block,
    /// `probability` — the per-attempt match probability.
    pub probability: f32,
}

impl RandomBlockMatchTest {
    /// `new RandomBlockMatchTest(Block, float)`.
    pub fn new(block: Block, probability: f32) -> Self {
        RandomBlockMatchTest { block, probability }
    }
}

impl RuleTest for RandomBlockMatchTest {
    /// `RandomBlockMatchTest.test` — `blockState.is(block) && random.nextFloat()
    /// < probability` (short-circuit: the draw happens only when the block
    /// matches, so the random stream position matches Java exactly).
    fn test<R: RandomSource>(&self, state: &BlockState, random: &mut R) -> bool {
        state.block() == self.block.id() && random.next_float() < self.probability
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::RANDOM_BLOCK_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `RandomBlockMatchTest.CODEC` — the record codec over `block` and
/// `probability`, as the ops-generic `random_block_match_test_map_codec::<Ops>()`
/// factory.
pub fn random_block_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RandomBlockMatchTest, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|t: &RandomBlockMatchTest| t.block),
                "block".to_string(),
                block_by_name_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|t: &RandomBlockMatchTest| t.probability),
                "probability".to_string(),
                codec::float_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|block: Block, probability: f32| {
                    RandomBlockMatchTest::new(block, probability)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::codec_test_util;
    use serde_json::json;

    #[test]
    fn matches_block_and_probability() {
        // probability 1.0 → every block match passes; probability 0.0 → fails.
        let stone = Block::from_name("minecraft:stone").unwrap();
        let air = Block::from_name("minecraft:air").unwrap();
        let stone_state = stone.default_block_state();
        let air_state = air.default_block_state();
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let always = RandomBlockMatchTest::new(stone, 1.0);
        assert!(always.test(&stone_state, &mut random));
        assert!(!always.test(&air_state, &mut random));
        let never = RandomBlockMatchTest::new(stone, 0.0);
        assert!(!never.test(&stone_state, &mut random));
        assert!(!never.test(&air_state, &mut random));
    }

    #[test]
    fn random_draw_only_happens_on_block_match() {
        // The block check short-circuits: a matching test consumes one draw
        // (the probability draw) while a non-matching test consumes none. Both
        // streams are `LegacyRandomSource(42)` (draw 1 = 0.7275637, draw 2 =
        // 0.054665208), so the next draw differs: `matching` is at draw 2 while
        // `non_matching` is still at draw 1. If the non-matching test had drawn,
        // both streams would be at the same position and the draws would match.
        let stone = Block::from_name("minecraft:stone").unwrap();
        let air = Block::from_name("minecraft:air").unwrap();
        let stone_state = stone.default_block_state();
        let air_state = air.default_block_state();
        let t = RandomBlockMatchTest::new(stone, 0.5);
        let mut matching = rivet_util::random::LegacyRandomSource::new(42);
        let _ = t.test(&stone_state, &mut matching);
        let mut non_matching = rivet_util::random::LegacyRandomSource::new(42);
        let _ = t.test(&air_state, &mut non_matching);
        assert_eq!(non_matching.next_float(), 0.7275637);
        assert_eq!(matching.next_float(), 0.054665208);
    }

    #[test]
    fn codec_round_trips() {
        let codec = codec_test_util::codec(random_block_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = RandomBlockMatchTest::new(Block::from_name("minecraft:stone").unwrap(), 0.5);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(
            encoded,
            json!({"block": "minecraft:stone", "probability": 0.5})
        );
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }

    #[test]
    fn codec_requires_probability_field() {
        let codec = codec_test_util::codec(random_block_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let result = codec_test_util::decode_result(&codec, &json!({"block": "minecraft:stone"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key probability"), "got: {msg}");
    }
}
