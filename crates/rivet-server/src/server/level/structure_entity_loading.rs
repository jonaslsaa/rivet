//! Actual EntityType.loadEntityRecursive LOAD,false boundary for the native
//! chest-minecart family. Other valid families/real passenger graphs refuse;
//! unknown IDs and genuine load failures are logged skips, as in Paper.
use super::structure_placement_adapter::StructureEntityCreation;
use rivet_nbt::{compound_tag::CompoundTag, nbt_ops::NbtOps};
use rivet_world::{
    entity::{
        EntityType,
        chest_minecart::{MinecartLoadError, NativeChestMinecart},
    },
    levelgen::structure::StructureError,
};

#[cfg(test)]
#[path = "structure_entity_loading_test.rs"]
mod tests;

pub fn load_native_entity(
    input: &CompoundTag,
    factory: &dyn StructureEntityCreation,
) -> Result<Option<NativeChestMinecart>, StructureError> {
    let entity_type = input.get("id").and_then(|id| {
        EntityType::codec::<NbtOps>()
            .parse(&NbtOps, id)
            .result_or_partial_silent()
    });
    let Some(entity_type) = entity_type else {
        tracing::warn!(id=?input.get_string("id"),"Skipping entity with invalid or unregistered id");
        return Ok(None);
    };
    if entity_type.get_key() != "minecraft:chest_minecart" {
        return Err(StructureError::PlacementUnavailable(format!(
            "EntityType.loadEntityRecursive for {} is unavailable",
            entity_type.get_key()
        )));
    }
    // Constructor gating and identity consumption precede load, even when an
    // input UUID replaces the constructor's freshly generated UUID.
    let Some(mut entity) = factory.create_chest_minecart()? else {
        return Ok(None);
    };
    match entity.load(input) {
        Ok(()) => Ok(Some(entity)),
        Err(MinecartLoadError::Invalid(message)) => {
            tracing::warn!(%message,"Exception loading entity");
            Ok(None)
        }
        Err(error @ MinecartLoadError::Unsupported(_)) => {
            Err(StructureError::PlacementUnavailable(error.to_string()))
        }
    }
}

/// FULL postLoad's supported native dispatch. Construction ALWAYS precedes
/// Entity.load, even when input restores an existing UUID. LimitedRegion's
/// existing cart-only return contract remains separate above.
pub fn load_native_world_entity(
    input: &CompoundTag,
    cart_factory: &dyn StructureEntityCreation,
    creature_factory: &dyn super::generation_creature_runtime::GenerationCreatureFactory,
    registries: &rivet_registry::RegistryAccess,
) -> Result<Option<rivet_world::entity::loaded_entity::NativeLoadedEntity>, StructureError> {
    use rivet_world::entity::{
        generation_creature::CreatureLoadError, loaded_entity::NativeLoadedEntity,
    };
    let entity_type = input.get("id").and_then(|id| {
        EntityType::codec::<NbtOps>()
            .parse(&NbtOps, id)
            .result_or_partial_silent()
    });
    let Some(entity_type) = entity_type else {
        tracing::warn!(id=?input.get_string("id"),"Skipping entity with invalid or unregistered id");
        return Ok(None);
    };
    if entity_type.get_key() == "minecraft:chest_minecart" {
        return load_native_entity(input, cart_factory)
            .map(|e| e.map(NativeLoadedEntity::ChestMinecart));
    }
    let Some(mut entity) = creature_factory
        .create(entity_type.get_key(), registries)
        .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))?
    else {
        return Ok(None);
    };
    match entity.load(input) {
        Ok(()) => Ok(Some(NativeLoadedEntity::Creature(entity))),
        Err(CreatureLoadError::Invalid(message)) => {
            tracing::warn!(%message,"Exception loading entity");
            Ok(None)
        }
        Err(error @ CreatureLoadError::Unsupported(_)) => {
            Err(StructureError::PlacementUnavailable(error.to_string()))
        }
    }
}
