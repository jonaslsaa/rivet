//! `net.minecraft.world.level.levelgen.blockpredicates` — the block-predicate
//! value/codec framework (issue #399, under #181).
//!
//! Owned by the `mc.world.level.levelgen.blockpredicates.{core,combinators,
//! simple,states}` manifest units (26.2). This slice ports the dependency-clean
//! core, the combinators, the full `.states` unit (the block-state-testing
//! predicates) and the remaining `.simple` leaves: [`BlockPredicate`] (the
//! dispatch hub interface), [`BlockPredicateType`] (the type registry),
//! [`CombiningPredicate`]/[`StateTestingPredicate`] (the shared bases),
//! [`AllOfPredicate`], [`AnyOfPredicate`], [`NotPredicate`],
//! [`InsideWorldBoundsPredicate`], [`TrueBlockPredicate`], plus
//! [`SolidPredicate`], [`ReplaceablePredicate`], [`MatchingBlocksPredicate`],
//! [`MatchingBlockTagPredicate`], [`MatchingFluidsPredicate`],
//! [`HasSturdyFacePredicate`], [`MatchingBiomesPredicate`],
//! [`UnobstructedPredicate`] and [`WouldSurvivePredicate`]. It does NOT port
//! placement execution, block-column generation, features, value/height
//! providers, generation, or any writes.
//!
//! ## Dispatch and recursion
//!
//! [`block_predicate_codec`] is `BlockPredicate.CODEC`: the `"type"` by-name
//! dispatch over `BuiltInRegistries.BLOCK_PREDICATE_TYPE`, wrapped in a
//! `codec::recursive` graph whose single `RecursiveSelf` threads through the
//! combinators (the same pattern `ComponentSerialization` uses), so arbitrary
//! nesting round-trips. The `"type"` registry codec reproduces Paper's exact
//! by-name error (`Unknown registry key in ResourceKey[minecraft:root /
//! minecraft:block_predicate_type]: {name}`). Because the `matching_blocks`/
//! `matching_fluids`/`matching_biomes` `"blocks"`/`"fluids"`/`"biomes"` fields
//! are `RegistryCodecs.homogeneousList(...)` (a `HolderSetCodec` whose element
//! codec requires the ops' `RegistryOpsLookup`), the dispatch is raised to
//! `Ops: DynamicOps + 'static + RegistryOpsLookup`.
//!
//! ## Capability-unavailable boundary (RivetTodo #399)
//!
//! The state-testing predicates (`StateTestingPredicate::test`) resolve
//! `WorldGenLevel.getBlockState(origin.offset(offset))`, and the simple leaves
//! resolve their world reads (`getBiome`, `isUnobstructed`, `isFaceSturdy`,
//! `canSurvive`); the real world-access implementation is not ported, so no
//! production `WorldGenLevel` provides it and every call through those seams
//! fails explicitly (panic) rather than fabricating a result — the established
//! worldgen-seam pattern (same as the `#181`/`#180` dispatch stubs). The pure
//! per-state predicate (`test_state`) and the codec values are fully ported
//! and tested.

pub mod all_of_predicate;
pub mod any_of_predicate;
pub mod block_predicate;
pub mod block_predicate_type;
pub mod combining_predicate;
pub mod has_sturdy_face_predicate;
pub mod inside_world_bounds_predicate;
pub mod matching_biomes_predicate;
pub mod matching_block_tag_predicate;
pub mod matching_blocks_predicate;
pub mod matching_fluids_predicate;
pub mod not_predicate;
pub mod replaceable_predicate;
pub mod solid_predicate;
pub mod state_testing_predicate;
pub mod true_block_predicate;
pub mod unobstructed_predicate;
pub mod would_survive_predicate;

pub use all_of_predicate::AllOfPredicate;
pub use any_of_predicate::AnyOfPredicate;
pub use block_predicate::{
    BlockPredicate, all_of, always_true, any_of, block_predicate_codec, inside_world, not,
};
pub use block_predicate_type::{BlockPredicateType, BlockPredicateTypeId};
pub use combining_predicate::CombiningPredicate;
pub use has_sturdy_face_predicate::HasSturdyFacePredicate;
pub use inside_world_bounds_predicate::InsideWorldBoundsPredicate;
pub use matching_biomes_predicate::MatchingBiomesPredicate;
pub use matching_block_tag_predicate::MatchingBlockTagPredicate;
pub use matching_blocks_predicate::MatchingBlocksPredicate;
pub use matching_fluids_predicate::MatchingFluidsPredicate;
pub use not_predicate::NotPredicate;
pub use replaceable_predicate::ReplaceablePredicate;
pub use solid_predicate::SolidPredicate;
pub use state_testing_predicate::StateTestingPredicate;
pub use true_block_predicate::TrueBlockPredicate;
pub use unobstructed_predicate::UnobstructedPredicate;
pub use would_survive_predicate::WouldSurvivePredicate;
