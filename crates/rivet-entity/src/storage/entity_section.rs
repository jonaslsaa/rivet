//! Ports of `net.minecraft.world.level.entity.EntitySection` and
//! `net.minecraft.world.level.entity.EntitySectionStorage` (Paper 26.2).
//!
//! Java sources:
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/EntitySection.java`
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/EntitySectionStorage.java`

use crate::storage::entity_record::Aabb;
use crate::storage::visibility::Visibility;
use indexmap::IndexSet;
use rivet_registry::core::{ChunkPos, SectionPos};
use std::collections::{BTreeSet, HashMap};

/// `EntitySectionStorage.CHONKY_ENTITY_SEARCH_GRACE`.
pub const CHONKY_ENTITY_SEARCH_GRACE: i32 = 2;

/// `EntitySectionStorage.MAX_NON_CHONKY_ENTITY_SIZE`.
pub const MAX_NON_CHONKY_ENTITY_SIZE: i32 = 4;

/// Control flow returned by spatial section callbacks (`AbortableIterationConsumer.Continuation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationControl {
    Continue,
    Abort,
}

/// Port of `net.minecraft.world.level.entity.EntitySection`.
///
/// Stores ordered entity network IDs belonging to a single 16x16x16 section
/// alongside the section's current `Visibility`.
#[derive(Clone, Debug)]
pub struct EntitySection {
    entity_ids: IndexSet<i32>,
    chunk_status: Visibility,
}

impl EntitySection {
    /// Creates an empty section with the given initial `Visibility`.
    #[must_use]
    pub fn new(chunk_status: Visibility) -> Self {
        Self {
            entity_ids: IndexSet::new(),
            chunk_status,
        }
    }

    /// Adds `entity_id` to this section. Returns `true` if newly inserted.
    pub fn add(&mut self, entity_id: i32) -> bool {
        self.entity_ids.insert(entity_id)
    }

    /// Removes `entity_id` from this section while preserving iteration order.
    pub fn remove(&mut self, entity_id: i32) -> bool {
        self.entity_ids.shift_remove(&entity_id)
    }

    /// Returns `true` if the section holds no entities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entity_ids.is_empty()
    }

    /// Returns the number of entities in this section.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entity_ids.len()
    }

    /// Iterates entity IDs in deterministic insertion order.
    pub fn entity_ids(&self) -> impl Iterator<Item = i32> + '_ {
        self.entity_ids.iter().copied()
    }

    /// Collects all entity IDs in this section into `into` (Paper's `getEntities(List)`).
    pub fn collect_entity_ids(&self, into: &mut Vec<i32>) {
        into.extend(self.entity_ids.iter().copied());
    }

    /// Returns the section's current visibility (`EntitySection.getStatus`).
    #[must_use]
    pub const fn status(&self) -> Visibility {
        self.chunk_status
    }

    /// Updates the section's visibility and returns the previous status
    /// (`EntitySection.updateChunkStatus`).
    pub fn update_chunk_status(&mut self, chunk_status: Visibility) -> Visibility {
        let prev = self.chunk_status;
        self.chunk_status = chunk_status;
        prev
    }
}

/// Port of `net.minecraft.world.level.entity.EntitySectionStorage`.
///
/// Lazily allocates `EntitySection` buckets keyed by packed `SectionPos` long
/// (`i64`) and maintains a sorted `BTreeSet<i64>` of existing section keys
/// (matching Paper's `LongAVLTreeSet`).
#[derive(Clone, Debug, Default)]
pub struct EntitySectionStorage {
    sections: HashMap<i64, EntitySection>,
    section_ids: BTreeSet<i64>,
}

impl EntitySectionStorage {
    /// Creates an empty section storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sections: HashMap::new(),
            section_ids: BTreeSet::new(),
        }
    }

    /// Computes the packed `ChunkPos` key for a packed `SectionPos` key.
    #[must_use]
    pub fn chunk_key_from_section_key(section_key: i64) -> i64 {
        ChunkPos::pack_coords(SectionPos::x_of(section_key), SectionPos::z_of(section_key))
    }

    /// Returns an existing section or creates one with `initial_visibility`.
    pub fn get_or_create_section(
        &mut self,
        section_key: i64,
        initial_visibility: Visibility,
    ) -> &mut EntitySection {
        self.section_ids.insert(section_key);
        self.sections
            .entry(section_key)
            .or_insert_with(|| EntitySection::new(initial_visibility))
    }

    /// Returns a reference to the section at `section_key`, if it exists.
    #[must_use]
    pub fn get_section(&self, section_key: i64) -> Option<&EntitySection> {
        self.sections.get(&section_key)
    }

    /// Returns a mutable reference to the section at `section_key`, if it exists.
    pub fn get_section_mut(&mut self, section_key: i64) -> Option<&mut EntitySection> {
        self.sections.get_mut(&section_key)
    }

    /// Removes the section at `section_key` (`EntitySectionStorage.remove`).
    pub fn remove(&mut self, section_key: i64) -> Option<EntitySection> {
        self.section_ids.remove(&section_key);
        self.sections.remove(&section_key)
    }

    /// Returns the number of existing sections.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Returns existing packed section positions in `chunk_key`, sorted in
    /// Paper's `LongAVLTreeSet` packed-long order.
    #[must_use]
    pub fn existing_section_positions_in_chunk(&self, chunk_key: i64) -> Vec<i64> {
        let x = ChunkPos::get_x(chunk_key);
        let z = ChunkPos::get_z(chunk_key);
        let lowest = SectionPos::of(x, 0, z).as_long_self();
        let highest = SectionPos::of(x, -1, z).as_long_self();
        self.section_ids.range(lowest..=highest).copied().collect()
    }

    /// Returns references to all existing sections in `chunk_key`, in sorted
    /// packed-long order.
    #[must_use]
    pub fn existing_sections_in_chunk(&self, chunk_key: i64) -> Vec<(i64, &EntitySection)> {
        self.existing_section_positions_in_chunk(chunk_key)
            .into_iter()
            .filter_map(|key| self.sections.get(&key).map(|section| (key, section)))
            .collect()
    }

    /// Returns all packed chunk keys that currently have at least one existing
    /// section (`EntitySectionStorage.getAllChunksWithExistingSections`).
    #[must_use]
    pub fn all_chunks_with_existing_sections(&self) -> BTreeSet<i64> {
        self.section_ids
            .iter()
            .map(|&section_key| Self::chunk_key_from_section_key(section_key))
            .collect()
    }

    /// Iterates every accessible, non-empty section intersecting the Paper-expanded
    /// search bounds around `bounds` (`EntitySectionStorage.forEachAccessibleNonEmptySection`).
    ///
    /// Bounds expansion matches Paper 26.2 verbatim:
    /// - `xMin = posToSectionCoord(minX - 2.0)`, `xMax = posToSectionCoord(maxX + 2.0)`
    /// - `yMin = posToSectionCoord(minY - 4.0)`, `yMax = posToSectionCoord(maxY + 0.0)`
    /// - `zMin = posToSectionCoord(minZ - 2.0)`, `zMax = posToSectionCoord(maxZ + 2.0)`
    pub fn for_each_accessible_non_empty_section<F>(&self, bounds: Aabb, mut f: F)
    where
        F: FnMut(i64, &EntitySection) -> IterationControl,
    {
        let grace = f64::from(CHONKY_ENTITY_SEARCH_GRACE);
        let max_size = f64::from(MAX_NON_CHONKY_ENTITY_SIZE);
        let x_min = SectionPos::pos_to_section_coord(bounds.min_x - grace);
        let y_min = SectionPos::pos_to_section_coord(bounds.min_y - max_size);
        let z_min = SectionPos::pos_to_section_coord(bounds.min_z - grace);
        let x_max = SectionPos::pos_to_section_coord(bounds.max_x + grace);
        let y_max = SectionPos::pos_to_section_coord(bounds.max_y + 0.0);
        let z_max = SectionPos::pos_to_section_coord(bounds.max_z + grace);

        for x in x_min..=x_max {
            let lowest = SectionPos::of(x, 0, 0).as_long_self();
            let highest = SectionPos::of(x, -1, -1).as_long_self();
            for &section_key in self.section_ids.range(lowest..=highest) {
                let y = SectionPos::y_of(section_key);
                let z = SectionPos::z_of(section_key);
                if y >= y_min
                    && y <= y_max
                    && z >= z_min
                    && z <= z_max
                    && let Some(section) = self.sections.get(&section_key)
                    && !section.is_empty()
                    && section.status().is_accessible()
                    && f(section_key, section) == IterationControl::Abort
                {
                    return;
                }
            }
        }
    }
}
