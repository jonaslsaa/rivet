//! Persisted chunk-status values from Paper 26.2.

use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types, WORLDGEN_HEIGHTMAPS};
use rivet_registry::identifier::Identifier;

/// `ChunkStatus.ChunkType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkType {
    ProtoChunk,
    LevelChunk,
}

/// The 26.2 built-in `ChunkStatus` registry ladder, in generation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChunkStatus {
    Empty,
    StructureStarts,
    StructureReferences,
    Biomes,
    Noise,
    Surface,
    Carvers,
    Features,
    InitializeLight,
    Light,
    Spawn,
    Full,
}

impl ChunkStatus {
    pub const ALL: [Self; 12] = [
        Self::Empty,
        Self::StructureStarts,
        Self::StructureReferences,
        Self::Biomes,
        Self::Noise,
        Self::Surface,
        Self::Carvers,
        Self::Features,
        Self::InitializeLight,
        Self::Light,
        Self::Spawn,
        Self::Full,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn serialization_name(self) -> &'static str {
        match self {
            Self::Empty => "minecraft:empty",
            Self::StructureStarts => "minecraft:structure_starts",
            Self::StructureReferences => "minecraft:structure_references",
            Self::Biomes => "minecraft:biomes",
            Self::Noise => "minecraft:noise",
            Self::Surface => "minecraft:surface",
            Self::Carvers => "minecraft:carvers",
            Self::Features => "minecraft:features",
            Self::InitializeLight => "minecraft:initialize_light",
            Self::Light => "minecraft:light",
            Self::Spawn => "minecraft:spawn",
            Self::Full => "minecraft:full",
        }
    }

    /// Decode a built-in identifier through the canonical registry-codec
    /// parser, including its default-namespace behavior.
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        let identifier = Identifier::by_separator_result(identifier, ':').ok()?;
        if identifier.namespace() != "minecraft" {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|status| status.serialization_name()["minecraft:".len()..] == *identifier.path())
    }

    pub const fn chunk_type(self) -> ChunkType {
        if matches!(self, Self::Full) {
            ChunkType::LevelChunk
        } else {
            ChunkType::ProtoChunk
        }
    }

    pub const fn heightmaps_after(self) -> &'static [Types] {
        if self.index() < Self::Carvers.index() {
            &WORLDGEN_HEIGHTMAPS
        } else {
            &FINAL_HEIGHTMAPS
        }
    }

    pub const fn is_or_after(self, other: Self) -> bool {
        self.index() >= other.index()
    }

    /// `ChunkStatus.isAfter` — strict.
    pub const fn is_after(self, other: Self) -> bool {
        self.index() > other.index()
    }

    /// `ChunkStatus.isBefore` — strict.
    pub const fn is_before(self, other: Self) -> bool {
        self.index() < other.index()
    }

    /// `ChunkStatus.isOrBefore(ChunkStatus)` — `this.getIndex() <=
    /// other.getIndex()` (the pipeline ring/status contract `WorldGenRegion`
    /// uses to bound a requested status by the step's per-ring dependency).
    pub const fn is_or_before(self, other: Self) -> bool {
        self.index() <= other.index()
    }

    /// `ChunkStatus.getParent()` — the previous rung of the ladder; `EMPTY`
    /// is its own parent (Java stores `this` when the parent is null). Derived
    /// from `ALL`/index order so there is a single source of truth for the
    /// chain (a transposed hand-written match would silently corrupt
    /// `byRadius`/`required_status_at_radius`).
    pub const fn parent(self) -> Self {
        if self.index() == 0 {
            Self::Empty
        } else {
            Self::ALL[self.index() - 1]
        }
    }

    /// `ChunkStatus.max(a, b)` — the later status (higher index). Java uses
    /// strict `isAfter`, so `max(a, a)` falls through to `b` (equal values).
    pub const fn max(a: Self, b: Self) -> Self {
        if a.is_after(b) { a } else { b }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_26_2_ladder_round_trips_with_default_namespace() {
        for (index, status) in ChunkStatus::ALL.into_iter().enumerate() {
            assert_eq!(status.index(), index);
            assert_eq!(
                ChunkStatus::from_identifier(status.serialization_name()),
                Some(status)
            );
            assert_eq!(
                ChunkStatus::from_identifier(&status.serialization_name()["minecraft:".len()..]),
                Some(status)
            );
            assert_eq!(
                ChunkStatus::from_identifier(&format!(
                    ":{}",
                    &status.serialization_name()["minecraft:".len()..]
                )),
                Some(status)
            );
        }
        assert_eq!(ChunkStatus::from_identifier(""), None);
        assert_eq!(ChunkStatus::from_identifier("minecraft:unknown"), None);
        assert_eq!(ChunkStatus::from_identifier("other:full"), None);
    }

    #[test]
    fn chunk_type_comparison_and_heightmap_boundary_are_exact() {
        for status in ChunkStatus::ALL {
            assert_eq!(
                status.chunk_type(),
                if status == ChunkStatus::Full {
                    ChunkType::LevelChunk
                } else {
                    ChunkType::ProtoChunk
                }
            );
            assert_eq!(status.is_or_after(ChunkStatus::Light), status.index() >= 9);
            assert_eq!(status.is_or_before(ChunkStatus::Light), status.index() <= 9);
            assert_eq!(status.is_before(ChunkStatus::Light), status.index() < 9);
            assert_eq!(status.is_after(ChunkStatus::Light), status.index() > 9);
        }
        // The exact ladder bounds: `is_or_before` is inclusive, `is_or_after`
        // is inclusive, and the strict `is_before`/`is_after` are exclusive.
        assert!(ChunkStatus::Empty.is_or_before(ChunkStatus::Full));
        assert!(!ChunkStatus::Full.is_or_before(ChunkStatus::Features));
        assert!(!ChunkStatus::Light.is_before(ChunkStatus::Light));
        assert!(!ChunkStatus::Light.is_after(ChunkStatus::Light));
        assert_eq!(
            ChunkStatus::Features.serialization_name(),
            "minecraft:features"
        );
        assert_eq!(
            ChunkStatus::Surface.heightmaps_after(),
            &WORLDGEN_HEIGHTMAPS
        );
        assert_eq!(ChunkStatus::Carvers.heightmaps_after(), &FINAL_HEIGHTMAPS);
    }
}
