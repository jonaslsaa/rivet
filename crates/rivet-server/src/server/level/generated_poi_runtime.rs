//! The real POI storage and consistency boundary of generated FULL activation.
//! Completing this boundary alone never publishes a sendable runtime chunk.
use super::ServerLevel;
use crate::server::level::poi_storage::{PoiStorageError, StoredPoiManager};
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::status::ChunkStatus;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum GeneratedPoiError {
    #[error("generated world has no retained writable storage")]
    MissingStorage,
    #[error("world has no canonical generator")]
    MissingGenerator,
    #[error("world POI storage is already open")]
    AlreadyOpen,
    #[error("world POI storage has not been opened")]
    NotOpen,
    #[error("chunk {0} has no completed canonical SPAWN proto")]
    NotSpawn(ChunkPos),
    #[error(transparent)]
    Storage(#[from] PoiStorageError),
}

impl ServerLevel {
    /// The boot owner supplies its actual synchronous-write setting. Opening
    /// the store retains the same root lock as the generated world.
    pub fn open_poi_storage(&mut self, sync: bool) -> Result<(), GeneratedPoiError> {
        if self.poi_storage.is_some() {
            return Err(GeneratedPoiError::AlreadyOpen);
        }
        let storage = self
            .writable_storage
            .as_ref()
            .ok_or(GeneratedPoiError::MissingStorage)?;
        let owner = self
            .generated_chunk_owner
            .as_ref()
            .ok_or(GeneratedPoiError::MissingGenerator)?;
        self.poi_storage = Some(StoredPoiManager::open(
            Arc::clone(storage),
            self.dimension.clone(),
            owner.generator().registry_access().clone(),
            self.min_y >> 4,
            (self.min_y + self.height - 1) >> 4,
            sync,
        )?);
        Ok(())
    }

    /// ChunkFullTask performs this before consuming/promoting the proto.
    /// The storage owner avoids repeating a disk load on a later retry.
    pub fn prepare_generated_poi(&mut self, pos: ChunkPos) -> Result<(), GeneratedPoiError> {
        let owner = self
            .generated_chunk_owner
            .as_ref()
            .ok_or(GeneratedPoiError::MissingGenerator)?;
        let state = owner
            .state(pos)
            .filter(|state| state.status() == ChunkStatus::Spawn)
            .ok_or(GeneratedPoiError::NotSpawn(pos))?;
        self.poi_storage
            .as_mut()
            .ok_or(GeneratedPoiError::NotOpen)?
            .prepare_chunk(pos, state.chunk().get_sections(), |state| *state)?;
        Ok(())
    }

    pub fn poi_storage(&self) -> Option<&StoredPoiManager> {
        self.poi_storage.as_ref()
    }

    pub fn poi_storage_mut(&mut self) -> Option<&mut StoredPoiManager> {
        self.poi_storage.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::{
        MissingChunkPolicy, OverworldGenerator, ServerLevelConfig,
        level_storage_access::LevelStorageAccess, player_chunk_loader::ChunkSendReadiness,
    };
    use rivet_world::block::Block;

    #[test]
    fn send_preparation_repairs_real_poi_storage_from_the_canonical_proto() {
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let generator = Arc::new(OverworldGenerator::new(42));
        let config = ServerLevelConfig {
            is_flat: false,
            sea_level: generator.generator().get_sea_level(),
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..ServerLevelConfig::default()
        };
        let mut level = ServerLevel::new_generated_with_storage(
            config,
            Arc::clone(&generator),
            Arc::clone(&storage),
        )
        .unwrap();
        level.open_poi_storage(false).unwrap();
        let pos = ChunkPos::ZERO;
        let mut state = generator.create_holder(pos).into_state();
        state.chunk_mut().set_block_state(
            3,
            1,
            5,
            Block::from_name("minecraft:composter")
                .unwrap()
                .default_block_state(),
        );
        let owner = level.generated_chunk_owner_mut().unwrap();
        assert!(owner.admit(state).is_ok());
        // Explicit lifecycle-boundary fixture, not proof of world generation.
        // The test exercises the real send caller, registry, region store and
        // canonical proto without repeating the entire generation pipeline.
        owner
            .state_mut(pos)
            .unwrap()
            .chunk_mut()
            .set_persisted_status(ChunkStatus::Spawn);
        assert!(matches!(
            level.prepare_chunk_for_send(pos),
            Ok(ChunkSendReadiness::Pending)
        ));
        assert!(
            level.chunk_map().is_empty(),
            "POI work alone cannot publish FULL"
        );
        let poi = level.poi_storage_mut().unwrap();
        assert!(poi.has_chunk(pos));
        assert!(poi.manager_mut().is_dirty(pos).unwrap());
        let saved = poi
            .manager_mut()
            .save(pos, |_, message| panic!("{message}"))
            .unwrap()
            .unwrap();
        let section = saved
            .get_compound("Sections")
            .unwrap()
            .get_compound("0")
            .unwrap();
        let records = section.get_list("Records").unwrap();
        assert_eq!(records.list.len(), 1);
        poi.save_chunk(pos).unwrap();
        assert!(!poi.manager_mut().is_dirty(pos).unwrap());
        level.prepare_generated_poi(pos).unwrap();
        assert!(
            !level
                .poi_storage_mut()
                .unwrap()
                .manager_mut()
                .is_dirty(pos)
                .unwrap(),
            "retry does not reinstall or rebuild an already valid section"
        );
        level.promote_generated_chunk(pos).unwrap();
        assert!(level.chunk_map().get_chunk(pos).is_some());
        assert!(level.generated_chunk_owner().unwrap().is_promoted(pos));
        assert!(matches!(
            level.prepare_chunk_for_send(pos),
            Ok(ChunkSendReadiness::Pending)
        ));
        assert!(
            level.promote_generated_chunk(pos).is_err(),
            "promotion must not replace the canonical runtime on retry"
        );
    }
}
