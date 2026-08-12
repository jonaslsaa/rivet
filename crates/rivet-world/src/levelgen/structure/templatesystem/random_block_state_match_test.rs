//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.RandomBlockStateMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching an exact `BlockState` with a per-attempt
//! probability. Its `CODEC` is a `RecordCodecBuilder` over `block_state`
//! (`BlockState.CODEC.fieldOf`) and `probability` (`Codec.FLOAT.fieldOf`), and
//! its `test` is `blockState == this.blockState && random.nextFloat() <
//! probability` (short-circuiting; identity `==` on the state id-handle). The
//! `BlockState.CODEC` half is NOT ported (RivetTodo #202, owned by the
//! `mc.world.level.block.state` unit); the [`block_state_codec`] STUB keeps the
//! dispatch table type-correct, and the field codec fails loudly through the
//! stub when actually used. The `test` equality and probability draw are fully
//! ported and tested; only the field codec defers.

use crate::levelgen::structure::templatesystem::block_state_codec::block_state_codec;
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
/// `random_block_state_match_test_map_codec::<Ops>()` factory. Constructing the
/// codec succeeds (so the `random_blockstate_match` dispatch entry resolves);
/// encoding/decoding through it fails loudly — the `BlockState.CODEC` half is
/// the `block_state_codec` STUB (RivetTodo #202).
pub fn random_block_state_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RandomBlockStateMatchTest, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|t: &RandomBlockStateMatchTest| t.block_state),
                "block_state".to_string(),
                block_state_codec::<Ops>(),
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
    fn codec_construction_succeeds_but_use_panics() {
        // The `BlockState.CODEC` half is the STUB (RivetTodo #202): building
        // the dispatch entry must not fail, but actually using it must fail
        // loudly rather than fabricate a state codec.
        use crate::levelgen::structure::templatesystem::codec_test_util;
        let codec = codec_test_util::codec(random_block_state_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let t = RandomBlockStateMatchTest::new(stone, 0.5);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = codec_test_util::encode(&codec, &t);
        }));
        assert!(result.is_err());
    }
}
