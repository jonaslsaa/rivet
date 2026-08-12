//! Port of `net.minecraft.world.level.block.state.predicate.BlockPredicate`
//! (MC 26.2).
//!
//! Java:
//! ```java
//! public class BlockPredicate implements Predicate<BlockState> {
//!     private final Block block;
//!     public BlockPredicate(final Block block) { this.block = block; }
//!     public static BlockPredicate forBlock(final Block block) {
//!         return new BlockPredicate(block);
//!     }
//!     @Override public boolean test(final @Nullable BlockState input) {
//!         return input != null && input.is(this.block);
//!     }
//! }
//! ```

use crate::block::Block;
use rivet_registry::block_state::BlockState;

use super::StatePredicate;

/// `net.minecraft.world.level.block.state.predicate.BlockPredicate` — the
/// `Predicate<BlockState>` matching states of one `Block`.
///
/// `PartialEq`/`Eq` are a deliberate ergonomic divergence from Java, where
/// `BlockPredicate` does not override `equals` (identity equality). Here `Block`
/// is the id-handle (OWNERSHIP: arenas + ids), so two predicates for the same
/// block compare equal — natural value equality; `test` is the only behavior,
/// and the id-handle model makes value equality observably identical to
/// identity equality for a porter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockPredicate {
    block: Block,
}

impl BlockPredicate {
    /// `new BlockPredicate(Block)`.
    pub fn new(block: Block) -> Self {
        BlockPredicate { block }
    }

    /// `BlockPredicate.forBlock(Block)`.
    pub fn for_block(block: Block) -> Self {
        BlockPredicate::new(block)
    }

    /// The matched block.
    pub fn block(&self) -> Block {
        self.block
    }
}

impl StatePredicate for BlockPredicate {
    /// `test(@Nullable BlockState)` — `input != null && input.is(this.block)`.
    /// `BlockState.is(Block)` is `getBlock() == block`; `Block` is the
    /// id-handle (OWNERSHIP: arenas + ids), so the comparison is the state's
    /// block id against the predicate's.
    fn test(&self, input: Option<&BlockState>) -> bool {
        input.is_some_and(|state| state.block() == self.block.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;

    #[test]
    fn matches_states_of_its_block_only() {
        let stone = BlockPredicate::for_block(Blocks::STONE);
        // A state of the matched block (its default state) passes.
        assert!(stone.test(Some(&Blocks::STONE.default_block_state())));
        // A state of a different block fails.
        assert!(!stone.test(Some(&Blocks::DIRT.default_block_state())));
        // Java `test(null)` is false.
        assert!(!stone.test(None));
    }

    #[test]
    fn new_and_for_block_are_equivalent() {
        let a = BlockPredicate::new(Blocks::STONE);
        let b = BlockPredicate::for_block(Blocks::STONE);
        assert_eq!(a, b);
        assert_eq!(a.block(), Blocks::STONE);
    }
}
