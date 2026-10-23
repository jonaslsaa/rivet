//! Post-load reconstruction and admission into the level's canonical owner.
//! Tracking callbacks are required; this boundary does not fabricate a tracker
//! or declare a chunk FULL merely because its entities were decoded.
use super::ServerLevel;
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
                .add_worldgen_chunk_entities(&mut batch.entities, Some(pos), &mut lifecycle)
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
