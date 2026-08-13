//! `net.minecraft.world.level.levelgen` — the noise-based chunk generator
//! slice (issues #183/#185, `mc.world.level.levelgen.noisegen` unit).
//!
//! Port of the seven-file class-level SCC that rides with the generator
//! classes:
//!
//! - [`Aquifer`] — the fluid-aquifer filler (`computeSubstance`/`FluidPicker`/
//!   `FluidStatus` + the `NoiseBasedAquifer` 4-closest-cell pressure model).
//! - [`NoiseBasedChunkGenerator`] — the `ChunkGenerator` subclass (the fluid
//!   picker, the noise-column iteration, the debug-screen info, the deferred
//!   biome/surface/carver surface — STUBs for the unported world types).
//! - [`NoiseChunk`] — the per-chunk interpolation context (`FunctionContext` +
//!   `ContextProvider`), the `wrap` marker dispatch, the
//!   `preliminarySurfaceLevel` cache, and the inner
//!   `NoiseInterpolator`/`FlatCache`/`Cache2D`/`CacheOnce`/`CacheAllInCell`/
//!   `BlendAlpha`/`BlendDensity`/`BlendOffset` density functions.
//! - [`NoiseGeneratorSettings`] — the 11-field record + `DIRECT_CODEC` +
//!   the seven preset keys + `bootstrap`/`dummy`.
//! - [`NoiseRouterData`] — the shared noise/function registry keys and the
//!   `overworld`/`nether`/`end`/`caves`/`floatingIslands` router builders.
//! - [`ore_veinifier::create`] — the ore-vein `BlockStateFiller`.
//! - [`RandomState`] — the per-world random/noise wiring (the
//!   `NoiseWiringHelper`/noise-flattener visitors, the
//!   `Climate.Sampler`, the aquifer/ore random factories).
//!
//! ## Reused layers
//!
//! The `noise` value layer (issue #177: `DensityFunction`/`DensityFunctions`/
//! `NoiseRouter`/`NoiseSettings`/`Noises`), the `synth` primitives, the
//! `blending`/`random`/`carver` leaves, the `biome` climate slice, and the
//! `data::worldgen` prerequisites (`BootstrapContext`/`TerrainProvider`/
//! `NoiseData` — the `mc.data.worldgen.prereq` unit) are all ported by their
//! owning units and consumed here.
//!
//! ## Deferred seams (sparse issue-linked markers)
//!
//! - `SurfaceRules` (`mc.world.level.levelgen.surface`): the
//!   `NoiseGeneratorSettings.surfaceRule` field, `SurfaceRuleData`, and the
//!   `SurfaceSystem` (`RandomState.surfaceSystem`/
//!   `NoiseBasedChunkGenerator.buildSurface`) are absorbed as `STUB`s until the
//!   surface unit lands (MANIFEST note).
//! - `BelowZeroRetrogen` (`settings`): `doCreateBiomes`'s biome-resolver seam
//!   is a `STUB`.
//! - `MaterialRuleList` (`mc.world.level.levelgen.material`): `NoiseChunk`'s
//!   block-state-rule list is a `STUB` value struct (the 2-line iteration) here.
//! - `Beardifier` (structure unit): the real `forStructuresInChunk` defers;
//!   `NoiseChunk`/`NoiseBasedChunkGenerator` use the `BeardifierMarker` value
//!   shell (RivetTodo #177).
//! - `OverworldBiomeBuilder` (biome unit): only the two leaves this SCC reads
//!   are inlined — `isDeepDarkRegion` (in `Aquifer`) and `spawnTarget()` (in
//!   `NoiseGeneratorSettings`).
//! - The still-unported world/level surfaces `NoiseBasedChunkGenerator` touches
//!   (`WorldGenRegion`, `StructureManager`, `BiomeSource`, `NaturalSpawner`,
//!   `NoiseColumn`) defer with their owning units; the methods that need them
//!   are `STUB`/`todo!`-free markers (see `noise_based_chunk_generator`). The
//!   surfaces that have landed (`BiomeManager`, `CarvingContext`/`CarvingMask`,
//!   `BiomeGenerationSettings`, `ConfiguredWorldCarver`, `LevelChunkSection`,
//!   `ProtoChunk`, `Heightmap`) are consumed directly.

pub mod aquifer;
pub mod column_pos;
pub mod noise_based_chunk_generator;
pub mod noise_chunk;
pub mod noise_generator_settings;
pub mod noise_router_data;
pub mod ore_veinifier;
pub mod random_state;

pub use aquifer::{Aquifer, FluidPicker, FluidStatus};
pub use column_pos::ColumnPos;
pub use noise_chunk::{BlockStateFiller, NoiseChunk};
pub use noise_generator_settings::NoiseGeneratorSettings;
pub use ore_veinifier::VeinType;
pub use random_state::RandomState;
