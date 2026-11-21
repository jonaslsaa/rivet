//! Port of `net.minecraft.world.level.entity.PersistentEntitySectionManager` (Paper 26.2).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/PersistentEntitySectionManager.java`
//!
//! Owns all loaded entities on the tick thread (`OWNERSHIP.md`), tracks section
//! ownership, chunk visibility/load state, dirtying, unload/reload lifecycle,
//! passenger tree reconstruction on save, and observable `LevelCallback` events.

use crate::storage::chunk_entities::{ChunkEntities, EntityRegionBackend, EntityStorage};
use crate::storage::entity_lookup::EntityLookup;
use crate::storage::entity_record::{
    Aabb, EntityRemovalReason, EntityStorageError, StoredEntityRecord,
};
use crate::storage::entity_section::{EntitySectionStorage, IterationControl};
use crate::storage::visibility::Visibility;
use indexmap::IndexMap;
use rivet_registry::core::{ChunkPos, Vec3};
use rivet_util::mth::Uuid;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Port of `PersistentEntitySectionManager.ChunkLoadStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChunkLoadStatus {
    #[default]
    Fresh,
    Pending,
    Loaded,
}

/// Observable lifecycle events mirroring `net.minecraft.world.level.entity.LevelCallback`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityLifecycleEvent {
    Created(i32),
    TrackingStart(i32),
    TrackingEnd(i32),
    TickingStart(i32),
    TickingEnd(i32),
    SectionChange {
        entity_id: i32,
        old_section: i64,
        new_section: i64,
    },
    Destroyed(i32),
}

/// Port of `net.minecraft.world.level.entity.PersistentEntitySectionManager`.
#[derive(Debug)]
pub struct PersistentEntitySectionManager<B: EntityRegionBackend> {
    /// All known UUIDs across loaded sections (visible and hidden).
    known_uuids: HashSet<Uuid>,
    /// Authoritative tick-thread arena of all currently loaded entities, keyed by network ID.
    entities: IndexMap<i32, StoredEntityRecord>,
    /// Current packed `SectionPos` key for each loaded entity.
    entity_sections: HashMap<i32, i64>,
    /// Visible (`Visibility::is_accessible`) entity lookup by network ID and UUID.
    visible_entity_storage: EntityLookup,
    /// Loaded 16x16x16 entity section storage.
    section_storage: EntitySectionStorage,
    /// Permanent chunk-entity storage backend.
    permanent_storage: EntityStorage<B>,
    /// Per-chunk visibility (`HIDDEN` when absent, matching Paper `defaultReturnValue(HIDDEN)`).
    chunk_visibility: HashMap<i64, Visibility>,
    /// Per-chunk load status (`FRESH` when absent, matching Paper `defaultReturnValue(FRESH)`).
    chunk_load_statuses: HashMap<i64, ChunkLoadStatus>,
    /// Chunks scheduled for unload once hidden.
    chunks_to_unload: BTreeSet<i64>,
    /// Chunks whose entity state changed since last save.
    dirty_chunks: BTreeSet<i64>,
    /// Loaded chunks queued for `process_pending_loads()`.
    loading_inbox: VecDeque<ChunkEntities<StoredEntityRecord>>,
    /// Recorded `LevelCallback` lifecycle events in exact firing order.
    events: Vec<EntityLifecycleEvent>,
}

impl<B: EntityRegionBackend> PersistentEntitySectionManager<B> {
    /// Creates a new `PersistentEntitySectionManager` backed by `permanent_storage`.
    #[must_use]
    pub fn new(permanent_storage: EntityStorage<B>) -> Self {
        Self {
            known_uuids: HashSet::new(),
            entities: IndexMap::new(),
            entity_sections: HashMap::new(),
            visible_entity_storage: EntityLookup::new(),
            section_storage: EntitySectionStorage::new(),
            permanent_storage,
            chunk_visibility: HashMap::new(),
            chunk_load_statuses: HashMap::new(),
            chunks_to_unload: BTreeSet::new(),
            dirty_chunks: BTreeSet::new(),
            loading_inbox: VecDeque::new(),
            events: Vec::new(),
        }
    }

    /// Returns a reference to the permanent `EntityStorage`.
    #[must_use]
    pub const fn permanent_storage(&self) -> &EntityStorage<B> {
        &self.permanent_storage
    }

    /// Returns a mutable reference to the permanent `EntityStorage`.
    pub fn permanent_storage_mut(&mut self) -> &mut EntityStorage<B> {
        &mut self.permanent_storage
    }

    /// Returns the `EntityLookup` of currently accessible (visible) entities.
    #[must_use]
    pub const fn visible_lookup(&self) -> &EntityLookup {
        &self.visible_entity_storage
    }

    /// Returns the underlying `EntitySectionStorage`.
    #[must_use]
    pub const fn section_storage(&self) -> &EntitySectionStorage {
        &self.section_storage
    }

    /// Drains all recorded `LevelCallback` lifecycle events since the last drain.
    pub fn drain_events(&mut self) -> Vec<EntityLifecycleEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns the current visibility of `pos` (`Visibility::Hidden` if absent).
    #[must_use]
    pub fn chunk_visibility(&self, pos: ChunkPos) -> Visibility {
        self.chunk_visibility
            .get(&pos.pack())
            .copied()
            .unwrap_or(Visibility::Hidden)
    }

    /// Returns the current load status of `pos` (`ChunkLoadStatus::Fresh` if absent).
    #[must_use]
    pub fn chunk_load_status(&self, pos: ChunkPos) -> ChunkLoadStatus {
        self.chunk_load_statuses
            .get(&pos.pack())
            .copied()
            .unwrap_or(ChunkLoadStatus::Fresh)
    }

    /// Returns `true` if entities for `pos` are loaded (`areEntitiesLoaded`).
    #[must_use]
    pub fn are_entities_loaded(&self, pos: ChunkPos) -> bool {
        self.chunk_load_status(pos) == ChunkLoadStatus::Loaded
    }

    /// Returns `true` if `pos` has unsaved entity changes.
    #[must_use]
    pub fn is_chunk_dirty(&self, pos: ChunkPos) -> bool {
        self.dirty_chunks.contains(&pos.pack())
    }

    /// Returns the number of loaded entities (both hidden and visible).
    #[must_use]
    pub fn loaded_entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Returns a loaded entity by network ID regardless of visibility.
    #[must_use]
    pub fn get_loaded_entity(&self, entity_id: i32) -> Option<&StoredEntityRecord> {
        self.entities.get(&entity_id)
    }

    /// Returns an accessible (visible) entity by network ID.
    #[must_use]
    pub fn get_visible_by_id(&self, entity_id: i32) -> Option<&StoredEntityRecord> {
        if self.visible_entity_storage.contains_id(entity_id) {
            self.entities.get(&entity_id)
        } else {
            None
        }
    }

    /// Returns an accessible (visible) entity by UUID.
    #[must_use]
    pub fn get_visible_by_uuid(&self, uuid: &Uuid) -> Option<&StoredEntityRecord> {
        let entity_id = self.visible_entity_storage.id_by_uuid(uuid)?;
        self.entities.get(&entity_id)
    }

    /// Returns the current packed section key for `entity_id`, if loaded.
    #[must_use]
    pub fn entity_section_key(&self, entity_id: i32) -> Option<i64> {
        self.entity_sections.get(&entity_id).copied()
    }

    /// Allocates a new network ID from the permanent storage allocator.
    pub fn allocate_network_id(&mut self) -> i32 {
        self.permanent_storage.allocate_network_id()
    }

    /// Adds a newly spawned entity (`PersistentEntitySectionManager.addNewEntity`).
    ///
    /// Marks the owning chunk dirty and fires `Created` (plus `TrackingStart`/`TickingStart`
    /// according to effective visibility). Returns `Ok(false)` if the entity is already
    /// removed, or `Err(DuplicateEntityUuid | DuplicateNetworkId)` on collision.
    pub fn add_new_entity(
        &mut self,
        entity: StoredEntityRecord,
    ) -> Result<bool, EntityStorageError> {
        let added = self.add_entity_internal(entity, false)?;
        Ok(added)
    }

    /// Adds a root entity and its nested `passengers` tree in pre-order,
    /// wiring `vehicle_id` / `passenger_ids` links and marking owning chunks dirty.
    pub fn add_new_entity_with_passengers(
        &mut self,
        root: StoredEntityRecord,
    ) -> Result<Vec<i32>, EntityStorageError> {
        let flat = root.flatten_tree();
        let mut added_ids = Vec::with_capacity(flat.len());
        for entity in flat {
            let id = entity.network_id;
            if self.add_entity_internal(entity, false)? {
                added_ids.push(id);
            }
        }
        Ok(added_ids)
    }

    /// Port of `PersistentEntitySectionManager.updateChunkStatus(ChunkPos, Visibility)`.
    ///
    /// Updates chunk visibility, schedules loads when becoming non-hidden, schedules
    /// unloads when becoming `Hidden`, and transitions existing sections' entities
    /// through `TrackingStart`/`TrackingEnd`/`TickingStart`/`TickingEnd`.
    pub fn update_chunk_status(
        &mut self,
        pos: ChunkPos,
        chunk_status: Visibility,
    ) -> Result<(), EntityStorageError> {
        let chunk_key = pos.pack();
        if chunk_status == Visibility::Hidden {
            self.chunk_visibility.remove(&chunk_key);
            self.chunks_to_unload.insert(chunk_key);
        } else {
            self.chunk_visibility.insert(chunk_key, chunk_status);
            self.chunks_to_unload.remove(&chunk_key);
            self.ensure_chunk_queued_for_load(chunk_key)?;
        }

        let section_keys = self
            .section_storage
            .existing_section_positions_in_chunk(chunk_key);

        for section_key in section_keys {
            let (previous_status, entity_ids) = {
                let Some(section) = self.section_storage.get_section_mut(section_key) else {
                    continue;
                };
                let prev = section.update_chunk_status(chunk_status);
                let ids: Vec<i32> = section.entity_ids().collect();
                (prev, ids)
            };

            let was_accessible = previous_status.is_accessible();
            let is_accessible = chunk_status.is_accessible();
            let was_ticking = previous_status.is_ticking();
            let is_ticking = chunk_status.is_ticking();

            if was_ticking && !is_ticking {
                for &entity_id in &entity_ids {
                    if !self.is_entity_always_ticking(entity_id) {
                        self.stop_ticking(entity_id);
                    }
                }
            }

            if was_accessible && !is_accessible {
                for &entity_id in &entity_ids {
                    if !self.is_entity_always_ticking(entity_id) {
                        self.stop_tracking(entity_id);
                    }
                }
            } else if !was_accessible && is_accessible {
                for &entity_id in &entity_ids {
                    if !self.is_entity_always_ticking(entity_id) {
                        self.start_tracking(entity_id);
                    }
                }
            }

            if !was_ticking && is_ticking {
                for &entity_id in &entity_ids {
                    if !self.is_entity_always_ticking(entity_id) {
                        self.start_ticking(entity_id);
                    }
                }
            }
        }

        Ok(())
    }

    /// Port of `PersistentEntitySectionManager.processPendingLoads()`.
    ///
    /// Drains `loading_inbox`, adds loaded entities into their sections without
    /// firing `Created` or dirtying the chunk, and marks each chunk `Loaded`.
    pub fn process_pending_loads(&mut self) -> Result<usize, EntityStorageError> {
        let mut loaded_chunks = 0;
        while let Some(chunk) = self.loading_inbox.pop_front() {
            let chunk_key = chunk.pos().pack();
            for entity in chunk.into_entities() {
                // Skip duplicate UUIDs on load (matching Paper `addEntityUuid` warning/skip).
                if self.known_uuids.contains(&entity.uuid) {
                    continue;
                }
                self.add_entity_internal(entity, true)?;
            }
            self.chunk_load_statuses
                .insert(chunk_key, ChunkLoadStatus::Loaded);
            loaded_chunks += 1;
        }
        Ok(loaded_chunks)
    }

    /// Port of `PersistentEntitySectionManager.processUnloads()`.
    ///
    /// Attempts to persist and unload every chunk in `chunks_to_unload`. If a
    /// chunk's write fails, ownership is retained in sections and `chunks_to_unload`
    /// is left intact for retry.
    pub fn process_unloads(&mut self) -> Result<usize, EntityStorageError> {
        let pending: Vec<i64> = self.chunks_to_unload.iter().copied().collect();
        let mut unloaded = 0;
        for chunk_key in pending {
            if self.process_chunk_unload(chunk_key)? {
                self.chunks_to_unload.remove(&chunk_key);
                unloaded += 1;
            }
        }
        Ok(unloaded)
    }

    /// Port of `PersistentEntitySectionManager.tick()`.
    pub fn tick(&mut self) -> Result<(), EntityStorageError> {
        self.process_unloads()?;
        self.process_pending_loads()?;
        Ok(())
    }

    /// Port of `PersistentEntitySectionManager.autoSave()`.
    ///
    /// Persists all loaded chunks that are either dirty or have existing sections,
    /// without unloading them.
    pub fn auto_save(&mut self) -> Result<usize, EntityStorageError> {
        let mut candidates = self.section_storage.all_chunks_with_existing_sections();
        candidates.extend(self.dirty_chunks.iter().copied());
        let mut saved = 0;
        for chunk_key in candidates {
            if self.chunk_load_statuses.get(&chunk_key).copied() == Some(ChunkLoadStatus::Loaded)
                && self.store_chunk_sections(chunk_key, false)?
            {
                saved += 1;
            }
        }
        Ok(saved)
    }

    /// Port of `PersistentEntitySectionManager.saveAll()`.
    ///
    /// Processes pending loads, persists all loaded chunks (including dirty empty
    /// chunks), and flushes the underlying region storage.
    pub fn save_all(&mut self) -> Result<usize, EntityStorageError> {
        self.process_pending_loads()?;
        let mut candidates = self.section_storage.all_chunks_with_existing_sections();
        candidates.extend(self.dirty_chunks.iter().copied());
        let mut saved = 0;
        for chunk_key in candidates {
            if self.store_chunk_sections(chunk_key, false)? {
                saved += 1;
            }
        }
        self.permanent_storage.flush(true)?;
        Ok(saved)
    }

    /// Moves a loaded entity to `new_pos` (`PersistentEntitySectionManager.Callback.onMove`).
    ///
    /// Updates section membership if the entity crossed a 16x16x16 section boundary,
    /// emits `SectionChange`, applies visibility transitions if the new section has
    /// a different `Visibility`, prunes an emptied old section, and marks affected
    /// chunks dirty.
    pub fn move_entity(
        &mut self,
        entity_id: i32,
        new_pos: Vec3,
    ) -> Result<bool, EntityStorageError> {
        if !new_pos.x.is_finite() || !new_pos.y.is_finite() || !new_pos.z.is_finite() {
            return Err(EntityStorageError::NonFinitePosition);
        }

        let (old_section_key, new_section_key, always_ticking) = {
            let Some(entity) = self.entities.get_mut(&entity_id) else {
                return Ok(false);
            };
            let old_key = self
                .entity_sections
                .get(&entity_id)
                .copied()
                .unwrap_or_else(|| entity.section_key());
            entity.set_position(new_pos);
            let new_key = entity.section_key();
            (old_key, new_key, entity.always_ticking)
        };

        let old_chunk_key = EntitySectionStorage::chunk_key_from_section_key(old_section_key);
        let new_chunk_key = EntitySectionStorage::chunk_key_from_section_key(new_section_key);
        self.dirty_chunks.insert(old_chunk_key);
        self.dirty_chunks.insert(new_chunk_key);

        if old_section_key == new_section_key {
            return Ok(true);
        }

        let old_visibility = self
            .section_storage
            .get_section(old_section_key)
            .map_or(Visibility::Hidden, |s| s.status());

        if let Some(old_section) = self.section_storage.get_section_mut(old_section_key) {
            old_section.remove(entity_id);
        }
        self.remove_section_if_empty(old_section_key);

        let new_chunk_visibility = self
            .chunk_visibility
            .get(&new_chunk_key)
            .copied()
            .unwrap_or(Visibility::Hidden);
        let new_section = self
            .section_storage
            .get_or_create_section(new_section_key, new_chunk_visibility);
        new_section.add(entity_id);
        let new_visibility = new_section.status();
        self.entity_sections.insert(entity_id, new_section_key);

        self.events.push(EntityLifecycleEvent::SectionChange {
            entity_id,
            old_section: old_section_key,
            new_section: new_section_key,
        });

        let old_effective = effective_visibility(always_ticking, old_visibility);
        let new_effective = effective_visibility(always_ticking, new_visibility);
        self.update_entity_status(entity_id, old_effective, new_effective);

        Ok(true)
    }

    /// Removes an entity with `reason` (`PersistentEntitySectionManager.Callback.onRemove`).
    ///
    /// Stops ticking and tracking as needed, removes the entity from its section
    /// and `known_uuids`, fires `Destroyed` if `reason.should_destroy()`, marks the
    /// owning chunk dirty, and prunes the section if it becomes empty.
    pub fn remove_entity(&mut self, entity_id: i32, reason: EntityRemovalReason) -> bool {
        let Some(mut entity) = self.entities.shift_remove(&entity_id) else {
            return false;
        };
        entity.removal_reason = Some(reason);
        let uuid = entity.uuid;
        let always_ticking = entity.always_ticking;

        // Detach from vehicle's passenger list if riding.
        if let Some(vehicle_id) = entity.vehicle_id
            && let Some(vehicle) = self.entities.get_mut(&vehicle_id)
        {
            vehicle.passenger_ids.retain(|&pid| pid != entity_id);
        }
        // Detach passengers' vehicle pointer if this entity was a vehicle.
        for passenger_id in &entity.passenger_ids {
            if let Some(passenger) = self.entities.get_mut(passenger_id)
                && passenger.vehicle_id == Some(entity_id)
            {
                passenger.vehicle_id = None;
            }
        }

        let section_key = self
            .entity_sections
            .remove(&entity_id)
            .unwrap_or_else(|| entity.section_key());
        let chunk_key = EntitySectionStorage::chunk_key_from_section_key(section_key);

        let section_visibility = self
            .section_storage
            .get_section(section_key)
            .map_or(Visibility::Hidden, |s| s.status());
        let effective = effective_visibility(always_ticking, section_visibility);

        if effective.is_ticking() {
            self.events
                .push(EntityLifecycleEvent::TickingEnd(entity_id));
        }
        if effective.is_accessible() {
            self.visible_entity_storage.remove(entity_id, uuid);
            self.events
                .push(EntityLifecycleEvent::TrackingEnd(entity_id));
        }
        if reason.should_destroy() {
            self.events.push(EntityLifecycleEvent::Destroyed(entity_id));
        }

        self.known_uuids.remove(&uuid);
        if let Some(section) = self.section_storage.get_section_mut(section_key) {
            section.remove(entity_id);
        }
        self.remove_section_if_empty(section_key);
        self.dirty_chunks.insert(chunk_key);

        true
    }

    /// Queries accessible entities whose bounding boxes intersect `bounds`, using
    /// Paper's `EntitySectionStorage.forEachAccessibleNonEmptySection` bounds expansion.
    #[must_use]
    pub fn get_entities_in_bounds(&self, bounds: Aabb) -> Vec<&StoredEntityRecord> {
        let mut results = Vec::new();
        self.section_storage.for_each_accessible_non_empty_section(
            bounds,
            |_section_key, section| {
                for entity_id in section.entity_ids() {
                    if let Some(entity) = self.entities.get(&entity_id)
                        && entity.bounding_box.intersects(&bounds)
                    {
                        results.push(entity);
                    }
                }
                IterationControl::Continue
            },
        );
        results
    }

    fn add_entity_internal(
        &mut self,
        entity: StoredEntityRecord,
        loaded_from_disk: bool,
    ) -> Result<bool, EntityStorageError> {
        if entity.is_removed() {
            return Ok(false);
        }
        if self.known_uuids.contains(&entity.uuid) {
            return Err(EntityStorageError::DuplicateEntityUuid { uuid: entity.uuid });
        }
        if self.entities.contains_key(&entity.network_id) {
            return Err(EntityStorageError::DuplicateNetworkId {
                id: entity.network_id,
            });
        }

        let entity_id = entity.network_id;
        let uuid = entity.uuid;
        let always_ticking = entity.always_ticking;
        let section_key = entity.section_key();
        let chunk_key = EntitySectionStorage::chunk_key_from_section_key(section_key);

        self.known_uuids.insert(uuid);
        self.entities.insert(entity_id, entity);
        self.entity_sections.insert(entity_id, section_key);

        let initial_visibility = self
            .chunk_visibility
            .get(&chunk_key)
            .copied()
            .unwrap_or(Visibility::Hidden);
        let section = self
            .section_storage
            .get_or_create_section(section_key, initial_visibility);
        section.add(entity_id);
        let section_status = section.status();

        if !loaded_from_disk {
            self.dirty_chunks.insert(chunk_key);
            self.events.push(EntityLifecycleEvent::Created(entity_id));
        }

        let effective = effective_visibility(always_ticking, section_status);
        if effective.is_accessible() {
            self.start_tracking(entity_id);
        }
        if effective.is_ticking() {
            self.start_ticking(entity_id);
        }

        Ok(true)
    }

    fn ensure_chunk_queued_for_load(&mut self, chunk_key: i64) -> Result<(), EntityStorageError> {
        let status = self
            .chunk_load_statuses
            .get(&chunk_key)
            .copied()
            .unwrap_or(ChunkLoadStatus::Fresh);
        if status == ChunkLoadStatus::Fresh {
            self.request_chunk_load(chunk_key)?;
        }
        Ok(())
    }

    fn request_chunk_load(&mut self, chunk_key: i64) -> Result<(), EntityStorageError> {
        self.chunk_load_statuses
            .insert(chunk_key, ChunkLoadStatus::Pending);
        let pos = ChunkPos::unpack(chunk_key);
        let loaded = self.permanent_storage.load_entities(pos)?;
        self.loading_inbox.push_back(loaded);
        Ok(())
    }

    fn process_chunk_unload(&mut self, chunk_key: i64) -> Result<bool, EntityStorageError> {
        let stored = self.store_chunk_sections(chunk_key, true)?;
        if !stored {
            return Ok(false);
        }
        self.chunk_load_statuses.remove(&chunk_key);
        Ok(true)
    }

    /// Port of `PersistentEntitySectionManager.storeChunkSections`.
    ///
    /// Reconstructs nested passenger trees for every root entity in `chunk_key`,
    /// persists them through `EntityStorage`, clears `dirty_chunks` on success,
    /// and (when `unload` is `true`) unloads every root and its passengers only
    /// after persistence succeeds.
    fn store_chunk_sections(
        &mut self,
        chunk_key: i64,
        unload: bool,
    ) -> Result<bool, EntityStorageError> {
        let load_status = self
            .chunk_load_statuses
            .get(&chunk_key)
            .copied()
            .unwrap_or(ChunkLoadStatus::Fresh);

        let section_entity_ids: Vec<i32> = self
            .section_storage
            .existing_sections_in_chunk(chunk_key)
            .into_iter()
            .flat_map(|(_, section)| section.entity_ids().collect::<Vec<_>>())
            .collect();

        let root_ids_to_save: Vec<i32> = section_entity_ids
            .iter()
            .copied()
            .filter(|&id| {
                self.entities
                    .get(&id)
                    .is_some_and(|e| !e.is_passenger() && e.should_be_saved())
            })
            .collect();

        let pos = ChunkPos::unpack(chunk_key);

        if root_ids_to_save.is_empty() {
            if load_status == ChunkLoadStatus::Loaded {
                self.permanent_storage
                    .store_entities(&ChunkEntities::empty(pos))?;
                self.dirty_chunks.remove(&chunk_key);
                if unload {
                    for id in section_entity_ids {
                        self.unload_entity_tree(id);
                    }
                }
                return Ok(true);
            }
            return Ok(true);
        }

        if load_status == ChunkLoadStatus::Fresh {
            self.request_chunk_load(chunk_key)?;
            return Ok(false);
        }

        let roots: Vec<StoredEntityRecord> = root_ids_to_save
            .iter()
            .filter_map(|&id| self.build_save_tree(id))
            .collect();
        self.permanent_storage
            .store_entities(&ChunkEntities::new(pos, roots))?;
        self.dirty_chunks.remove(&chunk_key);

        if unload {
            // Unload every root (and its passengers) plus any non-saveable entities left in the chunk.
            for id in root_ids_to_save {
                self.unload_entity_tree(id);
            }
            for id in section_entity_ids {
                if self.entities.contains_key(&id) {
                    self.unload_entity_tree(id);
                }
            }
        }

        Ok(true)
    }

    /// Reconstructs a `StoredEntityRecord` with its nested `passengers` tree from
    /// the manager's flat arena for serialization.
    fn build_save_tree(&self, entity_id: i32) -> Option<StoredEntityRecord> {
        let mut record = self.entities.get(&entity_id)?.clone();
        record.passengers = record
            .passenger_ids
            .iter()
            .filter_map(|&pid| self.build_save_tree(pid))
            .collect();
        Some(record)
    }

    fn unload_entity_tree(&mut self, root_id: i32) {
        let mut stack = vec![root_id];
        let mut to_unload = Vec::new();
        while let Some(id) = stack.pop() {
            to_unload.push(id);
            if let Some(entity) = self.entities.get(&id) {
                for &pid in &entity.passenger_ids {
                    stack.push(pid);
                }
            }
        }
        for id in to_unload {
            self.unload_single_entity(id);
        }
    }

    fn unload_single_entity(&mut self, entity_id: i32) {
        let Some(entity) = self.entities.shift_remove(&entity_id) else {
            return;
        };
        let uuid = entity.uuid;
        let always_ticking = entity.always_ticking;
        let section_key = self
            .entity_sections
            .remove(&entity_id)
            .unwrap_or_else(|| entity.section_key());
        let section_visibility = self
            .section_storage
            .get_section(section_key)
            .map_or(Visibility::Hidden, |s| s.status());
        let effective = effective_visibility(always_ticking, section_visibility);

        if effective.is_ticking() {
            self.events
                .push(EntityLifecycleEvent::TickingEnd(entity_id));
        }
        if effective.is_accessible() {
            self.visible_entity_storage.remove(entity_id, uuid);
            self.events
                .push(EntityLifecycleEvent::TrackingEnd(entity_id));
        }

        self.known_uuids.remove(&uuid);
        if let Some(section) = self.section_storage.get_section_mut(section_key) {
            section.remove(entity_id);
        }
        self.remove_section_if_empty(section_key);
    }

    fn remove_section_if_empty(&mut self, section_key: i64) {
        if self
            .section_storage
            .get_section(section_key)
            .is_some_and(crate::storage::entity_section::EntitySection::is_empty)
        {
            self.section_storage.remove(section_key);
        }
    }

    fn is_entity_always_ticking(&self, entity_id: i32) -> bool {
        self.entities
            .get(&entity_id)
            .is_some_and(|e| e.always_ticking)
    }

    fn update_entity_status(&mut self, entity_id: i32, old: Visibility, new: Visibility) {
        if old == new {
            return;
        }
        if old.is_ticking() && !new.is_ticking() {
            self.stop_ticking(entity_id);
        }
        if old.is_accessible() && !new.is_accessible() {
            self.stop_tracking(entity_id);
        } else if !old.is_accessible() && new.is_accessible() {
            self.start_tracking(entity_id);
        }
        if !old.is_ticking() && new.is_ticking() {
            self.start_ticking(entity_id);
        }
    }

    fn start_tracking(&mut self, entity_id: i32) {
        if let Some(entity) = self.entities.get(&entity_id) {
            self.visible_entity_storage.add(entity_id, entity.uuid);
            self.events
                .push(EntityLifecycleEvent::TrackingStart(entity_id));
        }
    }

    fn stop_tracking(&mut self, entity_id: i32) {
        if let Some(entity) = self.entities.get(&entity_id) {
            self.visible_entity_storage.remove(entity_id, entity.uuid);
            self.events
                .push(EntityLifecycleEvent::TrackingEnd(entity_id));
        }
    }

    fn start_ticking(&mut self, entity_id: i32) {
        self.events
            .push(EntityLifecycleEvent::TickingStart(entity_id));
    }

    fn stop_ticking(&mut self, entity_id: i32) {
        self.events
            .push(EntityLifecycleEvent::TickingEnd(entity_id));
    }
}

/// Port of `PersistentEntitySectionManager.getEffectiveStatus(T, Visibility)`.
#[must_use]
const fn effective_visibility(always_ticking: bool, section_status: Visibility) -> Visibility {
    if always_ticking {
        Visibility::Ticking
    } else {
        section_status
    }
}
