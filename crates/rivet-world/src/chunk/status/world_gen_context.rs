//! Port of `net.minecraft.world.level.chunk.status.WorldGenContext` (MC 26.2)
//! — the record of per-step worldgen dependencies — plus the value-layer
//! executor seam that runs the generation DAG through SPAWN.
//!
//! Java: `WorldGenContext.java` in `working/Paper` — a 6-field record
//! `(ServerLevel, ChunkGenerator, StructureTemplateManager,
//! ThreadedLevelLightEngine, Executor, UnsavedListener)`. In the value layer
//! the server/light/template surfaces do not exist yet (they defer with their
//! owning units), and the full `ChunkGenerator` is owned by the generator wave
//! (#306/#185 — today only the `&dyn ChunkGenerator` seam in
//! `chunk::chunk_generator` exists), so the record is reduced to the task seam
//! the generation pyramid the executor drives actually needs: the caller-supplied
//! closures that perform the BIOMES, NOISE, SURFACE, CARVERS, and FEATURES
//! work, plus the [`StarLightProvider`] the INITIALIZE_LIGHT/LIGHT tasks route
//! to, plus the optional spawn seam the SPAWN task routes to. FULL promotion is
//! a consuming holder boundary after SPAWN, not a borrowed executor seam. The
//! full record shape returns with the `mc.world.level.chunk.generator` wave
//! (RivetTodo #185). The closures are `'static`-owned (a value-layer
//! simplification): the real worldgen bodies borrow server/region state and run
//! in the #185 scheduler realization, not through this seam.
//!
//! The seam is the value-layer dispatch surface for the task identities and the
//! `BIOMES`-before-`NOISE` ordering contract (spec §3.2); the #185 scheduler
//! realization (spec §10 slice 3) drives the same task identities through the
//! holder/chunk-generation path. The seam is *honest* about the ordering:
//!
//! 1. `generate_through` walks the pyramid in status order, so it cannot skip
//!    the BIOMES step on the way to NOISE.
//! 2. The NOISE task dispatch requires the chunk's persisted status to already
//!    be at/after `BIOMES` — the record the BIOMES dispatch writes. A chunk is
//!    never labeled `NOISE` unless the BIOMES task actually ran (either in this
//!    generation run or in the generation that produced its persisted status).
//! 3. A pass-through step (one that produces no data) is refused when it would
//!    advance a chunk to `BIOMES`/`NOISE` that does not already carry that
//!    status — so the LOADING pyramid's loading stubs cannot fabricate a
//!    NOISE-labeled chunk from an `EMPTY` one.
//! 4. `generate_through` validates the *whole* path against the projected
//!    persisted status before any step runs, so a refused promotion (including
//!    a mispositioned `NOISE` task) leaves the chunk untouched.
//! 5. A wired generation task is honored only at its canonical rung — the
//!    `BIOMES` rung carries `GenerateBiomes`, the `NOISE` rung
//!    `GenerateNoise`. A malformed crate-internal pyramid that installs one
//!    elsewhere (e.g. `GenerateBiomes` at the `NOISE` rung) is refused before
//!    any work runs, so a chunk is never labeled `NOISE` by a task that
//!    produces no noise data.
//! 6. The INITIALIZE_LIGHT and LIGHT tasks are wired to the [`StarLightProvider`]
//!    seam exactly as `ChunkStatusTasks.initializeLight`/`light` + the Moonrise
//!    `ChunkLightTask.LightTask` do (spec §3.4): the INITIALIZE_LIGHT body
//!    records the engine (Java's `setLightEngine`; Paper's `initializeLight`
//!    completes immediately — the real compute is the Moonrise `ChunkLightTask`
//!    at the LIGHT rung), dereferencing it so a missing engine — or a
//!    provider-less one, treated the same — errors like Java's NPE but
//!    computing nothing; the LIGHT body preserves Java's
//!    load-vs-compute branch — a chunk that is already lighted (`isLighted` =
//!    persisted status at/after `LIGHT` && light-correct, the value-layer flag
//!    `ChunkAccess.isLightCorrect`) is *loaded* through the seam
//!    (`forceLoadInChunk` + `checkChunkEdges`), a chunk that is not is
//!    recomputed (`setLightCorrect(false)` → `lightChunk` →
//!    `setLightCorrect(true)`), and a ProtoChunk at `LIGHT.getParent()`
//!    (`INITIALIZE_LIGHT`) advances to `LIGHT` — `ChunkLightTask`'s status
//!    advance, and the only status advance that happens inside a light task.
//!    Fresh light data is published by the attached `rivet-server`
//!    `SkyLightProvider`; persisted loads require the provider's explicit edge-
//!    reconciliation capability and otherwise return a typed refusal. The LIGHT
//!    step also dispatches a caller-attached light write-back seam
//!    ([`WorldGenContext::with_light`] — `rivet-server`'s `GeneratedLightBridge`,
//!    which maps the generated center to runtime values, lights it, and persists
//!    the computed sky nibbles/emptiness/status back into the chunk); without it
//!    the engine-position path above runs. The provider seam still owns the
//!    concrete runtime storage and keeps the `GenerationChunkHolder`/`ChunkMap`
//!    boundary with #185.
//!
//! A non-`FULL` target already at/after the chunk's status is handled before
//! any work: `target == current` is an idempotent no-op (so a loaded chunk
//! persisted at `LIGHT` can be confirmed through the LOADING pyramid), and a
//! lower target is a demotion error rather than an unwired-status error. A
//! borrowed request targeting `FULL` is always refused, including a proto
//! already persisted at `FULL`, because `FULL` is the consuming
//! `ProtoChunk`→`LevelChunk` representation boundary.
//!
//! The SURFACE task body is wired (Java's `ChunkStatusTasks.generateSurface` →
//! `NoiseBasedChunkGenerator.buildSurface`), so the executor runs it at the
//! SURFACE rung and stamps the chunk `SURFACE` only after it returns. The
//! CARVERS task body is wired (Java's `ChunkStatusTasks.generateCarvers` →
//! `NoiseBasedChunkGenerator.applyCarvers`), stamped `CARVERS` only after it
//! returns; the ordering guard keeps it from running before the SURFACE task
//! ran (the carvers consume the SURFACE-produced top material). The FEATURES
//! task body is wired (Java's `ChunkStatusTasks.generateFeatures`, whose real
//! decoration body the caller — `rivet-server`'s generated-world — supplies
//! through the [`FeaturesSeam`], since it owns the bounded `WorldGenRegion`),
//! stamped `FEATURES` only after it returns; the ordering guard keeps it from
//! running before the CARVERS task ran (the decoration bodies read the
//! CARVERS-produced block data). The SPAWN task body is seam-wired like the
//! light steps: dispatching [`ChunkStatusTask::GenerateSpawn`] runs the
//! caller-supplied spawn seam (Java's `generator.spawnOriginalMobs` — the
//! natural spawner reads the LIGHT-produced light data, so the ordering guard
//! keeps it from running before the LIGHT task ran), attached through
//! [`WorldGenContext::with_spawn`].
//! FULL remains a scheduler-owned consuming promotion boundary rather than an
//! executor task: Paper changes the SPAWN `ProtoChunk` representation into a
//! FULL `LevelChunk`, so the G4 holder consumes the proto after lighting/spawn
//! completion. The generic executor never borrows, stamps, or recovers a
//! generated proto at `FULL`.

use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::status::chunk_status_task::ChunkStatusTask;
use crate::chunk::status::chunk_status_tasks;
use crate::chunk::status::chunk_step::ChunkStep;
use crate::chunk::status::{ChunkPyramid, ChunkStatus};
use crate::lighting::level_light_engine::LevelLightEngine;
use crate::lighting::star_light_provider::LightProviderError;

/// The region-backed decoration seam's failure modes — the typed failures the
/// `FEATURES` closure body (`addVanillaDecorations`) returns. The `ChunkPos`
/// comes from `rivet_registry::core`; the closure body (in `rivet-server`) owns
/// the `WorldGenRegion` the decoration runs against, so this crate carries only
/// the error shape.
use rivet_registry::core::ChunkPos;

/// The `generateBiomes` seam closure type.
type BiomesSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateNoise` seam closure type.
type NoiseSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateSurface` seam closure type.
type SurfaceSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateCarvers` seam closure type.
type CarversSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateFeatures` seam closure type — returns the decoration body's
/// typed failure (the region-backed neighbor-cache seam defers with #126, so
/// the body fails typed instead of panicking or silently skipping — for
/// seed-42 `GenError::FeaturePlacementDecode` at the first real placed-feature
/// value decode, with `GenError::SettingsNotGenerated` remaining as the
/// defensive path for a biome the tables do not carry).
type FeaturesSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError>;

/// The `generateSpawn` seam closure type — runs Java's
/// `ChunkStatusTasks.generateSpawn` (the caller's `generator.spawnOriginalMobs`
/// natural-spawner body, which spawns CREATURE mobs against the LIGHT-produced
/// light data through a bounded `WorldGenRegion`). The closure is caller-owned
/// (the `WorldGenRegion` the spawner reads is a bounded region-backed value in
/// `rivet-server`, exactly like the FEATURES body), so the executor carries
/// only the closure and the ordering guard.
type SpawnSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError>;

/// The `LIGHT` write-back seam — runs the caller-owned copy of the light
/// result into the chunk (Java's `ChunkLightTask.LightTask` computing light into
/// the `ChunkAccess` it is handed). The generic value layer cannot reference
/// `rivet-server`'s `GeneratedProto`/`RuntimeProto` value types, so the actual
/// round trip (worldgen proto → runtime `StateId`/server-biome values → the
/// `SkyLightProvider` → runtime proto → worldgen proto, persisting the computed
/// sky nibbles/emptiness/state/status back into the center) is the caller's —
/// alongside the [`SpawnSeam`] used by the SPAWN task.
/// `None` when the caller did not attach it: dispatching `Light` then falls back
/// to the engine-position path ([`Self::run_light_task`]) for contexts that
/// attach only a [`LevelLightEngine`] (e.g. the faithful-dispatch tests), or
/// refuses `UnsupportedTask`/`UnsupportedStatus` exactly as before the seam
/// existed.
type LightSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError>;

/// A complete generated-light task owned by the generation context.
///
/// This is the typed handoff for a server-side finite light workspace: the
/// implementation owns its tick-thread provider/storage and exposes only the
/// two Paper status-task operations. `has_usable_engine` is deliberately
/// separate from the LIGHT callback so the executor still refuses
/// `INITIALIZE_LIGHT` before mutating the proto when the provider/storage
/// dependency is absent.
pub trait GeneratedLightTask<T, B, S, R = ()>: 'static
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
    R: Send + 'static,
{
    /// Whether the task carries a usable engine/provider dependency.
    fn has_usable_engine(&self) -> bool;

    /// Validate coverage needed by the center-only LIGHT task without mutating
    /// the generated chunk. Implementations use this hook for deterministic
    /// refusals such as a missing required neighbour; failures discovered only
    /// while computing remain transactional failures from [`Self::light`].
    fn validate_light(&self, chunk: &ProtoChunk<T, B, S>) -> Result<(), GenError>;

    /// Paper's `ChunkStatusTasks.initializeLight`: associate the engine and
    /// initialize light sources, without computing or advancing LIGHT.
    fn initialize_light(&mut self, chunk: &mut ProtoChunk<T, B, S>) -> Result<(), GenError>;

    /// Paper's dedicated LIGHT task, including load-vs-compute semantics.
    fn light(&mut self, chunk: &mut ProtoChunk<T, B, S>) -> Result<(), GenError>;

    /// Detach every runtime value still owned by this task. The executor uses
    /// this consuming handoff when a generation attempt succeeds, fails typed,
    /// is replaced, or is torn down after a panic; the caller therefore never
    /// loses the finite LIGHT workspace merely because the task wrapper is
    /// dropped.
    fn take_owned_runtime_storage(&mut self) -> Option<R>;
}

/// `WorldGenContext` (value-layer seam shape) — the caller-supplied BIOMES,
/// NOISE, SURFACE, CARVERS, and FEATURES worldgen closures, plus the light
/// engine the INITIALIZE_LIGHT/LIGHT tasks route to, generic over the chunk's
/// block/biome value types.
pub struct WorldGenContext<T, B, S, R = ()>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
    R: Send + 'static,
{
    /// The `ChunkStatusTasks::generateBiomes` seam — fills the chunk's biomes.
    /// The real body is `ChunkGenerator.createBiomes` (deferred #185); the
    /// caller supplies the closure through this seam.
    biomes: Box<BiomesSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateNoise` seam — fills the chunk's blocks.
    /// The real body is `ChunkGenerator.fillFromNoise` (deferred #185); the
    /// seam's ordering guards are what keep this from running before BIOMES.
    noise: Box<NoiseSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateSurface` seam — runs the real
    /// `NoiseBasedChunkGenerator.buildSurface` over the chunk. The body is
    /// wired; the seam's ordering guard is what keeps this from running before
    /// NOISE (the surface rules consume the NOISE-produced block data).
    surface: Box<SurfaceSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateCarvers` seam — runs the real
    /// `NoiseBasedChunkGenerator.applyCarvers` over the chunk. The body is
    /// wired; the seam's ordering guard is what keeps this from running before
    /// SURFACE (the carvers consume the SURFACE-produced top material).
    carvers: Box<CarversSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateFeatures` seam — runs the real
    /// `NoiseBasedChunkGenerator.applyBiomeDecoration` over the chunk (the
    /// caller, `rivet-server`'s generated world, owns the bounded
    /// `WorldGenRegion` the decoration bodies read/write through, so the body
    /// is caller-supplied). The seam's ordering guard is what keeps this from
    /// running before CARVERS (the decoration bodies consume the CARVERS-
    /// produced block data). The closure returns the body's typed failure —
    /// for seed-42 [`GenError::FeaturePlacementDecode`] at the first real
    /// placed-feature value decode (#126), with
    /// [`GenError::SettingsNotGenerated`] remaining as the defensive path for a
    /// biome the tables do not carry — instead of panicking.
    features: Box<FeaturesSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateSpawn` seam — runs Java's
    /// `generator.spawnOriginalMobs` over the chunk (see [`SpawnSeam`]). `None`
    /// when the caller did not attach it: dispatching `GenerateSpawn` then
    /// returns the typed `UnsupportedTask`/`UnsupportedStatus` refusal, exactly
    /// as before the seam existed.
    spawn: Option<Box<SpawnSeam<T, B, S>>>,
    /// The `ThreadedLevelLightEngine` the INITIALIZE_LIGHT/LIGHT tasks store and
    /// route through (Java's `context.lightEngine()`). The facade holds the
    /// [`StarLightProvider`] (`rivet-server`'s Starlight impl; today
    /// `SkyLightProvider`) that actually lights the chunk — see the module
    /// doc. `None` when the chunk does not carry the light-engine dependency
    /// (the seam's worldgen runs below INITIALIZE_LIGHT, and the idempotent
    /// loaded-chunk confirm never dispatches a light task).
    light_engine: Option<LevelLightEngine>,
    /// The `LIGHT` write-back seam — the caller-owned copy of the computed
    /// light into the chunk (see [`LightSeam`]). `None` when the caller did not
    /// attach it: dispatching `Light` then routes through the engine-position
    /// path ([`Self::run_light_task`]) or refuses, exactly as before the seam
    /// existed.
    light: Option<Box<LightSeam<T, B, S>>>,
    /// A complete generated-light implementation that owns its finite
    /// tick-thread workspace/provider. This is the server handoff for a
    /// generated light workspace; it keeps engine capability and LIGHT
    /// execution in one value-owned owner.
    generated_light: Option<Box<dyn GeneratedLightTask<T, B, S, R>>>,
}

/// The executor seam's failure modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenError {
    /// The `NOISE` task dispatched before the `BIOMES` task ran — the
    /// generation-ordering violation.
    BiomesNotGenerated,
    /// A pass-through (loading) step would advance the chunk to `status`
    /// (`BIOMES`/`NOISE`) without that data being present — the loading pyramid
    /// cannot fabricate biomes or blocks.
    DataNotCarried { status: ChunkStatus },
    /// A light task dispatched without a light engine — the chunk's
    /// INITIALIZE_LIGHT/LIGHT step cannot route through a provider (mirrors
    /// Java's `NPE` on `context.lightEngine()`), and the seam never installs
    /// one on a run that ends below INITIALIZE_LIGHT.
    LightEngineMissing { status: ChunkStatus },
    /// The generated-light workspace could not resolve a required runtime
    /// chunk, including a center/coverage mismatch.
    LightChunkMissing { status: ChunkStatus, pos: ChunkPos },
    /// A provider-owned runtime storage callback panicked before the light
    /// result could be committed.
    LightProviderPanicked { status: ChunkStatus },
    /// A persisted light-correct chunk reached the load branch, but the
    /// attached provider cannot complete force-load reconciliation and edge
    /// correction. The chunk remains unchanged and must not be labeled ready.
    PersistedLightLoadUnsupported { status: ChunkStatus },
    /// The attached generated-light bridge refused after its typed provider,
    /// value-map, or conversion boundary. The bridge is transactional, so the
    /// proto remains unchanged and the caller may inspect or retry it.
    LightTaskFailed { status: ChunkStatus },
    /// The target status is beyond what this borrowed executor can complete —
    /// SPAWN needs the caller's spawn seam; FULL is a consuming representation
    /// change owned by the server holder and is completed outside this loop.
    UnsupportedStatus(ChunkStatus),
    /// A step carries a task body that is not wired in the value layer — the
    /// `GenerateSpawn` step dispatched when the caller attached no spawn seam,
    /// the consuming `Full` representation change dispatched through this
    /// borrowed executor, or a malformed custom pyramid installed a task at a
    /// rung whose body is otherwise absent.
    UnsupportedTask {
        status: ChunkStatus,
        task: ChunkStatusTask,
    },
    /// The `SURFACE` task dispatched before the `NOISE` task ran — the
    /// generation-ordering violation (the surface rules consume the
    /// NOISE-produced block data, so an un-noised chunk must not be surfaced).
    NoiseNotGenerated,
    /// The `CARVERS` task dispatched before the `SURFACE` task ran — the
    /// generation-ordering violation (the carvers consume the SURFACE-produced
    /// top material via the top-material binder, so an un-surfaced chunk must
    /// not be carved).
    CarversNotGenerated,
    /// The `FEATURES` task dispatched before the `CARVERS` task ran — the
    /// generation-ordering violation (the decoration bodies consume the
    /// CARVERS-produced block data, so an un-carved chunk must not be
    /// decorated).
    FeaturesNotGenerated,
    /// The `SPAWN` task dispatched before the `LIGHT` task ran — the
    /// generation-ordering violation (Java's `generateSpawn` →
    /// `generator.spawnOriginalMobs` → `spawnMobsForChunkGeneration` reads the
    /// LIGHT-produced light data through `checkSpawnRules`/
    /// `isSpawnPositionOk`, so an unlit chunk must not be spawned).
    SpawnNotGenerated,
    /// The SPAWN seam reached a non-empty CREATURE spawner list (with the
    /// `SPAWN_MOBS` rule enabled) but entity construction is not ported, so it
    /// refuses typed before pretending entities were produced (RivetTodo #185:
    /// the entity/level units). This is the faithful non-empty path: Java would
    /// draw a random spawner and instantiate mobs; Rivet stops at the exact
    /// point instead of fabricating entities.
    CreatureSpawnNotGenerated {
        /// The generating chunk's position.
        chunk_pos: ChunkPos,
        /// The resolving center biome's name (the `MOB_SPAWN_SETTINGS_BY_NAME`
        /// key), or `None` when the biome's dense id is not in `BIOME_BY_ID`.
        biome: Option<&'static str>,
    },
    /// The `FEATURES` decoration body could not place a configured feature —
    /// its placed-feature value decode (the `#126`-gated `PlacedFeature` JSON
    /// path `placeWithBiomeCheck` dereferences) is unavailable. The
    /// caller-supplied body runs the faithful `addVanillaDecorations` prologue
    /// (heightmap priming, the section-origin decoration-seed derivation, the
    /// bounded 3x3 biome-union gather, the per-step FeatureSorter data, the
    /// exact per-feature seeds) and then fails here, typed, at the exact first
    /// feature whose decode is missing — never panicking and never silently
    /// skipping; the chunk is never stamped FEATURES and no placed feature
    /// body runs.
    FeaturePlacementDecode {
        /// The generating chunk's position.
        chunk_pos: ChunkPos,
        /// `stepIndex` — the `GenerationStep.Decoration` ordinal being
        /// decorated.
        step_index: usize,
        /// `globalIndexOfFeature` — the feature's FeatureSorter global index
        /// (the `setFeatureSeed` index).
        global_feature_index: usize,
        /// The placed feature's registry key (`minecraft:placed_feature` name,
        /// e.g. `minecraft:lake_lava_underground`).
        feature_key: &'static str,
    },
    /// The `FEATURES` body could not resolve a possible biome's
    /// `BiomeGenerationSettings` while building the decoration FeatureSorter.
    /// Paper builds the sorter once from the FULL `biomeSource.possibleBiomes()`
    /// list (`ChunkGenerator.java` 97-100) and never fails (every possible
    /// biome has real settings). Rivet's generated tables now cover all 55
    /// possible biomes, so this is a defensive path: a biome the tables do not
    /// carry (a hand-edited generated file, or a registry that drifted from the
    /// fixture) fails typed here in source order instead of panicking through
    /// the phf index or fabricating/skipping the biome. No decoration runs; the
    /// chunk stays CARVERS.
    SettingsNotGenerated {
        /// The possible biome whose generation settings are missing (`None`
        /// when the biome's dense id is not in `BIOME_BY_ID` at all).
        biome: Option<&'static str>,
    },
    /// `ChunkGenerator.addVanillaDecorations` cannot establish the exact
    /// structure-consumed feature index because the structure manager and its
    /// decoration state are not ported. Feature seeds and placement must stop
    /// before this boundary rather than assuming zero structure consumption.
    StructureDecorationIndexUnavailable {
        /// The generating chunk whose decoration order is unavailable.
        chunk_pos: ChunkPos,
    },
    /// A wired generation task is installed at a rung it does not own —
    /// `GenerateBiomes` away from `BIOMES`, `GenerateNoise` away from `NOISE`.
    /// Only a malformed crate-internal pyramid can produce this: dispatching it
    /// would label the chunk with the target rung's status while running the
    /// wrong (or, for `GenerateBiomes` at `NOISE`, no) data-producing task.
    TaskStatusMismatch {
        status: ChunkStatus,
        task: ChunkStatusTask,
    },
    /// The target is before the chunk's current persisted status (generation
    /// never regresses a column).
    Demotion {
        /// The requested target status.
        target: ChunkStatus,
        /// The chunk's current persisted status.
        current: ChunkStatus,
    },
}

impl std::fmt::Display for GenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenError::BiomesNotGenerated => {
                write!(f, "cannot generate NOISE before the BIOMES task ran")
            }
            GenError::NoiseNotGenerated => {
                write!(f, "cannot generate SURFACE before the NOISE task ran")
            }
            GenError::CarversNotGenerated => {
                write!(f, "cannot generate CARVERS before the SURFACE task ran")
            }
            GenError::FeaturesNotGenerated => {
                write!(f, "cannot generate FEATURES before the CARVERS task ran")
            }
            GenError::SpawnNotGenerated => {
                write!(f, "cannot generate SPAWN before the LIGHT task ran")
            }
            GenError::CreatureSpawnNotGenerated { chunk_pos, biome } => write!(
                f,
                "cannot spawn CREATURE mobs for chunk {chunk_pos} (biome {biome:?}): \
                 entity construction is not ported (RivetTodo #185)"
            ),
            GenError::FeaturePlacementDecode {
                chunk_pos,
                step_index,
                global_feature_index,
                feature_key,
            } => write!(
                f,
                "cannot place feature {} (chunk {}, step {}, global feature index {}): \
                 its placed-feature value decode is unavailable (RivetTodo #126)",
                feature_key, chunk_pos, step_index, global_feature_index
            ),
            GenError::SettingsNotGenerated { biome } => match biome {
                Some(biome) => write!(
                    f,
                    "cannot build the decoration FeatureSorter: the possible biome {biome} \
                     has no generated generation settings (RivetTodo #126)"
                ),
                None => write!(
                    f,
                    "cannot build the decoration FeatureSorter: a possible biome's dense id \
                     is not in the generated biome table (RivetTodo #126)"
                ),
            },
            GenError::StructureDecorationIndexUnavailable { chunk_pos } => write!(
                f,
                "cannot decorate chunk {chunk_pos}: structure-consumed feature index is unavailable \
                 without the structure manager"
            ),
            GenError::DataNotCarried { status } => write!(
                f,
                "cannot label the chunk {status:?}: a pass-through step cannot fabricate that data"
            ),
            GenError::LightEngineMissing { status } => write!(
                f,
                "cannot run the {status:?} task: no light engine is attached to the context"
            ),
            GenError::LightChunkMissing { status, pos } => write!(
                f,
                "cannot run the {status:?} task: the required runtime chunk at {pos} is missing"
            ),
            GenError::LightProviderPanicked { status } => write!(
                f,
                "cannot run the {status:?} task: the light provider storage callback panicked"
            ),
            GenError::PersistedLightLoadUnsupported { status } => write!(
                f,
                "cannot load the persisted {status:?} chunk: the light provider cannot complete edge reconciliation"
            ),
            GenError::LightTaskFailed { status } => write!(
                f,
                "cannot run the {status:?} task: the generated-light bridge refused transactionally"
            ),
            GenError::UnsupportedStatus(status) => write!(
                f,
                "status {status:?} is not wired in the borrowed value executor (FULL uses consuming promotion)"
            ),
            GenError::UnsupportedTask { status, task } => write!(
                f,
                "task {task:?} at status {status:?} is not wired in the value layer (RivetTodo #185)"
            ),
            GenError::TaskStatusMismatch { status, task } => {
                write!(f, "task {task:?} is not the task of the {status:?} rung")
            }
            GenError::Demotion { target, current } => write!(
                f,
                "cannot generate to {target:?}: the chunk is already at {current:?}"
            ),
        }
    }
}

impl std::error::Error for GenError {}

fn ensure_canonical_rung(task: ChunkStatusTask, target: ChunkStatus) -> Result<(), GenError> {
    let canonical = match task {
        ChunkStatusTask::GenerateBiomes => ChunkStatus::Biomes,
        ChunkStatusTask::GenerateNoise => ChunkStatus::Noise,
        ChunkStatusTask::GenerateSurface => ChunkStatus::Surface,
        ChunkStatusTask::GenerateCarvers => ChunkStatus::Carvers,
        ChunkStatusTask::GenerateSpawn => ChunkStatus::Spawn,
        ChunkStatusTask::Full => ChunkStatus::Full,
        _ => return Ok(()),
    };
    if target != canonical {
        return Err(GenError::TaskStatusMismatch {
            status: target,
            task,
        });
    }
    Ok(())
}

impl<T, B, S, R> WorldGenContext<T, B, S, R>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + 'static,
    R: Send + 'static,
{
    /// Wraps the five worldgen seam closures (owned, mirroring the record).
    /// The light engine is left unattached — the slice wires it with
    /// [`Self::with_light_engine`] when a run targets the light statuses.
    pub fn new(
        biomes: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        noise: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        surface: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        carvers: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        features: impl FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError> + 'static,
    ) -> Self {
        WorldGenContext {
            biomes: Box::new(biomes),
            noise: Box::new(noise),
            surface: Box::new(surface),
            carvers: Box::new(carvers),
            features: Box::new(features),
            spawn: None,
            light_engine: None,
            light: None,
            generated_light: None,
        }
    }

    /// Attaches the light engine (Java's `context.lightEngine()`, the
    /// `ThreadedLevelLightEngine` record member) so the INITIALIZE_LIGHT/LIGHT
    /// tasks can route through it.
    pub fn with_light_engine(mut self, light_engine: LevelLightEngine) -> Self {
        self.light_engine = Some(light_engine);
        self
    }

    /// Attaches the `LIGHT` write-back seam — the caller-owned copy of the
    /// computed light into the chunk (see [`LightSeam`]; `rivet-server`'s
    /// `GeneratedLightBridge::light` runs the real provider and persists the
    /// sky nibbles/emptiness/state/status back into the generated center). Left
    /// `None` by default: dispatching `Light` then routes through the
    /// engine-position path ([`Self::run_light_task`]) for contexts that attach
    /// only a [`LevelLightEngine`].
    pub fn with_light(
        mut self,
        light: impl FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError> + 'static,
    ) -> Self {
        self.light = Some(Box::new(light));
        self
    }

    /// Attaches a complete generated-light task in place. Unlike
    /// [`Self::with_light`], this task carries the engine/provider capability
    /// and the transactional LIGHT implementation in one tick-thread-owned
    /// value. Replacing an existing task first detaches its owned runtime
    /// storage and returns it to the caller.
    pub fn attach_generated_light_task(
        &mut self,
        task: impl GeneratedLightTask<T, B, S, R>,
    ) -> Option<R> {
        // Extract while the old task is still in the context. If a custom task
        // panics during extraction, the context retains it for a later recovery
        // attempt instead of dropping its runtime owner during unwinding.
        let detached = self
            .generated_light
            .as_mut()
            .map(|task| task.take_owned_runtime_storage());
        self.generated_light = Some(Box::new(task));
        detached.flatten()
    }

    /// Detach the generated-light task's owned runtime storage without
    /// consuming the generation context. This is the recovery path after a
    /// successful run, a typed generation error, or a caught task panic.
    pub fn take_generated_light_storage(&mut self) -> Option<R> {
        let detached = self
            .generated_light
            .as_mut()
            .map(|task| task.take_owned_runtime_storage());
        // Only remove the wrapper after its extraction completed. A panic in a
        // custom extraction implementation therefore leaves the task—and its
        // still-owned storage—recoverable in this context.
        if detached.is_some() {
            self.generated_light.take();
        }
        detached.flatten()
    }

    /// Consume the context while explicitly extracting any runtime storage
    /// still owned by its generated-light task. Callers use this for teardown
    /// instead of relying on `Drop`, which cannot return values.
    pub fn into_generated_light_storage(mut self) -> Option<R> {
        self.take_generated_light_storage()
    }

    /// Attaches a complete generated-light task. Unlike [`Self::with_light`],
    /// this task carries the engine/provider capability and the transactional
    /// LIGHT implementation in one tick-thread-owned value. The generation
    /// preflight still requires [`GeneratedLightTask::has_usable_engine`]
    /// before it will run `INITIALIZE_LIGHT`.
    pub fn with_generated_light_task(mut self, task: impl GeneratedLightTask<T, B, S, R>) -> Self {
        let _ = self.attach_generated_light_task(task);
        self
    }

    /// Attaches the `generateSpawn` seam (Java's `context.generator().
    /// spawnOriginalMobs`, the caller-owned `spawnOriginalMobs` body) so the
    /// SPAWN task can run it. Left `None` by default (the `rivet-server` holder
    /// wires neither seam): dispatching `GenerateSpawn` then returns the typed
    /// [`GenError::UnsupportedTask`]/[`GenError::UnsupportedStatus`] refusal,
    /// exactly as before the seam existed.
    pub fn with_spawn(
        mut self,
        spawn: impl FnMut(&mut ProtoChunk<T, B, S>) -> Result<(), GenError> + 'static,
    ) -> Self {
        self.spawn = Some(Box::new(spawn));
        self
    }

    /// `ChunkStatusTasks.isLighted(ChunkAccess)` — `getPersistedStatus()
    /// .isOrAfter(ChunkStatus.LIGHT) && isLightCorrect()`. The flag is
    /// `ChunkAccess.isLightCorrect`.
    fn is_lighted(chunk: &ProtoChunk<T, B, S>) -> bool {
        chunk.get_persisted_status().is_or_after(ChunkStatus::Light) && chunk.is_light_correct()
    }

    /// Whether the context carries a light engine the light steps can use —
    /// present AND carrying a provider. `LevelLightEngine::new` builds an
    /// engine with the provider `None` (the seam attaches one through
    /// `with_provider`); a provider-less engine can neither light nor load, so
    /// the light steps treat it as absent. The atomic-refusal precheck and the
    /// single-step guards both require it, so a provider-less engine never lets
    /// earlier steps mutate a chunk before the light failure.
    fn has_usable_light_engine(&self) -> bool {
        if let Some(task) = self.generated_light.as_ref() {
            return task.has_usable_engine();
        }
        self.light_engine
            .as_ref()
            .is_some_and(|e| e.provider().is_some())
    }

    /// `ChunkStatusTasks.light` + `ChunkLightTask.LightTask.getAsBoolean` (the
    /// Moonrise per-status light dispatch; spec §3.4) — the LIGHT task body,
    /// dispatched through the provider seam. Java's branch: an already-lighted
    /// chunk is *loaded* (`forceLoadInChunk` + `checkChunkEdges`), otherwise it
    /// is recomputed (`setLightCorrect(false)` → `lightChunk` →
    /// `setLightCorrect(true)`), then a ProtoChunk at `LIGHT.getParent()` is
    /// advanced to `LIGHT`. The provider is `rivet-server`'s synchronous
    /// `SkyLightProvider`: fresh recomputation publishes the available nibble
    /// result, while a provider that cannot complete persisted edge
    /// reconciliation returns a typed refusal after the force-load/check-edge
    /// sequence. The INITIALIZE_LIGHT task
    /// never reaches here — it records the engine and computes nothing (see the
    /// [`Self::run_step`] `InitializeLight` arm).
    fn run_light_task(
        &mut self,
        chunk: &mut ProtoChunk<T, B, S>,
        status: ChunkStatus,
    ) -> Result<(), GenError> {
        let engine = self
            .light_engine
            .as_mut()
            .ok_or(GenError::LightEngineMissing { status })?;
        let empty_sections =
            crate::lighting::star_light_engine::get_empty_sections_for_chunk(chunk);
        // The provider is required before any mutation — a provider-less engine
        // (built with `LevelLightEngine::new`) cannot light or load, so the
        // recompute branch must not toggle light-correct before refusing.
        let provider = engine
            .provider_mut()
            .ok_or(GenError::LightEngineMissing { status })?;
        if Self::is_lighted(chunk) {
            provider
                .try_force_load_in_chunk(chunk.get_pos(), &empty_sections)
                .map_err(|error| match error {
                    LightProviderError::MissingChunk(pos) => {
                        GenError::LightChunkMissing { status, pos }
                    }
                    LightProviderError::CallbackPanicked => {
                        GenError::LightProviderPanicked { status }
                    }
                })?;
            provider.check_chunk_edges(chunk.get_pos());
            if !provider.supports_persisted_light_load() {
                return Err(GenError::PersistedLightLoadUnsupported {
                    status: chunk.get_persisted_status(),
                });
            }
        } else {
            let was_light_correct = chunk.is_light_correct();
            chunk.set_light_correct(false);
            if let Err(error) = provider.try_light_chunk(chunk.get_pos(), &empty_sections) {
                chunk.set_light_correct(was_light_correct);
                return Err(match error {
                    LightProviderError::MissingChunk(pos) => {
                        GenError::LightChunkMissing { status, pos }
                    }
                    LightProviderError::CallbackPanicked => {
                        GenError::LightProviderPanicked { status }
                    }
                });
            }
            chunk.set_light_correct(true);
        }
        // `ChunkLightTask`: advance the ProtoChunk at LIGHT's parent to LIGHT.
        if chunk.get_persisted_status() == ChunkStatus::Light.parent() {
            chunk.set_persisted_status(ChunkStatus::Light);
        }
        Ok(())
    }

    /// Run one step's task on the chunk and advance its persisted status
    /// (mirroring `ChunkStep.apply` + `completeChunkGeneration`, synchronously).
    ///
    /// The `EMPTY`/`STRUCTURE_STARTS`/`STRUCTURE_REFERENCES` bodies are the
    /// value-layer pass-through (RivetTodo #185: the real structure bodies call
    /// `generator.createStructures`/`createReferences` + `level
    /// .onStructureStartsAvailable`, deferred with the generator wave). A
    /// pass-through produces no data, so it is refused when it would advance a
    /// chunk to `BIOMES`/`NOISE` that does not already carry that status — the
    /// LOADING pyramid's loading stubs cannot fabricate a NOISE-labeled chunk.
    /// The `NOISE`/`SURFACE`/`CARVERS` bodies are gated on the
    /// persisted-status record, and a wired generation task is honored only at
    /// its canonical rung (`GenerateBiomes` at `BIOMES`, `GenerateNoise` at
    /// `NOISE`, `GenerateSurface` at `SURFACE`, `GenerateCarvers` at
    /// `CARVERS`). The `INITIALIZE_LIGHT` body mirrors
    /// `ChunkStatusTasks.initializeLight`
    /// (record the engine; Paper's `initializeLight` completes immediately, so
    /// the value layer computes nothing) and the `LIGHT` body mirrors
    /// `ChunkStatusTasks.light` + the Moonrise `ChunkLightTask` branch (see
    /// [`Self::run_light_task`]); both require the light engine.
    pub fn run_step(
        &mut self,
        step: &ChunkStep,
        chunk: &mut ProtoChunk<T, B, S>,
    ) -> Result<(), GenError> {
        match step.task() {
            // EMPTY, and the STRUCTURE_STARTS/STRUCTURE_REFERENCES pass-through.
            // RivetTodo(#185): the real structure bodies call
            // `generator.createStructures`/`createReferences` and
            // `level.onStructureStartsAvailable` (the `mc.world.level.chunk
            // .generator` wave); they are pass-throughs in the value layer.
            ChunkStatusTask::PassThrough
            | ChunkStatusTask::GenerateStructureStarts
            | ChunkStatusTask::LoadStructureStarts
            | ChunkStatusTask::GenerateStructureReferences => {
                if step.target_status().is_or_after(ChunkStatus::Biomes)
                    && !chunk
                        .get_persisted_status()
                        .is_or_after(step.target_status())
                {
                    return Err(GenError::DataNotCarried {
                        status: step.target_status(),
                    });
                }
                chunk_status_tasks::pass_through(chunk);
            }
            ChunkStatusTask::GenerateBiomes => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                (self.biomes)(chunk);
            }
            ChunkStatusTask::GenerateNoise => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                if !chunk
                    .get_persisted_status()
                    .is_or_after(ChunkStatus::Biomes)
                {
                    return Err(GenError::BiomesNotGenerated);
                }
                (self.noise)(chunk);
            }
            ChunkStatusTask::GenerateSurface => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // `ChunkStatusTasks.generateSurface` → Java's
                // `NoiseBasedChunkGenerator.buildSurface`: the surface rules
                // consume the NOISE-produced block data, so an un-noised chunk
                // is refused before any write (the surface body never runs and
                // the chunk is never labeled SURFACE).
                if !chunk.get_persisted_status().is_or_after(ChunkStatus::Noise) {
                    return Err(GenError::NoiseNotGenerated);
                }
                (self.surface)(chunk);
            }
            ChunkStatusTask::GenerateCarvers => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // `ChunkStatusTasks.generateCarvers` → Java's
                // `NoiseBasedChunkGenerator.applyCarvers`: the carvers consume
                // the SURFACE-produced top material (through the top-material
                // binder), so an un-surfaced chunk is refused before any write
                // (the carvers body never runs and the chunk is never labeled
                // CARVERS).
                if !chunk
                    .get_persisted_status()
                    .is_or_after(ChunkStatus::Surface)
                {
                    return Err(GenError::CarversNotGenerated);
                }
                (self.carvers)(chunk);
            }
            ChunkStatusTask::InitializeLight => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // `ChunkStatusTasks.initializeLight`:
                // `chunk.initializeLightSources(); chunk.setLightEngine(engine);
                // return engine.initializeLight(chunk, isLighted(chunk))`.
                // `initializeLightSources`/`getSkyLightSources` are empty in the
                // rewrite, and `ThreadedLevelLightEngine.initializeLight`
                // completes immediately in this Paper build (the real light
                // compute is the Moonrise `ChunkLightTask`, dispatched at the
                // LIGHT rung — see [`Self::run_light_task`]). The value layer
                // records the engine (Java's `setLightEngine`; the field itself
                // defers with the lighting unit, #184) and dereferences it — a
                // missing engine (or provider-less one) errors like Java's NPE
                // — but computes nothing: no light-correct toggle, no provider
                // call. A chunk stays at `INITIALIZE_LIGHT` until the LIGHT
                // task lights or loads it.
                if let Some(task) = self.generated_light.as_mut() {
                    if !task.has_usable_engine() {
                        return Err(GenError::LightEngineMissing {
                            status: ChunkStatus::InitializeLight,
                        });
                    }
                    task.initialize_light(chunk)?;
                } else {
                    if !self.has_usable_light_engine() {
                        return Err(GenError::LightEngineMissing {
                            status: ChunkStatus::InitializeLight,
                        });
                    }
                    // `initializeLightSources` is a Paper no-op in the
                    // Starlight rewrite; the association itself is observable
                    // on ProtoChunk and is recorded before the task completes.
                    chunk.set_light_engine();
                }
            }
            ChunkStatusTask::Light => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // A complete generated-light task owns the finite provider
                // workspace and is authoritative for the LIGHT write-back.
                // The older split seam remains for the G1 value-layer tests.
                if let Some(task) = self.generated_light.as_mut() {
                    if !task.has_usable_engine() {
                        return Err(GenError::LightEngineMissing {
                            status: ChunkStatus::Light,
                        });
                    }
                    task.light(chunk)?;
                } else if let Some(light) = self.light.as_mut() {
                    light(chunk)?;
                } else {
                    self.run_light_task(chunk, ChunkStatus::Light)?;
                }
            }
            ChunkStatusTask::GenerateFeatures => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // `ChunkStatusTasks.generateFeatures` → Java's
                // `NoiseBasedChunkGenerator.applyBiomeDecoration`: the
                // decoration bodies consume the CARVERS-produced block data
                // (through the bounded WorldGenRegion reads/writes), so an
                // un-carved chunk is refused before any write (the features
                // body never runs and the chunk is never labeled FEATURES).
                if !chunk
                    .get_persisted_status()
                    .is_or_after(ChunkStatus::Carvers)
                {
                    return Err(GenError::FeaturesNotGenerated);
                }
                (self.features)(chunk)?;
            }
            ChunkStatusTask::GenerateSpawn => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // `ChunkStatusTasks.generateSpawn` → Java's
                // `generator.spawnOriginalMobs` (the caller's
                // `spawnMobsForChunkGeneration` natural-spawner body, attached
                // with [`Self::with_spawn`]). The spawner reads the
                // LIGHT-produced light data through `checkSpawnRules`/
                // `isSpawnPositionOk`, so an unlit chunk is refused before any
                // write. With no spawn seam attached the step refuses exactly as
                // before the seam existed.
                // `ChunkStatusTasks.generateSpawn` skips the generator body for
                // a chunk carrying BelowZeroRetrogen. The status still advances:
                // retrogen owns the later upgrade path, so spawning here would
                // duplicate entities against the Java pipeline.
                if chunk.is_upgrading() {
                    if !chunk.get_persisted_status().is_or_after(ChunkStatus::Light) {
                        return Err(GenError::SpawnNotGenerated);
                    }
                } else {
                    let Some(spawn) = self.spawn.as_mut() else {
                        return Err(GenError::UnsupportedTask {
                            status: step.target_status(),
                            task: step.task(),
                        });
                    };
                    if !chunk.get_persisted_status().is_or_after(ChunkStatus::Light) {
                        return Err(GenError::SpawnNotGenerated);
                    }
                    spawn(chunk)?;
                }
            }
            ChunkStatusTask::Full => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                // Paper's FULL body consumes the parent SPAWN ProtoChunk and
                // returns a LevelChunk. A borrowed generic executor cannot
                // perform that representation change, so it refuses without
                // stamping or mutating the proto. The server holder performs
                // the consuming promotion transaction after SPAWN (G4).
                return Err(GenError::UnsupportedTask {
                    status: step.target_status(),
                    task: step.task(),
                });
            }
        }
        if chunk.get_persisted_status().is_before(step.target_status()) {
            chunk.set_persisted_status(step.target_status());
        }
        Ok(())
    }

    /// Generate a chunk from its current persisted status through `target`
    /// (inclusive), running each step's task in status order.
    ///
    /// The `BIOMES`-before-`NOISE`-before-`SURFACE`-before-`CARVERS`-
    /// before-`FEATURES`-before-`LIGHT`-before-`SPAWN`-before-`FULL` ordering is
    /// enforced at the *status-order* layer, not
    /// just inside the task bodies: a promotion whose
    /// target passes through the `BIOMES`/`NOISE` steps requires that data to
    /// be present — either carried in (the chunk's persisted status was already
    /// at/after the step) or produced by this run's earlier step. A pyramid
    /// whose `BIOMES` or `NOISE` step is a pass-through (the `LOADING_PYRAMID`'s
    /// loading stubs) cannot advance an `EMPTY` chunk past it, because that
    /// would label the chunk with data that was never generated. The whole
    /// path is validated before any work runs — a refused promotion leaves the
    /// chunk untouched. A wired generation task is honored only at its
    /// canonical rung (a malformed pyramid installing `GenerateBiomes` at
    /// `NOISE`, for example, is refused here, so the chunk is never labeled
    /// `NOISE` by a task that produces no noise data). The SPAWN step runs the
    /// caller's spawn seam (`with_spawn`). FULL is deliberately not part of
    /// this borrowed execution loop: its scheduler identity is completed by a
    /// consuming holder promotion after SPAWN. A target before the current
    /// status is a demotion error. The INITIALIZE_LIGHT/LIGHT steps are wired (see
    /// [`Self::run_light_task`]) but require a usable light engine — an
    /// engine-less *or provider-less* context targeting them fails
    /// [`GenError::LightEngineMissing`] before any work runs, and a run whose
    /// path passes through a light step is where the step leaves its
    /// light-correct/status record.
    pub fn generate_through(
        &mut self,
        pyramid: &ChunkPyramid,
        chunk: &mut ProtoChunk<T, B, S>,
        target: ChunkStatus,
    ) -> Result<(), GenError> {
        let current = chunk.get_persisted_status();
        // FULL is a consuming representation change, not a borrowed proto
        // status stamp. Refuse every borrowed FULL request, including one for
        // a proto already persisted at FULL; the holder owns the dedicated
        // promotion transaction and must produce a LevelChunk value.
        if target == ChunkStatus::Full {
            return Err(GenError::UnsupportedStatus(target));
        }
        // Idempotent no-op for every other status: a chunk already at the
        // target needs no work — this is what lets the LOADING pyramid confirm
        // persisted chunks at or past LIGHT.
        if target == current {
            return Ok(());
        }
        if target.index() < current.index() {
            return Err(GenError::Demotion { target, current });
        }
        // A target at SPAWN/FULL is reachable only when the caller attached the
        // matching seam; a caller that wires neither (the `rivet-server` holder)
        // keeps the past-LIGHT refusals exactly as before the seams existed. The
        // seam absence is decided here (the final target), while the precheck
        // below refuses a path that merely passes through a seam-less rung.
        if target == ChunkStatus::Spawn
            && chunk.is_upgrading()
            && !current.is_or_after(ChunkStatus::Light)
        {
            // A retrogen skip is still a SPAWN dependency, not an override for
            // malformed status input. Paper reaches generateSpawn only after
            // LIGHT has completed for the center.
            return Err(GenError::SpawnNotGenerated);
        }
        if target == ChunkStatus::Spawn && self.spawn.is_none() && !chunk.is_upgrading() {
            return Err(GenError::UnsupportedStatus(target));
        }
        // Validate the whole path before running any step, tracking the
        // persisted status each step will leave behind (`projected` mirrors the
        // run loop, so a step the loop would refuse fails here instead and a
        // refused promotion leaves the chunk untouched). Every step the loop
        // would run must be either a wired generation task or a pass-through
        // that does not fabricate BIOMES/NOISE data. The light steps also
        // require a usable engine (present AND provider-carrying): without one
        // the path cannot reach LIGHT, and a provider-less engine must not let
        // the earlier steps mutate the chunk before the LIGHT failure.
        let mut projected = current;
        let mut needs_light_engine = false;
        for index in current.index() + 1..=target.index() {
            let step = pyramid.get_step_to(ChunkStatus::ALL[index]);
            match step.task() {
                ChunkStatusTask::GenerateBiomes => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateNoise => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    if !projected.is_or_after(ChunkStatus::Biomes) {
                        return Err(GenError::BiomesNotGenerated);
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateSurface => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    if !projected.is_or_after(ChunkStatus::Noise) {
                        return Err(GenError::NoiseNotGenerated);
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateCarvers => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    if !projected.is_or_after(ChunkStatus::Surface) {
                        return Err(GenError::CarversNotGenerated);
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::InitializeLight => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    needs_light_engine = true;
                    projected = step.target_status();
                }
                ChunkStatusTask::Light => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    // A generated workspace can prove its finite neighbour
                    // coverage before INITIALIZE_LIGHT mutates the chunk. Keep
                    // deterministic coverage refusals atomic across the whole
                    // promotion path; runtime provider failures remain
                    // transactional at the LIGHT task boundary.
                    if let Some(task) = self.generated_light.as_ref() {
                        task.validate_light(chunk)?;
                        // `run_step` gives a complete generated-light task
                        // precedence over the older split `light` seam. Keep
                        // the preflight in lockstep so an unusable generated
                        // task cannot let earlier steps mutate the proto merely
                        // because a legacy seam is also attached.
                        needs_light_engine = true;
                    } else if self.light.is_none() {
                        // With a caller-attached light seam the write-back
                        // bridge is authoritative and needs no engine at this
                        // rung; without it the engine-position path needs one.
                        needs_light_engine = true;
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateFeatures => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    if !projected.is_or_after(ChunkStatus::Carvers) {
                        return Err(GenError::FeaturesNotGenerated);
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateSpawn => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    // Mirrors the run_step arm: with no spawn seam the path is
                    // refused before any work runs (the chunk keeps its status);
                    // with one, the LIGHT-produced light must be present.
                    if self.spawn.is_none() && !chunk.is_upgrading() {
                        return Err(GenError::UnsupportedTask {
                            status: step.target_status(),
                            task: step.task(),
                        });
                    }
                    if !projected.is_or_after(ChunkStatus::Light) {
                        return Err(GenError::SpawnNotGenerated);
                    }
                    projected = step.target_status();
                }
                ChunkStatusTask::Full => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    return Err(GenError::UnsupportedTask {
                        status: step.target_status(),
                        task: step.task(),
                    });
                }
                // Pass-through produces no data: reaching BIOMES/NOISE through
                // it requires the target status's data to already be carried.
                ChunkStatusTask::PassThrough
                | ChunkStatusTask::GenerateStructureStarts
                | ChunkStatusTask::LoadStructureStarts
                | ChunkStatusTask::GenerateStructureReferences => {
                    if step.target_status().is_or_after(ChunkStatus::Biomes)
                        && !projected.is_or_after(step.target_status())
                    {
                        return Err(GenError::DataNotCarried {
                            status: step.target_status(),
                        });
                    }
                    projected = step.target_status();
                }
            }
        }
        if needs_light_engine && !self.has_usable_light_engine() {
            return Err(GenError::LightEngineMissing { status: target });
        }
        for index in current.index() + 1..=target.index() {
            let step = pyramid.get_step_to(ChunkStatus::ALL[index]);
            self.run_step(step, chunk)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container_factory::PalettedContainerFactory;
    use crate::chunk::status::{GENERATION_PYRAMID, LOADING_PYRAMID};
    use crate::chunk::strategy::Strategy;
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::StateFlags;
    use crate::lighting::star_light_provider::StarLightProvider;
    use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy)]
    struct TestGlobalMap;
    impl GlobalIdMap<u8> for TestGlobalMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8> + Send + Sync> {
            Box::new(*self)
        }
    }

    fn block_strategy() -> Strategy<u8> {
        Strategy::create_for_block_states(Box::new(TestGlobalMap))
    }
    fn biome_strategy() -> Strategy<u8> {
        Strategy::create_for_biomes(Box::new(TestGlobalMap))
    }
    fn factory() -> PalettedContainerFactory<u8, u8> {
        PalettedContainerFactory::new(block_strategy(), 0, biome_strategy(), 0)
    }

    /// A fresh worldgen chunk at `EMPTY`.
    fn proto() -> ProtoChunk<u8, u8, &'static str> {
        ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory(),
            None,
            0,
            255,
            &|s: &u8| StateFlags {
                is_air: *s == 0,
                blocks_motion: *s != 0,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    /// A `WorldGenContext` whose closures record their invocation into shared
    /// `Rc<RefCell<...>>` logs. The context owns the closures (which own their
    /// `Rc` clones), so the logs stay readable while the context is alive.
    type RecordingContext = (
        WorldGenContext<u8, u8, &'static str>,
        Rc<RefCell<Vec<&'static str>>>,
        Rc<RefCell<Vec<&'static str>>>,
        Rc<RefCell<Vec<&'static str>>>,
        Rc<RefCell<Vec<&'static str>>>,
        Rc<RefCell<Vec<&'static str>>>,
    );

    fn recording_context() -> RecordingContext {
        let biomes_calls = Rc::new(RefCell::new(Vec::new()));
        let noise_calls = Rc::new(RefCell::new(Vec::new()));
        let surface_calls = Rc::new(RefCell::new(Vec::new()));
        let carvers_calls = Rc::new(RefCell::new(Vec::new()));
        let features_calls = Rc::new(RefCell::new(Vec::new()));
        let biomes_log = Rc::clone(&biomes_calls);
        let noise_log = Rc::clone(&noise_calls);
        let surface_log = Rc::clone(&surface_calls);
        let carvers_log = Rc::clone(&carvers_calls);
        let features_log = Rc::clone(&features_calls);
        let ctx = WorldGenContext::new(
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| biomes_log.borrow_mut().push("biomes"),
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| noise_log.borrow_mut().push("noise"),
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| {
                surface_log.borrow_mut().push("surface")
            },
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| {
                carvers_log.borrow_mut().push("carvers")
            },
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| {
                features_log.borrow_mut().push("features");
                Ok(())
            },
        );
        (
            ctx,
            biomes_calls,
            noise_calls,
            surface_calls,
            carvers_calls,
            features_calls,
        )
    }

    #[test]
    fn generate_through_promotes_step_by_step_in_dag_order_through_noise() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();

        // Half-way: STRUCTURE_REFERENCES — the pass-through tasks ran, no
        // worldgen seam invoked yet.
        ctx.generate_through(
            &GENERATION_PYRAMID,
            &mut chunk,
            ChunkStatus::StructureReferences,
        )
        .expect("through structure references");
        assert_eq!(
            chunk.get_persisted_status(),
            ChunkStatus::StructureReferences
        );
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // Resume to NOISE: the loop picks up from STRUCTURE_REFERENCES and
        // runs BIOMES, then NOISE — BIOMES before NOISE by construction.
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect("through noise");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Noise);
        assert_eq!(biomes_calls.borrow().as_slice(), &["biomes"]);
        assert_eq!(noise_calls.borrow().as_slice(), &["noise"]);
    }

    #[test]
    fn generating_only_through_biomes_runs_the_biomes_task_once() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Biomes)
            .expect("through biomes");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Biomes);
        assert_eq!(biomes_calls.borrow().as_slice(), &["biomes"]);
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn promotion_is_idempotent_at_target() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect("first promotion");
        // Already at NOISE — a second promotion is a no-op, no task reruns.
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect("idempotent");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Noise);
        assert_eq!(biomes_calls.borrow().as_slice(), &["biomes"]);
        assert_eq!(noise_calls.borrow().as_slice(), &["noise"]);
    }

    #[test]
    fn a_chunk_recorded_at_biomes_can_be_promoted_to_noise() {
        // The persisted status is the record of what ran: a chunk already at
        // BIOMES (from an earlier generation) may be promoted to NOISE without
        // re-running the BIOMES task.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Biomes);
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect("biomes already recorded");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Noise);
        assert!(biomes_calls.borrow().is_empty());
        assert_eq!(noise_calls.borrow().as_slice(), &["noise"]);
    }

    #[test]
    fn noise_step_alone_fails_without_biomes() {
        // The honest guard: dispatching the NOISE step directly on a chunk that
        // has not run BIOMES errors, and the chunk is not labeled NOISE.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let noise_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Noise);
        let err = ctx
            .run_step(noise_step, &mut chunk)
            .expect_err("noise needs biomes");
        assert_eq!(err, GenError::BiomesNotGenerated);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn surface_step_alone_fails_without_noise() {
        // The honest guard: dispatching the SURFACE step directly on a chunk
        // that has not run NOISE errors, and the chunk is not labeled SURFACE
        // (the surface body never runs).
        let (mut ctx, biomes_calls, noise_calls, surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let surface_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Surface);
        let err = ctx
            .run_step(surface_step, &mut chunk)
            .expect_err("surface needs noise");
        assert_eq!(err, GenError::NoiseNotGenerated);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
        assert!(surface_calls.borrow().is_empty());
    }

    #[test]
    fn generate_through_features_runs_the_wired_features_step() {
        // FEATURES is wired: a fresh EMPTY chunk targeting FEATURES runs BIOMES,
        // NOISE, SURFACE, CARVERS, then FEATURES in status order and is stamped
        // FEATURES.
        let (mut ctx, biomes_calls, noise_calls, surface_calls, carvers_calls, features_calls) =
            recording_context();
        let mut chunk = proto();
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Features)
            .expect("through features");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert_eq!(biomes_calls.borrow().as_slice(), &["biomes"]);
        assert_eq!(noise_calls.borrow().as_slice(), &["noise"]);
        assert_eq!(surface_calls.borrow().as_slice(), &["surface"]);
        assert_eq!(carvers_calls.borrow().as_slice(), &["carvers"]);
        assert_eq!(features_calls.borrow().as_slice(), &["features"]);

        // This context wires no SPAWN seam, and FULL is always a consuming boundary, so
        // a FULL target reports UnsupportedStatus before any later work runs.
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect_err("FULL requires consuming promotion");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Full));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert_eq!(features_calls.borrow().as_slice(), &["features"]);
    }

    #[test]
    fn features_step_alone_fails_without_carvers() {
        // The honest guard: dispatching the FEATURES step directly on a chunk
        // that has not run CARVERS errors, and the chunk is not labeled FEATURES
        // (the features body never runs).
        let (mut ctx, biomes_calls, noise_calls, surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let features_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);
        let err = ctx
            .run_step(features_step, &mut chunk)
            .expect_err("features need carvers");
        assert_eq!(err, GenError::FeaturesNotGenerated);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
        assert!(surface_calls.borrow().is_empty());
    }

    #[test]
    fn carvers_step_alone_fails_without_surface() {
        // The honest guard: dispatching the CARVERS step directly on a chunk
        // that has not run SURFACE errors, and the chunk is not labeled CARVERS
        // (the carvers body never runs).
        let (mut ctx, biomes_calls, noise_calls, surface_calls, carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let carvers_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Carvers);
        let err = ctx
            .run_step(carvers_step, &mut chunk)
            .expect_err("carvers need surface");
        assert_eq!(err, GenError::CarversNotGenerated);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
        assert!(surface_calls.borrow().is_empty());
        assert!(carvers_calls.borrow().is_empty());
    }

    #[test]
    fn loading_pyramid_cannot_promote_a_fresh_chunk_past_empty() {
        // The LOADING pyramid's BIOMES/NOISE steps are pass-through loading
        // stubs — they would advance the persisted status without running any
        // biomes task. The seam refuses that on a fresh (EMPTY) chunk: the
        // chunk must not be labeled BIOMES (let alone NOISE) through them.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let err = ctx
            .generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Biomes)
            .expect_err("loading cannot generate biomes");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Biomes
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        let err = ctx
            .generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect_err("loading cannot generate noise");
        // The pre-check walks in status order and reports the first fabrication
        // point on the path: the BIOMES step is reached before NOISE.
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Biomes
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn loading_pyramid_cannot_promote_a_biomes_chunk_to_noise() {
        // Even a chunk that already carries biomes cannot be advanced to NOISE
        // through the loading pyramid: its NOISE step is a pass-through that
        // produces no noise data, so the promotion would label the chunk NOISE
        // with no blocks. Loading only reflects data the chunk already has.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Biomes);
        let err = ctx
            .generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect_err("loading cannot fabricate noise");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Noise
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Biomes);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn run_step_loading_noise_stub_on_fresh_chunk_is_refused() {
        // The public single-step seam must not label a fresh EMPTY chunk NOISE
        // through the LOADING pyramid's pass-through NOISE stub (which produces
        // no noise data).
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let noise_step = LOADING_PYRAMID.get_step_to(ChunkStatus::Noise);
        let err = ctx.run_step(noise_step, &mut chunk).expect_err("no data");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Noise
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn a_pass_through_noise_step_cannot_fabricate_noise() {
        // Finding-2 guard: a pyramid whose BIOMES step generates biomes but
        // whose NOISE step is a pass-through would label a fresh chunk NOISE
        // with no blocks. The pre-check refuses it before any work runs.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        // Build a minimal 5-rung pyramid: EMPTY -> SS -> SR ->
        // BIOMES(GenerateBiomes) -> NOISE(pass-through). The NOISE step
        // produces no noise data.
        let pyramid = ChunkPyramid::builder()
            .step(ChunkStatus::Empty, |s| s)
            .step(ChunkStatus::StructureStarts, |s| {
                s.set_task(ChunkStatusTask::GenerateStructureStarts)
            })
            .step(ChunkStatus::StructureReferences, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateStructureReferences)
            })
            .step(ChunkStatus::Biomes, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .step(ChunkStatus::Noise, |s| {
                s.add_requirement(ChunkStatus::Biomes, 1)
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::PassThrough)
            })
            .build();
        let err = ctx
            .generate_through(&pyramid, &mut chunk, ChunkStatus::Noise)
            .expect_err("pass-through noise cannot fabricate blocks");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Noise
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn a_pass_through_surface_step_cannot_fabricate_carvers() {
        // Finding-2 guard for the CARVERS rung: a pyramid whose SURFACE step
        // is a pass-through would label a fresh chunk CARVERS with no surface
        // data for the carvers to carve through. The pre-check refuses it
        // before any work runs — as `DataNotCarried { Surface }`, the first
        // fabrication point on the path (the pass-through SURFACE step refuses
        // before the CARVERS step is reached). The CARVERS rung's own
        // `CarversNotGenerated` guard is run_step-level defence-in-depth,
        // covered separately by `carvers_step_alone_fails_without_surface`.
        let (mut ctx, biomes_calls, noise_calls, surface_calls, carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        // EMPTY -> SS -> SR -> BIOMES(GenerateBiomes) -> NOISE(GenerateNoise)
        // -> SURFACE(pass-through) -> CARVERS(GenerateCarvers): the SURFACE
        // step produces no surface data.
        let pyramid = ChunkPyramid::builder()
            .step(ChunkStatus::Empty, |s| s)
            .step(ChunkStatus::StructureStarts, |s| {
                s.set_task(ChunkStatusTask::GenerateStructureStarts)
            })
            .step(ChunkStatus::StructureReferences, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateStructureReferences)
            })
            .step(ChunkStatus::Biomes, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .step(ChunkStatus::Noise, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .add_requirement(ChunkStatus::Biomes, 1)
                    .set_task(ChunkStatusTask::GenerateNoise)
            })
            .step(ChunkStatus::Surface, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .add_requirement(ChunkStatus::Biomes, 1)
                    .set_task(ChunkStatusTask::PassThrough)
            })
            .step(ChunkStatus::Carvers, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .add_requirement(ChunkStatus::Biomes, 1)
                    .set_task(ChunkStatusTask::GenerateCarvers)
            })
            .build();
        let err = ctx
            .generate_through(&pyramid, &mut chunk, ChunkStatus::Carvers)
            .expect_err("pass-through surface cannot fabricate carvers");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Surface
            }
        );
        // The chunk is untouched AND none of the earlier seams ran.
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
        assert!(surface_calls.borrow().is_empty());
        assert!(carvers_calls.borrow().is_empty());
    }

    #[test]
    fn refused_promotion_is_atomic_across_the_whole_path() {
        // Atomicity: when any step in the target path is refused, no earlier
        // step runs either. A chunk already at STRUCTURE_REFERENCES promoted
        // to NOISE through a pyramid whose NOISE step is a pass-through would
        // have BIOMES (an earlier, valid step) in the dispatch loop — the
        // pre-check must refuse before BIOMES runs, leaving the chunk and the
        // biomes seam untouched.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::StructureReferences);
        let pyramid = ChunkPyramid::builder()
            .step(ChunkStatus::Empty, |s| s)
            .step(ChunkStatus::StructureStarts, |s| {
                s.set_task(ChunkStatusTask::GenerateStructureStarts)
            })
            .step(ChunkStatus::StructureReferences, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateStructureReferences)
            })
            .step(ChunkStatus::Biomes, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .step(ChunkStatus::Noise, |s| {
                s.add_requirement(ChunkStatus::Biomes, 1)
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::PassThrough)
            })
            .build();
        let err = ctx
            .generate_through(&pyramid, &mut chunk, ChunkStatus::Noise)
            .expect_err("pass-through noise refused");
        assert_eq!(
            err,
            GenError::DataNotCarried {
                status: ChunkStatus::Noise
            }
        );
        // The chunk is unchanged AND the earlier valid BIOMES step never ran.
        assert_eq!(
            chunk.get_persisted_status(),
            ChunkStatus::StructureReferences
        );
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn loading_noise_step_is_idempotent_on_a_chunk_at_noise() {
        // Loading a chunk that already carries NOISE data: the pass-through
        // NOISE step is allowed (the data is present) and leaves the status at
        // NOISE, running no worldgen seam.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Noise);
        let noise_step = LOADING_PYRAMID.get_step_to(ChunkStatus::Noise);
        ctx.run_step(noise_step, &mut chunk)
            .expect("noise data already present");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Noise);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn demotion_is_rejected() {
        let (mut ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Noise)
            .expect("promote");
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Biomes)
            .expect_err("cannot demote");
        assert_eq!(
            err,
            GenError::Demotion {
                target: ChunkStatus::Biomes,
                current: ChunkStatus::Noise,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Noise);
    }

    #[test]
    fn loading_a_light_chunk_is_an_idempotent_no_op() {
        // A chunk loaded from disk at LIGHT is confirmed as a no-op through the
        // LOADING pyramid: target == current returns Ok without dispatching a
        // borrowed generation task.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Light);
        ctx.generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect("already at light");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn demotion_is_reported_for_unwired_targets() {
        // The Demotion error is not shadowed by the unwired-status check: a
        // regression against a chunk at an unwired status reports Demotion, so
        // callers matching on it can short-circuit.
        let (mut ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Spawn);
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Surface)
            .expect_err("surface is below spawn");
        assert_eq!(
            err,
            GenError::Demotion {
                target: ChunkStatus::Surface,
                current: ChunkStatus::Spawn,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
    }

    #[test]
    fn mispositioned_generate_noise_is_refused_atomically() {
        // A pyramid that places the GenerateNoise task at the BIOMES rung must
        // be refused by the pre-check before any work runs — the documented
        // atomicity holds for the task/status mismatch too. The chunk is
        // untouched and neither seam ran.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let pyramid = ChunkPyramid::builder()
            .step(ChunkStatus::Empty, |s| s)
            .step(ChunkStatus::StructureStarts, |s| {
                s.set_task(ChunkStatusTask::GenerateStructureStarts)
            })
            .step(ChunkStatus::StructureReferences, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateStructureReferences)
            })
            .step(ChunkStatus::Biomes, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateNoise)
            })
            .step(ChunkStatus::Noise, |s| {
                s.add_requirement(ChunkStatus::Biomes, 1)
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .build();
        let err = ctx
            .generate_through(&pyramid, &mut chunk, ChunkStatus::Noise)
            .expect_err("mispositioned noise refused");
        assert_eq!(
            err,
            GenError::TaskStatusMismatch {
                status: ChunkStatus::Biomes,
                task: ChunkStatusTask::GenerateNoise,
            }
        );
        // The chunk is untouched AND neither seam ran.
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // The single-step seam refuses the same rung mismatch for the NOISE
        // task before running the misplaced task.
        let biomes_step = pyramid.get_step_to(ChunkStatus::Biomes);
        let err = ctx
            .run_step(biomes_step, &mut chunk)
            .expect_err("run_step refuses the rung mismatch");
        assert_eq!(
            err,
            GenError::TaskStatusMismatch {
                status: ChunkStatus::Biomes,
                task: ChunkStatusTask::GenerateNoise,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn generate_biomes_at_the_noise_rung_is_refused_atomically() {
        // The task's headline malformed pyramid: GenerateBiomes installed at
        // the NOISE rung. Dispatching it would run the biomes seam and label
        // the chunk NOISE with no noise data — both the pre-check and the
        // runtime guard refuse it before any mutation.
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        let pyramid = ChunkPyramid::builder()
            .step(ChunkStatus::Empty, |s| s)
            .step(ChunkStatus::StructureStarts, |s| {
                s.set_task(ChunkStatusTask::GenerateStructureStarts)
            })
            .step(ChunkStatus::StructureReferences, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateStructureReferences)
            })
            .step(ChunkStatus::Biomes, |s| {
                s.add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .step(ChunkStatus::Noise, |s| {
                s.add_requirement(ChunkStatus::Biomes, 1)
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .set_task(ChunkStatusTask::GenerateBiomes)
            })
            .build();
        // The pre-check walks the whole path first and refuses at the NOISE
        // rung, before the BIOMES step (which would otherwise run first) does.
        let err = ctx
            .generate_through(&pyramid, &mut chunk, ChunkStatus::Noise)
            .expect_err("generate biomes at the noise rung refused");
        assert_eq!(
            err,
            GenError::TaskStatusMismatch {
                status: ChunkStatus::Noise,
                task: ChunkStatusTask::GenerateBiomes,
            }
        );
        // The chunk is untouched AND neither seam ran.
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // The single-step seam refuses the same rung mismatch before running
        // the misplaced task.
        let noise_step = pyramid.get_step_to(ChunkStatus::Noise);
        let err = ctx
            .run_step(noise_step, &mut chunk)
            .expect_err("run_step refuses the rung mismatch");
        assert_eq!(
            err,
            GenError::TaskStatusMismatch {
                status: ChunkStatus::Noise,
                task: ChunkStatusTask::GenerateBiomes,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    /// A `StarLightProvider` recording every call, plus the light-correct
    /// flags a test sets directly on the chunk. Shared through `Arc<Mutex>` so
    /// the test can read the `dyn` calls after the engine owns the box.
    #[derive(Clone)]
    struct RecordingLight {
        log: Arc<Mutex<LightLog>>,
        supports_persisted_load: bool,
        missing_center: bool,
    }

    #[derive(Default)]
    struct LightLog {
        lit: Vec<(ChunkPos, Vec<Option<bool>>)>,
        force_loaded: Vec<(ChunkPos, Vec<Option<bool>>)>,
        edge_checks: Vec<ChunkPos>,
        block_changes: Vec<BlockPos>,
        section_changes: Vec<(SectionPos, bool)>,
    }

    impl StarLightProvider for RecordingLight {
        fn block_change(&mut self, pos: BlockPos) {
            self.log.lock().unwrap().block_changes.push(pos);
        }
        fn section_change(&mut self, pos: SectionPos, new_empty_value: bool) {
            self.log
                .lock()
                .unwrap()
                .section_changes
                .push((pos, new_empty_value));
        }
        fn light_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
            self.log
                .lock()
                .unwrap()
                .lit
                .push((pos, empty_sections.to_vec()));
        }
        fn try_light_chunk(
            &mut self,
            pos: ChunkPos,
            empty_sections: &[Option<bool>],
        ) -> Result<(), LightProviderError> {
            if self.missing_center {
                Err(LightProviderError::MissingChunk(pos))
            } else {
                self.light_chunk(pos, empty_sections);
                Ok(())
            }
        }
        fn force_load_in_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
            self.log
                .lock()
                .unwrap()
                .force_loaded
                .push((pos, empty_sections.to_vec()));
        }
        fn relight_chunks(&mut self, _chunks: &HashSet<ChunkPos>) {}
        fn check_chunk_edges(&mut self, pos: ChunkPos) {
            self.log.lock().unwrap().edge_checks.push(pos);
        }
        fn supports_persisted_light_load(&self) -> bool {
            self.supports_persisted_load
        }
        fn get_sky_light_value(&self, _pos: BlockPos) -> i32 {
            0
        }
        fn get_block_light_value(&self, _pos: BlockPos) -> i32 {
            0
        }
        fn get_data_layer_data(
            &self,
            _pos: SectionPos,
        ) -> Option<crate::chunk::data_layer::DataLayer> {
            None
        }
    }

    /// A generated task with an unusable provider. The legacy split LIGHT seam
    /// is attached alongside it to pin the executor's precedence and atomic
    /// preflight: the generated task wins, and no earlier step may run before
    /// the final capability refusal.
    struct UnusableGeneratedTask;

    impl GeneratedLightTask<u8, u8, &'static str> for UnusableGeneratedTask {
        fn has_usable_engine(&self) -> bool {
            false
        }

        fn validate_light(
            &self,
            _chunk: &ProtoChunk<u8, u8, &'static str>,
        ) -> Result<(), GenError> {
            Ok(())
        }

        fn initialize_light(
            &mut self,
            _chunk: &mut ProtoChunk<u8, u8, &'static str>,
        ) -> Result<(), GenError> {
            panic!("unusable generated task must fail in preflight")
        }

        fn light(&mut self, _chunk: &mut ProtoChunk<u8, u8, &'static str>) -> Result<(), GenError> {
            panic!("unusable generated task must fail in preflight")
        }

        fn take_owned_runtime_storage(&mut self) -> Option<()> {
            None
        }
    }

    fn light_engine() -> (LevelLightEngine, Arc<Mutex<LightLog>>) {
        light_engine_with_persisted_load(true)
    }

    fn light_engine_with_persisted_load(
        supports_persisted_load: bool,
    ) -> (LevelLightEngine, Arc<Mutex<LightLog>>) {
        let light = RecordingLight {
            log: Arc::new(Mutex::new(LightLog::default())),
            supports_persisted_load,
            missing_center: false,
        };
        let log = Arc::clone(&light.log);
        (
            LevelLightEngine::with_provider(
                Box::new(create_accessor(-64, 384)),
                true,
                true,
                Box::new(light),
            ),
            log,
        )
    }

    fn missing_center_light_engine() -> LevelLightEngine {
        let light = RecordingLight {
            log: Arc::new(Mutex::new(LightLog::default())),
            supports_persisted_load: true,
            missing_center: true,
        };
        LevelLightEngine::with_provider(
            Box::new(create_accessor(-64, 384)),
            true,
            true,
            Box::new(light),
        )
    }

    /// The fresh chunk has 24 all-air sections, so `getEmptySectionsForChunk`
    /// reports every section empty (`Some(true)`) — the mask the LIGHT task
    /// hands the provider.
    fn empty_mask() -> Vec<Option<bool>> {
        vec![Some(true); 24]
    }

    /// A chunk with its persisted status already at FEATURES — the last status
    /// before the light rungs, and the state a real generated chunk reaches
    /// before lighting. The light tests seed a chunk that has already passed
    /// the worldgen rungs and drive the INITIALIZE_LIGHT/LIGHT steps directly;
    /// the SPAWN seam tests start from the same chunk and attach a recording
    /// spawn seam.
    fn features_chunk() -> ProtoChunk<u8, u8, &'static str> {
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Features);
        chunk
    }

    fn light_step() -> &'static ChunkStep {
        GENERATION_PYRAMID.get_step_to(ChunkStatus::Light)
    }

    fn initialize_light_step() -> &'static ChunkStep {
        GENERATION_PYRAMID.get_step_to(ChunkStatus::InitializeLight)
    }

    fn spawn_step() -> &'static ChunkStep {
        GENERATION_PYRAMID.get_step_to(ChunkStatus::Spawn)
    }

    fn full_step() -> &'static ChunkStep {
        GENERATION_PYRAMID.get_step_to(ChunkStatus::Full)
    }

    /// A `WorldGenContext` carrying the SPAWN seam: a recording light engine
    /// plus a recording spawn closure. The spawn closure records its invocation
    /// into a shared log; the light log comes back alongside so a test can
    /// assert that the light step ran exactly once before SPAWN.
    type SpawnContext = (
        WorldGenContext<u8, u8, &'static str>,
        Rc<RefCell<Vec<&'static str>>>,
    );

    fn spawn_context() -> (SpawnContext, Arc<Mutex<LightLog>>) {
        let (ctx, _b, _n, _s, _c, _f) = recording_context();
        let (engine, log) = light_engine();
        let spawn_calls = Rc::new(RefCell::new(Vec::new()));
        let spawn_log = Rc::clone(&spawn_calls);
        let ctx = ctx.with_light_engine(engine).with_spawn(
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| {
                spawn_log.borrow_mut().push("spawn");
                Ok(())
            },
        );
        ((ctx, spawn_calls), log)
    }

    #[test]
    fn generated_light_preflight_wins_over_legacy_light_seam_atomically() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut ctx = ctx.with_light(|_chunk| {
            panic!("legacy LIGHT seam must not run when generated task is attached")
        });
        assert!(
            ctx.attach_generated_light_task(UnusableGeneratedTask)
                .is_none()
        );
        let mut chunk = proto();

        let error = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect_err("unusable generated task must fail before dispatch");
        assert_eq!(
            error,
            GenError::LightEngineMissing {
                status: ChunkStatus::Light
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
    }

    /// `ChunkLightTask.LightTask.getAsBoolean` on a fresh, unlit chunk: the
    /// INITIALIZE_LIGHT step records the engine and computes nothing; the LIGHT
    /// step recomputes (`setLightCorrect(false)` → `lightChunk` →
    /// `setLightCorrect(true)`) and advances the ProtoChunk from
    /// `LIGHT.getParent()` (`INITIALIZE_LIGHT`) to `LIGHT` — the only status
    /// advance inside a light task.
    #[test]
    fn generate_through_light_lights_a_fresh_chunk_and_advances_to_light() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let (engine, log) = light_engine();
        let mut ctx = ctx.with_light_engine(engine);
        let mut chunk = features_chunk();
        assert!(!chunk.is_light_correct());

        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect("through light");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        let seen = log.lock().unwrap();
        // The INITIALIZE_LIGHT step made no provider calls (Java's
        // `initializeLight` completes immediately); the LIGHT step lit once.
        assert_eq!(seen.lit, vec![(ChunkPos::ZERO, empty_mask())]);
        assert!(seen.force_loaded.is_empty());
        assert!(seen.edge_checks.is_empty());
    }

    /// A chunk already at `LIGHT` and light-correct is *loaded*, not relit:
    /// `forceLoadInChunk` + `checkChunkEdges`, no `lightChunk`, no
    /// light-correct toggle, status unchanged. Driven through the single-step
    /// seam because `generate_through` short-circuits at `target == current`.
    #[test]
    fn light_task_loads_an_already_lighted_chunk() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let (engine, log) = light_engine();
        let mut ctx = ctx.with_light_engine(engine);
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);

        ctx.run_step(light_step(), &mut chunk)
            .expect("already at light");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        let seen = log.lock().unwrap();
        assert!(seen.lit.is_empty());
        assert_eq!(seen.force_loaded, vec![(ChunkPos::ZERO, empty_mask())]);
        assert_eq!(seen.edge_checks, vec![ChunkPos::ZERO]);
    }

    #[test]
    fn persisted_light_load_refuses_without_edge_reconciliation() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let (engine, log) = light_engine_with_persisted_load(false);
        let mut ctx = ctx.with_light_engine(engine);
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);

        assert_eq!(
            ctx.run_step(light_step(), &mut chunk),
            Err(GenError::PersistedLightLoadUnsupported {
                status: ChunkStatus::Light
            })
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        let seen = log.lock().unwrap();
        assert!(seen.lit.is_empty());
        assert_eq!(seen.force_loaded, vec![(ChunkPos::ZERO, empty_mask())]);
        assert_eq!(seen.edge_checks, vec![ChunkPos::ZERO]);
    }

    /// Java's `LightTask` branch is the conjunction
    /// `isLightCorrect() && isOrAfter(LIGHT)` — a chunk at `LIGHT` that is not
    /// light-correct is *recomputed* (`setLightCorrect(false)` → `lightChunk`
    /// → `setLightCorrect(true)`), not loaded. `isLighted` (both true) is the
    /// only path to `forceLoadInChunk`.
    #[test]
    fn a_light_status_chunk_not_light_correct_is_recomputed() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let (engine, log) = light_engine();
        let mut ctx = ctx.with_light_engine(engine);
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Light);

        ctx.run_step(light_step(), &mut chunk)
            .expect("recompute a not-light-correct chunk");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        let seen = log.lock().unwrap();
        assert_eq!(seen.lit, vec![(ChunkPos::ZERO, empty_mask())]);
        assert!(seen.force_loaded.is_empty());
        assert!(seen.edge_checks.is_empty());
    }

    /// An engine-less context targeting `LIGHT` is refused before any step
    /// runs — the chunk stays at `FEATURES` and neither worldgen seam ran
    /// (Java would NPE on `context.lightEngine()`).
    #[test]
    fn light_requires_the_engine_atomically() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = features_chunk();
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect_err("no engine");
        assert_eq!(
            err,
            GenError::LightEngineMissing {
                status: ChunkStatus::Light
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // The single-step INITIALIZE_LIGHT body also refuses without an engine.
        let err = ctx
            .run_step(initialize_light_step(), &mut chunk)
            .expect_err("initialize light needs the engine");
        assert_eq!(
            err,
            GenError::LightEngineMissing {
                status: ChunkStatus::InitializeLight
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
    }

    #[test]
    fn missing_center_is_typed_and_does_not_promote_or_toggle_light() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut ctx = ctx.with_light_engine(missing_center_light_engine());
        let mut chunk = features_chunk();

        assert_eq!(
            ctx.run_step(light_step(), &mut chunk),
            Err(GenError::LightChunkMissing {
                status: ChunkStatus::Light,
                pos: ChunkPos::ZERO,
            })
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(!chunk.is_light_correct());
    }

    /// A provider-less engine is not a usable engine: `LevelLightEngine::new`
    /// (the public constructor) leaves the provider `None` — the seam attaches
    /// one via `with_provider`. `generate_through` must refuse it at the
    /// precheck, so the whole path is a no-op — no step runs, the chunk stays
    /// at `FEATURES` unlit, and neither worldgen seam ran. The atomic-refusal
    /// invariant holds for a present-but-provider-less engine, not just an
    /// absent one. (A fresh `EMPTY` chunk cannot reach LIGHT in the value
    /// layer — the FEATURES rung's placement bodies are unwired and it fails
    /// typed at the placement-decode boundary — so the light path starts from
    /// the pre-light `FEATURES` state, where INITIALIZE_LIGHT is the only step
    /// before LIGHT; the biomes/noise assertions prove no earlier step ran.)
    #[test]
    fn a_provider_less_engine_is_refused_atomically() {
        let (ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = features_chunk();
        let engine = LevelLightEngine::new(Box::new(create_accessor(-64, 384)), true, true);
        let mut ctx = ctx.with_light_engine(engine);

        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect_err("provider-less engine cannot light");
        assert_eq!(
            err,
            GenError::LightEngineMissing {
                status: ChunkStatus::Light
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(!chunk.is_light_correct());
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // The single-step INITIALIZE_LIGHT body also refuses a provider-less
        // engine — the step must not record it (the LIGHT step could never
        // light through it), so the chunk is left untouched.
        let err = ctx
            .run_step(initialize_light_step(), &mut chunk)
            .expect_err("initialize light needs a provider");
        assert_eq!(
            err,
            GenError::LightEngineMissing {
                status: ChunkStatus::InitializeLight
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(!chunk.is_light_correct());
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // The single-step LIGHT body refuses before mutating light-correct:
        // the provider deref precedes `setLightCorrect(false)`.
        let err = ctx
            .run_step(light_step(), &mut chunk)
            .expect_err("light needs a provider");
        assert_eq!(
            err,
            GenError::LightEngineMissing {
                status: ChunkStatus::Light
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(!chunk.is_light_correct());
    }

    /// `generate_through` may stop at `INITIALIZE_LIGHT` with an engine
    /// attached: the step records the engine, computes nothing, and leaves the
    /// chunk unlit at `INITIALIZE_LIGHT` (the LIGHT task lights it later).
    #[test]
    fn initialize_light_step_records_the_engine_and_computes_nothing() {
        let (ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let (engine, log) = light_engine();
        let mut ctx = ctx.with_light_engine(engine);
        let mut chunk = features_chunk();

        ctx.generate_through(
            &GENERATION_PYRAMID,
            &mut chunk,
            ChunkStatus::InitializeLight,
        )
        .expect("through initialize light");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::InitializeLight);
        assert!(!chunk.is_light_correct());
        let seen = log.lock().unwrap();
        assert!(seen.lit.is_empty());
        assert!(seen.force_loaded.is_empty());
        assert!(seen.edge_checks.is_empty());
    }

    /// A chunk loaded from disk at `LIGHT` and light-correct is confirmed as a
    /// no-op through the LOADING pyramid: `generate_through` short-circuits at
    /// `target == current` before examining any step. The LOADING pyramid's
    /// LIGHT step does carry the `Light` task (mirroring
    /// `ChunkLightTask.LightTask.getAsBoolean`, like the GENERATION pyramid's
    /// LIGHT rung), but it is never reached for an already-lighted chunk — the
    /// idempotent confirm is what keeps loading from relighting it.
    #[test]
    fn loading_confirms_an_existing_lighted_chunk_idempotently() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);

        ctx.generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect("confirm through loading");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    /// From a synthetic FEATURES chunk with a recording light engine and
    /// recording SPAWN seam, `generate_through(Spawn)` runs the light and spawn
    /// steps in order and stamps only SPAWN. FULL remains a separate consuming
    /// holder transaction; the borrowed executor refuses it without mutating
    /// the proto.
    #[test]
    fn generate_through_spawn_runs_light_and_spawn_in_order_once_each() {
        let ((mut ctx, spawn_calls), log) = spawn_context();
        let mut chunk = features_chunk();

        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Spawn)
            .expect("through spawn");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
        assert!(chunk.is_light_correct());
        let seen = log.lock().unwrap();
        assert_eq!(seen.lit, vec![(ChunkPos::ZERO, empty_mask())]);
        assert!(seen.force_loaded.is_empty());
        assert!(seen.edge_checks.is_empty());
        assert_eq!(spawn_calls.borrow().as_slice(), &["spawn"]);

        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect_err("FULL requires consuming promotion");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Full));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
        assert_eq!(spawn_calls.borrow().as_slice(), &["spawn"]);
    }

    /// The SPAWN step alone on an unlit chunk (seam attached, chunk below
    /// LIGHT) errors `SpawnNotGenerated` and never runs the spawn body — the
    /// natural spawner reads light through `checkSpawnRules`, so an unlit chunk
    /// must not be spawned.
    #[test]
    fn spawn_step_alone_fails_without_light() {
        let ((mut ctx, spawn_calls), _log) = spawn_context();
        let mut chunk = features_chunk();

        let err = ctx
            .run_step(spawn_step(), &mut chunk)
            .expect_err("spawn needs light");
        assert_eq!(err, GenError::SpawnNotGenerated);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(spawn_calls.borrow().is_empty());
    }

    /// The FULL step is retained in the status pyramid, but the borrowed
    /// executor refuses it because Paper's body consumes the SPAWN proto into a
    /// LevelChunk. The proto is not stamped or otherwise changed.
    #[test]
    fn full_step_requires_consuming_promotion() {
        let ((mut ctx, _spawn_calls), _log) = spawn_context();
        let mut chunk = features_chunk();
        chunk.set_persisted_status(ChunkStatus::Spawn);

        let err = ctx
            .run_step(full_step(), &mut chunk)
            .expect_err("full requires consuming promotion");
        assert_eq!(
            err,
            GenError::UnsupportedTask {
                status: ChunkStatus::Full,
                task: ChunkStatusTask::Full,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
    }

    /// `ChunkStatusTasks.generateSpawn` skips the generator body for a chunk
    /// carrying BelowZeroRetrogen. The status still completes, and this path
    /// does not require a spawn closure because Java never calls it.
    #[test]
    fn upgrading_spawn_skips_population_and_advances_without_a_seam() {
        let (mut ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = features_chunk();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_upgrading(ChunkStatus::Spawn);

        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Spawn)
            .expect("retrogen spawn is skipped");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
        assert!(!chunk.is_upgrading());
    }

    /// Dispatching the SPAWN step on a context with no spawn seam keeps the
    /// typed refusal the executor always produced — `UnsupportedTask` — and the
    /// chunk is not labeled SPAWN.
    #[test]
    fn spawn_step_without_seam_is_refused() {
        let (mut ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();

        let err = ctx
            .run_step(spawn_step(), &mut chunk)
            .expect_err("no spawn seam");
        assert_eq!(
            err,
            GenError::UnsupportedTask {
                status: ChunkStatus::Spawn,
                task: ChunkStatusTask::GenerateSpawn,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
    }

    /// Dispatching the FULL step through the borrowed context keeps the typed
    /// refusal — `UnsupportedTask` — and the chunk is not labeled FULL.
    #[test]
    fn full_step_is_refused_as_consuming_boundary() {
        let (mut ctx, _biomes_calls, _noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = proto();

        let err = ctx
            .run_step(full_step(), &mut chunk)
            .expect_err("FULL requires consuming promotion");
        assert_eq!(
            err,
            GenError::UnsupportedTask {
                status: ChunkStatus::Full,
                task: ChunkStatusTask::Full,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
    }

    /// Targeting SPAWN without a spawn seam reports `UnsupportedStatus` at the
    /// target gate before any work runs — the pre-seam refusal.
    #[test]
    fn generate_through_spawn_without_spawn_seam_is_refused() {
        let (mut ctx, biomes_calls, noise_calls, _surface_calls, _carvers_calls, _features_calls) =
            recording_context();
        let mut chunk = features_chunk();

        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Spawn)
            .expect_err("no spawn seam");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Spawn));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Features);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    /// Targeting FULL always stops at the consuming-promotion boundary before
    /// any borrowed executor work runs, even when a chunk is already at SPAWN.
    #[test]
    fn generate_through_full_refuses_before_mutating_the_proto() {
        let (mut ctx, _b, _n, _s, _c, _f) = recording_context();
        let mut chunk = features_chunk();
        chunk.set_persisted_status(ChunkStatus::Spawn);

        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect_err("FULL requires consuming promotion");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Full));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Spawn);
    }

    /// A persisted FULL proto is a loaded representation, not a borrowed
    /// executor input. The direct persisted-FULL case must still refuse the
    /// borrowed request instead of taking the target==current no-op path.
    #[test]
    fn generate_through_persisted_full_proto_refuses_borrowed_full() {
        let (mut ctx, biomes_calls, noise_calls, surface_calls, carvers_calls, features_calls) =
            recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Full);

        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect_err("a borrowed FULL request must use consuming promotion");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Full));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
        assert!(surface_calls.borrow().is_empty());
        assert!(carvers_calls.borrow().is_empty());
        assert!(features_calls.borrow().is_empty());
    }
}
