//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.BlockMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching a single block by id. Its `CODEC` is
//! `BuiltInRegistries.BLOCK.byNameCodec().fieldOf("block").xmap(BlockMatchTest::new,
//! t -> t.block)`, and its `test` is `blockState.is(this.block)` (Java `Block`
//! equality = registry id equality, so the port compares the state's owning
//! `BlockId` against the held `Block`'s id). The codec is ported here (as the
//! ops-generic `block_match_test_map_codec::<Ops>()` factory) and lifted to the
//! erased carrier in `rule_test`.

use crate::block::Block;
use crate::chunk::registry_codecs::block_by_name_codec;
use crate::levelgen::structure::templatesystem::rule_test::RuleTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{RuleTestTypeId, RuleTestTypes};
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlockMatchTest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMatchTest {
    /// `block` — the matched block.
    pub block: Block,
}

impl BlockMatchTest {
    /// `new BlockMatchTest(Block)`.
    pub fn new(block: Block) -> Self {
        BlockMatchTest { block }
    }
}

impl RuleTest for BlockMatchTest {
    /// `BlockMatchTest.test` — `blockState.is(this.block)`; `Block` equality is
    /// registry-id equality, so the port compares the state's owning block id.
    fn test<R: RandomSource>(&self, state: &BlockState, _random: &mut R) -> bool {
        state.block() == self.block.id()
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::BLOCK_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `BlockMatchTest.CODEC` — `BuiltInRegistries.BLOCK.byNameCodec().fieldOf(
/// "block").xmap(...)`, as the ops-generic `block_match_test_map_codec::<Ops>()`
/// factory.
pub fn block_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlockMatchTest, Ops>> {
    let field = codec::field_of(block_by_name_codec::<Ops>(), "block".to_string());
    map_codec::xmap(
        field,
        Arc::new(|b: &Block| BlockMatchTest::new(*b)),
        Arc::new(|t: &BlockMatchTest| t.block),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::codec_test_util;
    use serde_json::json;

    #[test]
    fn matches_block_by_id() {
        let t = BlockMatchTest::new(Block::from_name("minecraft:stone").unwrap());
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let stone = Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let air = Block::from_name("minecraft:air")
            .unwrap()
            .default_block_state();
        assert!(t.test(&stone, &mut random));
        assert!(!t.test(&air, &mut random));
    }

    #[test]
    fn codec_round_trips() {
        let codec = codec_test_util::codec(block_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = BlockMatchTest::new(Block::from_name("minecraft:stone").unwrap());
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(encoded, json!({"block": "minecraft:stone"}));
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }

    #[test]
    fn codec_unknown_block_name_errors() {
        let codec = codec_test_util::codec(block_match_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let result =
            codec_test_util::decode_result(&codec, &json!({"block": "minecraft:not_a_block"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: minecraft:not_a_block"),
            "got: {msg}"
        );
    }
}
