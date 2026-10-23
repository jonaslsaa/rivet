//! Canonical tick-owned native entity admission, from Moonrise EntityLookup
//! and ChunkEntitySlices. Native values live once; indexes contain IDs only.
//! World callbacks are REQUIRED authorities, not invented tracker/AI success.
use super::entity_id_allocator::EntityIdOccupancy;
use super::full_chunk_status::FullChunkStatus;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_util::mth::Uuid;
use rivet_world::entity::loaded_entity::NativeLoadedEntity;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

#[cfg(test)]
#[path = "entity_lookup_fixture.rs"]
mod fixture;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Hidden,
    Tracked,
    Ticking,
}
impl Visibility {
    pub fn is_accessible(self) -> bool {
        self != Self::Hidden
    }
    pub fn is_ticking(self) -> bool {
        self == Self::Ticking
    }
}
fn chunk_visibility(status: FullChunkStatus) -> Visibility {
    match status {
        FullChunkStatus::Inaccessible => Visibility::Hidden,
        FullChunkStatus::Full | FullChunkStatus::BlockTicking => Visibility::Tracked,
        FullChunkStatus::EntityTicking => Visibility::Ticking,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityLifecycleEvent {
    Created,
    TrackingStart,
    TrackingEnd,
    TickingStart,
    TickingEnd,
    Destroyed,
    SectionChange,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntityLookupError {
    #[error("entity lifecycle authority unavailable: {0}")]
    LifecycleUnavailable(String),
    #[error("entity chunk status change is reentrant at {0:?}")]
    ReentrantChunk(ChunkPos),
    #[error("entity {0} removal authority did not mark the native value removed")]
    RemovalNotMarked(i32),
}
pub trait EntityLifecycle {
    /// Other canonical owners, currently live session-owned players. This is
    /// an actual lookup, not a default claim that the destination is empty.
    fn external_uuid_in_use(&self, uuid: &Uuid) -> bool;
    fn on_event(
        &mut self,
        event: EntityLifecycleEvent,
        entity: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError>;
    /// Optional readonly observer of the source callback's final native-state
    /// phase. This default is not event-dispatch authority: a production
    /// tracker must explicitly implement actual world add/remove events here.
    fn after_event(
        &mut self,
        _event: EntityLifecycleEvent,
        _entity: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError> {
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionRejection {
    Removed,
    DuplicateId,
    DuplicateUuid,
    WrongChunk,
}
#[derive(Debug)]
pub enum Admission {
    Added(i32),
    Rejected {
        reason: AdmissionRejection,
        entity: Box<NativeLoadedEntity>,
    },
}
#[derive(Debug)]
pub struct EntityRemovalFailure {
    pub error: EntityLookupError,
    /// Some after unregister already committed; None if lookup still owns it.
    pub entity: Option<Box<NativeLoadedEntity>>,
}
#[derive(Debug)]
struct Record {
    entity: NativeLoadedEntity,
    chunk: ChunkPos,
    section_y: i32,
    visibility: Visibility,
    updating: bool,
}
#[derive(Debug)]
struct ChunkEntities {
    status: FullChunkStatus,
    ids: Vec<i32>,
    sections: Vec<Vec<i32>>,
    prevent_status_updates: bool,
}
pub struct EntityLookup {
    min_section: i32,
    max_section: i32,
    occupancy: EntityIdOccupancy,
    by_id: HashMap<i32, Record>,
    by_uuid: HashMap<Uuid, i32>,
    chunks: HashMap<ChunkPos, ChunkEntities>,
    accessible: Vec<i32>,
    tracked: Vec<i32>,
}
fn remove_id(ids: &mut Vec<i32>, id: i32) {
    if let Some(index) = ids.iter().position(|candidate| *candidate == id) {
        ids.swap_remove(index);
    }
}
impl EntityLookup {
    pub fn new(min_section: i32, max_section: i32, occupancy: EntityIdOccupancy) -> Self {
        assert!(
            min_section <= max_section,
            "actual dimension section bounds"
        );
        Self {
            min_section,
            max_section,
            occupancy,
            by_id: HashMap::new(),
            by_uuid: HashMap::new(),
            chunks: HashMap::new(),
            accessible: Vec::new(),
            tracked: Vec::new(),
        }
    }
    pub fn occupancy(&self) -> EntityIdOccupancy {
        self.occupancy.clone()
    }
    /// Source hasEntity does NOT mask HIDDEN values, including IDs held by
    /// the separately owned players sharing this same occupancy handle.
    pub fn has_entity(&self, id: i32) -> bool {
        self.occupancy.contains(id)
    }
    pub fn contains_uuid(&self, uuid: &Uuid) -> bool {
        self.by_uuid.contains_key(uuid)
    }
    pub fn get_raw(&self, id: i32) -> Option<&NativeLoadedEntity> {
        self.by_id.get(&id).map(|record| &record.entity)
    }
    pub fn get_raw_by_uuid(&self, uuid: &Uuid) -> Option<&NativeLoadedEntity> {
        self.by_uuid.get(uuid).and_then(|id| self.get_raw(*id))
    }
    pub fn get(&self, id: i32) -> Option<&NativeLoadedEntity> {
        self.by_id
            .get(&id)
            .filter(|record| record.visibility.is_accessible())
            .map(|record| &record.entity)
    }
    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<&NativeLoadedEntity> {
        self.by_uuid.get(uuid).and_then(|id| self.get(*id))
    }
    pub fn visibility(&self, id: i32) -> Option<Visibility> {
        self.by_id.get(&id).map(|record| record.visibility)
    }
    pub fn section(&self, id: i32) -> Option<(ChunkPos, i32)> {
        self.by_id
            .get(&id)
            .map(|record| (record.chunk, record.section_y))
    }
    pub fn all(&self) -> impl Iterator<Item = &NativeLoadedEntity> {
        self.accessible.iter().filter_map(|id| self.get_raw(*id))
    }
    pub fn all_mapped(&self) -> impl Iterator<Item = &NativeLoadedEntity> {
        self.by_id.values().map(|record| &record.entity)
    }
    pub fn chunk_ids(&self, pos: ChunkPos) -> &[i32] {
        self.chunks
            .get(&pos)
            .map_or(&[], |chunk| chunk.ids.as_slice())
    }
    /// Holder-owned entity-section load. Existing transient slices retain
    /// their actual status/membership; only a new empty slice inherits status.
    pub fn ensure_entity_chunk(&mut self, pos: ChunkPos, status: FullChunkStatus) {
        if !self.chunks.contains_key(&pos) {
            self.ensure_chunk(pos).status = status;
        }
    }
    pub fn unload_empty_entity_chunk(&mut self, pos: ChunkPos) -> bool {
        if self
            .chunks
            .get(&pos)
            .is_some_and(|chunk| chunk.ids.is_empty() && !chunk.prevent_status_updates)
        {
            self.chunks.remove(&pos);
            true
        } else {
            false
        }
    }
    pub fn section_ids(&self, pos: ChunkPos, y: i32) -> &[i32] {
        self.chunks
            .get(&pos)
            .and_then(|chunk| chunk.sections.get((y - self.min_section) as usize))
            .map_or(&[], Vec::as_slice)
    }
    pub fn tracked_ids(&self) -> &[i32] {
        &self.tracked
    }
    fn ensure_chunk(&mut self, pos: ChunkPos) -> &mut ChunkEntities {
        let count = (self.max_section - self.min_section + 1) as usize;
        self.chunks.entry(pos).or_insert_with(|| ChunkEntities {
            status: FullChunkStatus::Inaccessible,
            ids: Vec::new(),
            sections: vec![Vec::new(); count],
            prevent_status_updates: false,
        })
    }
    /// Worldgen roots are neither passengers nor cross-chunk relocations in
    /// this supported native slice. Passenger graph loading refuses earlier.
    pub fn add_worldgen_chunk_entities(
        &mut self,
        entities: &mut Vec<NativeLoadedEntity>,
        source: Option<ChunkPos>,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<Vec<Admission>, EntityLookupError> {
        self.add_chunk_entities(entities, source, false, lifecycle)
    }
    /// Actual entity-region disk admission suppresses onCreated and does not
    /// run proto-chunk duplicate-UUID regeneration policy.
    pub fn add_entity_chunk_entities(
        &mut self,
        entities: &mut Vec<NativeLoadedEntity>,
        source: ChunkPos,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<Vec<Admission>, EntityLookupError> {
        self.add_chunk_entities(entities, Some(source), true, lifecycle)
    }
    fn add_chunk_entities(
        &mut self,
        entities: &mut Vec<NativeLoadedEntity>,
        source: Option<ChunkPos>,
        from_disk: bool,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<Vec<Admission>, EntityLookupError> {
        let mut outcomes = Vec::with_capacity(entities.len());
        let mut pending = std::mem::take(entities).into_iter();
        while let Some(entity) = pending.next() {
            if source.is_some_and(|pos| {
                pos != ChunkPos::containing(&BlockPos::containing(
                    entity.position().x,
                    entity.position().y,
                    entity.position().z,
                ))
            }) {
                outcomes.push(Admission::Rejected {
                    reason: AdmissionRejection::WrongChunk,
                    entity: Box::new(entity),
                });
                continue;
            }
            match catch_unwind(AssertUnwindSafe(|| {
                self.add_entity(entity, from_disk, lifecycle)
            })) {
                Ok(Ok(outcome)) => outcomes.push(outcome),
                Ok(Err(error)) => {
                    // The failing value already belongs to the canonical
                    // lookup. Keep every not-yet-admitted native value with
                    // the caller; reconstruction consumed real identity/RNG.
                    entities.extend(pending);
                    return Err(error);
                }
                Err(panic) => {
                    entities.extend(pending);
                    resume_unwind(panic);
                }
            }
        }
        Ok(outcomes)
    }
    /// Maps/section membership commit BEFORE source callbacks. If a callback
    /// refuses, the native value remains owned and inspectable here; no false
    /// successful FULL result or rollback of already executed effects occurs.
    pub fn add_entity(
        &mut self,
        entity: NativeLoadedEntity,
        from_disk: bool,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<Admission, EntityLookupError> {
        if entity.is_removed() {
            return Ok(Admission::Rejected {
                reason: AdmissionRejection::Removed,
                entity: Box::new(entity),
            });
        }
        let id = entity.entity_id();
        let uuid = entity.uuid();
        if !self.occupancy.register(id) {
            return Ok(Admission::Rejected {
                reason: AdmissionRejection::DuplicateId,
                entity: Box::new(entity),
            });
        }
        if self.by_uuid.contains_key(&uuid) || lifecycle.external_uuid_in_use(&uuid) {
            self.occupancy.release(id);
            return Ok(Admission::Rejected {
                reason: AdmissionRejection::DuplicateUuid,
                entity: Box::new(entity),
            });
        }
        let pos = entity.position();
        let block = BlockPos::containing(pos.x, pos.y, pos.z);
        let chunk = ChunkPos::containing(&block);
        let y = (block.get_y() >> 4).clamp(self.min_section, self.max_section);
        let section_index = (y - self.min_section) as usize;
        let slices = self.ensure_chunk(chunk);
        slices.ids.push(id);
        slices.sections[section_index].push(id);
        let visibility = chunk_visibility(slices.status);
        self.by_uuid.insert(uuid, id);
        self.by_id.insert(
            id,
            Record {
                entity,
                chunk,
                section_y: y,
                visibility,
                updating: false,
            },
        );
        let record = self.by_id.get_mut(&id).unwrap();
        transition(
            record,
            Visibility::Hidden,
            visibility,
            !from_disk,
            false,
            &mut self.accessible,
            &mut self.tracked,
            lifecycle,
        )?;
        Ok(Admission::Added(id))
    }
    pub fn set_chunk_status(
        &mut self,
        pos: ChunkPos,
        status: FullChunkStatus,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<(), EntityLookupError> {
        let slices = self.ensure_chunk(pos);
        if slices.prevent_status_updates {
            return Err(EntityLookupError::ReentrantChunk(pos));
        }
        slices.status = status;
        slices.prevent_status_updates = true;
        let ids = slices.ids.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            for id in ids {
                let Some(record) = self.by_id.get_mut(&id) else {
                    continue;
                };
                let old = record.visibility;
                record.visibility = chunk_visibility(status);
                transition(
                    record,
                    old,
                    record.visibility,
                    false,
                    false,
                    &mut self.accessible,
                    &mut self.tracked,
                    lifecycle,
                )?;
            }
            Ok(())
        }));
        self.chunks.get_mut(&pos).unwrap().prevent_status_updates = false;
        match result {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
    /// Source EntityCallback removes raw indexes and section membership before
    /// tracking/ticking-end callbacks. Failure returns the removed owned value.
    /// Execute genuine removal hooks BEFORE the lookup's callback unregister.
    /// The authority may change removal/serialized subtype state, but must not
    /// move/rekey the entity. This is not a generic mutable-entity escape hatch.
    pub fn remove_with_authority(
        &mut self,
        id: i32,
        destroy: bool,
        lifecycle: &mut dyn EntityLifecycle,
        remove: impl FnOnce(&mut NativeLoadedEntity) -> Result<(), EntityLookupError>,
    ) -> Result<Option<NativeLoadedEntity>, EntityRemovalFailure> {
        let Some(record) = self.by_id.get_mut(&id) else {
            return Ok(None);
        };
        if let Err(error) = remove(&mut record.entity) {
            return Err(EntityRemovalFailure {
                error,
                entity: None,
            });
        }
        if !record.entity.is_removed() {
            return Err(EntityRemovalFailure {
                error: EntityLookupError::RemovalNotMarked(id),
                entity: None,
            });
        }
        self.unregister_removed(id, destroy, lifecycle)
    }
    fn unregister_removed(
        &mut self,
        id: i32,
        destroy: bool,
        lifecycle: &mut dyn EntityLifecycle,
    ) -> Result<Option<NativeLoadedEntity>, EntityRemovalFailure> {
        let Some(mut record) = self.by_id.remove(&id) else {
            return Ok(None);
        };
        self.by_uuid.remove(&record.entity.uuid());
        self.occupancy.release(id);
        if let Some(chunk) = self.chunks.get_mut(&record.chunk) {
            remove_id(&mut chunk.ids, id);
            remove_id(
                &mut chunk.sections[(record.section_y - self.min_section) as usize],
                id,
            );
        }
        let old = record.visibility;
        record.visibility = Visibility::Hidden;
        if let Err(error) = transition(
            &mut record,
            old,
            Visibility::Hidden,
            false,
            destroy,
            &mut self.accessible,
            &mut self.tracked,
            lifecycle,
        ) {
            return Err(EntityRemovalFailure {
                error,
                entity: Some(Box::new(record.entity)),
            });
        }
        Ok(Some(record.entity))
    }
}
#[allow(clippy::too_many_arguments)] // Source entityStatusChange plus split owned list borrows.
fn transition(
    record: &mut Record,
    old: Visibility,
    new: Visibility,
    created: bool,
    destroyed: bool,
    accessible: &mut Vec<i32>,
    tracked: &mut Vec<i32>,
    lifecycle: &mut dyn EntityLifecycle,
) -> Result<(), EntityLookupError> {
    if record.updating {
        return Ok(());
    }
    record.updating = true;
    let id = record.entity.entity_id();
    let result = catch_unwind(AssertUnwindSafe(|| {
        if created {
            record.entity.mark_created();
            lifecycle.on_event(EntityLifecycleEvent::Created, &record.entity)?;
            lifecycle.after_event(EntityLifecycleEvent::Created, &record.entity)?;
        }
        // Source returns early for equal visibility even when destroy=true.
        if old == new {
            return Ok(());
        }
        if new > old {
            if !old.is_accessible() && new.is_accessible() {
                tracked.push(id);
                accessible.push(id);
                record.entity.begin_tracking();
                lifecycle.on_event(EntityLifecycleEvent::TrackingStart, &record.entity)?;
                record.entity.initialize_tracking_origin();
                lifecycle.after_event(EntityLifecycleEvent::TrackingStart, &record.entity)?;
            }
            if !old.is_ticking() && new.is_ticking() {
                lifecycle.on_event(EntityLifecycleEvent::TickingStart, &record.entity)?;
            }
        } else {
            if old.is_ticking() && !new.is_ticking() {
                lifecycle.on_event(EntityLifecycleEvent::TickingEnd, &record.entity)?;
            }
            if old.is_accessible() && !new.is_accessible() {
                remove_id(tracked, id);
                remove_id(accessible, id);
                lifecycle.on_event(EntityLifecycleEvent::TrackingEnd, &record.entity)?;
                record.entity.mark_tracking_end();
                lifecycle.after_event(EntityLifecycleEvent::TrackingEnd, &record.entity)?;
            }
        }
        if destroyed {
            lifecycle.on_event(EntityLifecycleEvent::Destroyed, &record.entity)?;
        }
        Ok(())
    }));
    record.updating = false;
    match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    }
}
