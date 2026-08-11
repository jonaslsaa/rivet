//! Port of `net.minecraft.world.level.levelgen.blockpredicates.AnyOfPredicate`
//! (class, 26.2).
//!
//! Java: a `CombiningPredicate` whose `test` is the logical OR over the child
//! predicates (short-circuiting on the first true) and whose `type()` is
//! `BlockPredicateType.ANY_OF`. Its `CODEC` (`codec(AnyOfPredicate::new)` —
//! the shared `"predicates"` record codec) is ported in `block_predicate`
//! (where the dispatch lifts it to the erased carrier).

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::combining_predicate::CombiningPredicate;
use rivet_registry::core::BlockPos;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.AnyOfPredicate`.
#[derive(Debug, Clone)]
pub struct AnyOfPredicate {
    /// `this.predicates` — the child predicate list.
    predicates: Vec<Arc<dyn BlockPredicate>>,
}

impl AnyOfPredicate {
    /// `new AnyOfPredicate(List<BlockPredicate>)`.
    pub fn new(predicates: Vec<Arc<dyn BlockPredicate>>) -> Self {
        AnyOfPredicate { predicates }
    }

    /// `this.predicates`.
    pub fn predicates(&self) -> &[Arc<dyn BlockPredicate>] {
        &self.predicates
    }
}

impl CombiningPredicate for AnyOfPredicate {
    fn predicates(&self) -> &[Arc<dyn BlockPredicate>] {
        &self.predicates
    }
}

impl BlockPredicate for AnyOfPredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        for predicate in &self.predicates {
            if predicate.test(level, origin) {
                return true;
            }
        }
        false
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::ANY_OF
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
