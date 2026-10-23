//! Post-load reconstruction and admission into the level's canonical owner.
//! Tracking callbacks are required; this boundary does not fabricate a tracker
//! or declare a chunk FULL merely because its entities were decoded.
use super::ServerLevel;
use crate::server::level::{
    entity_lookup::{EntityLifecycle, EntityLookup},
    entity_post_load::{
        DuplicateUuidDecision, EntityPostLoadContext, ExistingEntityView, NativePostLoadError,
    },
};
use rivet_registry::{Identifier, core::ChunkPos};
use rivet_util::mth::Uuid;
use rivet_world::levelgen::structure::StructureError;

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
    #[error(transparent)]
    PostLoad(#[from] NativePostLoadError),
}

struct AdmissionWorld<'a, P> {
    entities: &'a mut EntityLookup,
    players: P,
    lifecycle: &'a mut dyn EntityLifecycle,
}

impl ServerLevel {
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
        let world_uuid = self
            .world_uuid
            .ok_or(LevelEntityError::MissingWorldIdentity)?;
        if context.creature_factory.world_uuid() != world_uuid
            || context.cart_factory.world_uuid() != Some(world_uuid)
        {
            return Err(LevelEntityError::ForeignWorld);
        }
        let entities = self
            .entity_lookup
            .as_mut()
            .ok_or(LevelEntityError::MissingOwner)?;
        let chunk = self
            .chunk_map
            .get_chunk_mut(pos)
            .ok_or(LevelEntityError::MissingChunk(pos))?;
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
                world
                    .entities
                    .add_worldgen_chunk_entities(&mut batch.entities, Some(pos), world.lifecycle)
                    .map(|_| ())
                    .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))
            },
        )?)
    }
}
