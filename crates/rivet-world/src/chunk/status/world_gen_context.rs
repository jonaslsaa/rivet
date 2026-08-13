//! Port of `net.minecraft.world.level.chunk.status.WorldGenContext` (MC 26.2)
//! — the record of per-step worldgen dependencies — plus the value-layer
//! executor seam that runs the generation DAG through NOISE.
//!
//! Java: `WorldGenContext.java` in `working/Paper` — a 6-field record
//! `(ServerLevel, ChunkGenerator, StructureTemplateManager,
//! ThreadedLevelLightEngine, Executor, UnsavedListener)`. In the value layer
//! the server/light/template surfaces do not exist yet (they defer with their
//! owning units), and the full `ChunkGenerator` is owned by the generator wave
//! (#306/#185 — today only the `&dyn ChunkGenerator` seam in
//! `chunk::chunk_generator` exists), so the record is reduced to the task seam
//! the generation pyramid through NOISE actually needs: the caller-supplied
//! closures that perform the BIOMES and NOISE work. The full record shape
//! returns with the `mc.world.level.chunk.generator` wave (RivetTodo #185).
//! The closures are `'static`-owned (a value-layer simplification): the real
//! worldgen bodies borrow server/region state and run in the #185 scheduler
//! realization, not through this seam.
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
//!
//! A target already at/after the chunk's status is handled before any work:
//! `target == current` is an idempotent no-op at *any* status (so a loaded
//! chunk persisted past `NOISE` can be confirmed through the LOADING pyramid),
//! and a lower target is a demotion error rather than an unwired-status error.
//!
//! The SURFACE..FULL task bodies are *not wired* (RivetTodo #185): dispatching
//! one returns [`GenError::UnsupportedTask`].

use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::status::chunk_status_task::ChunkStatusTask;
use crate::chunk::status::chunk_status_tasks;
use crate::chunk::status::chunk_step::ChunkStep;
use crate::chunk::status::{ChunkPyramid, ChunkStatus};

/// The `generateBiomes` seam closure type.
type BiomesSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);
/// The `generateNoise` seam closure type.
type NoiseSeam<T, B, S> = dyn FnMut(&mut ProtoChunk<T, B, S>);

/// `WorldGenContext` (value-layer seam shape) — the caller-supplied BIOMES and
/// NOISE worldgen closures, generic over the chunk's block/biome value types.
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
    /// The target status is beyond NOISE (SURFACE..FULL are not wired in the
    /// value layer — the real worldgen defers with #185).
    UnsupportedStatus(ChunkStatus),
    /// A step at a wired status (≤ NOISE) carries a task body that is not wired
    /// in the value layer (the SURFACE..FULL bodies defer with #185) — only a
    /// malformed custom pyramid can produce this.
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
    /// Wraps the two worldgen seam closures (owned, mirroring the record).
    pub fn new(
        biomes: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
        noise: impl FnMut(&mut ProtoChunk<T, B, S>) + 'static,
    ) -> Self {
        WorldGenContext {
            biomes: Box::new(biomes),
            noise: Box::new(noise),
        }
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
    /// (`GenerateBiomes` at `BIOMES`, `GenerateNoise` at `NOISE`).
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
            ChunkStatusTask::GenerateSurface
            | ChunkStatusTask::GenerateCarvers
            | ChunkStatusTask::GenerateFeatures
            | ChunkStatusTask::InitializeLight
            | ChunkStatusTask::Light
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
    /// (inclusive, ≤ `NOISE`), running each step's task in status order.
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
    /// `NOISE` by a task that produces no noise data). A target beyond `NOISE`
    /// is rejected before any work runs; a target before the current status is
    /// a demotion error.
    pub fn generate_through(
        &mut self,
        pyramid: &ChunkPyramid,
        chunk: &mut ProtoChunk<T, B, S>,
        target: ChunkStatus,
    ) -> Result<(), GenError> {
        let current = chunk.get_persisted_status();
        // Idempotent no-op at any status: a chunk already at the target (even an
        // unwired one, e.g. a loaded FULL chunk) needs no work — this is what
        // lets the LOADING pyramid confirm persisted chunks past NOISE.
        if target == current {
            return Ok(());
        }
        if target.index() < current.index() {
            return Err(GenError::Demotion { target, current });
        }
        if target.index() > ChunkStatus::Noise.index() {
            return Err(GenError::UnsupportedStatus(target));
        }
        // Validate the whole path before running any step, tracking the
        // persisted status each step will leave behind (`projected` mirrors the
        // run loop, so a step the loop would refuse fails here instead and a
        // refused promotion leaves the chunk untouched). Every step the loop
        // would run must be either a wired generation task or a pass-through
        // that does not fabricate BIOMES/NOISE data.
        let mut projected = current;
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
                ChunkStatusTask::GenerateSurface
                | ChunkStatusTask::GenerateCarvers
                | ChunkStatusTask::GenerateFeatures
                | ChunkStatusTask::InitializeLight
                | ChunkStatusTask::Light
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
    use rivet_registry::core::ChunkPos;
    use std::cell::RefCell;
    use std::rc::Rc;

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
    fn target_beyond_noise_is_rejected_before_any_work() {
        let (mut ctx, biomes_calls, noise_calls) = recording_context();
        let mut chunk = proto();
        let err = ctx
            .generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Surface)
            .expect_err("surface is not wired");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Surface));
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
}
