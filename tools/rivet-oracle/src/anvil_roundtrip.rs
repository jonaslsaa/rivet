//! Storage-only #231 V1a Anvil round-trip oracle.
//!
//! This command deliberately stops at the region container boundary: it owns
//! the committed CompoundTag payloads, writes them through a fresh
//! `RegionFileStorage` with compression pinned to `none`, and reads them back
//! through a newly-created read-only storage. It does not parse or reconstruct
//! runtime chunks and it does not claim V1b/FULL/generated-world evidence.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_registry::core::ChunkPos;
use rivet_util::DataInputStream;
use rivet_world::chunk::storage::{
    RegionFile, RegionFileStorage, RegionFileVersion, RegionStorageInfo, get_chunk_coordinate,
};
use rivet_world::level::{end, nether, overworld};
use serde::Serialize;

use crate::mutate::encode_payload;
use crate::{Error, sha256_hex};

const EXPECTED_CHUNK_COUNT: usize = 432;
const EXPECTED_M0_CAPTURE_COUNT: usize = 435;
const REGION_FILE_COMPRESSION: &str = "none";
const ROUNDTRIP_KIND: &str = "anvil-roundtrip-v1a";
const EXPECTED_M0_SEED: &str = "42";
const EXPECTED_M0_PAPER: &str = "26.2-DEV-main@0a99345";
const EXPECTED_M0_PAPER_VERSION: &str = "26.2";
const EXPECTED_M0_PAPER_COMMIT: &str = "0a993450f129c4942c2a9ed45ba047412b4667cf";
const EXPECTED_M0_MANIFEST_SHA256: &str =
    "0a3d588439ab34ce1d15cf2d6d783c2f544d1af48cb82fedff6717da618e1e9d";
/// SHA-256 of sorted canonical relative chunk paths and their raw payload bytes.
const EXPECTED_M0_CORPUS_RAW_SHA256: &str =
    "d36da5bef3cddc5845f7ebe745d5eedae91119020515960b8be8b6e6d9a24089";
const EXPECTED_DIMENSIONS: [&str; 3] = ["overworld", "the_nether", "the_end"];
const EXPECTED_AXIS_LEN: i32 = 12;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PayloadHash {
    bytes: usize,
    sha256: String,
    xxh3_64: String,
    xxh3_64_canonical: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RecordMetadata {
    region_file: String,
    slot: usize,
    sector_offset: u32,
    sector_count: u8,
    length: u32,
    compression: u8,
    payload_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ChunkEvidence {
    dim: String,
    region: String,
    cx: i32,
    cz: i32,
    fixture_path: String,
    source_path: String,
    saved_path: String,
    reloaded_path: String,
    source: PayloadHash,
    saved: PayloadHash,
    reloaded: PayloadHash,
    record: RecordMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TreeEntry {
    path: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct NegativeEvidence {
    mutation: String,
    artifact: String,
    slot: usize,
    chunk: String,
    rejection_stage: String,
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
    source_paper: String,
    source_paper_version: String,
    source_paper_commit: String,
    source_corpus_sha256: String,
    region_file_compression: String,
    expected_chunk_count: usize,
    source_chunk_count: usize,
    source_tree_hash_before_roundtrip: String,
    source_tree_hash_after_roundtrip: String,
    source_tree_file_count: usize,
    region_tree_hash_before_read_only_reload: String,
    region_tree_hash_after_read_only_reload: String,
    region_tree_file_count: usize,
    region_tree_files: Vec<TreeEntry>,
    chunks: Vec<ChunkEvidence>,
    corruption_negatives: Vec<NegativeEvidence>,
    non_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PayloadManifest {
    format: u32,
    kind: String,
    chunks: Vec<ChunkEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeHash {
    digest: String,
    files: Vec<TreeEntry>,
    directories: Vec<String>,
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
    TrailingPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectionStage {
    OpenHeader,
    PayloadRead,
}

impl RejectionStage {
    fn name(self) -> &'static str {
        match self {
            Self::OpenHeader => "open/header",
            Self::PayloadRead => "payload-read",
        }
    }
}

impl NegativeKind {
    fn name(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Compression => "compression-byte",
            Self::Location => "location-header",
            Self::Overlap => "sector-overlap",
            Self::Truncation => "truncation",
            Self::TrailingPayload => "trailing-payload",
        }
    }

    fn target_slot(self) -> usize {
        match self {
            Self::Overlap => 1,
            _ => 0,
        }
    }

    fn rejection_stage(self) -> RejectionStage {
        match self {
            Self::Location | Self::Overlap => RejectionStage::OpenHeader,
            Self::Length | Self::Compression | Self::Truncation | Self::TrailingPayload => {
                RejectionStage::PayloadRead
            }
        }
    }

    fn all() -> [Self; 6] {
        [
            Self::Length,
            Self::Compression,
            Self::Location,
            Self::Overlap,
            Self::Truncation,
            Self::TrailingPayload,
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
    if matches!(args, ["--help"] | ["-h"]) {
        println!(
            "usage: cargo run -p rivet-oracle -- anvil-roundtrip-v1a [fixtures] [--out <dir>]"
        );
        println!("  exits 0=PASS, 1=FAIL, 3=UNVERIFIED (missing external source evidence)");
        return Ok(());
    }
    let parsed = parse_cli(args)?;
    run_roundtrip(&parsed.fixture_root, &parsed.output)
}

#[derive(Debug, PartialEq, Eq)]
struct CliArgs {
    fixture_root: PathBuf,
    output: PathBuf,
}

fn parse_cli(args: &[&str]) -> Result<CliArgs, Error> {
    let default_fixture_root = crate::crate_dir().join("fixtures");
    let default_output = crate::crate_dir().join("work/anvil-roundtrip-v1a");
    let mut fixture_root = None;
    let mut output = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--out" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::Gate(
                        "anvil-roundtrip-v1a --out requires a destination directory".into(),
                    ));
                };
                if path.starts_with('-') {
                    return Err(Error::Gate(
                        "anvil-roundtrip-v1a --out requires a destination directory, not an option"
                            .into(),
                    ));
                }
                output = Some(PathBuf::from(path));
                i += 2;
            }
            value if value.starts_with('-') => {
                return Err(Error::Gate(format!(
                    "unknown anvil-roundtrip-v1a option `{value}`"
                )));
            }
            value => {
                if fixture_root.is_some() {
                    return Err(Error::Gate(
                        "anvil-roundtrip-v1a accepts only one fixture directory".into(),
                    ));
                }
                fixture_root = Some(PathBuf::from(value));
                i += 1;
            }
        }
    }
    Ok(CliArgs {
        fixture_root: fixture_root.unwrap_or(default_fixture_root),
        output: output.unwrap_or(default_output),
    })
}

fn run_roundtrip(fixture_root: &Path, output: &Path) -> Result<(), Error> {
    let (fixture_root, output) = canonicalize_paths(fixture_root, output)?;
    let source_before = hash_tree(&fixture_root)?;
    let result = (|| {
        let fixtures = load_fixtures(&fixture_root)?;
        if fixtures.len() != EXPECTED_CHUNK_COUNT {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a requires the committed {EXPECTED_CHUNK_COUNT}-chunk M0 corpus, found {}",
                fixtures.len()
            )));
        }
        run_roundtrip_inner(&fixture_root, &output, &fixtures, &source_before)
    })();
    finish_source_validation(&fixture_root, &source_before, result)
}

fn finish_source_validation(
    fixture_root: &Path,
    source_before: &TreeHash,
    result: Result<(), Error>,
) -> Result<(), Error> {
    let source_after = hash_tree(fixture_root).map_err(|error| {
        Error::Gate(format!(
            "anvil-roundtrip-v1a could not re-hash the source tree after execution: {error}"
        ))
    })?;
    if source_before != &source_after {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a mutated the source fixture tree: before {}, after {}",
            source_before.digest, source_after.digest
        )));
    }
    result
}

fn run_roundtrip_inner(
    fixture_root: &Path,
    output: &Path,
    fixtures: &[Fixture],
    source_before: &TreeHash,
) -> Result<(), Error> {
    prepare_output(output)?;
    let regions_root = output.join("regions");
    let evidence_root = output.join("evidence");
    fs::create_dir_all(&regions_root)?;
    fs::create_dir_all(&evidence_root)?;
    let mut saved_hashes = HashMap::new();

    write_fixture_regions(fixtures, &regions_root, &evidence_root, &mut saved_hashes)?;
    let before_reload = hash_tree(&regions_root)?;
    let records = scan_region_tree(&regions_root, fixtures)?;

    let reloaded_hashes = read_fresh_read_only(
        fixtures,
        &regions_root,
        &evidence_root,
        &saved_hashes,
        &records,
    )?;
    let after_reload = hash_tree(&regions_root)?;
    if before_reload.digest != after_reload.digest {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a read-only reload mutated the saved region tree: before {}, after {}",
            before_reload.digest, after_reload.digest
        )));
    }

    let mut chunks = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
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
            source_path: stage_payload_path("source", fixture),
            saved_path: stage_payload_path("saved", fixture),
            reloaded_path: stage_payload_path("reloaded", fixture),
            source,
            saved: saved.clone(),
            reloaded: reloaded.clone(),
            record: record.clone(),
        });
    }

    let negatives = run_corruption_negatives(output, &regions_root, fixtures)?;
    let manifest_bytes = fs::read(fixture_root.join("manifest.json"))?;
    let manifest = crate::load_manifest(fixture_root)?;
    let source_seed = manifest
        .seed
        .clone()
        .ok_or_else(|| Error::Gate("M0 fixture manifest is missing seed provenance".into()))?;
    let source_kind = manifest.kind.clone().ok_or_else(|| {
        Error::Gate("M0 fixture manifest is missing capture-kind provenance".into())
    })?;
    let source_paper = manifest
        .paper
        .clone()
        .ok_or_else(|| Error::Gate("M0 fixture manifest is missing Paper provenance".into()))?;
    let source_paper_commit = crate::parse_paper_pin(Some(&source_paper))
        .ok_or_else(|| Error::Gate("M0 fixture manifest Paper provenance has no commit".into()))?;
    let source_paper_version = source_paper
        .split_once('-')
        .map(|(version, _)| version.to_string())
        .ok_or_else(|| Error::Gate("M0 fixture manifest Paper provenance has no version".into()))?;
    let source_after = hash_tree(fixture_root)?;
    if source_before != &source_after {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a mutated the source fixture tree before report emission: before {}, after {}",
            source_before.digest, source_after.digest
        )));
    }

    let payload_manifest = PayloadManifest {
        format: 1,
        kind: ROUNDTRIP_KIND.to_string(),
        chunks: chunks.clone(),
    };
    let payload_manifest_bytes = serde_json::to_vec_pretty(&payload_manifest)
        .map_err(|e| Error::Gate(format!("cannot serialize payload evidence manifest: {e}")))?;
    fs::write(evidence_root.join("manifest.json"), payload_manifest_bytes)?;

    let report = Report {
        format: 1,
        kind: ROUNDTRIP_KIND.to_string(),
        verdict: "PASS".to_string(),
        source_fixture_root: fixture_root.display().to_string(),
        source_manifest_sha256: sha256_hex(&manifest_bytes),
        source_manifest_kind: source_kind,
        source_seed,
        source_paper,
        source_paper_version,
        source_paper_commit,
        source_corpus_sha256: EXPECTED_M0_CORPUS_RAW_SHA256.to_string(),
        region_file_compression: REGION_FILE_COMPRESSION.to_string(),
        expected_chunk_count: EXPECTED_CHUNK_COUNT,
        source_chunk_count: fixtures.len(),
        source_tree_hash_before_roundtrip: source_before.digest.clone(),
        source_tree_hash_after_roundtrip: source_after.digest,
        source_tree_file_count: source_after.files.len(),
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
    let root_metadata = fs::symlink_metadata(fixture_root).map_err(|error| {
        Error::Unverified(format!(
            "anvil-roundtrip-v1a source fixture prerequisite unavailable at {}: {error}",
            fixture_root.display()
        ))
    })?;
    if !root_metadata.file_type().is_dir() {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a fixture root is not a regular directory: {}",
            fixture_root.display()
        )));
    }

    let manifest_path = fixture_root.join("manifest.json");
    let manifest_metadata = fs::symlink_metadata(&manifest_path).map_err(|error| {
        Error::Unverified(format!(
            "anvil-roundtrip-v1a source provenance prerequisite unavailable at {}: {error}",
            manifest_path.display()
        ))
    })?;
    if !manifest_metadata.file_type().is_file() {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a manifest is not a regular file: {}",
            manifest_path.display()
        )));
    }
    let manifest_bytes = fs::read(&manifest_path)?;
    let declared_manifest = crate::load_manifest(fixture_root)?;
    validate_m0_manifest(&declared_manifest)?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    if manifest_sha256 != EXPECTED_M0_MANIFEST_SHA256 {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a committed M0 manifest identity mismatch: expected {}, found {}",
            EXPECTED_M0_MANIFEST_SHA256, manifest_sha256
        )));
    }
    for captured in &declared_manifest.captured {
        if !is_safe_relative_path(&captured.path) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest artifact path escapes fixture root: {}",
                captured.path
            )));
        }
        ensure_no_symlink_components(fixture_root, &captured.path)?;
        let path = fixture_root.join(&captured.path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Error::Unverified(format!(
                "anvil-roundtrip-v1a source capture prerequisite unavailable for {}: {error}",
                captured.path
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest artifact is not a regular file: {}",
                captured.path
            )));
        }
    }

    let manifest = crate::verify_fixtures(fixture_root)?;
    let chunk_root = fixture_root.join("chunk");
    let chunk_root_metadata = fs::symlink_metadata(&chunk_root).map_err(|error| {
        Error::Unverified(format!(
            "anvil-roundtrip-v1a source chunk prerequisite unavailable at {}: {error}",
            chunk_root.display()
        ))
    })?;
    if !chunk_root_metadata.file_type().is_dir() {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a chunk root is not a regular directory: {}",
            chunk_root.display()
        )));
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
        if !EXPECTED_DIMENSIONS.contains(&dim.as_str()) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a unsupported fixture dimension in tuple path: {}",
                captured.path
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
        if captured.dim.as_deref() != Some(dim.as_str()) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest dimension mismatch for {}: path {}, manifest {:?}",
                captured.path, dim, captured.dim
            )));
        }
        if captured.chunk.as_deref() != Some(stem) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest chunk mismatch for {}: path {}, manifest {:?}",
                captured.path, stem, captured.chunk
            )));
        }
        if captured.region.as_deref() != Some(region.as_str()) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a manifest region mismatch for {}: path {}, manifest {:?}",
                captured.path, region, captured.region
            )));
        }
        if !(0..EXPECTED_AXIS_LEN).contains(&cx) || !(0..EXPECTED_AXIS_LEN).contains(&cz) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a fixture coordinate outside exact 12x12 corpus: {} ({cx},{cz})",
                captured.path
            )));
        }
        if !expected_tuples.insert((dim.clone(), region.clone(), cx, cz)) {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a duplicate fixture tuple {dim}/{region}/{cx}.{cz}"
            )));
        }
        let path = fixture_root.join(&captured.path);
        let source_bytes = fs::read(&path)?;
        let source_tag = parse_payload_exact(&source_bytes)
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
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a fixture manifest contains {} chunk entries, expected {EXPECTED_CHUNK_COUNT}",
            fixtures.len()
        )));
    }
    let expected_per_dimension = (EXPECTED_AXIS_LEN * EXPECTED_AXIS_LEN) as usize;
    for dim in EXPECTED_DIMENSIONS {
        let count = fixtures.iter().filter(|fixture| fixture.dim == dim).count();
        if count != expected_per_dimension {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a dimension {dim} contains {count} chunks, expected {expected_per_dimension}"
            )));
        }
        for cx in 0..EXPECTED_AXIS_LEN {
            for cz in 0..EXPECTED_AXIS_LEN {
                if !expected_tuples.contains(&(dim.to_string(), "0.0".to_string(), cx, cz)) {
                    return Err(Error::Gate(format!(
                        "anvil-roundtrip-v1a exact corpus is missing {dim}/0.0/{cx}.{cz}.nbt"
                    )));
                }
            }
        }
    }
    verify_fixture_tree_closure(fixture_root, &expected_paths, &fixtures)?;
    let corpus_sha256 = crate::raw_corpus_identity(fixture_root, &manifest)?;
    if corpus_sha256 != EXPECTED_M0_CORPUS_RAW_SHA256 {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a committed M0 corpus identity mismatch: expected {}, found {}",
            EXPECTED_M0_CORPUS_RAW_SHA256, corpus_sha256
        )));
    }
    fixtures.sort_by_key(fixture_key);
    Ok(fixtures)
}

fn validate_m0_manifest(manifest: &crate::Manifest) -> Result<(), Error> {
    let chunk_entries = manifest
        .captured
        .iter()
        .filter(|c| c.path.starts_with("chunk/"))
        .count();
    if matches!(manifest.kind.as_deref(), None | Some(""))
        || matches!(manifest.seed.as_deref(), None | Some(""))
        || matches!(manifest.paper.as_deref(), None | Some(""))
    {
        return Err(Error::Gate(
            "anvil-roundtrip-v1a manifest is missing M0 provenance (kind, seed, or Paper)".into(),
        ));
    }
    if manifest.kind.as_deref() != Some(crate::KIND_M0) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires manifest kind m0, found {:?}",
            manifest.kind
        )));
    }
    if manifest.seed.as_deref() != Some(EXPECTED_M0_SEED) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires manifest seed {}, found {:?}",
            EXPECTED_M0_SEED, manifest.seed
        )));
    }
    if manifest.paper.as_deref() != Some(EXPECTED_M0_PAPER) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires Paper provenance {}, found {:?}",
            EXPECTED_M0_PAPER, manifest.paper
        )));
    }
    let commit = crate::parse_paper_pin(manifest.paper.as_deref()).ok_or_else(|| {
        Error::Gate("anvil-roundtrip-v1a manifest Paper provenance has no commit".into())
    })?;
    if !EXPECTED_M0_PAPER_COMMIT.starts_with(&commit) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires Paper commit {}, found {}",
            EXPECTED_M0_PAPER_COMMIT, commit
        )));
    }
    let version = manifest
        .paper
        .as_deref()
        .and_then(|paper| paper.split_once('-').map(|(v, _)| v));
    if version != Some(EXPECTED_M0_PAPER_VERSION) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires Paper version {}, found {:?}",
            EXPECTED_M0_PAPER_VERSION, version
        )));
    }
    if manifest.chunk_count != Some(EXPECTED_CHUNK_COUNT as u64) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires manifest chunk-count {}, found {:?}",
            EXPECTED_CHUNK_COUNT, manifest.chunk_count
        )));
    }
    if chunk_entries != EXPECTED_CHUNK_COUNT {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires exactly {EXPECTED_CHUNK_COUNT} manifest chunk entries, found {chunk_entries}"
        )));
    }
    if manifest.captured.len() != EXPECTED_M0_CAPTURE_COUNT {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires exactly {EXPECTED_M0_CAPTURE_COUNT} manifest entries, found {}",
            manifest.captured.len()
        )));
    }
    if manifest.level_type.as_deref() != Some("minecraft\\:flat") {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires manifest level-type minecraft\\:flat, found {:?}",
            manifest.level_type
        )));
    }
    if manifest.region_file_compression.as_deref() != Some(REGION_FILE_COMPRESSION) {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a requires manifest region-file-compression none, found {:?}",
            manifest.region_file_compression
        )));
    }
    Ok(())
}

fn ensure_no_symlink_components(root: &Path, relative: &str) -> Result<(), Error> {
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::Unverified(format!(
                    "anvil-roundtrip-v1a source capture prerequisite unavailable at {}: {error}",
                    current.display()
                ))
            } else {
                Error::Gate(format!(
                    "anvil-roundtrip-v1a cannot inspect manifest path component {}: {error}",
                    current.display()
                ))
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a symlink path component is not allowed: {}",
                current.display()
            )));
        }
    }
    Ok(())
}

fn parse_payload_exact(bytes: &[u8]) -> Result<CompoundTag, String> {
    let mut input = DataInputStream::new(Cursor::new(bytes));
    let tag =
        nbt_io::read_unlimited(&mut input).map_err(|error| format!("NBT read failed: {error}"))?;
    let cursor = input.into_inner();
    if cursor.position() != bytes.len() as u64 {
        return Err(format!(
            "NBT payload has {} trailing bytes",
            bytes.len() as u64 - cursor.position()
        ));
    }
    Ok(tag)
}

fn verify_fixture_tree_closure(
    fixture_root: &Path,
    expected_paths: &HashSet<String>,
    fixtures: &[Fixture],
) -> Result<(), Error> {
    let chunk_root = fixture_root.join("chunk");
    let metadata = fs::symlink_metadata(&chunk_root).map_err(|error| {
        Error::Unverified(format!(
            "anvil-roundtrip-v1a source chunk prerequisite unavailable at {}: {error}",
            chunk_root.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a chunk root is not a regular directory: {}",
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
    evidence_root: &Path,
    saved_hashes: &mut HashMap<String, PayloadHash>,
) -> Result<(), Error> {
    for fixture in fixtures {
        persist_stage_payload(evidence_root, "source", fixture, &fixture.source_bytes)?;
    }
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
            persist_stage_payload(evidence_root, "saved", fixture, &bytes)?;
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
    evidence_root: &Path,
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
            persist_stage_payload(evidence_root, "reloaded", fixture, &bytes)?;
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
        let ((cx, cz), artifact) = expected.iter().next().ok_or_else(|| {
            scanner_error(
                dim,
                path,
                "truncation",
                "no expected chunk identifies the mutation",
            )
        })?;
        return Err(scanner_error(
            dim,
            path,
            "truncation",
            &format!(
                "{artifact} slot {} chunk ({cx},{cz}) header/file length is not a complete sector layout",
                chunk_slot(region_x, region_z, *cx, *cz)
                    .map_err(|detail| { scanner_error(dim, path, "truncation", &detail) })?
            ),
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
        let (cx, cz) = checked_chunk_coordinate(region_x, region_z, slot).map_err(|detail| {
            scanner_error(
                dim,
                path,
                "location-header",
                &format!("slot {slot}: {detail}"),
            )
        })?;
        let artifact = expected.get(&(cx, cz)).ok_or_else(|| {
            scanner_error(
                dim,
                path,
                "location-header",
                &format!("extra allocated record at slot {slot} chunk ({cx},{cz})"),
            )
        })?;
        let sector_offset = packed >> 8;
        let sector_count = (packed & 0xff) as u8;
        if sector_offset < 2 || sector_count == 0 {
            return Err(scanner_error(
                dim,
                path,
                "location-header",
                &format!(
                    "{artifact} slot {slot} chunk ({cx},{cz}) has invalid location {packed:#x}"
                ),
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
                &format!("{artifact} slot {slot} chunk ({cx},{cz}) extends beyond file bounds"),
            ));
        }
        for (sector, occupied) in used.iter_mut().enumerate().take(end).skip(start) {
            if *occupied {
                return Err(scanner_error(
                    dim,
                    path,
                    "sector-overlap",
                    &format!("{artifact} slot {slot} chunk ({cx},{cz}) overlaps sector {sector}"),
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
                &format!("{artifact} slot {slot} chunk ({cx},{cz}) has no five-byte record header"),
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
                &format!("{artifact} slot {slot} chunk ({cx},{cz}) declares length {length_i32}"),
            ));
        }
        let compression = bytes[record_start + 4];
        if compression != 3 {
            return Err(scanner_error(
                dim,
                path,
                "compression-byte",
                &format!(
                    "{artifact} slot {slot} chunk ({cx},{cz}) declares codec {compression}, expected none (3)"
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
                &format!(
                    "{artifact} slot {slot} chunk ({cx},{cz}) length exceeds allocated sectors"
                ),
            ));
        }
        let payload_bytes = length - 1;
        let payload_start = record_start + 5;
        let payload_end = payload_start + payload_bytes;
        let tag = parse_payload_exact(&bytes[payload_start..payload_end]).map_err(|e| {
            let kind = if e.contains("trailing bytes") {
                "trailing-payload"
            } else {
                "truncation"
            };
            scanner_error(
                dim,
                path,
                kind,
                &format!("{artifact} slot {slot} chunk ({cx},{cz}) payload parse failed: {e}"),
            )
        })?;
        let actual = get_chunk_coordinate(&tag);
        if actual != ChunkPos::new(cx, cz) {
            return Err(scanner_error(
                dim,
                path,
                "coordinates",
                &format!(
                    "{artifact} slot {slot} chunk ({cx},{cz}) contains payload coordinate ({},{})",
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

#[derive(Debug)]
struct StorageRejection {
    stage: RejectionStage,
    message: String,
}

fn run_corruption_negatives(
    output: &Path,
    regions_root: &Path,
    fixtures: &[Fixture],
) -> Result<Vec<NegativeEvidence>, Error> {
    let negative_root = output.join("negatives");
    fs::create_dir_all(&negative_root)?;
    if !fixtures
        .iter()
        .any(|f| f.dim == "overworld" && f.cx == 0 && f.cz == 0)
    {
        return Err(Error::Unverified("anvil-roundtrip-v1a negative-control evidence unavailable: missing overworld/0.0/0.0.nbt".into()));
    }
    let mut results = Vec::new();
    for kind in NegativeKind::all() {
        let target_slot = kind.target_slot();
        let target = fixtures
            .iter()
            .find(|fixture| {
                fixture.dim == "overworld"
                    && fixture.cx == (target_slot as i32 & 31)
                    && fixture.cz == (target_slot as i32 >> 5)
            })
            .ok_or_else(|| {
                Error::Gate(format!(
                    "anvil-roundtrip-v1a negative {} target slot {} is absent",
                    kind.name(),
                    target_slot
                ))
            })?;
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
        let target_chunk = format!("chunk ({},{})", target.cx, target.cz);
        if !scanner_error.contains(kind.name())
            || !scanner_error.contains("r.0.0.mca")
            || !scanner_error.contains(&target_chunk)
            || !scanner_error.contains(&format!("slot {target_slot}"))
        {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a corruption negative `{}` did not attribute intended artifact {}, slot {}, and {}: {scanner_error}",
                kind.name(),
                artifact.display(),
                target_slot,
                target_chunk
            )));
        }
        let storage = storage_rejects_corruption(&root.join("regions"), target, kind)?;
        if storage.stage != kind.rejection_stage() {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a corruption negative `{}` was rejected at {} instead of {} for slot {} {}: {}",
                kind.name(),
                storage.stage.name(),
                kind.rejection_stage().name(),
                target_slot,
                target_chunk,
                storage.message
            )));
        }
        results.push(NegativeEvidence {
            mutation: kind.name().to_string(),
            artifact: "overworld/r.0.0.mca".to_string(),
            slot: target_slot,
            chunk: format!("{}.{}", target.cx, target.cz),
            rejection_stage: storage.stage.name().to_string(),
            detected: format!(
                "strict scanner: {scanner_error}; storage stage {}: {}",
                storage.stage.name(),
                storage.message
            ),
        });
    }
    Ok(results)
}

fn storage_rejects_corruption(
    regions_root: &Path,
    fixture: &Fixture,
    kind: NegativeKind,
) -> Result<StorageRejection, Error> {
    let source_region_dir = regions_root.join("overworld");
    let source_artifact = source_region_dir.join("r.0.0.mca");
    let artifact_label = "overworld/r.0.0.mca";
    let slot = chunk_slot(0, 0, fixture.cx, fixture.cz)
        .map_err(|detail| Error::Gate(format!("anvil-roundtrip-v1a {detail}")))?;

    let storage_root = tempfile::tempdir()?;
    let region_dir = storage_root.path().join("overworld");
    fs::create_dir_all(&region_dir)?;
    let artifact = region_dir.join("r.0.0.mca");
    let mut bytes = fs::read(&source_artifact)?;
    isolate_storage_slots(&mut bytes, kind)?;
    fs::write(&artifact, bytes)?;

    let open_result = RegionFile::open_read_only(
        info_for_dimension("overworld"),
        artifact,
        region_dir.clone(),
        RegionFileVersion::VERSION_NONE,
    );
    let (stage, error) = match open_result {
        Err(error) => (RejectionStage::OpenHeader, error.to_string()),
        Ok(region) => {
            drop(region);
            let mut storage =
                RegionFileStorage::new_read_only(info_for_dimension("overworld"), region_dir);
            match storage.read(&ChunkPos::new(fixture.cx, fixture.cz)) {
                Err(error) => (RejectionStage::PayloadRead, error.to_string()),
                Ok(value) => {
                    return Err(Error::Gate(format!(
                        "anvil-roundtrip-v1a {} corruption reached read-only storage as {value:?} for {} slot {slot} chunk ({},{}), unrelated to the intended mutation",
                        kind.name(),
                        artifact_label,
                        fixture.cx,
                        fixture.cz
                    )));
                }
            }
        }
    };
    Ok(StorageRejection {
        stage,
        message: format!(
            "{} RegionFileStorage rejected {} slot {slot} chunk ({},{}): {error}",
            kind.name(),
            artifact_label,
            fixture.cx,
            fixture.cz
        ),
    })
}

fn isolate_storage_slots(bytes: &mut [u8], kind: NegativeKind) -> Result<(), Error> {
    if bytes.len() < 8192 {
        return Err(Error::Gate(
            "anvil-roundtrip-v1a cannot isolate storage corruption from a truncated header".into(),
        ));
    }
    for slot in 0..1024usize {
        let keep = match kind {
            NegativeKind::Overlap => slot <= 1,
            _ => slot == kind.target_slot(),
        };
        if !keep {
            bytes[slot * 4..slot * 4 + 4].fill(0);
        }
    }
    Ok(())
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
        NegativeKind::TrailingPayload => {
            let length = i32::from_be_bytes(
                bytes[sector_start..sector_start + 4]
                    .try_into()
                    .expect("record length"),
            );
            if length < 1 {
                return Err(Error::Gate(
                    "cannot append trailing payload to invalid record".into(),
                ));
            }
            let record_end = sector_start
                .checked_add(((location & 0xff) as usize) * 4096)
                .ok_or_else(|| Error::Gate("trailing payload record bounds overflow".into()))?;
            let append_at = sector_start
                .checked_add(4)
                .and_then(|offset| offset.checked_add(length as usize))
                .ok_or_else(|| Error::Gate("trailing payload offset overflow".into()))?;
            let append_end = append_at
                .checked_add(4)
                .ok_or_else(|| Error::Gate("trailing payload end overflow".into()))?;
            if append_end > record_end || append_end > bytes.len() {
                return Err(Error::Gate(
                    "record has no sector padding for trailing payload".into(),
                ));
            }
            let expanded_length = length
                .checked_add(4)
                .ok_or_else(|| Error::Gate("trailing payload length overflow".into()))?;
            bytes[sector_start..sector_start + 4].copy_from_slice(&expanded_length.to_be_bytes());
            bytes[append_at..append_end].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        }
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn persist_stage_payload(
    evidence_root: &Path,
    stage: &str,
    fixture: &Fixture,
    bytes: &[u8],
) -> Result<(), Error> {
    let path = evidence_root.join(stage).join(&fixture.path);
    let parent = path.parent().ok_or_else(|| {
        Error::Gate(format!(
            "anvil-roundtrip-v1a evidence path has no parent: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent)?;
    fs::write(path, bytes)?;
    Ok(())
}

fn stage_payload_path(stage: &str, fixture: &Fixture) -> String {
    format!("evidence/{stage}/{}", fixture.path)
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
    let mut directories = Vec::new();
    for path in collect_directories(root)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|e| Error::Gate(format!("cannot relativize saved directory: {e}")))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        directories.push(relative);
    }
    directories.sort();
    let mut material = String::new();
    for directory in &directories {
        material.push('D');
        material.push('\0');
        material.push_str(directory);
        material.push('\n');
    }
    for file in &files {
        material.push('F');
        material.push('\0');
        material.push_str(&file.path);
        material.push('\0');
        material.push_str(&file.sha256);
        material.push('\n');
    }
    Ok(TreeHash {
        digest: sha256_hex(material.as_bytes()),
        files,
        directories,
    })
}

fn collect_directories(root: &Path) -> Result<Vec<PathBuf>, Error> {
    if !root.is_dir() {
        return Err(Error::Gate(format!(
            "artifact directory is missing: {}",
            root.display()
        )));
    }
    let mut directories = Vec::new();
    collect_directories_inner(root, &mut directories)?;
    directories.sort();
    Ok(directories)
}

fn collect_directories_inner(root: &Path, directories: &mut Vec<PathBuf>) -> Result<(), Error> {
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
            directories.push(path.clone());
            collect_directories_inner(&path, directories)?;
        } else if !file_type.is_file() {
            return Err(Error::Gate(format!(
                "unsupported artifact entry: {}",
                path.display()
            )));
        }
    }
    Ok(())
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

fn canonicalize_paths(fixture_root: &Path, output: &Path) -> Result<(PathBuf, PathBuf), Error> {
    reject_symlink_components(fixture_root, "fixture root")?;
    reject_symlink_components(output, "output")?;
    let source = fs::canonicalize(fixture_root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Unverified(format!(
                "anvil-roundtrip-v1a missing fixture provenance/artifacts at {}: {error}",
                fixture_root.display()
            ))
        } else {
            Error::Gate(format!(
                "anvil-roundtrip-v1a cannot canonicalize fixture root {}: {error}",
                fixture_root.display()
            ))
        }
    })?;
    let destination = canonical_destination(output)?;
    if destination == source || destination.starts_with(&source) || source.starts_with(&destination)
    {
        return Err(Error::Gate(format!(
            "anvil-roundtrip-v1a source/output paths overlap: source {}, output {}",
            source.display(),
            destination.display()
        )));
    }
    Ok((source, destination))
}

fn reject_symlink_components(path: &Path, role: &str) -> Result<(), Error> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a {role} symlink path component is not allowed: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::Gate(format!(
                    "anvil-roundtrip-v1a cannot inspect {role} path component {}: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn canonical_destination(path: &Path) -> Result<PathBuf, Error> {
    reject_symlink_components(path, "output")?;
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    while match fs::symlink_metadata(&existing) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a cannot inspect output {}: {error}",
                existing.display()
            )));
        }
    } {
        let Some(name) = existing.file_name() else {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a output has no canonicalizable parent: {}",
                path.display()
            )));
        };
        missing.push(name.to_os_string());
        existing = existing
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
    }
    let mut canonical = fs::canonicalize(&existing).map_err(|error| {
        Error::Gate(format!(
            "anvil-roundtrip-v1a cannot canonicalize output {}: {error}",
            path.display()
        ))
    })?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn prepare_output(output: &Path) -> Result<(), Error> {
    reject_symlink_components(output, "output")?;
    match fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::Gate(format!(
                "anvil-roundtrip-v1a output symlink is not allowed: {}",
                output.display()
            )));
        }
        Ok(_) => fs::remove_dir_all(output)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
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

fn checked_chunk_coordinate(
    region_x: i32,
    region_z: i32,
    slot: usize,
) -> Result<(i32, i32), String> {
    let base_x = region_x
        .checked_mul(32)
        .ok_or_else(|| format!("region x {region_x} overflows chunk coordinate arithmetic"))?;
    let base_z = region_z
        .checked_mul(32)
        .ok_or_else(|| format!("region z {region_z} overflows chunk coordinate arithmetic"))?;
    let local_x = (slot & 31) as i32;
    let local_z = (slot >> 5) as i32;
    let cx = base_x
        .checked_add(local_x)
        .ok_or_else(|| format!("chunk x base {base_x} overflows at slot {slot}"))?;
    let cz = base_z
        .checked_add(local_z)
        .ok_or_else(|| format!("chunk z base {base_z} overflows at slot {slot}"))?;
    Ok((cx, cz))
}

fn chunk_slot(region_x: i32, region_z: i32, cx: i32, cz: i32) -> Result<usize, String> {
    let region_base_x = region_x
        .checked_mul(32)
        .ok_or_else(|| format!("region x {region_x} overflows chunk coordinate arithmetic"))?;
    let region_base_z = region_z
        .checked_mul(32)
        .ok_or_else(|| format!("region z {region_z} overflows chunk coordinate arithmetic"))?;
    let local_x = cx
        .checked_sub(region_base_x)
        .ok_or_else(|| format!("chunk x {cx} underflows region base {region_base_x}"))?;
    let local_z = cz
        .checked_sub(region_base_z)
        .ok_or_else(|| format!("chunk z {cz} underflows region base {region_base_z}"))?;
    if !(0..32).contains(&local_x) || !(0..32).contains(&local_z) {
        return Err(format!(
            "chunk ({cx},{cz}) is outside region ({region_x},{region_z})"
        ));
    }
    Ok((local_z as usize) * 32 + local_x as usize)
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
    fn cli_parser_accepts_zero_or_one_fixture_root_before_or_after_flags() {
        let default_fixture_root = crate::crate_dir().join("fixtures");
        let default_output = crate::crate_dir().join("work/anvil-roundtrip-v1a");
        assert_eq!(
            parse_cli(&[]).expect("defaults parse"),
            CliArgs {
                fixture_root: default_fixture_root.clone(),
                output: default_output.clone(),
            }
        );

        let custom_fixture_root = PathBuf::from("/tmp/custom-fixtures");
        let custom_output = PathBuf::from("/tmp/custom-output");
        assert_eq!(
            parse_cli(&[
                custom_fixture_root.to_str().unwrap(),
                "--out",
                custom_output.to_str().unwrap()
            ])
            .expect("fixture root before flag parses"),
            CliArgs {
                fixture_root: custom_fixture_root.clone(),
                output: custom_output.clone(),
            }
        );
        assert_eq!(
            parse_cli(&[
                "--out",
                custom_output.to_str().unwrap(),
                custom_fixture_root.to_str().unwrap()
            ])
            .expect("fixture root after flag parses"),
            CliArgs {
                fixture_root: custom_fixture_root,
                output: custom_output,
            }
        );
    }

    #[test]
    fn cli_parser_rejects_duplicate_default_and_custom_fixture_roots() {
        let default_fixture_root = crate::crate_dir().join("fixtures");
        let default_fixture_root = default_fixture_root.to_str().unwrap();
        let error = parse_cli(&[default_fixture_root, default_fixture_root])
            .expect_err("duplicate default fixture root must be rejected");
        assert!(
            matches!(error, Error::Gate(message) if message.contains("only one fixture directory"))
        );

        let custom_fixture_root = "/tmp/custom-fixtures";
        let error = parse_cli(&[custom_fixture_root, custom_fixture_root])
            .expect_err("duplicate custom fixture root must be rejected");
        assert!(
            matches!(error, Error::Gate(message) if message.contains("only one fixture directory"))
        );
    }

    #[test]
    fn cli_parser_rejects_unknown_flags_and_missing_output_values() {
        let error = parse_cli(&["--unknown"]).expect_err("unknown flags must be rejected");
        assert!(
            matches!(error, Error::Gate(message) if message.contains("unknown anvil-roundtrip-v1a option"))
        );

        let error = parse_cli(&["--out"]).expect_err("missing output value must be rejected");
        assert!(
            matches!(error, Error::Gate(message) if message.contains("requires a destination directory"))
        );

        let error = parse_cli(&["--out", "--unknown"])
            .expect_err("flag used as output value must be rejected");
        assert!(matches!(error, Error::Gate(message) if message.contains("not an option")));
    }

    #[test]
    fn negative_names_and_targets_are_load_bearing() {
        let kinds = NegativeKind::all();
        let names: Vec<_> = kinds.into_iter().map(NegativeKind::name).collect();
        assert_eq!(
            names,
            [
                "length",
                "compression-byte",
                "location-header",
                "sector-overlap",
                "truncation",
                "trailing-payload"
            ]
        );
        assert_eq!(NegativeKind::Overlap.target_slot(), 1);
        assert_eq!(NegativeKind::Length.target_slot(), 0);
        assert_eq!(
            NegativeKind::Truncation.rejection_stage(),
            RejectionStage::PayloadRead
        );
        assert_eq!(
            NegativeKind::TrailingPayload.rejection_stage(),
            RejectionStage::PayloadRead
        );
    }

    #[test]
    fn committed_identity_recomputes_from_manifest_and_raw_bytes() {
        let root = crate::crate_dir().join("fixtures");
        let manifest = crate::load_manifest(&root).expect("committed manifest must parse");
        assert_eq!(
            crate::raw_corpus_identity(&root, &manifest).expect("raw corpus must hash"),
            EXPECTED_M0_CORPUS_RAW_SHA256
        );
        let bytes = fs::read(root.join("manifest.json")).expect("manifest must read");
        assert_eq!(sha256_hex(&bytes), EXPECTED_M0_MANIFEST_SHA256);
    }

    #[test]
    fn malformed_provenance_is_fail() {
        let root = crate::crate_dir().join("fixtures");
        let mut manifest = crate::load_manifest(&root).expect("committed manifest must parse");
        manifest.seed = Some(String::new());
        assert!(matches!(
            validate_m0_manifest(&manifest),
            Err(Error::Gate(_))
        ));
        let mut manifest = crate::load_manifest(&root).expect("committed manifest must parse");
        manifest.paper = Some(String::new());
        assert!(matches!(
            validate_m0_manifest(&manifest),
            Err(Error::Gate(_))
        ));
        let mut manifest = crate::load_manifest(&root).expect("committed manifest must parse");
        let extra = manifest
            .captured
            .iter()
            .find(|captured| captured.path.starts_with("chunk/"))
            .expect("committed manifest has chunks")
            .clone();
        manifest.captured.push(extra);
        assert!(matches!(
            validate_m0_manifest(&manifest),
            Err(Error::Gate(_))
        ));
    }

    #[test]
    fn strict_payload_parser_rejects_trailing_bytes() {
        let tag = CompoundTag::new();
        let bytes = encode_payload(&tag).expect("empty compound encodes");
        let mut corrupted = bytes.clone();
        corrupted.push(0);
        assert!(parse_payload_exact(&corrupted).is_err());
    }

    #[test]
    fn canonical_paths_reject_equal_nested_and_ancestor_outputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source");
        assert!(matches!(
            canonicalize_paths(&source, &source),
            Err(Error::Gate(_))
        ));
        assert!(matches!(
            canonicalize_paths(&source, &source.join("nested")),
            Err(Error::Gate(_))
        ));
        assert!(matches!(
            canonicalize_paths(&source, temp.path()),
            Err(Error::Gate(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn canonical_paths_reject_symlink_fixture_root() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let alias = temp.path().join("alias");
        let output = temp.path().join("output");
        std::fs::create_dir(&source).expect("source");
        symlink(&source, &alias).expect("symlink");
        assert!(matches!(
            canonicalize_paths(&alias, &output),
            Err(Error::Gate(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn canonical_paths_reject_symlink_ancestors() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let victim = temp.path().join("victim");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&source).expect("source");
        std::fs::create_dir_all(victim.join("child/out")).expect("victim");
        symlink(&victim, &alias).expect("symlink");
        assert!(matches!(
            canonicalize_paths(&source, &alias.join("child/out")),
            Err(Error::Gate(_))
        ));
        let fixture_root = alias.join("fixtures");
        assert!(matches!(
            canonicalize_paths(&fixture_root, &temp.path().join("output")),
            Err(Error::Gate(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn prepare_output_rejects_symlink_ancestor_without_deleting_target() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("tempdir");
        let victim = temp.path().join("victim");
        let alias = temp.path().join("alias");
        let output = alias.join("child/out");
        std::fs::create_dir_all(victim.join("child/out")).expect("victim");
        std::fs::write(victim.join("child/out/KEEP"), b"keep").expect("sentinel");
        symlink(&victim, &alias).expect("symlink");
        assert!(matches!(prepare_output(&output), Err(Error::Gate(_))));
        assert!(victim.join("child/out/KEEP").is_file());
    }

    #[test]
    fn source_hash_validation_runs_after_failed_execution() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source");
        std::fs::write(source.join("manifest.json"), b"before").expect("manifest");
        let before = hash_tree(&source).expect("source hash");
        std::fs::write(source.join("manifest.json"), b"after").expect("tamper");
        let result = finish_source_validation(
            &source,
            &before,
            Err(Error::Unverified("fixture unavailable".into())),
        );
        assert!(
            matches!(result, Err(Error::Gate(message)) if message.contains("mutated the source fixture tree"))
        );
    }

    #[test]
    fn checked_region_coordinate_arithmetic_rejects_overflow() {
        assert!(checked_chunk_coordinate(i32::MAX, 0, 0).is_err());
        assert!(chunk_slot(i32::MAX, 0, 0, 0).is_err());
    }
}
