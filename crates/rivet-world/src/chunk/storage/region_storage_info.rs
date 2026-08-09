//! Port of `net.minecraft.world.level.chunk.storage.RegionStorageInfo` (MC 26.2).
//!
//! A value type describing a region-file population: which world level, which
//! dimension, which storage type (`"region"`, `"entities"`, `"poi"`, ...), and
//! whether the payloads are chunk data. Paper carries the last as the mutable
//! `DataFixTypes[] dfuType` array hack (`info.dfuType()[0] = CHUNK`); per the
//! storage-worker amendment in `OWNERSHIP.md` that becomes a plain
//! `is_chunk_data: bool` field — a shared-mutable array is never reproduced.
//!
//! `is_chunk_data` is what `RegionFile` reads to decide `canRecalcHeader` and
//! what `RegionFileStorage.read` uses to gate its coordinate guard. `RegionFile`
//! itself only consults it; `RegionFileVersion` selection and the `.mcc`
//! directory come from elsewhere.

use rivet_registry::ResourceKey;
use rivet_registry::registries::Level as LevelKey;

/// `net.minecraft.world.level.chunk.storage.RegionStorageInfo`.
///
/// `level` is the world/level name, `dimension` the `ResourceKey<Level>`, and
/// `storage_type` the population suffix ("region", "entities", ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionStorageInfo {
    /// Java `level` — the world/level name this storage belongs to.
    pub level: String,
    /// Java `dimension` — the dimension's `ResourceKey<Level>`.
    pub dimension: ResourceKey<LevelKey>,
    /// Java `type` — the storage population ("region", "entities", "poi", ...).
    pub storage_type: String,
    /// Java `dfuType()[0] == DataFixTypes.CHUNK` — Paper's mutable-array hack,
    /// flattened to a bool per OWNERSHIP.md. `RegionFile.canRecalcHeader` and
    /// `RegionFileStorage`'s coordinate guard both key off this.
    pub is_chunk_data: bool,
}

impl RegionStorageInfo {
    /// `RegionStorageInfo(String level, ResourceKey<Level> dimension, String type)`
    /// — the Paper constructor; the `dfuType` array starts with a single `null`
    /// slot, which the caller overwrites with `CHUNK` or another fixer. In the
    /// flattened field that is `is_chunk_data: false` until a `CHUNK` storage
    /// asserts otherwise.
    pub fn new(
        level: String,
        dimension: ResourceKey<LevelKey>,
        storage_type: String,
        is_chunk_data: bool,
    ) -> Self {
        Self {
            level,
            dimension,
            storage_type,
            is_chunk_data,
        }
    }

    /// `withTypeSuffix(String suffix)` — a copy with `suffix` appended to the
    /// storage type (e.g. the world upgrader's `"region"` -> `"regionsource"`).
    ///
    /// Java routes this through the three-arg constructor rather than copying
    /// the record, so the suffixed storage gets a fresh `dfuType` array with a
    /// null slot — `dfuType()[0] == null`, never `CHUNK`. Flattened: the
    /// chunk-data flag is reset, so a suffixed storage cannot recalculate
    /// headers.
    pub fn with_type_suffix(&self, suffix: &str) -> Self {
        let mut storage_type = self.storage_type.clone();
        storage_type.push_str(suffix);
        Self {
            storage_type,
            // Fresh-array constructor nulls `dfuType[0]`; the base storage's
            // chunk-data flag is deliberately not carried over.
            is_chunk_data: false,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rivet_registry::core::ChunkPos;

    use super::*;
    use crate::chunk::storage::region_file::RegionFile;
    use crate::chunk::storage::region_file_version::RegionFileVersion;

    fn region_info(is_chunk_data: bool) -> RegionStorageInfo {
        RegionStorageInfo::new(
            "test".to_string(),
            crate::level::overworld(),
            "region".to_string(),
            is_chunk_data,
        )
    }

    /// A corrupt-header region file: location[0] points at sector 1 (the
    /// header itself), which is never a valid chunk location.
    fn write_corrupt_header_region(dir: &Path) {
        let mut header = [0u8; 8192];
        header[0..4].copy_from_slice(&((1i32 << 8) | 1).to_be_bytes());
        fs::write(dir.join("r.0.0.mca"), header).unwrap();
    }

    #[test]
    fn with_type_suffix_resets_chunk_data_flag() {
        // Java `withTypeSuffix` routes through the three-arg constructor, which
        // allocates a fresh `dfuType` array with a null slot — so even a CHUNK
        // storage reports non-chunk-data after suffixing.
        let base = region_info(true);
        assert!(base.is_chunk_data);

        let suffixed = base.with_type_suffix("source");
        assert_eq!(suffixed.storage_type, "regionsource");
        assert!(
            !suffixed.is_chunk_data,
            "suffixed storage is not chunk data"
        );
        // The base is untouched.
        assert!(base.is_chunk_data);

        // A non-chunk-data base stays non-chunk-data.
        let also = region_info(false).with_type_suffix("source");
        assert!(!also.is_chunk_data);
    }

    #[test]
    fn suffixed_storage_cannot_recalculate_headers() {
        // The header-recalculation decision keys off `is_chunk_data`: a
        // suffixed (non-chunk-data) storage zeroes the corrupt entry instead
        // of running recalc, while the chunk-data base relinks it.
        let dir = tempfile::tempdir().unwrap();

        // Chunk-data base storage: recalc runs at open.
        write_corrupt_header_region(dir.path());
        let base = RegionFile::open(
            region_info(true),
            dir.path().join("r.0.0.mca"),
            dir.path().to_path_buf(),
            RegionFileVersion::VERSION_NONE,
            false,
        )
        .unwrap();
        assert!(base.get_recalculate_count() >= 1, "chunk data recalculates");

        // Suffixed storage: cannot recalculate — the corrupt entry is removed
        // from the header instead.
        write_corrupt_header_region(dir.path());
        let suffixed = RegionFile::open(
            region_info(true).with_type_suffix("source"),
            dir.path().join("r.0.0.mca"),
            dir.path().to_path_buf(),
            RegionFileVersion::VERSION_NONE,
            false,
        )
        .unwrap();
        assert_eq!(
            suffixed.get_recalculate_count(),
            0,
            "suffixed storage cannot recalculate"
        );
        assert!(
            !suffixed.has_chunk(&ChunkPos::new(0, 0)),
            "corrupt entry removed from the header"
        );
    }
}
