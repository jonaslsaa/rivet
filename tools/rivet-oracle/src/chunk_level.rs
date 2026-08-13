//! Faithful port of Paper 26.2's chunk-status accumulated-dependency
//! reachability: `ChunkLevel`, `ChunkStep`, `ChunkDependencies`, and the
//! `ChunkPyramid.GENERATION_PYRAMID` builder that produces `FULL_CHUNK_STEP`.
//!
//! The harness must answer *"which serialized status does a chunk at each
//! distance from the forced FULL center get?"* — the question the generated-
//! world scoreboard needs instead of the naive "one status per forced level"
//! assumption. Paper computes it from the pyramid's accumulated dependencies
//! (`ChunkStep.buildAccumulatedDependencies` + the Paper `byRadius` table), so
//! the Rust side reproduces that algorithm exactly and the oracle tests assert
//! the result against the live-Paper capture (`full-chunk-step` in the
//! composed-noise fixture). The translation mirrors `ChunkLevel.java`,
//! `ChunkStep.java`, `ChunkDependencies.java`, `ChunkPyramid.java`, and the
//! `ChunkStatus.getParent`/`max` helpers.

use rivet_world::chunk::status::ChunkStatus;

/// `ChunkStatus.getParent()` — Paper stores `parent == null ? this : parent`,
/// so EMPTY is its own parent and every other status's parent is the previous
/// ladder element.
fn parent(status: ChunkStatus) -> ChunkStatus {
    if status.index() == 0 {
        status
    } else {
        ChunkStatus::ALL[status.index() - 1]
    }
}

/// `ChunkStatus.max(a, b)` — the later (higher-index) status.
fn max(a: ChunkStatus, b: ChunkStatus) -> ChunkStatus {
    if a.index() > b.index() { a } else { b }
}

/// Paper's `ChunkDependencies`: a status-per-distance list plus the derived
/// radius-per-status table (`getRadiusOf` returns the last radius whose
/// dependency covers the status's index — this is why the reachability is
/// non-monotonic and not one-status-per-level).
#[derive(Clone, Debug)]
pub struct ChunkDependencies {
    dependency_by_radius: Vec<ChunkStatus>,
    radius_by_dependency: Vec<usize>,
}

impl ChunkDependencies {
    fn new(dependency_by_radius: Vec<ChunkStatus>) -> Self {
        let size = dependency_by_radius
            .first()
            .map(|first| first.index() + 1)
            .unwrap_or(0);
        let mut radius_by_dependency = vec![0usize; size];
        for (radius, dependency) in dependency_by_radius.iter().enumerate() {
            for slot in radius_by_dependency.iter_mut().take(dependency.index() + 1) {
                *slot = radius;
            }
        }
        Self {
            dependency_by_radius,
            radius_by_dependency,
        }
    }

    pub fn size(&self) -> usize {
        self.dependency_by_radius.len()
    }

    fn get_radius_of(&self, status: ChunkStatus) -> usize {
        // Mirrors ChunkDependencies.getRadiusOf's IllegalArgumentException for a
        // status outside the dependency range (never hit by the harness, which
        // only queries statuses within FULL_CHUNK_STEP's range).
        let index = status.index();
        assert!(
            index < self.radius_by_dependency.len(),
            "Requesting a ChunkStatus({status:?}) outside of dependency range({:?})",
            self.dependency_by_radius
        );
        self.radius_by_dependency[index]
    }

    pub fn get_radius(&self) -> usize {
        self.dependency_by_radius.len().saturating_sub(1)
    }

    fn get(&self, distance: usize) -> ChunkStatus {
        self.dependency_by_radius[distance]
    }
}

/// Paper's `ChunkStep` — the reachability surface. The harness answers "which
/// serialized status does a chunk at each distance from the forced FULL center
/// get" purely from the accumulated dependencies, exactly like Paper's
/// `byStatus` (`FULL_CHUNK_LEVEL + getAccumulatedRadiusOf(status)`) and
/// `getStatusAroundFullChunk` do. Paper additionally stores a `directDependencies`
/// list and a derived `byRadius` table for its chunk scheduler
/// (`moonrise$getRequiredStatusAtRadius`); both are dead weight for reachability
/// and are omitted here (the Rust port of `getAccumulatedRadiusOf` is what the
/// by-distance capture asserts against).
#[derive(Clone, Debug)]
pub struct ChunkStep {
    pub target_status: ChunkStatus,
    pub accumulated_dependencies: ChunkDependencies,
}

impl ChunkStep {
    fn new(target_status: ChunkStatus, accumulated_dependencies: ChunkDependencies) -> Self {
        Self {
            target_status,
            accumulated_dependencies,
        }
    }

    fn get_accumulated_radius_of(
        accumulated: &ChunkDependencies,
        target_status: ChunkStatus,
        status: ChunkStatus,
    ) -> usize {
        if status == target_status {
            0
        } else {
            accumulated.get_radius_of(status)
        }
    }
}

/// `ChunkLevel.byStatus`: the chunk level a chunk must be held at so its
/// serialization runs the given status's task — 33 (FULL) plus the accumulated
/// radius of the status from `FULL_CHUNK_STEP`.
pub fn by_status(full_chunk_step: &ChunkStep, status: ChunkStatus) -> i32 {
    ChunkLevelConsts::FULL_CHUNK_LEVEL
        + ChunkStep::get_accumulated_radius_of(
            &full_chunk_step.accumulated_dependencies,
            full_chunk_step.target_status,
            status,
        ) as i32
}

/// The `ChunkLevel` 26.2 constants (FULL_CHUNK_LEVEL = 33).
pub struct ChunkLevelConsts;

impl ChunkLevelConsts {
    pub const FULL_CHUNK_LEVEL: i32 = 33;
}

/// `ChunkLevel.getStatusAroundFullChunk(distance)`: the serialized status of a
/// chunk at `distance` from the forced FULL center. distance 0 is FULL; beyond
/// the accumulated radius it is EMPTY; otherwise the accumulated dependency at
/// that distance.
pub fn status_around_full_chunk(step: &ChunkStep, distance: usize) -> ChunkStatus {
    if distance == 0 {
        ChunkStatus::Full
    } else if distance > step.accumulated_dependencies.get_radius() {
        ChunkStatus::Empty
    } else {
        step.accumulated_dependencies.get(distance)
    }
}

/// `ChunkPyramid` with its `GENERATION_PYRAMID` builder — the exact `.step(...)`
/// requirements from `ChunkPyramid.java` (the tasks themselves are irrelevant
/// to reachability, so the port omits them).
#[derive(Clone, Debug)]
pub struct ChunkPyramid {
    pub steps: Vec<ChunkStep>,
}

impl ChunkPyramid {
    pub fn get_step_to(&self, status: ChunkStatus) -> &ChunkStep {
        &self.steps[status.index()]
    }

    /// `ChunkPyramid.GENERATION_PYRAMID` built exactly as Paper's builder does.
    pub fn generation_pyramid() -> Self {
        let mut b = PyramidBuilder::default();
        b.step(ChunkStatus::Empty, |s| s);
        b.step(ChunkStatus::StructureStarts, |s| s);
        b.step(ChunkStatus::StructureReferences, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
        });
        b.step(ChunkStatus::Biomes, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
        });
        b.step(ChunkStatus::Noise, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .add_requirement(ChunkStatus::Biomes, 1)
        });
        b.step(ChunkStatus::Surface, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .add_requirement(ChunkStatus::Biomes, 1)
        });
        b.step(ChunkStatus::Carvers, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
        });
        b.step(ChunkStatus::Features, |s| {
            s.add_requirement(ChunkStatus::StructureStarts, 8)
                .add_requirement(ChunkStatus::Carvers, 1)
        });
        b.step(ChunkStatus::InitializeLight, |s| s);
        b.step(ChunkStatus::Light, |s| {
            s.add_requirement(ChunkStatus::InitializeLight, 1)
        });
        b.step(ChunkStatus::Spawn, |s| {
            s.add_requirement(ChunkStatus::Biomes, 1)
        });
        b.step(ChunkStatus::Full, |s| s);
        Self { steps: b.steps }
    }
}

#[derive(Default)]
struct PyramidBuilder {
    steps: Vec<ChunkStep>,
}

impl PyramidBuilder {
    fn step<F: FnOnce(StepBuilder) -> StepBuilder>(&mut self, status: ChunkStatus, op: F) {
        let builder = if self.steps.is_empty() {
            StepBuilder::new_first(status)
        } else {
            StepBuilder::new_with_parent(status, self.steps.last().unwrap().clone())
        };
        self.steps.push(op(builder).build());
    }
}

/// `ChunkStep.Builder` — the accumulated-dependency folding.
struct StepBuilder {
    status: ChunkStatus,
    parent_step: Option<ChunkStep>,
    direct_dependencies_by_radius: Vec<ChunkStatus>,
}

impl StepBuilder {
    fn new_first(status: ChunkStatus) -> Self {
        debug_assert_eq!(
            parent(status),
            status,
            "first status must be its own parent (EMPTY)"
        );
        Self {
            status,
            parent_step: None,
            direct_dependencies_by_radius: Vec::new(),
        }
    }

    fn new_with_parent(status: ChunkStatus, parent_step: ChunkStep) -> Self {
        debug_assert_eq!(parent_step.target_status.index(), status.index() - 1);
        let direct_dependencies_by_radius = vec![parent_step.target_status];
        Self {
            status,
            parent_step: Some(parent_step),
            direct_dependencies_by_radius,
        }
    }

    fn add_requirement(mut self, status: ChunkStatus, radius: usize) -> Self {
        assert!(
            status.index() < self.status.index(),
            "Status {:?} can not be required by {:?}",
            status,
            self.status
        );
        let previous = std::mem::take(&mut self.direct_dependencies_by_radius);
        let new_length = radius + 1;
        let mut direct = if new_length > previous.len() {
            vec![status; new_length]
        } else {
            previous.clone()
        };
        for i in 0..new_length.min(previous.len()) {
            direct[i] = max(previous[i], status);
        }
        self.direct_dependencies_by_radius = direct;
        self
    }

    fn build(self) -> ChunkStep {
        let accumulated = self.build_accumulated_dependencies();
        ChunkStep::new(self.status, accumulated)
    }

    fn build_accumulated_dependencies(&self) -> ChunkDependencies {
        let Some(parent_step) = &self.parent_step else {
            return ChunkDependencies::new(self.direct_dependencies_by_radius.clone());
        };
        let radius_of_parent = self.get_radius_of_parent(parent_step.target_status);
        let parent_dependencies = &parent_step.accumulated_dependencies;
        let len = (radius_of_parent + parent_dependencies.size())
            .max(self.direct_dependencies_by_radius.len());
        let mut accumulated = Vec::with_capacity(len);
        for distance in 0..len {
            let distance_in_parent = distance as isize - radius_of_parent as isize;
            let entry = if distance_in_parent < 0
                || distance_in_parent as usize >= parent_dependencies.size()
            {
                self.direct_dependencies_by_radius[distance]
            } else if distance >= self.direct_dependencies_by_radius.len() {
                parent_dependencies.get(distance_in_parent as usize)
            } else {
                max(
                    self.direct_dependencies_by_radius[distance],
                    parent_dependencies.get(distance_in_parent as usize),
                )
            };
            accumulated.push(entry);
        }
        ChunkDependencies::new(accumulated)
    }

    fn get_radius_of_parent(&self, status: ChunkStatus) -> usize {
        for (i, dep) in self.direct_dependencies_by_radius.iter().enumerate().rev() {
            if dep.index() >= status.index() {
                return i;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_chunk_step() -> ChunkStep {
        ChunkPyramid::generation_pyramid()
            .get_step_to(ChunkStatus::Full)
            .clone()
    }

    /// The 26.2 constants Paper reports: FULL_CHUNK_LEVEL 33, accumulated
    /// radius 11, MAX_LEVEL 44 (captured live by ComposedNoiseProbe).
    #[test]
    fn full_chunk_step_constants_match_live_paper() {
        let step = full_chunk_step();
        assert_eq!(ChunkLevelConsts::FULL_CHUNK_LEVEL, 33);
        assert_eq!(step.accumulated_dependencies.get_radius(), 11);
        assert_eq!(
            ChunkLevelConsts::FULL_CHUNK_LEVEL + step.accumulated_dependencies.get_radius() as i32,
            44
        );
    }

    /// `getStatusAroundFullChunk` reproduces the live-Paper `by-distance` map
    /// captured in the composed-noise fixture: 0=full, 1=initialize_light,
    /// 2=carvers, 3=biomes, 4..=11=structure_starts.
    #[test]
    fn status_around_full_chunk_matches_live_paper_capture() {
        let step = full_chunk_step();
        let expected = [
            (0, "minecraft:full"),
            (1, "minecraft:initialize_light"),
            (2, "minecraft:carvers"),
            (3, "minecraft:biomes"),
            (4, "minecraft:structure_starts"),
            (5, "minecraft:structure_starts"),
            (6, "minecraft:structure_starts"),
            (7, "minecraft:structure_starts"),
            (8, "minecraft:structure_starts"),
            (9, "minecraft:structure_starts"),
            (10, "minecraft:structure_starts"),
            (11, "minecraft:structure_starts"),
        ];
        for (distance, name) in expected {
            assert_eq!(
                status_around_full_chunk(&step, distance).serialization_name(),
                name,
                "distance {distance}"
            );
        }
        assert_eq!(status_around_full_chunk(&step, 12), ChunkStatus::Empty);
    }

    /// `byStatus` exposes the non-trivial reachability — the "don't assume one
    /// status per forced level" numbers. NOISE/SURFACE/CARVERS all live at
    /// level 35 (radius 2), FEATURES/LIGHT at 34 (radius 1), SPAWN at 33
    /// (radius 0); STRUCTURE_STARTS reaches all the way out at 44.
    #[test]
    fn by_status_exposes_accumulated_reachability() {
        let step = full_chunk_step();
        let cases = [
            (ChunkStatus::StructureStarts, 44),
            (ChunkStatus::StructureReferences, 36),
            (ChunkStatus::Biomes, 36),
            (ChunkStatus::Noise, 35),
            (ChunkStatus::Surface, 35),
            (ChunkStatus::Carvers, 35),
            (ChunkStatus::Features, 34),
            (ChunkStatus::InitializeLight, 34),
            (ChunkStatus::Light, 33),
            (ChunkStatus::Spawn, 33),
            (ChunkStatus::Full, 33),
        ];
        for (status, level) in cases {
            assert_eq!(by_status(&step, status), level, "byStatus({:?})", status);
        }
    }
}
