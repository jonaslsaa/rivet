//! `net.minecraft.world.level.levelgen.material` (26.2).
//!
//! The `mc.world.level.levelgen.material` unit — the `MaterialRuleList` record,
//! the `NoiseChunk.BlockStateFiller` list. Ported faithfully from
//! `MaterialRuleList.java`: the first rule that returns a non-`None` block state
//! wins; if every rule returns `None` the list itself returns `None` (Java's
//! `null`).
//!
//! Java's class implements `NoiseChunk.BlockStateFiller`; the Rust port
//! implements the `noisegen` trait [`BlockStateFiller`]. The `BlockStateFiller`
//! trait and the `FunctionContext` it takes are owned by the `noisegen` unit
//! (the noise-chunk SCC), so the material unit consumes them rather than
//! re-defining them.

use crate::block::BlockState;
use crate::levelgen::noise::density_function::FunctionContext;
use crate::levelgen::noisegen::noise_chunk::BlockStateFiller;
use std::sync::Arc;

/// `MaterialRuleList(NoiseChunk.BlockStateFiller[] materialRuleList)` — the
/// record wrapping the ordered rule list.
pub struct MaterialRuleList {
    /// `materialRuleList` — the `NoiseChunk.BlockStateFiller[]`.
    material_rule_list: Vec<Arc<dyn BlockStateFiller>>,
}

impl MaterialRuleList {
    /// `MaterialRuleList(NoiseChunk.BlockStateFiller[])`.
    pub fn new(material_rule_list: Vec<Arc<dyn BlockStateFiller>>) -> Self {
        MaterialRuleList { material_rule_list }
    }

    /// `materialRuleList()` — the record accessor (read-only; the rules are
    /// consulted in order by [`BlockStateFiller::calculate`]).
    pub fn material_rule_list(&self) -> &[Arc<dyn BlockStateFiller>] {
        &self.material_rule_list
    }
}

impl BlockStateFiller for MaterialRuleList {
    /// `calculate(FunctionContext)` — iterates the rules in order and returns
    /// the first non-`None` block state; `None` if every rule is `None`.
    fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState> {
        for rule in &self.material_rule_list {
            let state = rule.calculate(context);
            if state.is_some() {
                return state;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::density_function::SinglePointContext;
    use rivet_registry::generated::block_states::StateId;

    /// A `BlockStateFiller` test double that returns a fixed `BlockState` when
    /// `y == hit_y`, and `None` otherwise.
    struct AtY {
        hit_y: i32,
        state: BlockState,
    }

    impl BlockStateFiller for AtY {
        fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState> {
            if context.block_y() == self.hit_y {
                Some(self.state)
            } else {
                None
            }
        }
    }

    fn filler(hit_y: i32, id: u16) -> Arc<dyn BlockStateFiller> {
        Arc::new(AtY {
            hit_y,
            state: BlockState::new(StateId(id)),
        })
    }

    #[test]
    fn empty_list_returns_none() {
        let list = MaterialRuleList::new(Vec::new());
        let context = SinglePointContext::new(0, 0, 0);
        assert_eq!(list.calculate(&context), None);
    }

    #[test]
    fn first_non_none_rule_wins_in_order() {
        // Rule 0 hits only y=1, rule 1 hits only y=2. The list must return the
        // first rule's state for y=1 and the second for y=2, preserving order.
        let list = MaterialRuleList::new(vec![filler(1, 42), filler(2, 43)]);
        let at_one = SinglePointContext::new(0, 1, 0);
        let at_two = SinglePointContext::new(0, 2, 0);
        assert_eq!(list.calculate(&at_one), Some(BlockState::new(StateId(42))));
        assert_eq!(list.calculate(&at_two), Some(BlockState::new(StateId(43))));
    }

    #[test]
    fn short_circuits_on_first_hit() {
        // Both rules hit y=1; the list must return the first and never consult
        // the second (the second returns a different state that would be
        // observable if it were consulted).
        let list = MaterialRuleList::new(vec![filler(1, 42), filler(1, 43)]);
        let context = SinglePointContext::new(0, 1, 0);
        assert_eq!(list.calculate(&context), Some(BlockState::new(StateId(42))));
    }

    #[test]
    fn all_none_returns_none() {
        let list = MaterialRuleList::new(vec![filler(1, 42), filler(2, 43)]);
        let context = SinglePointContext::new(0, 3, 0);
        assert_eq!(list.calculate(&context), None);
    }
}
