//! Moonrise entity-region storage, separate from chunk-data persistence.
//! Region IO retains the real dimension/root lock; native values remain owned
//! by EntityLookup. A failed write cannot evict them or acknowledge a deletion.
use super::{entity_lookup::EntityLookup, level_storage_access::LevelStorageAccess};
use rivet_nbt::{
    compound_tag::CompoundTag, list_tag::ListTag, nbt_utils::CURRENT_DATA_VERSION, tag::Tag,
};
use rivet_registry::{ResourceKey, core::ChunkPos, registries::Level};
use rivet_world::{
    chunk::storage::{RegionFileStorage, RegionFileVersion, RegionStorageInfo},
    entity::loaded_entity::NativeLoadedEntity,
    levelgen::structure::StructureError,
};
use std::{collections::HashMap, io, sync::Arc};

#[cfg(test)]
#[path = "entity_region_storage_test.rs"]
mod tests;

pub use super::entity_save_limits::EntitySaveLimits;
#[derive(Debug, thiserror::Error)]
pub enum EntityRegionError {
    #[error("entity region IO at {position:?}: {source}")]
    Io {
        position: Option<ChunkPos>,
        #[source]
        source: io::Error,
    },
    #[error("entity chunk {pos} needs historical conversion from {stored} to {current}")]
    DataConversionUnavailable {
        pos: ChunkPos,
        stored: i32,
        current: i32,
    },
    #[error(
        "entity chunk {0} has an incomplete native load; retained failure must be handled, not reconstructed again"
    )]
    IncompleteLoad(ChunkPos),
    #[error("entity chunk {0} has no prepared disk load")]
    NotPrepared(ChunkPos),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityChunkReadiness {
    Transient,
    Prepared,
    Loaded,
    Incomplete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySaveOutcome {
    Written,
    Deleted,
    SkippedEmpty,
    SkippedTransient,
}
#[derive(Debug)]
pub struct EntityChunkLoadFailure {
    pub source: StructureError,
    pub input_index: Option<usize>,
}
#[derive(Debug)]
struct EntityChunkState {
    status: EntityChunkReadiness,
    /// Retained only until native loading completes. This is disk input, not a
    /// second live-entity authority and can never shadow native mutations.
    input: Option<CompoundTag>,
    prepared: Vec<NativeLoadedEntity>,
    failure: Option<EntityChunkLoadFailure>,
    last_save_null: bool,
}
impl Default for EntityChunkState {
    fn default() -> Self {
        Self {
            status: EntityChunkReadiness::Transient,
            input: None,
            prepared: Vec::new(),
            failure: None,
            last_save_null: false,
        }
    }
}
pub struct StoredEntityRegion {
    storage: Arc<LevelStorageAccess>,
    dimension: ResourceKey<Level>,
    region: RegionFileStorage,
    chunks: HashMap<ChunkPos, EntityChunkState>,
    limits: EntitySaveLimits,
}
impl StoredEntityRegion {
    pub fn save_limits(&self) -> &EntitySaveLimits {
        &self.limits
    }
    pub fn open(
        storage: Arc<LevelStorageAccess>,
        dimension: ResourceKey<Level>,
        sync: bool,
        limits: EntitySaveLimits,
    ) -> Self {
        Self::open_with_version(
            storage,
            dimension,
            sync,
            limits,
            RegionFileVersion::get_selected(),
        )
    }
    pub fn open_with_version(
        storage: Arc<LevelStorageAccess>,
        dimension: ResourceKey<Level>,
        sync: bool,
        limits: EntitySaveLimits,
        version: RegionFileVersion,
    ) -> Self {
        let info = RegionStorageInfo::new(
            dimension.identifier().to_string(),
            dimension.clone(),
            "entities".into(),
            false,
        );
        let region = RegionFileStorage::new_with_version(
            info,
            storage.dimension_path(&dimension).join("entities"),
            sync,
            version,
        );
        Self {
            storage,
            dimension,
            region,
            chunks: HashMap::new(),
            limits,
        }
    }
    pub fn belongs_to(
        &self,
        storage: &Arc<LevelStorageAccess>,
        dimension: &ResourceKey<Level>,
    ) -> bool {
        Arc::ptr_eq(&self.storage, storage) && &self.dimension == dimension
    }
    pub fn readiness(&self, pos: ChunkPos) -> EntityChunkReadiness {
        self.chunks
            .get(&pos)
            .map_or(EntityChunkReadiness::Transient, |s| s.status)
    }
    pub fn failure(&self, pos: ChunkPos) -> Option<&EntityChunkLoadFailure> {
        self.chunks.get(&pos)?.failure.as_ref()
    }
    pub fn retained_entities(&self, pos: ChunkPos) -> &[NativeLoadedEntity] {
        self.chunks.get(&pos).map_or(&[], |s| s.prepared.as_slice())
    }
    fn read_raw(&mut self, pos: ChunkPos) -> Result<Option<CompoundTag>, EntityRegionError> {
        // Existing-only RegionFileStorage must not collapse metadata errors to
        // "absent" through Path::exists. Absence is only actual NotFound.
        let path = self
            .storage
            .dimension_path(&self.dimension)
            .join("entities")
            .join(RegionFileStorage::get_region_file_name(pos.x(), pos.z()));
        if let Err(source) = std::fs::metadata(path)
            && source.kind() != io::ErrorKind::NotFound
        {
            return Err(EntityRegionError::Io {
                position: Some(pos),
                source,
            });
        }
        self.region
            .read(&pos)
            .map_err(|source| EntityRegionError::Io {
                position: Some(pos),
                source,
            })
    }
    /// Actual Moonrise data conversion default is -1. Empty compounds bypass
    /// conversion; current/future versions have no reverse conversion. Unlike
    /// vanilla EntityStorage this path does NOT validate the Position tag.
    pub fn prepare_disk_load(&mut self, pos: ChunkPos) -> Result<(), EntityRegionError> {
        match self.readiness(pos) {
            EntityChunkReadiness::Loaded | EntityChunkReadiness::Prepared => return Ok(()),
            EntityChunkReadiness::Incomplete => return Err(EntityRegionError::IncompleteLoad(pos)),
            EntityChunkReadiness::Transient => {}
        }
        let input = self.read_raw(pos)?;
        if let Some(tag) = &input
            && !tag.is_empty()
        {
            let stored = tag.get_int_or("DataVersion", -1);
            if stored < CURRENT_DATA_VERSION {
                return Err(EntityRegionError::DataConversionUnavailable {
                    pos,
                    stored,
                    current: CURRENT_DATA_VERSION,
                });
            }
        }
        let state = self.chunks.entry(pos).or_default();
        state.input = input;
        state.status = EntityChunkReadiness::Prepared;
        Ok(())
    }
    pub(super) fn disk_inputs(&self, pos: ChunkPos) -> Result<Vec<CompoundTag>, EntityRegionError> {
        let state = self
            .chunks
            .get(&pos)
            .ok_or(EntityRegionError::NotPrepared(pos))?;
        if state.status != EntityChunkReadiness::Prepared {
            return Err(EntityRegionError::NotPrepared(pos));
        }
        Ok(state
            .input
            .as_ref()
            .and_then(|tag| tag.get_list("Entities"))
            .map_or_else(Vec::new, |list| {
                list.list
                    .iter()
                    .filter_map(|tag| {
                        if let Tag::Compound(tag) = tag {
                            Some(tag.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            }))
    }
    pub(super) fn prepared_entities_mut(
        &mut self,
        pos: ChunkPos,
    ) -> Result<&mut Vec<NativeLoadedEntity>, EntityRegionError> {
        let state = self
            .chunks
            .get_mut(&pos)
            .ok_or(EntityRegionError::NotPrepared(pos))?;
        if !matches!(
            state.status,
            EntityChunkReadiness::Prepared | EntityChunkReadiness::Incomplete
        ) {
            return Err(EntityRegionError::NotPrepared(pos));
        }
        Ok(&mut state.prepared)
    }
    pub(super) fn begin_native_load(&mut self, pos: ChunkPos) {
        self.chunks
            .get_mut(&pos)
            .expect("prepared disk chunk")
            .status = EntityChunkReadiness::Incomplete;
    }
    pub(super) fn fail_load(
        &mut self,
        pos: ChunkPos,
        source: StructureError,
        input_index: Option<usize>,
    ) {
        let state = self.chunks.get_mut(&pos).expect("prepared disk chunk");
        state.status = EntityChunkReadiness::Incomplete;
        state.failure = Some(EntityChunkLoadFailure {
            source,
            input_index,
        });
    }
    pub(super) fn finish_load(&mut self, pos: ChunkPos) {
        let state = self.chunks.get_mut(&pos).expect("prepared disk chunk");
        assert!(state.prepared.is_empty());
        state.input = None;
        state.status = EntityChunkReadiness::Loaded;
    }
    pub(super) fn forget_loaded_chunk(&mut self, pos: ChunkPos) {
        self.chunks.remove(&pos);
    }
    pub(super) fn begin_unload(&mut self, pos: ChunkPos) {
        let state = self.chunks.entry(pos).or_default();
        state.status = EntityChunkReadiness::Incomplete;
        state.failure = Some(EntityChunkLoadFailure {
            source: StructureError::PlacementUnavailable(
                "entity unload has acknowledged storage but has not completed native removal"
                    .into(),
            ),
            input_index: None,
        });
    }
    pub fn save_chunk(
        &mut self,
        pos: ChunkPos,
        lookup: &EntityLookup,
        unloading: bool,
    ) -> Result<EntitySaveOutcome, EntityRegionError> {
        let readiness = self.readiness(pos);
        if matches!(
            readiness,
            EntityChunkReadiness::Prepared | EntityChunkReadiness::Incomplete
        ) {
            return Err(EntityRegionError::IncompleteLoad(pos));
        }
        let mut saved = serialize_entity_chunk(
            lookup
                .chunk_ids(pos)
                .iter()
                .filter_map(|id| lookup.get_raw(*id))
                .filter(|entity| !entity.is_removed()),
            pos,
            &self.limits,
            false,
        );
        if readiness == EntityChunkReadiness::Transient {
            if !unloading {
                return Ok(EntitySaveOutcome::SkippedTransient);
            }
            let Some(tag) = &mut saved else {
                return Ok(EntitySaveOutcome::SkippedEmpty);
            };
            // Source transient unload prepends unchanged raw on-disk entities.
            // It does not deserialize/re-save them or run entity converters.
            let disk = self.read_raw(pos)?;
            copy_entities(disk.as_ref(), tag);
        } else if saved.is_none()
            && self
                .chunks
                .get(&pos)
                .is_some_and(|state| state.last_save_null)
        {
            return Ok(EntitySaveOutcome::SkippedEmpty);
        }
        self.region
            .write_ref(&pos, saved.as_ref())
            .map_err(|source| EntityRegionError::Io {
                position: Some(pos),
                source,
            })?;
        self.chunks.entry(pos).or_default().last_save_null = saved.is_none();
        Ok(if saved.is_some() {
            EntitySaveOutcome::Written
        } else {
            EntitySaveOutcome::Deleted
        })
    }
    pub fn flush(&mut self) -> Result<(), EntityRegionError> {
        self.region.flush().map_err(|source| EntityRegionError::Io {
            position: None,
            source,
        })
    }
    pub fn close(&mut self) -> Result<(), EntityRegionError> {
        self.region.close().map_err(|source| EntityRegionError::Io {
            position: None,
            source,
        })
    }
}

/// ChunkEntitySlices.saveEntityChunk0: limits count BEFORE save() can decline,
/// per-type counts never reorder input, force keeps an empty entity list.
pub fn serialize_entity_chunk<'a>(
    entities: impl IntoIterator<Item = &'a NativeLoadedEntity>,
    pos: ChunkPos,
    limits: &EntitySaveLimits,
    force: bool,
) -> Option<CompoundTag> {
    let mut counts = HashMap::<&str, i32>::new();
    let mut list = ListTag::new();
    for entity in entities {
        if !limits.accepts(entity.encode_id(), &mut counts) {
            continue;
        }
        if let Some(tag) = entity.save() {
            list.list.push(Tag::Compound(tag));
        }
    }
    if !force && list.list.is_empty() {
        return None;
    }
    let mut tag = CompoundTag::new();
    tag.put_int("DataVersion", CURRENT_DATA_VERSION);
    tag.put("Entities".into(), Tag::List(list));
    tag.put_int_array("Position", vec![pos.x(), pos.z()]);
    Some(tag)
}
pub fn copy_entities(from: Option<&CompoundTag>, into: &mut CompoundTag) {
    let Some(from) = from
        .and_then(|tag| tag.get_list("Entities"))
        .filter(|list| !list.list.is_empty())
    else {
        return;
    };
    let mut list = into.get_list("Entities").cloned().unwrap_or_default();
    list.list.splice(0..0, from.list.iter().cloned());
    into.put("Entities".into(), Tag::List(list));
}
