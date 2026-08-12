//! Port of `net.minecraft.world.level.levelgen.placement.PlacementModifierType`
//! (interface, 26.2).
//!
//! Java is the interface every placement modifier's `type()` returns; its
//! fifteen constants are `register(...)` calls into
//! `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE`, each holding the modifier's
//! `MapCodec`. The Rust port mirrors `Feature`'s identity split: the modifier's
//! type identity is the opaque `PlacementModifierTypeId` handle (the registry
//! element identity), and `placement_modifier_type(id)` resolves it to the
//! type behavior. The registration table is generated content — the `#181`
//! hub (same codegen as `Feature.register`) — so this core unit does NOT
//! hand-port the fifteen constants, and the per-type `MapCodec` is deferred
//! with the codec surface (`#126`). Until the generated table lands the lookup
//! panics unconditionally (the pre-wire stand-in); once wired, an unknown type
//! id throws `IllegalStateException` like Java's `Registry.getValueOrThrow`
//! (which throws only when the key is genuinely missing).

use std::fmt::Debug;

/// The fifteen `PlacementModifierTypes` constants — Paper's exact declaration
/// order in `PlacementModifierType.java` (the `BuiltInRegistries.
/// PLACEMENT_MODIFIER_TYPE` insertion order, so element ids 0..=14).
///
/// The concrete modifier units (filter/simple/repeating) report their identity
/// from these constants instead of inlining `PlacementModifierTypeId::new`.
/// These are pure registry-identity declarations mirroring
/// `BlockPredicateTypes`; the registration table itself (the `#181` generated
/// dispatch) and the per-type `MapCodec`s still defer with the codec surface
/// (`#126`, see the module doc and the `placement.core` STUB below).
pub struct PlacementModifierTypes;
impl PlacementModifierTypes {
    /// `register("block_predicate_filter", BlockPredicateFilter.CODEC)`.
    pub const BLOCK_PREDICATE_FILTER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(0, "minecraft:block_predicate_filter");
    /// `register("rarity_filter", RarityFilter.CODEC)`.
    pub const RARITY_FILTER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(1, "minecraft:rarity_filter");
    /// `register("surface_relative_threshold_filter", …)`.
    pub const SURFACE_RELATIVE_THRESHOLD_FILTER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(2, "minecraft:surface_relative_threshold_filter");
    /// `register("surface_water_depth_filter", …)`.
    pub const SURFACE_WATER_DEPTH_FILTER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(3, "minecraft:surface_water_depth_filter");
    /// `register("biome", BiomeFilter.CODEC)`.
    pub const BIOME_FILTER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(4, "minecraft:biome");
    /// `register("count", CountPlacement.CODEC)`.
    #[allow(dead_code)] // consumed by the CountPlacement unit once it lands
    pub const COUNT: PlacementModifierTypeId = PlacementModifierTypeId::new(5, "minecraft:count");
    /// `register("noise_based_count", NoiseBasedCountPlacement.CODEC)`.
    #[allow(dead_code)] // consumed by the NoiseBasedCountPlacement unit once it lands
    pub const NOISE_BASED_COUNT: PlacementModifierTypeId =
        PlacementModifierTypeId::new(6, "minecraft:noise_based_count");
    /// `register("noise_threshold_count", NoiseThresholdCountPlacement.CODEC)`.
    #[allow(dead_code)] // consumed by the NoiseThresholdCountPlacement unit once it lands
    pub const NOISE_THRESHOLD_COUNT: PlacementModifierTypeId =
        PlacementModifierTypeId::new(7, "minecraft:noise_threshold_count");
    /// `register("count_on_every_layer", CountOnEveryLayerPlacement.CODEC)`.
    pub const COUNT_ON_EVERY_LAYER: PlacementModifierTypeId =
        PlacementModifierTypeId::new(8, "minecraft:count_on_every_layer");
    /// `register("environment_scan", EnvironmentScanPlacement.CODEC)`.
    pub const ENVIRONMENT_SCAN: PlacementModifierTypeId =
        PlacementModifierTypeId::new(9, "minecraft:environment_scan");
    /// `register("heightmap", HeightmapPlacement.CODEC)`.
    pub const HEIGHTMAP: PlacementModifierTypeId =
        PlacementModifierTypeId::new(10, "minecraft:heightmap");
    /// `register("height_range", HeightRangePlacement.CODEC)`.
    pub const HEIGHT_RANGE: PlacementModifierTypeId =
        PlacementModifierTypeId::new(11, "minecraft:height_range");
    /// `register("in_square", InSquarePlacement.CODEC)`.
    pub const IN_SQUARE: PlacementModifierTypeId =
        PlacementModifierTypeId::new(12, "minecraft:in_square");
    /// `register("random_offset", RandomOffsetPlacement.CODEC)`.
    pub const RANDOM_OFFSET: PlacementModifierTypeId =
        PlacementModifierTypeId::new(13, "minecraft:random_offset");
    /// `register("fixed_placement", FixedPlacement.CODEC)`.
    pub const FIXED_PLACEMENT: PlacementModifierTypeId =
        PlacementModifierTypeId::new(14, "minecraft:fixed_placement");
}

/// `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All fifteen Paper entries are
/// registered, so every known location resolves; only the codec resolution
/// (not the registry) is gated by the `#126` codec surface.
#[allow(dead_code)] // consumed by the registry-by-name codec surface once wired
pub fn placement_modifier_type_by_name(name: &str) -> Option<PlacementModifierTypeId> {
    match name {
        "minecraft:block_predicate_filter" => Some(PlacementModifierTypes::BLOCK_PREDICATE_FILTER),
        "minecraft:rarity_filter" => Some(PlacementModifierTypes::RARITY_FILTER),
        "minecraft:surface_relative_threshold_filter" => {
            Some(PlacementModifierTypes::SURFACE_RELATIVE_THRESHOLD_FILTER)
        }
        "minecraft:surface_water_depth_filter" => {
            Some(PlacementModifierTypes::SURFACE_WATER_DEPTH_FILTER)
        }
        "minecraft:biome" => Some(PlacementModifierTypes::BIOME_FILTER),
        "minecraft:count" => Some(PlacementModifierTypes::COUNT),
        "minecraft:noise_based_count" => Some(PlacementModifierTypes::NOISE_BASED_COUNT),
        "minecraft:noise_threshold_count" => Some(PlacementModifierTypes::NOISE_THRESHOLD_COUNT),
        "minecraft:count_on_every_layer" => Some(PlacementModifierTypes::COUNT_ON_EVERY_LAYER),
        "minecraft:environment_scan" => Some(PlacementModifierTypes::ENVIRONMENT_SCAN),
        "minecraft:heightmap" => Some(PlacementModifierTypes::HEIGHTMAP),
        "minecraft:height_range" => Some(PlacementModifierTypes::HEIGHT_RANGE),
        "minecraft:in_square" => Some(PlacementModifierTypes::IN_SQUARE),
        "minecraft:random_offset" => Some(PlacementModifierTypes::RANDOM_OFFSET),
        "minecraft:fixed_placement" => Some(PlacementModifierTypes::FIXED_PLACEMENT),
        _ => None,
    }
}

/// The `PlacementModifierType<P>` registry element identity — the per-type
/// `u32` id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `FeatureId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlacementModifierTypeId {
    /// The per-type `u32` identity (insertion index in the modifier-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register("count",
    /// …)` → `minecraft:count`).
    pub location: &'static str,
}

impl PlacementModifierTypeId {
    /// `new PlacementModifierTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> PlacementModifierTypeId {
        PlacementModifierTypeId { id, location }
    }
}

/// `net.minecraft.world.level.levelgen.placement.PlacementModifierType<P>` —
/// the object-safe carrier of a modifier type's identity.
///
/// `P` is erased in Rust (like the `Feature` half of `ConfiguredFeature`); the
/// per-type `MapCodec<P>` (`PlacementModifierType.codec()`) lands with the
/// codec surface (`#126`) and the `#181` generated table. Until then this is a
/// marker: concrete modifier structs report their `PlacementModifierTypeId`
/// from `PlacementModifier::type_id`, and the registry holds the uniform
/// behavior-bearing reference.
pub trait PlacementModifierType: Debug + Send + Sync + 'static {}

/// Resolve a `PlacementModifierTypeId` to its type behavior.
///
/// STUB(mc.world.level.levelgen.placement.core) — the generated
/// `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE` table (emitted by `rivet-codegen`
/// per the `#181` manifest note). Panics unconditionally until the table is
/// wired; once wired, an unresolvable id throws `IllegalStateException` like
/// Java's `Registry.getValueOrThrow` (which throws only when the key is
/// genuinely missing).
pub fn placement_modifier_type(
    _id: &PlacementModifierTypeId,
) -> &'static dyn PlacementModifierType {
    // The generated table is not wired yet — this unconditional panic is the
    // pre-wire stand-in for the generated dispatch's unknown-id `getValueOrThrow`.
    panic!("Trying to access placement modifier type with no registered behavior (#181 codegen)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE` element ids equal the
        // insertion index in `PlacementModifierType.java`'s declaration order.
        let constants = [
            PlacementModifierTypes::BLOCK_PREDICATE_FILTER,
            PlacementModifierTypes::RARITY_FILTER,
            PlacementModifierTypes::SURFACE_RELATIVE_THRESHOLD_FILTER,
            PlacementModifierTypes::SURFACE_WATER_DEPTH_FILTER,
            PlacementModifierTypes::BIOME_FILTER,
            PlacementModifierTypes::COUNT,
            PlacementModifierTypes::NOISE_BASED_COUNT,
            PlacementModifierTypes::NOISE_THRESHOLD_COUNT,
            PlacementModifierTypes::COUNT_ON_EVERY_LAYER,
            PlacementModifierTypes::ENVIRONMENT_SCAN,
            PlacementModifierTypes::HEIGHTMAP,
            PlacementModifierTypes::HEIGHT_RANGE,
            PlacementModifierTypes::IN_SQUARE,
            PlacementModifierTypes::RANDOM_OFFSET,
            PlacementModifierTypes::FIXED_PLACEMENT,
        ];
        for (i, id) in constants.iter().enumerate() {
            assert_eq!(id.id, i as u32, "element id {i}");
        }
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            PlacementModifierTypes::COUNT_ON_EVERY_LAYER.location,
            "minecraft:count_on_every_layer"
        );
        assert_eq!(
            PlacementModifierTypes::ENVIRONMENT_SCAN.location,
            "minecraft:environment_scan"
        );
        assert_eq!(
            PlacementModifierTypes::RANDOM_OFFSET.location,
            "minecraft:random_offset"
        );
        assert_eq!(
            PlacementModifierTypes::FIXED_PLACEMENT.location,
            "minecraft:fixed_placement"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        let constants = [
            PlacementModifierTypes::BLOCK_PREDICATE_FILTER,
            PlacementModifierTypes::RARITY_FILTER,
            PlacementModifierTypes::SURFACE_RELATIVE_THRESHOLD_FILTER,
            PlacementModifierTypes::SURFACE_WATER_DEPTH_FILTER,
            PlacementModifierTypes::BIOME_FILTER,
            PlacementModifierTypes::COUNT,
            PlacementModifierTypes::NOISE_BASED_COUNT,
            PlacementModifierTypes::NOISE_THRESHOLD_COUNT,
            PlacementModifierTypes::COUNT_ON_EVERY_LAYER,
            PlacementModifierTypes::ENVIRONMENT_SCAN,
            PlacementModifierTypes::HEIGHTMAP,
            PlacementModifierTypes::HEIGHT_RANGE,
            PlacementModifierTypes::IN_SQUARE,
            PlacementModifierTypes::RANDOM_OFFSET,
            PlacementModifierTypes::FIXED_PLACEMENT,
        ];
        for id in constants {
            assert_eq!(placement_modifier_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(placement_modifier_type_by_name("minecraft:nope"), None);
        assert_eq!(placement_modifier_type_by_name("count"), None);
    }
}
