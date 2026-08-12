//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.BlockStateMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching an exact `BlockState`. Its `CODEC` is
//! `BlockState.CODEC.fieldOf("block_state").xmap(...)`, and its `test` is
//! `blockState == this.blockState` (identity `==` — `BlockState` is an
//! immutable id-handle, so identity equals value equality and the port derives
//! `PartialEq` on the state id). The `BlockState.CODEC` itself is NOT ported
//! (RivetTodo #202, owned by the `mc.world.level.block.state` unit); the
//! [`block_state_codec`] STUB keeps the dispatch table type-correct, and the
//! field codec fails loudly through the stub when actually used. The `test`
//! equality is fully ported and tested; only the field codec defers.

use crate::levelgen::structure::templatesystem::block_state_codec::block_state_codec;
use crate::levelgen::structure::templatesystem::rule_test::RuleTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{RuleTestTypeId, RuleTestTypes};
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlockStateMatchTest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStateMatchTest {
    /// `blockState` — the matched block state.
    pub block_state: BlockState,
}

impl BlockStateMatchTest {
    /// `new BlockStateMatchTest(BlockState)`.
    pub fn new(block_state: BlockState) -> Self {
        BlockStateMatchTest { block_state }
    }
}

impl RuleTest for BlockStateMatchTest {
    /// `BlockStateMatchTest.test` — `blockState == this.blockState` (Java
    /// identity `==`; `BlockState` is an immutable id-handle, so the port's
    /// value-equality on the state id reproduces it).
    fn test<R: RandomSource>(&self, state: &BlockState, _random: &mut R) -> bool {
        *state == self.block_state
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::BLOCKSTATE_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `BlockStateMatchTest.CODEC` — `BlockState.CODEC.fieldOf("block_state")
/// .xmap(...)`, as the ops-generic `block_state_match_test_map_codec::<Ops>()`
/// factory. Constructing the codec succeeds (so the `blockstate_match` dispatch
/// entry resolves); encoding/decoding through it fails loudly — the
/// `BlockState.CODEC` half is the `block_state_codec` STUB (RivetTodo #202).
pub fn block_state_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlockStateMatchTest, Ops>> {
    let field = codec::field_of(block_state_codec::<Ops>(), "block_state".to_string());
    map_codec::xmap(
        field,
        Arc::new(|b: &BlockState| BlockStateMatchTest::new(*b)),
        Arc::new(|t: &BlockStateMatchTest| t.block_state),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_state_by_identity() {
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let air = crate::block::Block::from_name("minecraft:air")
            .unwrap()
            .default_block_state();
        let t = BlockStateMatchTest::new(stone);
        assert!(t.test(&stone, &mut random));
        assert!(!t.test(&air, &mut random));
    }

    #[test]
    fn codec_construction_succeeds_but_use_panics() {
        // The `BlockState.CODEC` half is the STUB (RivetTodo #202): building
        // the dispatch entry must not fail, but actually using it must fail
        // loudly rather than fabricate a state codec.
        use crate::levelgen::structure::templatesystem::codec_test_util;
        let codec = codec_test_util::codec(block_state_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let t = BlockStateMatchTest::new(stone);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = codec_test_util::encode(&codec, &t);
        }));
        assert!(result.is_err());
    }
}
