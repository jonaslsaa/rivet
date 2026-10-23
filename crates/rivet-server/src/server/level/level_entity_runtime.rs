//! Post-load reconstruction and admission into the level's canonical owner.
//! Tracking callbacks are required; this boundary does not fabricate a tracker
//! or declare a chunk FULL merely because its entities were decoded.
use super::ServerLevel;
#[cfg(unix)]
use crate::server::level::entity_region_storage::{
    EntityChunkReadiness, EntityRegionError, EntitySaveLimits, EntitySaveOutcome,
    StoredEntityRegion,
};
use crate::server::level::{
    entity_lookup::{
        EntityLifecycle, EntityLifecycleEvent, EntityLookup, EntityLookupError, Visibility,
    },
    entity_post_load::{
        DuplicateUuidDecision, EntityPostLoadContext, ExistingEntityView, NativePostLoadError,
    },
    entity_tracking::{EntityTrackingConfig, EntityTrackingRuntime, TrackingAuthority},
    level_chunk::LevelChunk,
};
use rivet_registry::{Identifier, RegistryAccess, core::ChunkPos};
use rivet_util::mth::Uuid;
use rivet_world::{entity::loaded_entity::NativeLoadedEntity, levelgen::structure::StructureError};

#[derive(Debug, thiserror::Error)]
pub enum LevelEntityError {
    #[error("level has no shared entity ownership binding")]
    MissingOwner,
    #[error("level has no world UUID for native post-load construction")]
    MissingWorldIdentity,
    #[error("post-load factories do not belong to the receiving world")]
    ForeignWorld,
    #[error("chunk {0} has no canonical runtime owner")]
    MissingChunk(ChunkPos),
    #[error("world entity tracking is already installed")]
    TrackingAlreadyInstalled,
    #[error("world entity tracking is not installed")]
    TrackingNotInstalled,
    #[error("world has no retained writable entity-region storage")]
    MissingEntityStorage,
    #[error("world entity-region storage is already open")]
    EntityStorageAlreadyOpen,
    #[error("chunk {0} has no actual holder FULL status binding")]
    MissingFullStatus(ChunkPos),
    #[cfg(unix)]
    #[error(transparent)]
    EntityRegion(#[from] EntityRegionError),
    #[error(transparent)]
    Tracking(#[from] EntityLookupError),
    #[error(transparent)]
    PostLoad(#[from] NativePostLoadError),
}

struct AdmissionWorld<'a, P> {
    entities: &'a mut EntityLookup,
    players: P,
    lifecycle: &'a mut dyn EntityLifecycle,
}
/// A failed unload retains values already removed from canonical indexes.
/// Remaining values still belong to EntityLookup; writes are never rolled back.
#[derive(Debug, thiserror::Error)]
#[error("entity chunk unload failed: {source}")]
pub struct EntityChunkUnloadFailure {
    pub source: LevelEntityError,
    pub removed: Vec<NativeLoadedEntity>,
    pub current: Option<Box<NativeLoadedEntity>>,
}
struct PlayerCollisionLifecycle<'a, P> {
    players: &'a P,
    runtime: &'a mut dyn EntityLifecycle,
}
impl<P: Fn(Uuid) -> Option<ExistingEntityView>> EntityLifecycle
    for PlayerCollisionLifecycle<'_, P>
{
    fn external_uuid_in_use(&self, uuid: &Uuid) -> bool {
        (self.players)(*uuid).is_some() || self.runtime.external_uuid_in_use(uuid)
    }
    fn on_event(
        &mut self,
        event: EntityLifecycleEvent,
        entity: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError> {
        self.runtime.on_event(event, entity)
    }
    fn after_event(
        &mut self,
        event: EntityLifecycleEvent,
        entity: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError> {
        self.runtime.after_event(event, entity)
    }
}

impl ServerLevel {
    #[cfg(unix)]
    pub fn open_entity_storage(
        &mut self,
        sync: bool,
        limits: EntitySaveLimits,
    ) -> Result<(), LevelEntityError> {
        if self.entity_storage.is_some() {
            return Err(LevelEntityError::EntityStorageAlreadyOpen);
        }
        let storage = self
            .writable_storage
            .as_ref()
            .ok_or(LevelEntityError::MissingEntityStorage)?;
        self.entity_storage = Some(StoredEntityRegion::open(
            std::sync::Arc::clone(storage),
            self.dimension.clone(),
            sync,
            limits,
        ));
        Ok(())
    }
    #[cfg(unix)]
    pub fn entity_storage(&self) -> Option<&StoredEntityRegion> {
        self.entity_storage.as_ref()
    }
    #[cfg(unix)]
    pub fn entity_storage_mut(&mut self) -> Option<&mut StoredEntityRegion> {
        self.entity_storage.as_mut()
    }
    #[cfg(unix)]
    pub fn save_entity_chunk(
        &mut self,
        pos: ChunkPos,
        unloading: bool,
    ) -> Result<EntitySaveOutcome, LevelEntityError> {
        let entities = self
            .entity_lookup
            .as_ref()
            .ok_or(LevelEntityError::MissingOwner)?;
        Ok(self
            .entity_storage
            .as_mut()
            .ok_or(LevelEntityError::MissingEntityStorage)?
            .save_chunk(pos, entities, unloading)?)
    }
    /// Save acknowledgement precedes every removal. The caller supplies real
    /// UNLOADED_TO_CHUNK/Cause.UNLOAD event/onRemoval/scheduler authority; this
    /// is not container discard and must not execute inventory-drop hooks.
    #[cfg(unix)]
    pub fn unload_entity_chunk_after_save(
        &mut self,
        pos: ChunkPos,
        authority: &mut dyn TrackingAuthority,
        remove: &mut dyn FnMut(&mut NativeLoadedEntity) -> Result<(), EntityLookupError>,
    ) -> Result<Vec<NativeLoadedEntity>, EntityChunkUnloadFailure> {
        let fail = |source| EntityChunkUnloadFailure {
            source,
            removed: Vec::new(),
            current: None,
        };
        // Missing lifecycle/lookup authorities must refuse before IO/effects.
        if self.entity_tracking.is_none() {
            return Err(fail(LevelEntityError::TrackingNotInstalled));
        }
        if self.entity_lookup.is_none() {
            return Err(fail(LevelEntityError::MissingOwner));
        }
        self.save_entity_chunk(pos, true).map_err(fail)?;
        self.entity_storage.as_mut().unwrap().begin_unload(pos);
        let entities = self.entity_lookup.as_mut().unwrap();
        let tracker = self.entity_tracking.as_mut().unwrap();
        let ids = entities.chunk_ids(pos).to_vec();
        let mut removed = Vec::new();
        let mut lifecycle = tracker.lifecycle(authority);
        let recovery_start = entities.interrupted_removals().len();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for id in ids {
                if entities
                    .get_raw(id)
                    .is_none_or(NativeLoadedEntity::is_removed)
                {
                    continue;
                }
                match entities
                    .remove_with_authority(id, false, &mut lifecycle, |entity| remove(entity))
                {
                    Ok(Some(entity)) => removed.push(entity),
                    Ok(None) => {}
                    Err(error) => {
                        return Err(EntityChunkUnloadFailure {
                            source: LevelEntityError::Tracking(error.error),
                            removed: std::mem::take(&mut removed),
                            current: error.entity,
                        });
                    }
                }
            }
            Ok(())
        }));
        match result {
            Ok(result) => result?,
            Err(panic) => {
                // unregister_removed retained the current unindexed value;
                // preserve this operation's completed prefix ahead of it too.
                // A panic in the removal hook itself leaves current indexed.
                entities.retain_interrupted_removals(recovery_start, removed);
                std::panic::resume_unwind(panic);
            }
        }
        entities.unload_empty_entity_chunk(pos);
        // Any remaining values are transient source slices, never overwritten
        // as if the old disk population had already been consumed again.
        self.entity_storage
            .as_mut()
            .unwrap()
            .forget_loaded_chunk(pos);
        Ok(removed)
    }
    /// Moonrise getOrCreateEntityChunk(pos,false). The disk batch is separate
    /// from proto postLoad: no duplicate-UUID regeneration and no onCreated.
    /// A failed/partial native load remains retained and cannot be redrawn on
    /// an implicit retry. Current source chunk data and status stay canonical.
    #[cfg(unix)]
    pub fn ensure_entity_chunk_loaded(
        &mut self,
        pos: ChunkPos,
        context: &mut EntityPostLoadContext<'_>,
        authority: &mut dyn TrackingAuthority,
    ) -> Result<(), LevelEntityError> {
        validate_factories(self.world_uuid, context)?;
        let status = self
            .chunk_map
            .get_chunk(pos)
            .ok_or(LevelEntityError::MissingChunk(pos))?
            .get_full_status()
            .ok_or(LevelEntityError::MissingFullStatus(pos))?;
        let storage = self
            .entity_storage
            .as_mut()
            .ok_or(LevelEntityError::MissingEntityStorage)?;
        let tracker = self
            .entity_tracking
            .as_mut()
            .ok_or(LevelEntityError::TrackingNotInstalled)?;
        let entities = self
            .entity_lookup
            .as_mut()
            .ok_or(LevelEntityError::MissingOwner)?;
        if storage.readiness(pos) == EntityChunkReadiness::Loaded {
            return Ok(());
        }
        storage.prepare_disk_load(pos)?;
        let inputs = storage.disk_inputs(pos)?;
        entities.ensure_entity_chunk(pos, status);
        storage.begin_native_load(pos);
        let mut loaded_counts = std::collections::HashMap::new();
        for (index, input) in inputs.iter().enumerate() {
            match crate::server::level::structure_entity_loading::load_native_world_entity_root(
                input,
                context.cart_factory,
                context.creature_factory,
                context.registries,
            ) {
                Ok(Some(entity)) => {
                    if storage
                        .save_limits()
                        .accepts(entity.encode_id(), &mut loaded_counts)
                    {
                        storage.prepared_entities_mut(pos)?.push(entity);
                        if let Err(source)=crate::server::level::structure_entity_loading::require_supported_passengers(input){
                            storage.fail_load(pos,source,Some(index));
                            return Err(EntityRegionError::IncompleteLoad(pos).into());
                        }
                    }
                }
                Ok(None) => {}
                Err(source) => {
                    storage.fail_load(pos, source, Some(index));
                    return Err(EntityRegionError::IncompleteLoad(pos).into());
                }
            }
        }
        let mut lifecycle = tracker.lifecycle(authority);
        match entities.add_entity_chunk_entities(
            storage.prepared_entities_mut(pos)?,
            pos,
            &mut lifecycle,
        ) {
            Ok(_) => {
                storage.finish_load(pos);
                Ok(())
            }
            Err(error) => {
                storage.fail_load(
                    pos,
                    StructureError::PlacementUnavailable(error.to_string()),
                    None,
                );
                Err(EntityRegionError::IncompleteLoad(pos).into())
            }
        }
    }
    pub fn install_entity_tracking(
        &mut self,
        access: RegistryAccess,
        config: EntityTrackingConfig,
    ) -> Result<(), LevelEntityError> {
        if self.entity_tracking.is_some() {
            return Err(LevelEntityError::TrackingAlreadyInstalled);
        }
        let world_uuid = self
            .world_uuid
            .ok_or(LevelEntityError::MissingWorldIdentity)?;
        if config.world_uuid != world_uuid {
            return Err(LevelEntityError::ForeignWorld);
        }
        self.entity_tracking = Some(EntityTrackingRuntime::new(access, config));
        Ok(())
    }
    pub fn entity_tracking(&self) -> Option<&EntityTrackingRuntime> {
        self.entity_tracking.as_ref()
    }

    /// Retains the real tracker across post-load calls; this cannot reset
    /// already-committed packet, navigation or ticking state on retry.
    pub fn run_tracked_chunk_entity_post_load(
        &mut self,
        pos: ChunkPos,
        context: &mut EntityPostLoadContext<'_>,
        players: impl Fn(Uuid) -> Option<ExistingEntityView>,
        authority: &mut dyn TrackingAuthority,
        secure_uuid: impl FnMut() -> std::io::Result<Uuid>,
    ) -> Result<Vec<DuplicateUuidDecision>, LevelEntityError> {
        validate_factories(self.world_uuid, context)?;
        let tracker = self
            .entity_tracking
            .as_mut()
            .ok_or(LevelEntityError::TrackingNotInstalled)?;
        let entities = self
            .entity_lookup
            .as_mut()
            .ok_or(LevelEntityError::MissingOwner)?;
        let chunk = self
            .chunk_map
            .get_chunk_mut(pos)
            .ok_or(LevelEntityError::MissingChunk(pos))?;
        let mut lifecycle = tracker.lifecycle(authority);
        run_post_load(
            chunk,
            entities,
            pos,
            context,
            players,
            &mut lifecycle,
            secure_uuid,
        )
    }

    /// Moonrise newTrackerTick: visit canonical loaded tracker entities, update
    /// actual observers, and send changes iff viewed or ENTITY_TICKING. This
    /// is network tracking only, not the native entity AI/movement tick.
    pub fn tick_native_entity_tracking(
        &mut self,
        authority: &mut dyn TrackingAuthority,
    ) -> Result<(), LevelEntityError> {
        let tracker = self
            .entity_tracking
            .as_mut()
            .ok_or(LevelEntityError::TrackingNotInstalled)?;
        let entities = self
            .entity_lookup
            .as_ref()
            .ok_or(LevelEntityError::MissingOwner)?;
        for id in entities.tracked_ids() {
            let entity = entities.get_raw(*id).expect("canonical tracked entity");
            tracker.tick_entity(
                entity,
                entities.visibility(*id) == Some(Visibility::Ticking),
                authority,
            )?;
        }
        Ok(())
    }
    /// Execute the actual post-load carrier once through the canonical native
    /// store. Player lookup borrows the session owner; no copied player index
    /// is installed. Caller retains errors and must not blindly retry partial
    /// registration as a transactional operation.
    pub fn run_chunk_entity_post_load(
        &mut self,
        pos: ChunkPos,
        context: &mut EntityPostLoadContext<'_>,
        players: impl Fn(Uuid) -> Option<ExistingEntityView>,
        lifecycle: &mut dyn EntityLifecycle,
        secure_uuid: impl FnMut() -> std::io::Result<Uuid>,
    ) -> Result<Vec<DuplicateUuidDecision>, LevelEntityError> {
        validate_factories(self.world_uuid, context)?;
        let entities = self
            .entity_lookup
            .as_mut()
            .ok_or(LevelEntityError::MissingOwner)?;
        let chunk = self
            .chunk_map
            .get_chunk_mut(pos)
            .ok_or(LevelEntityError::MissingChunk(pos))?;
        run_post_load(
            chunk,
            entities,
            pos,
            context,
            players,
            lifecycle,
            secure_uuid,
        )
    }
}

fn validate_factories(
    world_uuid: Option<Uuid>,
    context: &EntityPostLoadContext<'_>,
) -> Result<(), LevelEntityError> {
    let world_uuid = world_uuid.ok_or(LevelEntityError::MissingWorldIdentity)?;
    if context.creature_factory.world_uuid() != world_uuid
        || context.cart_factory.world_uuid() != Some(world_uuid)
    {
        return Err(LevelEntityError::ForeignWorld);
    }
    Ok(())
}
fn run_post_load(
    chunk: &mut LevelChunk,
    entities: &mut EntityLookup,
    pos: ChunkPos,
    context: &mut EntityPostLoadContext<'_>,
    players: impl Fn(Uuid) -> Option<ExistingEntityView>,
    lifecycle: &mut dyn EntityLifecycle,
    secure_uuid: impl FnMut() -> std::io::Result<Uuid>,
) -> Result<Vec<DuplicateUuidDecision>, LevelEntityError> {
    let initial_status = chunk.get_full_status();
    let mut world = AdmissionWorld {
        entities,
        players,
        lifecycle,
    };
    Ok(chunk.run_native_post_load(
        context,
        &mut world,
        |world, uuid| {
            (world.players)(uuid).or_else(|| {
                let entity = world.entities.get_by_uuid(&uuid)?;
                Some(ExistingEntityView {
                    encode_id: Some(Identifier::parse(entity.encode_id())),
                    position: entity.position(),
                    is_removed: entity.is_removed(),
                })
            })
        },
        secure_uuid,
        |world, batch| {
            let mut lifecycle = PlayerCollisionLifecycle {
                players: &world.players,
                runtime: world.lifecycle,
            };
            world
                .entities
                .add_worldgen_chunk_entities(
                    &mut batch.entities,
                    Some(pos),
                    initial_status,
                    &mut lifecycle,
                )
                .map(|_| ())
                .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::ServerLevelConfig;
    #[test]
    fn retained_tracker_requires_real_world_identity_and_cannot_be_rebound() {
        let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
        let mut level = ServerLevel::new(ServerLevelConfig::default());
        let uuid = Uuid { most: 1, least: 2 };
        let config = EntityTrackingConfig {
            world_uuid: uuid,
            animal_range_blocks: 48,
            other_range_blocks: 64,
            animal_range_y: None,
            other_range_y: None,
        };
        assert!(matches!(
            level.install_entity_tracking(access.clone(), config),
            Err(LevelEntityError::MissingWorldIdentity)
        ));
        assert!(level.entity_tracking().is_none());
        level.install_world_uuid(uuid);
        let foreign = EntityTrackingConfig {
            world_uuid: Uuid { most: 9, least: 8 },
            ..config
        };
        assert!(matches!(
            level.install_entity_tracking(access.clone(), foreign),
            Err(LevelEntityError::ForeignWorld)
        ));
        assert!(level.entity_tracking().is_none());
        level
            .install_entity_tracking(access.clone(), config)
            .unwrap();
        let retained = std::ptr::from_ref(level.entity_tracking().unwrap());
        assert!(matches!(
            level.install_entity_tracking(access, foreign),
            Err(LevelEntityError::TrackingAlreadyInstalled)
        ));
        assert_eq!(
            std::ptr::from_ref(level.entity_tracking().unwrap()),
            retained
        );
        assert_eq!(level.entity_tracking().unwrap().world_uuid(), uuid);
    }
}
