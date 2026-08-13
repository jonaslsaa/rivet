//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.RandomBlockStateMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching an exact `BlockState` with a per-attempt
//! probability. Its `CODEC` is a `RecordCodecBuilder` over `block_state`
//! (`BlockState.CODEC.fieldOf`) and `probability` (`Codec.FLOAT.fieldOf`), and
//! its `test` is `blockState == this.blockState && random.nextFloat() <
//! probability` (short-circuiting; identity `==` on the state id-handle). The
//! `BlockState.CODEC` half is the ported `rivet_registry::block_state_codec`
//! (issue #391); the field codec round-trips through it like every other
//! `BlockState`-carrying codec in the codebase.

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

/// `net.minecraft.world.level.levelgen.structure.templatesystem.RandomBlockStateMatchTest`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RandomBlockStateMatchTest {
    /// `blockState` — the matched block state.
    pub block_state: BlockState,
    /// `probability` — the per-attempt match probability.
    pub probability: f32,
}

impl RandomBlockStateMatchTest {
    /// `new RandomBlockStateMatchTest(BlockState, float)`.
    pub fn new(block_state: BlockState, probability: f32) -> Self {
        RandomBlockStateMatchTest {
            block_state,
            probability,
        }
    }
}

impl RuleTest for RandomBlockStateMatchTest {
    /// `RandomBlockStateMatchTest.test` — `blockState == this.blockState &&
    /// random.nextFloat() < probability` (Java identity `==` on the state
    /// id-handle, short-circuiting so the draw happens only on a state match).
    fn test<R: RandomSource>(&self, state: &BlockState, random: &mut R) -> bool {
        *state == self.block_state && random.next_float() < self.probability
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::RANDOM_BLOCKSTATE_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `RandomBlockStateMatchTest.CODEC` — the record codec over `block_state` and
/// `probability`, as the ops-generic
/// `random_block_state_match_test_map_codec::<Ops>()` factory. The
/// `BlockState.CODEC` half is the ported `rivet_registry::block_state_codec`
/// (issue #391).
pub fn random_block_state_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RandomBlockStateMatchTest, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|t: &RandomBlockStateMatchTest| t.block_state),
                "block_state".to_string(),
                rivet_registry::block_state_codec::block_state_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|t: &RandomBlockStateMatchTest| t.probability),
                "probability".to_string(),
                codec::float_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|block_state: BlockState, probability: f32| {
                    RandomBlockStateMatchTest::new(block_state, probability)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_state_and_probability() {
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let air = crate::block::Block::from_name("minecraft:air")
            .unwrap()
            .default_block_state();
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let always = RandomBlockStateMatchTest::new(stone, 1.0);
        assert!(always.test(&stone, &mut random));
        assert!(!always.test(&air, &mut random));
        let never = RandomBlockStateMatchTest::new(stone, 0.0);
        assert!(!never.test(&stone, &mut random));
        assert!(!never.test(&air, &mut random));
    }

    #[test]
    fn random_draw_only_happens_on_state_match() {
        // The state check short-circuits: a matching test consumes one draw
        // (the probability draw) while a non-matching test consumes none. Both
        // streams are `LegacyRandomSource(42)` (draw 1 = 0.7275637, draw 2 =
        // 0.054665208), so the next draw differs: `matching` is at draw 2 while
        // `non_matching` is still at draw 1. If the non-matching test had drawn,
        // both streams would be at the same position and the draws would match.
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let air = crate::block::Block::from_name("minecraft:air")
            .unwrap()
            .default_block_state();
        let t = RandomBlockStateMatchTest::new(stone, 0.5);
        let mut matching = rivet_util::random::LegacyRandomSource::new(42);
        let _ = t.test(&stone, &mut matching);
        let mut non_matching = rivet_util::random::LegacyRandomSource::new(42);
        let _ = t.test(&air, &mut non_matching);
        assert_eq!(non_matching.next_float(), 0.7275637);
        assert_eq!(matching.next_float(), 0.054665208);
    }

    #[test]
    fn codec_round_trips() {
        use crate::levelgen::structure::templatesystem::codec_test_util;
        use rivet_registry::generated::blocks::BlockId;
        use rivet_serialization::json_ops::JsonOps;
        use serde_json::json;

        let codec = codec_test_util::codec(random_block_state_match_test_map_codec::<JsonOps>());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let t = RandomBlockStateMatchTest::new(stone, 0.5);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(
            encoded,
            json!({"block_state": {"Name": "minecraft:stone"}, "probability": 0.5})
        );
        let decoded = codec_test_util::decode(&codec, &encoded);
        assert_eq!(decoded, t);
    }
}
