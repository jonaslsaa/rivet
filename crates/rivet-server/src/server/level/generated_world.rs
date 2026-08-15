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
//! bounded region-backed 3x3 composition — the decoration-seed derivation
//! (`SectionPos.of(centerPos, level.getMinSectionY()).origin()` fed to
//! `setDecorationSeed`), a `WorldGenRegion` that borrows the center chunk and
//! owns eight ring chunks generated EMPTY→CARVERS through the same real bodies
//! the other closures wire, and the Paper-order biome-union gather +
//! `retainAll`. It then resolves generation settings for the FULL
//! `biomeSource.possibleBiomes()` list in source order and builds the
//! FeatureSorter once from it (Paper's `ChunkGenerator.featuresPerStep`,
//! `ChunkGenerator.java` 97-100 — the 3x3 union only picks which feature
//! indices execute per step). The generated feature tables cover EVERY
//! overworld possible biome (55 — the full list, not the reachable subset),
//! so the full list resolves and the run proceeds to the per-step loop, where
//! it fails typed (`GenError::FeaturePlacementDecode`) at the exact first
//! placed feature whose value decode is unavailable — seed-42 chunk (0,0):
//! step 1 (LAKES), global feature index 0, `minecraft:lake_lava_underground`
//! (#126). The chunk stays CARVERS. The INITIALIZE_LIGHT/
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
//! ## The deferred `GenerationChunkHolderView` seam
//!
//! The `WorldGenRegion` view contract is typed to the server's dense chunk
//! (`ChunkAccess<StateId, ServerBiomeId, StructureKey>`); the worldgen chunk
//! carries `BlockState` + `section_reconstruction::BiomeId`. Bridging them is
//! the `ChunkAccess::map_values` conversion, which consumes the chunk and is
//! wired only from FULL reconstructions (`LevelChunk::from_bridge`) — a sub-FULL
//! `ProtoChunk` has no such path, so the holder intentionally does not
//! implement the view trait (RivetTodo #185: the status executor that completes
//! a chunk to FULL and bridges it lands with the `.chunk.generator` pipeline
//! unit). The holder hands out the chunk's status and typed generation results
//! instead.
//!
//! Ownership follows OWNERSHIP.md: the generator/biome source are immutable
//! per-world config shared by `Arc` (no `Arc<RwLock>` game state — the only
//! interior mutability is `RandomState`'s own uncontended noise cache), the
//! holder and its `ProtoChunk` live on the sync tick thread by value.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

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
    BIOME_GENERATION_SETTINGS_BY_NAME, PLACED_FEATURE_BY_NAME,
};
use rivet_registry::holder::Holder;
use rivet_registry::holder::RegistryId;
use rivet_registry::holder_lookup::HolderGetter;
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
use rivet_world::levelgen::feature::registry_keys::PLACED_FEATURE;
use rivet_world::levelgen::generation_step::Decoration;
use rivet_world::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types};
use rivet_world::levelgen::noise::registry_keys::NOISE_SETTINGS;
use rivet_world::levelgen::noisegen::noise_based_chunk_generator::NoiseBasedChunkGenerator;
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::random_state::RandomState;
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
    /// `GenError::LightEngineMissing`, and the wired FEATURES rung fails typed
    /// as `GenError::FeaturePlacementDecode` at the first placed-feature value
    /// decode — see [`GenerationChunkHolder::new`].)
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
    /// Java's `ChunkStatusTasks.generateFeatures` — it runs
    /// `addVanillaDecorations`'s region-free prologue (the four
    /// `FINAL_HEIGHTMAPS` and the decoration-seed derivation) and then fails
    /// typed at the first genuinely unavailable dependency: the 3x3
    /// biome-union gather, whose `level.getChunk` reads go through the bounded
    /// `WorldGenRegion`'s `StaticCache2D` neighbor-chunk cache the holder
    /// neither owns nor implements the view trait for (the deferred
    /// `GenerationChunkHolderView` seam — see the module doc).
    pub fn new(pos: ChunkPos, generator: Arc<OverworldGenerator>) -> Self {
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
                // section-origin decoration-seed derivation, the bounded 3x3
                // region (the borrowed center chunk + eight owned ring chunks
                // generated EMPTY→CARVERS through the same real bodies the
                // other closures wire), the Paper-order biome-union gather +
                // `retainAll`, the FULL-source-list settings resolution
                // (`ChunkGenerator.featuresPerStep`, `ChunkGenerator.java`
                // 97-100) and FeatureSorter, and the exact per-feature seeds —
                // and then fails typed (`GenError::FeaturePlacementDecode`) at
                // the exact first placed feature whose value decode is
                // unavailable (seed-42 chunk (0,0): step 1, global index 0,
                // `minecraft:lake_lava_underground` — #126). It must never be
                // "improved" into a silent skip or a blanket UnsupportedTask.
                // The closure captures one generator clone (the free helper is
                // why the ownership test's `strong_count == base + 5` holds).
                let generator = Arc::clone(&generator);
                move |chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>| {
                    run_biome_decoration(chunk, &generator)
                }
            },
        );
        GenerationChunkHolder { chunk, context }
    }

    /// The chunk's persisted status — `EMPTY` before any step, `CARVERS` after a
    /// successful BIOMES→NOISE→SURFACE→CARVERS run, and never `FULL` (the
    /// executor refuses to stamp it). A FEATURES run primes the final heightmaps,
    /// drives the bounded 3x3 region, resolves the FULL possible-biome settings
    /// and builds the FeatureSorter, and then fails typed at the first placed
    /// feature whose value decode is unavailable (`FeaturePlacementDecode`, the
    /// `#126` blocker — seed-42: `lake_lava_underground`), so the chunk is never
    /// stamped FEATURES.
    pub fn status(&self) -> ChunkStatus {
        self.chunk.get_persisted_status()
    }

    /// Drive the chunk from its current persisted status through `target`
    /// (inclusive). The BIOMES→NOISE→SURFACE→CARVERS task bodies are wired (an
    /// EMPTY chunk can reach CARVERS); the FEATURES task body is wired (it runs
    /// Java's `ChunkStatusTasks.generateFeatures` + `addVanillaDecorations`'s
    /// bounded 3x3 composition and then fails typed at the first placed feature
    /// whose value decode is unavailable — see
    /// [`GenerationChunkHolder::new`]). A
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
/// The per-step loop then runs the union's placed features in global-index
/// order and fails typed (`GenError::FeaturePlacementDecode`) at the exact
/// first placed feature whose value decode is unavailable — seed-42 chunk
/// (0,0), step 1 (LAKES), global index 0: `minecraft:lake_lava_underground`
/// (`#126`). The generated settings tables are the full 55-biome surface (no
/// `SettingsNotGenerated`), so this decode boundary is reached deterministically
/// every run. No placement ever runs, no phf index ever panics, and no biome is
/// fabricated or silently skipped.
///
/// Compose the bounded 3x3 `WorldGenRegion` `addVanillaDecorations` reads:
/// the borrowed center chunk at CARVERS (the executor's ordering guard) plus
/// the eight owned ring chunks generated EMPTY→CARVERS. The holders are laid
/// out in `StaticCache2D::from_entries`'s storage order (X-outer, Z-inner —
/// the row-major `(x - minX) * sizeZ + (z - minZ)` index), NOT
/// `ChunkPos::range_closed`'s X-fastest order, which is the transpose: a
/// `getChunk(x, z)` would otherwise read the neighbor built for `(z, x)`.
fn compose_feature_region<'a>(
    chunk: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey> {
    let center_pos = chunk.get_pos();
    let center_status = chunk.get_persisted_status();
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> + 'a>,
    > = Vec::with_capacity(9);
    // The holders must be in `StaticCache2D::from_entries`'s storage order
    // (X-outer, Z-inner — index `(x - minX) * sizeZ + (z - minZ)`), not
    // `ChunkPos::range_closed`'s X-fastest order: the two are transposes, and a
    // `getChunk(x, z)` would otherwise read the neighbor built for the
    // transposed `(z, x)`. Ring positions are gathered in storage order first so
    // the center chunk's borrow is taken exactly once, at its own (index-4) slot.
    let mut ring_positions = Vec::with_capacity(8);
    for dx in -1..=1 {
        for dz in -1..=1 {
            let pos = ChunkPos::new(
                center_pos.x().wrapping_add(dx),
                center_pos.z().wrapping_add(dz),
            );
            if pos != center_pos {
                ring_positions.push(pos);
            }
        }
    }
    for pos in ring_positions {
        holders.push(Box::new(OwnedHolder::new(
            generate_ring_chunk(pos, generator),
            ChunkStatus::Carvers,
        )));
    }
    holders.insert(
        4,
        Box::new(CenterHolder::new(chunk.base_mut(), center_status)),
    );
    let cache = StaticCache2D::from_entries(center_pos.x() - 1, center_pos.z() - 1, 3, 3, holders);
    WorldGenRegion::new(
        cache,
        center_pos,
        GENERATION_PYRAMID
            .get_step_to(ChunkStatus::Features)
            .clone(),
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

fn run_biome_decoration(
    chunk: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
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

    let region = compose_feature_region(chunk, generator);
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

    // The per-step loop — Paper's `addVanillaDecorations`. The structure loop
    // is skipped (the port has no structure manager; Java's
    // `structureManager.shouldGenerateStructures()` gate is a faithful no-op,
    // the #185 structures deferral).
    let generation_steps = Decoration::VALUES.len().max(feature_list.len());
    // Paper walks steps in ascending order and, within a step, the sorted
    // global feature indices of the *union* biomes mapped through the full-list
    // sorter's `indexMapping` — the union selects which indices execute, the
    // sorter was built from the full possible-biome list.
    let first_feature = (0..generation_steps).find_map(|step_index| {
        if step_index >= feature_list.len() {
            return None;
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
        possible_features_this_step
            .into_iter()
            .next()
            .map(|global_feature_index| (step_index, global_feature_index))
    });
    let Some((step_index, global_feature_index)) = first_feature else {
        // No feature in any union step. The typed error names the region
        // rather than silently passing.
        return Err(GenError::FeaturePlacementDecode {
            chunk_pos: center_pos,
            step_index: 0,
            global_feature_index: 0,
            feature_key: "minecraft:unknown",
        });
    };
    let feature = &feature_list[step_index].features[global_feature_index];
    let feature_key = match feature {
        Holder::Reference { id, .. } => {
            placed_by_id.get(id).copied().unwrap_or("minecraft:unknown")
        }
        Holder::Direct(_) => "minecraft:unknown",
    };
    // `setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)` — the
    // exact per-feature seed Paper sets before placement. The Paper
    // configurable-feature-seed override is a no-op (no paper config, so
    // `featurePopulationSeed == decorationSeed`).
    random.set_feature_seed(
        decoration_seed,
        global_feature_index as i32,
        step_index as i32,
    );
    Err(GenError::FeaturePlacementDecode {
        chunk_pos: center_pos,
        step_index,
        global_feature_index,
        feature_key,
    })
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
    use rivet_world::levelgen::heightmap::Types;

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
    /// features body primes the final heightmaps, runs the bounded 3x3 region
    /// and per-step loop, and fails typed at the first placed feature whose
    /// value decode is unavailable, so the chunk is never stamped FEATURES.
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
    /// region-backed 3x3 gather — `Heightmap.primeHeightmaps(chunk,
    /// FINAL_HEIGHTMAPS)` (the `ChunkStatusTasks.generateFeatures` priming),
    /// the decoration-seed derivation (`SectionPos.of(centerPos,
    /// level.getMinSectionY()).origin()` fed to `setDecorationSeed(seed,
    /// originX, originZ)`), the bounded `WorldGenRegion` (borrowed center +
    /// eight owned ring chunks generated EMPTY→CARVERS), the Paper-order
    /// biome-union gather + `retainAll`, and then resolves generation settings
    /// for the FULL `biomeSource.possibleBiomes()` list in source order (the
    /// exact argument Paper's `ChunkGenerator.featuresPerStep` memoizes,
    /// `ChunkGenerator.java` 97-100). Every possible biome (55) now resolves,
    /// so the full-list `FeatureSorter` is built, the per-step loop maps the
    /// 3x3 union through it, and the FIRST executing global feature index fails
    /// typed at `GenError::FeaturePlacementDecode` — the new seed-42 frontier.
    /// For seed 42 chunk (0,0) that is step 1 (lakes), global feature index 0,
    /// placed key `minecraft:lake_lava_underground` (every non-deep-dark biome
    /// carries the two lava lakes at step 1, and the ocean union selects index
    /// 0 first). The chunk is never stamped FEATURES (it stays CARVERS) — no
    /// silent skip, no blanket `UnsupportedTask`, no phf panic, and no
    /// fabricated/skipped biome.
    #[test]
    fn generate_through_features_runs_prologue_then_fails_typed() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(0, 0));
        holder
            .generate_through(ChunkStatus::Carvers)
            .expect("CARVERS");
        assert_eq!(holder.status(), ChunkStatus::Carvers);

        let err = holder
            .generate_through(ChunkStatus::Features)
            .expect_err("FEATURES must fail typed at the first real blocker");
        match err {
            GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index,
                global_feature_index,
                feature_key,
            }) => {
                assert_eq!(chunk_pos, ChunkPos::new(0, 0));
                assert_eq!(step_index, 1, "first executing step must be lakes");
                assert_eq!(
                    global_feature_index, 0,
                    "first global feature index must be 0"
                );
                assert_eq!(
                    feature_key, "minecraft:lake_lava_underground",
                    "first placed feature key must be the first lava lake"
                );
            }
            other => panic!(
                "FEATURES must fail with FeaturePlacementDecode at lake_lava_underground; \
                 got {other:?}"
            ),
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

    /// The FEATURES region is a bounded 3x3 that borrows the center chunk (the
    /// CARVERS chunk the executor is decorating) and owns the eight ring chunks
    /// generated EMPTY→CARVERS through the same real bodies. Every one of the
    /// nine `ChunkPos.rangeClosed(center, 1)` reads resolves to a chunk at its
    /// own position — the `level.getChunk` surface `addVanillaDecorations` reads
    /// the biome union from (non-vacuous: a region that silently dropped the
    /// center borrow or generated the ring with different positions would fail
    /// the position equality below).
    #[test]
    fn feature_region_is_bounded_3x3_borrowing_center_and_owning_ring() {
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
                "the bounded region must serve every 3x3 chunk at its own position"
            );
        }
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
