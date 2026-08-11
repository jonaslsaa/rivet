//! Port of `net.minecraft.world.level.levelgen.blockpredicates.BlockPredicateType`
//! (interface, 26.2).
//!
//! Java is the interface every predicate's `type()` returns; its fourteen
//! constants are `register(...)` calls into
//! `BuiltInRegistries.BLOCK_PREDICATE_TYPE` (each holding the predicate's
//! `MapCodec`), in this exact declaration order. The Rust port mirrors
//! `Feature`/`PlacementModifierType`'s identity split: the predicate's type
//! identity is the opaque [`BlockPredicateTypeId`] handle (the registry element
//! identity — element id == insertion index), and [`BlockPredicateType`] is the
//! behavior-carrying marker (the object-safe carrier concrete predicates are
//! keyed by). The per-type `MapCodec`s are resolved by the `#399` dispatch
//! table in `block_predicate`, not stored on the id or the trait.
//!
//! All fourteen Paper constants are declared with their exact registry
//! identity/declaration order; only the five in-scope predicates
//! (`inside_world_bounds`, `any_of`, `all_of`, `not`, `true`) have codecs
//! wired — dispatching to the remaining nine fails explicitly
//! (RivetTodo #399, see `block_predicate::codec_for_type`).

use std::fmt::Debug;

/// `BlockPredicateType<P>` — the object-safe carrier of a predicate type's
/// identity.
///
/// `P` is erased in Rust (like the `Feature` half of `ConfiguredFeature`); the
/// per-type `MapCodec<P>` is resolved by the `#399` dispatch table, not
/// through this trait.
pub trait BlockPredicateType: Debug + Send + Sync + 'static {}

/// The `BlockPredicateType<P>` registry element identity — the per-type `u32`
/// id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `FeatureId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockPredicateTypeId {
    /// The per-type `u32` identity (insertion index in the predicate-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "all_of", …)` → `minecraft:all_of`).
    pub location: &'static str,
}

impl BlockPredicateTypeId {
    /// `new BlockPredicateTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> BlockPredicateTypeId {
        BlockPredicateTypeId { id, location }
    }
}

/// The fourteen `BlockPredicateTypes` constants — Paper's exact declaration
/// order in `BlockPredicateType.java` (the `BuiltInRegistries.BLOCK_PREDICATE_TYPE`
/// insertion order, so element ids 0..=13).
pub struct BlockPredicateTypes;
impl BlockPredicateTypes {
    /// `register("matching_blocks", MatchingBlocksPredicate.CODEC)`.
    pub const MATCHING_BLOCKS: BlockPredicateTypeId =
        BlockPredicateTypeId::new(0, "minecraft:matching_blocks");
    /// `register("matching_block_tag", MatchingBlockTagPredicate.CODEC)`.
    pub const MATCHING_BLOCK_TAG: BlockPredicateTypeId =
        BlockPredicateTypeId::new(1, "minecraft:matching_block_tag");
    /// `register("matching_fluids", MatchingFluidsPredicate.CODEC)`.
    pub const MATCHING_FLUIDS: BlockPredicateTypeId =
        BlockPredicateTypeId::new(2, "minecraft:matching_fluids");
    /// `register("matching_biomes", MatchingBiomesPredicate.CODEC)`.
    pub const MATCHING_BIOMES: BlockPredicateTypeId =
        BlockPredicateTypeId::new(3, "minecraft:matching_biomes");
    /// `register("has_sturdy_face", HasSturdyFacePredicate.CODEC)`.
    pub const HAS_STURDY_FACE: BlockPredicateTypeId =
        BlockPredicateTypeId::new(4, "minecraft:has_sturdy_face");
    /// `register("solid", SolidPredicate.CODEC)`.
    pub const SOLID: BlockPredicateTypeId = BlockPredicateTypeId::new(5, "minecraft:solid");
    /// `register("replaceable", ReplaceablePredicate.CODEC)`.
    pub const REPLACEABLE: BlockPredicateTypeId =
        BlockPredicateTypeId::new(6, "minecraft:replaceable");
    /// `register("would_survive", WouldSurvivePredicate.CODEC)`.
    pub const WOULD_SURVIVE: BlockPredicateTypeId =
        BlockPredicateTypeId::new(7, "minecraft:would_survive");
    /// `register("inside_world_bounds", InsideWorldBoundsPredicate.CODEC)`.
    pub const INSIDE_WORLD_BOUNDS: BlockPredicateTypeId =
        BlockPredicateTypeId::new(8, "minecraft:inside_world_bounds");
    /// `register("any_of", AnyOfPredicate.CODEC)`.
    pub const ANY_OF: BlockPredicateTypeId = BlockPredicateTypeId::new(9, "minecraft:any_of");
    /// `register("all_of", AllOfPredicate.CODEC)`.
    pub const ALL_OF: BlockPredicateTypeId = BlockPredicateTypeId::new(10, "minecraft:all_of");
    /// `register("not", NotPredicate.CODEC)`.
    pub const NOT: BlockPredicateTypeId = BlockPredicateTypeId::new(11, "minecraft:not");
    /// `register("true", TrueBlockPredicate.CODEC)`.
    pub const TRUE: BlockPredicateTypeId = BlockPredicateTypeId::new(12, "minecraft:true");
    /// `register("unobstructed", UnobstructedPredicate.CODEC)`.
    pub const UNOBSTRUCTED: BlockPredicateTypeId =
        BlockPredicateTypeId::new(13, "minecraft:unobstructed");
}

/// `BuiltInRegistries.BLOCK_PREDICATE_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All fourteen Paper entries are
/// registered (matching Java's `registerSimple`-populated registry), so every
/// known location resolves; only the codec resolution (not the registry) is
/// gated by the `#399` scope.
pub fn block_predicate_type_by_name(name: &str) -> Option<BlockPredicateTypeId> {
    match name {
        "minecraft:matching_blocks" => Some(BlockPredicateTypes::MATCHING_BLOCKS),
        "minecraft:matching_block_tag" => Some(BlockPredicateTypes::MATCHING_BLOCK_TAG),
        "minecraft:matching_fluids" => Some(BlockPredicateTypes::MATCHING_FLUIDS),
        "minecraft:matching_biomes" => Some(BlockPredicateTypes::MATCHING_BIOMES),
        "minecraft:has_sturdy_face" => Some(BlockPredicateTypes::HAS_STURDY_FACE),
        "minecraft:solid" => Some(BlockPredicateTypes::SOLID),
        "minecraft:replaceable" => Some(BlockPredicateTypes::REPLACEABLE),
        "minecraft:would_survive" => Some(BlockPredicateTypes::WOULD_SURVIVE),
        "minecraft:inside_world_bounds" => Some(BlockPredicateTypes::INSIDE_WORLD_BOUNDS),
        "minecraft:any_of" => Some(BlockPredicateTypes::ANY_OF),
        "minecraft:all_of" => Some(BlockPredicateTypes::ALL_OF),
        "minecraft:not" => Some(BlockPredicateTypes::NOT),
        "minecraft:true" => Some(BlockPredicateTypes::TRUE),
        "minecraft:unobstructed" => Some(BlockPredicateTypes::UNOBSTRUCTED),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.BLOCK_PREDICATE_TYPE` element ids equal the
        // insertion index in `BlockPredicateType.java`'s declaration order.
        assert_eq!(BlockPredicateTypes::MATCHING_BLOCKS.id, 0);
        assert_eq!(BlockPredicateTypes::MATCHING_BLOCK_TAG.id, 1);
        assert_eq!(BlockPredicateTypes::MATCHING_FLUIDS.id, 2);
        assert_eq!(BlockPredicateTypes::MATCHING_BIOMES.id, 3);
        assert_eq!(BlockPredicateTypes::HAS_STURDY_FACE.id, 4);
        assert_eq!(BlockPredicateTypes::SOLID.id, 5);
        assert_eq!(BlockPredicateTypes::REPLACEABLE.id, 6);
        assert_eq!(BlockPredicateTypes::WOULD_SURVIVE.id, 7);
        assert_eq!(BlockPredicateTypes::INSIDE_WORLD_BOUNDS.id, 8);
        assert_eq!(BlockPredicateTypes::ANY_OF.id, 9);
        assert_eq!(BlockPredicateTypes::ALL_OF.id, 10);
        assert_eq!(BlockPredicateTypes::NOT.id, 11);
        assert_eq!(BlockPredicateTypes::TRUE.id, 12);
        assert_eq!(BlockPredicateTypes::UNOBSTRUCTED.id, 13);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(BlockPredicateTypes::ALL_OF.location, "minecraft:all_of");
        assert_eq!(
            BlockPredicateTypes::INSIDE_WORLD_BOUNDS.location,
            "minecraft:inside_world_bounds"
        );
        assert_eq!(BlockPredicateTypes::NOT.location, "minecraft:not");
        assert_eq!(BlockPredicateTypes::TRUE.location, "minecraft:true");
        assert_eq!(BlockPredicateTypes::ANY_OF.location, "minecraft:any_of");
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            BlockPredicateTypes::MATCHING_BLOCKS,
            BlockPredicateTypes::MATCHING_BLOCK_TAG,
            BlockPredicateTypes::MATCHING_FLUIDS,
            BlockPredicateTypes::MATCHING_BIOMES,
            BlockPredicateTypes::HAS_STURDY_FACE,
            BlockPredicateTypes::SOLID,
            BlockPredicateTypes::REPLACEABLE,
            BlockPredicateTypes::WOULD_SURVIVE,
            BlockPredicateTypes::INSIDE_WORLD_BOUNDS,
            BlockPredicateTypes::ANY_OF,
            BlockPredicateTypes::ALL_OF,
            BlockPredicateTypes::NOT,
            BlockPredicateTypes::TRUE,
            BlockPredicateTypes::UNOBSTRUCTED,
        ] {
            assert_eq!(block_predicate_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(block_predicate_type_by_name("minecraft:nope"), None);
        assert_eq!(block_predicate_type_by_name("all_of"), None);
    }
}
