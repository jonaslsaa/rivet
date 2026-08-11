//! `net.minecraft.world.level.levelgen.blockpredicates` — the block-predicate
//! value/codec framework (issue #399, under #181).
//!
//! Owned by the `mc.world.level.levelgen.blockpredicates.{core,combinators,
//! simple,states}` manifest units (26.2). This slice ports the dependency-clean
//! core plus the combinators and the two simple predicates that need no
//! block-state world access: [`BlockPredicate`] (the dispatch hub interface),
//! [`BlockPredicateType`] (the type registry), [`CombiningPredicate`]/
//! [`StateTestingPredicate`] (the shared bases), [`AllOfPredicate`],
//! [`AnyOfPredicate`], [`NotPredicate`], [`InsideWorldBoundsPredicate`] and
//! [`TrueBlockPredicate`]. It does NOT port placement execution,
//! block-column generation, features, value/height providers, generation, or
//! any writes.
//!
//! ## Dispatch and recursion
//!
//! [`block_predicate_codec`] is `BlockPredicate.CODEC`: the `"type"` by-name
//! dispatch over `BuiltInRegistries.BLOCK_PREDICATE_TYPE`, wrapped in a
//! `codec::recursive` graph whose single `RecursiveSelf` threads through the
//! combinators (the same pattern `ComponentSerialization` uses), so arbitrary
//! nesting round-trips. The `"type"` registry codec reproduces Paper's exact
//! by-name error (`Unknown registry key in ResourceKey[minecraft:root /
//! minecraft:block_predicate_type]: {name}`).
//!
//! ## Capability-unavailable boundary (RivetTodo #399)
//!
//! The state-testing predicates (`StateTestingPredicate::test`) resolve
//! `WorldGenLevel.getBlockState(origin.offset(offset))`; the real world-access
//! implementation is not ported, so no production `WorldGenLevel` provides it
//! and every call through that seam fails explicitly (panic) rather than
//! fabricating a state — the established worldgen-seam pattern (same as the
//! `#181`/`#180` dispatch stubs). The nine out-of-scope Paper predicate types
//! (`matching_blocks`, `matching_block_tag`, `matching_fluids`,
//! `matching_biomes`, `has_sturdy_face`, `solid`, `replaceable`,
//! `would_survive`, `unobstructed`) are likewise deferred: their registry
//! identity is present (all fourteen constants, exact declaration order) but
//! their codec lookup fails explicitly.

pub mod all_of_predicate;
pub mod any_of_predicate;
pub mod block_predicate;
pub mod block_predicate_type;
pub mod combining_predicate;
pub mod inside_world_bounds_predicate;
pub mod not_predicate;
pub mod state_testing_predicate;
pub mod true_block_predicate;

pub use all_of_predicate::AllOfPredicate;
pub use any_of_predicate::AnyOfPredicate;
pub use block_predicate::{
    BlockPredicate, all_of, always_true, any_of, block_predicate_codec, inside_world, not,
};
pub use block_predicate_type::{BlockPredicateType, BlockPredicateTypeId};
pub use combining_predicate::CombiningPredicate;
pub use inside_world_bounds_predicate::InsideWorldBoundsPredicate;
pub use not_predicate::NotPredicate;
pub use state_testing_predicate::StateTestingPredicate;
pub use true_block_predicate::TrueBlockPredicate;
