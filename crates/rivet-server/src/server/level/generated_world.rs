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
//! BIOMES→NOISE through the `GENERATION_PYRAMID` executor.
//!
//! ## The typed downstream boundary
//!
//! `WorldGenContext::generate_through` refuses any target past `NOISE` with a
//! `GenError::UnsupportedStatus` *before* running work (the SURFACE..FULL
//! stages are the unwired #185 ladder), so the holder's
//! [`GenerationChunkHolder::generate_through`] surfaces a typed
//! [`GeneratedChunkError::UnsupportedStatus`] rather than stamping a status
//! that was never generated. And a generated chunk can never enter the server
//! authority: [`ChunkMap::install`] accepts only a `LevelChunk` (the FULL chunk
//! type), and no conversion from a sub-FULL `ProtoChunk` exists — the
//! [`GenerationChunkHolder::to_level_chunk`] gate fails loudly with
//! [`GeneratedChunkError::InstallRequiresFull`] instead of fabricating a FULL
//! chunk or falling back to superflat.
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

use std::fmt;
use std::sync::Arc;

use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::ChunkPos;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_world::biome::BiomeResolver;
use rivet_world::biome::biome_manager::NoiseBiomeSource;
use rivet_world::biome::climate::Sampler;
use rivet_world::biome::generated_biome_source::{dense_biome_id, overworld_biome_source};
use rivet_world::biome::multi_noise_biome_source::MultiNoiseBiomeSource;
use rivet_world::block::blocks::Blocks;
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
use rivet_world::levelgen::heightmap::Types;
use rivet_world::levelgen::noise::registry_keys::NOISE_SETTINGS;
use rivet_world::levelgen::noisegen::noise_based_chunk_generator::NoiseBasedChunkGenerator;
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::random_state::RandomState;

use crate::server::level::level_chunk::{LevelChunk, StructureKey};

/// The overworld generated-chunk error surface — every failure is typed, never
/// a silent fallback.
#[derive(Debug)]
pub enum GeneratedChunkError {
    /// The status executor refused the promotion: a missing data prerequisite
    /// (`GenError::BiomesNotGenerated`/`DataNotCarried`), a demotion, or a
    /// wired-task mismatch. The chunk is left untouched.
    Generation(GenError),
    /// A target past `NOISE` — the executor rejected it before running any
    /// work. The SURFACE..FULL stages are unwired (#185); naming the requested
    /// status makes the downstream boundary explicit.
    UnsupportedStatus(ChunkStatus),
    /// The genuine-FULL-only install gate: a generated chunk is a `ProtoChunk`
    /// through `NOISE` and cannot be converted into the `LevelChunk` (FULL)
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
                "generating to {status:?} is unsupported: the SURFACE..FULL stages are unwired (RivetTodo #185)"
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
            seed,
        }
    }

    /// The seed this generator was realized for.
    pub fn seed(&self) -> i64 {
        self.seed
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
#[derive(Debug)]
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
/// `StructureKey`) and the BIOMES→NOISE executor over the shared worldgen
/// objects.
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
    /// empty blender.
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
            Blocks::AIR.default_block_state(),
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
        );
        GenerationChunkHolder { chunk, context }
    }

    /// The chunk's persisted status — `EMPTY` before any step, `NOISE` after a
    /// successful BIOMES→NOISE run, and never `FULL` (the executor refuses to
    /// stamp it).
    pub fn status(&self) -> ChunkStatus {
        self.chunk.get_persisted_status()
    }

    /// Drive the chunk from its current persisted status through `target`
    /// (inclusive). `target ≤ NOISE` runs real work; `target ≥ SURFACE` is
    /// rejected by the executor before any work with a typed
    /// [`GeneratedChunkError::UnsupportedStatus`] — the chunk is left
    /// untouched.
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
    /// `LevelChunk` (FULL), and a generated chunk is a `ProtoChunk` through
    /// `NOISE`. No conversion from a sub-FULL `ProtoChunk` exists or may be
    /// added without the unwired SURFACE..FULL stages (RivetTodo #185), so this
    /// always fails loudly with the chunk's real status — never stamping FULL
    /// and never falling back to superflat.
    pub fn to_level_chunk(&self) -> Result<LevelChunk, GeneratedChunkError> {
        Err(GeneratedChunkError::InstallRequiresFull(
            self.chunk.get_persisted_status(),
        ))
    }
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

    /// The executor drives a fresh chunk EMPTY→BIOMES→NOISE, and a second run
    /// to the same status is an idempotent no-op — the Paper status-ladder
    /// contract through the wired rungs.
    #[test]
    fn generate_through_biomes_then_noise() {
        let generator = test_generator();
        let mut holder = generator.create_holder(ChunkPos::new(1, -2));
        assert_eq!(holder.status(), ChunkStatus::Empty);

        holder
            .generate_through(ChunkStatus::Biomes)
            .expect("BIOMES");
        assert_eq!(holder.status(), ChunkStatus::Biomes);

        holder.generate_through(ChunkStatus::Noise).expect("NOISE");
        assert_eq!(holder.status(), ChunkStatus::Noise);

        // Re-running to the same status is an idempotent no-op (the chunk is
        // already at NOISE).
        holder
            .generate_through(ChunkStatus::Noise)
            .expect("idempotent");
        assert_eq!(holder.status(), ChunkStatus::Noise);
    }

    /// Hostile: a downstream target (SURFACE and beyond) is rejected before any
    /// work runs, with the typed unsupported status, and the chunk is never
    /// stamped past NOISE — fresh, and again after a successful NOISE.
    #[test]
    fn downstream_stages_fail_loudly_and_never_stamp() {
        let generator = test_generator();
        // A fresh chunk: the executor rejects SURFACE..FULL with the typed
        // boundary before any work, so the chunk stays EMPTY.
        let mut fresh = generator.create_holder(ChunkPos::ZERO);
        for status in [
            ChunkStatus::Surface,
            ChunkStatus::Carvers,
            ChunkStatus::Features,
            ChunkStatus::InitializeLight,
            ChunkStatus::Light,
            ChunkStatus::Spawn,
            ChunkStatus::Full,
        ] {
            assert!(
                matches!(
                    fresh.generate_through(status),
                    Err(GeneratedChunkError::UnsupportedStatus(s)) if s == status
                ),
                "target {status:?} must be rejected as UnsupportedStatus"
            );
            assert_eq!(fresh.status(), ChunkStatus::Empty);
        }

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
        let mut map = ChunkMap::empty(4);
        assert!(map.get_chunk(ChunkPos::ZERO).is_none());
        let _: &mut ChunkMap = &mut map; // (install's LevelChunk-only signature is the compile-time gate)
    }

    /// Ownership: the holder owns its ProtoChunk by value (no `Arc<RwLock>`
    /// game state) while the immutable worldgen config is shared across holders
    /// by `Arc` — the executor closures each capture a clone. This test builds
    /// its own exclusive generator (the shared `LazyLock` would be touched by
    /// the other parallel tests, making the strong count global/racy).
    #[test]
    fn holder_owns_chunk_by_value_and_shares_immutable_config() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let base = Arc::strong_count(&generator);
        let holder = generator.create_holder(ChunkPos::new(2, 3));
        // The two executor closures each hold a clone of the shared generator.
        assert_eq!(Arc::strong_count(&generator), base + 2);
        drop(holder);
        assert_eq!(Arc::strong_count(&generator), base);
    }
}
