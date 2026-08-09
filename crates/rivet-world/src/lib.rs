//! `net.minecraft.world.level` — world/chunk layer.
//!
//! #108 (M1.1) — chunk wire format. This crate ports the pure value/wire
//! structures for the superflat chunk section plus the issue #100 content
//! construction:
//!
//! - `chunk::paletted_container` — `PalettedContainer<T>` (+ `Data`, the
//!   `PackedData` NBT form, `unpack`/`pack` re-encode)
//! - `chunk::palette` — `Palette<T>` + `SingleValuePalette`/`LinearPalette`/
//!   `HashMapPalette`/`GlobalPalette`, the `GlobalIdMap` surface, `IdForResult`
//! - `chunk::strategy` — `Strategy<T>` (block-states/biomes ladders)
//! - `chunk::configuration` — `Configuration` (Global/Simple)
//! - `chunk::level_chunk_section` — `LevelChunkSection` wire write/size/recalc
//! - `chunk::data_layer` — `DataLayer` (16³ light layer)
//! - `levelgen::heightmap` — `Heightmap`/`primeHeightmaps`
//! - `levelgen::feature` — the `feature.core` slice (the
//!   `mc.world.level.levelgen.feature.core` unit): `ConfiguredFeature` (and its
//!   erased wildcard), `FeatureId`, `FeatureBehavior`, `FeaturePlaceContext`,
//!   `FeatureCountTracker`; the `#181` codegen feature dispatch
//!   (`feature_place`) stays a STUB
//! - `levelgen::feature::configurations` — the `FeatureConfiguration` trait,
//!   `NoneFeatureConfiguration` and their DFU codecs (the
//!   `mc.world.level.levelgen.feature.configurations.core` unit), plus an
//!   out-of-unit proactive port of `ProbabilityFeatureConfiguration` (owned by
//!   the `.probabilityfeature` unit)
//! - `levelgen::carver` — the `ConfiguredWorldCarver` type shell (the
//!   `mc.world.level.levelgen.carver` unit): `CarverConfiguration` (the bound
//!   as a marker trait), `WorldCarverId`/`WorldCarverBehavior`, and the
//!   `ConfiguredWorldCarver` record; the `#180` carver algorithm and the
//!   `carve` surface stay a STUB
//! - `levelgen::generation_step` — the `GenerationStep.Decoration` enum,
//!   proactively ported from the pending `mc.world.level.levelgen.settings`
//!   unit (the settings wave must not re-port it)
//! - `levelgen::placement` — the placement core slice (the
//!   `mc.world.level.levelgen.placement.core` unit): `PlacedFeature`/
//!   `PlacementContext`/`PlacementFilter`/`PlacementModifier`/
//!   `PlacementModifierType`; the `#181` codegen modifier dispatch
//!   (`placement_get_positions`) stays a STUB
//! - `levelgen::world_generation_context` — the `WorldGenerationContext`
//!   minY/height window (the `mc.world.level.levelgen.noise` unit's class; only
//!   the Paper `level()` accessor defers — RivetTodo #232)
//! - `lighting::light_update_data` — the light payload producer
//! - `superflat` — the deterministic single-stone superflat content builder
//!   (issue #100) that feeds the #94 `ClientboundLevelChunkWithLightPacket`.
//!
//! Bit packing lives in `rivet-util` (`SimpleBitStorage`/`ZeroBitStorage`).
//! No world state or chunk-send plumbing here yet (#100 keeps the content
//! construction pure; `PlayerChunkSender` is deferred).
//!
//! #232 (M2) — the Level value slice. `level` ports the non-ticking
//! `LevelHeightAccessor`/`BlockGetter`/`LevelReader`/`LevelAccessor`/`Level`
//! interface chain plus the `LevelData` game-time seam (`getGameTime`) and
//! re-exports the registry-owned `ChunkPos` (issue #125); the concrete world
//! (`ServerLevel`) lives in `rivet-server`.

pub mod chunk;
pub mod level;
pub mod levelgen;
pub mod lighting;
pub mod superflat;
