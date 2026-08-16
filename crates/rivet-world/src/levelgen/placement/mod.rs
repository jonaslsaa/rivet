//! `net.minecraft.world.level.levelgen.placement` — placement modifiers and
//! placed features.
//!
//! Owned by the `mc.world.level.levelgen.placement.core` manifest unit (26.2):
//! `PlacedFeature.java`, `PlacementContext.java`, `PlacementFilter.java`,
//! `PlacementModifier.java`, `PlacementModifierType.java`, `package-info.java`.
//! This unit co-lands with `feature.core` (Java SCC: `PlacedFeature` refers to
//! `ConfiguredFeature`/`FeatureCountTracker`, and `Feature`'s registration
//! table + `PlacementModifierType`'s registration table are reverse edges of
//! the same generated-content hub).
//!
//! ## Placement modifier identity semantics
//!
//! Java `PlacementModifier` is an abstract class whose `type()` returns the
//! registry-held `PlacementModifierType<?>` handle, and `PlacementModifier.CODEC`
//! dispatches on that type (`BuiltInRegistries.PLACEMENT_MODIFIER_TYPE
//! .byNameCodec().dispatch(PlacementModifier::type, PlacementModifierType::codec)`).
//! Like `Feature`, the identity is a registry object. `PlacementModifier` is
//! the generic behavior contract (concrete modifier structs implement it; its
//! `get_positions` is generic over the random source, so it is not
//! object-safe), and `PlacedFeature` stores its heterogeneous modifier list
//! erased (`ErasedPlacementModifier`); the per-modifier dispatch is the `#181`
//! codegen match (`placement_get_positions`), exactly like `feature_place`.
//! The registration table (`PlacementModifierType.register` x 15) is generated
//! content — the `#181` hub — so this core unit does NOT hand-port it, and the
//! per-type `MapCodec` defers with the codec surface (`#126`).
//!
//! Every Java modifier draws eagerly *inside* `getPositions` and returns a
//! pure stream, so the port's `get_positions` returns an eager `Vec<BlockPos>`;
//! Java's laziness is in *when* `getPositions` runs — its lazy `flatMap` invokes
//! it per upstream position, interleaved with placements. `PlacedFeature`
//! reproduces that with a depth-first walk; see `place_walk` there for the
//! authoritative parity account (why the ordering of RNG draws and level-state
//! reads matters, and the #181 revisit note).

mod biome_filter;
mod block_predicate_filter;
mod cave_surface;
mod count_on_every_layer_placement;
mod environment_scan_placement;
mod fixed_placement;
mod height_range_placement;
mod heightmap_placement;
mod in_square_placement;
mod placed_feature;
mod placement_context;
mod placement_filter;
mod placement_modifier;
mod placement_modifier_type;
mod random_offset_placement;
mod rarity_filter;
mod surface_relative_threshold_filter;
mod surface_water_depth_filter;

pub use biome_filter::BiomeFilter;
pub use block_predicate_filter::BlockPredicateFilter;
pub use cave_surface::{CaveSurface, cave_surface_codec};
pub use count_on_every_layer_placement::CountOnEveryLayerPlacement;
pub use environment_scan_placement::EnvironmentScanPlacement;
pub use fixed_placement::FixedPlacement;
pub use height_range_placement::HeightRangePlacement;
pub use heightmap_placement::HeightmapPlacement;
pub use in_square_placement::InSquarePlacement;
pub use placed_feature::PlacedFeature;
pub use placement_context::PlacementContext;
pub use placement_filter::PlacementFilter;
pub use placement_modifier::{ErasedPlacementModifier, PlacementModifier, placement_get_positions};
pub use placement_modifier_type::{
    PlacementModifierType, PlacementModifierTypeId, placement_modifier_type,
};
pub use random_offset_placement::RandomOffsetPlacement;
pub use rarity_filter::RarityFilter;
pub use surface_relative_threshold_filter::SurfaceRelativeThresholdFilter;
pub use surface_water_depth_filter::SurfaceWaterDepthFilter;
