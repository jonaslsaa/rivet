//! Port of `net.minecraft.world.level.levelgen.blockpredicates.NotPredicate`
//! (class, 26.2).
//!
//! Java: a single-child predicate whose `test` negates the child and whose
//! `type()` is `BlockPredicateType.NOT`. Its `CODEC` is a record codec over the
//! `"predicate"` field (`BlockPredicate.CODEC`, the shared recursive graph).
//! The codec is ported here (as the ops-generic
//! `not_predicate_map_codec::<Ops>(top)` factory taking the shared `top`
//! element codec) and lifted to the erased carrier in `block_predicate`.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.NotPredicate`.
#[derive(Debug, Clone)]
pub struct NotPredicate {
    /// `this.predicate` — the child predicate.
    predicate: Arc<dyn BlockPredicate>,
}

impl NotPredicate {
    /// `new NotPredicate(BlockPredicate)`.
    pub fn new(predicate: Arc<dyn BlockPredicate>) -> Self {
        NotPredicate { predicate }
    }

    /// `this.predicate`.
    pub fn predicate(&self) -> &Arc<dyn BlockPredicate> {
        &self.predicate
    }
}

impl BlockPredicate for NotPredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        !self.predicate.test(level, origin)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::NOT
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `NotPredicate.CODEC` — the record codec over the `"predicate"` field
/// (`BlockPredicate.CODEC`), as the ops-generic `not_predicate_map_codec::<Ops>
/// (top)` factory. `top` is the shared `RecursiveSelf` of the block-predicate
/// graph (the same codec the `"predicate"` element recurses into).
pub fn not_predicate_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn BlockPredicate>, Ops>>,
) -> Arc<dyn MapCodec<NotPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|p: &NotPredicate| p.predicate.clone()),
                "predicate".to_string(),
                top,
            ))
            .apply(instance, Arc::new(NotPredicate::new))
    })
}
