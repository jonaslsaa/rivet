//! The generated-world pipeline spine (issue #185) — the server-side
//! realization that replaces the superflat fixture with real generated-world
//! ownership, without ever serving a ProtoChunk as FULL.
//!
//! ## The spine
//!
//! Given a seed, [`OverworldGenerator`] realizes the OVERWORLD
//! `NoiseGeneratorSettings`/`NoiseBasedChunkGenerator` from the merged worldgen
//! registries (one leak of the immutable `RegistryAccess` + `RandomState` per
//! world/seed — see [`RandomState`]'s borrow), and [`OverworldNoiseBiomeSource`]
//! adapts the overworld `MultiNoiseBiomeSource` over the realized climate
//! sampler. [`GenerationChunkHolder`] owns a real `ProtoChunk` and drives it
//! BIOMES→NOISE→SURFACE→CARVERS through the `GENERATION_PYRAMID` executor.
//!
//! **This is the standalone foundation, not yet the live ticket path.** No
//! production caller (ticket, `ChunkMap`, or boot) creates a holder yet — the
//! `RivetTodo(#185)`s mark the wiring that lands with the `.chunk.generator`
//! pipeline unit. The leak of the per-world registries/`RandomState` is the
//! mechanism that keeps the worldgen objects `'static` for the holder closures;
//! RivetTodo(#185): the world-level registry-ownership unit replaces it with a
//! reclaimable per-world owner before this is wired to the live path.
//!
//! ## The typed downstream boundary
//!
//! `WorldGenContext::generate_through` supports generation through `CARVERS` —
//! the BIOMES→NOISE→SURFACE→CARVERS task bodies are wired to the real Paper
//! drivers (`fillFromNoise` / `buildSurface` / `applyCarvers`), so an EMPTY
//! chunk can reach CARVERS. The FEATURES task body is also wired: the
//! caller-supplied [`GenerationChunkHolder::new`] features closure runs Java's
//! `ChunkStatusTasks.generateFeatures` — the `Heightmap.primeHeightmaps(chunk,
//! FINAL_HEIGHTMAPS)` priming and the `addVanillaDecorations` prologue over a
//! bounded region-backed FEATURES dependency window — the decoration-seed
//! derivation (`SectionPos.of(centerPos, level.getMinSectionY()).origin()` fed
//! to `setDecorationSeed`), a `WorldGenRegion` that borrows the center chunk
//! and owns the complete 17x17 dependency cache (CARVERS at distances 0/1,
//! STRUCTURE_STARTS through distance 8), and the Paper-order 3x3 biome-union
//! gather + `retainAll`. It then resolves generation settings for the FULL
//! `biomeSource.possibleBiomes()` list in source order and builds the
//! FeatureSorter once from it (Paper's `ChunkGenerator.featuresPerStep`,
//! `ChunkGenerator.java` 97-100 — the 3x3 union only picks which feature
//! indices execute per step). The generated feature tables cover EVERY
//! overworld possible biome (55 — the full list, not the reachable subset),
//! so the full list resolves. FEATURES continues only for the fixture that
//! explicitly omits the structure-decoration loop; that loop is not modeled,
//! and feature seeds therefore use Paper's per-step global feature index with
//! no fabricated global offset. The normal unavailable boundary returns
//! `GenError::StructureDecorationUnavailable` before feature RNG, placement,
//! or tick side effects. The chunk stays CARVERS on refusal.
//! The INITIALIZE_LIGHT/
//! LIGHT steps are executor-wired but engine-gated (the holder wires no light
//! engine, so it cannot reach LIGHT).
//! Everything the value layer does not wire is refused *before* running work: a
//! path through a light step with no engine is refused as
//! `GenError::LightEngineMissing`, and a target past LIGHT (SPAWN/FULL) is out
//! of range (`GenError::UnsupportedStatus`). The holder's
//! [`GenerationChunkHolder::generate_through`] surfaces these as typed
//! [`GeneratedChunkError::Generation`] / [`GeneratedChunkError::UnsupportedStatus`]
//! rather than stamping a status that was never generated. And a generated
//! chunk can never enter the server authority: [`ChunkMap::install`] accepts
//! only a `LevelChunk` (the FULL chunk type), and no conversion from a sub-FULL
//! `ProtoChunk` exists — the [`GenerationChunkHolder::to_level_chunk`] gate
//! fails loudly with [`GeneratedChunkError::InstallRequiresFull`] instead of
//! fabricating a FULL chunk or falling back to superflat.
//!
//! ## The `GenerationChunkHolderView` seam
//!
//! The `WorldGenRegion` view contract is generic over the chunk value types
//! (`GenerationChunkHolderView<T, B, S>`), so the worldgen executor's region
//! (`BlockState`/`section_reconstruction::BiomeId`) uses the generic chunk-view
//! methods while the dense server region keeps its block-state `WorldGenLevel`
//! facade on the `StateId`/`ServerBiomeId`/`StructureKey` specialization. The
//! FEATURES body composes its full dependency window through [`CenterHolder`]
//! (which borrows the center chunk's base) and [`OwnedHolder`] (which owns the
//! ring chunks) — see [`compose_feature_region`]. What still defers (RivetTodo
//! #185) is the FULL-reconstruction bridge: `ChunkAccess::map_values` is wired
//! only from `LevelChunk::from_bridge`, so a sub-FULL `ProtoChunk` still cannot
//! be converted into the dense server chunk and never enters the ChunkMap
//! authority ([`GenerationChunkHolder::to_level_chunk`] fails loudly instead).
//! The holder hands out the chunk's status and typed generation results too.
//!
//! Ownership follows OWNERSHIP.md: the generator/biome source are immutable
//! per-world config shared by `Arc` (no `Arc<RwLock>` game state — the only
//! interior mutability is `RandomState`'s own uncontended noise cache), the
//! holder and its `ProtoChunk` live on the sync tick thread by value.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use rivet_registry::Registry;
use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::builder::RegistryBuilder;
use rivet_registry::core::BlockPos;
use rivet_registry::core::ChunkPos;
use rivet_registry::core::SectionPos;
use rivet_registry::generated::biomes::BIOME_BY_ID;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::generated::feature_data::{
    BIOME_GENERATION_SETTINGS_BY_NAME, CONFIGURED_FEATURE_BY_NAME, PLACED_FEATURE_BY_NAME,
};
use rivet_registry::holder::Holder;
use rivet_registry::holder::RegistryId;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::registry_ops::RegistryOps;
use rivet_registry::{Identifier, RegistrationInfo, ResourceKey};
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;
use rivet_util::StaticCache2D;
use rivet_util::WorldgenRandom;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::random_source::random_support;
use rivet_world::biome::BiomeManager;
use rivet_world::biome::BiomeResolver;
use rivet_world::biome::BiomeSource;
use rivet_world::biome::biome_generation_settings::{BiomeGenerationSettings, PlainBuilder};
use rivet_world::biome::biome_manager::NoiseBiomeSource;
use rivet_world::biome::climate::Sampler;
use rivet_world::biome::feature_sorter::build_features_per_step;
use rivet_world::biome::generated_biome_source::{dense_biome_id, overworld_biome_source};
use rivet_world::biome::multi_noise_biome_source::MultiNoiseBiomeSource;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID, GenError, WorldGenContext};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_height_accessor;
use rivet_world::levelgen::blending::blender::Blender;
use rivet_world::levelgen::feature::configurations::{
    FeatureConfiguration, NoneFeatureConfiguration,
};
use rivet_world::levelgen::feature::lake_feature::lake_configuration_codec;
use rivet_world::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use rivet_world::levelgen::feature::{
    ConfiguredFeatureErased, FeatureId, feature_id_from_registry_name,
};
use rivet_world::levelgen::generation_step::Decoration;
use rivet_world::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types};
use rivet_world::levelgen::noise::registry_keys::NOISE_SETTINGS;
use rivet_world::levelgen::noisegen::noise_based_chunk_generator::NoiseBasedChunkGenerator;
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::random_state::RandomState;
use rivet_world::levelgen::placement::{
    ErasedPlacementModifier, PlacedFeature, biome_filter_codec, block_predicate_filter_codec,
    count_on_every_layer_placement_codec, count_placement_codec, environment_scan_placement_codec,
    fixed_placement_codec, height_range_placement_codec, heightmap_placement_codec,
    in_square_placement_codec, noise_based_count_placement_codec,
    noise_threshold_count_placement_codec, random_offset_placement_codec, rarity_filter_codec,
    surface_relative_threshold_filter_codec, surface_water_depth_filter_codec,
};
use rivet_world::levelgen::world_generation_context::WorldGenerationContext;

use crate::server::level::level_chunk::{LevelChunk, StructureKey};
use crate::server::level::world_gen_region::{
    CenterHolder, GenerationChunkHolderView, OwnedHolder, WorldGenRegion,
};

/// The overworld generated-chunk error surface — every failure is typed, never
/// a silent fallback.
#[derive(Debug)]
pub enum GeneratedChunkError {
    /// The status executor refused the promotion: a missing data prerequisite
    /// (`GenError::BiomesNotGenerated`/`DataNotCarried`), a demotion, or a
    /// wired-task mismatch. The chunk is left untouched.
    Generation(GenError),
    /// A target past `LIGHT` — the executor rejected it before running any
    /// work. Naming the requested status makes the downstream boundary explicit.
    /// (A target through a light step with no engine is instead refused as
    /// `GenError::LightEngineMissing`, and the wired FEATURES rung stops at
    /// the first selected path outside the decoded lake slice — see
    /// [`GenerationChunkHolder::new`].)
    UnsupportedStatus(ChunkStatus),
    /// The genuine-FULL-only install gate: a generated chunk is a `ProtoChunk`
    /// through `SURFACE` and cannot be converted into the `LevelChunk` (FULL)
    /// that `ChunkMap::install` requires. The status is the chunk's actual
    /// persisted status — never `FULL` (the spine does not fabricate it).
    InstallRequiresFull(ChunkStatus),
}

impl fmt::Display for GeneratedChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeneratedChunkError::Generation(inner) => {
                write!(f, "chunk generation failed: {inner}")
            }
            GeneratedChunkError::UnsupportedStatus(status) => write!(
                f,
                "generating to {status:?} is unsupported: the SPAWN/FULL stages are unwired (RivetTodo #185)"
            ),
            GeneratedChunkError::InstallRequiresFull(status) => write!(
                f,
                "a generated chunk at {status:?} cannot enter the ChunkMap: only a genuine FULL LevelChunk may be installed"
            ),
        }
    }
}

impl std::error::Error for GeneratedChunkError {}

/// The per-world OVERWORLD generator realization — `NoiseBasedChunkGenerator`
/// resolved from the merged worldgen registries for a seed, plus the realized
/// `RandomState` and overworld biome source.
///
/// `RandomState` borrows the registries it resolves, so the immutable worldgen
/// `RegistryAccess` and the `RandomState` are leaked once per world/seed
/// (`Box::leak` → `'static`); the value shell's `NoiseBasedChunkGenerator`
/// holds its settings as a `Holder::Direct` (a `Reference` holder would panic
/// without a threaded `HolderLookup` — see `settings_value`).
pub struct OverworldGenerator {
    generator: NoiseBasedChunkGenerator,
    random_state: &'static RandomState<'static>,
    biome_source: OverworldNoiseBiomeSource,
    /// The leaked worldgen `RegistryAccess` — the `registryAccess()` back
    /// reference the FEATURES decoration body (and the `WorldGenRegion` it
    /// constructs) resolves the placed-feature registry through, and the
    /// `lookupOrThrow(Registries.STRUCTURE)`/`Registries.PLACED_FEATURE`
    /// lookups Java's `addVanillaDecorations` performs. Stored alongside the
    /// random state it already shares the leak of (see [`OverworldGenerator::new`]).
    access: &'static RegistryAccess,
    seed: i64,
}

impl OverworldGenerator {
    /// Realize the OVERWORLD generator for `seed`.
    ///
    /// `build_worldgen_registries` bundles the NOISE/DENSITY_FUNCTION/BIOME/
    /// NOISE_SETTINGS registries; `RandomState::create_from_provider` resolves
    /// the `overworld` settings preset through `NOISE_SETTINGS` and wires the
    /// router/sampler/surface system. The generator's settings holder is the
    /// resolved `overworld` value (Direct), matching the shell's value model.
    pub fn new(seed: i64) -> Self {
        let access: &'static RegistryAccess = Box::leak(Box::new(build_worldgen_registries()));
        let random_state: &'static RandomState<'static> = Box::leak(Box::new(
            RandomState::create_from_provider(access, &OVERWORLD, seed),
        ));
        // The settings the random state resolved through `NOISE_SETTINGS`; the
        // generator needs an owned `Holder::Direct` (see the module doc).
        let settings = {
            let settings_registry = access.lookup_or_throw(&NOISE_SETTINGS);
            let settings_holder = settings_registry.get_or_throw(&OVERWORLD);
            settings_holder.value(settings_registry).clone()
        };
        let generator = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
        OverworldGenerator {
            generator,
            random_state,
            biome_source: OverworldNoiseBiomeSource::new(random_state),
            access,
            seed,
        }
    }

    /// The seed this generator was realized for.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// The leaked worldgen `RegistryAccess` — the FEATURES decoration body's
    /// registry back-reference (`registryAccess()`, `lookupOrThrow`).
    pub fn registry_access(&self) -> &'static RegistryAccess {
        self.access
    }

    /// The value shell — the source of truth for the real world-surface bodies
    /// ([`ChunkGenerator`] delegates to it below).
    pub fn generator(&self) -> &NoiseBasedChunkGenerator {
        &self.generator
    }

    /// The realized per-world random state (leaked `'static`, shared by all
    /// holders of this world).
    pub fn random_state(&self) -> &'static RandomState<'static> {
        self.random_state
    }

    /// The overworld biome source over this world's climate sampler.
    pub fn biome_source(&self) -> &OverworldNoiseBiomeSource {
        &self.biome_source
    }

    /// Create a generation holder for `pos`, wiring the BIOMES→NOISE executor
    /// closures over the shared worldgen objects (`self` is `Arc`-shared so the
    /// `'static` closures capture a cheap clone).
    pub fn create_holder(self: &Arc<Self>, pos: ChunkPos) -> GenerationChunkHolder {
        GenerationChunkHolder::new(pos, Arc::clone(self))
    }

    /// Create a holder with an explicitly proven structure-consumed feature
    /// count. Production callers use [`Self::create_holder`], which leaves the
    /// count unavailable and therefore refuses FEATURES before feature RNG.
    /// Fixture/oracle callers may pass `Some(0)` only when they have verified
    /// that this chunk's structure decoration loop consumed no feature index.
    pub fn create_holder_with_structure_feature_count(
        self: &Arc<Self>,
        pos: ChunkPos,
        structure_feature_count: Option<usize>,
    ) -> GenerationChunkHolder {
        GenerationChunkHolder::new_with_structure_feature_count(
            pos,
            Arc::clone(self),
            structure_feature_count,
        )
    }
}

/// The `ChunkGenerator` realization — delegates the abstract world-surface
/// reads to the noisegen value shell's real bodies, resolving the trait seams
/// the `.chunk.generator` module doc's `RivetTodo(#185)` reconciliation note
/// requires (no separate source of truth).
impl ChunkGenerator for OverworldGenerator {
    fn get_min_y(&self) -> i32 {
        self.generator.get_min_y()
    }

    fn get_gen_depth(&self) -> i32 {
        self.generator.get_gen_depth()
    }

    fn get_sea_level(&self) -> i32 {
        self.generator.get_sea_level()
    }

    fn get_base_height(
        &self,
        x: i32,
        z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> i32 {
        self.generator
            .get_base_height(x, z, ty, height_accessor, random_state)
    }

    fn get_base_column(
        &self,
        x: i32,
        z: i32,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        self.generator
            .get_base_column(x, z, height_accessor, random_state)
    }

    fn add_debug_screen_info(
        &self,
        result: &mut Vec<String>,
        random_state: &RandomState,
        feet_pos: &BlockPos,
    ) {
        self.generator
            .add_debug_screen_info(result, random_state, feet_pos)
    }
}

/// The overworld `NoiseBiomeSource` adapter — the `MultiNoiseBiomeSource`
/// (Java's `biomeSource` field, built from the `overworld` preset table) over
/// this world's climate `Sampler` (Java's `randomState.sampler()`). Shared
/// immutably by `Arc`; also a `BiomeResolver` so the BIOMES step can drive
/// `fill_biomes_from_noise` with the same source Paper uses.
#[derive(Debug, Clone)]
pub struct OverworldNoiseBiomeSource {
    source: MultiNoiseBiomeSource,
    sampler: Sampler,
}

impl OverworldNoiseBiomeSource {
    /// Build the source over the random state's realized sampler.
    pub fn new(random_state: &RandomState) -> Self {
        OverworldNoiseBiomeSource {
            source: overworld_biome_source(),
            sampler: random_state.sampler().clone(),
        }
    }

    /// The climate sampler this source samples with.
    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }

    /// `BiomeSource.possibleBiomes()` — the overworld source's possible-biome
    /// set (the `retainAll` argument of Java's `addVanillaDecorations` biome
    /// union: `possibleBiomes.retainAll(this.biomeSource.possibleBiomes())`).
    pub fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        self.source.possible_biomes()
    }
}

impl NoiseBiomeSource for OverworldNoiseBiomeSource {
    /// `BiomeManager.NoiseBiomeSource.getNoiseBiome` — samples this world's
    /// sampler and resolves the biome through the overworld table.
    fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Holder<BiomeId> {
        self.source
            .get_noise_biome(quart_x, quart_y, quart_z, &self.sampler)
    }
}

impl BiomeResolver for OverworldNoiseBiomeSource {
    /// `BiomeResolver.getNoiseBiome(qx, qy, qz, sampler)` — the `BiomeSource`
    /// default (`getNoiseBiome(sampler.sample(...))`), with the resolver's own
    /// table.
    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId> {
        self.source
            .get_noise_biome(quart_x, quart_y, quart_z, sampler)
    }
}

/// A real generated chunk being driven through the pipeline — owns the
/// worldgen `ProtoChunk` (block element `BlockState`, biome element the
/// worldgen `section_reconstruction::BiomeId`, structure key the server
/// `StructureKey`) and the BIOMES→NOISE→SURFACE→CARVERS→FEATURES executor over
/// the shared worldgen objects.
pub struct GenerationChunkHolder {
    chunk: ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    context: WorldGenContext<BlockState, WorldgenBiomeId, StructureKey>,
}

impl GenerationChunkHolder {
    /// Create a holder for `pos` under `generator` and wire the executor.
    ///
    /// The `'static` closures capture the shared `Arc<OverworldGenerator>`: the
    /// BIOMES body runs `ChunkAccess.fillBiomesFromNoise` (Java's
    /// `createBiomes` default) over the overworld biome source and this world's
    /// sampler, mapping each `Holder<BiomeId>` to the dense worldgen biome id;
    /// the NOISE body runs the shell's real `fillFromNoise` block write over an
    /// empty blender; the SURFACE body runs the real `buildSurface`; the
    /// CARVERS body runs the real `applyCarvers` (the overworld-carvers
    /// center-chunk loop — see the noisegen driver's doc for the deferred
    /// `WorldGenRegion`/`StructureManager` seams); the FEATURES body starts
    /// Java's `ChunkStatusTasks.generateFeatures` — `run_biome_decoration`
    /// runs `addVanillaDecorations` faithfully: the `FINAL_HEIGHTMAPS`
    /// priming, the decoration-seed derivation, a dependency-window composition (`compose_feature_region`: a
    /// `WorldGenRegion` that borrows the center chunk and owns the 17x17
    /// FEATURES cache, with CARVERS at distances 0/1 and
    /// STRUCTURE_STARTS through distance 8), and the Paper-order biome-union
    /// gather + `retainAll` — and then decodes and runs
    /// `minecraft:lake_lava_underground` from its registry-backed JSON at the
    /// exact feature seed before stopping at the first selected unsupported
    /// path (seed-42 chunk (0,0): `minecraft:monster_room`); the chunk stays
    /// CARVERS.
    pub fn new(pos: ChunkPos, generator: Arc<OverworldGenerator>) -> Self {
        Self::new_with_structure_feature_count(pos, generator, None)
    }

    /// Construct a holder with the structure decoration index capability
    /// supplied explicitly. `None` is the normal production value and is a
    /// typed FEATURES boundary; `Some(count)` is reserved for a caller that
    /// has established the exact per-chunk structure consumption.
    pub fn new_with_structure_feature_count(
        pos: ChunkPos,
        generator: Arc<OverworldGenerator>,
        structure_feature_count: Option<usize>,
    ) -> Self {
        let height_accessor = create_height_accessor(
            generator.generator().get_min_y(),
            generator.generator().get_gen_depth(),
        );
        let chunk = ProtoChunk::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &current_version_container_factory(),
            None,
            Blocks::AIR.default_block_state(),
            // Paper: `ProtoChunk.getBlockState` returns `Blocks.VOID_AIR`
            // (`minecraft:void_air`, raw id 794) outside build height. The
            // named `Blocks` subset has no `VOID_AIR` constant, so resolve it
            // by raw id here — `BlockState::of` reads `BLOCK_STATE_BASES[794]`
            // (default state 15292). A wrong id silently resolves to another
            // block's default (830 is `minecraft:mud_brick_wall` → 18441), so
            // this must stay pinned to the generated registry.
            BlockState::of(BlockId(794)),
            &resolve_state_flags,
        );
        let context = WorldGenContext::new(
            {
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let source = &generator.biome_source;
                    chunk.fill_biomes_from_noise(source, &source.sampler, &|holder| {
                        WorldgenBiomeId(dense_biome_id(holder))
                    });
                }
            },
            {
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    generator.generator().fill_from_noise(
                        Blender::empty(),
                        generator.random_state(),
                        chunk,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateSurface` → the real
                // `NoiseBasedChunkGenerator.buildSurface` (the ported SURFACE
                // driver). The `BiomeManager` is built over the world's biome
                // source with the obfuscated seed and the generation context
                // over the generator + height accessor — the same arguments
                // Java's `NoiseBasedChunkGenerator.buildSurface` receives.
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let height_accessor = chunk.height_accessor();
                    let biome_manager = Arc::new(BiomeManager::new(
                        Arc::new(generator.biome_source.clone()),
                        BiomeManager::obfuscate_seed(generator.seed()),
                    ));
                    let generation_context =
                        Arc::new(WorldGenerationContext::new(&*generator, &height_accessor));
                    generator.generator().build_surface(
                        generator.random_state(),
                        biome_manager,
                        generation_context,
                        chunk,
                        None,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateCarvers` → the real
                // `NoiseBasedChunkGenerator.applyCarvers` (the ported CARVERS
                // driver). Same `BiomeManager` argument the SURFACE closure
                // builds (Java's `applyCarvers` receives the same manager the
                // `buildSurface` call used and derives the
                // `withDifferentSource` corrected one inside), plus the
                // overworld biome source — the `biomeSource` field the Java
                // driver wraps.
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    let biome_manager = Arc::new(BiomeManager::new(
                        Arc::new(generator.biome_source.clone()),
                        BiomeManager::obfuscate_seed(generator.seed()),
                    ));
                    generator.generator().apply_carvers(
                        &*generator,
                        generator.seed(),
                        generator.random_state(),
                        &biome_manager,
                        Arc::new(generator.biome_source.clone()),
                        chunk,
                    );
                }
            },
            {
                // `ChunkStatusTasks.generateFeatures` (Java) → the caller-owned
                // decoration body, typed. The real body is
                // `NoiseBasedChunkGenerator.applyBiomeDecoration` over a bounded
                // `WorldGenRegion`; `run_biome_decoration` runs Java's
                // `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`
                // faithfully — the `FINAL_HEIGHTMAPS` priming, the
                // section-origin decoration-seed derivation, the complete
                // FEATURES dependency window (the borrowed center chunk plus
                // the 17x17 cache with CARVERS at distances 0/1 and
                // STRUCTURE_STARTS through distance 8), and the Paper-order
                // biome-union gather + `retainAll`, the FULL-source-list
                // settings resolution (`ChunkGenerator.featuresPerStep`,
                // `ChunkGenerator.java` 97-100) and FeatureSorter, and the
                // exact per-feature seeds — and then decodes and runs the
                // registry-backed `minecraft:lake_lava_underground` entry
                // before failing typed at the first selected unsupported path
                // (`minecraft:monster_room`). It must never be "improved" into
                // a silent skip or a blanket UnsupportedTask.
                // The closure captures one generator clone (the free helper is
                // why the ownership test's `strong_count == base + 5` holds).
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    run_biome_decoration(chunk, &generator, structure_feature_count)
                }
            },
        );
        GenerationChunkHolder { chunk, context }
    }

    /// The chunk's persisted status — `EMPTY` before any step, `CARVERS` after a
    /// successful BIOMES→NOISE→SURFACE→CARVERS run, and never `FULL` (the
    /// executor refuses to stamp it). A FEATURES run primes the final heightmaps,
    /// drives the full dependency-window region, resolves the FULL possible-biome
    /// settings and builds the FeatureSorter. For the pinned seed-42 origin the
    /// known-zero structure fixture permits the registry-backed lake path and
    /// stops at `FeaturePlacementDecode` (`minecraft:monster_room`); unknown
    /// structure offsets refuse before feature seeding. The chunk is never
    /// stamped FEATURES.
    pub fn status(&self) -> ChunkStatus {
        self.chunk.get_persisted_status()
    }

    /// Drive the chunk from its current persisted status through `target`
    /// (inclusive). The BIOMES→NOISE→SURFACE→CARVERS task bodies are wired (an
    /// EMPTY chunk can reach CARVERS); the FEATURES task body is wired (it runs
    /// Java's `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`'s
    /// full dependency-window composition, and either (for the pinned seed-42
    /// origin) decodes/runs the lake path before the first unsupported path or
    /// refuses before feature seeding when structure consumption is unknown —
    /// see [`GenerationChunkHolder::new`]). A
    /// target the value layer does not wire is rejected by the executor before
    /// any work with a typed error — a path through a light step with no engine
    /// is refused as `GenError::LightEngineMissing`, and a target past LIGHT
    /// (SPAWN/FULL) is out of range
    /// ([`GeneratedChunkError::UnsupportedStatus`]). The chunk is left
    /// untouched by every such refusal. (The wired FEATURES rung is the
    /// exception: it runs Java's priming prologue — heightmap priming, the
    /// decoration-seed derivation, the bounded 3x3 region read — and then fails
    /// typed, so the chunk's heightmaps advance while its persisted status is
    /// never stamped past CARVERS; see [`GenerationChunkHolder::status`].)
    pub fn generate_through(&mut self, target: ChunkStatus) -> Result<(), GeneratedChunkError> {
        self.context
            .generate_through(&GENERATION_PYRAMID, &mut self.chunk, target)
            .map_err(|error| match error {
                GenError::UnsupportedStatus(status) => {
                    GeneratedChunkError::UnsupportedStatus(status)
                }
                other => GeneratedChunkError::Generation(other),
            })
    }

    /// The genuine-FULL-only install gate: `ChunkMap::install` accepts only a
    /// `LevelChunk` (FULL), and a generated chunk is a `ProtoChunk` that stops
    /// at `CARVERS` (the FEATURES rung fails typed at the first placed feature
    /// whose value decode is unavailable — see [`GenerationChunkHolder::new`]).
    /// No conversion from a sub-FULL `ProtoChunk` exists or may be added without
    /// the unwired SPAWN/FULL stages (RivetTodo #185), so this always fails
    /// loudly with the chunk's real status — never stamping FULL and never
    /// falling back to superflat.
    pub fn to_level_chunk(&self) -> Result<LevelChunk, GeneratedChunkError> {
        Err(GeneratedChunkError::InstallRequiresFull(
            self.chunk.get_persisted_status(),
        ))
    }
}

/// A fresh EMPTY worldgen chunk — the same construction the holder uses for its
/// own chunk, so a ring chunk generated through CARVERS is built with the exact
/// worldgen element types and void-air out-of-height default.
fn fresh_worldgen_chunk(
    pos: ChunkPos,
    generator: &OverworldGenerator,
) -> ProtoChunk<BlockState, WorldgenBiomeId, StructureKey> {
    let height_accessor = create_height_accessor(
        generator.generator().get_min_y(),
        generator.generator().get_gen_depth(),
    );
    ProtoChunk::new(
        pos,
        UpgradeData::empty(height_accessor.get_sections_count() as usize),
        height_accessor,
        &current_version_container_factory(),
        None,
        Blocks::AIR.default_block_state(),
        BlockState::of(BlockId(794)),
        &resolve_state_flags,
    )
}

/// Drive a fresh EMPTY chunk through the BIOMES→NOISE→SURFACE→CARVERS rungs the
/// FEATURES ring reads — the same real bodies the holder's executor closures
/// wire (Java's `ChunkStatusTasks.generateBiomes/generateNoise/generateSurface/
/// generateCarvers` for a neighbor chunk Paper generates with the identical
/// shared worldgen config).
fn generate_ring_chunk(
    pos: ChunkPos,
    generator: &Arc<OverworldGenerator>,
) -> ChunkAccess<BlockState, WorldgenBiomeId, StructureKey> {
    let mut chunk = fresh_worldgen_chunk(pos, generator);
    let source = &generator.biome_source;
    chunk.fill_biomes_from_noise(source, &source.sampler, &|holder| {
        WorldgenBiomeId(dense_biome_id(holder))
    });
    generator
        .generator()
        .fill_from_noise(Blender::empty(), generator.random_state(), &mut chunk);
    let height_accessor = chunk.height_accessor();
    let biome_manager = Arc::new(BiomeManager::new(
        Arc::new(generator.biome_source.clone()),
        BiomeManager::obfuscate_seed(generator.seed()),
    ));
    let generation_context = Arc::new(WorldGenerationContext::new(&**generator, &height_accessor));
    generator.generator().build_surface(
        generator.random_state(),
        biome_manager,
        generation_context,
        &mut chunk,
        None,
    );
    let biome_manager = Arc::new(BiomeManager::new(
        Arc::new(generator.biome_source.clone()),
        BiomeManager::obfuscate_seed(generator.seed()),
    ));
    generator.generator().apply_carvers(
        &**generator,
        generator.seed(),
        generator.random_state(),
        &biome_manager,
        Arc::new(generator.biome_source.clone()),
        &mut chunk,
    );
    chunk.into_base()
}

/// `ChunkGenerator.addVanillaDecorations` (Paper 26.2) over the bounded 3x3
/// region — the FEATURES body's real prologue, gather, and per-step loop, up to
/// the first placed feature whose value decode is unavailable.
///
/// In Java order:
///   1. `Heightmap.primeHeightmaps(chunk, FINAL_HEIGHTMAPS)` primes the four
///      final heightmaps the decoration bodies read (Java's
///      `ChunkStatusTasks.generateFeatures`, before `applyBiomeDecoration`);
///   2. `SectionPos.of(centerPos, level.getMinSectionY()).origin()` derives the
///      section-origin block position and `setDecorationSeed(seed, origin.x,
///      origin.z)` the decoration seed;
///   3. the region is composed: the center `ProtoChunk` (at CARVERS, the rung
///      the executor guarantees) is borrowed through a [`CenterHolder`], and
///      the eight ring chunks are generated EMPTY→CARVERS through the same real
///      bodies and owned through [`OwnedHolder`]s — the `StaticCache2D` the
///      bounded `WorldGenRegion` reads `level.getChunk` from;
///   4. the 3x3 biome union is gathered in Paper order (`ChunkPos.rangeClosed
///      (sectionPos.chunk(), 1)` → sections → `biomes().getAll`) and
///      `retainAll`-ed against the biome source's possible biomes;
///   5. the FULL `biomeSource.possibleBiomes()` list resolves its
///      `BiomeGenerationSettings` in source order (the exact argument Paper's
///      `ChunkGenerator.featuresPerStep` memoizes at construction,
///      `ChunkGenerator.java` 97-100) and `build_features_per_step` produces
///      the per-step data from that full list — the 3x3 union only picks which
///      global indices execute per step, exactly like Paper's
///      `addVanillaDecorations` (`generationSteps =
///      max(Decoration.values().length, featureStepCount)`).
///
/// Before the per-step loop, the value layer refuses with
/// `GenError::StructureDecorationIndexUnavailable`: Paper consumes structure
/// decoration entries before placed-feature work, and the unported structure
/// manager is the only source for that exact index. No feature seed or
/// placement runs with a fabricated zero offset, no phf index panics, and no
/// biome is fabricated or silently skipped.
///
/// Compose the FEATURES `WorldGenRegion` over the complete accumulated
/// dependency window of the FEATURES step. Paper's direct dependencies are
/// `CARVERS` at distances 0 and 1, followed by `STRUCTURE_STARTS` through
/// distance 8, so the cache is 17x17. The decoration biome union reads only
/// the center 3x3, but placement and worldgen reads are bounded by the full
/// status contract and must not be backed by an undersized cache.
fn compose_feature_region<'a>(
    chunk: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey> {
    let center_pos = chunk.get_pos();
    let center_status = chunk.get_persisted_status();
    let step = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Features)
        .clone();
    let dependencies = step.direct_dependencies();
    let radius = dependencies.size() as i32 - 1;
    let width = radius * 2 + 1;
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> + 'a>,
    > = Vec::with_capacity((width * width) as usize);

    // `StaticCache2D::from_entries` stores X outer, Z inner — index
    // `(x - minX) * sizeZ + (z - minZ)`. Build in that order so every
    // `getChunk(x, z)` resolves the holder for its own coordinates.
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let pos = ChunkPos::new(
                center_pos.x().wrapping_add(dx),
                center_pos.z().wrapping_add(dz),
            );
            if pos == center_pos {
                continue;
            }
            let distance = dx.abs().max(dz.abs()) as usize;
            let status = dependencies.get(distance);
            let base = match status {
                ChunkStatus::Carvers => generate_ring_chunk(pos, generator),
                ChunkStatus::StructureStarts => {
                    let mut structure_chunk = fresh_worldgen_chunk(pos, generator);
                    structure_chunk.set_persisted_status(ChunkStatus::StructureStarts);
                    structure_chunk.into_base()
                }
                other => {
                    panic!("unsupported FEATURES cache dependency {other:?} at distance {distance}")
                }
            };
            holders.push(Box::new(OwnedHolder::new(base, status)));
        }
    }

    let center_index = (radius * width + radius) as usize;
    holders.insert(
        center_index,
        Box::new(CenterHolder::new(chunk.base_mut(), center_status)),
    );
    let cache = StaticCache2D::from_entries(
        center_pos.x().wrapping_sub(radius),
        center_pos.z().wrapping_sub(radius),
        width,
        width,
        holders,
    );
    WorldGenRegion::new(
        cache,
        center_pos,
        step,
        generator.seed(),
        generator.generator().get_min_y(),
        generator.generator().get_gen_depth(),
        generator.generator().get_sea_level(),
        Arc::new(generator.biome_source.clone()),
        generator.registry_access().clone(),
    )
}

/// `possibleBiomes` — the 3x3 biome union, gathered in Paper order and
/// `retainAll`-ed against the overworld source's possible biomes. The dense
/// worldgen biome id maps through `BIOME_BY_ID` (the registry-id-indexed
/// name table); ids outside the table are skipped (a hostile chunk can't
/// take the gather down).
fn gather_possible_biomes(
    region: &WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> HashSet<&'static str> {
    let center_pos = region.get_center();
    let mut possible_biomes = HashSet::new();
    for pos in ChunkPos::range_closed(&center_pos, 1) {
        let chunk_in_range = region.get_chunk(pos.x(), pos.z());
        for section in chunk_in_range.get_sections() {
            section.biomes().get_all(|biome: WorldgenBiomeId| {
                if let Some(name) = BIOME_BY_ID.get(biome.0 as usize) {
                    possible_biomes.insert(*name);
                }
            });
        }
    }
    let possible = generator.biome_source().possible_biomes();
    possible_biomes.retain(|name| {
        possible.iter().any(|p| {
            BIOME_BY_ID
                .get(dense_biome_id(p) as usize)
                .is_some_and(|possible_name| *name == *possible_name)
        })
    });
    possible_biomes
}

/// Resolve one possible biome's `BiomeGenerationSettings` from the generated
/// feature tables.
///
/// The placed-feature holders are `Holder::Reference` over one fabricated
/// `PLACED_FEATURE` registry id (the generated tables are keyed by name; the
/// FeatureSorter keys on holder identity, so a single fabricated registry
/// collapses the biomes' shared steps exactly like Paper's registry does).
/// `placed_by_id` collects the reverse id → key map the typed error names.
///
/// A biome whose dense id is not in `BIOME_BY_ID`, or whose name has no
/// generated settings, fails typed (`GenError::SettingsNotGenerated`) — never
/// a phf panic, never a fabricated or silently-skipped biome.
fn resolve_biome_settings(
    name: &'static str,
    placed_registry_id: RegistryId,
    placed_by_id: &mut HashMap<u32, &'static str>,
) -> Result<BiomeGenerationSettings, GenError> {
    let table = BIOME_GENERATION_SETTINGS_BY_NAME
        .get(name)
        .ok_or(GenError::SettingsNotGenerated { biome: Some(name) })?;
    let mut builder = PlainBuilder::default();
    for (step, step_features) in table.features.iter().enumerate() {
        for feature_name in *step_features {
            let id = PLACED_FEATURE_BY_NAME
                .get(feature_name)
                .ok_or(GenError::SettingsNotGenerated { biome: Some(name) })?
                .id as u32;
            placed_by_id.entry(id).or_insert(*feature_name);
            builder =
                builder.add_feature_index(step as i32, Holder::reference(placed_registry_id, id));
        }
    }
    Ok(builder.build())
}

/// Resolve the FULL `biomeSource.possibleBiomes()` list in source order — the
/// exact argument Paper's `ChunkGenerator.featuresPerStep` memoizes
/// (`ChunkGenerator.java` 97-100: `FeatureSorter.buildFeaturesPerStep(List.
/// copyOf(biomeSource.possibleBiomes()), ...)`). The FeatureSorter must be
/// built once from this full list, not the per-chunk 3x3 union; the union only
/// decides which feature indices execute per step. The first possible biome
/// that cannot resolve its settings fails typed in source order.
///
/// Each resolved source is paired with its biome name (the `BIOME_BY_ID` name
/// at its full-list position) so the per-step loop can map a union biome back
/// to its full-list source by name — the generated table is name-keyed, so the
/// union and the full list resolve structurally identical `Reference` holders.
fn resolve_feature_settings(
    possible_biomes: &[Holder<BiomeId>],
    placed_registry_id: RegistryId,
    placed_by_id: &mut HashMap<u32, &'static str>,
) -> Result<Vec<(BiomeGenerationSettings, &'static str)>, GenError> {
    let mut settings_sources = Vec::with_capacity(possible_biomes.len());
    for holder in possible_biomes {
        let dense = dense_biome_id(holder) as usize;
        let name = *BIOME_BY_ID
            .get(dense)
            .ok_or(GenError::SettingsNotGenerated { biome: None })?;
        let settings = resolve_biome_settings(name, placed_registry_id, placed_by_id)?;
        settings_sources.push((settings, name));
    }
    Ok(settings_sources)
}

type FeatureOps = RegistryOps<Value, JsonOps>;

fn decode_value<T: Clone>(
    codec: Arc<dyn Codec<T, FeatureOps>>,
    ops: &FeatureOps,
    value: &Value,
    label: &str,
) -> Result<T, String> {
    let result = codec.parse(ops, value);
    match result.result() {
        Some(value) => Ok(value.clone()),
        None => Err(result
            .error_ref()
            .map(|error| format!("{label}: {}", error.message()))
            .unwrap_or_else(|| format!("{label}: codec returned no result"))),
    }
}

fn without_type(value: &Value, label: &str) -> Result<Value, String> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{label} must be an object"))?;
    object.remove("type");
    Ok(Value::Object(object))
}

fn decode_placement_modifier(
    value: &Value,
    ops: &FeatureOps,
    label: &str,
) -> Result<Arc<dyn ErasedPlacementModifier>, String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} has no type"))?;
    let value_without_type = without_type(value, label)?;
    let modifier: Arc<dyn ErasedPlacementModifier> = match kind {
        "minecraft:block_predicate_filter" => Arc::new(decode_value(
            block_predicate_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} block_predicate_filter"),
        )?),
        "minecraft:rarity_filter" => Arc::new(decode_value(
            rarity_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} rarity_filter"),
        )?),
        "minecraft:in_square" => Arc::new(decode_value(
            in_square_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} in_square"),
        )?),
        "minecraft:height_range" => Arc::new(decode_value(
            height_range_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} height_range"),
        )?),
        "minecraft:environment_scan" => Arc::new(decode_value(
            environment_scan_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} environment_scan"),
        )?),
        "minecraft:surface_relative_threshold_filter" => Arc::new(decode_value(
            surface_relative_threshold_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} surface_relative_threshold_filter"),
        )?),
        "minecraft:biome" => Arc::new(decode_value(
            biome_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} biome"),
        )?),
        "minecraft:count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(count_placement_codec::<FeatureOps>()),
            ops,
            &value_without_type,
            &format!("decode {label} count"),
        )?),
        "minecraft:count_on_every_layer" => Arc::new(decode_value(
            count_on_every_layer_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} count_on_every_layer"),
        )?),
        "minecraft:noise_based_count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(
                noise_based_count_placement_codec::<FeatureOps>(),
            ),
            ops,
            &value_without_type,
            &format!("decode {label} noise_based_count"),
        )?),
        "minecraft:noise_threshold_count" => Arc::new(decode_value(
            rivet_serialization::map_codec::codec_of(noise_threshold_count_placement_codec::<
                FeatureOps,
            >()),
            ops,
            &value_without_type,
            &format!("decode {label} noise_threshold_count"),
        )?),
        "minecraft:heightmap" => Arc::new(decode_value(
            heightmap_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} heightmap"),
        )?),
        "minecraft:random_offset" => Arc::new(decode_value(
            random_offset_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} random_offset"),
        )?),
        "minecraft:surface_water_depth_filter" => Arc::new(decode_value(
            surface_water_depth_filter_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} surface_water_depth_filter"),
        )?),
        "minecraft:fixed_placement" => Arc::new(decode_value(
            fixed_placement_codec::<FeatureOps>(),
            ops,
            &value_without_type,
            &format!("decode {label} fixed_placement"),
        )?),
        other => {
            return Err(format!(
                "{label} has unsupported placement modifier {other}"
            ));
        }
    };
    Ok(modifier)
}

fn decode_configured_feature(
    configured_key: &str,
    ops: &FeatureOps,
) -> Result<ConfiguredFeatureErased, String> {
    let entry = CONFIGURED_FEATURE_BY_NAME
        .get(configured_key)
        .ok_or_else(|| format!("missing generated {configured_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {configured_key} JSON: {error}"))?;
    let feature_type = json
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{configured_key} JSON has no feature type"))?;
    let feature = feature_id_from_registry_name(feature_type)
        .ok_or_else(|| format!("{configured_key} has unsupported feature type {feature_type}"))?;
    let config_value = json
        .get("config")
        .ok_or_else(|| format!("{configured_key} JSON has no config"))?;
    let config: Arc<dyn FeatureConfiguration> = match feature_type {
        "minecraft:lake" => Arc::new(decode_value(
            lake_configuration_codec::<FeatureOps>(),
            ops,
            config_value,
            &format!("decode {configured_key} config"),
        )?),
        "minecraft:monster_room" => Arc::new(NoneFeatureConfiguration),
        other => {
            return Err(format!(
                "{configured_key} has unsupported feature type {other}"
            ));
        }
    };
    Ok(ConfiguredFeatureErased { feature, config })
}

fn decode_placement_modifiers(
    placed_key: &str,
    generator: &OverworldGenerator,
) -> Result<Vec<Arc<dyn ErasedPlacementModifier>>, String> {
    let entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    let ops =
        RegistryOps::create_from_access(&JsonOps::INSTANCE, generator.registry_access().clone());
    json.get("placement")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{placed_key} JSON has no placement list"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            decode_placement_modifier(value, &ops, &format!("{placed_key} placement {index}"))
        })
        .collect()
}

struct DecodedPlacedFeature {
    placed_registry: Registry<PlacedFeature>,
    configured_registry: Registry<ConfiguredFeatureErased>,
    placed_holder: Holder<PlacedFeature>,
    feature_key: &'static str,
}

impl DecodedPlacedFeature {
    fn place_with_biome_check(
        &self,
        level: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
        generator: &Arc<OverworldGenerator>,
        random: &mut WorldgenRandom<XoroshiroRandomSource>,
        origin: &BlockPos,
    ) -> bool {
        let placed = self.placed_holder.value(&self.placed_registry);
        let selection_generator = FeatureSelectionGenerator {
            generator: Arc::clone(generator),
            feature_key: self.feature_key,
        };
        placed.place_with_biome_check(
            &self.configured_registry,
            level,
            &selection_generator,
            random,
            origin,
        )
    }
}

fn decode_placed_feature(
    placed_key: &'static str,
    generator: &OverworldGenerator,
) -> Result<DecodedPlacedFeature, String> {
    let entry = PLACED_FEATURE_BY_NAME
        .get(placed_key)
        .ok_or_else(|| format!("missing generated {placed_key} entry"))?;
    let json: Value = serde_json::from_str(entry.json)
        .map_err(|error| format!("decode {placed_key} JSON: {error}"))?;
    let configured_key = json
        .get("feature")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{placed_key} JSON has no configured feature"))?;
    let ops =
        RegistryOps::create_from_access(&JsonOps::INSTANCE, generator.registry_access().clone());
    let configured = Arc::new(decode_configured_feature(configured_key, &ops)?);
    let mut configured_builder = RegistryBuilder::new(&*CONFIGURED_FEATURE);
    let configured_registry_id = configured_builder.registry_id();
    let configured_resource_key =
        ResourceKey::create(&*CONFIGURED_FEATURE, Identifier::parse(configured_key));
    let configured_id = configured_builder.register(
        &configured_resource_key,
        configured,
        RegistrationInfo::BUILT_IN,
    );
    let configured_registry = configured_builder.freeze();

    let modifiers = decode_placement_modifiers(placed_key, generator)?;
    let placed_value = Arc::new(PlacedFeature::new(
        Holder::reference(configured_registry_id, configured_id.0),
        modifiers,
    ));
    let mut placed_builder = RegistryBuilder::new(&*PLACED_FEATURE);
    let placed_registry_id = placed_builder.registry_id();
    let placed_resource_key = ResourceKey::create(&*PLACED_FEATURE, Identifier::parse(placed_key));
    let placed_id = placed_builder.register(
        &placed_resource_key,
        placed_value,
        RegistrationInfo::BUILT_IN,
    );
    let placed_registry = placed_builder.freeze();
    Ok(DecodedPlacedFeature {
        placed_registry,
        configured_registry,
        placed_holder: Holder::reference(placed_registry_id, placed_id.0),
        feature_key: placed_key,
    })
}

struct FeatureSelectionGenerator {
    generator: Arc<OverworldGenerator>,
    feature_key: &'static str,
}

impl ChunkGenerator for FeatureSelectionGenerator {
    fn get_min_y(&self) -> i32 {
        self.generator.get_min_y()
    }

    fn get_gen_depth(&self) -> i32 {
        self.generator.get_gen_depth()
    }

    fn get_biome_generation_settings_has_feature(
        &self,
        biome: &Holder<BiomeId>,
        _feature: &PlacedFeature,
    ) -> bool {
        let Holder::Direct(biome) = biome else {
            return false;
        };
        let Some(name) = BIOME_BY_ID.get(biome.0 as usize) else {
            return false;
        };
        BIOME_GENERATION_SETTINGS_BY_NAME
            .get(name)
            .is_some_and(|settings| {
                settings
                    .features
                    .iter()
                    .any(|step| step.contains(&self.feature_key))
            })
    }
}

fn placement_selects(
    region: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    random: &mut WorldgenRandom<XoroshiroRandomSource>,
    origin: &BlockPos,
    feature_key: &'static str,
) -> Result<bool, String> {
    let modifiers = decode_placement_modifiers(feature_key, generator)?;
    let selection_generator = FeatureSelectionGenerator {
        generator: Arc::clone(generator),
        feature_key,
    };
    let dummy_feature = ConfiguredFeatureErased {
        feature: FeatureId::new(0),
        config: Arc::new(NoneFeatureConfiguration),
    };
    let placed = PlacedFeature::new(Holder::Direct(dummy_feature), modifiers);
    Ok(placed.has_placement_positions(region, &selection_generator, random, origin))
}

/// Resolve the feature-index offset consumed by Paper's structure
/// decoration loop. The structure manager and its per-step decoration state
/// are not available in this value layer, so pretending the offset is zero
/// would seed every placed feature incorrectly. Only an explicitly supplied
/// fixture/oracle count may cross this boundary; unknown counts stop before
/// feature seeding.
fn structure_consumed_feature_count(
    chunk_pos: ChunkPos,
    known_count: Option<usize>,
) -> Result<usize, GenError> {
    known_count.ok_or(GenError::StructureDecorationIndexUnavailable { chunk_pos })
}

fn run_biome_decoration(
    chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    structure_feature_count: Option<usize>,
) -> Result<(), GenError> {
    chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
    let center_pos = chunk.get_pos();
    let origin =
        SectionPos::of_chunk_pos(&center_pos, chunk.height_accessor().get_min_section_y()).origin();
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
        random_support::generate_unique_seed(),
    ));
    let decoration_seed =
        random.set_decoration_seed(generator.seed(), origin.get_x(), origin.get_z());

    let mut region = compose_feature_region(chunk, generator);
    let union_biomes = gather_possible_biomes(&region, generator);

    // Resolve the FULL `biomeSource.possibleBiomes()` list in source order and
    // build the FeatureSorter once from it — Paper's
    // `ChunkGenerator.featuresPerStep` (`ChunkGenerator.java` 97-100), NOT the
    // 3x3 union. The union only picks which global indices execute per step.
    // The placed-feature holders are `Holder::Reference` over one fabricated
    // `PLACED_FEATURE` registry id (the generated tables are keyed by name; the
    // FeatureSorter keys on holder identity, so a single fabricated registry
    // collapses the biomes' shared steps exactly like Paper's registry does).
    // The `features` lists are `DECORATION_STEP_COUNT` long (the generated
    // data's step count).
    let placed_registry_id = RegistryBuilder::new(&*PLACED_FEATURE).registry_id();
    // Reverse id → key for the typed error: the holders carry the generated
    // table's placed-feature registry id, and the error names the key.
    let mut placed_by_id = HashMap::new();
    let full_possible_biomes = generator.biome_source().possible_biomes();
    let settings_sources =
        resolve_feature_settings(&full_possible_biomes, placed_registry_id, &mut placed_by_id)?;
    let feature_list =
        build_features_per_step(&settings_sources, |(settings, _)| settings.features(), true);

    // Paper consumes structure decoration entries before the placed-feature
    // loop. Without the structure manager the exact starting feature index is
    // unavailable, so stop before any `setFeatureSeed` or placement call.
    let structure_feature_index_offset =
        structure_consumed_feature_count(center_pos, structure_feature_count)?;

    // The per-step loop — Paper's `addVanillaDecorations`. The structure loop
    // remains outside this value layer; its explicitly supplied consumed count
    // accounts for the feature-index offset, and an unknown count already
    // returned the typed refusal above.
    let generation_steps = Decoration::VALUES.len().max(feature_list.len());
    // Paper walks steps in ascending order and, within a step, the sorted
    // global feature indices of the union biomes mapped through the full-list
    // sorter's `indexMapping`. The decoded lake is the first slice; keep
    // advancing its exact feature seeds until the first selected path outside
    // that slice, then stop with a typed boundary.
    let mut saw_feature = false;
    for step_index in 0..generation_steps {
        if step_index >= feature_list.len() {
            continue;
        }
        let step_feature_data = &feature_list[step_index];
        let mut possible_features_this_step = Vec::new();
        for name in &union_biomes {
            let Some(settings) = settings_sources
                .iter()
                .find(|(_, source_name)| *source_name == *name)
                .map(|(settings, _)| settings)
            else {
                continue;
            };
            if step_index < settings.features().len() {
                for holder in settings.features()[step_index].iter() {
                    if let Some(index) = step_feature_data.index_mapping(holder) {
                        possible_features_this_step.push(index);
                    }
                }
            }
        }
        possible_features_this_step.sort_unstable();
        possible_features_this_step.dedup();
        for global_feature_index in possible_features_this_step {
            saw_feature = true;
            let feature = &feature_list[step_index].features[global_feature_index];
            let feature_key = match feature {
                Holder::Reference { id, .. } => {
                    placed_by_id.get(id).copied().unwrap_or("minecraft:unknown")
                }
                Holder::Direct(_) => "minecraft:unknown",
            };
            // `setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)`
            // starts after every structure-consumed feature index.
            random.set_feature_seed(
                decoration_seed,
                (structure_feature_index_offset + global_feature_index) as i32,
                step_index as i32,
            );
            if matches!(
                feature_key,
                "minecraft:lake_lava_underground" | "minecraft:lake_lava_surface"
            ) {
                let placed = decode_placed_feature(feature_key, generator).map_err(|_| {
                    GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key,
                    }
                })?;
                placed.place_with_biome_check(&mut region, generator, &mut random, &origin);
                continue;
            }
            let selected =
                placement_selects(&mut region, generator, &mut random, &origin, feature_key)
                    .map_err(|_| GenError::FeaturePlacementDecode {
                        chunk_pos: center_pos,
                        step_index,
                        global_feature_index,
                        feature_key,
                    })?;
            if selected {
                return Err(GenError::FeaturePlacementDecode {
                    chunk_pos: center_pos,
                    step_index,
                    global_feature_index,
                    feature_key,
                });
            }
        }
    }
    if !saw_feature {
        return Err(GenError::FeaturePlacementDecode {
            chunk_pos: center_pos,
            step_index: 0,
            global_feature_index: 0,
            feature_key: "minecraft:unknown",
        });
    }
    Ok(())
}

impl fmt::Debug for GenerationChunkHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerationChunkHolder")
            .field("pos", &self.chunk.get_pos())
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::chunk_map::ChunkMap;
    use rivet_registry::generated::block_states::StateId;
    use rivet_util::RandomSource;
    use rivet_world::block::Block;
    use rivet_world::level::WorldGenLevel;
    use rivet_world::level::height_accessor::LevelHeightAccessor;
    use rivet_world::levelgen::heightmap::Types;
    use std::collections::HashMap;

    struct LakeTestLevel {
        states: HashMap<BlockPos, BlockState>,
        block_ticks: Vec<(BlockPos, Block, i32)>,
        post_processing: Vec<BlockPos>,
    }

    impl LakeTestLevel {
        fn with_lava_cavity(origin: BlockPos) -> Self {
            let mut states = HashMap::new();
            let cavity_origin = origin.offset(-8, -4, -8);
            for xx in 0..16 {
                for zz in 0..16 {
                    for yy in 0..8 {
                        let pos = cavity_origin.offset(xx, yy, zz);
                        let state = if yy < 4 {
                            Blocks::LAVA.default_block_state()
                        } else if yy == 7 {
                            Blocks::STONE.default_block_state()
                        } else {
                            Blocks::AIR.default_block_state()
                        };
                        states.insert(pos, state);
                    }
                }
            }
            for xx in 0..16 {
                for zz in 0..16 {
                    for yy in 8..10 {
                        states.insert(
                            cavity_origin.offset(xx, yy, zz),
                            Blocks::STONE.default_block_state(),
                        );
                    }
                }
            }
            LakeTestLevel {
                states,
                block_ticks: Vec::new(),
                post_processing: Vec::new(),
            }
        }
    }

    impl LevelHeightAccessor for LakeTestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for LakeTestLevel {
        fn get_seed(&self) -> i64 {
            42
        }

        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.states
                .get(pos)
                .copied()
                .unwrap_or_else(|| Blocks::AIR.default_block_state())
        }

        fn set_block(&mut self, pos: &BlockPos, state: BlockState, _flags: u32) -> bool {
            self.states.insert(*pos, state);
            true
        }

        fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
            self.post_processing.push(*pos);
        }

        fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
            self.block_ticks.push((*pos, block, delay));
        }
    }

    /// The shared test realization (built once — the worldgen registry
    /// bootstrap is not free). The seed mirrors the pinned loaded-world corpus.
    fn test_generator() -> Arc<OverworldGenerator> {
        static GENERATOR: std::sync::LazyLock<Arc<OverworldGenerator>> =
            std::sync::LazyLock::new(|| Arc::new(OverworldGenerator::new(42)));
        GENERATOR.clone()
    }

    /// The `ChunkGenerator` realization delegates to the noisegen shell's real
    /// bodies — the `.chunk.generator` reconciliation note's single source of
    /// truth, grounded in the overworld preset's geometry (min_y -64, height
    /// 384, sea level 63, per Paper's `OverworldOptions`).
    #[test]
    fn generator_delegates_to_the_shell_real_bodies() {
        let generator = test_generator();
        let shell = generator.generator();
        // The realization answers the abstract contract from the settings.
        assert_eq!(generator.get_min_y(), shell.get_min_y());
        assert_eq!(generator.get_gen_depth(), shell.get_gen_depth());
        assert_eq!(generator.get_sea_level(), shell.get_sea_level());
        // Paper overworld geometry.
        assert_eq!(generator.get_min_y(), -64);
        assert_eq!(generator.get_gen_depth(), 384);
        assert_eq!(generator.get_sea_level(), 63);
        // The seed the generator was realized for is carried.
        assert_eq!(generator.seed(), 42);
        // `get_first_free_height` delegates to the real `get_base_height`: a
        // real surface height (above the void) rather than a panic seam.
        let height_accessor =
            create_height_accessor(generator.get_min_y(), generator.get_gen_depth());
        let height = generator.get_first_free_height(
            0,
            0,
            Types::WorldSurfaceWg,
            &height_accessor,
            generator.random_state(),
        );
        assert!(
            height > generator.get_min_y(),
            "base height at (0,0) should be above the void, got {height}"
        );
    }

    /// The biome source resolves the overworld table over the realized climate
    /// sampler: deterministic, consistent between the two trait surfaces, and
    /// non-trivial (more than one biome over the broad sample grid).
    #[test]
    fn biome_source_resolves_over_the_realized_sampler() {
        let generator = test_generator();
        let source = generator.biome_source();
        // Deterministic for a fixed quart position (the `NoiseBiomeSource`
        // path — the trait's own sampler).
        assert_eq!(
            NoiseBiomeSource::get_noise_biome(source, 0, 0, 0),
            NoiseBiomeSource::get_noise_biome(source, 0, 0, 0)
        );
        // The `NoiseBiomeSource` path (internal sampler) agrees with the
        // explicit `BiomeResolver` path over the same sampler.
        let via_noise_source = NoiseBiomeSource::get_noise_biome(source, 0, 0, 0);
        let via_resolver = BiomeResolver::get_noise_biome(source, 0, 0, 0, source.sampler());
        assert_eq!(via_noise_source, via_resolver);
        // The overworld table is non-trivial over a broad grid.
        let mut seen = std::collections::HashSet::new();
        for qx in (0..128).step_by(8) {
            for qz in (0..128).step_by(8) {
                seen.insert(dense_biome_id(&NoiseBiomeSource::get_noise_biome(
                    source, qx, 0, qz,
                )));
            }
        }
        assert!(
            seen.len() >= 2,
            "overworld biome source should vary; got {seen:?}"
        );
    }

    /// The executor drives a fresh chunk EMPTY→BIOMES→NOISE and each body
    /// produces real data — the BIOMES body fills the biome container, the
    /// NOISE body writes terrain blocks and the WORLDGEN heightmaps. A second
    /// run to the same status is an idempotent no-op.
    #[test]
    fn generate_through_biomes_then_noise() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(1, -2));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        // The BIOMES body ran: it must change the biome container away from
        // the empty default (a real overworld chunk can legitimately be a
        // single biome, so the honest check is that generation filled it).
        let mut before = std::collections::HashSet::new();
        let mut after = std::collections::HashSet::new();
        for qx in 0..4 {
            for qz in 0..4 {
                before.insert(holder.chunk.get_noise_biome(qx, 0, qz));
            }
        }
        holder
            .generate_through(ChunkStatus::Biomes)
            .expect("BIOMES");
        assert_eq!(holder.status(), ChunkStatus::Biomes);
        for qx in 0..4 {
            for qz in 0..4 {
                after.insert(holder.chunk.get_noise_biome(qx, 0, qz));
            }
        }
        assert!(
            before != after,
            "BIOMES must replace the empty biome container with resolved biomes; before={before:?} after={after:?}"
        );

        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.status(), ChunkStatus::Noise);
        // The NOISE body ran: a surface block was written above the void at a
        // surface height, and the WORLDGEN surface heightmap was primed (an
        // unprimed heightmap reads as `min_y - 1`).
        let min_y = holder.chunk.get_min_y();
        let world_surface = holder.chunk.heightmaps()[Types::WorldSurfaceWg as usize]
            .as_ref()
            .expect("fill_from_noise primes the WORLD_SURFACE_WG heightmap");
        let height = world_surface.get_height_at(0, 0, min_y);
        assert!(
            height > min_y,
            "NOISE should write terrain; world surface height at (0,0) = {height}"
        );
        let block = holder.chunk.get_block_state(0, height, 0);
        assert_ne!(
            block,
            Blocks::AIR.default_block_state(),
            "a surface block (not AIR) should sit at the surface height"
        );

        // Re-running to the same status is an idempotent no-op (the chunk is
        // already at NOISE).
        holder
            .generate_through(ChunkStatus::Noise)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Noise);
    }

    /// The SURFACE rung runs the real `NoiseBasedChunkGenerator.buildSurface`
    /// over the NOISE output: the executor drives EMPTY→BIOMES→NOISE→SURFACE,
    /// stamps the chunk SURFACE, and the surface body replaced at least one
    /// NOISE-default cell with a biome surface material — the overworld surface
    /// rule's top band defaults to `grass_or_dirt_if_underwater` (never the
    /// stone `default_block`), so a land column's top cell must change. The
    /// worldgen surface heights are preserved: the surface write replaces
    /// non-air with non-air, so `WORLD_SURFACE_WG` never moves (Paper's
    /// `buildSurface` writes through `ChunkAccess::setBlockState`, which keeps
    /// the heightmap). Re-running to SURFACE is an idempotent no-op.
    #[test]
    fn generate_through_biomes_then_noise_then_surface() {
        fn surface_height(chunk: &GenerationChunkHolder, x: i32, z: i32, min_y: i32) -> i32 {
            chunk.chunk.heightmaps()[Types::WorldSurfaceWg as usize]
                .as_ref()
                .expect("WORLD_SURFACE_WG primed")
                .get_height_at(x, z, min_y)
        }

        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(1, -2));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.status(), ChunkStatus::Noise);
        let min_y = holder.chunk.get_min_y();

        // Snapshot, per column, the worldgen surface height and the 16 cells
        // below it — deep enough to hold the overworld top material plus the
        // band depth.
        let mut before_heights = Vec::with_capacity(256);
        let mut before_band: Vec<Vec<BlockState>> = Vec::with_capacity(256);
        for x in 0..16i32 {
            for z in 0..16i32 {
                let h = surface_height(&holder, x, z, min_y);
                before_heights.push(h);
                before_band.push(
                    (h - 16..=h)
                        .map(|y| holder.chunk.get_block_state(x, y, z))
                        .collect(),
                );
            }
        }

        holder
            .generate_through(ChunkStatus::Surface)
            .expect("SURFACE");
        assert_eq!(holder.status(), ChunkStatus::Surface);

        // The surface body ran: at least one column's surface band changed away
        // from the NOISE stone default (the overworld top band defaults to
        // grass/dirt, never stone). A cell counts only if the surface height is
        // stable, so a height-only change cannot satisfy this.
        let mut any_changed = false;
        let mut after_heights = Vec::with_capacity(256);
        for x in 0..16i32 {
            for z in 0..16i32 {
                let h = surface_height(&holder, x, z, min_y);
                after_heights.push(h);
                let index = x as usize * 16 + z as usize;
                if h == before_heights[index] {
                    let band = &before_band[index];
                    if band.iter().enumerate().any(|(i, before)| {
                        let y = h - 16 + i as i32;
                        *before != holder.chunk.get_block_state(x, y, z)
                    }) {
                        any_changed = true;
                    }
                }
            }
        }
        assert!(
            any_changed,
            "SURFACE must replace at least one NOISE-default cell with a surface material"
        );
        // The worldgen surface heights are stable: the surface write replaced
        // non-air with non-air, so WORLD_SURFACE_WG never moves.
        assert_eq!(after_heights, before_heights);

        // Re-running to the same status is an idempotent no-op.
        holder
            .generate_through(ChunkStatus::Surface)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Surface);
    }

    /// The CARVERS rung runs the real `NoiseBasedChunkGenerator.applyCarvers`
    /// over the SURFACE output: the executor drives EMPTY→BIOMES→NOISE→SURFACE→
    /// CARVERS, stamps the chunk CARVERS, and the carvers body actually carved —
    /// the carving mask is written back (only `applyCarvers` writes it) and is
    /// non-empty, and at least one carved cell differs from the SURFACE snapshot
    /// the carvers consumed (air/water carved through the SURFACE output, which
    /// is what the top-material binder feeds them).
    #[test]
    fn generate_through_carvers_runs_the_real_apply_carvers() {
        let generator = test_generator();
        let pos = ChunkPos::new(2, 3);
        let mut holder = generator.create_holder(pos);
        assert_eq!(holder.status(), ChunkStatus::Empty);

        holder
            .generate_through(ChunkStatus::Surface)
            .expect("SURFACE");
        assert_eq!(holder.status(), ChunkStatus::Surface);
        let min_y = holder.chunk.get_min_y();
        let height = holder.chunk.get_height();

        // Snapshot the full SURFACE output — the carvers consume this (the
        // top-material binder) and carve through it.
        let mut before = Vec::with_capacity((16 * 16 * height) as usize);
        for y in min_y..min_y + height {
            for z in 0..16i32 {
                for x in 0..16i32 {
                    before.push(holder.chunk.get_block_state(x, y, z));
                }
            }
        }
        let index =
            |x: i32, y: i32, z: i32| -> usize { ((y - min_y) * 16 * 16 + z * 16 + x) as usize };

        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(holder.status(), ChunkStatus::Carvers);

        // The real driver ran: only `applyCarvers` writes the carving mask, and
        // it came back present with carved cells — a fresh EMPTY chunk has no
        // mask, and no other step touches it.
        let mask = holder
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        let carved: Vec<BlockPos> = mask.stream(&pos).collect();
        assert!(
            !carved.is_empty(),
            "the overworld carvers must carve at least one cell at {pos:?}"
        );

        // And the carve wrote blocks: each mask bit is a cell the driver
        // carved, so at least one such cell must differ from the SURFACE output
        // it consumed (air/water through the surface material).
        let mut any_carved_cell_changed = false;
        for block in &carved {
            let x = block.get_x();
            let z = block.get_z();
            let y = block.get_y();
            if y >= min_y
                && y < min_y + height
                && before[index(x, y, z)] != holder.chunk.get_block_state(x, y, z)
            {
                any_carved_cell_changed = true;
            }
        }
        assert!(
            any_carved_cell_changed,
            "carving must write air/water through the SURFACE output at a carved cell"
        );

        // Re-running to the same status is an idempotent no-op — the already
        // stamped chunk is not carved twice.
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// The CARVERS step is deterministic: two independent chunks at the same
    /// position, driven through the same seed-42 generator, carve identically.
    /// This pins the RNG draw order — the shared `WorldgenRandom` is re-seeded
    /// per source chunk/carver with `setLargeFeatureSeed(seed + index, x, z)`
    /// (wrapping long arithmetic), so the same seed and position must reproduce
    /// the exact same carving mask and the exact same carved blocks.
    #[test]
    fn generate_through_carvers_is_deterministic() {
        let generator = test_generator();
        let pos = ChunkPos::new(-3, 5);
        let mut a = generator.create_holder(pos);
        let mut b = generator.create_holder(pos);

        a.generate_through(ChunkStatus::Carvers).expect("a CARVERS");
        b.generate_through(ChunkStatus::Carvers).expect("b CARVERS");

        // Same seed + position → identical carving mask (the bit pattern, not
        // just the stream). A fresh EMPTY chunk starts mask-less, so the mask
        // presence is part of the determinism contract.
        let mask_a = a
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        let mask_b = b
            .chunk
            .get_carving_mask()
            .expect("applyCarvers must write the carving mask");
        assert_eq!(
            mask_a.to_array(),
            mask_b.to_array(),
            "same seed + position must reproduce the identical carving mask"
        );
        assert!(
            !mask_a.to_array().is_empty(),
            "determinism test must be non-vacuous: the carvers carved at {pos:?}"
        );

        // And the carve wrote identical blocks: every carved cell's state
        // matches between the two runs.
        let min_y = a.chunk.get_min_y();
        let height = a.chunk.get_height();
        for block in mask_a.stream(&pos) {
            let x = block.get_x();
            let z = block.get_z();
            let y = block.get_y();
            if y >= min_y && y < min_y + height {
                assert_eq!(
                    a.chunk.get_block_state(x, y, z),
                    b.chunk.get_block_state(x, y, z),
                    "carved cell ({x}, {y}, {z}) must be deterministic"
                );
            }
        }
    }

    /// Hostile: the out-of-build-height read default is real `void_air` — raw
    /// id 794, default state 15292 — not AIR and not another block's default.
    /// The NOISE test reads at the surface height (inside build height), so it
    /// can never observe this default; this test pins the state-id contract
    /// directly, catching a wrong raw id (830 resolves to
    /// `minecraft:mud_brick_wall`'s default 18441) that the heightmap/terrain
    /// walks in `fill_from_noise` would silently feed back.
    #[test]
    fn get_block_state_outside_build_height_returns_void_air() {
        let generator = test_generator();
        let holder = generator.create_holder(ChunkPos::new(3, -1));
        // Build height is [min_y, min_y + height - 1]; one below and one above
        // are both outside it.
        let max_y = holder.chunk.get_min_y() + holder.chunk.get_height() - 1;
        for y in [holder.chunk.get_min_y() - 1, max_y + 1] {
            let state = holder.chunk.get_block_state(0, y, 0);
            assert_eq!(
                state,
                BlockState::of(BlockId(794)),
                "out-of-build-height read at y={y} must be the void_air default state"
            );
            assert_eq!(state.id(), StateId(15292));
            assert_ne!(state, Blocks::AIR.default_block_state());
            assert_ne!(
                state.id(),
                StateId(18441),
                "must not resolve to minecraft:mud_brick_wall's default"
            );
        }
    }

    #[test]
    fn feature_region_outside_build_height_reads_void_air() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let min_y = holder.chunk.get_min_y();
        let region = compose_feature_region(&mut holder.chunk, &generator);
        assert_eq!(
            region.get_block_state(&BlockPos::new(0, min_y - 1, 0)),
            BlockState::of(BlockId(794)),
        );
    }

    /// Hostile: the stages the holder does not wire are refused before any work
    /// runs, with a typed error, and the chunk is never stamped past the
    /// supported rung — fresh, and again after a successful NOISE.
    ///
    /// The value-layer boundary is `LIGHT`: the INITIALIZE_LIGHT/LIGHT steps are
    /// wired (`WorldGenContext::generate_through`, engine-gated) but the holder
    /// wires no light engine, so a fresh EMPTY chunk targeting either is
    /// refused as `GenError::LightEngineMissing` before any work runs (the
    /// chunk stays EMPTY). A target past LIGHT (SPAWN/FULL) is out of range
    /// (`UnsupportedStatus`). CARVERS itself is wired (the real
    /// `NoiseBasedChunkGenerator.applyCarvers`, see
    /// `generate_through_carvers_runs_the_real_apply_carvers`), so a fresh
    /// EMPTY chunk targeting it runs BIOMES→NOISE→SURFACE→CARVERS and is
    /// stamped CARVERS. FEATURES is wired-but-blocked (see
    /// `generate_through_features_runs_prologue_then_fails_typed`): the
    /// features body primes the final heightmaps and runs the full dependency-
    /// window prologue. The pinned seed-42 origin continues through the known
    /// zero structure offset to the first unsupported path; unknown offsets
    /// refuse before feature seeding, so the chunk is never stamped FEATURES.
    #[test]
    fn downstream_stages_fail_loudly_and_never_stamp() {
        let generator = test_generator();
        let mut fresh = generator.create_holder(ChunkPos::ZERO);
        // INITIALIZE_LIGHT..LIGHT: the path (through the wired FEATURES step)
        // needs a light engine, and the holder wires none, so the whole path is
        // refused before any work runs. The chunk is untouched.
        for status in [ChunkStatus::InitializeLight, ChunkStatus::Light] {
            assert!(
                matches!(
                    fresh.generate_through(status),
                    Err(GeneratedChunkError::Generation(
                        GenError::LightEngineMissing { .. }
                    ))
                ),
                "target {status:?} must be rejected as LightEngineMissing (no light engine)"
            );
            assert_eq!(fresh.status(), ChunkStatus::Empty);
        }
        // SPAWN/FULL are past the LIGHT range: rejected as UnsupportedStatus.
        for status in [ChunkStatus::Spawn, ChunkStatus::Full] {
            assert!(
                matches!(
                    fresh.generate_through(status),
                    Err(GeneratedChunkError::UnsupportedStatus(s)) if s == status
                ),
                "target {status:?} must be rejected as UnsupportedStatus"
            );
            assert_eq!(fresh.status(), ChunkStatus::Empty);
        }

        // CARVERS is wired: a fresh EMPTY chunk targeting it runs the real
        // carvers body and is stamped CARVERS (see
        // `generate_through_carvers_runs_the_real_apply_carvers`).
        let mut carvers = generator.create_holder(ChunkPos::new(0, 1));
        carvers
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(carvers.status(), ChunkStatus::Carvers);
        // From CARVERS, INITIALIZE_LIGHT is refused as LightEngineMissing
        // before any work, and the persisted status stays CARVERS — never a
        // silent stamp past it.
        let err = carvers
            .generate_through(ChunkStatus::InitializeLight)
            .unwrap_err();
        assert!(matches!(
            err,
            GeneratedChunkError::Generation(GenError::LightEngineMissing { .. })
        ));
        assert_eq!(carvers.status(), ChunkStatus::Carvers);

        // After a real NOISE, requesting a downstream stage still fails loudly
        // and the persisted status stays NOISE — never a silent stamp to FULL.
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        let err = holder.generate_through(ChunkStatus::Full).unwrap_err();
        assert!(matches!(
            err,
            GeneratedChunkError::UnsupportedStatus(ChunkStatus::Full)
        ));
        assert_eq!(holder.status(), ChunkStatus::Noise);
    }

    /// The FEATURES rung runs `addVanillaDecorations`'s full prologue and
    /// dependency-window cache — `Heightmap.primeHeightmaps(chunk,
    /// FINAL_HEIGHTMAPS)` (the `ChunkStatusTasks.generateFeatures` priming),
    /// the decoration-seed derivation (`SectionPos.of(centerPos,
    /// level.getMinSectionY()).origin()` fed to `setDecorationSeed(seed,
    /// originX, originZ)`), the 17x17 `WorldGenRegion` cache (borrowed center,
    /// CARVERS at distances 0/1, and STRUCTURE_STARTS through distance 8), the
    /// Paper-order 3x3 biome-union gather + `retainAll`, and then resolves
    /// generation settings for the FULL `biomeSource.possibleBiomes()` list in
    /// source order (the exact argument Paper's `ChunkGenerator.featuresPerStep`
    /// memoizes, `ChunkGenerator.java` 97-100). Every possible biome (55) now
    /// resolves, so the full-list `FeatureSorter` is built. The seed-42 origin
    /// is the pinned fixture whose structure-consumed count is known to be zero,
    /// so the exact per-step seed loop continues to the first unsupported path.
    #[test]
    fn generate_through_features_stops_at_first_selected_path_mismatch() {
        let generator = test_generator();
        let mut holder =
            generator.create_holder_with_structure_feature_count(ChunkPos::new(0, 0), Some(0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(holder.status(), ChunkStatus::Carvers);

        let err = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must stop at the first selected mismatch");
        match err {
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index,
                global_feature_index,
                feature_key,
            }) => {
                assert_eq!(chunk_pos, ChunkPos::new(0, 0));
                assert_eq!(step_index, 3);
                assert_eq!(global_feature_index, 2);
                assert_eq!(feature_key, "minecraft:monster_room");
            }
            other => panic!("known-zero fixture must reach monster_room boundary; got {other:?}"),
        }

        // The prologue ran faithfully: all four final heightmaps are primed
        // (Java primes them for the decoration bodies to read).
        for ty in FINAL_HEIGHTMAPS {
            assert!(
                holder.chunk.heightmaps()[ty as usize].is_some(),
                "FEATURES must prime the {ty:?} final heightmap before failing typed"
            );
        }
        // The chunk is never stamped FEATURES — the typed error propagates
        // before the status advance, so it stays CARVERS.
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// A non-fixture seed has no structure-manager-backed feature offset. The
    /// typed refusal happens before any per-feature RNG or owner tick mutation.
    #[test]
    fn unknown_structure_consumption_refuses_before_feature_seed() {
        let generator = Arc::new(OverworldGenerator::new(43));
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let err = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("unknown structure consumption must refuse");
        assert!(matches!(
            err,
            GeneratedChunkError::Generation(GenError::StructureDecorationIndexUnavailable {
                chunk_pos
            }) if chunk_pos == ChunkPos::ZERO
        ));
        assert_eq!(holder.chunk.get_block_ticks().count(), 0);
        assert!(holder.chunk.get_post_processing().iter().all(Vec::is_empty));
        assert_eq!(holder.status(), ChunkStatus::Carvers);
    }

    /// The generated placed/configured pair is decoded through registry-backed
    /// holders. The configured feature identity comes from the JSON dispatch
    /// type, and every placement modifier is selected by its own type rather
    /// than by a positional assumption in the generated list.
    #[test]
    fn lake_placed_feature_decodes_through_registry_holders() {
        let generator = test_generator();
        let decoded = decode_placed_feature("minecraft:lake_lava_underground", &generator)
            .expect("the seed-42 lake entry must decode");
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        assert_eq!(placed.placement().len(), 6);
        assert!(matches!(
            placed.feature(),
            Holder::Reference { registry, .. } if *registry == decoded.configured_registry.registry_id()
        ));
        let configured = placed.feature().value(&decoded.configured_registry);
        assert_eq!(
            configured.feature,
            feature_id_from_registry_name("minecraft:lake")
                .expect("the lake dispatch type must be registered")
        );
    }

    /// Force the registry-backed underground lake through its complete
    /// placement chain until a rarity pass reaches the later modifiers. This
    /// keeps the generated-world path covered even though the seed-42 golden
    /// drops the lake rarity roll.
    #[test]
    fn underground_lake_path_reaches_past_a_passing_rarity_roll() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        let origin = BlockPos::new(0, 64, 0);
        let decoded = decode_placed_feature("minecraft:lake_lava_underground", &generator)
            .expect("the underground lake placement list must decode");
        let placed = decoded.placed_holder.value(&decoded.placed_registry);
        let selection_generator = FeatureSelectionGenerator {
            generator: Arc::clone(&generator),
            feature_key: "minecraft:lake_lava_underground",
        };
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let mut passing_seed = None;
        let mut passing_position = None;
        for seed in 0..100_000i64 {
            random.set_seed(seed);
            if let Some(position) = placed.first_placement_position(
                &mut region,
                &selection_generator,
                &mut random,
                &origin,
            ) {
                passing_seed = Some(seed);
                passing_position = Some(position);
                break;
            }
        }
        let passing_seed = passing_seed
            .expect("a forced lake path must pass rarity and reach a terminal position");
        let passing_position = passing_position.expect("the selected lake position must be kept");

        // Re-run the modifier chain with that exact seed to advance the RNG
        // through the rarity and placement modifiers, then run the decoded
        // configured lake leaf in a controlled lava/air cavity. This proves the
        // forced path reaches the real configured feature, not a dummy leaf.
        random.set_seed(passing_seed);
        let selected_position = placed
            .first_placement_position(&mut region, &selection_generator, &mut random, &origin)
            .expect("the forced seed must select the same terminal position");
        assert_eq!(selected_position, passing_position);
        let mut lake_level = LakeTestLevel::with_lava_cavity(passing_position);
        let leaf = PlacedFeature::new(placed.feature.clone(), Vec::new());
        assert!(
            leaf.place_with_biome_check(
                &decoded.configured_registry,
                &mut lake_level,
                &selection_generator,
                &mut random,
                &passing_position,
            ),
            "the forced lake seed must place the decoded configured feature"
        );
        assert!(
            !lake_level.block_ticks.is_empty(),
            "a successful lake write must schedule cave-air ticks"
        );
        assert!(
            !lake_level.post_processing.is_empty(),
            "a successful lake cave-air write must mark post-processing"
        );
    }

    /// The decoration-seed prologue is deterministic and matches the pinned
    /// seed-42 goldens: chunk (0,0) has section origin (0, -64, 0), and
    /// `setDecorationSeed(42, 0, 0)` == 42 == the world seed (both scale terms
    /// vanish), so a seed-42 run at the origin chunk decorates with seed 42.
    /// Chunk (1,0) has origin (16, -64, 0); `setDecorationSeed(42, 16, 0)` is
    /// pinned to the literal golden `-1348197766006825830` (computed against a
    /// live Paper 26.2 load and cross-checked against the crate's
    /// `set_decoration_seed(12345, 3, -7)` golden) — a nonzero-coordinate
    /// literal, not merely "differ from the world seed".
    #[test]
    fn decoration_prologue_matches_pinned_seed42_golden() {
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let origin_seed_00 = random.set_decoration_seed(42, 0, 0);
        assert_eq!(origin_seed_00, 42, "chunk (0,0) must decorate with seed 42");

        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let origin_seed_10 = random.set_decoration_seed(42, 16, 0);
        assert_eq!(
            origin_seed_10, -1348197766006825830,
            "chunk (1,0)'s decoration seed must match the pinned Paper golden"
        );
        // The derived seed is the same regardless of the unique seed base
        // (`set_decoration_seed` resets the source to the world seed first),
        // which is exactly why it is deterministic per chunk.
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        assert_eq!(
            random.set_decoration_seed(42, 16, 0),
            origin_seed_10,
            "the decoration seed must not depend on the unique-seed base"
        );
    }

    /// The decoration-seed prologue is position- and world-seed-sensitive: a
    /// different chunk section or a different world seed must derive a
    /// different decoration seed (non-vacuous — the prologue actually feeds
    /// position and seed into the RNG).
    #[test]
    fn decoration_prologue_seed_is_position_and_seed_sensitive() {
        let seed = |world_seed: i64, chunk_x: i32, chunk_z: i32| {
            WorldgenRandom::new(XoroshiroRandomSource::new(
                random_support::generate_unique_seed(),
            ))
            .set_decoration_seed(world_seed, chunk_x, chunk_z)
        };
        assert_ne!(seed(42, 0, 0), seed(42, 1, 0), "x position must matter");
        assert_ne!(seed(42, 0, 0), seed(42, 0, 1), "z position must matter");
        assert_ne!(seed(42, 0, 0), seed(43, 0, 0), "world seed must matter");
    }

    /// Hostile: a generated ProtoChunk can never enter the ChunkMap authority —
    /// the install gate fails loudly with the chunk's real status, and an empty
    /// map has no placeholder to serve it (genuine-FULL-only installation).
    #[test]
    fn generated_chunk_never_reaches_chunkmap_authority() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.generate_through(ChunkStatus::Noise).expect("NOISE");

        // The install gate rejects the sub-FULL chunk with its real status
        // (never FULL, never a superflat fallback).
        assert!(matches!(
            holder.to_level_chunk(),
            Err(GeneratedChunkError::InstallRequiresFull(ChunkStatus::Noise))
        ));

        // An empty map does not serve the generated position: it holds no
        // superflat placeholder and `install` accepts only the genuine FULL
        // `LevelChunk` type, which no sub-FULL ProtoChunk can produce.
        let map = ChunkMap::empty(4);
        assert!(map.get_chunk(ChunkPos::ZERO).is_none());

        // Positive control: the boundary is real — `install` accepts a genuine
        // `LevelChunk` (the only FULL representation), proving the gate is not
        // "reject everything" but exactly "reject everything except FULL". If a
        // future ProtoChunk-to-LevelChunk fabrication path were added, it would
        // have to produce this genuine FULL chunk to be served.
        let mut map = ChunkMap::empty(4);
        map.install(ChunkPos::ZERO, LevelChunk::new(ChunkPos::ZERO));
        assert!(
            map.get_chunk(ChunkPos::ZERO).is_some(),
            "genuine FULL LevelChunk must be installable"
        );
    }

    /// Ownership: the holder owns its ProtoChunk by value (no `Arc<RwLock>`
    /// game state) while the immutable worldgen config is shared across holders
    /// by `Arc` — the five executor closures (BIOMES, NOISE, SURFACE, CARVERS,
    /// FEATURES) each capture a clone. This test builds its own exclusive
    /// generator (the shared `LazyLock` would be touched by the other parallel
    /// tests, making the strong count global/racy).
    #[test]
    fn holder_owns_chunk_by_value_and_shares_immutable_config() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let base = Arc::strong_count(&generator);
        let holder = generator.create_holder(ChunkPos::new(2, 3));
        // The five executor closures each hold a clone of the shared generator.
        assert_eq!(Arc::strong_count(&generator), base + 5);
        drop(holder);
        assert_eq!(Arc::strong_count(&generator), base);
    }

    /// The FEATURES region uses the complete 17x17 dependency window: it
    /// borrows the center CARVERS chunk, owns CARVERS chunks at distances 1,
    /// and owns STRUCTURE_STARTS chunks through distance 8. The nine
    /// `ChunkPos.rangeClosed(center, 1)` reads used by the biome union still
    /// resolve to their own positions, and the outer dependency ring is also
    /// present at its required status.
    #[test]
    fn feature_region_uses_dependency_window_borrowing_center_and_owning_ring() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let region = compose_feature_region(&mut holder.chunk, &generator);
        assert_eq!(region.get_center(), ChunkPos::new(0, 0));
        for pos in ChunkPos::range_closed(&ChunkPos::new(0, 0), 1) {
            let chunk = region.get_chunk(pos.x(), pos.z());
            assert_eq!(
                chunk.get_pos(),
                pos,
                "the dependency window must serve every 3x3 chunk at its own position"
            );
        }
        assert_eq!(
            region
                .try_get_chunk(8, 0, ChunkStatus::StructureStarts, true)
                .expect("the outer FEATURES dependency ring must be present")
                .get_pos(),
            ChunkPos::new(8, 0)
        );
        assert!(
            region
                .try_get_chunk(9, 0, ChunkStatus::Empty, true)
                .is_err(),
            "the FEATURES cache must stop at the direct dependency radius"
        );
    }

    /// `WorldGenRegion` retains block ticks through the same
    /// `ProtoChunkTicks.schedule` path Paper uses. The container stores zero
    /// relative delay and deduplicates by block type and position.
    #[test]
    fn feature_region_retains_deduplicated_block_ticks() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        let mut region = compose_feature_region(&mut holder.chunk, &generator);
        let pos = BlockPos::new(17, 64, 2);
        rivet_world::level::WorldGenLevel::schedule_block_tick(
            &mut region,
            &pos,
            Blocks::CAVE_AIR,
            7,
        );
        rivet_world::level::WorldGenLevel::schedule_block_tick(
            &mut region,
            &pos,
            Blocks::CAVE_AIR,
            2,
        );

        let owner_ticks = region.get_chunk(1, 0).get_block_ticks().scheduled_ticks();
        assert_eq!(owner_ticks.len(), 1);
        assert_eq!(owner_ticks[0].r#type, Blocks::CAVE_AIR);
        assert_eq!(owner_ticks[0].pos, pos);
        assert_eq!(owner_ticks[0].delay, 0);
        assert!(
            region
                .get_chunk(0, 0)
                .get_block_ticks()
                .scheduled_ticks()
                .is_empty(),
            "a ring tick must not land in the generating chunk"
        );
    }

    /// The seed-42 origin 3x3 biome union — the exact set the seed-42 (0,0)
    /// chunk decorates with — is `{minecraft:beach, minecraft:dark_forest,
    /// minecraft:lush_caves, minecraft:river}` (the pinned union from the live
    /// Paper load). All four resolve in `BIOME_GENERATION_SETTINGS_BY_NAME`, and
    /// so does every biome in the FULL source list (all 55), so settings
    /// resolution never blocks; the first typed blocker is the first executing
    /// placed feature's value decode, as
    /// `generate_through_features_runs_prologue_then_fails_typed` asserts.
    #[test]
    fn seed42_origin_biome_union_is_the_exact_paper_set() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let region = compose_feature_region(&mut holder.chunk, &generator);
        let possible_biomes = gather_possible_biomes(&region, &generator);
        let mut names: Vec<&str> = possible_biomes.into_iter().collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "minecraft:beach",
                "minecraft:dark_forest",
                "minecraft:lush_caves",
                "minecraft:river",
            ],
            "the seed-42 origin union must be the pinned Paper set, retainAll-ed against the biome source"
        );
        for name in &names {
            assert!(
                BIOME_GENERATION_SETTINGS_BY_NAME.contains_key(name),
                "every union biome must resolve its generation settings"
            );
        }
    }

    /// The FeatureSorter orders the step-1 features by *global first-appearance
    /// index*, not by registry id: lake_lava_underground (id 80) gets global
    /// index 0 and lake_lava_surface (id 79) index 1, so the sorted
    /// possible-features list places the underground lava lake first. The
    /// decoration then seeds the random with
    /// `setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)`;
    /// pin the exact RNG state that produces.
    ///
    /// This drives the sorter exactly like production (`run_biome_decoration`):
    /// from the FULL `biomeSource.possibleBiomes()` list in source order (all
    /// 55 now resolve their generated settings), not the 3x3 union — the union
    /// only picks which global indices execute. `mushroom_fields` (source index
    /// 0) still carries the two lava lakes at step 1 first, so their global
    /// first-appearance indices are 0/1.
    #[test]
    fn feature_sorter_orders_lava_lakes_by_global_index_and_seeds_them() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");

        let placed_registry_id = RegistryBuilder::new(&*PLACED_FEATURE).registry_id();
        let mut placed_by_id = HashMap::new();
        let mut settings_sources = Vec::new();
        for holder in generator.biome_source().possible_biomes() {
            let dense = dense_biome_id(&holder) as usize;
            let name = *BIOME_BY_ID
                .get(dense)
                .expect("every possible biome has a dense registry id");
            settings_sources.push((
                resolve_biome_settings(name, placed_registry_id, &mut placed_by_id)
                    .expect("every full-list biome resolves its generated settings"),
                name,
            ));
        }
        assert_eq!(settings_sources.len(), 55);
        let feature_list = build_features_per_step(
            &settings_sources,
            |(settings, _)| settings.features(),
            false,
        );

        // Every union biome has step 0 empty; step 1 (LAKES) holds the two
        // lava lakes.
        assert!(
            feature_list[0].features.is_empty(),
            "all four union biomes' step 0 (RAW_GENERATION) is empty"
        );
        let step1 = &feature_list[1];
        let global_of = |id: u32| step1.index_mapping(&Holder::reference(placed_registry_id, id));
        assert_eq!(
            global_of(80),
            Some(0),
            "lake_lava_underground is the step-1 global index 0"
        );
        assert_eq!(
            global_of(79),
            Some(1),
            "lake_lava_surface is the step-1 global index 1"
        );
        assert_eq!(
            placed_by_id.get(&80).copied(),
            Some("minecraft:lake_lava_underground"),
            "the reverse id→key map names the underground lava lake"
        );

        // The exact per-feature seed: `setFeatureSeed(decorationSeed, index,
        // step)` sets `decorationSeed + index + 10000 * step`. For chunk (0,0)
        // the decoration seed is 42, so lake_lava_underground (index 0, step 1)
        // seeds with 10042 and the RNG state matches a fresh source seeded
        // directly with that value.
        let mut reference = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        reference.set_seed(10042);
        let mut decorated = WorldgenRandom::new(XoroshiroRandomSource::new(
            random_support::generate_unique_seed(),
        ));
        let decoration_seed = decorated.set_decoration_seed(42, 0, 0);
        assert_eq!(decoration_seed, 42);
        decorated.set_feature_seed(decoration_seed, 0, 1);
        assert_eq!(
            decorated.next_int(),
            reference.next_int(),
            "setFeatureSeed(42, 0, 1) must seed the exact RNG state placement would consume"
        );
    }
}
