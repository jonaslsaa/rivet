//! Actual Moonrise ChunkFullTask ordering over the level's canonical owners.
//! Persisted FULL, loaded state, ticket status and published readiness are
//! distinct. A failed later step keeps earlier effects and retained carriers.
use super::{BlockEntityWorldServices, LevelEntityError, ServerLevel, TickRuntimeError};
use crate::server::level::{
    entity_post_load::{EntityPostLoadContext, ExistingEntityView},
    entity_save_limits::{EntitySaveLimitError, EntitySaveLimits},
    entity_tracking::TrackingAuthority,
    generated_chunk_owner::GenerationPromotionError,
    level_block_entity_runtime::BlockEntityTickerError,
};
use rivet_registry::core::ChunkPos;
use rivet_util::mth::Uuid;
use std::sync::Arc;

#[cfg(all(test, unix))]
#[path = "level_full_activation_test.rs"]
mod tests;

/// Authorities for effects not encoded in a chunk. Existing POI, entity,
/// tracker, BE and scheduler work is called directly, not replaced by these.
pub struct FullActivationContext<'entity, 'world, S: BlockEntityWorldServices> {
    pub entities: EntityPostLoadContext<'entity>,
    pub players: &'world dyn Fn(Uuid) -> Option<ExistingEntityView>,
    pub secure_uuid: &'world mut dyn FnMut() -> std::io::Result<Uuid>,
    pub tracking: &'world mut dyn TrackingAuthority,
    pub block_entities: &'world mut S,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullActivationStage {
    Poi,
    Promotion,
    HolderBinding,
    PostLoad,
    EntityChunk,
    Loaded,
    BlockEntities,
    TickContainers,
    UnsavedListener,
    Completion,
}

#[derive(Debug, thiserror::Error)]
pub enum FullActivationCause {
    #[error("world has no canonical generated chunk owner")]
    MissingGenerationOwner,
    #[error("world runtime settings have not been installed")]
    MissingRuntimeSettings,
    #[error(transparent)]
    InvalidSaveLimits(#[from] EntitySaveLimitError),
    #[error("entity load/save limit authority does not match the owning world's runtime settings")]
    ForeignSaveLimits,
    #[error("chunk {0} collides with an existing runtime owner")]
    RuntimeCollision(ChunkPos),
    #[error("canonical runtime chunk {0} is missing")]
    MissingRuntime(ChunkPos),
    #[cfg(unix)]
    #[error(transparent)]
    Poi(#[from] super::GeneratedPoiError),
    #[error(transparent)]
    Promotion(#[from] GenerationPromotionError),
    #[error(transparent)]
    Entities(#[from] LevelEntityError),
    #[error(transparent)]
    BlockEntities(#[from] BlockEntityTickerError),
    #[error(transparent)]
    Ticks(#[from] TickRuntimeError),
    #[error("FULL task panicked: {0}")]
    Panic(String),
}

#[derive(Debug, thiserror::Error)]
#[error("chunk FULL activation failed at {stage:?}: {cause}")]
pub struct FullActivationError {
    pub stage: FullActivationStage,
    pub cause: FullActivationCause,
}

#[cfg(unix)]
impl ServerLevel {
    /// Source ChunkFullTask.run. A completed call publishes actual FULL only
    /// after every owner completed its real step. An error is cached by the
    /// generation holder; explicit owner.retry is required before reattempting
    /// partially executed work, never a transaction rollback or hot retry.
    pub fn activate_generated_chunk<S: BlockEntityWorldServices>(
        &mut self,
        pos: ChunkPos,
        context: &mut FullActivationContext<'_, '_, S>,
    ) -> Result<(), Arc<FullActivationError>> {
        if let Some(owner) = &self.generated_chunk_owner {
            if owner.is_activated(pos) {
                return Ok(());
            }
            if let Some(error) = owner.full_activation_error(pos) {
                return Err(Arc::clone(error));
            }
        }
        let mut stage = FullActivationStage::Poi;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.activate_generated_chunk_inner(pos, context, &mut stage)
        }));
        let cause = match outcome {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(cause)) => cause,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|value| (*value).into()))
                    .unwrap_or_else(|| "non-string panic payload".into());
                FullActivationCause::Panic(message)
            }
        };
        let error = Arc::new(FullActivationError { stage, cause });
        if let Some(owner) = self.generated_chunk_owner.as_mut() {
            owner.record_full_activation_error(pos, Arc::clone(&error));
        }
        Err(error)
    }

    fn activate_generated_chunk_inner<S: BlockEntityWorldServices>(
        &mut self,
        pos: ChunkPos,
        context: &mut FullActivationContext<'_, '_, S>,
        stage: &mut FullActivationStage,
    ) -> Result<(), FullActivationCause> {
        let settings = self
            .runtime_settings()
            .ok_or(FullActivationCause::MissingRuntimeSettings)?;
        let ticking_enabled = settings.block_entity_ticking;
        let save_limits = EntitySaveLimits::from_entries(
            settings
                .entity_save_limits
                .iter()
                .map(|(key, value)| (key.clone(), *value)),
        )?;
        if !context.entities.save_limits.equivalent(&save_limits)
            || self
                .entity_storage
                .as_ref()
                .is_some_and(|storage| !storage.save_limits().equivalent(&save_limits))
        {
            return Err(FullActivationCause::ForeignSaveLimits);
        }
        context.entities.settings = settings.duplicate_uuid;
        let promoted = self
            .generated_chunk_owner
            .as_ref()
            .ok_or(FullActivationCause::MissingGenerationOwner)?
            .is_promoted(pos);
        *stage = FullActivationStage::Poi;
        if promoted {
            // Re-running source FULL over an Imposter still checks POI against
            // the same canonical runtime terrain, never an old proto snapshot.
            let runtime = self
                .chunk_map
                .get_chunk(pos)
                .ok_or(FullActivationCause::MissingRuntime(pos))?;
            self.poi_storage
                .as_mut()
                .ok_or(super::GeneratedPoiError::NotOpen)?
                .prepare_chunk(pos, runtime.get_sections(), |state| {
                    rivet_registry::block_state::BlockState::new(*state)
                })
                .map_err(super::GeneratedPoiError::from)?;
        } else {
            self.prepare_generated_poi(pos)?;
            *stage = FullActivationStage::Promotion;
            if self.chunk_map.get_chunk(pos).is_some() {
                return Err(FullActivationCause::RuntimeCollision(pos));
            }
            let runtime = self
                .generated_chunk_owner
                .as_mut()
                .expect("validated owner")
                .promote_spawn(pos)?;
            // Holder replacement/Imposter and canonical storage precede all
            // post-load callbacks. send readiness remains pending meanwhile.
            self.chunk_map.install(pos, runtime);
        }
        *stage = FullActivationStage::HolderBinding;
        let holder_status = self
            .generated_chunk_owner
            .as_mut()
            .expect("validated canonical owner")
            .holder_status_binding(pos);
        self.chunk_map
            .get_chunk_mut(pos)
            .expect("canonical promoted runtime")
            .bind_full_status(holder_status);

        // Pinned PaperHooks.setCurrentlyLoading is empty. The source finally
        // calls that same empty body with null; there is no native loading
        // pointer to invent or leak, including on Err/panic below.
        *stage = FullActivationStage::PostLoad;
        self.run_tracked_chunk_entity_post_load(
            pos,
            &mut context.entities,
            |uuid| (context.players)(uuid),
            context.tracking,
            || (context.secure_uuid)(),
        )?;
        *stage = FullActivationStage::EntityChunk;
        self.ensure_entity_chunk_loaded(pos, &mut context.entities, context.tracking)?;
        *stage = FullActivationStage::Loaded;
        self.chunk_map
            .get_chunk_mut(pos)
            .expect("canonical runtime")
            .set_loaded(true);
        *stage = FullActivationStage::BlockEntities;
        self.register_chunk_block_entities(pos, ticking_enabled, true, context.block_entities)?;
        *stage = FullActivationStage::TickContainers;
        self.register_chunk_ticks(pos)?;
        *stage = FullActivationStage::UnsavedListener;
        self.chunk_map
            .get_chunk_mut(pos)
            .expect("canonical runtime")
            .install_paper_unsaved_listener();
        *stage = FullActivationStage::Completion;
        // PaperHooks.chunkFullStatusComplete is also genuinely empty. Task
        // completion itself publishes the ready result only after this point.
        self.generated_chunk_owner
            .as_mut()
            .expect("canonical generation holder")
            .complete_full_activation(pos, &self.chunk_map)?;
        Ok(())
    }
}
