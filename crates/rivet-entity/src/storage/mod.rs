//! M3 current-format entity persistence and loaded-section storage (Paper 26.2).
//!
//! Ports the Paper 26.2 entity persistence and loaded-section lifecycle layer:
//! - [`Visibility`] and [`FullChunkStatus`] (`net.minecraft.world.level.entity.Visibility`)
//! - [`EntityLookup`] (`net.minecraft.world.level.entity.EntityLookup`)
//! - [`EntitySection`] and [`EntitySectionStorage`] (`net.minecraft.world.level.entity.EntitySection*`)
//! - [`StoredEntityRecord`], [`EntitySaveLimits`], [`NetworkIdAllocator`], and current-format
//!   NBT codecs (`Entity.save`/`saveAsPassenger`/`load`, `EntityType.loadEntitiesRecursive`)
//! - [`ChunkEntities`], [`EntityRegionBackend`], and [`EntityStorage`] (`net.minecraft.world.level.chunk.storage.EntityStorage`)
//! - [`PersistentEntitySectionManager`] (`net.minecraft.world.level.entity.PersistentEntitySectionManager`)

pub mod chunk_entities;
pub mod entity_lookup;
pub mod entity_record;
pub mod entity_section;
pub mod section_manager;
pub mod visibility;

#[cfg(test)]
mod tests;

pub use chunk_entities::{
    ChunkEntities, EntityRegionBackend, EntityStorage, InMemoryEntityRegionBackend,
};
pub use entity_lookup::EntityLookup;
pub use entity_record::{
    Aabb, EntityRemovalReason, EntitySaveLimits, EntityStorageError, MAX_HORIZONTAL_POSITION,
    MAX_MOTION_COMPONENT, MAX_VERTICAL_POSITION, NetworkIdAllocator, StoredEntityRecord,
    TAG_DATA_VERSION, TAG_ENTITIES, TAG_ID, TAG_PASSENGERS, TAG_POSITION, TAG_UUID,
    decode_chunk_entities_nbt, encode_chunk_entities_nbt,
};
pub use entity_section::{
    CHONKY_ENTITY_SEARCH_GRACE, EntitySection, EntitySectionStorage, IterationControl,
    MAX_NON_CHONKY_ENTITY_SIZE,
};
pub use section_manager::{ChunkLoadStatus, EntityLifecycleEvent, PersistentEntitySectionManager};
pub use visibility::{FullChunkStatus, Visibility};
