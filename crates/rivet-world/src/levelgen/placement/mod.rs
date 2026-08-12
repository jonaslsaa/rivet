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
//! lazy `Stream<BlockPos>`, so the port's `get_positions` draws eagerly and
//! returns a lazy `Box<dyn Iterator<Item = BlockPos> + 'a>` (Java's laziness in
//! *when* `getPositions` runs — its lazy `flatMap` invokes it per upstream
//! position, interleaved with placements — is reproduced by `PlacedFeature`'s
//! depth-first walk; the iterator form additionally keeps `RepeatingPlacement`'s
//! unbounded `count` from materializing a `count`-length `Vec`). See
//! `place_walk` there for the authoritative parity account (why the ordering of
//! RNG draws and level-state reads matters, and the #181 revisit note).

use crate::levelgen::synth::perlin_simplex_noise::PerlinSimplexNoise;
use rivet_util::random::LegacyRandomSource;
use rivet_util::worldgen_random::WorldgenRandom;
use std::sync::LazyLock;

/// `Biome.BIOME_INFO_NOISE` — `new PerlinSimplexNoise(new WorldgenRandom(new
/// LegacyRandomSource(2345L)), ImmutableList.of(0))`, marked
/// `@Deprecated(forRemoval = true)` in Java.
///
/// STUB(mc.world.level.biome.core) — the `Biome` value core (issue #178) owns
/// this static noise field. It has not landed yet, and the placement modifiers
/// (`mc.world.level.levelgen.placement.repeating`) sample `BIOME_INFO_NOISE` in
/// their `count` hooks, so it is declared HERE as a functional out-of-unit stub
/// built on the already-ported `synth::PerlinSimplexNoise` — the exact
/// seed/RNG construction from `Biome.java`'s static initializer. The
/// declaration deliberately lives in this consuming unit, NOT `biome.rs`, so it
/// cannot collide with the owning `biome.core` declaration (issue #178) when
/// that unit lands it; the placement unit then reads it through `crate::biome::`
/// and this stub is deleted.
pub static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

mod biome_filter;
mod block_predicate_filter;
mod cave_surface;
mod count_on_every_layer_placement;
mod count_placement;
mod environment_scan_placement;
mod fixed_placement;
mod height_range_placement;
mod heightmap_placement;
mod in_square_placement;
mod noise_based_count_placement;
mod noise_threshold_count_placement;
mod placed_feature;
mod placement_context;
mod placement_filter;
mod placement_modifier;
mod placement_modifier_type;
mod random_offset_placement;
mod rarity_filter;
mod repeating_placement;
mod surface_relative_threshold_filter;
mod surface_water_depth_filter;

pub use biome_filter::BiomeFilter;
pub use block_predicate_filter::BlockPredicateFilter;
pub use cave_surface::CaveSurface;
pub use count_on_every_layer_placement::CountOnEveryLayerPlacement;
pub use count_placement::{CountPlacement, count_placement_codec};
pub use environment_scan_placement::EnvironmentScanPlacement;
pub use fixed_placement::FixedPlacement;
pub use height_range_placement::HeightRangePlacement;
pub use heightmap_placement::HeightmapPlacement;
pub use in_square_placement::InSquarePlacement;
pub use noise_based_count_placement::{
    NoiseBasedCountPlacement, noise_based_count_placement_codec,
};
pub use noise_threshold_count_placement::{
    NoiseThresholdCountPlacement, noise_threshold_count_placement_codec,
};
pub use placed_feature::PlacedFeature;
pub use placement_context::PlacementContext;
pub use placement_filter::PlacementFilter;
pub use placement_modifier::{ErasedPlacementModifier, PlacementModifier, placement_get_positions};
pub use placement_modifier_type::{
    PlacementModifierType, PlacementModifierTypeId, placement_modifier_type,
};
pub use random_offset_placement::RandomOffsetPlacement;
pub use rarity_filter::RarityFilter;
pub use repeating_placement::RepeatingPlacement;
pub use surface_relative_threshold_filter::SurfaceRelativeThresholdFilter;
pub use surface_water_depth_filter::SurfaceWaterDepthFilter;
