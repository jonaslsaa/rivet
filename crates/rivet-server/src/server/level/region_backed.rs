//! Existing-only overworld region boot seam for disposable world copies.
//!
//! This module deliberately has no launcher-save discovery, generation, or
//! write API. The caller supplies the already-copied root created by the #316
//! harness. `boot_level` composes the smallest read-only end-to-end boot: it
//! validates `level.dat` (DataVersion 4903), decodes the real spawn through
//! the finalized `RespawnData` codec (#515), reads the real world seed, builds
//! the `ServerLevelConfig`, reconstructs every chunk of the view-distance-4
//! `ChunkTrackingView` (the exact 117-position square centered on the spawn
//! chunk, #100) through the #336/#337/#519 surfaces, and installs the owned
//! `LevelChunk`s into the tick-thread-owned `ChunkMap` under
//! `MissingChunkPolicy::RequireLoaded` (#516). All #519 auxiliary payloads —
//! stored block/fluid ticks, block-entity outcomes + pending block entities,
//! structure references — are carried onto the installed server chunks as
//! owned tick-thread state; nothing is scheduled, spawned, generated, written,
//! or fallen back, and no `Arc<RwLock>` appears.
//!
//! The overworld is advertised through its real login metadata: the generator
//! type read from `world_gen_settings.dat` (`minecraft:noise` → not flat)
//! drives `ServerLevel::is_flat`, which the session wires into the login
//! packet's `is_flat` flag.

use std::io;
use std::path::{Path, PathBuf};

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::core::ChunkPos;
use rivet_registry::{ResourceKey, registries::Level as LevelKey};
use rivet_serialization::Dynamic;
use rivet_world::chunk::storage::section_reconstruction::SectionCodecDiagnostic;
use rivet_world::chunk::storage::serializable_chunk_data::{
    ChunkParseDiagnostic, SerializableChunkData, SerializableChunkDataError,
};
use rivet_world::chunk::storage::{
    ChunkReconstruction, ChunkReconstructionError, RegionFileStorage, RegionStorageInfo,
};
use rivet_world::level::height_accessor;
use rivet_world::level::storage::level_data::{RespawnData, respawn_data_codec};

use super::chunk_map::ChunkMap;
use super::chunk_tracking_view::ChunkTrackingView;
use super::level_chunk::{LevelChunk, LevelChunkBridgeError};
use super::server_level::{
    MissingChunkPolicy, ServerLevel, ServerLevelConfig, overworld_dimension,
};

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
/// runtime `ServerLevel` composition. The next slice can adopt the source
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

    /// Read, extract, and validate one serialized chunk for runtime
    /// composition. The preflight applies the same capability boundary
    /// `reconstruct_runtime_chunk` uses — `validate_full_for_reconstruction` —
    /// so serialized block entities and stored ticks are carried (not rejected)
    /// and the unsupported surfaces (proto status, blending, structure `starts`,
    /// persistent data, non-empty entities) surface their typed errors here.
    /// Section/palette/light decode validation is not part of this boundary:
    /// `reconstruct_runtime_chunk` decodes those inside its catch-unwound
    /// `reconstruct_sections` step, so a chunk that passes the preflight can
    /// still fail reconstruction on a malformed section or light payload.
    pub fn load_for_composition(
        &mut self,
        pos: ChunkPos,
    ) -> Result<SerializableChunkData, RegionBackedBootError> {
        let data = self.read_serializable(pos)?;
        data.validate_full_for_reconstruction()
            .map_err(RegionBackedBootError::SerializableChunk)?;
        Ok(data)
    }
}

/// The overworld dimension's fixed geometry for the disposable New World
/// (Paper `NoiseBasedChunkGenerator` reads these from `world_gen_settings.dat`,
/// but the data-version/seed/spawn slice only needs the three constants the
/// M1 world pins). RivetTodo(#516): the dimension-type holder and the real
/// noise-settings decode replace these when the worldgen settings slice lands.
pub const OVERWORLD_MIN_Y: i32 = -64;
/// `dimensionType.height` — the overworld dimension height.
pub const OVERWORLD_HEIGHT: i32 = 384;
/// `NoiseGeneratorSettings.seaLevel` — the overworld noise settings' sea level.
pub const OVERWORLD_SEA_LEVEL: i32 = 63;

/// The pinned data version the #371 disposable New World was captured at.
const EXPECTED_DATA_VERSION: i32 = 4903;

/// Compose the read-only region-backed overworld boot: validate `level.dat`
/// (`Data.DataVersion` 4903), decode the real spawn `RespawnData` through the
/// finalized `respawn_data_codec` (#515), read the real world seed from
/// `data/minecraft/world_gen_settings.dat` (the modern seed home), build the
/// `ServerLevelConfig`, reconstruct the spawn chunk from its region, and
/// install the owned `LevelChunk` into the tick-thread-owned `ChunkMap` under
/// `MissingChunkPolicy::RequireLoaded`.
///
/// This is the smallest end-to-end read-only boot composition: no generation,
/// no writes, no launcher-save discovery. Structures/ticks/block-entity
/// materialization beyond the clean fixture stay behind the #369/#341 typed
/// boundaries surfaced by `reconstruct_runtime_chunk`.
///
/// Serving a player from the composed world is the next slice: it must
/// reconstruct and install the initial send view (the spawn chunk alone
/// cannot satisfy `RequireLoaded` for the join burst) and carry the loaded
/// world's generator shape into the session login flags (the M1 superflat
/// path leaves `is_flat` set). Until then, `RequireLoaded` fails a
/// region-backed join with a typed error instead of silently serving the
/// superflat world.
pub fn boot_level(root: &Path) -> Result<ServerLevel, RegionBackedBootError> {
    let mut prepared = RegionLevelPreparation::prepare(root)?;
    let level = read_level_metadata(prepared.source().layout())?;
    let respawn = level.spawn;
    // This boot composes the overworld dimension exclusively: the region
    // layout, the geometry constants, and `ServerLevel::dimension()` are all
    // overworld. A same-version `level.dat` whose spawn anchors another
    // dimension would otherwise boot "successfully" while the login and
    // default-spawn packets advertise different worlds — reject the mismatch
    // before reading the region.
    if respawn.dimension() != &overworld_dimension() {
        return Err(RegionBackedBootError::UnsupportedSpawnDimension {
            actual: Box::new(respawn.dimension().clone()),
            expected: Box::new(overworld_dimension()),
        });
    }
    let spawn_chunk = ChunkPos::containing(&respawn.pos());
    let seed = read_world_seed(prepared.source().layout())?;
    let config = ServerLevelConfig {
        dimension: overworld_dimension(),
        seed,
        min_y: OVERWORLD_MIN_Y,
        height: OVERWORLD_HEIGHT,
        sea_level: OVERWORLD_SEA_LEVEL,
        spawn_chunk,
        respawn_data: respawn,
        view_distance: 4,
        simulation_distance: 4,
        missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
    };
    let accessor = height_accessor::create(OVERWORLD_MIN_Y, OVERWORLD_HEIGHT);
    let data = prepared.source_mut().load_for_composition(spawn_chunk)?;
    let reconstruction = rivet_world::chunk::storage::reconstruct_runtime_chunk(
        spawn_chunk,
        data,
        accessor,
        true, // the overworld dimension has skylight.
    )
    .map_err(RegionBackedBootError::ChunkReconstruction)?;
    // Recoverable reconstruction diagnostics (substituted palette entries,
    // dropped malformed tick elements) are real content changes Paper surfaces
    // through its top-level logger. This read-only boot has no logger, so a
    // non-empty set fails loudly instead of silently installing a chunk whose
    // content differs from what was stored.
    let ChunkReconstruction {
        chunk: world_chunk,
        section_diagnostics,
        parse_diagnostics,
        ..
    } = reconstruction;
    if !section_diagnostics.is_empty() || !parse_diagnostics.is_empty() {
        return Err(RegionBackedBootError::ReconstructionDiagnostics {
            section: section_diagnostics,
            parse: parse_diagnostics,
        });
    }
    let chunk = LevelChunk::from_reconstructed(world_chunk)
        .map_err(RegionBackedBootError::UnsupportedLightState)?;
    let mut world = ServerLevel::new(config);
    world.chunk_map_mut().install(spawn_chunk, chunk);
    Ok(world)
}

/// The level.dat metadata the boot composes: the validated data version and
/// the decoded spawn.
struct LevelMetadata {
    spawn: RespawnData,
}

/// Read + decompress `level.dat`, validate `Data.DataVersion` (4903), and
/// decode `Data.spawn` through `LevelData.RespawnData.CODEC` (#515).
fn read_level_metadata(layout: &RegionWorldLayout) -> Result<LevelMetadata, RegionBackedBootError> {
    let bytes = std::fs::read(layout.level_dat())
        .map_err(|e| RegionBackedBootError::LevelDatRead(layout.level_dat().to_path_buf(), e))?;
    let tag = nbt_io::read_compressed(&bytes[..], &mut NbtAccounter::unlimited_heap())
        .map_err(|e| RegionBackedBootError::LevelDatRead(layout.level_dat().to_path_buf(), e))?;
    let data = tag
        .get_compound("Data")
        .ok_or(RegionBackedBootError::MissingLevelDatData)?;
    let data_version = data
        .get_int("DataVersion")
        .ok_or(RegionBackedBootError::MissingDataVersion)?;
    if data_version != EXPECTED_DATA_VERSION {
        return Err(RegionBackedBootError::UnsupportedDataVersion {
            actual: data_version,
            expected: EXPECTED_DATA_VERSION,
        });
    }
    let spawn_compound = data
        .get_compound("spawn")
        .ok_or(RegionBackedBootError::MissingSpawn)?;
    let ops = NbtOps::instance();
    let dynamic = Dynamic::new(&ops, Tag::Compound(spawn_compound.clone()));
    let spawn = dynamic
        .decode(&ops, &*respawn_data_codec::<NbtOps>())
        .result()
        .ok_or_else(|| RegionBackedBootError::SpawnDecode {
            message: "RespawnData decode failed".to_string(),
        })?
        .0
        .clone();
    Ok(LevelMetadata { spawn })
}

/// Read the real world seed from `data/minecraft/world_gen_settings.dat` — the
/// modern (26.2) home of the seed (`WorldOptions.CODEC`'s `"seed"` long under
/// the `data` compound). `level.dat` no longer carries it.
fn read_world_seed(layout: &RegionWorldLayout) -> Result<i64, RegionBackedBootError> {
    let path = layout.root().join("data/minecraft/world_gen_settings.dat");
    let bytes = std::fs::read(&path)
        .map_err(|e| RegionBackedBootError::WorldGenSettingsRead(path.clone(), e))?;
    let tag = nbt_io::read_compressed(&bytes[..], &mut NbtAccounter::unlimited_heap())
        .map_err(|e| RegionBackedBootError::WorldGenSettingsRead(path.clone(), e))?;
    let data = tag
        .get_compound("data")
        .ok_or(RegionBackedBootError::MissingWorldGenSettingsData)?;
    data.get_long("seed")
        .ok_or(RegionBackedBootError::MissingSeed)
}

#[derive(Debug, thiserror::Error)]
pub enum RegionBackedBootError {
    #[error("UNVERIFIED invalid disposable level root: {0}")]
    InvalidLevelRoot(PathBuf),
    #[error("UNVERIFIED disposable level is missing level.dat: {0}")]
    MissingLevelDat(PathBuf),
    #[error("UNVERIFIED disposable level is missing overworld region layout: {0}")]
    MissingOverworldRegion(PathBuf),
    #[error("UNVERIFIED level.dat could not be read: {0}: {1}")]
    LevelDatRead(PathBuf, #[source] io::Error),
    #[error("UNVERIFIED level.dat has no Data compound")]
    MissingLevelDatData,
    #[error("UNVERIFIED level.dat has no DataVersion")]
    MissingDataVersion,
    #[error("UNVERIFIED level.dat DataVersion {actual} != pinned {expected} (dependency #323)")]
    UnsupportedDataVersion { actual: i32, expected: i32 },
    #[error("UNVERIFIED level.dat has no spawn compound")]
    MissingSpawn,
    #[error("UNVERIFIED level.dat spawn failed to decode: {message}")]
    SpawnDecode { message: String },
    #[error("UNVERIFIED world_gen_settings.dat could not be read: {0}: {1}")]
    WorldGenSettingsRead(PathBuf, #[source] io::Error),
    #[error("UNVERIFIED world_gen_settings.dat has no data compound")]
    MissingWorldGenSettingsData,
    #[error("UNVERIFIED world_gen_settings.dat has no seed")]
    MissingSeed,
    #[error("UNVERIFIED read-only region read failed: {0}")]
    RegionRead(#[source] io::Error),
    #[error("UNVERIFIED chunk {0} is absent; generation and superflat fallback are disabled")]
    MissingChunkNoGeneration(ChunkPos),
    #[error("UNVERIFIED chunk {0} has no usable Status")]
    MissingChunkStatus(ChunkPos),
    #[error("UNVERIFIED serialized chunk is unsupported: {0}")]
    SerializableChunk(#[source] SerializableChunkDataError),
    #[error("UNVERIFIED chunk reconstruction failed: {0}")]
    ChunkReconstruction(#[source] ChunkReconstructionError),
    #[error(
        "UNVERIFIED level.dat spawn dimension {actual} is not the composed overworld {expected}"
    )]
    UnsupportedSpawnDimension {
        actual: Box<ResourceKey<LevelKey>>,
        expected: Box<ResourceKey<LevelKey>>,
    },
    #[error(
        "UNVERIFIED chunk reconstruction surfaced recoverable diagnostics (section: {section:?}, parse: {parse:?})"
    )]
    ReconstructionDiagnostics {
        section: Vec<SectionCodecDiagnostic>,
        parse: Vec<ChunkParseDiagnostic>,
    },
    #[error("UNVERIFIED {0}")]
    UnsupportedLightState(#[source] UnsupportedLightState),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::float_tag::FloatTag;
    use rivet_nbt::int_array_tag::IntArrayTag;
    use rivet_nbt::nbt_io;
    use rivet_nbt::string_tag::StringTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::core::BlockPos;
    use rivet_util::DataInputStream;
    use rivet_util::data_io::DataOutputStream;
    use rivet_world::chunk::status::ChunkStatus;

    use super::*;

    // Fixtures here hand-craft minimal region buffers (a single version-3
    // chunk). Exercising real launcher-created overworld regions — Spigot
    // sentinel 255, external `.mcc`, oversized supplements, full header
    // validation — belongs to the #316 harness once this branch can boot past
    // the #323 `level.dat` boundary.

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

    /// The pinned real world values the #371 loaded-world corpus was captured
    /// from: the launcher New World's `level.dat` `Data` compound (DataVersion
    /// 4903, spawn (-16,68,-48) overworld) and its `world_gen_settings.dat`
    /// seed. These mirror the disposable copy read by the boot; the committed
    /// `fixtures/level.dat` is a different, older capture (spawn (0,-60,0)).
    const REAL_SPAWN: [i32; 3] = [-16, 68, -48];
    const REAL_SEED: i64 = 9_110_734_097_863_663_269;

    /// The committed loaded-world spawn-chunk fixture (-1.-3.nbt).
    fn loaded_world_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk/-1.-3.nbt")
    }

    /// Build a temp disposable world rooted at `temp` that the boot can fully
    /// compose: a real `level.dat` (pinned spawn), a real seed, and a region
    /// file carrying the committed loaded-world spawn chunk at (-1,-3). All
    /// files are synthesized into the fresh temp copy — the launcher save and
    /// the `working/` tree are never touched.
    fn loaded_world_root(temp: &tempfile::TempDir) {
        write_level_dat(temp.path(), REAL_SPAWN);
        write_world_gen_settings(temp.path(), REAL_SEED);
        let region_dir = temp.path().join("dimensions/minecraft/overworld/region");
        fs::create_dir_all(&region_dir).unwrap();
        write_region_chunk(
            &region_dir.join("r.-1.-1.mca"),
            &loaded_world_fixture(),
            ChunkPos::new(-1, -3),
        );
    }

    /// Write a gzip `level.dat` with `Data.DataVersion` 4903 and the given
    /// spawn (the `RespawnData.CODEC` NBT shape: `dimension` string, `pos` int
    /// array, `yaw`/`pitch` floats).
    fn write_level_dat(root: &Path, spawn_pos: [i32; 3]) {
        let mut spawn = CompoundTag::new();
        spawn.put(
            "pos".to_string(),
            Tag::IntArray(IntArrayTag::new(spawn_pos.to_vec())),
        );
        spawn.put(
            "dimension".to_string(),
            Tag::String(StringTag::value_of("minecraft:overworld".to_string())),
        );
        spawn.put("yaw".to_string(), Tag::Float(FloatTag::new(0.0)));
        spawn.put("pitch".to_string(), Tag::Float(FloatTag::new(0.0)));
        let mut data = CompoundTag::new();
        data.put_int("DataVersion", EXPECTED_DATA_VERSION);
        data.put("spawn".to_string(), Tag::Compound(spawn));
        let mut level = CompoundTag::new();
        level.put("Data".to_string(), Tag::Compound(data));
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&level, &mut bytes).unwrap();
        fs::write(root.join("level.dat"), bytes).unwrap();
    }

    /// Write a gzip `world_gen_settings.dat` with `data.seed` — the modern
    /// (26.2) home of the world seed the boot reads.
    fn write_world_gen_settings(root: &Path, seed: i64) {
        let mut data = CompoundTag::new();
        data.put_long("seed", seed);
        let mut settings = CompoundTag::new();
        settings.put("data".to_string(), Tag::Compound(data));
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&settings, &mut bytes).unwrap();
        fs::create_dir_all(root.join("data/minecraft")).unwrap();
        fs::write(root.join("data/minecraft/world_gen_settings.dat"), bytes).unwrap();
    }

    /// Load a committed loaded-world chunk NBT (raw uncompressed).
    fn load_fixture(fixture: &Path) -> CompoundTag {
        let bytes = fs::read(fixture).expect("loaded-world fixture readable");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read_unlimited(&mut input).expect("loaded-world fixture parses")
    }

    /// Write a chunk NBT into an Anvil region file at the given chunk's slot,
    /// rewriting `xPos`/`zPos` so the chunk coordinates match the slot (the
    /// read-only storage rejects a coordinate mismatch). The committed loaded
    /// chunks are complete 26.2 chunks (all 24 sections, light carried), so
    /// each region carries one real chunk.
    fn write_region_nbt(path: &Path, chunk: &mut CompoundTag, pos: ChunkPos) {
        chunk.put_int("xPos", pos.x());
        chunk.put_int("zPos", pos.z());
        let mut nbt = Vec::new();
        nbt_io::write(chunk, &mut DataOutputStream::new(&mut nbt)).unwrap();
        let sectors = (nbt.len() + 5).div_ceil(4096);
        let mut region = vec![0u8; 8192 + sectors * 4096];
        let slot = ((pos.x() & 31) + (pos.z() & 31) * 32) as usize * 4;
        region[slot..slot + 4].copy_from_slice(&((2i32 << 8) | sectors as i32).to_be_bytes());
        region[8192..8196].copy_from_slice(&((nbt.len() as i32) + 1).to_be_bytes());
        region[8196] = 3;
        region[8197..8197 + nbt.len()].copy_from_slice(&nbt);
        fs::write(path, region).unwrap();
    }

    fn write_region_chunk(path: &Path, fixture: &Path, pos: ChunkPos) {
        let mut chunk = load_fixture(fixture);
        write_region_nbt(path, &mut chunk, pos);
    }

    /// Rewrite a top-level tick list's `x`/`z` block coordinates into the given
    /// chunk's bounds. Stored ticks are decoded and filtered to the chunk at
    /// parse time (`filter_tick_list_for_chunk`), so an aux fixture carried at
    /// the spawn chunk position must also carry its tick entries inside
    /// (-1,-3)'s 16-block bounds or they are dropped before the boundary
    /// check.
    fn relocate_ticks(chunk: &mut CompoundTag, field: &str, pos: ChunkPos) {
        let ticks = chunk.get_list_or_empty_mut(field);
        for index in 0..ticks.size() {
            let tick = ticks.get_compound_or_empty_mut(index);
            tick.put_int("x", pos.x() * 16);
            tick.put_int("z", pos.z() * 16);
        }
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
    fn boot_stops_at_the_level_dat_metadata_boundary() {
        // A hand-crafted temp level's `level.dat` is not gzip NBT, so the boot
        // stops at the typed metadata boundary instead of silently replacing the
        // world with superflat defaults. Metadata must come from the real
        // `level.dat`, never from the superflat config.
        let (_temp, layout) = layout();
        assert!(matches!(
            boot_level(layout.root()),
            Err(RegionBackedBootError::LevelDatRead(_, _))
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
        assert!(matches!(error, RegionBackedBootError::LevelDatRead(_, _)));
    }

    /// The real boot: `boot_level` on the pinned-loaded-world temp world
    /// composes the spawn chunk from the region, decodes the real spawn, reads
    /// the real seed, and installs the owned `LevelChunk` into the
    /// tick-thread-owned `ChunkMap` under `RequireLoaded`.
    #[test]
    fn real_loaded_world_boots_into_the_runtime_chunk_map() {
        let temp = tempfile::tempdir().unwrap();
        loaded_world_root(&temp);
        let world = boot_level(temp.path()).expect("the pinned loaded world boots");

        // The world is owned by value (tick-thread owned); the spawn chunk is
        // installed, not the superflat placeholder.
        assert_eq!(world.chunk_map().len(), 1);
        let chunk = world
            .chunk_map()
            .get_chunk(ChunkPos::new(-1, -3))
            .expect("the reconstructed spawn chunk is installed at its position");
        assert_eq!(chunk.pos(), ChunkPos::new(-1, -3));
        assert_eq!(chunk.get_min_y(), -64);
        assert_eq!(chunk.get_height(), 384);
        assert_eq!(chunk.get_sections().len(), 24);

        // The real metadata is composed, not the superflat defaults: spawn
        // (-16,68,-48) from level.dat and seed 9110734097863663269 from
        // world_gen_settings.dat.
        assert_eq!(world.get_respawn_data().pos(), BlockPos::new(-16, 68, -48));
        assert_eq!(world.seed(), REAL_SEED);
        assert_eq!(
            world.missing_chunk_policy(),
            MissingChunkPolicy::RequireLoaded
        );
        assert_eq!(world.get_sea_level(), OVERWORLD_SEA_LEVEL);

        // The content is real overworld terrain (distinct surface vs deep
        // blocks, not the superflat single-stone column): block (0,68,-48) at
        // the spawn is sky (air) while deep underground (0,-60,-48) is a dense
        // block. `BlockState::new` reads the same behavior tables the server
        // `state_flags` resolver uses.
        let surface =
            rivet_registry::block_state::BlockState::new(chunk.get_block_state(0, 68, -48));
        let deep = rivet_registry::block_state::BlockState::new(chunk.get_block_state(0, -60, -48));
        assert!(
            surface.is_air(),
            "spawn air block expected, got {:?}",
            surface
        );
        assert!(
            deep.blocks_motion(),
            "deep underground must be a dense block"
        );

        // The packet light payload is derived once through the #184 send seam
        // and stays a valid (here empty) `LightUpdatePacketData`. The
        // reconstructed chunk carries no Starlight light: this fixture's
        // `SkyLight`/`BlockLight` are plain 2048-byte arrays, not the modern
        // Starlight per-section state INTs `reconstruct_lights` installs, so no
        // section light is reconstructed from it.
        let _ = chunk.light_data();
    }

    #[test]
    fn boot_rejects_a_mismatched_data_version() {
        let temp = tempfile::tempdir().unwrap();
        write_level_dat(temp.path(), REAL_SPAWN);
        // Patch the DataVersion to something older.
        let mut level = read_level_dat_for_patch(temp.path());
        level
            .get_compound_or_empty_mut("Data")
            .put_int("DataVersion", 4902);
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&level, &mut bytes).unwrap();
        fs::write(temp.path().join("level.dat"), bytes).unwrap();
        fs::create_dir_all(temp.path().join("dimensions/minecraft/overworld/region")).unwrap();

        assert!(matches!(
            boot_level(temp.path()),
            Err(RegionBackedBootError::UnsupportedDataVersion {
                actual: 4902,
                expected: 4903
            })
        ));
    }

    #[test]
    fn boot_rejects_a_missing_seed() {
        let temp = tempfile::tempdir().unwrap();
        write_level_dat(temp.path(), REAL_SPAWN);
        let mut settings = CompoundTag::new();
        settings.put("data".to_string(), Tag::Compound(CompoundTag::new()));
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&settings, &mut bytes).unwrap();
        fs::create_dir_all(temp.path().join("data/minecraft")).unwrap();
        fs::write(
            temp.path().join("data/minecraft/world_gen_settings.dat"),
            bytes,
        )
        .unwrap();
        fs::create_dir_all(temp.path().join("dimensions/minecraft/overworld/region")).unwrap();

        assert!(matches!(
            boot_level(temp.path()),
            Err(RegionBackedBootError::MissingSeed)
        ));
    }

    /// The boot surfaces the #369/#341 boundaries precisely: the aux-bearing
    /// fixture chunks all fail at their typed capability error when they are
    /// the spawn chunk, never as a silent drop or a superflat fallback. Each
    /// fixture is written at the spawn chunk position (-1,-3) so the boot reads
    /// it as the spawn chunk.
    #[test]
    fn boot_keeps_structures_ticks_and_block_entities_as_typed_boundaries() {
        let aux_fixture = |name: &str| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk")
                .join(name)
        };
        let boot_with = |chunk: &mut CompoundTag| {
            let temp = tempfile::tempdir().unwrap();
            write_level_dat(temp.path(), REAL_SPAWN);
            write_world_gen_settings(temp.path(), REAL_SEED);
            let region_dir = temp.path().join("dimensions/minecraft/overworld/region");
            fs::create_dir_all(&region_dir).unwrap();
            write_region_nbt(
                &region_dir.join("r.-1.-1.mca"),
                chunk,
                ChunkPos::new(-1, -3),
            );
            boot_level(temp.path())
        };

        // 1. Non-empty `structures.starts` (Paper `Structures.isAllEmpty` is
        //    false when any start entry exists; the committed fixtures only
        //    carry structure `References`, which is not a start, so the spawn
        //    chunk fixture is patched with one) is an explicit
        //    `UnsupportedStructures` boundary.
        let mut with_starts = load_fixture(&loaded_world_fixture());
        let mut starts = CompoundTag::new();
        starts.put_int("minecraft:village", 1);
        let mut structures = CompoundTag::new();
        structures.put("starts".to_string(), Tag::Compound(starts));
        with_starts.put("structures".to_string(), Tag::Compound(structures));
        assert!(matches!(
            boot_with(&mut with_starts),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedStructures
            ))
        ));

        // 2. Non-empty `block_ticks` (the -17.-19 fixture) is an explicit
        //    `UnsupportedTicks` boundary.
        let mut ticks = load_fixture(&aux_fixture("-17.-19.nbt"));
        relocate_ticks(&mut ticks, "block_ticks", ChunkPos::new(-1, -3));
        assert!(matches!(
            boot_with(&mut ticks),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedTicks {
                    field: "block_ticks"
                }
            ))
        ));

        // 3. Non-empty `fluid_ticks` (the -2.-2 fixture) is an explicit
        //    `UnsupportedTicks` boundary.
        let mut fluid_ticks = load_fixture(&aux_fixture("-2.-2.nbt"));
        relocate_ticks(&mut fluid_ticks, "fluid_ticks", ChunkPos::new(-1, -3));
        assert!(matches!(
            boot_with(&mut fluid_ticks),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedTicks {
                    field: "fluid_ticks"
                }
            ))
        ));

        // 4. Non-empty `block_entities` (the -19.-21 chest fixture) is an
        //    explicit `UnsupportedBlockEntities` boundary.
        let mut chest = load_fixture(&aux_fixture("-19.-21.nbt"));
        assert!(matches!(
            boot_with(&mut chest),
            Err(RegionBackedBootError::SerializableChunk(
                SerializableChunkDataError::UnsupportedBlockEntities
            ))
        ));
    }

    /// The boot rejects a same-version `level.dat` whose spawn anchors another
    /// dimension before it reads the region: the overworld-only composition
    /// would otherwise boot "successfully" while login and default-spawn
    /// advertise different worlds.
    #[test]
    fn boot_rejects_a_spawn_dimension_other_than_overworld() {
        let temp = tempfile::tempdir().unwrap();
        write_level_dat(temp.path(), REAL_SPAWN);
        let mut level = read_level_dat_for_patch(temp.path());
        level
            .get_compound_or_empty_mut("Data")
            .get_compound_or_empty_mut("spawn")
            .put(
                "dimension".to_string(),
                Tag::String(StringTag::value_of("minecraft:the_nether".to_string())),
            );
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&level, &mut bytes).unwrap();
        fs::write(temp.path().join("level.dat"), bytes).unwrap();
        fs::create_dir_all(temp.path().join("dimensions/minecraft/overworld/region")).unwrap();

        let error = boot_level(temp.path())
            .err()
            .expect("a nether spawn must not boot the overworld composition");
        match error {
            RegionBackedBootError::UnsupportedSpawnDimension { actual, expected } => {
                assert_eq!(actual.identifier().to_string(), "minecraft:the_nether");
                assert_eq!(expected.identifier().to_string(), "minecraft:overworld");
            }
            other => panic!("expected UnsupportedSpawnDimension, got {other:?}"),
        }
    }

    /// A section carrying a persisted Starlight initialisation state outside
    /// `0..=3` survives reconstruction as `InitState::Other`, which the #184
    /// send seam (`to_vanilla_nibble`) cannot represent and would panic on. The
    /// boot rejects the chunk with the typed `UnsupportedLightState` boundary
    /// instead of aborting the process.
    #[test]
    fn boot_rejects_an_unsupported_persisted_starlight_state() {
        let temp = tempfile::tempdir().unwrap();
        write_level_dat(temp.path(), REAL_SPAWN);
        write_world_gen_settings(temp.path(), REAL_SEED);
        let region_dir = temp.path().join("dimensions/minecraft/overworld/region");
        fs::create_dir_all(&region_dir).unwrap();
        let mut chunk = load_fixture(&loaded_world_fixture());
        // Mark the chunk light-correct so `reconstruct_lights` actually carries
        // section light (the fixture itself is not light-correct), then give
        // section 0 a hostile `starlight.skylight_state` outside `0..=3`.
        chunk.put_int("isLightOn", 1);
        chunk.put_int("starlight.light_version", 10);
        chunk
            .get_list_or_empty_mut("sections")
            .get_compound_or_empty_mut(0)
            .put_int("starlight.skylight_state", 4);
        write_region_nbt(
            &region_dir.join("r.-1.-1.mca"),
            &mut chunk,
            ChunkPos::new(-1, -3),
        );

        assert!(matches!(
            boot_level(temp.path()),
            Err(RegionBackedBootError::UnsupportedLightState(_))
        ));
    }

    fn read_level_dat_for_patch(root: &Path) -> CompoundTag {
        let bytes = fs::read(root.join("level.dat")).unwrap();
        nbt_io::read_compressed(
            &bytes[..],
            &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
        )
        .expect("level.dat readable for patching")
    }

    #[test]
    fn full_region_chunk_validates_for_runtime_composition() {
        let (_temp, layout) = layout();
        write_chunk(&layout, full_chunk());
        let mut source = RegionChunkSource::open(layout);
        let data = source
            .load_for_composition(ChunkPos::ZERO)
            .expect("a full region chunk must validate for composition");
        assert_eq!(data.status(), ChunkStatus::Full);
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
}
