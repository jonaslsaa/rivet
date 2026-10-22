//! Server-wide entity identity state, separate from deterministic worldgen RNG.
//! Paper's Entity.SHARED_RANDOM uses the same 48-bit LCG/nextLong sequence as
//! LegacyRandomSource. No reseeding surface is exposed after construction.
use super::entity_id_allocator::EntityIdSequence;
use super::structure_placement_adapter::StructureEntityCreation;
use rivet_util::{
    mth::{Uuid, create_insecure_uuid},
    random::{LegacyRandomSource, random_support},
};
use rivet_world::{
    entity::chest_minecart::{EntityConstructionContext, NativeChestMinecart},
    flag::{feature_flag_set::FeatureFlagSet, feature_flags},
    levelgen::structure::StructureError,
};
use std::sync::{Arc, Mutex};

/// Clones share both the existing server ID sequence and entity UUID stream.
/// The owning server installs one of these across its generated worlds.
#[derive(Clone)]
pub struct EntityConstructionRuntime {
    ids: EntityIdSequence,
    random: Arc<Mutex<LegacyRandomSource>>,
}

#[derive(Debug, thiserror::Error)]
#[error("shared entity random source was poisoned")]
pub struct EntityConstructionError;

/// The world-bound CHEST_MINECART factory. The caller supplies actual enabled
/// features and a live destination-level lookup, never an assumed empty store.
pub struct StructureMinecartFactory {
    runtime: EntityConstructionRuntime,
    world_uuid: Uuid,
    enabled_features: FeatureFlagSet,
    is_in_use: Arc<dyn Fn(i32) -> bool + Send + Sync>,
}

impl StructureMinecartFactory {
    pub fn new(
        runtime: EntityConstructionRuntime,
        world_uuid: Uuid,
        enabled_features: FeatureFlagSet,
        is_in_use: Arc<dyn Fn(i32) -> bool + Send + Sync>,
    ) -> Self {
        Self {
            runtime,
            world_uuid,
            enabled_features,
            is_in_use,
        }
    }
}

impl StructureEntityCreation for StructureMinecartFactory {
    fn require_creation_authority(&self) -> Result<(), StructureError> {
        // Construction requires an explicit world identity, enabled flags,
        // shared server identity runtime and destination occupancy callback.
        // A poisoned RNG is unavailable even before the first terrain write.
        if self.runtime.random.is_poisoned() {
            return Err(StructureError::PlacementUnavailable(
                EntityConstructionError.to_string(),
            ));
        }
        Ok(())
    }

    fn create_chest_minecart(&self) -> Result<Option<NativeChestMinecart>, StructureError> {
        // EntityTypes.CHEST_MINECART uses Builder's VANILLA_SET default and
        // MISC is allowed in peaceful. The feature gate precedes construction.
        if !feature_flags::vanilla_set().is_subset_of(&self.enabled_features) {
            return Ok(None);
        }
        let (uuid, entity_id) = self
            .runtime
            .allocate(|id| (self.is_in_use)(id))
            .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))?;
        Ok(Some(NativeChestMinecart::from_identity(
            EntityConstructionContext {
                entity_id,
                world_uuid: self.world_uuid,
                experimental_minecart_movement: self
                    .enabled_features
                    .contains(&feature_flags::MINECART_IMPROVEMENTS),
            },
            uuid,
        )))
    }
}

impl EntityConstructionRuntime {
    /// Continue actual Entity.SHARED_RANDOM construction after UUID and ID
    /// initialization. LivingEntity and subtype constructor draws belong here,
    /// never to the population or WorldGenRegion random sources.
    pub(super) fn with_entity_random<T>(
        &self,
        body: impl FnOnce(&mut LegacyRandomSource) -> T,
    ) -> Result<T, EntityConstructionError> {
        let mut random = self.random.lock().map_err(|_| EntityConstructionError)?;
        Ok(body(&mut random))
    }
    pub fn new(ids: EntityIdSequence) -> Self {
        Self::with_seed(ids, random_support::generate_unique_seed())
    }

    /// Explicit seed injection for replay/oracle comparisons, never a world seed.
    pub fn with_seed(ids: EntityIdSequence, entity_seed: i64) -> Self {
        Self {
            ids,
            random: Arc::new(Mutex::new(LegacyRandomSource::new(entity_seed))),
        }
    }

    /// Entity's UUID field initializer precedes getNextEntityId in its
    /// constructor. Failed admission does not rewind either stream. The caller
    /// supplies the destination world's real live-entity lookup.
    pub fn allocate(
        &self,
        is_in_use: impl FnMut(i32) -> bool,
    ) -> Result<(Uuid, i32), EntityConstructionError> {
        let uuid = {
            let mut random = self.random.lock().map_err(|_| EntityConstructionError)?;
            create_insecure_uuid(&mut *random)
        };
        Ok((uuid, self.ids.next_id(is_in_use)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::entity_id_allocator::EntityIdAllocator;

    #[test]
    fn disabled_factory_consumes_no_identity_and_enabled_factory_queries_destination() {
        let mut players = EntityIdAllocator::new();
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let world_uuid = Uuid { most: 7, least: 9 };
        let disabled = StructureMinecartFactory::new(
            runtime.clone(),
            world_uuid,
            FeatureFlagSet::of(),
            Arc::new(|_| panic!("disabled factory queried entities")),
        );
        assert!(disabled.create_chest_minecart().unwrap().is_none());
        let occupancy = players.occupancy();
        let enabled = StructureMinecartFactory::new(
            runtime,
            world_uuid,
            feature_flags::vanilla_set(),
            Arc::new(move |id| occupancy.contains(id)),
        );
        // A live admission after factory construction must be observed.
        players.mark_in_use(1);
        let entity = enabled.create_chest_minecart().unwrap().unwrap();
        assert_eq!(entity.entity_id(), 2);
        assert_eq!(entity.world_uuid(), world_uuid);
        assert_eq!(
            entity.uuid(),
            create_insecure_uuid(&mut LegacyRandomSource::new(42))
        );
        assert_eq!(players.next_id(), 3);
    }

    #[test]
    fn shared_identity_stream_is_not_rolled_back_after_failed_construction() {
        let mut players = EntityIdAllocator::new();
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let other_world = runtime.clone();
        let first = runtime.allocate(|_| false).unwrap();
        assert_eq!(first.1, 1);
        assert_eq!(players.next_id(), 2);
        // Simulate a failed destination lookup after UUID initialization.
        assert!(
            std::panic::catch_unwind(|| other_world.allocate(|_| panic!("lookup failed"))).is_err()
        );
        let after_failure = runtime.allocate(|_| false).unwrap();
        assert_eq!(after_failure.1, 4);
        let mut expected = LegacyRandomSource::new(42);
        assert_eq!(first.0, create_insecure_uuid(&mut expected));
        let _consumed = create_insecure_uuid(&mut expected);
        assert_eq!(after_failure.0, create_insecure_uuid(&mut expected));
    }
}
