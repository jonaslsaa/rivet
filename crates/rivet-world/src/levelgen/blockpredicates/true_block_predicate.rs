//! Port of `net.minecraft.world.level.levelgen.blockpredicates.TrueBlockPredicate`
//! (class, 26.2).
//!
//! Java: a singleton (`INSTANCE`) whose `test` is always true and whose
//! `type()` is `BlockPredicateType.TRUE`. Its `CODEC` is `MapCodec.unit(
//! INSTANCE)` — encodes to `{}` and always decodes to the singleton. The codec
//! is ported here (as the ops-generic
//! `true_block_predicate_map_codec::<Ops>()` factory) and lifted to the erased
//! carrier in `block_predicate`.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.TrueBlockPredicate`.
///
/// `Clone` mirrors the Java singleton (`INSTANCE`) — cloning yields the same
/// always-true predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrueBlockPredicate;

impl TrueBlockPredicate {
    /// `TrueBlockPredicate.INSTANCE`.
    pub const INSTANCE: TrueBlockPredicate = TrueBlockPredicate;

    /// `TrueBlockPredicate.INSTANCE`.
    pub fn instance() -> Self {
        TrueBlockPredicate
    }
}

impl BlockPredicate for TrueBlockPredicate {
    fn test(&self, _level: &dyn WorldGenLevel, _origin: &BlockPos) -> bool {
        true
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::TRUE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `TrueBlockPredicate.CODEC` — `MapCodec.unit(INSTANCE)`, as the ops-generic
/// `true_block_predicate_map_codec::<Ops>()` factory.
pub fn true_block_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<TrueBlockPredicate, Ops>> {
    map_codec::unit_with(Arc::new(|| TrueBlockPredicate::INSTANCE))
}
