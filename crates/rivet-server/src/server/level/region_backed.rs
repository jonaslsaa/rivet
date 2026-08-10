//! Existing-only overworld region boot seam for disposable world copies.
//!
//! This module deliberately has no launcher-save discovery, generation, or
//! write API. The caller supplies the already-copied root created by the #316
//! harness. Construction stops at explicit dependency boundaries until #323
//! and #336 land.

use std::io;
use std::path::{Path, PathBuf};

use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::storage::serializable_chunk_data::{
    SerializableChunkData, SerializableChunkDataError,
};
use rivet_world::chunk::storage::{RegionFileStorage, RegionStorageInfo};
use rivet_world::level::height_accessor;

use super::level_chunk::{BiomeId, StateId};
use super::server_level::ServerLevel;

/// Pure Paper overworld storage layout rooted at the supplied disposable copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldLayout {
    root: PathBuf,
    level_dat: PathBuf,
    overworld_region: PathBuf,
}

impl RegionWorldLayout {
    pub fn resolve(root: impl Into<PathBuf>) -> Result<Self, RegionBackedBootError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(RegionBackedBootError::InvalidLevelRoot(root));
        }
        let level_dat = root.join("level.dat");
        if !level_dat.is_file() {
            return Err(RegionBackedBootError::MissingLevelDat(level_dat));
        }
        let overworld_region = root
            .join("dimensions")
            .join("minecraft")
            .join("overworld")
            .join("region");
        if !overworld_region.is_dir() {
            return Err(RegionBackedBootError::MissingOverworldRegion(
                overworld_region,
            ));
        }
        Ok(Self {
            root,
            level_dat,
            overworld_region,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn level_dat(&self) -> &Path {
        &self.level_dat
    }

    pub fn overworld_region(&self) -> &Path {
        &self.overworld_region
    }
}

/// Existing-only region chunk source. Missing chunks remain missing: this type
/// owns no generator and exposes no store/delete/flush operation.
pub struct RegionChunkSource {
    layout: RegionWorldLayout,
    storage: RegionFileStorage,
}

/// Retained ownership seam between layout/region preparation and the future
/// #323/#336 `ServerLevel` composition. Dependencies can fill the next step
/// without changing how the read-only source is owned.
pub struct RegionLevelPreparation {
    source: RegionChunkSource,
}

impl RegionLevelPreparation {
    pub fn prepare(root: &Path) -> Result<Self, RegionBackedBootError> {
        let layout = RegionWorldLayout::resolve(root)?;
        Ok(Self {
            source: RegionChunkSource::open(layout),
        })
    }

    pub fn source(&self) -> &RegionChunkSource {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut RegionChunkSource {
        &mut self.source
    }

    pub fn into_source(self) -> RegionChunkSource {
        self.source
    }
}

impl RegionChunkSource {
    pub fn open(layout: RegionWorldLayout) -> Self {
        let level_name = layout
            .root()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("disposable-world")
            .to_owned();
        let info = RegionStorageInfo::new(
            level_name,
            rivet_world::level::overworld(),
            "region".to_owned(),
            true,
        );
        let storage =
            RegionFileStorage::new_read_only(info, layout.overworld_region().to_path_buf());
        Self { layout, storage }
    }

    pub fn layout(&self) -> &RegionWorldLayout {
        &self.layout
    }

    /// Read and extract one serialized chunk. `None` from storage is a typed
    /// no-generation error, never a request for a superflat replacement.
    pub fn read_serializable(
        &mut self,
        pos: ChunkPos,
    ) -> Result<SerializableChunkData, RegionBackedBootError> {
        let tag = self
            .storage
            .read(&pos)
            .map_err(RegionBackedBootError::RegionRead)?
            .ok_or(RegionBackedBootError::MissingChunkNoGeneration(pos))?;
        SerializableChunkData::parse(height_accessor::create(-64, 384), &tag)
            .map_err(RegionBackedBootError::SerializableChunk)?
            .ok_or(RegionBackedBootError::MissingChunkStatus(pos))
    }

    /// The composition point that #336 completes. Existing serializable
    /// validation runs first so proto/generation and blending boundaries retain
    /// their precise typed errors.
    pub fn load_for_composition(
        &mut self,
        pos: ChunkPos,
    ) -> Result<SerializableChunkData, RegionBackedBootError> {
        let data = self.read_serializable(pos)?;
        data.validate_full_capabilities()
            .map_err(RegionBackedBootError::SerializableChunk)?;
        Err(RegionBackedBootError::SectionReconstructionUnavailable)
    }
}

/// Honest current boot boundary. Opening the layout/source proves the supplied
/// copy is structurally usable; metadata must not be replaced by superflat
/// defaults because spawn/seed/game settings belong to `level.dat`.
pub fn boot_level(root: &Path) -> Result<ServerLevel, RegionBackedBootError> {
    let _prepared = RegionLevelPreparation::prepare(root)?;
    Err(RegionBackedBootError::LevelDataCodecsUnavailable)
}

/// Minimal registry bridge for #336 output: block states reuse the generated
/// global id directly; biomes validate raw generated registry ids against the
/// canonical generated count.
pub fn bridge_block_state(state: BlockState) -> StateId {
    state.id()
}

pub fn bridge_biome_id(id: u16) -> Result<BiomeId, RegionBackedBootError> {
    BiomeId::try_from(id).map_err(|id| RegionBackedBootError::UnknownBiomeId(id))
}

#[derive(Debug, thiserror::Error)]
pub enum RegionBackedBootError {
    #[error("UNVERIFIED invalid disposable level root: {0}")]
    InvalidLevelRoot(PathBuf),
    #[error("UNVERIFIED disposable level is missing level.dat: {0}")]
    MissingLevelDat(PathBuf),
    #[error("UNVERIFIED disposable level is missing overworld region layout: {0}")]
    MissingOverworldRegion(PathBuf),
    #[error("UNVERIFIED level.dat codecs are unavailable (dependency #323)")]
    LevelDataCodecsUnavailable,
    #[error("UNVERIFIED read-only region read failed: {0}")]
    RegionRead(#[source] io::Error),
    #[error("UNVERIFIED chunk {0} is absent; generation and superflat fallback are disabled")]
    MissingChunkNoGeneration(ChunkPos),
    #[error("UNVERIFIED chunk {0} has no usable Status")]
    MissingChunkStatus(ChunkPos),
    #[error("UNVERIFIED serialized chunk is unsupported: {0}")]
    SerializableChunk(#[source] SerializableChunkDataError),
    #[error("UNVERIFIED section/palette reconstruction is unavailable (dependency #336)")]
    SectionReconstructionUnavailable,
    #[error("UNVERIFIED biome registry id {0} is outside the generated registry")]
    UnknownBiomeId(u16),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::nbt_io;
    use rivet_nbt::tag::Tag;
    use rivet_registry::generated::biomes::BIOME_COUNT;
    use rivet_util::data_io::DataOutputStream;
    use rivet_world::chunk::status::ChunkStatus;

    use super::*;

    fn layout() -> (tempfile::TempDir, RegionWorldLayout) {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("level.dat"), b"copied-level").unwrap();
        fs::create_dir_all(temp.path().join("dimensions/minecraft/overworld/region")).unwrap();
        let layout = RegionWorldLayout::resolve(temp.path()).unwrap();
        (temp, layout)
    }

    fn write_chunk(layout: &RegionWorldLayout, mut chunk: CompoundTag) {
        chunk.put_int("xPos", 0);
        chunk.put_int("zPos", 0);
        let mut nbt = Vec::new();
        nbt_io::write(&chunk, &mut DataOutputStream::new(&mut nbt)).unwrap();
        let sectors = (nbt.len() + 5).div_ceil(4096);
        let mut region = vec![0u8; 8192 + sectors * 4096];
        region[..4].copy_from_slice(&((2i32 << 8) | sectors as i32).to_be_bytes());
        region[8192..8196].copy_from_slice(&((nbt.len() as i32) + 1).to_be_bytes());
        region[8196] = 3;
        region[8197..8197 + nbt.len()].copy_from_slice(&nbt);
        fs::write(layout.overworld_region().join("r.0.0.mca"), region).unwrap();
    }

    fn full_chunk() -> CompoundTag {
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:full");
        chunk
    }

    #[test]
    fn resolves_only_the_overworld_layout_below_supplied_root() {
        let (temp, layout) = layout();
        assert_eq!(layout.root(), temp.path());
        assert_eq!(layout.level_dat(), temp.path().join("level.dat"));
        assert_eq!(
            layout.overworld_region(),
            temp.path().join("dimensions/minecraft/overworld/region")
        );
    }

    #[test]
    fn layout_reports_metadata_and_region_prerequisites_separately() {
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            RegionWorldLayout::resolve(temp.path()),
            Err(RegionBackedBootError::MissingLevelDat(_))
        ));

        fs::write(temp.path().join("level.dat"), b"copied-level").unwrap();
        assert!(matches!(
            RegionWorldLayout::resolve(temp.path()),
            Err(RegionBackedBootError::MissingOverworldRegion(_))
        ));
    }

    #[test]
    fn missing_chunk_never_creates_region_or_falls_back() {
        let (_temp, layout) = layout();
        let region = layout.overworld_region().to_path_buf();
        let mut source = RegionChunkSource::open(layout);
        assert!(matches!(
            source.read_serializable(ChunkPos::ZERO),
            Err(RegionBackedBootError::MissingChunkNoGeneration(pos)) if pos == ChunkPos::ZERO
        ));
        assert!(fs::read_dir(region).unwrap().next().is_none());
    }

    #[test]
    fn allocated_corrupt_chunk_is_a_read_error_not_absence() {
        let (_temp, layout) = layout();
        let path = layout.overworld_region().join("r.0.0.mca");
        let mut bytes = vec![0u8; 3 * 4096];
        bytes[..4].copy_from_slice(&((2i32 << 8) | 1).to_be_bytes());
        fs::write(&path, &bytes).unwrap();

        let mut source = RegionChunkSource::open(layout);
        assert!(matches!(
            source.read_serializable(ChunkPos::ZERO),
            Err(RegionBackedBootError::RegionRead(ref error))
                if error.kind() == io::ErrorKind::InvalidData
        ));
        drop(source);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn boot_stops_at_level_data_codec_boundary() {
        let (_temp, layout) = layout();
        assert!(matches!(
            boot_level(layout.root()),
            Err(RegionBackedBootError::LevelDataCodecsUnavailable)
        ));
    }

    #[test]
    fn configured_level_is_not_ignored_when_join_is_disabled() {
        let (_temp, layout) = layout();
        let error = crate::server::Server::try_new(crate::server::ServerConfig {
            enable_join: false,
            level_path: Some(layout.root().to_path_buf()),
            ..Default::default()
        })
        .err()
        .expect("configured level must fail at its current typed boundary");
        assert!(matches!(
            error,
            RegionBackedBootError::LevelDataCodecsUnavailable
        ));
    }

    #[test]
    fn full_region_chunk_reaches_section_reconstruction_boundary() {
        let (_temp, layout) = layout();
        write_chunk(&layout, full_chunk());
        let mut source = RegionChunkSource::open(layout);
        assert!(matches!(
            source.load_for_composition(ChunkPos::ZERO),
            Err(RegionBackedBootError::SectionReconstructionUnavailable)
        ));
    }

    #[test]
    fn proto_and_blending_boundaries_remain_typed() {
        let (_temp, proto_layout) = layout();
        let mut proto = CompoundTag::new();
        proto.put_string("Status", "minecraft:noise");
        write_chunk(&proto_layout, proto);
        let mut source = RegionChunkSource::open(proto_layout);
        assert!(matches!(
            source.load_for_composition(ChunkPos::ZERO),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedChunkStatus {
                    status: ChunkStatus::Noise
                }
            ))
        ));

        let (_temp, blending_layout) = layout();
        let mut blending = full_chunk();
        let mut data = CompoundTag::new();
        data.put_int("min_section", -4);
        data.put_int("max_section", 20);
        blending.put("blending_data".to_owned(), Tag::Compound(data));
        write_chunk(&blending_layout, blending);
        let mut source = RegionChunkSource::open(blending_layout);
        assert!(matches!(
            source.load_for_composition(ChunkPos::ZERO),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedBlendingData
            ))
        ));
    }

    #[test]
    fn registry_bridge_reuses_generated_ids_and_bounds() {
        let state = BlockState::new(StateId(123));
        assert_eq!(bridge_block_state(state), StateId(123));
        assert_eq!(bridge_biome_id(40).unwrap(), BiomeId(40));
        assert!(matches!(
            bridge_biome_id(BIOME_COUNT as u16),
            Err(RegionBackedBootError::UnknownBiomeId(_))
        ));
    }
}
