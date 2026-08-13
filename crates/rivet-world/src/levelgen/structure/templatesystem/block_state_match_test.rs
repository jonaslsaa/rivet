//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.BlockStateMatchTest`
//! (class, 26.2).
//!
//! Java: a rule test matching an exact `BlockState`. Its `CODEC` is
//! `BlockState.CODEC.fieldOf("block_state").xmap(...)`, and its `test` is
//! `blockState == this.blockState` (identity `==` — `BlockState` is an
//! immutable id-handle, so identity equals value equality and the port derives
//! `PartialEq` on the state id). The `BlockState.CODEC` half is the ported
//! `rivet_registry::block_state_codec` (issue #391); the field codec round-trips
//! through it like every other `BlockState`-carrying codec in the codebase.

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
/// factory. The `BlockState.CODEC` half is the ported
/// `rivet_registry::block_state_codec` (issue #391).
pub fn block_state_match_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlockStateMatchTest, Ops>> {
    let field = codec::field_of(
        rivet_registry::block_state_codec::block_state_codec::<Ops>(),
        "block_state".to_string(),
    );
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
    fn codec_round_trips_singleton_state() {
        // A singleton block state (stone) encodes through the real
        // `BlockState.CODEC` as just `{"Name": ...}`, and the `block_state`
        // field wraps it.
        use crate::levelgen::structure::templatesystem::codec_test_util;
        use rivet_registry::generated::blocks::BlockId;
        use rivet_serialization::json_ops::JsonOps;
        use serde_json::json;

        let codec = codec_test_util::codec(block_state_match_test_map_codec::<JsonOps>());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let t = BlockStateMatchTest::new(stone);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(encoded, json!({"block_state": {"Name": "minecraft:stone"}}));
        let decoded = codec_test_util::decode(&codec, &encoded);
        assert_eq!(decoded, t);
    }

    #[test]
    fn codec_round_trips_state_with_properties() {
        // A multi-property state (oak_log with a non-default axis) round-trips
        // through the `Properties` fold, mirroring the `block_state_codec`
        // tests.
        use crate::levelgen::structure::templatesystem::codec_test_util;
        use rivet_registry::generated::block_properties::BlockPropertyId;
        use rivet_registry::generated::blocks::BlockId;
        use rivet_serialization::json_ops::JsonOps;
        use serde_json::json;

        let codec = codec_test_util::codec(block_state_match_test_map_codec::<JsonOps>());
        let oak_log = BlockId::from_name("minecraft:oak_log").unwrap();
        let state = BlockState::of(oak_log)
            .set_property(BlockPropertyId::Axis, 0)
            .unwrap();
        let t = BlockStateMatchTest::new(state);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(
            encoded,
            json!({"block_state": {"Name": "minecraft:oak_log", "Properties": {"axis": "x"}}})
        );
        let decoded = codec_test_util::decode(&codec, &encoded);
        assert_eq!(decoded, t);
    }
}
