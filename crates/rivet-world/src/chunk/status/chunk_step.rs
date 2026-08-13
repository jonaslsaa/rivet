//! Port of `net.minecraft.world.level.chunk.status.ChunkStep` (MC 26.2) — one
//! rung of the chunk-status pyramid: the target status, its direct and
//! accumulated dependency tables, the block-state write radius, the task
//! identity, and the `byRadius` required-status table.
//!
//! Java: `ChunkStep.java` in `working/Paper` (plus the Moonrise
//! `ChunkSystemChunkStep` seam interface, folded in as
//! [`ChunkStep::required_status_at_radius`]). The `Builder` accumulates the
//! direct dependencies by radius (max-merged by later status) and folds the
//! parent's *accumulated* dependencies through `radiusOfParent` to build each
//! step's full transitive table. The constructor then derives `byRadius`: the
//! minimum status that must be generated at each neighbour distance.
//!
//! The value-layer pyramid through NOISE is built by `ChunkPyramid`; the task
//! bodies and the ordering enforcement live in the executor seam
//! (`world_gen_context.rs`). `ChunkStep.apply`'s async `CompletableFuture`
//! dispatch is deferred with the scheduler (#185); the synchronous
//! `WorldGenContext::run_step` mirrors it.

use crate::chunk::status::ChunkStatus;
use crate::chunk::status::chunk_dependencies::ChunkDependencies;
use crate::chunk::status::chunk_status_task::ChunkStatusTask;

/// `net.minecraft.world.level.chunk.status.ChunkStep`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkStep {
    target_status: ChunkStatus,
    direct_dependencies: ChunkDependencies,
    accumulated_dependencies: ChunkDependencies,
    block_state_write_radius: i32,
    task: ChunkStatusTask,
    /// `byRadius` — `byRadius[d]` = the minimum status required at distance `d`.
    by_radius: Vec<ChunkStatus>,
}

impl ChunkStep {
    /// The Java constructor: builds `byRadius` by walking the parent chain and
    /// prefix-filling each parent status up to its accumulated radius.
    fn new(
        target_status: ChunkStatus,
        direct_dependencies: ChunkDependencies,
        accumulated_dependencies: ChunkDependencies,
        block_state_write_radius: i32,
        task: ChunkStatusTask,
    ) -> Self {
        let radius_of_empty =
            accumulated_radius_of(&accumulated_dependencies, target_status, ChunkStatus::Empty);
        let mut by_radius = vec![None; radius_of_empty + 1];
        by_radius[0] = Some(target_status.parent());
        let mut status = target_status.parent();
        while status != ChunkStatus::Empty {
            let radius = accumulated_radius_of(&accumulated_dependencies, target_status, status);
            for slot in by_radius.iter_mut().take(radius + 1) {
                if slot.is_none() {
                    *slot = Some(status);
                }
            }
            status = status.parent();
        }
        let by_radius = by_radius.into_iter().map(Option::unwrap).collect();
        ChunkStep {
            target_status,
            direct_dependencies,
            accumulated_dependencies,
            block_state_write_radius,
            task,
            by_radius,
        }
    }

    /// `getAccumulatedRadiusOf(ChunkStatus)` — the accumulated radius of a
    /// transitive dependency, or `0` for the target itself.
    pub fn get_accumulated_radius_of(&self, status: ChunkStatus) -> usize {
        if status == self.target_status {
            0
        } else {
            self.accumulated_dependencies.get_radius_of(status)
        }
    }

    /// `moonrise$getRequiredStatusAtRadius(int)` (the `ChunkSystemChunkStep`
    /// seam) — the minimum status the neighbour at distance `radius` must have
    /// reached for this step to run.
    pub fn required_status_at_radius(&self, radius: usize) -> ChunkStatus {
        self.by_radius[radius]
    }

    /// `targetStatus()`.
    pub fn target_status(&self) -> ChunkStatus {
        self.target_status
    }

    /// `directDependencies()`.
    pub fn direct_dependencies(&self) -> &ChunkDependencies {
        &self.direct_dependencies
    }

    /// `accumulatedDependencies()`.
    pub fn accumulated_dependencies(&self) -> &ChunkDependencies {
        &self.accumulated_dependencies
    }

    /// `blockStateWriteRadius()`.
    pub fn block_state_write_radius(&self) -> i32 {
        self.block_state_write_radius
    }

    /// `task()`.
    pub fn task(&self) -> ChunkStatusTask {
        self.task
    }
}

fn accumulated_radius_of(
    deps: &ChunkDependencies,
    target: ChunkStatus,
    status: ChunkStatus,
) -> usize {
    if status == target {
        0
    } else {
        deps.get_radius_of(status)
    }
}

/// `ChunkStep.Builder` — the per-step builder the pyramid's `step()` closures
/// configure. The `addRequirement` max-merge and the accumulated fold are ported
/// verbatim (they are pure functions of the builder calls).
pub struct ChunkStepBuilder {
    status: ChunkStatus,
    direct_dependencies_by_radius: Vec<ChunkStatus>,
    block_state_write_radius: i32,
    task: ChunkStatusTask,
    /// The parent step's identity and accumulated deps (`None` for `EMPTY`).
    parent: Option<ParentStep>,
}

/// The parent step's target + accumulated dependencies, cloned into the builder
/// so `buildAccumulatedDependencies` can fold them.
#[derive(Clone)]
struct ParentStep {
    target: ChunkStatus,
    accumulated: ChunkDependencies,
}

impl ChunkStepBuilder {
    /// The no-parent constructor — only valid for `EMPTY` (Java: "Not starting
    /// with the first status").
    pub fn new(status: ChunkStatus) -> Self {
        assert!(
            status.parent() == status,
            "Not starting with the first status: {status:?}"
        );
        ChunkStepBuilder {
            status,
            direct_dependencies_by_radius: Vec::new(),
            block_state_write_radius: -1,
            task: ChunkStatusTask::PassThrough,
            parent: None,
        }
    }

    /// The parent constructor — `parent` must be the immediately-previous
    /// status (Java: "Out of order status").
    pub fn with_parent(status: ChunkStatus, parent: &ChunkStep) -> Self {
        assert!(
            parent.target_status().index() as i64 == status.index() as i64 - 1,
            "Out of order status: {status:?}"
        );
        ChunkStepBuilder {
            status,
            direct_dependencies_by_radius: vec![parent.target_status()],
            block_state_write_radius: -1,
            task: ChunkStatusTask::PassThrough,
            parent: Some(ParentStep {
                target: parent.target_status(),
                accumulated: parent.accumulated_dependencies().clone(),
            }),
        }
    }

    /// `addRequirement(ChunkStatus, int)` — require `status` at `radius`,
    /// max-merging into the direct table (later status wins).
    pub fn add_requirement(mut self, status: ChunkStatus, radius: usize) -> Self {
        assert!(
            !status.is_or_after(self.status),
            "Status {status:?} can not be required by {:?}",
            self.status
        );
        let previous = self.direct_dependencies_by_radius.clone();
        let new_length = radius + 1;
        if new_length > previous.len() {
            self.direct_dependencies_by_radius = vec![status; new_length];
        }
        for (slot, prev) in self
            .direct_dependencies_by_radius
            .iter_mut()
            .zip(previous.iter())
            .take(new_length.min(previous.len()))
        {
            *slot = ChunkStatus::max(*prev, status);
        }
        self
    }

    /// `blockStateWriteRadius(int)` — the builder-level write radius the task
    /// bodies consult (the dispatch write radius is the separate scheduler
    /// config, #185).
    pub fn block_state_write_radius(mut self, radius: i32) -> Self {
        self.block_state_write_radius = radius;
        self
    }

    /// `setTask(ChunkStatusTask)`.
    pub fn set_task(mut self, task: ChunkStatusTask) -> Self {
        self.task = task;
        self
    }

    /// `build()`.
    pub fn build(self) -> ChunkStep {
        let direct = ChunkDependencies::new(self.direct_dependencies_by_radius.clone());
        let accumulated = ChunkDependencies::new(self.build_accumulated_dependencies());
        ChunkStep::new(
            self.status,
            direct,
            accumulated,
            self.block_state_write_radius,
            self.task,
        )
    }

    /// `getRadiusOfParent(ChunkStatus)` — the last direct-dependency radius at
    /// which the parent (or a later status) is required.
    fn get_radius_of_parent(&self) -> usize {
        let target = self.parent.as_ref().expect("parent exists").target;
        for i in (0..self.direct_dependencies_by_radius.len()).rev() {
            if self.direct_dependencies_by_radius[i].is_or_after(target) {
                return i;
            }
        }
        0
    }

    /// `buildAccumulatedDependencies()` — folds the parent's accumulated deps
    /// through `radiusOfParent` with the same max-merge.
    fn build_accumulated_dependencies(&self) -> Vec<ChunkStatus> {
        let Some(parent) = &self.parent else {
            return self.direct_dependencies_by_radius.clone();
        };
        let radius_of_parent = self.get_radius_of_parent();
        let parent_deps = parent.accumulated.as_list();
        let len =
            (radius_of_parent + parent_deps.len()).max(self.direct_dependencies_by_radius.len());
        let mut accumulated = Vec::with_capacity(len);
        for distance in 0..len {
            let distance_in_parent = distance as i64 - radius_of_parent as i64;
            if distance_in_parent < 0 || distance_in_parent >= parent_deps.len() as i64 {
                accumulated.push(self.direct_dependencies_by_radius[distance]);
            } else if distance >= self.direct_dependencies_by_radius.len() {
                accumulated.push(parent_deps[distance_in_parent as usize]);
            } else {
                accumulated.push(ChunkStatus::max(
                    self.direct_dependencies_by_radius[distance],
                    parent_deps[distance_in_parent as usize],
                ));
            }
        }
        accumulated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the generation chain up to NOISE (EMPTY → SS → SR → BIOMES →
    /// NOISE) exactly as the pyramid builder does.
    fn noise_step() -> ChunkStep {
        let empty = ChunkStepBuilder::new(ChunkStatus::Empty).build();
        let ss = ChunkStepBuilder::with_parent(ChunkStatus::StructureStarts, &empty)
            .set_task(ChunkStatusTask::GenerateStructureStarts)
            .build();
        let sr = ChunkStepBuilder::with_parent(ChunkStatus::StructureReferences, &ss)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .set_task(ChunkStatusTask::GenerateStructureReferences)
            .build();
        let biomes = ChunkStepBuilder::with_parent(ChunkStatus::Biomes, &sr)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .set_task(ChunkStatusTask::GenerateBiomes)
            .build();
        ChunkStepBuilder::with_parent(ChunkStatus::Noise, &biomes)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .add_requirement(ChunkStatus::Biomes, 1)
            .block_state_write_radius(0)
            .set_task(ChunkStatusTask::GenerateNoise)
            .build()
    }

    #[test]
    fn noised_direct_dependencies_merge_biomes_then_structure_starts() {
        // NOISE's builder calls: STRUCTURE_STARTS at 8, then BIOMES at 1.
        // The parent (BIOMES) is later than STRUCTURE_STARTS, so the radius-0/1
        // entries keep BIOMES and the SS requirement fills 2..=8.
        let step = noise_step();
        assert_eq!(step.target_status(), ChunkStatus::Noise);
        assert_eq!(step.block_state_write_radius(), 0);
        assert_eq!(step.task(), ChunkStatusTask::GenerateNoise);
        assert_eq!(
            step.direct_dependencies().as_list(),
            &[
                ChunkStatus::Biomes,
                ChunkStatus::Biomes,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
            ]
        );
    }

    #[test]
    fn noise_accumulated_dependencies_and_by_radius_are_exact() {
        let step = noise_step();
        // Accumulated: [BIOMES, BIOMES, SS x8] — radius 9.
        assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Empty), 9);
        assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Biomes), 1);
        // STRUCTURE_STARTS appears at distances 2..=9, so its accumulated
        // radius is the largest of those — 9.
        assert_eq!(
            step.get_accumulated_radius_of(ChunkStatus::StructureStarts),
            9
        );
        assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Noise), 0);
        // byRadius: BIOMES at distance 0 and 1, SS at 2..=9.
        assert_eq!(step.required_status_at_radius(0), ChunkStatus::Biomes);
        assert_eq!(step.required_status_at_radius(1), ChunkStatus::Biomes);
        assert_eq!(
            step.required_status_at_radius(2),
            ChunkStatus::StructureStarts
        );
        assert_eq!(
            step.required_status_at_radius(9),
            ChunkStatus::StructureStarts
        );
    }

    #[test]
    fn full_accumulated_dependencies_match_the_spec_table() {
        // The FULL step's accumulated table is the spec §3.2 12-entry DAG:
        // [SPAWN, INITIALIZE_LIGHT, CARVERS, BIOMES, SS x8] — radius 11.
        // Built through the real builder chain (as `ChunkPyramid` does).
        let step = full_step();
        assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Empty), 11);
        assert_eq!(
            step.accumulated_dependencies().as_list(),
            &[
                ChunkStatus::Spawn,
                ChunkStatus::InitializeLight,
                ChunkStatus::Carvers,
                ChunkStatus::Biomes,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
                ChunkStatus::StructureStarts,
            ]
        );
        assert_eq!(step.required_status_at_radius(0), ChunkStatus::Spawn);
        assert_eq!(
            step.required_status_at_radius(1),
            ChunkStatus::InitializeLight
        );
        assert_eq!(step.required_status_at_radius(3), ChunkStatus::Biomes);
        assert_eq!(
            step.required_status_at_radius(11),
            ChunkStatus::StructureStarts
        );
    }

    /// Rebuilds the generation pyramid's FULL step by chaining the builder
    /// exactly as `ChunkPyramid::generation()` does (duplicated here so the
    /// step test is independent of the pyramid's static).
    fn full_step() -> ChunkStep {
        let empty = ChunkStepBuilder::new(ChunkStatus::Empty).build();
        let ss = ChunkStepBuilder::with_parent(ChunkStatus::StructureStarts, &empty)
            .set_task(ChunkStatusTask::GenerateStructureStarts)
            .build();
        let sr = ChunkStepBuilder::with_parent(ChunkStatus::StructureReferences, &ss)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .set_task(ChunkStatusTask::GenerateStructureReferences)
            .build();
        let biomes = ChunkStepBuilder::with_parent(ChunkStatus::Biomes, &sr)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .set_task(ChunkStatusTask::GenerateBiomes)
            .build();
        let noise = ChunkStepBuilder::with_parent(ChunkStatus::Noise, &biomes)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .add_requirement(ChunkStatus::Biomes, 1)
            .block_state_write_radius(0)
            .set_task(ChunkStatusTask::GenerateNoise)
            .build();
        let surface = ChunkStepBuilder::with_parent(ChunkStatus::Surface, &noise)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .add_requirement(ChunkStatus::Biomes, 1)
            .block_state_write_radius(0)
            .set_task(ChunkStatusTask::GenerateSurface)
            .build();
        let carvers = ChunkStepBuilder::with_parent(ChunkStatus::Carvers, &surface)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .block_state_write_radius(0)
            .set_task(ChunkStatusTask::GenerateCarvers)
            .build();
        let features = ChunkStepBuilder::with_parent(ChunkStatus::Features, &carvers)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .add_requirement(ChunkStatus::Carvers, 1)
            .block_state_write_radius(1)
            .set_task(ChunkStatusTask::GenerateFeatures)
            .build();
        let init_light = ChunkStepBuilder::with_parent(ChunkStatus::InitializeLight, &features)
            .set_task(ChunkStatusTask::InitializeLight)
            .build();
        let light = ChunkStepBuilder::with_parent(ChunkStatus::Light, &init_light)
            .add_requirement(ChunkStatus::InitializeLight, 1)
            .set_task(ChunkStatusTask::Light)
            .build();
        let spawn = ChunkStepBuilder::with_parent(ChunkStatus::Spawn, &light)
            .add_requirement(ChunkStatus::Biomes, 1)
            .set_task(ChunkStatusTask::GenerateSpawn)
            .build();
        ChunkStepBuilder::with_parent(ChunkStatus::Full, &spawn)
            .set_task(ChunkStatusTask::Full)
            .build()
    }
}
