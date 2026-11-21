//! Port of `net.minecraft.world.level.entity.EntityLookup` (Paper 26.2).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/EntityLookup.java`
//!
//! Maintains insertion-ordered lookup by integer network entity ID (mirroring
//! fastutil's `Int2ObjectLinkedOpenHashMap`) alongside a UUID index.

use indexmap::IndexMap;
use rivet_util::mth::Uuid;
use std::collections::HashMap;

/// Insertion-ordered entity lookup by network ID and UUID.
#[derive(Clone, Debug)]
pub struct EntityLookup {
    /// Insertion-ordered map from network entity ID to UUID.
    by_id: IndexMap<i32, Uuid>,
    /// Map from UUID to network entity ID.
    by_uuid: HashMap<Uuid, i32>,
}

impl Default for EntityLookup {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityLookup {
    /// Creates an empty `EntityLookup`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_id: IndexMap::new(),
            by_uuid: HashMap::new(),
        }
    }

    /// Registers `(entity_id, uuid)`.
    ///
    /// Matches Paper `EntityLookup.add`: returns `false` without mutating state
    /// if `uuid` (or `entity_id`) is already present.
    pub fn add(&mut self, entity_id: i32, uuid: Uuid) -> bool {
        if self.by_uuid.contains_key(&uuid) || self.by_id.contains_key(&entity_id) {
            return false;
        }
        self.by_uuid.insert(uuid, entity_id);
        self.by_id.insert(entity_id, uuid);
        true
    }

    /// Removes `(entity_id, uuid)` from both indexes.
    ///
    /// Returns `true` if the entry was present.
    pub fn remove(&mut self, entity_id: i32, uuid: Uuid) -> bool {
        let removed_uuid = self.by_uuid.remove(&uuid).is_some();
        let removed_id = self.by_id.shift_remove(&entity_id).is_some();
        removed_uuid || removed_id
    }

    /// Returns the UUID for `entity_id`, if present.
    #[must_use]
    pub fn uuid_by_id(&self, entity_id: i32) -> Option<Uuid> {
        self.by_id.get(&entity_id).copied()
    }

    /// Returns the network ID for `uuid`, if present.
    #[must_use]
    pub fn id_by_uuid(&self, uuid: &Uuid) -> Option<i32> {
        self.by_uuid.get(uuid).copied()
    }

    /// Returns `true` if `entity_id` is present.
    #[must_use]
    pub fn contains_id(&self, entity_id: i32) -> bool {
        self.by_id.contains_key(&entity_id)
    }

    /// Returns `true` if `uuid` is present.
    #[must_use]
    pub fn contains_uuid(&self, uuid: &Uuid) -> bool {
        self.by_uuid.contains_key(uuid)
    }

    /// Iterates all registered network IDs in insertion order (`EntityLookup.getAllEntities`).
    pub fn iter_ids(&self) -> impl Iterator<Item = i32> + '_ {
        self.by_id.keys().copied()
    }

    /// Returns the number of tracked entities (`EntityLookup.count`).
    #[must_use]
    pub fn count(&self) -> usize {
        self.by_uuid.len()
    }

    /// Returns `true` if there are no tracked entities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }
}
