//! Storage-only #231 V1a Anvil round-trip oracle.
//!
//! This command deliberately stops at the region container boundary: it owns
//! the committed CompoundTag payloads, writes them through a fresh
//! `RegionFileStorage` with compression pinned to `none`, and reads them back
//! through a newly-created read-only storage. It does not parse or reconstruct
//! runtime chunks and it does not claim V1b/FULL/generated-world evidence.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::storage::{RegionFileStorage, RegionStorageInfo, get_chunk_coordinate};
use rivet_world::level::{end, nether, overworld};
use serde::Serialize;

use crate::mutate::{encode_payload, parse_payload};
use crate::{Error, sha256_hex};

const EXPECTED_CHUNK_COUNT: usize = 432;
const REGION_FILE_COMPRESSION: &str = "none";
const ROUNDTRIP_KIND: &str = "anvil-roundtrip-v1a";

#[derive(Debug, Clone)]
struct Fixture {
    dim: String,
    region: String,
    cx: i32,
    cz: i32,
    path: String,
    source_bytes: Vec<u8>,
    source_tag: CompoundTag,
}

#[derive(Debug, Clone, Serialize)]
struct PayloadHash {
    bytes: usize,
    sha256: String,
    xxh3_64: String,
    xxh3_64_canonical: String,
}

#[derive(Debug, Clone, Serialize)]
struct RecordMetadata {
    region_file: String,
    slot: usize,
    sector_offset: u32,
    sector_count: u8,
    length: u32,
    compression: u8,
    payload_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkEvidence {
    dim: String,
    region: String,
    cx: i32,
    cz: i32,
    fixture_path: String,
    source: PayloadHash,
    saved: PayloadHash,
    reloaded: PayloadHash,
    record: RecordMetadata,
}

#[derive(Debug, Clone, Serialize)]
struct TreeEntry {
    path: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct NegativeEvidence {
    name: String,
    artifact: String,
    detected: String,
}

#[derive(Debug, Clone, Serialize)]
struct Report {
    format: u32,
    kind: String,
    verdict: String,
    source_fixture_root: String,
    source_manifest_sha256: String,
    source_manifest_kind: String,
    source_seed: String,
    region_file_compression: String,
    expected_chunk_count: usize,
    source_chunk_count: usize,
    region_tree_hash_before_read_only_reload: String,
    region_tree_hash_after_read_only_reload: String,
    region_tree_file_count: usize,
    region_tree_files: Vec<TreeEntry>,
    chunks: Vec<ChunkEvidence>,
    corruption_negatives: Vec<NegativeEvidence>,
    non_evidence: Vec<String>,
}

#[derive(Debug, Clone)]
struct TreeHash {
    digest: String,
    files: Vec<TreeEntry>,
}

#[derive(Debug, Clone)]
struct RegionScan {
    records: BTreeMap<(i32, i32), RecordMetadata>,
}

#[derive(Debug, Clone, Copy)]
enum NegativeKind {
    Length,
    Compression,
    Location,
    Overlap,
    Truncation,
}

impl NegativeKind {
    fn name(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Compression => "compression-byte",
            Self::Location => "location-header",
            Self::Overlap => "sector-overlap",
            Self::Truncation => "truncation",
        }
    }

    fn all() -> [Self; 5] {
        [
            Self::Length,
            Self::Compression,
            Self::Location,
            Self::Overlap,
            Self::Truncation,
        ]
    }
}

/// Run `anvil-roundtrip-v1a`.
///
/// The optional positional argument selects the committed fixture root. Use
/// `--out <dir>` to retain the region tree, corruption copies, and machine
/// readable evidence report. With no `--out`, the report is retained under the
/// ignored oracle work directory.
pub fn run_cli(args: &[&str]) -> Result<(), Error> {
    let mut fixture_root = crate::crate_dir().join("fixtures");
    let mut output = crate::crate_dir().join("work/anvil-roundtrip-v1a");
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--help" | "-h" if args.len() == 1 => {
                println!(
                    "usage: cargo run -p rivet-oracle -- anvil-roundtrip-v1a [fixtures] [--out <dir>]"
                );
                return Ok(());
            }
            "--out" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::Gate(
                        "anvil-roundtrip-v1a --out requires a destination directory".into(),
                    ));
                };
                output = PathBuf::from(path);
                i += 2;
            }
            value if value.starts_with('-') => {
                return Err(Error::Gate(format!(
                    "unknown anvil-roundtrip-v1a option `{value}`"
                )));
            }
            value => {
                if fixture_root != crate::crate_dir().join("fixtures") {
                    return Err(Error::Gate(
                        "anvil-roundtrip-v1a accepts only one fixture directory".into(),
                    ));
                }
                fixture_root = PathBuf::from(value);
                i += 1;
            }
        }
    }

    run_roundtrip(&fixture_root, &output)
}

fn run_roundtrip(fixture_root: &Path, output: &Path) -> Result<(), Error> {
    let fixtures = load_fixtures(fixture_root)?;
    if fixtures.len() != EXPECTED_CHUNK_COUNT {
        return Err(Error::Unverified(format!(
            "anvil-roundtrip-v1a requires the committed {EXPECTED_CHUNK_COUNT}-chunk M0 corpus, found {}",
            fixtures.len()
        )));
    }

    prepare_output(output)?;
    let regions_root = output.join("regions");
    fs::create_dir_all(&regions_root)?;
    let mut saved_hashes = HashMap::new();

    write_fixture_regions(&fixtures, &regions_root, &mut saved_hashes)?;
    let before_reload = hash_tree(&regions_root)?;
    let records = scan_region_tree(&regions_root, &fixtures)?;

    let reloaded_hashes = read_fresh_read_only(&fixtures, &regions_root, &saved_hashes, &records)?;
    let after_reload = hash_tree(&regions_root)?;
    if before_reload.digest != after_reload.digest {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a read-only reload mutated the saved region tree: before {}, after {}",
            before_reload.digest, after_reload.digest
        )));
    }

    let mut chunks = Vec::with_capacity(fixtures.len());
    for fixture in &fixtures {
        let key = fixture_key(fixture);
        let source = payload_hash(&fixture.source_bytes, &fixture.source_tag)?;
        let saved = saved_hashes.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "anvil-roundtrip-v1a missing saved payload evidence for {}",
                fixture.path
            ))
        })?;
        let reloaded = reloaded_hashes.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "anvil-roundtrip-v1a missing reloaded payload evidence for {}",
                fixture.path
            ))
        })?;
        let record = records
            .get(&fixture.dim)
            .and_then(|scan| scan.records.get(&(fixture.cx, fixture.cz)))
            .ok_or_else(|| {
                Error::Gate(format!(
                    "anvil-roundtrip-v1a missing record metadata for {}",
                    fixture.path
                ))
            })?;
        chunks.push(ChunkEvidence {
            dim: fixture.dim.clone(),
            region: fixture.region.clone(),
            cx: fixture.cx,
            cz: fixture.cz,
            fixture_path: fixture.path.clone(),
            source,
            saved: saved.clone(),
            reloaded: reloaded.clone(),
            record: record.clone(),
        });
    }

    let negatives = run_corruption_negatives(output, &regions_root, &fixtures)?;
    let manifest_bytes = fs::read(fixture_root.join("manifest.json"))?;
    let manifest = crate::load_manifest(fixture_root)?;
    let source_seed = manifest.seed.clone().ok_or_else(|| {
        Error::Unverified("M0 fixture manifest is missing seed provenance".into())
    })?;
    let source_kind = manifest.kind.clone().ok_or_else(|| {
        Error::Unverified("M0 fixture manifest is missing capture-kind provenance".into())
    })?;

    let report = Report {
        format: 1,
        kind: ROUNDTRIP_KIND.to_string(),
        verdict: "PASS".to_string(),
        source_fixture_root: fixture_root.display().to_string(),
        source_manifest_sha256: sha256_hex(&manifest_bytes),
        source_manifest_kind: source_kind,
        source_seed,
        region_file_compression: REGION_FILE_COMPRESSION.to_string(),
        expected_chunk_count: EXPECTED_CHUNK_COUNT,
        source_chunk_count: fixtures.len(),
        region_tree_hash_before_read_only_reload: before_reload.digest,
        region_tree_hash_after_read_only_reload: after_reload.digest,
        region_tree_file_count: before_reload.files.len(),
        region_tree_files: before_reload.files,
        chunks,
        corruption_negatives: negatives,
        non_evidence: vec![
            "V1a covers storage-only CompoundTag region round-trip; it does not claim V1b whole-region parity.".into(),
            "V1a does not run SerializableChunkData parsing/reconstruction or generated-world FULL parity.".into(),
            "Semantic xxh3_64 is diagnostic evidence only; raw payload equality is authoritative.".into(),
        ],
    };
    let report_path = output.join("report.json");
    let report_bytes = serde_json::to_vec_pretty(&report)
        .map_err(|e| Error::Gate(format!("cannot serialize anvil-roundtrip-v1a report: {e}")))?;
    fs::write(&report_path, report_bytes)?;

    println!(
        "PASS: anvil-roundtrip-v1a storage-only round-trip verified {} chunks; evidence {}",
        fixtures.len(),
        report_path.display()
    );
    Ok(())
}

fn load_fixtures(fixture_root: &Path) -> Result<Vec<Fixture>, Error> {
    if !fixture_root.is_dir() || !fixture_root.join("manifest.json").is_file() {
        return Err(Error::Unverified(format!(
            "anvil-roundtrip-v1a missing fixture provenance/artifacts at {}",
            fixture_root.display()
        )));
    }
    let declared_manifest = crate::load_manifest(fixture_root)?;
    for captured in &declared_manifest.captured {
        if !is_safe_relative_path(&captured.path) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest artifact path escapes fixture root: {}",
                captured.path
            )));
        }
        if !fixture_root.join(&captured.path).exists() {
            return Err(Error::Unverified(format!(
                "anvil-roundtrip-v1a missing captured artifact {}",
                captured.path
            )));
        }
    }
    let manifest = crate::verify_fixtures(fixture_root)?;
    if manifest.kind.as_deref() != Some(crate::KIND_M0) {
        return Err(Error::Unverified(format!(
            "anvil-roundtrip-v1a requires manifest kind m0, found {:?}",
            manifest.kind
        )));
    }
    if manifest.seed.is_none() || manifest.paper.is_none() {
        return Err(Error::Unverified(
            "anvil-roundtrip-v1a requires seed and Paper provenance in manifest.json".into(),
        ));
    }

    let mut expected_paths = HashSet::new();
    let mut expected_tuples = HashSet::new();
    let mut fixtures = Vec::new();
    for captured in manifest
        .captured
        .iter()
        .filter(|c| c.path.starts_with("chunk/"))
    {
        if !expected_paths.insert(captured.path.clone()) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a duplicate fixture artifact in manifest: {}",
                captured.path
            )));
        }
        let parts: Vec<&str> = captured.path.split('/').collect();
        if parts.len() != 4 || parts[0] != "chunk" || !parts[3].ends_with(".nbt") {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a malformed fixture tuple path: {}",
                captured.path
            )));
        }
        let dim = parts[1].to_string();
        if !matches!(dim.as_str(), "overworld" | "the_nether" | "the_end") {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a unsupported fixture dimension in tuple path: {}",
                captured.path
            )));
        }
        if let Some(declared_dim) = captured.dim.as_deref()
            && declared_dim != dim
        {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a fixture dimension provenance mismatch for {}: path {}, manifest {}",
                captured.path, dim, declared_dim
            )));
        }
        let region = parts[2].to_string();
        let stem = parts[3].strip_suffix(".nbt").unwrap_or_default();
        let (cx, cz) = parse_coordinate(stem).map_err(|message| {
            Error::Gate(format!(
                "anvil-roundtrip-v1a malformed fixture tuple {}: {message}",
                captured.path
            ))
        })?;
        let expected_region = format!("{}.{}", cx >> 5, cz >> 5);
        if region != expected_region {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a fixture region tuple mismatch for {}: coordinate ({cx},{cz}) belongs to {expected_region}, path says {region}",
                captured.path
            )));
        }
        if !expected_tuples.insert((dim.clone(), region.clone(), cx, cz)) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a duplicate fixture tuple {dim}/{region}/{cx}.{cz}"
            )));
        }
        let path = fixture_root.join(&captured.path);
        let metadata = fs::symlink_metadata(&path).map_err(|e| {
            Error::Unverified(format!(
                "anvil-roundtrip-v1a missing fixture artifact {}: {e}",
                captured.path
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a fixture artifact is not a regular file: {}",
                captured.path
            )));
        }
        let source_bytes = fs::read(&path)?;
        let source_tag = parse_payload(&source_bytes)
            .map_err(|e| Error::Gate(format!("cannot parse {}: {e}", captured.path)))?;
        fixtures.push(Fixture {
            dim,
            region,
            cx,
            cz,
            path: captured.path.clone(),
            source_bytes,
            source_tag,
        });
    }
    if fixtures.len() != EXPECTED_CHUNK_COUNT {
        return Err(Error::Unverified(format!(
            "anvil-roundtrip-v1a fixture manifest contains {} chunk entries, expected {EXPECTED_CHUNK_COUNT}",
            fixtures.len()
        )));
    }
    verify_fixture_tree_closure(fixture_root, &expected_paths, &fixtures)?;
    fixtures.sort_by_key(fixture_key);
    Ok(fixtures)
}

fn verify_fixture_tree_closure(
    fixture_root: &Path,
    expected_paths: &HashSet<String>,
    fixtures: &[Fixture],
) -> Result<(), Error> {
    let chunk_root = fixture_root.join("chunk");
    if !chunk_root.is_dir() {
        return Err(Error::Unverified(format!(
            "anvil-roundtrip-v1a missing chunk fixture directory {}",
            chunk_root.display()
        )));
    }
    let actual = collect_files(&chunk_root)?;
    let mut identities = HashMap::new();
    for path in &actual {
        let relative = path
            .strip_prefix(fixture_root)
            .map_err(|e| {
                Error::Gate(format!(
                    "cannot relativize fixture path {}: {e}",
                    path.display()
                ))
            })?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if !expected_paths.contains(&relative) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a extra fixture artifact: {relative}"
            )));
        }
        let identity = file_identity(path)?;
        if let Some(previous) = identities.insert(identity, relative.clone()) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a aliased fixture artifacts: {previous} and {relative}"
            )));
        }
    }
    if actual.len() != fixtures.len() {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a fixture file closure mismatch: expected {} files, found {}",
            fixtures.len(),
            actual.len()
        )));
    }
    Ok(())
}

fn write_fixture_regions(
    fixtures: &[Fixture],
    regions_root: &Path,
    saved_hashes: &mut HashMap<String, PayloadHash>,
) -> Result<(), Error> {
    rivet_world::chunk::storage::RegionFileVersion::configure(REGION_FILE_COMPRESSION);
    let mut by_dim: BTreeMap<&str, Vec<&Fixture>> = BTreeMap::new();
    for fixture in fixtures {
        by_dim.entry(&fixture.dim).or_default().push(fixture);
    }
    for (dim, dim_fixtures) in by_dim {
        let folder = regions_root.join(dim);
        fs::create_dir_all(&folder)?;
        let mut storage = RegionFileStorage::new(info_for_dimension(dim), folder, false);
        for fixture in dim_fixtures {
            storage
                .write(
                    &ChunkPos::new(fixture.cx, fixture.cz),
                    Some(fixture.source_tag.clone()),
                )
                .map_err(|e| Error::Gate(format!("writing {} failed: {e}", fixture.path)))?;
        }
        for fixture in fixtures.iter().filter(|f| f.dim == dim) {
            let tag = storage
                .read(&ChunkPos::new(fixture.cx, fixture.cz))
                .map_err(|e| Error::Gate(format!("reading saved {} failed: {e}", fixture.path)))?
                .ok_or_else(|| Error::Gate(format!("saved payload absent for {}", fixture.path)))?;
            let bytes = encode_payload(&tag)
                .map_err(|e| Error::Gate(format!("encoding saved {} failed: {e}", fixture.path)))?;
            let hash = payload_hash(&bytes, &tag)?;
            if bytes != fixture.source_bytes {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a saved payload mismatch at {}: source {} saved {}",
                    fixture.path,
                    crate::hash::xxh3_64_hex(&fixture.source_bytes),
                    hash.xxh3_64
                )));
            }
            saved_hashes.insert(fixture_key(fixture), hash);
        }
        storage
            .flush()
            .map_err(|e| Error::Gate(format!("flushing dimension {dim} failed: {e}")))?;
        storage
            .close()
            .map_err(|e| Error::Gate(format!("closing dimension {dim} failed: {e}")))?;
    }
    Ok(())
}

fn read_fresh_read_only(
    fixtures: &[Fixture],
    regions_root: &Path,
    saved_hashes: &HashMap<String, PayloadHash>,
    records: &BTreeMap<String, RegionScan>,
) -> Result<HashMap<String, PayloadHash>, Error> {
    let mut reloaded = HashMap::new();
    let dimensions: BTreeSet<String> = fixtures.iter().map(|f| f.dim.clone()).collect();
    for dim in dimensions {
        let folder = regions_root.join(&dim);
        let mut storage = RegionFileStorage::new_read_only(info_for_dimension(&dim), folder);
        for fixture in fixtures.iter().filter(|f| f.dim == dim) {
            let tag = storage
                .read(&ChunkPos::new(fixture.cx, fixture.cz))
                .map_err(|e| {
                    Error::Gate(format!("read-only reload of {} failed: {e}", fixture.path))
                })?
                .ok_or_else(|| {
                    Error::Gate(format!("read-only payload absent for {}", fixture.path))
                })?;
            let bytes = encode_payload(&tag).map_err(|e| {
                Error::Gate(format!("encoding reloaded {} failed: {e}", fixture.path))
            })?;
            let hash = payload_hash(&bytes, &tag)?;
            let saved = saved_hashes.get(&fixture_key(fixture)).ok_or_else(|| {
                Error::Gate(format!("saved evidence absent for {}", fixture.path))
            })?;
            if bytes != fixture.source_bytes || hash.xxh3_64 != saved.xxh3_64 {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a reloaded payload mismatch at {}: source {} reloaded {}",
                    fixture.path,
                    crate::hash::xxh3_64_hex(&fixture.source_bytes),
                    hash.xxh3_64
                )));
            }
            if !records
                .get(&dim)
                .is_some_and(|scan| scan.records.contains_key(&(fixture.cx, fixture.cz)))
            {
                return Err(Error::Gate(format!(
                    "record metadata disappeared before reloading {}",
                    fixture.path
                )));
            }
            reloaded.insert(fixture_key(fixture), hash);
        }
        storage
            .close()
            .map_err(|e| Error::Gate(format!("closing read-only dimension {dim} failed: {e}")))?;
    }
    Ok(reloaded)
}

fn scan_region_tree(
    regions_root: &Path,
    fixtures: &[Fixture],
) -> Result<BTreeMap<String, RegionScan>, Error> {
    let mut expected_by_dim: BTreeMap<String, BTreeMap<(i32, i32), String>> = BTreeMap::new();
    for fixture in fixtures {
        expected_by_dim
            .entry(fixture.dim.clone())
            .or_default()
            .insert((fixture.cx, fixture.cz), fixture.path.clone());
    }
    let mut scans = BTreeMap::new();
    for (dim, expected) in expected_by_dim {
        let folder = regions_root.join(&dim);
        let files = collect_files(&folder)?;
        let mut region_files = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(&folder)
                .unwrap_or(&path)
                .to_string_lossy();
            if !relative.starts_with("r.") || !relative.ends_with(".mca") {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a extra saved artifact in {dim}: {relative}"
                )));
            }
            region_files.push(path);
        }
        if region_files.len() != 1 {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a expected one saved region for {dim}, found {}",
                region_files.len()
            )));
        }
        let scan = scan_region_file(&region_files[0], &dim, &expected)?;
        scans.insert(dim, scan);
    }
    Ok(scans)
}

fn scan_region_file(
    path: &Path,
    dim: &str,
    expected: &BTreeMap<(i32, i32), String>,
) -> Result<RegionScan, Error> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let (region_x, region_z) = parse_region_filename(name).ok_or_else(|| {
        Error::Gate(format!(
            "anvil-roundtrip-v1a invalid region artifact {dim}/{}",
            path.display()
        ))
    })?;
    let bytes = fs::read(path)?;
    if bytes.len() < 8192 || bytes.len() % 4096 != 0 {
        return Err(scanner_error(
            dim,
            path,
            "truncation",
            "header/file length is not a complete sector layout",
        ));
    }
    let total_sectors = bytes.len() / 4096;
    let mut used = vec![false; total_sectors];
    used[0] = true;
    used[1] = true;
    let mut records = BTreeMap::new();
    for slot in 0..1024usize {
        let packed = u32::from_be_bytes(
            bytes[slot * 4..slot * 4 + 4]
                .try_into()
                .expect("location entry is four bytes"),
        );
        if packed == 0 {
            continue;
        }
        let cx = region_x * 32 + (slot as i32 & 31);
        let cz = region_z * 32 + (slot as i32 >> 5);
        let artifact = expected.get(&(cx, cz)).ok_or_else(|| {
            scanner_error(
                dim,
                path,
                "location-header",
                &format!("extra allocated record at chunk ({cx},{cz})"),
            )
        })?;
        let sector_offset = packed >> 8;
        let sector_count = (packed & 0xff) as u8;
        if sector_offset < 2 || sector_count == 0 {
            return Err(scanner_error(
                dim,
                path,
                "location-header",
                &format!("{artifact} chunk ({cx},{cz}) has invalid location {packed:#x}"),
            ));
        }
        let start = usize::try_from(sector_offset).map_err(|_| {
            scanner_error(
                dim,
                path,
                "location-header",
                "sector offset does not fit usize",
            )
        })?;
        let end = start
            .checked_add(usize::from(sector_count))
            .ok_or_else(|| scanner_error(dim, path, "location-header", "sector span overflows"))?;
        if end > total_sectors {
            return Err(scanner_error(
                dim,
                path,
                "truncation",
                &format!("{artifact} chunk ({cx},{cz}) extends beyond file bounds"),
            ));
        }
        for (sector, occupied) in used.iter_mut().enumerate().take(end).skip(start) {
            if *occupied {
                return Err(scanner_error(
                    dim,
                    path,
                    "sector-overlap",
                    &format!("{artifact} chunk ({cx},{cz}) overlaps sector {sector}"),
                ));
            }
            *occupied = true;
        }
        let record_start = start * 4096;
        let record_end = end * 4096;
        if record_end - record_start < 5 {
            return Err(scanner_error(
                dim,
                path,
                "truncation",
                &format!("{artifact} chunk ({cx},{cz}) has no five-byte record header"),
            ));
        }
        let length_i32 = i32::from_be_bytes(
            bytes[record_start..record_start + 4]
                .try_into()
                .expect("record length is four bytes"),
        );
        if length_i32 < 1 {
            return Err(scanner_error(
                dim,
                path,
                "length",
                &format!("{artifact} chunk ({cx},{cz}) declares length {length_i32}"),
            ));
        }
        let compression = bytes[record_start + 4];
        if compression != 3 {
            return Err(scanner_error(
                dim,
                path,
                "compression-byte",
                &format!(
                    "{artifact} chunk ({cx},{cz}) declares codec {compression}, expected none (3)"
                ),
            ));
        }
        let length = usize::try_from(length_i32)
            .map_err(|_| scanner_error(dim, path, "length", "record length does not fit usize"))?;
        let total_record = length
            .checked_add(4)
            .ok_or_else(|| scanner_error(dim, path, "length", "record length overflows"))?;
        if total_record > record_end - record_start || length < 1 {
            return Err(scanner_error(
                dim,
                path,
                "length",
                &format!("{artifact} chunk ({cx},{cz}) length exceeds allocated sectors"),
            ));
        }
        let payload_bytes = length - 1;
        let payload_start = record_start + 5;
        let payload_end = payload_start + payload_bytes;
        let tag = parse_payload(&bytes[payload_start..payload_end]).map_err(|e| {
            scanner_error(
                dim,
                path,
                "truncation",
                &format!("{artifact} chunk ({cx},{cz}) payload parse failed: {e}"),
            )
        })?;
        let actual = get_chunk_coordinate(&tag);
        if actual != ChunkPos::new(cx, cz) {
            return Err(scanner_error(
                dim,
                path,
                "coordinates",
                &format!(
                    "{artifact} slot ({cx},{cz}) contains payload coordinate ({},{})",
                    actual.x(),
                    actual.z()
                ),
            ));
        }
        records.insert(
            (cx, cz),
            RecordMetadata {
                region_file: format!("{dim}/{name}"),
                slot,
                sector_offset,
                sector_count,
                length: length_i32 as u32,
                compression,
                payload_bytes,
            },
        );
    }
    for ((cx, cz), artifact) in expected {
        if !records.contains_key(&(*cx, *cz)) {
            return Err(scanner_error(
                dim,
                path,
                "location-header",
                &format!("missing expected record {artifact} at chunk ({cx},{cz})"),
            ));
        }
    }
    Ok(RegionScan { records })
}

fn run_corruption_negatives(
    output: &Path,
    regions_root: &Path,
    fixtures: &[Fixture],
) -> Result<Vec<NegativeEvidence>, Error> {
    let negative_root = output.join("negatives");
    fs::create_dir_all(&negative_root)?;
    let first = fixtures
        .iter()
        .find(|f| f.dim == "overworld" && f.cx == 0 && f.cz == 0)
        .ok_or_else(|| Error::Unverified("negative controls need overworld/0.0/0.0.nbt".into()))?;
    let mut results = Vec::new();
    for kind in NegativeKind::all() {
        let root = negative_root.join(kind.name());
        copy_tree(regions_root, &root.join("regions"))?;
        let artifact = root.join("regions/overworld/r.0.0.mca");
        mutate_region(&artifact, kind)?;
        let result = scan_region_tree(&root.join("regions"), fixtures);
        let scanner_error = match result {
            Ok(_) => {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a corruption negative `{}` was not detected in {}",
                    kind.name(),
                    artifact.display()
                )));
            }
            Err(Error::Gate(message)) => message,
            Err(other) => {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a corruption negative `{}` produced non-diagnostic result for {}: {other}",
                    kind.name(),
                    artifact.display()
                )));
            }
        };
        if !scanner_error.contains(kind.name()) || !scanner_error.contains("r.0.0.mca") {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a corruption negative `{}` did not name its intended artifact {}: {scanner_error}",
                kind.name(),
                artifact.display()
            )));
        }
        let storage_error = storage_rejects_corruption(&root.join("regions"), first, kind)?;
        results.push(NegativeEvidence {
            name: kind.name().to_string(),
            artifact: format!("overworld/r.0.0.mca chunk ({},{})", first.cx, first.cz),
            detected: format!("strict scanner: {scanner_error}; storage: {storage_error}"),
        });
    }
    Ok(results)
}

fn storage_rejects_corruption(
    regions_root: &Path,
    fixture: &Fixture,
    kind: NegativeKind,
) -> Result<String, Error> {
    let mut storage = RegionFileStorage::new_read_only(
        info_for_dimension("overworld"),
        regions_root.join("overworld"),
    );
    match storage.read(&ChunkPos::new(fixture.cx, fixture.cz)) {
        Err(error) => Ok(format!(
            "{} RegionFileStorage rejected overworld/r.0.0.mca chunk ({},{}): {error}",
            kind.name(),
            fixture.cx,
            fixture.cz
        )),
        Ok(value) => Err(Error::Gate(format!(
            "anvil-roundtrip-v1a {} corruption reached read-only storage as {value:?} for overworld/r.0.0.mca chunk ({},{})",
            kind.name(),
            fixture.cx,
            fixture.cz
        ))),
    }
}

fn mutate_region(path: &Path, kind: NegativeKind) -> Result<(), Error> {
    let mut bytes = fs::read(path)?;
    let location = u32::from_be_bytes(bytes[0..4].try_into().expect("location"));
    let sector_offset = (location >> 8) as usize;
    let sector_start = sector_offset * 4096;
    match kind {
        NegativeKind::Length => {
            bytes[sector_start..sector_start + 4].copy_from_slice(&i32::MAX.to_be_bytes())
        }
        NegativeKind::Compression => bytes[sector_start + 4] = 2,
        NegativeKind::Location => bytes[0..4].copy_from_slice(&((1u32 << 8) | 1).to_be_bytes()),
        NegativeKind::Overlap => bytes[4..8].copy_from_slice(&location.to_be_bytes()),
        NegativeKind::Truncation => {
            let new_len = (sector_start + 5).min(bytes.len());
            bytes.truncate(new_len);
        }
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn payload_hash(bytes: &[u8], tag: &CompoundTag) -> Result<PayloadHash, Error> {
    let canonical = crate::semantic_hash::canonical_xxh3_64(tag)
        .map_err(|e| Error::Gate(format!("semantic hash failed: {e}")))?;
    Ok(PayloadHash {
        bytes: bytes.len(),
        sha256: sha256_hex(bytes),
        xxh3_64: crate::hash::xxh3_64_hex(bytes),
        xxh3_64_canonical: canonical,
    })
}

fn hash_tree(root: &Path) -> Result<TreeHash, Error> {
    let mut files = Vec::new();
    for path in collect_files(root)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|e| Error::Gate(format!("cannot relativize saved tree: {e}")))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let bytes = fs::read(&path)?;
        files.push(TreeEntry {
            path: relative,
            bytes: bytes.len(),
            sha256: sha256_hex(&bytes),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut material = String::new();
    for file in &files {
        material.push_str(&file.path);
        material.push('\0');
        material.push_str(&file.sha256);
        material.push('\n');
    }
    Ok(TreeHash {
        digest: sha256_hex(material.as_bytes()),
        files,
    })
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    if !root.is_dir() {
        return Err(Error::Gate(format!(
            "artifact directory is missing: {}",
            root.display()
        )));
    }
    let mut files = Vec::new();
    collect_files_inner(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_inner(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), Error> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(Error::Gate(format!(
                "aliased artifact is not allowed: {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_files_inner(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        } else {
            return Err(Error::Gate(format!(
                "unsupported artifact entry: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FileIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(not(unix))]
    len: u64,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
}

fn file_identity(path: &Path) -> Result<FileIdentity, Error> {
    let metadata = fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(FileIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        Ok(FileIdentity {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), Error> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(Error::Gate(format!(
                "cannot copy aliased artifact {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn prepare_output(output: &Path) -> Result<(), Error> {
    if output.exists() {
        fs::remove_dir_all(output)?;
    }
    fs::create_dir_all(output)?;
    Ok(())
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, std::path::Component::ParentDir))
}

fn parse_coordinate(value: &str) -> Result<(i32, i32), String> {
    let (x, z) = value
        .split_once('.')
        .ok_or_else(|| "expected <chunkX>.<chunkZ>".to_string())?;
    Ok((
        x.parse().map_err(|e| format!("bad chunk x: {e}"))?,
        z.parse().map_err(|e| format!("bad chunk z: {e}"))?,
    ))
}

fn parse_region_filename(value: &str) -> Option<(i32, i32)> {
    let stem = value.strip_suffix(".mca")?;
    let parts: Vec<&str> = stem.split('.').collect();
    if parts.len() != 3 || parts[0] != "r" {
        return None;
    }
    Some((parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn fixture_key(fixture: &Fixture) -> String {
    format!(
        "{}/{}/{}.{}",
        fixture.dim, fixture.region, fixture.cx, fixture.cz
    )
}

fn info_for_dimension(dim: &str) -> RegionStorageInfo {
    let dimension = match dim {
        "overworld" => overworld(),
        "the_nether" => nether(),
        "the_end" => end(),
        other => panic!("unsupported committed fixture dimension {other}"),
    };
    RegionStorageInfo::new(
        "anvil-roundtrip-v1a".to_string(),
        dimension,
        "region".to_string(),
        true,
    )
}

fn scanner_error(dim: &str, path: &Path, kind: &str, detail: &str) -> Error {
    Error::Gate(format!(
        "anvil-roundtrip-v1a {kind} corruption in {dim}/{}: {detail}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<region>"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_parser_accepts_negative_values() {
        assert_eq!(parse_coordinate("-31.0").unwrap(), (-31, 0));
        assert_eq!(parse_region_filename("r.-1.2.mca"), Some((-1, 2)));
    }

    #[test]
    fn negative_names_are_load_bearing() {
        let names: Vec<_> = NegativeKind::all()
            .into_iter()
            .map(NegativeKind::name)
            .collect();
        assert_eq!(
            names,
            [
                "length",
                "compression-byte",
                "location-header",
                "sector-overlap",
                "truncation"
            ]
        );
    }
}
