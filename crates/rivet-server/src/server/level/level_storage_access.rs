//! Writable world-root ownership from LevelStorageSource.LevelStorageAccess.
//! This is the storage/identity slice, not level.dat bootstrap or a claim that
//! generated chunks are playable. Read-only imported worlds do not use it.
use super::{directory_lock::DirectoryLock, world_identity};
use rivet_nbt::{compound_tag::CompoundTag, nbt_accounter::NbtAccounter, nbt_io};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelDataTimeSave {
    MissingLevelData,
    Written,
}

/// Prepared from the existing complete metadata. Borrowing storage retains its
/// real directory lock through commit; no detached/fabricated level.dat root.
pub(super) struct PreparedLevelDataTime<'a> {
    storage: &'a LevelStorageAccess,
    root: CompoundTag,
}
impl PreparedLevelDataTime<'_> {
    pub(super) fn commit(self) -> io::Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let folder = self.storage.lock.directory();
        let (path, mut file) = loop {
            let path = folder.join(format!(
                "level{}-{}.dat",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => break (path, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let write = nbt_io::write_compressed(&self.root, &mut file).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        let target = folder.join("level.dat");
        let backup = folder.join("level.dat_old");
        let result = safe_replace(&target, &path, &backup);
        // The source temp file is not canonical data. Keep no stray successful
        // write on an error; the original remains target or backup after rollback.
        let _ = std::fs::remove_file(&path);
        result
    }
}
fn delete_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
fn retry(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut last = None;
    for _ in 0..10 {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) => last = Some(error),
        }
    }
    Err(last.expect("ten attempted source file operations"))
}
/// Util.safeReplaceFile: backup -> remove target -> install temp, restoring
/// the backup if installation fails. Failures are surfaced, not logged as a
/// successful metadata save. No recursive/broad deletion is performed.
fn safe_replace(target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
    if target.try_exists()? {
        retry(|| {
            delete_if_exists(backup)?;
            std::fs::rename(target, backup)
        })?;
    }
    retry(|| delete_if_exists(target))?;
    if let Err(error) = retry(|| std::fs::rename(temp, target)) {
        if let Err(restore) = retry(|| std::fs::rename(backup, target)) {
            return Err(io::Error::other(format!(
                "metadata install failed: {error}; backup restoration failed: {restore}; backup {}",
                backup.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

impl LevelStorageAccess {
    pub(super) fn prepare_existing_level_data_time(
        &self,
        game_time: i64,
    ) -> io::Result<Option<PreparedLevelDataTime<'_>>> {
        let file = match std::fs::File::open(self.lock.directory().join("level.dat")) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            nbt_io::read_compressed(file, &mut NbtAccounter::unlimited_heap())
        }))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid level.dat NBT"))??;
        let mut root = root;
        if root.get_compound("Data").is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "level.dat lacks its Data compound",
            ));
        }
        root.get_compound_or_empty_mut("Data")
            .put_long("Time", game_time);
        Ok(Some(PreparedLevelDataTime {
            storage: self,
            root,
        }))
    }
    pub fn save_existing_level_data_time(&self, game_time: i64) -> io::Result<LevelDataTimeSave> {
        match self.prepare_existing_level_data_time(game_time)? {
            Some(prepared) => {
                prepared.commit()?;
                Ok(LevelDataTimeSave::Written)
            }
            None => Ok(LevelDataTimeSave::MissingLevelData),
        }
    }
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
        let mut level = ServerLevel::new_generated_with_storage(
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
        assert_eq!(
            level.save_world_metadata().unwrap(),
            LevelDataTimeSave::MissingLevelData
        );
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

    #[test]
    fn existing_level_time_write_preserves_full_metadata_and_original_backup() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LevelStorageAccess::open(dir.path()).unwrap();
        assert_eq!(
            storage.save_existing_level_data_time(42).unwrap(),
            LevelDataTimeSave::MissingLevelData
        );
        assert!(!dir.path().join("level.dat").exists());
        let mut root=rivet_nbt::tag_parser::parse_compound_fully("{Data:{Time:2L,DataVersion:4903,LevelName:\"keep\",Custom:{Nested:[1,2,3]}},Outer:\"also keep\"}").unwrap();
        nbt_io::write_compressed(
            &root,
            std::fs::File::create(dir.path().join("level.dat")).unwrap(),
        )
        .unwrap();
        let original = std::fs::read(dir.path().join("level.dat")).unwrap();
        assert_eq!(
            storage.save_existing_level_data_time(42).unwrap(),
            LevelDataTimeSave::Written
        );
        root.get_compound_or_empty_mut("Data").put_long("Time", 42);
        let saved = nbt_io::read_compressed(
            std::fs::File::open(dir.path().join("level.dat")).unwrap(),
            &mut NbtAccounter::unlimited_heap(),
        )
        .unwrap();
        assert_eq!(saved, root, "the bounded writer may only change Data.Time");
        assert_eq!(
            std::fs::read(dir.path().join("level.dat_old")).unwrap(),
            original
        );
    }

    #[test]
    fn replacement_failure_restores_original_instead_of_reporting_success() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("level.dat");
        let backup = dir.path().join("level.dat_old");
        let absent = dir.path().join("absent.tmp");
        std::fs::write(&target, b"original").unwrap();
        assert!(safe_replace(&target, &absent, &backup).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
    }
}
