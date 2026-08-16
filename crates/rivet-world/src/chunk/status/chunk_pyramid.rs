//! Port of `net.minecraft.world.level.chunk.status.ChunkPyramid` (MC 26.2) —
//! the ordered list of `ChunkStep`s that makes up the generation/loading DAG,
//! plus the pure access-radius tables.
//!
//! Java: `ChunkPyramid.java` in `working/Paper`. `GENERATION_PYRAMID` and
//! `LOADING_PYRAMID` are the two static pyramids; `getStepTo(status)` indexes
//! the steps by status index. The value-layer pyramid is the full 12-rung
//! ladder — the accumulated tables of the later steps are pure functions of the
//! builder calls and the access-radius table (§3.5 of `chunk-pipeline-spec.md`)
//! needs all 12 entries. The worldgen steps through CARVERS and the
//! INITIALIZE_LIGHT/LIGHT steps are *wired*: the executor seam
//! (`world_gen_context.rs`) runs the CARVERS and FEATURES task bodies at their
//! canonical rungs, wires SPAWN through the caller's spawn seam, and refuses
//! borrowed FULL because that rung transfers ownership (RivetTodo #185).
//!
//! The access radii are the `ChunkTaskScheduler.getAccessRadius0` recursion
//! (the `ca.spottedleaf.moonrise...scheduling` cluster) ported as a pure
//! function of the two pyramids — the *scheduler dispatch* itself defers with
//! #185; the table is exact parity data.

use std::sync::LazyLock;

use crate::chunk::status::ChunkStatus;
use crate::chunk::status::chunk_status_task::ChunkStatusTask;
use crate::chunk::status::chunk_step::{ChunkStep, ChunkStepBuilder};

/// `ChunkPyramid.GENERATION_PYRAMID` — the 26.2 generation DAG.
pub static GENERATION_PYRAMID: LazyLock<ChunkPyramid> = LazyLock::new(build_generation_pyramid);
/// `ChunkPyramid.LOADING_PYRAMID` — the all-zero-radius loading DAG.
pub static LOADING_PYRAMID: LazyLock<ChunkPyramid> = LazyLock::new(build_loading_pyramid);

/// `net.minecraft.world.level.chunk.status.ChunkPyramid` — the ordered steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkPyramid {
    steps: Vec<ChunkStep>,
}

impl ChunkPyramid {
    /// `new Builder().step(...)...build()`. Crate-internal: the public value
    /// layer is the two static pyramids; the builder exists to construct them
    /// and for the module's malformed-pyramid tests.
    pub(crate) fn builder() -> ChunkPyramidBuilder {
        ChunkPyramidBuilder::new()
    }

    /// `getStepTo(ChunkStatus)` — the step targeting `status`.
    pub fn get_step_to(&self, status: ChunkStatus) -> &ChunkStep {
        &self.steps[status.index()]
    }

    /// The steps, in generation order.
    pub fn steps(&self) -> &[ChunkStep] {
        &self.steps
    }

    /// `ChunkTaskScheduler.getAccessRadius(ChunkStatus)` — the maximum
    /// neighbour distance a chunk operation for `status` can read or write
    /// (spec §3.5). Combined max of the GENERATION and LOADING pyramids,
    /// computed once into [`ACCESS_RADIUS_TABLE`] (Java's static-final array).
    pub fn access_radius(status: ChunkStatus) -> i32 {
        ACCESS_RADIUS_TABLE[status.index()]
    }

    /// `ChunkTaskScheduler.getMaxAccessRadius()` — `11` for the full status.
    pub fn max_access_radius() -> i32 {
        ACCESS_RADIUS_TABLE[ChunkStatus::Full.index()]
    }
}

/// The combined `ACCESS_RADIUS_TABLE` — Java's static-final `getAccessRadius`
/// lookup, computed once from the two pyramids.
pub static ACCESS_RADIUS_TABLE: LazyLock<[i32; 12]> =
    LazyLock::new(|| access_radius_table(&GENERATION_PYRAMID, &LOADING_PYRAMID));

/// `ChunkPyramid.Builder` — the chainable step builder.
pub(crate) struct ChunkPyramidBuilder {
    steps: Vec<ChunkStep>,
}

impl ChunkPyramidBuilder {
    fn new() -> Self {
        ChunkPyramidBuilder { steps: Vec::new() }
    }

    /// `step(ChunkStatus, UnaryOperator<ChunkStep.Builder>)` — appends a step
    /// built from the previous one (or fresh for `EMPTY`).
    pub fn step(
        mut self,
        status: ChunkStatus,
        operator: impl FnOnce(ChunkStepBuilder) -> ChunkStepBuilder,
    ) -> Self {
        let builder = if self.steps.is_empty() {
            ChunkStepBuilder::new(status)
        } else {
            ChunkStepBuilder::with_parent(status, self.steps.last().expect("non-empty"))
        };
        self.steps.push(operator(builder).build());
        self
    }

    /// `build()`.
    pub fn build(self) -> ChunkPyramid {
        ChunkPyramid { steps: self.steps }
    }
}

/// `ChunkPyramid.GENERATION_PYRAMID` — the builder calls ported verbatim from
/// the Java static initializer. The tasks through FEATURES are wired in the
/// executor context, SPAWN is caller-seam dependent, and borrowed FULL is
/// intentionally refused because it transfers ownership (RivetTodo #185).
fn build_generation_pyramid() -> ChunkPyramid {
    ChunkPyramid::builder()
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
                .block_state_write_radius(0)
                .set_task(ChunkStatusTask::GenerateNoise)
        })
        .step(ChunkStatus::Surface, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .add_requirement(ChunkStatus::Biomes, 1)
                .block_state_write_radius(0)
                .set_task(ChunkStatusTask::GenerateSurface)
        })
        .step(ChunkStatus::Carvers, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .block_state_write_radius(0)
                .set_task(ChunkStatusTask::GenerateCarvers)
        })
        .step(ChunkStatus::Features, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .add_requirement(ChunkStatus::Carvers, 1)
                .block_state_write_radius(1)
                .set_task(ChunkStatusTask::GenerateFeatures)
        })
        .step(ChunkStatus::InitializeLight, |s| {
            s.set_task(ChunkStatusTask::InitializeLight)
        })
        .step(ChunkStatus::Light, |s| {
            s.add_requirement(ChunkStatus::InitializeLight, 1)
                .set_task(ChunkStatusTask::Light)
        })
        .step(ChunkStatus::Spawn, |s| {
            s.add_requirement(ChunkStatus::Biomes, 1)
                .set_task(ChunkStatusTask::GenerateSpawn)
        })
        .step(ChunkStatus::Full, |s| s.set_task(ChunkStatusTask::Full))
        .build()
}

/// `ChunkPyramid.LOADING_PYRAMID` — the all-zero-radius DAG.
fn build_loading_pyramid() -> ChunkPyramid {
    ChunkPyramid::builder()
        .step(ChunkStatus::Empty, |s| s)
        .step(ChunkStatus::StructureStarts, |s| {
            s.set_task(ChunkStatusTask::LoadStructureStarts)
        })
        .step(ChunkStatus::StructureReferences, |s| s)
        .step(ChunkStatus::Biomes, |s| s)
        .step(ChunkStatus::Noise, |s| s)
        .step(ChunkStatus::Surface, |s| s)
        .step(ChunkStatus::Carvers, |s| s)
        .step(ChunkStatus::Features, |s| s)
        .step(ChunkStatus::InitializeLight, |s| {
            s.set_task(ChunkStatusTask::InitializeLight)
        })
        .step(ChunkStatus::Light, |s| s.set_task(ChunkStatusTask::Light))
        .step(ChunkStatus::Spawn, |s| s)
        .step(ChunkStatus::Full, |s| s.set_task(ChunkStatusTask::Full))
        .build()
}

/// `getAccessRadius0(toStatus, pyramid)` — the recursive max over the `byRadius`
/// table, reading the already-computed *combined* table (Java's
/// `ACCESS_RADIUS_TABLE`).
fn get_access_radius0(pyramid: &ChunkPyramid, status: ChunkStatus, table: &[i32; 12]) -> i32 {
    if status == ChunkStatus::Empty {
        return 0;
    }
    let step = pyramid.get_step_to(status);
    let radius = step.get_accumulated_radius_of(ChunkStatus::Empty);
    let mut max_range = radius as i32;
    for dist in 0..=radius {
        let required = step.required_status_at_radius(dist);
        max_range = max_range.max(dist as i32 + table[required.index()]);
    }
    max_range
}

/// The full `ACCESS_RADIUS_TABLE` — computed in status order exactly like the
/// Java static block (`EMPTY = 0`, then each status recurses through the
/// already-computed entries), combined as `max(LOADING, GENERATION)`.
pub fn access_radius_table(generation: &ChunkPyramid, loading: &ChunkPyramid) -> [i32; 12] {
    let mut table = [0i32; 12];
    for i in 1..12 {
        let status = ChunkStatus::ALL[i];
        let generation_radius = get_access_radius0(generation, status, &table);
        let loading_radius = get_access_radius0(loading, status, &table);
        table[i] = generation_radius.max(loading_radius);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec §3.5 access radii, in status order: EMPTY 0, SS 0, SR 8, BIOMES 8,
    /// NOISE 9, SURFACE 9, CARVERS 9, FEATURES 10, INIT_LIGHT 10, LIGHT 11,
    /// SPAWN 11, FULL 11.
    const EXPECTED_ACCESS_RADII: [i32; 12] = [0, 0, 8, 8, 9, 9, 9, 10, 10, 11, 11, 11];

    #[test]
    fn generation_pyramid_steps_are_in_exact_status_order() {
        let steps = GENERATION_PYRAMID.steps();
        assert_eq!(steps.len(), 12);
        for (i, status) in ChunkStatus::ALL.iter().enumerate() {
            assert_eq!(steps[i].target_status(), *status);
        }
    }

    #[test]
    fn generation_step_tasks_match_the_java_builder_calls() {
        let steps = GENERATION_PYRAMID.steps();
        let expected = [
            ChunkStatusTask::PassThrough,
            ChunkStatusTask::GenerateStructureStarts,
            ChunkStatusTask::GenerateStructureReferences,
            ChunkStatusTask::GenerateBiomes,
            ChunkStatusTask::GenerateNoise,
            ChunkStatusTask::GenerateSurface,
            ChunkStatusTask::GenerateCarvers,
            ChunkStatusTask::GenerateFeatures,
            ChunkStatusTask::InitializeLight,
            ChunkStatusTask::Light,
            ChunkStatusTask::GenerateSpawn,
            ChunkStatusTask::Full,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(steps[i].task(), *want, "step {}", i);
        }
    }

    #[test]
    fn generation_block_state_write_radii_match_java() {
        // NOISE/SURFACE/CARVERS = 0, FEATURES = 1, everything else -1.
        for (i, status) in ChunkStatus::ALL.iter().enumerate() {
            let want = match status {
                ChunkStatus::Noise | ChunkStatus::Surface | ChunkStatus::Carvers => 0,
                ChunkStatus::Features => 1,
                _ => -1,
            };
            assert_eq!(
                GENERATION_PYRAMID
                    .get_step_to(*status)
                    .block_state_write_radius(),
                want,
                "step {}",
                i
            );
        }
    }

    #[test]
    fn noise_accumulated_radius_and_by_radius_are_exact() {
        let noise = GENERATION_PYRAMID.get_step_to(ChunkStatus::Noise);
        assert_eq!(noise.get_accumulated_radius_of(ChunkStatus::Empty), 9);
        assert_eq!(noise.required_status_at_radius(0), ChunkStatus::Biomes);
        assert_eq!(noise.required_status_at_radius(1), ChunkStatus::Biomes);
        assert_eq!(
            noise.required_status_at_radius(2),
            ChunkStatus::StructureStarts
        );
        assert_eq!(
            noise.required_status_at_radius(9),
            ChunkStatus::StructureStarts
        );
        assert_eq!(
            noise.accumulated_dependencies().as_list(),
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
                ChunkStatus::StructureStarts,
            ]
        );
    }

    #[test]
    fn full_accumulated_dependencies_match_the_spec_table() {
        let full = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);
        assert_eq!(full.get_accumulated_radius_of(ChunkStatus::Empty), 11);
        assert_eq!(
            full.accumulated_dependencies().as_list(),
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
    }

    #[test]
    fn full_step_accumulated_index_zero_is_spawn_not_full() {
        // Arbitration for the FULL step's dependency representation (pipeline
        // PR #591): FULL's builder is parent=SPAWN with no addRequirement calls,
        // so direct = [SPAWN]. radiusOfParent(SPAWN) = 0, and the accumulated
        // fold gives accumulated[0] = max(direct[0]=SPAWN, SPAWN.accum[0]=LIGHT)
        // = SPAWN (index 10 > 9). byRadius[0] = targetStatus.parent() = SPAWN.
        // The target status is never its own dependency in either the
        // accumulated list or the byRadius table.
        let full = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);
        assert_eq!(full.accumulated_dependencies().get(0), ChunkStatus::Spawn);
        assert_eq!(full.direct_dependencies().get(0), ChunkStatus::Spawn);
        assert_eq!(full.required_status_at_radius(0), ChunkStatus::Spawn);
        assert!(
            !full
                .accumulated_dependencies()
                .as_list()
                .contains(&ChunkStatus::Full)
        );
    }

    #[test]
    fn loading_pyramid_is_all_zero_radius() {
        for step in LOADING_PYRAMID.steps() {
            assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Empty), 0);
            assert_eq!(
                step.required_status_at_radius(0),
                step.target_status().parent()
            );
        }
    }

    #[test]
    fn access_radius_table_matches_the_spec() {
        for (i, status) in ChunkStatus::ALL.iter().enumerate() {
            assert_eq!(
                ChunkPyramid::access_radius(*status),
                EXPECTED_ACCESS_RADII[i],
                "status {status:?}"
            );
        }
        assert_eq!(ChunkPyramid::max_access_radius(), 11);
    }

    #[test]
    fn access_radius_table_is_pure_over_both_pyramids() {
        // The two-argument form agrees with the convenience form (which uses
        // the statics), proving the table is a pure function of the pyramids.
        let table = access_radius_table(&GENERATION_PYRAMID, &LOADING_PYRAMID);
        assert_eq!(table, EXPECTED_ACCESS_RADII);
    }
}
