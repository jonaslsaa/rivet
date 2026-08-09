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
    /// storage type (e.g. `"region"` -> `"region"` + `"_chunk_status"`).
    pub fn with_type_suffix(&self, suffix: &str) -> Self {
        let mut storage_type = self.storage_type.clone();
        storage_type.push_str(suffix);
        Self {
            storage_type,
            ..self.clone()
        }
    }
}
