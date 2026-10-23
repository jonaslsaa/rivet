//! Storage-backed canonical POI ownership. Successful region reads distinguish
//! confirmed absence from unavailable/corrupt IO. FULL preparation performs
//! real POI load and block consistency but does not publish a runtime chunk.
use super::{
    level_storage_access::LevelStorageAccess,
    poi_chunk::PoiChunk,
    poi_manager::{PoiManager, PoiManagerError},
};
use rivet_nbt::nbt_utils::CURRENT_DATA_VERSION;
use rivet_registry::{
    RegistryAccess, ResourceKey,
    block_state::BlockState,
    core::{ChunkPos, SectionPos},
    registries::Level,
};
use rivet_world::chunk::{
    level_chunk_section::LevelChunkSection,
    storage::{RegionFileStorage, RegionFileVersion, RegionStorageInfo},
};
use std::{io, sync::Arc};

#[cfg(test)]
#[path = "poi_storage_test.rs"]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum PoiStorageError {
    #[error(transparent)]
    Manager(#[from] PoiManagerError),
    #[error("POI storage IO for {position:?}: {source}")]
    Io {
        position: Option<ChunkPos>,
        #[source]
        source: io::Error,
    },
    #[error("POI chunk {pos} needs data conversion from version {stored} to {current}")]
    DataConversionUnavailable {
        pos: ChunkPos,
        stored: i32,
        current: i32,
    },
    #[error("POI consistency for {pos} expected {expected} canonical sections, got {actual}")]
    SectionCountMismatch {
        pos: ChunkPos,
        expected: usize,
        actual: usize,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoiSaveOutcome {
    SkippedClean,
    Written,
    Deleted,
}

pub struct StoredPoiManager {
    manager: PoiManager,
    region: RegionFileStorage,
    storage: Arc<LevelStorageAccess>,
    dimension: ResourceKey<Level>,
    min_section: i32,
    max_section: i32,
}
impl StoredPoiManager {
    pub fn open(
        storage: Arc<LevelStorageAccess>,
        dimension: ResourceKey<Level>,
        access: RegistryAccess,
        min_section: i32,
        max_section: i32,
        sync: bool,
    ) -> Result<Self, PoiStorageError> {
        Self::open_with_version(
            storage,
            dimension,
            access,
            min_section,
            max_section,
            sync,
            RegionFileVersion::get_selected(),
        )
    }
    /// Snapshots the actual startup compression choice, never silently changes
    /// an unsupported selected writer to a different codec.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_version(
        storage: Arc<LevelStorageAccess>,
        dimension: ResourceKey<Level>,
        access: RegistryAccess,
        min_section: i32,
        max_section: i32,
        sync: bool,
        version: RegionFileVersion,
    ) -> Result<Self, PoiStorageError> {
        let manager = PoiManager::new(access, min_section, max_section)?;
        let folder = storage.dimension_path(&dimension).join("poi");
        let info = RegionStorageInfo::new(
            dimension.identifier().to_string(),
            dimension.clone(),
            "poi".into(),
            false,
        );
        Ok(Self {
            manager,
            region: RegionFileStorage::new_with_version(info, folder, sync, version),
            storage,
            dimension,
            min_section,
            max_section,
        })
    }
    pub fn belongs_to(
        &self,
        storage: &Arc<LevelStorageAccess>,
        dimension: &ResourceKey<Level>,
    ) -> bool {
        Arc::ptr_eq(&self.storage, storage) && &self.dimension == dimension
    }
    pub fn manager_mut(&mut self) -> &mut PoiManager {
        &mut self.manager
    }
    pub fn has_chunk(&self, pos: ChunkPos) -> bool {
        self.manager.has_chunk(pos)
    }
    /// Loads exactly once into canonical ownership. An IO/version refusal does
    /// not install an empty replacement and remains retryable on this owner.
    pub fn load_chunk(&mut self, pos: ChunkPos) -> Result<(), PoiStorageError> {
        if self.manager.has_chunk(pos) {
            return Ok(());
        }
        // RegionFileStorage's existing-only cache uses Path::exists, which
        // collapses metadata failures to false. Establish that this read is
        // not a permission/path-traversal failure before accepting its None.
        let path = self
            .storage
            .dimension_path(&self.dimension)
            .join("poi")
            .join(RegionFileStorage::get_region_file_name(pos.x(), pos.z()));
        if let Err(source) = std::fs::metadata(path)
            && source.kind() != io::ErrorKind::NotFound
        {
            return Err(PoiStorageError::Io {
                position: Some(pos),
                source,
            });
        }
        let tag = self
            .region
            .read(&pos)
            .map_err(|source| PoiStorageError::Io {
                position: Some(pos),
                source,
            })?;
        if let Some(tag) = &tag
            && !tag.is_empty()
        {
            // Moonrise's DEFAULT_POI_DATA_VERSION is 1945. A native data
            // fixer is not yet available; missing/old versions must not be
            // relabelled current or mistaken for genuinely absent storage.
            let stored = tag.get_int_or("DataVersion", 1945);
            // Paper's POI converters are historical, with no POI hooks or
            // walkers. At/above current the real conversion is a no-op;
            // source does not reject future data or downgrade its tag.
            if stored < CURRENT_DATA_VERSION {
                return Err(PoiStorageError::DataConversionUnavailable {
                    pos,
                    stored,
                    current: CURRENT_DATA_VERSION,
                });
            }
        }
        self.manager.install_from_storage(pos,tag.as_ref(),|section,error|tracing::error!(%pos,section,%error,"POI decode retained source codec partial"))?;
        Ok(())
    }
    pub fn prepare_chunk<T, B>(
        &mut self,
        pos: ChunkPos,
        sections: &[LevelChunkSection<T, B>],
        state: impl Fn(&T) -> BlockState,
    ) -> Result<(), PoiStorageError>
    where
        T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    {
        let expected = (i64::from(self.max_section) - i64::from(self.min_section) + 1) as usize;
        if sections.len() != expected {
            return Err(PoiStorageError::SectionCountMismatch {
                pos,
                expected,
                actual: sections.len(),
            });
        }
        self.load_chunk(pos)?;
        for (offset, section) in sections.iter().enumerate() {
            self.manager.check_consistency_with_blocks(
                SectionPos::of_chunk_pos(&pos, self.min_section + offset as i32),
                section,
                &state,
            )?;
        }
        Ok(())
    }
    /// Dirty is cleared only after the actual write/delete succeeds. Failed
    /// compression, IO or close of the chunk stream leaves the canonical POI
    /// owner dirty for retry; absence of a serialized tag means real deletion.
    pub fn save_chunk(&mut self, pos: ChunkPos) -> Result<PoiSaveOutcome, PoiStorageError> {
        if !self.manager.is_dirty(pos)? {
            return Ok(PoiSaveOutcome::SkippedClean);
        }
        let tag=self.manager.save(pos,|section,error|tracing::error!(%pos,section,%error,"POI encode retained source codec partial"))?;
        self.region
            .write_ref(&pos, tag.as_ref())
            .map_err(|source| PoiStorageError::Io {
                position: Some(pos),
                source,
            })?;
        self.manager.mark_saved(pos)?;
        Ok(if tag.is_some() {
            PoiSaveOutcome::Written
        } else {
            PoiSaveOutcome::Deleted
        })
    }
    /// The caller's chunk eviction uses this real save boundary. On failure no
    /// canonical POI chunk or distance source is removed.
    pub fn unload_after_save(&mut self, pos: ChunkPos) -> Result<PoiChunk, PoiStorageError> {
        self.save_chunk(pos)?;
        self.manager
            .take_after_save(pos)
            .ok_or(PoiManagerError::MissingChunk(pos).into())
    }
    /// Durability for already-enqueued synchronous writes, not an implicit
    /// autosave of every dirty manager chunk.
    pub fn flush(&mut self) -> Result<(), PoiStorageError> {
        self.region.flush().map_err(|source| PoiStorageError::Io {
            position: None,
            source,
        })
    }
    pub fn close(&mut self) -> Result<(), PoiStorageError> {
        self.region.close().map_err(|source| PoiStorageError::Io {
            position: None,
            source,
        })
    }
}
