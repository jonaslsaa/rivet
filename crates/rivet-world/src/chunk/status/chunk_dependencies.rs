//! Port of `net.minecraft.world.level.chunk.status.ChunkDependencies` (MC 26.2)
//! — the radius-keyed dependency table a `ChunkStep` carries.
//!
//! Java: `ChunkDependencies.java` in `working/Paper`. A step's dependencies are
//! stored as a `dependencyByRadius` list (`dependencyByRadius[d]` = the status
//! the neighbour at Chebyshev distance `d` must have reached), which the
//! constructor folds into the inverse `radiusByDependency` table
//! (`radiusByDependency[status]` = the distance at which that status is
//! required). The builder's max-merge fills each distance slot with the *latest*
//! status required there, so the list is not monotonic in status — NOISE
//! requires BIOMES at radius 0..=1 but only STRUCTURE_STARTS at 2..=8. The
//! constructor's prefix-fill is what makes `radiusByDependency` correct: it
//! records the last distance at which each status (or a later one) is required.

use crate::chunk::status::ChunkStatus;

/// `net.minecraft.world.level.chunk.status.ChunkDependencies`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkDependencies {
    /// `dependencyByRadius` — the immutable list Java builds from the builder's
    /// radius array.
    dependency_by_radius: Vec<ChunkStatus>,
    /// `radiusByDependency` — `radiusByDependency[status.index()]` = the
    /// largest distance at which that status (or a later one) is required.
    radius_by_dependency: Vec<usize>,
}

impl ChunkDependencies {
    /// The Java constructor: `size = list.isEmpty() ? 0 :
    /// list.getFirst().getIndex() + 1`, then the prefix fill
    /// `radiusByDependency[0..=dep.index] = radius` for every radius. A
    /// dependency whose index is beyond `size` overflows the Java array
    /// (`ArrayIndexOutOfBoundsException`); the port panics with the same
    /// contract.
    pub fn new(dependency_by_radius: Vec<ChunkStatus>) -> Self {
        let size = match dependency_by_radius.first() {
            Some(first) => first.index() + 1,
            None => 0,
        };
        let mut radius_by_dependency = vec![0usize; size];
        for (radius, dependency) in dependency_by_radius.iter().enumerate() {
            let index = dependency.index();
            assert!(
                index < radius_by_dependency.len(),
                "dependency {dependency:?} at radius {radius} is outside the dependency range \
                 (size {}) — like Java's ArrayIndexOutOfBoundsException",
                radius_by_dependency.len()
            );
            for entry in radius_by_dependency.iter_mut().take(index + 1) {
                *entry = radius;
            }
        }
        ChunkDependencies {
            dependency_by_radius,
            radius_by_dependency,
        }
    }

    /// `asList()` — the `@VisibleForTesting` dependency-by-radius list.
    pub fn as_list(&self) -> &[ChunkStatus] {
        &self.dependency_by_radius
    }

    /// `size()`.
    ///
    /// Mirrors Java's accessor; the deferred scheduler (#185) consumes it when
    /// reading `ChunkDependencies.size()` for the pyramid's radius bounds.
    pub fn size(&self) -> usize {
        self.dependency_by_radius.len()
    }

    /// `getRadiusOf(ChunkStatus)` — the distance at which `status` is required;
    /// throws `IllegalArgumentException` when the status is outside the
    /// dependency range (Rust: `panic!`, matching Java's crash).
    pub fn get_radius_of(&self, status: ChunkStatus) -> usize {
        let index = status.index();
        if index >= self.radius_by_dependency.len() {
            panic!(
                "Requesting a ChunkStatus({status:?}) outside of dependency range({:?})",
                self.dependency_by_radius
            );
        }
        self.radius_by_dependency[index]
    }

    /// `getRadius()` — `Math.max(0, size() - 1)`.
    ///
    /// Mirrors Java's accessor; the deferred scheduler (#185) reads it as the
    /// step's accumulated radius bound.
    pub fn get_radius(&self) -> usize {
        self.dependency_by_radius.len().saturating_sub(1)
    }

    /// `get(int distance)` — the required status at that distance.
    ///
    /// Mirrors Java's accessor; the deferred scheduler (#185) reads it from the
    /// FULL step's accumulated dependencies for the status-by-radius dispatch.
    pub fn get(&self, distance: usize) -> ChunkStatus {
        self.dependency_by_radius[distance]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_of_dependency_prefix_fill_matches_java() {
        // `NOISE`'s direct deps: [BIOMES, BIOMES, STRUCTURE_STARTS x7].
        let deps = ChunkDependencies::new(vec![
            ChunkStatus::Biomes,
            ChunkStatus::Biomes,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
        ]);
        assert_eq!(deps.size(), 9);
        assert_eq!(deps.get_radius(), 8);
        // The prefix fill records, per status index, the LAST distance at which
        // a dependency at-or-after that status appears. BIOMES (index 3) is the
        // dep at distances 0..=1; STRUCTURE_STARTS (index 1) at 2..=8. So EMPTY
        // and STRUCTURE_STARTS resolve to 8, STRUCTURE_REFERENCES (2) to 1 (the
        // BIOMES entries at distance 1 are >= it), BIOMES to 1.
        assert_eq!(deps.get_radius_of(ChunkStatus::Empty), 8);
        assert_eq!(deps.get_radius_of(ChunkStatus::StructureStarts), 8);
        assert_eq!(deps.get_radius_of(ChunkStatus::StructureReferences), 1);
        assert_eq!(deps.get_radius_of(ChunkStatus::Biomes), 1);
        assert_eq!(deps.get(0), ChunkStatus::Biomes);
        assert_eq!(deps.get(1), ChunkStatus::Biomes);
        assert_eq!(deps.get(8), ChunkStatus::StructureStarts);
    }

    #[test]
    fn empty_dependencies_have_zero_size_and_radius() {
        let deps = ChunkDependencies::new(vec![]);
        assert_eq!(deps.size(), 0);
        assert_eq!(deps.get_radius(), 0);
        assert!(deps.as_list().is_empty());
    }

    #[test]
    #[should_panic(expected = "outside of dependency range")]
    fn get_radius_of_out_of_range_status_panics_like_java() {
        let deps = ChunkDependencies::new(vec![ChunkStatus::StructureStarts]);
        // NOISE (index 4) is beyond the dependency range of a step whose only
        // dep is STRUCTURE_STARTS — Java throws IllegalArgumentException.
        deps.get_radius_of(ChunkStatus::Noise);
    }
}
