//! `net.minecraft.world.level.block.state.predicate` — the simple block-state
//! predicates (issue #547): [`BlockPredicate`] and [`BlockStatePredicate`],
//! the `java.util.function.Predicate<BlockState>` implementations the block
//! pattern framework builds on (`EndPortalFrameBlock`, `CarvedPumpkinBlock`,
//! `WitherSkullBlock`, `DesertWellFeature`, `EnderDragonFight`).
//!
//! Java's `Predicate<BlockState>` is the `java.util.function.Predicate`
//! interface; the Rust mirror is the [`StatePredicate`] trait, implemented by
//! [`BlockPredicate`], [`BlockStatePredicate`] and the
//! `BlockStatePredicate::ANY` constant ([`AnyBlockState`]). These are the
//! `state.predicate` package's own types — distinct from the
//! `levelgen.blockpredicates` framework (`mc.world.level.levelgen.
//! blockpredicates`), which is the worldgen block-predicate codec dispatch.

pub mod block_predicate;
pub mod block_state_predicate;

use rivet_registry::block_state::BlockState;

pub use block_predicate::BlockPredicate;
pub use block_state_predicate::{AnyBlockState, BlockStatePredicate};

/// `java.util.function.Predicate<net.minecraft.world.level.block.state.BlockState>`
/// — a test of a block state. Java's `Predicate` is an interface; its three
/// implementations in this package are [`BlockPredicate`],
/// [`BlockStatePredicate`] and `BlockStatePredicate.ANY` ([`AnyBlockState`]).
pub trait StatePredicate {
    /// `Predicate.test(@Nullable BlockState)` — Java's `test` takes a nullable
    /// input (`null` never matches the concrete predicates here); the Rust view
    /// is `Option<&BlockState>`.
    fn test(&self, input: Option<&BlockState>) -> bool;

    /// `Predicate.and(Predicate<? super T>)` — the short-circuit conjunction
    /// `t -> test(t) && other.test(t)`. Java's combinators are default methods
    /// on the `Predicate` interface returning an anonymous `Predicate<T>`; the
    /// port returns the concrete [`AndPredicate`], which implements
    /// [`StatePredicate`] — so, like Java's anonymous result, it can itself be
    /// combined further.
    ///
    /// The concrete result structs ([`AndPredicate`], [`OrPredicate`],
    /// [`NegatedPredicate`]) are a deliberate API-shape choice over Java's
    /// opaque anonymous result: they are the minimal composable form for the
    /// trait-object-less memory model here, keep the result chainable, and
    /// expose no mutable state. No consumer has any need of Java's opacity
    /// (all combinators are consumed through [`StatePredicate`]).
    fn and<O>(self, other: O) -> AndPredicate<Self, O>
    where
        Self: Sized,
        O: StatePredicate,
    {
        AndPredicate(self, other)
    }

    /// `Predicate.or(Predicate<? super T>)` — the short-circuit disjunction
    /// `t -> test(t) || other.test(t)`. This is the combinator a consumer like
    /// `WitherSkullBlock` uses:
    /// `forBlock(WITHER_SKELETON_SKULL).or(forBlock(WITHER_SKELETON_WALL_SKULL))`.
    fn or<O>(self, other: O) -> OrPredicate<Self, O>
    where
        Self: Sized,
        O: StatePredicate,
    {
        OrPredicate(self, other)
    }

    /// `Predicate.negate()` — `t -> !test(t)`.
    fn negate(self) -> NegatedPredicate<Self>
    where
        Self: Sized,
    {
        NegatedPredicate(self)
    }
}

/// `Predicate.and`'s anonymous result — `t -> left.test(t) && right.test(t)`.
pub struct AndPredicate<L: StatePredicate, R: StatePredicate>(L, R);

impl<L: StatePredicate, R: StatePredicate> StatePredicate for AndPredicate<L, R> {
    fn test(&self, input: Option<&BlockState>) -> bool {
        self.0.test(input) && self.1.test(input)
    }
}

/// `Predicate.or`'s anonymous result — `t -> left.test(t) || right.test(t)`.
pub struct OrPredicate<L: StatePredicate, R: StatePredicate>(L, R);

impl<L: StatePredicate, R: StatePredicate> StatePredicate for OrPredicate<L, R> {
    fn test(&self, input: Option<&BlockState>) -> bool {
        self.0.test(input) || self.1.test(input)
    }
}

/// `Predicate.negate`'s anonymous result — `t -> !inner.test(t)`.
pub struct NegatedPredicate<P: StatePredicate>(P);

impl<P: StatePredicate> StatePredicate for NegatedPredicate<P> {
    fn test(&self, input: Option<&BlockState>) -> bool {
        !self.0.test(input)
    }
}

/// A boxed predicate is itself a [`StatePredicate`] — the pattern framework
/// (`BlockInWorld.hasState`) stores `Predicate<BlockState>` values, and a boxed
/// trait object must stay usable and chainable once boxed.
impl StatePredicate for Box<dyn StatePredicate + '_> {
    fn test(&self, input: Option<&BlockState>) -> bool {
        self.as_ref().test(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;

    fn stone_state() -> BlockState {
        Blocks::STONE.default_block_state()
    }

    fn dirt_state() -> BlockState {
        Blocks::DIRT.default_block_state()
    }

    #[test]
    fn or_matches_if_either_side_matches() {
        // The `WitherSkullBlock.java:106` shape:
        // forBlock(WITHER_SKELETON_SKULL).or(forBlock(WITHER_SKELETON_WALL_SKULL)).
        let stone_or_dirt =
            BlockPredicate::for_block(Blocks::STONE).or(BlockPredicate::for_block(Blocks::DIRT));
        assert!(stone_or_dirt.test(Some(&stone_state())));
        assert!(stone_or_dirt.test(Some(&dirt_state())));
        assert!(!stone_or_dirt.test(Some(&Blocks::AIR.default_block_state())));
        assert!(!stone_or_dirt.test(None));
    }

    #[test]
    fn and_matches_only_when_both_sides_match() {
        // `t -> test(t) && other.test(t)`: the block check and the
        // property-constrained `BlockStatePredicate` must both pass.
        let stone_and_axis = BlockPredicate::for_block(Blocks::STONE)
            .and(BlockStatePredicate::for_block(Blocks::STONE));
        assert!(stone_and_axis.test(Some(&stone_state())));
        assert!(!stone_and_axis.test(Some(&dirt_state())));

        // A contradictory conjunction matches nothing.
        let contradictory =
            BlockPredicate::for_block(Blocks::STONE).and(BlockPredicate::for_block(Blocks::DIRT));
        assert!(!contradictory.test(Some(&stone_state())));
        assert!(!contradictory.test(Some(&dirt_state())));
    }

    #[test]
    fn negate_inverts_a_predicate() {
        let not_stone = BlockPredicate::for_block(Blocks::STONE).negate();
        assert!(!not_stone.test(Some(&stone_state())));
        assert!(not_stone.test(Some(&dirt_state())));
        // Java `negate` of a false test on `null` is true.
        assert!(not_stone.test(None));
    }

    #[test]
    fn combinators_chain_and_short_circuit() {
        // `or` after `and`: matches dirt via the `or` side.
        let p = BlockPredicate::for_block(Blocks::STONE)
            .and(BlockPredicate::for_block(Blocks::STONE))
            .or(BlockPredicate::for_block(Blocks::DIRT));
        assert!(p.test(Some(&dirt_state())));
        assert!(p.test(Some(&stone_state())));
        assert!(!p.test(Some(&Blocks::AIR.default_block_state())));
    }
}
