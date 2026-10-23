//! Writable world-root ownership from LevelStorageSource.LevelStorageAccess.
//! This is the storage/identity slice, not level.dat bootstrap or a claim that
//! generated chunks are playable. Read-only imported worlds do not use it.
use super::{directory_lock::DirectoryLock, world_identity};
use rivet_registry::{ResourceKey, registries::Level};
use rivet_util::mth::Uuid;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct LevelStorageAccess {
    lock: DirectoryLock,
}

impl LevelStorageAccess {
    pub fn open(root: &Path) -> io::Result<Self> {
        Ok(Self {
            lock: DirectoryLock::create(root)?,
        })
    }

    /// DimensionType.getStorageFolder, including Identifier's escape check.
    pub fn dimension_path(&self, dimension: &ResourceKey<Level>) -> PathBuf {
        dimension
            .identifier()
            .resolve_against(&self.lock.directory().join("dimensions"))
    }

    pub fn load_world_uuid(&self, dimension: &ResourceKey<Level>) -> io::Result<Uuid> {
        world_identity::load_or_create_world_uuid(&self.dimension_path(dimension))
    }

    pub fn save_world_uuid(&self, dimension: &ResourceKey<Level>, uuid: Uuid) -> io::Result<()> {
        world_identity::save_world_uuid(&self.dimension_path(dimension), uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::{
        MissingChunkPolicy, OverworldGenerator, ServerLevel, ServerLevelConfig, overworld_dimension,
    };
    use std::sync::Arc;

    #[test]
    fn generated_level_retains_root_lock_and_saves_its_chosen_identity() {
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let generator = Arc::new(OverworldGenerator::new(42));
        let config = ServerLevelConfig {
            is_flat: false,
            sea_level: generator.generator().get_sea_level(),
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..ServerLevelConfig::default()
        };
        let level = ServerLevel::new_generated_with_storage(
            config.clone(),
            Arc::clone(&generator),
            Arc::clone(&storage),
        )
        .unwrap();
        let uuid = level.world_uuid().unwrap();
        let dimension_path = storage.dimension_path(&overworld_dimension());
        assert_eq!(
            dimension_path,
            root.path().join("dimensions/minecraft/overworld")
        );
        assert_eq!(
            world_identity::read_world_uuid(&dimension_path).unwrap(),
            None
        );
        drop(storage);
        assert!(DirectoryLock::is_locked(root.path()).unwrap());
        assert_eq!(
            LevelStorageAccess::open(root.path()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        level.save_world_metadata().unwrap();
        assert_eq!(
            world_identity::read_world_uuid(&dimension_path).unwrap(),
            Some(uuid)
        );
        drop(level);
        assert!(!DirectoryLock::is_locked(root.path()).unwrap());
        let reopened = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let level = ServerLevel::new_generated_with_storage(config, generator, reopened).unwrap();
        assert_eq!(level.world_uuid(), Some(uuid));
        assert!(
            level.chunk_map().is_empty(),
            "metadata boot must not create placeholder terrain"
        );
    }
}
