//! Port of `net.minecraft.world.level.chunk.status.WorldGenContext` (MC 26.2)
//! — the record of per-step worldgen dependencies — plus the value-layer
//! executor seam that runs the generation DAG through LIGHT.
//!
//! Java: `WorldGenContext.java` in `working/Paper` — a 6-field record
//! `(ServerLevel, ChunkGenerator, StructureTemplateManager,
//! ThreadedLevelLightEngine, Executor, UnsavedListener)`. In the value layer
//! the server/light/template surfaces do not exist yet (they defer with their
//! owning units), and the full `ChunkGenerator` is owned by the generator wave
//! (#306/#185 — today only the `&dyn ChunkGenerator` seam in
//! `chunk::chunk_generator` exists), so the record is reduced to the task seam
//! the generation pyramid through LIGHT actually needs: the caller-supplied
//! closures that perform the BIOMES and NOISE work, plus the
//! [`StarLightProvider`] the INITIALIZE_LIGHT/LIGHT tasks route to. The full
//! record shape returns with the `mc.world.level.chunk.generator` wave
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
//!    at the LIGHT rung), dereferencing it so a missing engine errors like
//!    Java's NPE but computing nothing; the LIGHT body preserves Java's
//!    load-vs-compute branch — a chunk that is already lighted (`isLighted` =
//!    persisted status at/after `LIGHT` && light-correct, the value-layer flag
//!    `ChunkAccess.isLightCorrect`) is *loaded* through the seam
//!    (`forceLoadInChunk` + `checkChunkEdges`), a chunk that is not is
//!    recomputed (`setLightCorrect(false)` → `lightChunk` →
//!    `setLightCorrect(true)`), and a ProtoChunk at `LIGHT.getParent()`
//!    (`INITIALIZE_LIGHT`) advances to `LIGHT` — `ChunkLightTask`'s status
//!    advance, and the only status advance that happens inside a light task.
//!    Real light data is never produced here: the provider is the `rivet-server`
//!    Starlight impl (today `StubStarLightProvider`), so the slice is faithful
//!    to the *dispatch* — the engine that computes the nibbles defers with the
//!    `ca.spottedleaf.moonrise.patches.starlight.light` unit (#184).
//!
//! A target already at/after the chunk's status is handled before any work:
//! `target == current` is an idempotent no-op at *any* status (so a loaded
//! chunk persisted past `LIGHT` can be confirmed through the LOADING pyramid),
//! and a lower target is a demotion error rather than an unwired-status error.
//!
//! The SURFACE..FEATURES and SPAWN/FULL task bodies are *not wired*
//! (RivetTodo #185): dispatching one returns [`GenError::UnsupportedTask`].

use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::status::chunk_status_task::ChunkStatusTask;
use crate::chunk::status::chunk_status_tasks;
use crate::chunk::status::chunk_step::ChunkStep;
use crate::chunk::status::{ChunkPyramid, ChunkStatus};
use crate::lighting::level_light_engine::LevelLightEngine;

/// The `generateBiomes` seam closure type.
type BiomesSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateNoise` seam closure type.
type NoiseSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);

/// `WorldGenContext` (value-layer seam shape) — the caller-supplied BIOMES and
/// NOISE worldgen closures, plus the light engine the INITIALIZE_LIGHT/LIGHT
/// tasks route to, generic over the chunk's block/biome value types.
pub struct WorldGenContext<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// The `ChunkStatusTasks::generateBiomes` seam — fills the chunk's biomes.
    /// The real body is `ChunkGenerator.createBiomes` (deferred #185); the
    /// caller supplies the closure through this seam.
    biomes: Box<BiomesSeam<T, B, S>>,
    /// The `ChunkStatusTasks::generateNoise` seam — fills the chunk's blocks.
    /// The real body is `ChunkGenerator.fillFromNoise` (deferred #185); the
    /// seam's ordering guards are what keep this from running before BIOMES.
    noise: Box<NoiseSeam<T, B, S>>,
    /// The `ThreadedLevelLightEngine` the INITIALIZE_LIGHT/LIGHT tasks store and
    /// route through (Java's `context.lightEngine()`). The facade holds the
    /// [`StarLightProvider`] (`rivet-server`'s Starlight impl; today
    /// `StubStarLightProvider`) that actually lights the chunk — see the module
    /// doc. `None` when the chunk does not carry the light-engine dependency
    /// (the seam's worldgen runs below INITIALIZE_LIGHT, and the idempotent
    /// loaded-chunk confirm never dispatches a light task).
    light_engine: Option<LevelLightEngine>,
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
    /// The target status is beyond LIGHT (SPAWN/FULL are not wired in the value
    /// layer — the real worldgen defers with #185).
    UnsupportedStatus(ChunkStatus),
    /// A step at a wired status (≤ LIGHT) carries a task body that is not wired
    /// in the value layer (the SURFACE..FEATURES and SPAWN/FULL bodies defer
    /// with #185) — only a malformed custom pyramid can produce this.
    UnsupportedTask {
        status: ChunkStatus,
        task: ChunkStatusTask,
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
            GenError::DataNotCarried { status } => write!(
                f,
                "cannot label the chunk {status:?}: a pass-through step cannot fabricate that data"
            ),
            GenError::LightEngineMissing { status } => write!(
                f,
                "cannot run the {status:?} task: no light engine is attached to the context"
            ),
            GenError::UnsupportedStatus(status) => write!(
                f,
                "status {status:?} is not wired in the value layer (RivetTodo #185)"
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

impl<T, B, S> WorldGenContext<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// Wraps the two worldgen seam closures (owned, mirroring the record). The
    /// light engine is left unattached — the slice wires it with
    /// [`Self::with_light_engine`] when a run targets the light statuses.
    pub fn new(
        biomes: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        noise: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
    ) -> Self {
        WorldGenContext {
            biomes: Box::new(biomes),
            noise: Box::new(noise),
            light_engine: None,
        }
    }

    /// Attaches the light engine (Java's `context.lightEngine()`, the
    /// `ThreadedLevelLightEngine` record member) so the INITIALIZE_LIGHT/LIGHT
    /// tasks can route through it.
    pub fn with_light_engine(mut self, light_engine: LevelLightEngine) -> Self {
        self.light_engine = Some(light_engine);
        self
    }

    /// `ChunkStatusTasks.isLighted(ChunkAccess)` — `getPersistedStatus()
    /// .isOrAfter(ChunkStatus.LIGHT) && isLightCorrect()`. The flag is
    /// `ChunkAccess.isLightCorrect`.
    fn is_lighted(chunk: &ProtoChunk<T, B, S>) -> bool {
        chunk.get_persisted_status().is_or_after(ChunkStatus::Light) && chunk.is_light_correct()
    }

    /// `ChunkStatusTasks.light` + `ChunkLightTask.LightTask.getAsBoolean` (the
    /// Moonrise per-status light dispatch; spec §3.4) — the LIGHT task body,
    /// dispatched through the provider seam. Java's branch: an already-lighted
    /// chunk is *loaded* (`forceLoadInChunk` + `checkChunkEdges`), otherwise it
    /// is recomputed (`setLightCorrect(false)` → `lightChunk` →
    /// `setLightCorrect(true)`), then a ProtoChunk at `LIGHT.getParent()` is
    /// advanced to `LIGHT`. The provider is `rivet-server`'s Starlight impl
    /// (today `StubStarLightProvider`): the nibble computation defers with the
    /// `starlight.light` unit (#184), so the light-correct flag toggling and the
    /// status advance carry the slice's semantics. The INITIALIZE_LIGHT task
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
        if Self::is_lighted(chunk) {
            let provider = engine
                .provider_mut()
                .ok_or(GenError::LightEngineMissing { status })?;
            provider.force_load_in_chunk(chunk.get_pos(), &empty_sections);
            provider.check_chunk_edges(chunk.get_pos());
        } else {
            chunk.set_light_correct(false);
            let provider = engine
                .provider_mut()
                .ok_or(GenError::LightEngineMissing { status })?;
            provider.light_chunk(chunk.get_pos(), &empty_sections);
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
    /// The `NOISE` body is gated on the persisted-status record, and a wired
    /// generation task is honored only at its canonical rung
    /// (`GenerateBiomes` at `BIOMES`, `GenerateNoise` at `NOISE`). The
    /// `INITIALIZE_LIGHT` body mirrors `ChunkStatusTasks.initializeLight`
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
                // missing engine errors like Java's NPE — but computes nothing:
                // no light-correct toggle, no provider call. A chunk stays at
                // `INITIALIZE_LIGHT` until the LIGHT task lights or loads it.
                if self.light_engine.is_none() {
                    return Err(GenError::LightEngineMissing {
                        status: ChunkStatus::InitializeLight,
                    });
                }
            }
            ChunkStatusTask::Light => {
                ensure_canonical_rung(step.task(), step.target_status())?;
                self.run_light_task(chunk, ChunkStatus::Light)?;
            }
            ChunkStatusTask::GenerateSurface
            | ChunkStatusTask::GenerateCarvers
            | ChunkStatusTask::GenerateFeatures
            | ChunkStatusTask::GenerateSpawn
            | ChunkStatusTask::Full => {
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
    /// (inclusive, ≤ `LIGHT`), running each step's task in status order.
    ///
    /// The `BIOMES`-before-`NOISE` ordering is enforced at the *status-order*
    /// layer, not just inside the `NOISE` task body: a promotion whose target
    /// passes through the `BIOMES` step requires the biomes data to be present
    /// — either carried in (the chunk's persisted status was already at/after
    /// `BIOMES`) or produced by this run's `BIOMES` step. A pyramid whose
    /// `BIOMES` or `NOISE` step is a pass-through (the `LOADING_PYRAMID`'s
    /// loading stubs) cannot advance an `EMPTY` chunk past it, because that
    /// would label the chunk with data that was never generated. The whole
    /// path is validated before any work runs — a refused promotion leaves the
    /// chunk untouched. A wired generation task is honored only at its
    /// canonical rung (a malformed pyramid installing `GenerateBiomes` at
    /// `NOISE`, for example, is refused here, so the chunk is never labeled
    /// `NOISE` by a task that produces no noise data). A target beyond `LIGHT`
    /// is rejected before any work runs; a target before the current status is
    /// a demotion error. The INITIALIZE_LIGHT/LIGHT steps are wired (see
    /// [`Self::run_light_task`]) but require the light engine — an engine-less
    /// context targeting them fails [`GenError::LightEngineMissing`] before any
    /// work runs, and a run whose path passes through a light step is where the
    /// step leaves its light-correct/status record.
    pub fn generate_through(
        &mut self,
        pyramid: &ChunkPyramid,
        chunk: &mut ProtoChunk<T, B, S>,
        target: ChunkStatus,
    ) -> Result<(), GenError> {
        let current = chunk.get_persisted_status();
        // Idempotent no-op at any status: a chunk already at the target (even an
        // unwired one, e.g. a loaded FULL chunk) needs no work — this is what
        // lets the LOADING pyramid confirm persisted chunks past LIGHT.
        if target == current {
            return Ok(());
        }
        if target.index() < current.index() {
            return Err(GenError::Demotion { target, current });
        }
        if target.index() > ChunkStatus::Light.index() {
            return Err(GenError::UnsupportedStatus(target));
        }
        // Validate the whole path before running any step, tracking the
        // persisted status each step will leave behind (`projected` mirrors the
        // run loop, so a step the loop would refuse fails here instead and a
        // refused promotion leaves the chunk untouched). Every step the loop
        // would run must be either a wired generation task or a pass-through
        // that does not fabricate BIOMES/NOISE data. The LIGHT step also
        // requires the engine: without it the path cannot reach LIGHT.
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
                ChunkStatusTask::InitializeLight => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    needs_light_engine = true;
                    projected = step.target_status();
                }
                ChunkStatusTask::Light => {
                    ensure_canonical_rung(step.task(), step.target_status())?;
                    needs_light_engine = true;
                    projected = step.target_status();
                }
                ChunkStatusTask::GenerateSurface
                | ChunkStatusTask::GenerateCarvers
                | ChunkStatusTask::GenerateFeatures
                | ChunkStatusTask::GenerateSpawn
                | ChunkStatusTask::Full => {
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
        if needs_light_engine && self.light_engine.is_none() {
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
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8>> {
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
    );

    fn recording_context() -> RecordingContext {
        let biomes_calls = Rc::new(RefCell::new(Vec::new()));
        let noise_calls = Rc::new(RefCell::new(Vec::new()));
        let biomes_log = Rc::clone(&biomes_calls);
        let noise_log = Rc::clone(&noise_calls);
        let ctx = WorldGenContext::new(
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| biomes_log.borrow_mut().push("biomes"),
            move |_c: &mut ProtoChunk<u8, u8, &'static str>| noise_log.borrow_mut().push("noise"),
        );
        (ctx, biomes_calls, noise_calls)
    }

    #[test]
    fn generate_through_promotes_step_by_step_in_dag_order_through_noise() {
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
        let mut chunk = proto();
        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Biomes)
            .expect("through biomes");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Biomes);
        assert_eq!(biomes_calls.borrow().as_slice(), &["biomes"]);
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn promotion_is_idempotent_at_target() {
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
    fn an_unwired_surface_target_is_rejected_before_any_work() {
        // SURFACE is inside the supported status range (the range check
        // rejects only targets past LIGHT), but its step is unwired
        // (RivetTodo #185): the pre-check reports UnsupportedTask before any
        // work runs, and the chunk is untouched.
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
        let mut chunk = proto();
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Surface)
            .expect_err("surface step is not wired");
        assert_eq!(
            err,
            GenError::UnsupportedTask {
                status: ChunkStatus::Surface,
                task: ChunkStatusTask::GenerateSurface,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());

        // A target past LIGHT (SPAWN/FULL) is out of the supported range and
        // reports UnsupportedStatus instead.
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect_err("full is out of range");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Full));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn dispatching_an_unwired_step_errors() {
        // Even the single-step seam refuses the SURFACE body (RivetTodo #185).
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
        let mut chunk = proto();
        let surface_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Surface);
        let err = ctx
            .run_step(surface_step, &mut chunk)
            .expect_err("surface unwired");
        assert_eq!(
            err,
            GenError::UnsupportedTask {
                status: ChunkStatus::Surface,
                task: ChunkStatusTask::GenerateSurface,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Empty);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn loading_pyramid_cannot_promote_a_fresh_chunk_past_empty() {
        // The LOADING pyramid's BIOMES/NOISE steps are pass-through loading
        // stubs — they would advance the persisted status without running any
        // biomes task. The seam refuses that on a fresh (EMPTY) chunk: the
        // chunk must not be labeled BIOMES (let alone NOISE) through them.
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
    fn refused_promotion_is_atomic_across_the_whole_path() {
        // Atomicity: when any step in the target path is refused, no earlier
        // step runs either. A chunk already at STRUCTURE_REFERENCES promoted
        // to NOISE through a pyramid whose NOISE step is a pass-through would
        // have BIOMES (an earlier, valid step) in the dispatch loop — the
        // pre-check must refuse before BIOMES runs, leaving the chunk and the
        // biomes seam untouched.
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, _biomes_calls, _noise_calls) = recording_context();
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
    fn loading_a_full_chunk_is_an_idempotent_no_op() {
        // A chunk loaded from disk at FULL is confirmed as a no-op through the
        // LOADING pyramid: target == current returns Ok without wiring any
        // SURFACE..FULL task body (RivetTodo #185).
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Full);
        ctx.generate_through(&LOADING_PYRAMID, &mut chunk, ChunkStatus::Full)
            .expect("already at full");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert!(biomes_calls.borrow().is_empty());
        assert!(noise_calls.borrow().is_empty());
    }

    #[test]
    fn demotion_is_reported_for_unwired_targets() {
        // The Demotion error is not shadowed by the unwired-status check: a
        // regression against a chunk at an unwired status reports Demotion, so
        // callers matching on it can short-circuit.
        let (mut ctx, _biomes_calls, _noise_calls) = recording_context();
        let mut chunk = proto();
        chunk.set_persisted_status(ChunkStatus::Full);
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Surface)
            .expect_err("surface is below full");
        assert_eq!(
            err,
            GenError::Demotion {
                target: ChunkStatus::Surface,
                current: ChunkStatus::Full,
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
    }

    #[test]
    fn mispositioned_generate_noise_is_refused_atomically() {
        // A pyramid that places the GenerateNoise task at the BIOMES rung must
        // be refused by the pre-check before any work runs — the documented
        // atomicity holds for the task/status mismatch too. The chunk is
        // untouched and neither seam ran.
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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

    fn light_engine() -> (LevelLightEngine, Arc<Mutex<LightLog>>) {
        let light = RecordingLight {
            log: Arc::new(Mutex::new(LightLog::default())),
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

    /// The fresh chunk has 24 all-air sections, so `getEmptySectionsForChunk`
    /// reports every section empty (`Some(true)`) — the mask the LIGHT task
    /// hands the provider.
    fn empty_mask() -> Vec<Option<bool>> {
        vec![Some(true); 24]
    }

    /// A chunk with its persisted status already at FEATURES — the last status
    /// before the light rungs, and the state a real generated chunk reaches
    /// before lighting. The generation pyramid's SURFACE..FEATURES steps are
    /// unwired (RivetTodo #185), so the light tests seed a chunk that has
    /// already passed them and drive the INITIALIZE_LIGHT/LIGHT steps directly.
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

    /// `ChunkLightTask.LightTask.getAsBoolean` on a fresh, unlit chunk: the
    /// INITIALIZE_LIGHT step records the engine and computes nothing; the LIGHT
    /// step recomputes (`setLightCorrect(false)` → `lightChunk` →
    /// `setLightCorrect(true)`) and advances the ProtoChunk from
    /// `LIGHT.getParent()` (`INITIALIZE_LIGHT`) to `LIGHT` — the only status
    /// advance inside a light task.
    #[test]
    fn generate_through_light_lights_a_fresh_chunk_and_advances_to_light() {
        let (ctx, _biomes_calls, _noise_calls) = recording_context();
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
        let (ctx, _biomes_calls, _noise_calls) = recording_context();
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

    /// Java's `LightTask` branch is the conjunction
    /// `isLightCorrect() && isOrAfter(LIGHT)` — a chunk at `LIGHT` that is not
    /// light-correct is *recomputed* (`setLightCorrect(false)` → `lightChunk`
    /// → `setLightCorrect(true)`), not loaded. `isLighted` (both true) is the
    /// only path to `forceLoadInChunk`.
    #[test]
    fn a_light_status_chunk_not_light_correct_is_recomputed() {
        let (ctx, _biomes_calls, _noise_calls) = recording_context();
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
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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

    /// `generate_through` may stop at `INITIALIZE_LIGHT` with an engine
    /// attached: the step records the engine, computes nothing, and leaves the
    /// chunk unlit at `INITIALIZE_LIGHT` (the LIGHT task lights it later).
    #[test]
    fn initialize_light_step_records_the_engine_and_computes_nothing() {
        let (ctx, _biomes_calls, _noise_calls) = recording_context();
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
    /// no-op through the LOADING pyramid: the LIGHT loading step is a
    /// pass-through here (the loading DAG carries the chunk's existing status,
    /// never relighting it — the value-layer loading step has no light task).
    #[test]
    fn loading_confirms_an_existing_lighted_chunk_idempotently() {
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
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
}
