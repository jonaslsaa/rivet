//! ChunkStatusTasks.postLoadProtoChunk: construct and apply duplicate policy
//! to the complete source stream before the entity lookup admits any value.
//! This module does not implement tracking, AI, spawn events or default config.
use super::{
    generation_creature_runtime::GenerationCreatureFactory,
    structure_entity_loading::{load_native_world_entity_root, require_supported_passengers},
    structure_placement_adapter::StructureEntityCreation,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{Identifier, RegistryAccess, core::Vec3};
use rivet_util::mth::Uuid;
use rivet_world::{entity::loaded_entity::NativeLoadedEntity, levelgen::structure::StructureError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateUuidMode {
    SafeRegen,
    Delete,
    Warn,
    Nothing,
}

/// Explicit world configuration authority. No implicit Paper-default mode or
/// range is supplied when the owning world's configuration is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateUuidSettings {
    pub mode: DuplicateUuidMode,
    pub safe_regen_delete_range: i32,
}

/// Borrowed lookup projection of an actual existing world entity, including
/// removed-but-not-yet-unregistered values. Health-zero is not isRemoved.
#[derive(Debug, Clone, PartialEq)]
pub struct ExistingEntityView {
    pub encode_id: Option<Identifier>,
    pub position: Vec3,
    pub is_removed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateUuidDecision {
    Keep,
    Regenerated,
    /// Source SAFE_REGEN near-duplicate calls discard(null).
    DiscardWithoutCause,
    /// Source DELETE calls discard(EntityRemoveEvent.Cause.DISCARD).
    Discard,
}
impl DuplicateUuidDecision {
    fn is_discard(self) -> bool {
        matches!(self, Self::DiscardWithoutCause | Self::Discard)
    }
}

/// Removal is executable world behavior, not an isRemoved assignment. In
/// particular minecart containers drop inventory, even empty slots draw RNG.
pub trait EntityRemoval {
    fn discard(
        &mut self,
        entity: &mut NativeLoadedEntity,
        decision: DuplicateUuidDecision,
    ) -> Result<(), StructureError>;
}

/// Supported detached animals have no passengers, leash, equipment or indexed
/// level callback. Their source discard has no container-removal override.
pub struct DetachedAnimalRemoval;
impl EntityRemoval for DetachedAnimalRemoval {
    fn discard(
        &mut self,
        entity: &mut NativeLoadedEntity,
        decision: DuplicateUuidDecision,
    ) -> Result<(), StructureError> {
        if decision == DuplicateUuidDecision::Discard && !entity.generation() {
            return Err(StructureError::PlacementUnavailable(
                "DELETE removal requires actual EntityRemoveEvent authority".into(),
            ));
        }
        match entity {
            NativeLoadedEntity::Creature(_) => {
                entity.discard();
                Ok(())
            }
            NativeLoadedEntity::ChestMinecart(_) => Err(StructureError::PlacementUnavailable(
                "chest minecart removal requires world inventory-drop authority".into(),
            )),
        }
    }
}

/// Concrete source removal for the currently supported empty, lootless cart
/// inventory. The caller must lend the actual world's random source, never
/// the constructor UUID stream or a feature RNG. Loot-table unpacking/items
/// require their genuine runtime and remain explicitly unavailable.
pub struct EmptyInventoryEntityRemoval<'a, R: rivet_util::RandomSource> {
    pub world_random: &'a mut R,
}
impl<R: rivet_util::RandomSource> EmptyInventoryEntityRemoval<'_, R> {
    /// Source calls the removal event after container drops and before the
    /// removed flag. This requires a real event dispatcher from the caller;
    /// passing a recording sink is appropriate only in an oracle test.
    pub fn discard_with_event_authority(
        &mut self,
        entity: &mut NativeLoadedEntity,
        decision: DuplicateUuidDecision,
        event: &mut dyn FnMut(&NativeLoadedEntity) -> Result<(), StructureError>,
    ) -> Result<(), StructureError> {
        if let NativeLoadedEntity::ChestMinecart(cart) = entity {
            if cart.loot_table().is_some() {
                return Err(StructureError::PlacementUnavailable(
                    "chest minecart removal requires actual loot-table unpacking".into(),
                ));
            }
            for _ in 0..27 {
                for _ in 0..3 {
                    self.world_random.next_double();
                }
            }
        }
        if decision == DuplicateUuidDecision::Discard && !entity.generation() {
            event(entity)?;
        }
        entity.discard();
        Ok(())
    }
}
impl<R: rivet_util::RandomSource> EntityRemoval for EmptyInventoryEntityRemoval<'_, R> {
    fn discard(
        &mut self,
        entity: &mut NativeLoadedEntity,
        decision: DuplicateUuidDecision,
    ) -> Result<(), StructureError> {
        if decision == DuplicateUuidDecision::Discard && !entity.generation() {
            return Err(StructureError::PlacementUnavailable(
                "DELETE removal requires actual EntityRemoveEvent authority".into(),
            ));
        }
        if let NativeLoadedEntity::ChestMinecart(cart) = entity {
            if cart.loot_table().is_some() {
                return Err(StructureError::PlacementUnavailable(
                    "chest minecart removal requires actual loot-table unpacking".into(),
                ));
            }
            // Containers.dropContents calls dropItemStack for all 27 empty
            // slots; each computes xyz offsets before checking isEmpty.
            for _ in 0..27 {
                for _ in 0..3 {
                    self.world_random.next_double();
                }
            }
            entity.discard();
            Ok(())
        } else {
            DetachedAnimalRemoval.discard(entity, decision)
        }
    }
}

/// Source policy checks only the already-admitted world lookup. Previously
/// reconstructed elements in the same stream must not appear in `lookup`.
/// This seam accepts newly constructed detached values, so the source's
/// `other == entity` branch cannot occur. Numeric IDs are not object identity.
pub fn check_duplicate_uuid(
    entity: &mut NativeLoadedEntity,
    settings: DuplicateUuidSettings,
    lookup: &mut impl FnMut(Uuid) -> Option<ExistingEntityView>,
    secure_uuid: &mut impl FnMut() -> std::io::Result<Uuid>,
    removal: &mut dyn EntityRemoval,
) -> Result<DuplicateUuidDecision, StructureError> {
    if settings.mode == DuplicateUuidMode::Nothing {
        return Ok(DuplicateUuidDecision::Keep);
    }
    let Some(other) = lookup(entity.uuid()) else {
        return Ok(DuplicateUuidDecision::Keep);
    };
    if settings.mode == DuplicateUuidMode::SafeRegen
        && !other.is_removed
        && other
            .encode_id
            .as_ref()
            .is_some_and(|id| id.to_string() == entity.encode_id())
    {
        let position = entity.position();
        let dx = position.x - other.position.x;
        let dy = position.y - other.position.y;
        let dz = position.z - other.position.z;
        if (dx * dx + dy * dy + dz * dz).sqrt() < f64::from(settings.safe_regen_delete_range) {
            removal.discard(entity, DuplicateUuidDecision::DiscardWithoutCause)?;
            return Ok(DuplicateUuidDecision::DiscardWithoutCause);
        }
    }
    if !other.is_removed {
        match settings.mode {
            DuplicateUuidMode::SafeRegen => {
                entity.set_uuid(secure_uuid().map_err(|error| {
                    StructureError::PlacementUnavailable(format!(
                        "secure duplicate UUID generation: {error}"
                    ))
                })?);
                return Ok(DuplicateUuidDecision::Regenerated);
            }
            DuplicateUuidMode::Delete => {
                removal.discard(entity, DuplicateUuidDecision::Discard)?;
                return Ok(DuplicateUuidDecision::Discard);
            }
            DuplicateUuidMode::Warn | DuplicateUuidMode::Nothing => {}
        }
    }
    Ok(DuplicateUuidDecision::Keep)
}

#[derive(Debug)]
pub struct PreparedEntityBatch {
    /// Source encounter order, with invalid loads and policy discards omitted.
    pub entities: Vec<NativeLoadedEntity>,
    /// Policy-visited encounter order after load limits, including discards.
    pub decisions: Vec<DuplicateUuidDecision>,
}

pub struct EntityPostLoadContext<'a> {
    pub save_limits: &'a super::entity_save_limits::EntitySaveLimits,
    pub cart_factory: &'a dyn StructureEntityCreation,
    pub creature_factory: &'a dyn GenerationCreatureFactory,
    pub registries: &'a RegistryAccess,
    pub settings: DuplicateUuidSettings,
    pub removal: &'a mut dyn EntityRemoval,
}

#[derive(Debug, thiserror::Error)]
pub enum NativePostLoadError {
    #[error(transparent)]
    Prepare(#[from] Box<EntityPostLoadFailure>),
    #[error("native entity admission failed: {source}")]
    Admission {
        source: StructureError,
        batch: Box<PreparedEntityBatch>,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("native entity post-load failed at source index {input_index}: {source}")]
pub struct EntityPostLoadFailure {
    pub input_index: usize,
    pub source: StructureError,
    /// Retain already-constructed values on unavailable support. No admission
    /// happened, but identity/RNG draws are real and cannot be rolled back.
    pub prepared: PreparedEntityBatch,
    /// A policy/removal failure happens after constructing this actual value.
    pub current: Option<Box<NativeLoadedEntity>>,
}

pub fn prepare_native_entities(
    inputs: &[CompoundTag],
    context: &mut EntityPostLoadContext<'_>,
    mut lookup: impl FnMut(Uuid) -> Option<ExistingEntityView>,
    mut secure_uuid: impl FnMut() -> std::io::Result<Uuid>,
) -> Result<PreparedEntityBatch, Box<EntityPostLoadFailure>> {
    let mut prepared = PreparedEntityBatch {
        entities: Vec::new(),
        decisions: Vec::new(),
    };
    let mut loaded_counts = std::collections::HashMap::new();
    for (input_index, input) in inputs.iter().enumerate() {
        let mut entity = match load_native_world_entity_root(
            input,
            context.cart_factory,
            context.creature_factory,
            context.registries,
        ) {
            Ok(Some(entity)) => entity,
            Ok(None) => continue,
            Err(source) => {
                return Err(Box::new(EntityPostLoadFailure {
                    input_index,
                    source,
                    prepared,
                    current: None,
                }));
            }
        };
        if !context
            .save_limits
            .accepts(entity.encode_id(), &mut loaded_counts)
        {
            continue;
        }
        let decision = match check_duplicate_uuid(
            &mut entity,
            context.settings,
            &mut lookup,
            &mut secure_uuid,
            context.removal,
        ) {
            Ok(decision) => decision,
            Err(error) => {
                return Err(Box::new(EntityPostLoadFailure {
                    input_index,
                    source: error,
                    prepared,
                    current: Some(Box::new(entity)),
                }));
            }
        };
        prepared.decisions.push(decision);
        // mapMulti emits the root to the downstream duplicate-policy filter
        // during postLoad, then EntityType recurses its passengers. A limit-
        // rejected root never reaches either of those subsequent operations.
        if let Err(source) = require_supported_passengers(input) {
            return Err(Box::new(EntityPostLoadFailure {
                input_index,
                source,
                prepared,
                current: Some(Box::new(entity)),
            }));
        }
        if !decision.is_discard() {
            prepared.entities.push(entity);
        }
    }
    Ok(prepared)
}
