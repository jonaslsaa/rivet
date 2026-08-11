//! Loaded-world ground-truth extraction (issue #374).
//!
//! The official-client loaded-world PASS contract needs genuine per-coordinate
//! chunk content that cannot be faked by a server that merely echoes repeated
//! superflat bytes. This module reads a disposable copy of the safe copied
//! Minecraft 26.2 world (`working/client-worlds/New World`) *read-only* and
//! emits a deterministic manifest of the
//! overworld chunk content: for every region file under
//! `dimensions/minecraft/overworld/region`, for every allocated chunk, the
//! parsed `SerializableChunkData` fingerprint (status, stored position,
//! capability flags) plus the reconstructed section content sampled at
//! representative coordinates.
//!
//! Every read goes through `RegionFileStorage::new_read_only` /
//! `RegionFile::open_read_only` (added in this slice), which opens a plain read
//! descriptor and treats an allocated corrupt chunk as a hard `InvalidData`
//! error rather than an absent chunk. The extractor therefore can never create,
//! truncate, repair, back up, or otherwise mutate the disposable copy it
//! inspects.
//!
//! The manifest shape is deliberately small and canonical: it is what the
//! `rivet-loaded-world` runner later compares against the official client's
//! observed per-coordinate content, and what the negative controls tamper to
//! prove the comparator is not vacuously green. The `distinct` set of block
//! names across the sampled coordinates is per-chunk evidence of block
//! variety — a real chunk's columns carry more than one block type (surface,
//! bedrock, under-feet), so a server serving a uniform floor or identical
//! bytes for every chunk cannot reproduce the recorded per-coordinate content.
//! It does not claim the set is unique per chunk; the per-coordinate
//! `surface`/`bedrock`/`below_feet` arrays pin the content at the sampled
//! points, and it is that comparison the runner actually enforces.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::status::ChunkStatus;
use rivet_world::chunk::storage::region_file_storage::{
    RegionFileStorage, get_region_file_coordinates,
};
use rivet_world::chunk::storage::region_storage_info::RegionStorageInfo;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId, SectionBlockPredicates, current_version_container_factory, reconstruct_sections,
};
use rivet_world::chunk::storage::serializable_chunk_data::SerializableChunkData;
use rivet_world::level::height_accessor::{self, LevelHeightAccessor, SimpleLevelHeightAccessor};
use rivet_world::level::level::overworld;

/// The overworld's vertical extent (`minY=-64`, `height=384`). Section indices
/// returned by `reconstruct_sections` are `sectionY - minSectionY`.
const OVERWORLD_MIN_Y: i32 = -64;
const OVERWORLD_HEIGHT: i32 = 384;
/// Chunk-local column dimension (16×16 columns).
const CHUNK_SIZE: i32 = 16;
/// Chunk-local vertical dimension (16 blocks per section).
const SECTION_SIZE: i32 = 16;
/// The copied world's bedrock slab Y — the coordinate `#369`'s read-only
/// probe observed genuine bedrock at, and what the deep-structure evidence
/// samples at.
const BEDROCK_Y: i32 = -60;
/// One block below the bedrock slab — the dense stone body underneath, which a
/// superflat floor (bedrock only) does not have.
const BELOW_BEDROCK_Y: i32 = -61;

/// One chunk's ground-truth fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkFingerprint {
    /// The chunk's `Status` serialization name (e.g. `minecraft:full`).
    pub status: String,
    /// The parsed `xPos`/`zPos`.
    pub stored_pos: [i32; 2],
    /// Capability flags from `SerializableChunkData`: a chunk that the merged
    /// #519 full-chunk construction boundary cannot yet carry (non-empty
    /// `structures.starts`, non-empty entities) must be reported honestly, so
    /// the runner refuses PASS rather than trusting an incomplete server.
    pub capability_flags: Vec<String>,
    /// The distinct block names across the sampled coordinates, sorted — the
    /// anti-superflat evidence.
    pub distinct: Vec<String>,
    /// The 16×16 surface block names (block at the highest non-air y of that
    /// column, `minecraft:air` when the column is empty), row-major `z*16+x`.
    pub surface: Vec<String>,
    /// The 16×16 bedrock block names at `y=-60` — the deep-structure evidence
    /// that survives even when the surface is overgrown.
    pub bedrock: Vec<String>,
    /// The 16×16 block names at `y=-61`, one below the bedrock slab — proves
    /// depth into the dense stone body.
    pub below_feet: Vec<String>,
    /// The count of distinct block state ids across all populated sections — a
    /// coarse entropy measure that a repeated superflat byte pattern cannot
    /// reproduce per chunk.
    pub distinct_state_ids: usize,
    /// The number of populated (non-empty) sections.
    pub section_count: usize,
}

/// The deterministic ground-truth manifest for one disposable world copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldManifest {
    /// A stable manifest-format marker.
    pub format: u32,
    /// The overworld region directory the manifest was extracted from
    /// (world-relative, for diagnostics).
    pub overworld_region: String,
    /// Per-chunk fingerprints keyed by `"<x>,<z>"`.
    pub chunks: BTreeMap<String, ChunkFingerprint>,
}

/// Why a read-only world extraction failed. `Unverified` is a missing
/// prerequisite (no overworld region layout / no region files) — the harness
/// maps it to exit 3, never a fabricated green. Everything else is a hard gate
/// error.
#[derive(Debug)]
pub enum ExtractError {
    Unverified(String),
    Gate(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Unverified(m) => write!(f, "{m}"),
            ExtractError::Gate(m) => write!(f, "{m}"),
            ExtractError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<std::io::Error> for ExtractError {
    fn from(e: std::io::Error) -> Self {
        ExtractError::Io(e)
    }
}

/// The overworld region directory beneath a world root.
pub fn overworld_region_dir(root: &Path) -> PathBuf {
    root.join("dimensions")
        .join("minecraft")
        .join("overworld")
        .join("region")
}

/// The region storage info for a disposable overworld (chunk data, so the
/// coordinate guard runs — a mismatched chunk is reported, never misread).
fn storage_info(root: &Path) -> RegionStorageInfo {
    RegionStorageInfo::new(
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("disposable-world")
            .to_owned(),
        overworld(),
        "region".to_owned(),
        true,
    )
}

fn section_predicates() -> SectionBlockPredicates {
    SectionBlockPredicates {
        is_air: |state: &BlockState| state.is_air(),
        is_randomly_ticking: |state: &BlockState| state.random_ticking(),
        fluid_is_empty: |state: &BlockState| state.fluid_empty(),
        // Lava is Paper's only randomly-ticking vanilla fluid; water
        // (including waterlogged states) is not randomly ticking.
        fluid_is_randomly_ticking: |state: &BlockState| {
            !state.fluid_empty() && state.block().name() == "minecraft:lava"
        },
        // The extracted world contains no large collision shapes / moving
        // pistons; the callback seam is tested separately in rivet-world.
        is_special_colliding: |_| false,
    }
}

/// Extract the loaded-world ground-truth manifest from a disposable world copy.
///
/// The world root is read only: every region is opened with a read descriptor
/// and closed descriptor-only. Returns [`ExtractError::Unverified`] when the
/// overworld region layout is absent, so the runner reports UNVERIFIED rather
/// than fabricating a green.
pub fn extract_world(root: &Path) -> Result<WorldManifest, ExtractError> {
    let region_dir = overworld_region_dir(root);
    let entries = match std::fs::read_dir(&region_dir) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ExtractError::Unverified(format!(
                "disposable world {} has no overworld region directory {}",
                root.display(),
                region_dir.display()
            )));
        }
        Err(e) => return Err(ExtractError::Io(e)),
    };

    // Carry the parsed region coordinates alongside each path so the read loop
    // never re-parses (and never has to unwrap a filtered parse).
    let mut region_files: Vec<(PathBuf, ChunkPos)> = entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().map(|e| e == "mca").unwrap_or(false) {
                get_region_file_coordinates(&path).map(|coords| (path, coords))
            } else {
                None
            }
        })
        .collect();
    region_files.sort_by(|a, b| a.0.cmp(&b.0));
    if region_files.is_empty() {
        return Err(ExtractError::Unverified(format!(
            "disposable world {} has no overworld region files under {}",
            root.display(),
            region_dir.display()
        )));
    }

    let mut chunks = BTreeMap::new();
    let mut storage = RegionFileStorage::new_read_only(storage_info(root), region_dir.clone());
    let height = height_accessor::create(OVERWORLD_MIN_Y, OVERWORLD_HEIGHT);
    let factory = current_version_container_factory();
    let predicates = section_predicates();

    for (region_path, region_coords) in &region_files {
        for local_x in 0..32i32 {
            for local_z in 0..32i32 {
                let pos = ChunkPos::new(region_coords.x() + local_x, region_coords.z() + local_z);
                let Some(tag) = storage.read(&pos).map_err(|e| {
                    ExtractError::Gate(format!(
                        "failed read-only region read of {} ({pos}): {e}",
                        region_path.display()
                    ))
                })?
                else {
                    continue;
                };
                let data = match SerializableChunkData::parse(height, &tag) {
                    Ok(Some(data)) => data,
                    Ok(None) => continue, // no Status — Paper drops it before DataVersion
                    Err(e) => {
                        return Err(ExtractError::Gate(format!(
                            "parsing chunk {pos} from {}: {e}",
                            region_path.display()
                        )));
                    }
                };

                let fingerprint = fingerprint_chunk(&data, &factory, &predicates, &height)?;
                chunks.insert(format!("{},{}", pos.x(), pos.z()), fingerprint);
            }
        }
    }

    Ok(WorldManifest {
        format: 1,
        overworld_region: region_dir
            .strip_prefix(root)
            .unwrap_or(&region_dir)
            .to_string_lossy()
            .into_owned(),
        chunks,
    })
}

fn fingerprint_chunk(
    data: &SerializableChunkData,
    factory: &rivet_world::chunk::paletted_container_factory::PalettedContainerFactory<
        BlockState,
        BiomeId,
    >,
    predicates: &SectionBlockPredicates,
    height: &SimpleLevelHeightAccessor,
) -> Result<ChunkFingerprint, ExtractError> {
    let min_section = height.get_min_section_y();
    let max_section = height.get_max_section_y();
    let reconstruction = reconstruct_sections(
        data.section_tags(),
        min_section,
        max_section,
        factory,
        *predicates,
    )
    .map_err(|e| {
        ExtractError::Gate(format!(
            "reconstructing sections of chunk {}: {e}",
            data.stored_pos()
        ))
    })?;

    let pos = data.stored_pos();
    let chunk_x = pos.x();
    let chunk_z = pos.z();

    let mut surface = vec!["minecraft:air".to_owned(); (CHUNK_SIZE * CHUNK_SIZE) as usize];
    let mut bedrock = vec!["minecraft:air".to_owned(); (CHUNK_SIZE * CHUNK_SIZE) as usize];
    let mut below_feet = vec!["minecraft:air".to_owned(); (CHUNK_SIZE * CHUNK_SIZE) as usize];
    let mut distinct: BTreeSet<String> = BTreeSet::new();
    let mut distinct_state_ids: BTreeSet<u16> = BTreeSet::new();
    let mut section_count = 0usize;

    for (index, section) in reconstruction.sections.iter().enumerate() {
        let Some(section) = section else { continue };
        section_count += 1;
        let section_y = min_section + index as i32;
        for local_x in 0..CHUNK_SIZE {
            for local_z in 0..CHUNK_SIZE {
                for local_y in 0..SECTION_SIZE {
                    let state = section.get_block_state(local_x, local_y, local_z);
                    if state.is_air() {
                        continue;
                    }
                    let name = state.block().name();
                    distinct.insert(name.to_owned());
                    distinct_state_ids.insert(state.id().0);
                    let abs_y = section_y * SECTION_SIZE + local_y;
                    // Highest non-air per column. Sections iterate low→high and
                    // local_y 0→15, so the last non-air write is the highest.
                    surface[(local_z * CHUNK_SIZE + local_x) as usize] = name.to_owned();
                    if abs_y == BEDROCK_Y {
                        bedrock[(local_z * CHUNK_SIZE + local_x) as usize] = name.to_owned();
                    }
                    if abs_y == BELOW_BEDROCK_Y {
                        below_feet[(local_z * CHUNK_SIZE + local_x) as usize] = name.to_owned();
                    }
                }
            }
        }
    }

    // Capability flags: report only what the merged #519 full-chunk
    // construction boundary cannot yet carry, so a chunk beyond that boundary
    // is recorded honestly, never silently accepted. (#519 constructs FULL
    // chunks carrying block entities, stored block/fluid ticks, and
    // `structures.References`; an empty or references-only structures compound
    // is no longer a flag. The still-uncarried surfaces are non-empty
    // `structures.starts` — the `StructureStart` load path is not ported —
    // and non-empty entities.) `status` non-FULL is folded in separately.
    let mut flags = Vec::new();
    if data.has_unsupported_structure_starts() {
        flags.push("structures".to_owned());
    }
    if !data.entities().is_empty() {
        flags.push("entities".to_owned());
    }

    Ok(ChunkFingerprint {
        status: data.status().serialization_name().to_owned(),
        stored_pos: [chunk_x, chunk_z],
        capability_flags: {
            let mut all = Vec::new();
            if data.status() != ChunkStatus::Full {
                all.push(format!("status:{}", data.status().serialization_name()));
            }
            all.extend_from_slice(&flags);
            all
        },
        distinct: distinct.into_iter().collect(),
        surface,
        bedrock,
        below_feet,
        distinct_state_ids: distinct_state_ids.len(),
        section_count,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::tag::Tag;

    use super::*;

    /// A real Minecraft 26.2 chunk NBT (a fully-populated `minecraft:full`
    /// chunk) captured from the pinned Paper oracle. Sharing the fixture that
    /// rivet-world's read-only storage tests already exercise means this test
    /// is a genuine end-to-end read, not a hand-built approximation.
    const PAPER_CHUNK_0_0: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/regions/superflat-full/chunk/overworld/0.0/0.0.nbt"
    ));

    /// Build a disposable world root holding one region file with the given
    /// chunk at `pos` (uncompressed v3 record, mirroring rivet-world's
    /// `write_region` test helper).
    fn world_with_chunk(pos: ChunkPos, payload: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let region = dir.path().join("dimensions/minecraft/overworld/region");
        fs::create_dir_all(&region).unwrap();

        let path = region.join(format!("r.{}.{}.mca", pos.x(), pos.z()));
        let record_len = 5 + payload.len();
        let sectors = record_len.div_ceil(4096);
        let mut bytes = vec![0u8; 8192 + sectors * 4096];
        let slot = (pos.get_region_local_x() + pos.get_region_local_z() * 32) as usize;
        bytes[slot * 4..slot * 4 + 4]
            .copy_from_slice(&((2i32 << 8) | sectors as i32).to_be_bytes());
        bytes[8192..8196].copy_from_slice(&((payload.len() as i32) + 1).to_be_bytes());
        bytes[8196] = 3;
        bytes[8197..8197 + payload.len()].copy_from_slice(payload);
        fs::write(path, bytes).unwrap();
        dir
    }

    #[test]
    fn extract_world_reads_real_full_chunk_content() {
        let dir = world_with_chunk(ChunkPos::ZERO, PAPER_CHUNK_0_0);
        let manifest = extract_world(dir.path()).unwrap();

        assert_eq!(manifest.format, 1);
        assert_eq!(
            manifest.overworld_region,
            "dimensions/minecraft/overworld/region"
        );
        let chunks: Vec<_> = manifest.chunks.keys().collect();
        assert_eq!(chunks, vec!["0,0"]);

        let chunk = &manifest.chunks["0,0"];
        assert_eq!(chunk.status, "minecraft:full");
        assert_eq!(chunk.stored_pos, [0, 0]);
        // A real chunk carries a distinct content signature: the anti-superflat
        // evidence. A server echoing repeated bytes for every chunk cannot
        // reproduce a distinct set of three different blocks.
        assert_eq!(
            chunk.distinct,
            vec![
                "minecraft:bedrock",
                "minecraft:dirt",
                "minecraft:grass_block"
            ]
        );
        assert_eq!(chunk.distinct_state_ids, 3);
        assert_eq!(chunk.section_count, 24);
        // Every column has a non-air surface (grass/dirt), and depth evidence
        // one block below the bedrock slab.
        assert_eq!(
            chunk
                .surface
                .iter()
                .filter(|b| **b != "minecraft:air")
                .count(),
            256
        );
        assert_eq!(
            chunk
                .below_feet
                .iter()
                .filter(|b| **b != "minecraft:air")
                .count(),
            256
        );
    }

    #[test]
    fn extract_world_reports_non_full_status_honestly() {
        // Relabel the fixture chunk's Status to a pre-full status. The
        // extractor must not silently upgrade it: it reports the true status and
        // stamps a `status:<prefull>` capability flag so the runner refuses
        // PASS. (The section tags in the fixture are still the real full
        // sections — the label alone does not empty them — so the surface is
        // faithfully reported, not fabricated.)
        let payload = tamper_status(PAPER_CHUNK_0_0, "minecraft:structure_starts");
        let dir = world_with_chunk(ChunkPos::ZERO, &payload);
        let manifest = extract_world(dir.path()).unwrap();
        let chunk = &manifest.chunks["0,0"];

        assert_eq!(chunk.status, "minecraft:structure_starts");
        assert!(
            chunk
                .capability_flags
                .contains(&"status:minecraft:structure_starts".to_owned())
        );
    }

    #[test]
    fn extract_world_is_unverified_without_region_layout() {
        // No dimensions/minecraft/overworld/region dir at all -> UNVERIFIED,
        // never a fabricated green.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("level.dat"), b"not a world").unwrap();
        assert!(matches!(
            extract_world(dir.path()),
            Err(ExtractError::Unverified(_))
        ));
    }

    #[test]
    fn extract_world_is_unverified_without_region_files() {
        // The region dir exists but holds no `.mca` region files (only a stray
        // file) -> UNVERIFIED.
        let dir = tempfile::tempdir().unwrap();
        let region = dir.path().join("dimensions/minecraft/overworld/region");
        fs::create_dir_all(&region).unwrap();
        fs::write(region.join("readme.txt"), b"not a region").unwrap();
        assert!(matches!(
            extract_world(dir.path()),
            Err(ExtractError::Unverified(_))
        ));
    }

    #[test]
    fn extract_world_never_mutates_the_disposable_copy() {
        let dir = world_with_chunk(ChunkPos::ZERO, PAPER_CHUNK_0_0);
        let region_path = dir
            .path()
            .join("dimensions/minecraft/overworld/region/r.0.0.mca");
        let before = fs::read(&region_path).unwrap();

        extract_world(dir.path()).unwrap();

        // Byte-for-byte unchanged: no padding, no header repair, no backup.
        assert_eq!(fs::read(&region_path).unwrap(), before);
        assert_eq!(
            fs::read_dir(dir.path().join("dimensions/minecraft/overworld/region"))
                .unwrap()
                .count(),
            1
        );
    }

    /// Rewrite the fixture chunk's `Status` field to `status` (a string-tag
    /// replacement, preserving everything else) so a test can observe how the
    /// extractor classifies a non-FULL chunk.
    fn tamper_status(original: &[u8], status: &str) -> Vec<u8> {
        use std::io::Cursor;

        use rivet_nbt::nbt_io;
        use rivet_util::data_io::{DataInputStream, DataOutputStream};

        let mut input = DataInputStream::new(Cursor::new(original));
        let mut tag = nbt_io::read_unlimited(&mut input).unwrap();
        tag.put(
            "Status".to_owned(),
            rivet_nbt::tag::Tag::String(rivet_nbt::string_tag::StringTag::value_of(
                status.to_owned(),
            )),
        );
        let mut out = Vec::new();
        nbt_io::write(&tag, &mut DataOutputStream::new(&mut out)).unwrap();
        out
    }

    /// Rewrite the fixture chunk's `structures` compound to `structures`
    /// (preserving everything else) so a test can pin the #519 capability
    /// boundary: an empty container and a References-only container must not be
    /// flagged, while a non-empty `starts` compound must.
    fn tamper_structures(original: &[u8], structures: CompoundTag) -> Vec<u8> {
        use std::io::Cursor;

        use rivet_nbt::nbt_io;
        use rivet_util::data_io::{DataInputStream, DataOutputStream};

        let mut input = DataInputStream::new(Cursor::new(original));
        let mut tag = nbt_io::read_unlimited(&mut input).unwrap();
        tag.put("structures".to_owned(), Tag::Compound(structures));
        let mut out = Vec::new();
        nbt_io::write(&tag, &mut DataOutputStream::new(&mut out)).unwrap();
        out
    }

    fn fingerprint_flags(payload: &[u8]) -> Vec<String> {
        let dir = world_with_chunk(ChunkPos::ZERO, payload);
        let manifest = extract_world(dir.path()).unwrap();
        manifest.chunks["0,0"].capability_flags.clone()
    }

    /// An empty `structures` compound is within the #519 boundary — nothing to
    /// flag.
    #[test]
    fn fingerprint_does_not_flag_an_empty_structures_container() {
        let flags = fingerprint_flags(&tamper_structures(PAPER_CHUNK_0_0, CompoundTag::new()));
        assert!(
            !flags.iter().any(|f| f == "structures"),
            "an empty structures container must not be flagged, got {flags:?}"
        );
    }

    /// A References-only structures compound (the normal FULL-chunk case:
    /// `structures.References` decodes into carried `StructureReference`s) is
    /// within the #519 boundary — not flagged merely because keys exist.
    #[test]
    fn fingerprint_does_not_flag_a_references_only_structures_container() {
        let mut references = CompoundTag::new();
        references.put_long_array("minecraft:mineshaft", vec![-25769803771]);
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));

        let flags = fingerprint_flags(&tamper_structures(PAPER_CHUNK_0_0, structures));
        assert!(
            !flags.iter().any(|f| f == "structures"),
            "references-only structures must not be flagged, got {flags:?}"
        );
    }

    /// A non-empty `structures.starts` compound is the one structures surface
    /// the #519 boundary cannot yet carry (the `StructureStart` load path is
    /// not ported) — it must be flagged so the runner refuses PASS.
    #[test]
    fn fingerprint_flags_a_non_empty_structures_starts_container() {
        let mut starts = CompoundTag::new();
        starts.put_int("minecraft:village", 1);
        let mut structures = CompoundTag::new();
        structures.put("starts".into(), Tag::Compound(starts));

        let flags = fingerprint_flags(&tamper_structures(PAPER_CHUNK_0_0, structures));
        assert!(
            flags.iter().any(|f| f == "structures"),
            "non-empty structures.starts must be flagged, got {flags:?}"
        );
    }
}
