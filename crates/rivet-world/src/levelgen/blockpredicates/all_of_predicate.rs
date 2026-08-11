//! Port of `net.minecraft.world.level.levelgen.blockpredicates.AllOfPredicate`
//! (class, 26.2).
//!
//! Java: a `CombiningPredicate` whose `test` is the logical AND over the child
//! predicates (short-circuiting on the first false) and whose `type()` is
//! `BlockPredicateType.ALL_OF`. Its `CODEC` (`codec(AllOfPredicate::new)` —
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

/// `net.minecraft.world.level.levelgen.blockpredicates.AllOfPredicate`.
#[derive(Debug, Clone)]
pub struct AllOfPredicate {
    /// `this.predicates` — the child predicate list.
    predicates: Vec<Arc<dyn BlockPredicate>>,
}

impl AllOfPredicate {
    /// `new AllOfPredicate(List<BlockPredicate>)`.
    pub fn new(predicates: Vec<Arc<dyn BlockPredicate>>) -> Self {
        AllOfPredicate { predicates }
    }

    /// `this.predicates`.
    pub fn predicates(&self) -> &[Arc<dyn BlockPredicate>] {
        &self.predicates
    }
}

impl CombiningPredicate for AllOfPredicate {
    fn predicates(&self) -> &[Arc<dyn BlockPredicate>] {
        &self.predicates
    }
}

impl BlockPredicate for AllOfPredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        for predicate in &self.predicates {
            if !predicate.test(level, origin) {
                return false;
            }
        }
        true
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::ALL_OF
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
