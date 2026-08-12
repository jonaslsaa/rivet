//! STUB(mc.world.level.block.state) — `BlockState.CODEC`.
//!
//! `BlockStateMatchTest`/`RandomBlockStateMatchTest` reference `BlockState.CODEC`
//! (`codec(BuiltInRegistries.BLOCK.byNameCodec(), Block::defaultBlockState,
//! Block::getStateDefinition).stable()`), which is NOT ported — it belongs to
//! the `mc.world.level.block.state` unit (RivetTodo #202) and its owning
//! worktree. This unit declares the minimal seam the rule-test codecs type-check
//! against: a codec that can be *constructed* (so the dispatch table for the
//! `blockstate_match` / `random_blockstate_match` types resolves) but that
//! panics when actually used to encode/decode a `BlockState`. No fabricated
//! state codec — the real one lands with the owning unit.
//!
//! The `BlockStateMatchTest::test` equality itself (Java `==` on the state
//! id-handle) is fully ported and tested; only the field codec defers.

use rivet_registry::block_state::BlockState;
use rivet_serialization::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder::Encoder;
use std::sync::Arc;

/// A codec whose every operation panics — the `BlockState.CODEC` stand-in
/// (see the module doc).
struct BlockStateCodecStub<Ops> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops> std::fmt::Debug for BlockStateCodecStub<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockState.CODEC[STUB]")
    }
}

impl<Ops: DynamicOps + 'static> Encoder<BlockState, Ops> for BlockStateCodecStub<Ops> {
    fn encode(
        &self,
        _input: &BlockState,
        _ops: &Ops,
        _prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        panic!("BlockState.CODEC is not implemented (RivetTodo #202, STUB)")
    }
}

impl<Ops: DynamicOps + 'static> Decoder<BlockState, Ops> for BlockStateCodecStub<Ops> {
    fn decode(&self, _ops: &Ops, _input: &Ops::Output) -> DataResult<(BlockState, Ops::Output)> {
        panic!("BlockState.CODEC is not implemented (RivetTodo #202, STUB)")
    }
}

impl<Ops: DynamicOps + 'static> Codec<BlockState, Ops> for BlockStateCodecStub<Ops> {}

/// `BlockState.CODEC`, as the ops-generic `block_state_codec::<Ops>()` factory.
/// Constructing the handle succeeds (so the rule-test dispatch table resolves);
/// encoding/decoding through it panics.
pub fn block_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockState, Ops>> {
    Arc::new(BlockStateCodecStub {
        _ops: std::marker::PhantomData,
    })
}
