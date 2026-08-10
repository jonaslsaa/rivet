//! Persisted chunk-status values from Paper 26.2.

use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types, WORLDGEN_HEIGHTMAPS};

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

    /// Decode a built-in identifier. An omitted namespace defaults to
    /// `minecraft`, as `Identifier.tryParse` does for registry codecs.
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        let (namespace, path) = identifier
            .split_once(':')
            .map_or(("minecraft", identifier), |(namespace, path)| {
                (namespace, path)
            });
        if namespace != "minecraft" || path.is_empty() || path.contains(':') {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|status| status.serialization_name()["minecraft:".len()..] == *path)
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
        }
        assert_eq!(
            ChunkStatus::Surface.heightmaps_after(),
            &WORLDGEN_HEIGHTMAPS
        );
        assert_eq!(ChunkStatus::Carvers.heightmaps_after(), &FINAL_HEIGHTMAPS);
    }
}
