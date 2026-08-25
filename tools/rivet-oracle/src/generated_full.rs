//! Source-disjoint generated normal-overworld FULL parity harness.
//!
//! This module deliberately does not reuse the committed loaded-world,
//! superflat-FULL, or generated-expected fixtures.  It verifies the future G4
//! corpus as decompressed payloads under `fixtures/generated-full/` and keeps
//! all Rivet output in `work/generated-full/`.
//!
//! The verifier derives a hash manifest from every payload on every run.  A
//! producer manifest is only an integrity witness: it is never used as the
//! source of a digest.  This matters for both the byte-level parity gate and
//! the tamper controls, which rebuild only the derived manifest after changing
//! one payload.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use rivet_nbt::compound_tag::CompoundTag;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::corpus;
use crate::hash;
use crate::hash_manifest::{self, CaptureProvenance, HashManifest};
use crate::mutate::{self, TamperKind};

pub const KIND: &str = "generated-full";
pub const CONTRACT_BASENAME: &str = "contract.json";
pub const PROVENANCE_BASENAME: &str = "provenance.json";
pub const SEED_CONFIG_BASENAME: &str = "seed-config.json";
pub const MANIFEST_BASENAME: &str = "manifest.json";
pub const EXPECTED_STATUS: &str = "minecraft:full";
pub const EXPECTED_LEVEL_TYPE: &str = "minecraft:normal";
pub const EXPECTED_DIMENSION: &str = "overworld";
pub const EXPECTED_COMPRESSION: &str = "none";
pub const EXPECTED_STAGE: &str = "FULL";
pub const EXPECTED_NORMALIZATION: &str = "LastUpdate=0";
pub const EXPECTED_WORKER_THREADS: u32 = 1;
pub const EXPECTED_IO_THREADS: u32 = 1;
const EXPECTED_PAPER_TICKET_LEVEL: u32 = 33;
const EXPECTED_PAPER_SOURCE_PAYLOADS: usize = 2764;
const EXPECTED_PAPER_SOURCE_FULL: usize = 10;
const EXPECTED_PAPER_SAVE_COMPLETION: &str =
    "SIGTERM+All dimensions are saved+RegionFile-I/O shutdown/read-back";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Coordinate {
    pub x: i32,
    pub z: i32,
}

impl Coordinate {
    fn tuple(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    #[serde(rename = "worker-threads")]
    pub worker_threads: u32,
    #[serde(rename = "io-threads")]
    pub io_threads: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPaths {
    #[serde(rename = "paper-materialized-jar")]
    pub paper_materialized_jar: String,
    #[serde(rename = "paper-capture-properties")]
    pub paper_capture_properties: String,
    #[serde(rename = "rivet-producer-binary")]
    pub rivet_producer_binary: String,
    #[serde(rename = "rivet-capture-config")]
    pub rivet_capture_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneratedContract {
    pub format: u64,
    pub kind: String,
    #[serde(rename = "corpus-version")]
    pub corpus_version: String,
    pub stage: String,
    pub seeds: Vec<u64>,
    #[serde(rename = "level-type")]
    pub level_type: String,
    pub dimension: String,
    #[serde(rename = "region-file-compression")]
    pub region_file_compression: String,
    pub status: String,
    #[serde(rename = "hash-algorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "hash-scope")]
    pub hash_scope: String,
    #[serde(rename = "chunk-concurrency")]
    pub chunk_concurrency: Concurrency,
    #[serde(rename = "normalization-rule")]
    pub normalization_rule: String,
    pub regions: Vec<String>,
    pub coordinates: Vec<Coordinate>,
    #[serde(rename = "artifact-paths")]
    pub artifact_paths: ArtifactPaths,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneratedProvenance {
    pub kind: String,
    pub side: String,
    #[serde(rename = "seed-u64")]
    pub seed_u64: u64,
    #[serde(rename = "seed-java-long")]
    pub seed_java_long: i64,
    #[serde(rename = "level-type")]
    pub level_type: String,
    pub dimension: String,
    #[serde(rename = "region-file-compression")]
    pub region_file_compression: String,
    pub status: String,
    #[serde(rename = "hash-algorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "hash-scope")]
    pub hash_scope: String,
    #[serde(rename = "corpus-version")]
    pub corpus_version: String,
    pub stage: String,
    pub regions: Vec<String>,
    pub coordinates: Vec<Coordinate>,
    #[serde(rename = "chunk-concurrency")]
    pub chunk_concurrency: Concurrency,
    #[serde(rename = "normalization-rule")]
    pub normalization_rule: String,
    #[serde(rename = "paper-java-version")]
    pub paper_java_version: String,
    #[serde(rename = "paper-ticket-level")]
    pub paper_ticket_level: u32,
    #[serde(rename = "paper-ticket-coordinates")]
    pub paper_ticket_coordinates: Vec<Coordinate>,
    #[serde(rename = "paper-source-payload-count")]
    pub paper_source_payload_count: usize,
    #[serde(rename = "paper-source-full-count")]
    pub paper_source_full_count: usize,
    #[serde(rename = "paper-boot-log-sha256")]
    pub paper_boot_log_sha256: String,
    #[serde(rename = "paper-save-completion")]
    pub paper_save_completion: String,
    #[serde(rename = "paper-twin-run-sha256")]
    pub paper_twin_run_sha256: String,
    #[serde(rename = "paper-commit")]
    pub paper_commit: String,
    #[serde(rename = "materialized-jar-sha256")]
    pub materialized_jar_sha256: String,
    #[serde(rename = "paper-config-sha256")]
    pub paper_config_sha256: String,
    #[serde(rename = "paper-config-template-sha256")]
    pub paper_config_template_sha256: String,
    #[serde(rename = "rivet-commit")]
    pub rivet_commit: String,
    #[serde(rename = "capture-binary-sha256")]
    pub capture_binary_sha256: String,
    #[serde(rename = "capture-config-sha256")]
    pub capture_config_sha256: String,
    #[serde(rename = "seed-config-sha256")]
    pub seed_config_sha256: String,
    pub synthetic: bool,
}

impl GeneratedProvenance {
    #[cfg(test)]
    fn for_test(contract: &GeneratedContract, side: &str, seed: u64) -> Self {
        Self {
            kind: KIND.to_string(),
            side: side.to_string(),
            seed_u64: seed,
            seed_java_long: java_seed_long(seed),
            level_type: contract.level_type.clone(),
            dimension: contract.dimension.clone(),
            region_file_compression: contract.region_file_compression.clone(),
            status: contract.status.clone(),
            hash_algorithm: contract.hash_algorithm.clone(),
            hash_scope: contract.hash_scope.clone(),
            corpus_version: contract.corpus_version.clone(),
            stage: contract.stage.clone(),
            regions: contract.regions.clone(),
            coordinates: contract.coordinates.clone(),
            chunk_concurrency: contract.chunk_concurrency.clone(),
            normalization_rule: contract.normalization_rule.clone(),
            paper_java_version: "25-test".to_string(),
            paper_ticket_level: EXPECTED_PAPER_TICKET_LEVEL,
            paper_ticket_coordinates: contract.coordinates.clone(),
            paper_source_payload_count: EXPECTED_PAPER_SOURCE_PAYLOADS,
            paper_source_full_count: EXPECTED_PAPER_SOURCE_FULL,
            paper_boot_log_sha256: "5".repeat(64),
            paper_save_completion: EXPECTED_PAPER_SAVE_COMPLETION.to_string(),
            paper_twin_run_sha256: "6".repeat(64),
            paper_commit: hash_manifest::PAPER_PIN.to_string(),
            materialized_jar_sha256: "0".repeat(64),
            paper_config_sha256: "2".repeat(64),
            paper_config_template_sha256: "3".repeat(64),
            rivet_commit: "test-rivet-commit".to_string(),
            capture_binary_sha256: "1".repeat(64),
            capture_config_sha256: "3".repeat(64),
            seed_config_sha256: "4".repeat(64),
            synthetic: true,
        }
    }
}

#[derive(Debug, Clone)]
struct VerifiedSide {
    provenance: GeneratedProvenance,
    manifest: HashManifest,
}

#[derive(Debug, Clone)]
struct Mismatch {
    seed: u64,
    coordinate: (i32, i32),
    expected: String,
    actual: String,
    order_only: bool,
}

/// The canonical contract represented by the committed `contract.json`.
/// Keeping this derivation next to the verifier lets the test suite detect a
/// hand-edited contract that drops a negative coordinate or a seam region.
fn canonical_contract() -> GeneratedContract {
    let coordinates = corpus::COORDINATES
        .iter()
        .map(|&(x, z)| Coordinate { x, z })
        .collect::<Vec<_>>();
    let regions = coordinates
        .iter()
        .map(|c| region_for(c.x, c.z))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    GeneratedContract {
        format: 1,
        kind: KIND.to_string(),
        corpus_version: hash_manifest::CORPUS_VERSION.to_string(),
        stage: EXPECTED_STAGE.to_string(),
        seeds: corpus::corpus_seeds(),
        level_type: EXPECTED_LEVEL_TYPE.to_string(),
        dimension: EXPECTED_DIMENSION.to_string(),
        region_file_compression: EXPECTED_COMPRESSION.to_string(),
        status: EXPECTED_STATUS.to_string(),
        hash_algorithm: hash_manifest::HASH_ALGORITHM.to_string(),
        hash_scope: hash_manifest::HASH_SCOPE.to_string(),
        chunk_concurrency: Concurrency {
            worker_threads: EXPECTED_WORKER_THREADS,
            io_threads: EXPECTED_IO_THREADS,
        },
        normalization_rule: EXPECTED_NORMALIZATION.to_string(),
        regions,
        coordinates,
        artifact_paths: ArtifactPaths {
            paper_materialized_jar: "work/generated-full/artifacts/paper-26.2.jar".to_string(),
            paper_capture_properties: "fixtures/generated-full/server-normal-full.properties"
                .to_string(),
            rivet_producer_binary: "work/generated-full/artifacts/rivet-capture".to_string(),
            rivet_capture_config: "work/generated-full/artifacts/rivet-capture-config.json"
                .to_string(),
        },
    }
}

/// Load and structurally validate the generated-FULL contract.
pub fn load_contract(path: &Path) -> Result<GeneratedContract, Error> {
    let raw = read_stable_file(path, "generated-full contract")?;
    let raw = std::str::from_utf8(&raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full contract {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    let contract: GeneratedContract = serde_json::from_str(raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full contract {} is malformed: {e}",
            path.display()
        ))
    })?;
    validate_contract(&contract)?;
    Ok(contract)
}

fn validate_contract(contract: &GeneratedContract) -> Result<(), Error> {
    let expected = canonical_contract();
    if contract.format != expected.format {
        return Err(Error::Gate(format!(
            "generated-full contract format {} is unsupported (expected {})",
            contract.format, expected.format
        )));
    }
    if contract.kind != expected.kind
        || contract.corpus_version != expected.corpus_version
        || contract.stage != expected.stage
        || contract.seeds != expected.seeds
        || contract.level_type != expected.level_type
        || contract.dimension != expected.dimension
        || contract.region_file_compression != expected.region_file_compression
        || contract.status != expected.status
        || contract.hash_algorithm != expected.hash_algorithm
        || contract.hash_scope != expected.hash_scope
        || contract.chunk_concurrency != expected.chunk_concurrency
        || contract.normalization_rule != expected.normalization_rule
        || contract.regions != expected.regions
        || contract.coordinates != expected.coordinates
        || contract.artifact_paths != expected.artifact_paths
    {
        return Err(Error::Gate(
            "generated-full contract does not match the pinned normal-overworld FULL corpus, including the four origin-adjacent regions and both seam coordinates".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ArtifactInputs {
    paper_jar: PathBuf,
    paper_properties: PathBuf,
    rivet_binary: PathBuf,
    rivet_config: PathBuf,
    rivet_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactIdentity {
    paper_commit: String,
    materialized_jar_sha256: String,
    paper_config_template_sha256: String,
    paper_config_template: Vec<u8>,
    rivet_commit: String,
    capture_binary_sha256: String,
    capture_config_sha256: String,
}

impl ArtifactInputs {
    fn from_contract(contract: &GeneratedContract) -> Result<Self, Error> {
        // Artifact paths are part of the checked-in contract.  They are not
        // overridable through environment variables: an unrecorded jar/config
        // is not an attestation and would make the parity result irreproducible.
        let path = |value: &str| -> Result<PathBuf, Error> {
            let relative = Path::new(value);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(Error::Gate(format!(
                    "generated-full contract artifact path {value:?} must be a relative path inside the oracle crate"
                )));
            }
            Ok(crate::crate_dir().join(relative))
        };
        let rivet_commit = std::process::Command::new("git")
            .args(["-C", &crate::crate_dir().to_string_lossy(), "rev-parse", "--verify", "HEAD"])
            .output()
            .map_err(|e| {
                Error::Unverified(format!(
                    "generated-full Rivet producer HEAD cannot be derived from the tested checkout: {e}"
                ))
            })?;
        if !rivet_commit.status.success() {
            return Err(Error::Unverified(
                "generated-full Rivet producer HEAD cannot be derived from the tested checkout"
                    .into(),
            ));
        }
        let rivet_commit = String::from_utf8(rivet_commit.stdout)
            .map_err(|e| Error::Unverified(format!("generated-full Rivet HEAD is not UTF-8: {e}")))?
            .trim()
            .to_string();
        if !is_git_commit(&rivet_commit) {
            return Err(Error::Unverified(format!(
                "generated-full Rivet producer HEAD is not a commit hash: {rivet_commit:?}"
            )));
        }
        Ok(Self {
            paper_jar: path(&contract.artifact_paths.paper_materialized_jar)?,
            paper_properties: path(&contract.artifact_paths.paper_capture_properties)?,
            rivet_binary: path(&contract.artifact_paths.rivet_producer_binary)?,
            rivet_config: path(&contract.artifact_paths.rivet_capture_config)?,
            rivet_commit,
        })
    }

    fn identity(&self) -> Result<ArtifactIdentity, Error> {
        let read_hash = |label: &str, path: &Path| -> Result<(Vec<u8>, String), Error> {
            let bytes = read_stable_file(path, &format!("generated-full expected {label}"))?;
            if bytes.is_empty() {
                return Err(Error::Gate(format!(
                    "generated-full expected {label} {} is empty; an existing artifact is malformed",
                    path.display()
                )));
            }
            let digest = crate::sha256_hex(&bytes);
            Ok((bytes, digest))
        };
        let mut paper_jar = open_stable_regular(
            &self.paper_jar,
            "generated-full expected Paper materialized jar",
        )?;
        let mut paper_jar_bytes = Vec::new();
        paper_jar.read_to_end(&mut paper_jar_bytes).map_err(|error| {
            Error::Gate(format!(
                "generated-full Paper materialized jar {} cannot be read through its opened descriptor: {error}",
                self.paper_jar.display()
            ))
        })?;
        if paper_jar_bytes.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full Paper materialized jar {} is empty; an existing artifact is malformed",
                self.paper_jar.display()
            )));
        }
        let materialized_jar_sha256 = crate::sha256_hex(&paper_jar_bytes);
        let paper_commit = read_jar_git_commit_opened(&paper_jar).map_err(|e| {
            Error::Gate(format!(
                "generated-full Paper jar attestation failed for existing artifact: {e}"
            ))
        })?.ok_or_else(|| {
            Error::Gate(format!(
                "generated-full Paper jar {} has no Git-Commit manifest attribute; existing artifact is not a pinned Paper server jar",
                self.paper_jar.display()
            ))
        })?;
        if paper_commit != hash_manifest::PAPER_PIN {
            return Err(Error::Gate(format!(
                "generated-full Paper jar {} carries Git-Commit {paper_commit}, expected authoritative PAPER_PIN {}",
                self.paper_jar.display(),
                hash_manifest::PAPER_PIN
            )));
        }
        let paper_properties = read_canonical_paper_properties(&self.paper_properties)?;
        let rivet_binary = read_executable_file(&self.rivet_binary)?;
        let (_, capture_config_sha256) = read_hash("Rivet capture config", &self.rivet_config)?;
        Ok(ArtifactIdentity {
            paper_commit: hash_manifest::PAPER_PIN.to_string(),
            materialized_jar_sha256,
            paper_config_template_sha256: crate::sha256_hex(&paper_properties),
            paper_config_template: paper_properties,
            rivet_commit: self.rivet_commit.clone(),
            capture_binary_sha256: crate::sha256_hex(&rivet_binary),
            capture_config_sha256,
        })
    }
}

/// Verify every declared seed using the default source-disjoint tree layout.
pub fn verify_default() -> Result<(), Error> {
    let fixture_root = crate::crate_dir().join("fixtures/generated-full");
    verify_fixture_outer_closure(&fixture_root)?;
    let contract = load_contract(&fixture_root.join(CONTRACT_BASENAME))?;
    let paper_root = fixture_root.join("paper");
    let rivet_root = crate::crate_dir().join("work/generated-full/rivet");
    let artifacts = ArtifactInputs::from_contract(&contract)?;
    let identity = artifacts.identity()?;
    verify_roots_with_identity(&contract, &paper_root, &rivet_root, &identity, false)
}

/// Verify a contract against explicit Paper and Rivet seed roots. Production
/// verification has no synthetic escape hatch: it requires the canonical
/// artifact attestation supplied by `verify-generated-full` or the promotion
/// gate. Unit tests use `verify_synthetic_roots` for parser/tamper coverage.
pub fn verify_roots(
    contract: &GeneratedContract,
    paper_root: &Path,
    rivet_root: &Path,
) -> Result<(), Error> {
    validate_contract(contract)?;
    let artifacts = ArtifactInputs::from_contract(contract)?;
    let identity = artifacts.identity()?;
    verify_roots_with_identity(contract, paper_root, rivet_root, &identity, false)
}

#[cfg(test)]
fn verify_synthetic_roots(
    contract: &GeneratedContract,
    paper_root: &Path,
    rivet_root: &Path,
) -> Result<(), Error> {
    let identity = ArtifactIdentity {
        paper_commit: hash_manifest::PAPER_PIN.to_string(),
        materialized_jar_sha256: "0".repeat(64),
        paper_config_template_sha256: "2".repeat(64),
        paper_config_template: Vec::new(),
        rivet_commit: "test-rivet-commit".to_string(),
        capture_binary_sha256: "1".repeat(64),
        capture_config_sha256: "3".repeat(64),
    };
    verify_roots_with_identity(contract, paper_root, rivet_root, &identity, true)
}

fn verify_roots_with_identity(
    contract: &GeneratedContract,
    paper_root: &Path,
    rivet_root: &Path,
    identity: &ArtifactIdentity,
    allow_synthetic: bool,
) -> Result<(), Error> {
    validate_contract(contract)?;
    hash::self_check().map_err(Error::Gate)?;
    verify_seed_root_closure(paper_root, contract, "paper")?;
    verify_seed_root_closure(rivet_root, contract, "rivet")?;

    let mut paper_producer: Option<GeneratedProvenance> = None;
    let mut rivet_producer: Option<GeneratedProvenance> = None;
    for &seed in &contract.seeds {
        let paper = paper_root.join(seed.to_string());
        let rivet = rivet_root.join(seed.to_string());
        let paper_canon = canonical_side_path(&paper)?;
        let rivet_canon = canonical_side_path(&rivet)?;
        if paper_canon == rivet_canon {
            return Err(Error::Gate(format!(
                "generated-full seed {seed}: Paper and Rivet resolve to the same tree {} — refusing a Paper-vs-Paper self-diff as malformed aliased input",
                paper_canon.display()
            )));
        }
        let paper_side = verify_side(contract, &paper, seed, "paper", identity, allow_synthetic)?;
        let rivet_side = verify_side(contract, &rivet, seed, "rivet", identity, allow_synthetic)?;
        if let Some(reference) = &paper_producer {
            if !same_producer_attestation(reference, &paper_side.provenance) {
                return Err(Error::Gate(format!(
                    "generated-full Paper producer attestation changes across seeds; seed {seed} does not use one captured jar/config identity"
                )));
            }
        } else {
            paper_producer = Some(paper_side.provenance.clone());
        }
        if let Some(reference) = &rivet_producer {
            if !same_producer_attestation(reference, &rivet_side.provenance) {
                return Err(Error::Gate(format!(
                    "generated-full Rivet producer attestation changes across seeds; seed {seed} does not use one captured binary/config identity"
                )));
            }
        } else {
            rivet_producer = Some(rivet_side.provenance.clone());
        }
        compare_seed(contract, seed, &paper_side, &rivet_side)?;
    }
    println!(
        "PASS: generated-full normal-overworld FULL parity verified ({} seeds, {} coordinates, four origin-adjacent regions)",
        contract.seeds.len(),
        contract.coordinates.len()
    );
    Ok(())
}

fn verify_fixture_outer_closure(root: &Path) -> Result<(), Error> {
    // Inspect link metadata before `exists()`: a broken symlink is an existing
    // malformed artifact, not an absent capture that may be reported as merely
    // UNVERIFIED.
    reject_symlink_components(root, "generated-full fixture root")?;
    reject_symlink_if_present(root, "generated-full fixture root")?;
    if !root.exists() {
        return Err(Error::Unverified(format!(
            "generated-full fixture root {} is absent — FULL coverage is UNVERIFIED",
            root.display()
        )));
    }
    reject_symlink(root, "generated-full fixture root")?;
    if !is_real_dir(root) {
        return Err(Error::Gate(format!(
            "generated-full fixture root {} is not a directory",
            root.display()
        )));
    }
    let expected = BTreeSet::from([
        CONTRACT_BASENAME.to_string(),
        "paper".to_string(),
        "server-normal-full.properties".to_string(),
    ]);
    let mut actual = BTreeSet::new();
    for entry in read_dir_strict(root, "fixture", 0)? {
        let path = entry.path();
        reject_symlink(&path, "generated-full fixture root entry")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !expected.contains(&name) {
            return Err(Error::Gate(format!(
                "generated-full fixture root contains extra entry {name:?}; exact outer closure is contract.json, server-normal-full.properties, and paper/"
            )));
        }
        if name == CONTRACT_BASENAME {
            if !is_regular_file(&path) {
                return Err(Error::Gate(
                    "generated-full fixture contract.json is not a regular file".into(),
                ));
            }
            reject_hardlink(&path, "generated-full fixture contract")?;
        } else if name == "server-normal-full.properties" {
            if !is_regular_file(&path) {
                return Err(Error::Gate(
                    "generated-full fixture server-normal-full.properties is not a regular file"
                        .into(),
                ));
            }
            reject_hardlink(&path, "generated-full fixture Paper properties")?;
        } else if !is_real_dir(&path) {
            return Err(Error::Gate(
                "generated-full fixture paper entry is not a directory".into(),
            ));
        }
        actual.insert(name);
    }
    if actual != expected {
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(Error::Unverified(format!(
            "generated-full fixture root is missing declared entries {missing:?} — FULL coverage is UNVERIFIED"
        )));
    }
    Ok(())
}

fn canonical_side_path(path: &Path) -> Result<PathBuf, Error> {
    reject_symlink_components(path, "generated-full side path")?;
    path.canonicalize().map_err(|e| {
        Error::Unverified(format!(
            "generated-full side {} is absent or cannot be resolved: {e}",
            path.display()
        ))
    })
}

fn verify_seed_root_closure(
    root: &Path,
    contract: &GeneratedContract,
    side: &str,
) -> Result<(), Error> {
    // A broken symlink is malformed existing output, so inspect link metadata
    // before the existence check that classifies a genuinely absent root.
    reject_symlink_components(root, &format!("generated-full {side} root"))?;
    reject_symlink_if_present(root, &format!("generated-full {side} root"))?;
    if !root.exists() {
        return Err(Error::Unverified(format!(
            "generated-full {side} root {} is absent — FULL coverage is UNVERIFIED",
            root.display()
        )));
    }
    reject_symlink(root, &format!("generated-full {side} root"))?;
    if !is_real_dir(root) {
        return Err(Error::Gate(format!(
            "generated-full {side} root {} is not a directory",
            root.display()
        )));
    }
    let expected: BTreeSet<String> = contract.seeds.iter().map(ToString::to_string).collect();
    let mut actual = BTreeSet::new();
    for entry in read_dir_strict(root, side, 0)? {
        let path = entry.path();
        reject_symlink(&path, "generated-full seed-root entry")?;
        reject_hardlink(&path, "generated-full seed-root entry")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_real_dir(&path) {
            return Err(Error::Gate(format!(
                "generated-full {side} root contains non-directory entry {name:?}; only declared seed directories are permitted",
            )));
        }
        if !expected.contains(&name) {
            return Err(Error::Gate(format!(
                "generated-full {side} root contains extra seed directory {name:?}; declared seeds are {:?}",
                contract.seeds
            )));
        }
        actual.insert(name);
    }
    if actual != expected {
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(Error::Unverified(format!(
            "generated-full {side} root is missing declared seed directories {missing:?} — FULL coverage is UNVERIFIED",
        )));
    }
    Ok(())
}

fn verify_side(
    contract: &GeneratedContract,
    root: &Path,
    seed: u64,
    side: &str,
    identity: &ArtifactIdentity,
    allow_synthetic: bool,
) -> Result<VerifiedSide, Error> {
    // Distinguish a genuinely absent seed tree from a present malformed one;
    // inspect symlink metadata before `is_dir()` so a broken symlink cannot be
    // downgraded to UNVERIFIED.
    reject_symlink_components(root, &format!("generated-full {side} seed tree"))?;
    reject_symlink_if_present(root, &format!("generated-full {side} seed tree"))?;
    if !root.exists() {
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} tree {} is absent — FULL coverage is UNVERIFIED",
            root.display()
        )));
    }
    if !is_real_dir(root) {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} tree {} is not a directory",
            root.display()
        )));
    }
    reject_symlink(root, &format!("generated-full {side} root"))?;
    let seed_config_sha256 = read_seed_config(root, contract, seed, side)?;
    let provenance = read_provenance(
        root,
        contract,
        seed,
        side,
        identity,
        &seed_config_sha256,
        allow_synthetic,
    )?;
    let expected_root_entries = BTreeSet::from([
        PROVENANCE_BASENAME.to_string(),
        SEED_CONFIG_BASENAME.to_string(),
        MANIFEST_BASENAME.to_string(),
        "chunk".to_string(),
    ]);
    let mut actual_root_entries = BTreeSet::new();
    for entry in read_dir_strict(root, side, seed)? {
        let path = entry.path();
        reject_symlink(&path, "generated-full seed tree")?;
        reject_hardlink(&path, "generated-full seed tree")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !expected_root_entries.contains(&name) {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} has extra root entry {name:?}; only provenance.json, seed-config.json, manifest.json, and chunk/ are permitted",
            )));
        }
        actual_root_entries.insert(name);
    }
    if actual_root_entries != expected_root_entries {
        let missing = expected_root_entries
            .difference(&actual_root_entries)
            .cloned()
            .collect::<Vec<_>>();
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} is missing declared root entries {missing:?} — FULL coverage is UNVERIFIED",
        )));
    }
    let manifest_path = root.join(MANIFEST_BASENAME);
    reject_symlink_components(&manifest_path, "generated-full manifest")?;
    reject_symlink_if_present(&manifest_path, "generated-full manifest")?;
    if !manifest_path.exists() {
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} has no {MANIFEST_BASENAME} — derived FULL coverage is UNVERIFIED",
        )));
    }
    if !is_regular_file(&manifest_path) {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} {MANIFEST_BASENAME} is not a regular file",
        )));
    }
    let supplied_manifest = read_manifest(&manifest_path)?;
    let discovered = discover_payloads(root, contract, seed, side)?;
    if discovered.is_empty() {
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} has no FULL payloads — G4 output is UNVERIFIED",
        )));
    }

    // Check each payload's internal position before deriving hashes. A
    // relabeled file must not become a valid coordinate merely because its
    // filename is in the expected closure. The LastUpdate rule is the one
    // canonicalization invariant for this harness: captures must serialize an
    // explicit root `LastUpdate` long with value 0, and no other normalization
    // is applied before byte-level comparison.
    let mut content_fingerprints = BTreeSet::new();
    for payload in &discovered {
        let bytes = &payload.bytes;
        let compound = mutate::parse_payload(bytes).map_err(|e| {
            Error::Gate(format!(
                "generated-full {side} seed-{seed} payload {} is malformed: {e}",
                payload.path.display()
            ))
        })?;
        let stored = (compound.get_int("xPos"), compound.get_int("zPos"));
        if stored != (Some(payload.cx), Some(payload.cz)) {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} payload {} stores xPos/zPos {:?}, expected ({}, {})",
                payload.path.display(),
                stored,
                payload.cx,
                payload.cz
            )));
        }
        require_canonical_last_update(&compound, &payload.path)?;
        content_fingerprints.insert(content_fingerprint(&compound)?);
    }
    if content_fingerprints.len() < 2 {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} payloads are content-identical after removing coordinates and tick counters; normal-overworld output is indistinguishable from a superflat echo"
        )));
    }

    let capture = CaptureProvenance {
        level_type: contract.level_type.clone(),
        region_file_compression: contract.region_file_compression.clone(),
        corpus_version: contract.corpus_version.clone(),
    };
    let stable_payloads = discovered
        .iter()
        .map(|payload| hash_manifest::PayloadBytes {
            dim: contract.dimension.clone(),
            region: region_for(payload.cx, payload.cz),
            cx: payload.cx,
            cz: payload.cz,
            bytes: payload.bytes.clone(),
        })
        .collect::<Vec<_>>();
    let derived = hash_manifest::build_from_payload_bytes_with(
        &stable_payloads,
        &seed.to_string(),
        &contract.level_type,
        &capture,
    )
    .map_err(|e| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} cannot derive manifest from payloads: {e}"
        ))
    })?;
    if derived.entries.len() != contract.coordinates.len()
        || derived.full_count != contract.coordinates.len()
        || derived.entries.iter().any(|entry| !entry.is_full())
    {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} contains lower-status or incomplete payloads; all {} declared chunks must be minecraft:full",
            contract.coordinates.len()
        )));
    }
    if supplied_manifest != derived {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} manifest.json is stale or producer-supplied digests do not match the payload-derived manifest; never trusting it",
        )));
    }
    Ok(VerifiedSide {
        provenance,
        manifest: derived,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedConfig {
    #[serde(rename = "seed-u64")]
    seed_u64: u64,
    #[serde(rename = "seed-java-long")]
    seed_java_long: i64,
    #[serde(rename = "level-type")]
    level_type: String,
    dimension: String,
    #[serde(rename = "region-file-compression")]
    region_file_compression: String,
    status: String,
    stage: String,
}

fn read_seed_config(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
) -> Result<String, Error> {
    let path = root.join(SEED_CONFIG_BASENAME);
    let bytes = read_stable_file(&path, "generated-full seed config")?;
    let config: SeedConfig = serde_json::from_slice(&bytes).map_err(|e| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} config is malformed: {e}"
        ))
    })?;
    if config.seed_u64 != seed
        || config.seed_java_long != java_seed_long(seed)
        || config.level_type != contract.level_type
        || config.dimension != contract.dimension
        || config.region_file_compression != contract.region_file_compression
        || config.status != contract.status
        || config.stage != contract.stage
    {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} config is stale or bound to a different seed/capture contract",
        )));
    }
    Ok(crate::sha256_hex(&bytes))
}

fn read_provenance(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
    identity: &ArtifactIdentity,
    seed_config_sha256: &str,
    allow_synthetic: bool,
) -> Result<GeneratedProvenance, Error> {
    let path = root.join(PROVENANCE_BASENAME);
    let raw = read_stable_file(&path, "generated-full provenance")?;
    let raw = std::str::from_utf8(&raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} provenance {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    let provenance: GeneratedProvenance = serde_json::from_str(raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} provenance is malformed: {e}"
        ))
    })?;
    if provenance.synthetic && !allow_synthetic {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} provenance is marked synthetic; synthetic fixtures may exercise parsing/tamper only and can never produce a production PASS",
        )));
    }
    if !provenance_matches(
        contract,
        &provenance,
        seed,
        side,
        identity,
        seed_config_sha256,
        allow_synthetic,
    ) {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} provenance is stale, tampered, or bound to the wrong seed; expected raw u64 seed {}, signed Java-long {}, authoritative Paper pin {}, and attested Paper/Rivet artifact identity",
            seed,
            java_seed_long(seed),
            hash_manifest::PAPER_PIN
        )));
    }
    Ok(provenance)
}

fn provenance_matches(
    contract: &GeneratedContract,
    provenance: &GeneratedProvenance,
    seed: u64,
    side: &str,
    identity: &ArtifactIdentity,
    seed_config_sha256: &str,
    allow_synthetic: bool,
) -> bool {
    provenance.kind == contract.kind
        && provenance.side == side
        && provenance.seed_u64 == seed
        && provenance.seed_java_long == java_seed_long(seed)
        && provenance.level_type == contract.level_type
        && provenance.dimension == contract.dimension
        && provenance.region_file_compression == contract.region_file_compression
        && provenance.status == contract.status
        && provenance.hash_algorithm == contract.hash_algorithm
        && provenance.hash_scope == contract.hash_scope
        && provenance.corpus_version == contract.corpus_version
        && provenance.stage == contract.stage
        && provenance.regions == contract.regions
        && provenance.coordinates == contract.coordinates
        && provenance.chunk_concurrency == contract.chunk_concurrency
        && provenance.normalization_rule == contract.normalization_rule
        && provenance.paper_java_version.starts_with("25")
        && provenance.paper_ticket_level == EXPECTED_PAPER_TICKET_LEVEL
        && provenance.paper_ticket_coordinates == contract.coordinates
        && provenance.paper_source_payload_count == EXPECTED_PAPER_SOURCE_PAYLOADS
        && provenance.paper_source_full_count == EXPECTED_PAPER_SOURCE_FULL
        && is_sha256(&provenance.paper_boot_log_sha256)
        && provenance.paper_save_completion == EXPECTED_PAPER_SAVE_COMPLETION
        && is_sha256(&provenance.paper_twin_run_sha256)
        && provenance.seed_config_sha256 == seed_config_sha256
        && is_sha256(&provenance.seed_config_sha256)
        && match_side_artifacts(provenance, identity, allow_synthetic)
}

fn match_side_artifacts(
    provenance: &GeneratedProvenance,
    identity: &ArtifactIdentity,
    allow_synthetic: bool,
) -> bool {
    if allow_synthetic && provenance.synthetic {
        return provenance.paper_commit == hash_manifest::PAPER_PIN
            && is_sha256(&provenance.materialized_jar_sha256)
            && is_sha256(&provenance.paper_config_sha256)
            && is_sha256(&provenance.paper_config_template_sha256)
            && !provenance.rivet_commit.is_empty()
            && is_sha256(&provenance.capture_binary_sha256)
            && is_sha256(&provenance.capture_config_sha256);
    }
    let Ok(expected_config) = expected_paper_properties_sha256(identity, provenance.seed_u64)
    else {
        return false;
    };
    provenance.paper_commit == identity.paper_commit
        && provenance.materialized_jar_sha256 == identity.materialized_jar_sha256
        && provenance.paper_config_template_sha256 == identity.paper_config_template_sha256
        && provenance.paper_config_sha256 == expected_config
        && provenance.rivet_commit == identity.rivet_commit
        && provenance.capture_binary_sha256 == identity.capture_binary_sha256
        && provenance.capture_config_sha256 == identity.capture_config_sha256
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_git_commit(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

const PAPER_CAPTURE_SERVER_PORT: &str = "0";
const PAPER_CAPTURE_TEMPLATE_SHA256: &str =
    "e4db15743f738ad16056c41dac29d4e2ffa0bcc06cd280f08c26b5a87f0b1875";

/// Read and validate the dedicated pre-boot Paper properties template. Paper
/// rewrites/timestamps the run-dir copy during boot, so the verifier never
/// hashes that mutable file. This canonical source is the only accepted
/// configuration evidence and only `level-seed`/`server-port` may be rewritten
/// for an individual seed run.
fn read_canonical_paper_properties(path: &Path) -> Result<Vec<u8>, Error> {
    let bytes = read_stable_file(path, "generated-full Paper capture properties")?;
    validate_canonical_paper_properties_bytes(&bytes, path)?;
    let actual = crate::sha256_hex(&bytes);
    if actual != PAPER_CAPTURE_TEMPLATE_SHA256 {
        return Err(Error::Gate(format!(
            "generated-full Paper capture properties {} do not match the pinned canonical template SHA-256 {}; got {actual}",
            path.display(),
            PAPER_CAPTURE_TEMPLATE_SHA256
        )));
    }
    Ok(bytes)
}

fn validate_canonical_paper_properties_bytes(bytes: &[u8], path: &Path) -> Result<(), Error> {
    if bytes.is_empty() {
        return Err(Error::Gate(format!(
            "generated-full Paper capture properties {} are empty; an existing template is malformed",
            path.display()
        )));
    }
    if !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
        return Err(Error::Gate(format!(
            "generated-full Paper capture properties {} must use a canonical LF-terminated template",
            path.display()
        )));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        Error::Gate(format!(
            "generated-full Paper capture properties {} are not UTF-8: {e}",
            path.display()
        ))
    })?;
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            Error::Gate(format!(
                "generated-full Paper capture properties {} has malformed line {line:?}",
                path.display()
            ))
        })?;
        if key.is_empty() || values.insert(key, value).is_some() {
            return Err(Error::Gate(format!(
                "generated-full Paper capture properties {} has a duplicate or empty property key {key:?}",
                path.display()
            )));
        }
    }
    let required = [
        ("level-seed", "0"),
        ("server-port", PAPER_CAPTURE_SERVER_PORT),
        ("level-type", "minecraft\\:normal"),
        ("generate-structures", "true"),
        ("region-file-compression", "none"),
        ("view-distance", "2"),
        ("simulation-distance", "2"),
        ("online-mode", "false"),
        ("enable-status", "false"),
        ("sync-chunk-writes", "true"),
    ];
    for (key, expected) in required {
        if values.get(key).copied() != Some(expected) {
            return Err(Error::Gate(format!(
                "generated-full Paper capture properties {} must contain {key}={expected}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn rewrite_paper_properties_for_seed(template: &[u8], seed: u64) -> Result<Vec<u8>, Error> {
    let path = Path::new("generated-full Paper capture properties template");
    validate_canonical_paper_properties_bytes(template, path)?;
    let text = std::str::from_utf8(template).map_err(|e| Error::Gate(e.to_string()))?;
    let seed_line = format!("level-seed={}", java_seed_long(seed));
    let mut rewritten = String::with_capacity(text.len() + 24);
    for line in text.lines() {
        if line.starts_with("level-seed=") {
            rewritten.push_str(&seed_line);
        } else if line.starts_with("server-port=") {
            rewritten.push_str("server-port=");
            rewritten.push_str(PAPER_CAPTURE_SERVER_PORT);
        } else {
            rewritten.push_str(line);
        }
        rewritten.push('\n');
    }
    Ok(rewritten.into_bytes())
}

fn expected_paper_properties_sha256(
    identity: &ArtifactIdentity,
    seed: u64,
) -> Result<String, Error> {
    if identity.paper_config_template.is_empty() {
        return Err(Error::Gate(
            "generated-full Paper canonical properties bytes are unavailable for provenance attestation".into(),
        ));
    }
    let rewritten = rewrite_paper_properties_for_seed(&identity.paper_config_template, seed)?;
    Ok(crate::sha256_hex(&rewritten))
}

#[derive(Debug, Clone)]
struct PayloadFile {
    path: PathBuf,
    relative: String,
    cx: i32,
    cz: i32,
    /// Bytes read from the same descriptor whose metadata was checked.  The
    /// verifier never reopens `path` after this evidence is acquired.
    bytes: Vec<u8>,
}

fn require_canonical_last_update(compound: &CompoundTag, path: &Path) -> Result<(), Error> {
    match compound.get_long("LastUpdate") {
        Some(0) => Ok(()),
        Some(value) => Err(Error::Gate(format!(
            "generated-full payload {} has LastUpdate={value}; canonical normalization requires LastUpdate=0",
            path.display()
        ))),
        None => Err(Error::Gate(format!(
            "generated-full payload {} has no LastUpdate; canonical normalization requires an explicit LastUpdate=0",
            path.display()
        ))),
    }
}

fn content_fingerprint(compound: &CompoundTag) -> Result<String, Error> {
    let mut content = compound.clone();
    // Coordinates and tick counters are capture metadata, not worldgen
    // content. Removing exactly these fields makes the anti-superflat check
    // test terrain/content variation rather than filename or position noise.
    for key in ["xPos", "zPos", "LastUpdate", "InhabitedTime"] {
        content.tags.shift_remove(key);
    }
    // Use the semantic NBT digest rather than re-encoding the insertion-ordered
    // compound: key-order-only mutations must not manufacture terrain diversity.
    crate::semantic_hash::canonical_xxh3_64(&content).map_err(|e| {
        Error::Gate(format!(
            "generated-full payload content cannot be canonicalized for anti-superflat validation: {e}"
        ))
    })
}

fn discover_payloads(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
) -> Result<Vec<PayloadFile>, Error> {
    let chunk_root = root.join("chunk");
    // A broken chunk symlink is malformed existing output; only a truly absent
    // chunk tree is an UNVERIFIED missing prerequisite.
    reject_symlink_components(&chunk_root, "generated-full chunk root")?;
    reject_symlink_if_present(&chunk_root, "generated-full chunk root")?;
    if !chunk_root.exists() {
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} has no chunk/ payload tree — FULL coverage is UNVERIFIED",
        )));
    }
    reject_symlink(&chunk_root, "generated-full chunk root")?;
    if !is_real_dir(&chunk_root) {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} chunk/ is not a directory",
        )));
    }
    let expected_paths = expected_paths(contract);
    let expected_regions: BTreeSet<&str> = contract.regions.iter().map(String::as_str).collect();
    let mut dimensions = BTreeSet::new();
    let mut regions = BTreeSet::new();
    let mut discovered = Vec::new();
    let mut logical_coordinates = BTreeSet::new();

    for dim_entry in read_dir_strict(&chunk_root, side, seed)? {
        let dim_path = dim_entry.path();
        reject_symlink(&dim_path, "generated-full chunk tree")?;
        if !is_real_dir(&dim_path) {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} has a non-directory entry under chunk/: {}",
                dim_path.display()
            )));
        }
        let dim = dim_entry.file_name().to_string_lossy().into_owned();
        dimensions.insert(dim.clone());
        if dim != contract.dimension {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} contains dimension {dim:?}; exact closure is overworld only",
            )));
        }
        for region_entry in read_dir_strict(&dim_path, side, seed)? {
            let region_path = region_entry.path();
            reject_symlink(&region_path, "generated-full region tree")?;
            if !is_real_dir(&region_path) {
                return Err(Error::Gate(format!(
                    "generated-full {side} seed-{seed} has a non-directory entry under {}",
                    region_path.display()
                )));
            }
            let region = region_entry.file_name().to_string_lossy().into_owned();
            regions.insert(region.clone());
            if !expected_regions.contains(region.as_str()) {
                return Err(Error::Gate(format!(
                    "generated-full {side} seed-{seed} has extra region {region}; declared region closure is {:?}",
                    contract.regions
                )));
            }
            for file_entry in read_dir_strict(&region_path, side, seed)? {
                let path = file_entry.path();
                reject_symlink(&path, "generated-full payload tree")?;
                reject_hardlink(&path, "generated-full payload tree")?;
                if !is_regular_file(&path) {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} has a non-file payload entry {}",
                        path.display()
                    )));
                }
                if path.extension().and_then(|s| s.to_str()) != Some("nbt") {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} has an extra non-NBT file {}",
                        path.display()
                    )));
                }
                let (cx, cz) = parse_coordinate_filename(&path).map_err(|e| {
                    Error::Gate(format!(
                        "generated-full {side} seed-{seed} payload {} is malformed: {e}",
                        path.display()
                    ))
                })?;
                if region_for(cx, cz) != region {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} payload {cx}.{cz}.nbt is in region {region}, expected {}",
                        region_for(cx, cz)
                    )));
                }
                if !logical_coordinates.insert((cx, cz)) {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} has duplicate coordinate {cx}.{cz}"
                    )));
                }
                let relative = format!("chunk/{}/{}/{}.{}.nbt", contract.dimension, region, cx, cz);
                let bytes = read_stable_file(&path, "generated-full nested payload")?;
                discovered.push(PayloadFile {
                    path,
                    relative,
                    cx,
                    cz,
                    bytes,
                });
            }
        }
    }

    if !dimensions.contains(&contract.dimension) {
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} is missing declared dimension {} — FULL coverage is UNVERIFIED",
            contract.dimension
        )));
    }
    if dimensions != BTreeSet::from([contract.dimension.clone()]) {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} dimension closure {:?} != {:?}",
            dimensions,
            std::slice::from_ref(&contract.dimension)
        )));
    }
    let actual_region_refs: BTreeSet<&str> = regions.iter().map(String::as_str).collect();
    if actual_region_refs != expected_regions {
        // A missing region is an incomplete producer output.  Extra regions
        // were rejected above as malformed artifacts; this branch names the
        // missing closure without converting a not-yet-supplied corpus to green.
        let missing = expected_regions
            .difference(&actual_region_refs)
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(Error::Unverified(format!(
                "generated-full {side} seed-{seed} is missing declared regions {missing:?} — FULL coverage is UNVERIFIED",
            )));
        }
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} region closure {:?} != {:?}",
            regions, contract.regions
        )));
    }

    discovered.sort_by(|a, b| a.relative.cmp(&b.relative));
    let actual_paths: BTreeSet<String> = discovered.iter().map(|p| p.relative.clone()).collect();
    if actual_paths != expected_paths {
        let missing = expected_paths
            .difference(&actual_paths)
            .cloned()
            .collect::<Vec<_>>();
        let extra = actual_paths
            .difference(&expected_paths)
            .cloned()
            .collect::<Vec<_>>();
        if !extra.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} payload closure has extra paths: {}",
                extra.join(", ")
            )));
        }
        return Err(Error::Unverified(format!(
            "generated-full {side} seed-{seed} payload closure is incomplete; missing {} declared paths — FULL coverage is UNVERIFIED",
            missing.join(", ")
        )));
    }
    Ok(discovered)
}

fn read_dir_strict(path: &Path, side: &str, seed: u64) -> Result<Vec<fs::DirEntry>, Error> {
    fs::read_dir(path)
        .map_err(|e| {
            Error::Gate(format!(
                "generated-full {side} seed-{seed} cannot read {}: {e}",
                path.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            Error::Gate(format!(
                "generated-full {side} seed-{seed} cannot enumerate {}: {e}",
                path.display()
            ))
        })
}

fn expected_paths(contract: &GeneratedContract) -> BTreeSet<String> {
    contract
        .coordinates
        .iter()
        .map(|c| {
            let region = region_for(c.x, c.z);
            format!(
                "chunk/{}/{}/{}.{}.nbt",
                contract.dimension, region, c.x, c.z
            )
        })
        .collect()
}

fn parse_coordinate_filename(path: &Path) -> Result<(i32, i32), String> {
    let stem = path
        .file_stem()
        .ok_or_else(|| "missing filename stem".to_string())?
        .to_string_lossy();
    let (x, z) = stem
        .split_once('.')
        .ok_or_else(|| "expected <cx>.<cz>.nbt".to_string())?;
    if z.contains('.') {
        return Err("expected exactly one coordinate separator".to_string());
    }
    let cx = x.parse::<i32>().map_err(|e| format!("bad cx: {e}"))?;
    let cz = z.parse::<i32>().map_err(|e| format!("bad cz: {e}"))?;
    if x != cx.to_string() || z != cz.to_string() {
        return Err("coordinate components must use canonical decimal spelling (no +0, -0, or leading zeros)".into());
    }
    Ok((cx, cz))
}

fn region_for(cx: i32, cz: i32) -> String {
    format!("{}.{}", cx.div_euclid(32), cz.div_euclid(32))
}

fn java_seed_long(seed: u64) -> i64 {
    i64::from_ne_bytes(seed.to_ne_bytes())
}

/// Open one evidence file without following symlinks in any path component.
/// Linux `openat2(2)` resolves the complete path while holding the kernel's
/// no-symlink rule, then all validation and reads below use the returned fd.
/// This is deliberately a single acquisition operation: a prior
/// `symlink_metadata` check followed by `File::open(path)` would leave a rename
/// or link-substitution window.
fn open_stable_read(path: &Path) -> std::io::Result<fs::File> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{CWD, Mode, OFlags, ResolveFlags, openat2};

        openat2(
            CWD,
            path,
            OFlags::RDONLY | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map(fs::File::from)
        .map_err(std::io::Error::from)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "stable no-symlink evidence opening requires Linux openat2",
        ))
    }
}

fn is_symlink_open_error(error: &std::io::Error) -> bool {
    #[cfg(target_os = "linux")]
    {
        error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = error;
        false
    }
}

fn reject_hardlink_metadata(metadata: &fs::Metadata, path: &Path, what: &str) -> Result<(), Error> {
    #[cfg(unix)]
    if metadata.file_type().is_file() && std::os::unix::fs::MetadataExt::nlink(metadata) > 1 {
        return Err(Error::Gate(format!(
            "{what} {} is a hardlink (link count > 1); aliased payloads are forbidden",
            path.display()
        )));
    }
    Ok(())
}

fn open_stable_regular(path: &Path, what: &str) -> Result<fs::File, Error> {
    let file = match open_stable_read(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Unverified(format!(
                "{what} {} is absent",
                path.display()
            )));
        }
        Err(error) if is_symlink_open_error(&error) => {
            return Err(Error::Gate(format!(
                "{what} {} is a symlink or traverses a symlink component; symlink-aliased artifacts are forbidden",
                path.display()
            )));
        }
        Err(error) => {
            return Err(Error::Gate(format!(
                "{what} {} cannot be opened without following links: {error}",
                path.display()
            )));
        }
    };
    let metadata = file.metadata().map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be inspected through its opened descriptor: {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(Error::Gate(format!(
            "{what} {} is not a regular file; existing artifact is malformed",
            path.display()
        )));
    }
    reject_hardlink_metadata(&metadata, path, what)?;
    Ok(file)
}

/// Acquire, type-check, hardlink-check, and consume one regular evidence file
/// through the same opened descriptor.  Missing files retain the existing
/// UNVERIFIED classification; every present nonregular/link/error state is a
/// hard failure.
fn read_stable_file(path: &Path, what: &str) -> Result<Vec<u8>, Error> {
    let mut file = open_stable_regular(path, what)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be read through its opened descriptor: {error}",
            path.display()
        ))
    })?;
    Ok(bytes)
}

/// Read the Paper manifest from the already-opened jar descriptor.  The
/// verifier must not hash/validate a jar and then ask `unzip` to reopen its
/// pathname, because a rename or replacement in between would mix provenance
/// from two different files.  Linux's `/proc/self/fd` names the same inherited
/// descriptor; the file is intentionally opened without CLOEXEC for this
/// narrow child-process handoff.
fn read_jar_git_commit_opened(file: &fs::File) -> Result<Option<String>, Error> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let fd_path = format!("/proc/self/fd/{}", file.as_raw_fd());
        let output = Command::new("unzip")
            .args(["-p", &fd_path, "META-INF/MANIFEST.MF"])
            .output()
            .map_err(|error| {
                Error::Gate(format!(
                    "failed to run unzip through opened Paper jar descriptor: {error}"
                ))
            })?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter_map(|line| std::str::from_utf8(line).ok())
            .map(str::trim)
            .find_map(|line| {
                line.strip_prefix("Git-Commit:")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            }))
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        Err(Error::Gate(
            "reading Paper jar provenance from a stable descriptor is unsupported on this platform"
                .into(),
        ))
    }
}

/// Reject symlink aliases in every existing path component, not only when the
/// leaf itself is a link.  An artifact directory such as
/// `work/generated-full/artifacts -> /tmp/capture` would otherwise bypass the
/// leaf check while still escaping the pinned checkout.  The stable open above
/// is the authoritative check for files consumed by this verifier; this helper
/// remains for directory-closure diagnostics and non-file tree entries.
fn reject_symlink_components(path: &Path, what: &str) -> Result<(), Error> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Gate(format!(
                    "{what} {} traverses symlink component {}; symlink-aliased artifacts are forbidden",
                    path.display(),
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(Error::Gate(format!(
                    "{what} {} cannot inspect path component {}: {error}",
                    path.display(),
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path, what: &str) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|e| {
        Error::Gate(format!(
            "{what} {} cannot be inspected: {e}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Gate(format!(
            "{what} {} is a symlink; symlink-aliased payloads are forbidden",
            path.display()
        )));
    }
    Ok(())
}

fn reject_symlink_if_present(path: &Path, what: &str) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => reject_symlink(path, what),
        Ok(_) => reject_hardlink(path, what),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Gate(format!(
            "{what} {} cannot be inspected: {error}",
            path.display()
        ))),
    }
}

/// A payload tree must contain independent files.  A hardlink can make a
/// supposedly isolated Paper/Rivet side mutate the other side or make a
/// scratch tamper modify the source fixture, so link count is part of the
/// closure contract.  Unix is the supported oracle platform; other platforms
/// retain the symlink/type checks above.
fn reject_hardlink(path: &Path, what: &str) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|e| {
        Error::Gate(format!(
            "{what} {} cannot be inspected for hardlinks: {e}",
            path.display()
        ))
    })?;
    reject_hardlink_metadata(&metadata, path, what)
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_file())
        .unwrap_or(false)
}

fn read_executable_file(path: &Path) -> Result<Vec<u8>, Error> {
    let mut file = open_stable_regular(path, "generated-full Rivet producer binary")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = file.metadata().map_err(|error| {
            Error::Gate(format!(
                "Rivet producer binary {} cannot be inspected through its opened descriptor: {error}",
                path.display()
            ))
        })?;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(Error::Gate(format!(
                "Rivet producer binary {} is not executable; existing artifact is malformed",
                path.display()
            )));
        }
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        Error::Gate(format!(
            "Rivet producer binary {} cannot be read through its opened descriptor: {error}",
            path.display()
        ))
    })?;
    if bytes.is_empty() {
        return Err(Error::Gate(format!(
            "Rivet producer binary {} is empty; an existing artifact is malformed",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
fn require_executable(path: &Path) -> Result<(), Error> {
    read_executable_file(path).map(|_| ())
}

fn read_manifest(path: &Path) -> Result<HashManifest, Error> {
    let raw = read_stable_file(path, "generated-full manifest")?;
    let raw = std::str::from_utf8(&raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full manifest {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    serde_json::from_str(raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full manifest {} is malformed: {e}",
            path.display()
        ))
    })
}

fn same_producer_attestation(a: &GeneratedProvenance, b: &GeneratedProvenance) -> bool {
    a.paper_commit == b.paper_commit
        && a.materialized_jar_sha256 == b.materialized_jar_sha256
        && a.paper_config_template_sha256 == b.paper_config_template_sha256
        && a.rivet_commit == b.rivet_commit
        && a.capture_binary_sha256 == b.capture_binary_sha256
        && a.capture_config_sha256 == b.capture_config_sha256
        && a.level_type == b.level_type
        && a.dimension == b.dimension
        && a.region_file_compression == b.region_file_compression
        && a.status == b.status
        && a.hash_algorithm == b.hash_algorithm
        && a.hash_scope == b.hash_scope
        && a.corpus_version == b.corpus_version
        && a.stage == b.stage
        && a.regions == b.regions
        && a.coordinates == b.coordinates
        && a.chunk_concurrency == b.chunk_concurrency
        && a.normalization_rule == b.normalization_rule
        && a.paper_java_version == b.paper_java_version
        && a.paper_ticket_level == b.paper_ticket_level
        && a.paper_ticket_coordinates == b.paper_ticket_coordinates
        && a.paper_save_completion == b.paper_save_completion
}

fn compare_seed(
    contract: &GeneratedContract,
    seed: u64,
    paper: &VerifiedSide,
    rivet: &VerifiedSide,
) -> Result<(), Error> {
    if paper.provenance.seed_u64 != rivet.provenance.seed_u64
        || paper.provenance.seed_java_long != rivet.provenance.seed_java_long
        || paper.provenance.level_type != rivet.provenance.level_type
        || paper.provenance.dimension != rivet.provenance.dimension
        || paper.provenance.corpus_version != rivet.provenance.corpus_version
        || paper.provenance.paper_commit != rivet.provenance.paper_commit
        || paper.provenance.materialized_jar_sha256 != rivet.provenance.materialized_jar_sha256
        || paper.provenance.paper_config_sha256 != rivet.provenance.paper_config_sha256
        || paper.provenance.paper_config_template_sha256
            != rivet.provenance.paper_config_template_sha256
        || paper.provenance.rivet_commit != rivet.provenance.rivet_commit
        || paper.provenance.capture_binary_sha256 != rivet.provenance.capture_binary_sha256
        || paper.provenance.capture_config_sha256 != rivet.provenance.capture_config_sha256
        || paper.provenance.seed_config_sha256 != rivet.provenance.seed_config_sha256
        || paper.provenance.paper_java_version != rivet.provenance.paper_java_version
        || paper.provenance.paper_ticket_level != rivet.provenance.paper_ticket_level
        || paper.provenance.paper_ticket_coordinates != rivet.provenance.paper_ticket_coordinates
        || paper.provenance.paper_source_payload_count
            != rivet.provenance.paper_source_payload_count
        || paper.provenance.paper_source_full_count != rivet.provenance.paper_source_full_count
        || paper.provenance.paper_boot_log_sha256 != rivet.provenance.paper_boot_log_sha256
        || paper.provenance.paper_save_completion != rivet.provenance.paper_save_completion
        || paper.provenance.paper_twin_run_sha256 != rivet.provenance.paper_twin_run_sha256
    {
        return Err(Error::Gate(format!(
            "generated-full seed {seed} Paper/Rivet provenance differs — stale or fake cross-side artifact identity",
        )));
    }
    let mismatches = manifest_mismatches(contract, seed, &paper.manifest, &rivet.manifest);
    if mismatches.is_empty() {
        println!(
            "  seed {seed}: {} FULL overworld payloads match byte-for-byte",
            contract.coordinates.len()
        );
        return Ok(());
    }
    let mut details = Vec::new();
    for mismatch in &mismatches {
        let triage = if mismatch.order_only {
            " (canonical-identical; NBT order only)"
        } else {
            ""
        };
        details.push(format!(
            "seed={} overworld/{}.{}: expected {} got {}{}",
            mismatch.seed,
            mismatch.coordinate.0,
            mismatch.coordinate.1,
            mismatch.expected,
            mismatch.actual,
            triage
        ));
    }
    Err(Error::Gate(format!(
        "generated-full parity FAIL: {} divergent payloads; every named mismatch follows:\n{}",
        mismatches.len(),
        details.join("\n")
    )))
}

fn manifest_mismatches(
    contract: &GeneratedContract,
    seed: u64,
    paper: &HashManifest,
    rivet: &HashManifest,
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    for coord in &contract.coordinates {
        let Some(pe) = paper.full_entry(&contract.dimension, coord.x, coord.z) else {
            continue;
        };
        let Some(re) = rivet.full_entry(&contract.dimension, coord.x, coord.z) else {
            continue;
        };
        if pe.xxh3_64 != re.xxh3_64 || pe.sha256 != re.sha256 {
            mismatches.push(Mismatch {
                seed,
                coordinate: coord.tuple(),
                expected: format!("xxh3={} sha256={}", pe.xxh3_64, pe.sha256),
                actual: format!("xxh3={} sha256={}", re.xxh3_64, re.sha256),
                order_only: pe.xxh3_64_canonical == re.xxh3_64_canonical,
            });
        }
    }
    mismatches
}

pub fn tamper_negative_default(selected: Option<TamperKind>) -> Result<(), Error> {
    let fixture_root = crate::crate_dir().join("fixtures/generated-full");
    let contract = load_contract(&fixture_root.join(CONTRACT_BASENAME))?;
    let artifacts = ArtifactInputs::from_contract(&contract)?;
    let identity = artifacts.identity()?;
    let paper_root = fixture_root.join("paper");
    let rivet_root = crate::crate_dir().join("work/generated-full/rivet");
    tamper_negative_roots_with_identity(
        &contract,
        &paper_root,
        &rivet_root,
        &identity,
        false,
        selected,
    )
}

#[cfg(test)]
fn tamper_negative_roots(
    contract: &GeneratedContract,
    paper_root: &Path,
    rivet_root: &Path,
    selected: Option<TamperKind>,
) -> Result<(), Error> {
    let identity = ArtifactIdentity {
        paper_commit: hash_manifest::PAPER_PIN.to_string(),
        materialized_jar_sha256: "0".repeat(64),
        paper_config_template_sha256: "2".repeat(64),
        paper_config_template: Vec::new(),
        rivet_commit: "test-rivet-commit".to_string(),
        capture_binary_sha256: "1".repeat(64),
        capture_config_sha256: "3".repeat(64),
    };
    tamper_negative_roots_with_identity(contract, paper_root, rivet_root, &identity, true, selected)
}

fn tamper_negative_roots_with_identity(
    contract: &GeneratedContract,
    paper_root: &Path,
    rivet_root: &Path,
    identity: &ArtifactIdentity,
    allow_synthetic: bool,
    selected: Option<TamperKind>,
) -> Result<(), Error> {
    validate_contract(contract)?;
    let kinds = selected.map_or_else(|| TamperKind::ALL.to_vec(), |kind| vec![kind]);
    let seed = *contract
        .seeds
        .first()
        .ok_or_else(|| Error::Gate("generated-full contract has no seeds".to_string()))?;
    let paper = verify_side(
        contract,
        &paper_root.join(seed.to_string()),
        seed,
        "paper",
        identity,
        allow_synthetic,
    )?;
    let original_rivet_root = rivet_root.join(seed.to_string());
    verify_side(
        contract,
        &original_rivet_root,
        seed,
        "rivet",
        identity,
        allow_synthetic,
    )?;
    let target = contract
        .coordinates
        .first()
        .ok_or_else(|| Error::Gate("generated-full contract has no coordinates".to_string()))?;
    let target_path = original_rivet_root.join(expected_payload_path(contract, target));

    for kind in kinds {
        let scratch = tempfile::tempdir().map_err(|e| {
            Error::Gate(format!(
                "generated-full tamper scratch directory failed: {e}"
            ))
        })?;
        let scratch_side = scratch.path().join(seed.to_string());
        crate::copy_dir_recursive(&original_rivet_root, &scratch_side)?;
        let scratch_target = scratch_side.join(expected_payload_path(contract, target));
        let bytes = fs::read(&scratch_target).map_err(|e| {
            Error::Gate(format!(
                "generated-full tamper target {} is unreadable: {e}",
                target_path.display()
            ))
        })?;
        let tampered = mutate::tamper(&bytes, kind).map_err(Error::Gate)?;
        fs::write(&scratch_target, tampered).map_err(|e| {
            Error::Gate(format!(
                "generated-full tamper target {} cannot be written: {e}",
                scratch_target.display()
            ))
        })?;
        rebuild_manifest(contract, seed, &scratch_side)?;
        if kind == TamperKind::LastUpdate {
            let error = verify_side(
                contract,
                &scratch_side,
                seed,
                "rivet",
                identity,
                allow_synthetic,
            )
            .expect_err("nonzero LastUpdate must be rejected by the payload validator");
            let message = error.to_string();
            if !message.contains("LastUpdate") {
                return Err(Error::NegativeControl {
                    message: format!(
                        "generated-full {} tamper was not rejected by the LastUpdate normalization rule",
                        kind.cli_name()
                    ),
                });
            }
            println!(
                "tamper control PASS: {} changed only seed={} overworld/{}.{}, then was rejected by LastUpdate=0",
                kind.cli_name(),
                seed,
                target.x,
                target.z
            );
            continue;
        }
        let tampered_rivet = verify_side(
            contract,
            &scratch_side,
            seed,
            "rivet",
            identity,
            allow_synthetic,
        )?;
        let mismatches =
            manifest_mismatches(contract, seed, &paper.manifest, &tampered_rivet.manifest);
        if mismatches.len() != 1
            || mismatches[0].coordinate != target.tuple()
            || !mismatches[0].expected.starts_with("xxh3=")
        {
            return Err(Error::NegativeControl {
                message: format!(
                    "generated-full {} tamper was not the only named mismatch at overworld/{}.{}",
                    kind.cli_name(),
                    target.x,
                    target.z
                ),
            });
        }
        println!(
            "tamper control PASS: {} changed only seed={} overworld/{}.{}",
            kind.cli_name(),
            seed,
            target.x,
            target.z
        );
    }
    Ok(())
}

fn expected_payload_path(contract: &GeneratedContract, coord: &Coordinate) -> PathBuf {
    PathBuf::from(format!(
        "chunk/{}/{}/{}.{}.nbt",
        contract.dimension,
        region_for(coord.x, coord.z),
        coord.x,
        coord.z
    ))
}

fn rebuild_manifest(
    contract: &GeneratedContract,
    seed: u64,
    side_root: &Path,
) -> Result<(), Error> {
    let capture = CaptureProvenance {
        level_type: contract.level_type.clone(),
        region_file_compression: contract.region_file_compression.clone(),
        corpus_version: contract.corpus_version.clone(),
    };
    let manifest = hash_manifest::build_from_payloads_with(
        side_root,
        &seed.to_string(),
        &contract.level_type,
        &capture,
    )
    .map_err(Error::Gate)?;
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Gate(format!("serializing generated-full manifest: {e}")))?;
    fs::write(side_root.join(MANIFEST_BASENAME), format!("{json}\n"))?;
    Ok(())
}

/// `generated-full` CLI.  With no options it verifies the committed Paper
/// baseline against G4's work-tree output.  `--tamper [kind]` runs one control;
/// omitting the kind runs all six controls.
pub fn run_cli(args: &[&str]) -> Result<(), Error> {
    let mut contract_path = None;
    let mut paper_root = None;
    let mut rivet_root = None;
    let mut tamper = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--contract" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::Gate(
                        "generated-full --contract requires a path".into(),
                    ));
                };
                contract_path = Some(PathBuf::from(path));
                i += 2;
            }
            "--paper" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::Gate("generated-full --paper requires a path".into()));
                };
                paper_root = Some(PathBuf::from(path));
                i += 2;
            }
            "--rivet" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::Gate("generated-full --rivet requires a path".into()));
                };
                rivet_root = Some(PathBuf::from(path));
                i += 2;
            }
            "--tamper" | "--expect-fail" => {
                if tamper.is_some() {
                    return Err(Error::Gate(
                        "generated-full accepts only one --tamper/--expect-fail option".into(),
                    ));
                }
                let Some(kind) = args.get(i + 1) else {
                    tamper = Some(None);
                    i += 1;
                    continue;
                };
                if kind.starts_with('-') {
                    tamper = Some(None);
                    i += 1;
                } else if *kind == "all" {
                    tamper = Some(None);
                    i += 2;
                } else {
                    tamper = Some(Some(TamperKind::from_cli(kind).ok_or_else(|| {
                        Error::Gate(format!("generated-full unknown tamper kind {kind}"))
                    })?));
                    i += 2;
                }
            }
            other => {
                return Err(Error::Gate(format!(
                    "generated-full unknown option or positional argument {other}"
                )));
            }
        }
    }
    let fixture_root = crate::crate_dir().join("fixtures/generated-full");
    let contract_path = contract_path.unwrap_or_else(|| fixture_root.join(CONTRACT_BASENAME));
    let contract = load_contract(&contract_path)?;
    let paper_root = paper_root.unwrap_or_else(|| fixture_root.join("paper"));
    let rivet_root =
        rivet_root.unwrap_or_else(|| crate::crate_dir().join("work/generated-full/rivet"));
    if tamper.is_some() {
        let artifacts = ArtifactInputs::from_contract(&contract)?;
        let identity = artifacts.identity()?;
        tamper_negative_roots_with_identity(
            &contract,
            &paper_root,
            &rivet_root,
            &identity,
            false,
            tamper.flatten(),
        )
    } else {
        verify_roots(&contract, &paper_root, &rivet_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_contract() -> GeneratedContract {
        canonical_contract()
    }

    fn write_provenance(root: &Path, provenance: &GeneratedProvenance) {
        fs::write(
            root.join(PROVENANCE_BASENAME),
            format!("{}\n", serde_json::to_string_pretty(provenance).unwrap()),
        )
        .unwrap();
    }

    fn write_seed_set(root: &Path, contract: &GeneratedContract, side: &str, payload_seed: i64) {
        for &seed in &contract.seeds {
            write_tree(
                &root.join(seed.to_string()),
                contract,
                side,
                seed,
                payload_seed,
            );
        }
    }

    fn write_tree(
        root: &Path,
        contract: &GeneratedContract,
        side: &str,
        seed: u64,
        payload_seed: i64,
    ) {
        fs::create_dir_all(root).unwrap();
        let mut provenance = GeneratedProvenance::for_test(contract, side, seed);
        let seed_config = serde_json::json!({
            "seed-u64": seed,
            "seed-java-long": java_seed_long(seed),
            "level-type": contract.level_type,
            "dimension": contract.dimension,
            "region-file-compression": contract.region_file_compression,
            "status": contract.status,
            "stage": contract.stage,
        });
        fs::write(
            root.join(SEED_CONFIG_BASENAME),
            format!("{}\n", serde_json::to_string_pretty(&seed_config).unwrap()),
        )
        .unwrap();
        let seed_config_bytes = fs::read(root.join(SEED_CONFIG_BASENAME)).unwrap();
        provenance.seed_config_sha256 = crate::sha256_hex(&seed_config_bytes);
        write_provenance(root, &provenance);
        for coordinate in &contract.coordinates {
            let region = region_for(coordinate.x, coordinate.z);
            let dir = root.join("chunk").join(EXPECTED_DIMENSION).join(region);
            fs::create_dir_all(&dir).unwrap();
            let payload =
                mutate::fixture_full_payload_with_seed(coordinate.x, coordinate.z, payload_seed);
            fs::write(
                dir.join(format!("{}.{}.nbt", coordinate.x, coordinate.z)),
                payload,
            )
            .unwrap();
        }
        rebuild_manifest(contract, seed, root).unwrap();
    }

    #[test]
    fn canonical_contract_covers_all_origin_adjacent_regions_and_seams() {
        let contract = test_contract();
        assert_eq!(contract.regions, vec!["-1.-1", "-1.0", "0.-1", "0.0"]);
        assert!(contract.coordinates.iter().any(|c| c.tuple() == (-1, 0)));
        assert!(contract.coordinates.iter().any(|c| c.tuple() == (0, -1)));
        assert!(java_seed_long(contract.seeds[1]) < 0);
        validate_contract(&contract).unwrap();
    }

    #[test]
    fn strict_metadata_schema_rejects_unknown_fields() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();

        let mut contract_json = serde_json::to_value(&contract).unwrap();
        contract_json
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        let contract_path = temp.path().join(CONTRACT_BASENAME);
        fs::write(&contract_path, format!("{}\n", contract_json)).unwrap();
        assert!(matches!(
            load_contract(&contract_path),
            Err(Error::Gate(message)) if message.contains("unknown field")
        ));

        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let seed_root = rivet_root.join(contract.seeds[0].to_string());

        let provenance_path = seed_root.join(PROVENANCE_BASENAME);
        let mut provenance = serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&provenance_path).unwrap(),
        )
        .unwrap();
        provenance
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        fs::write(&provenance_path, format!("{}\n", provenance)).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("provenance is malformed")
        ));

        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let seed_config_path = seed_root.join(SEED_CONFIG_BASENAME);
        let mut seed_config = serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&seed_config_path).unwrap(),
        )
        .unwrap();
        seed_config
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        fs::write(&seed_config_path, format!("{}\n", seed_config)).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("config is malformed")
        ));

        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let manifest_path = seed_root.join(MANIFEST_BASENAME);
        let mut manifest =
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&manifest_path).unwrap())
                .unwrap();
        manifest
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        fs::write(&manifest_path, format!("{}\n", manifest)).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("manifest") && message.contains("unknown field")
        ));
    }

    #[test]
    fn absent_contract_is_unverified_but_existing_nonregular_contract_fails() {
        let temp = tempfile::tempdir().unwrap();
        let contract_path = temp.path().join(CONTRACT_BASENAME);
        assert!(matches!(
            load_contract(&contract_path),
            Err(Error::Unverified(message)) if message.contains("contract") && message.contains("absent")
        ));

        fs::create_dir(&contract_path).unwrap();
        assert!(matches!(
            load_contract(&contract_path),
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));

        fs::remove_dir(&contract_path).unwrap();
        let target = temp.path().join("contract-target.json");
        let contract = serde_json::to_vec(&test_contract()).unwrap();
        fs::write(&target, contract).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &contract_path).unwrap();
            assert!(matches!(
                load_contract(&contract_path),
                Err(Error::Gate(message)) if message.contains("symlink")
            ));

            fs::remove_file(&contract_path).unwrap();
            fs::hard_link(&target, &contract_path).unwrap();
            assert!(matches!(
                load_contract(&contract_path),
                Err(Error::Gate(message)) if message.contains("hardlink")
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn absent_rivet_binary_is_unverified_but_existing_binary_failures_are_hard() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("rivet-capture");
        assert!(matches!(
            require_executable(&binary),
            Err(Error::Unverified(message)) if message.contains("binary") && message.contains("absent")
        ));

        fs::create_dir(&binary).unwrap();
        assert!(matches!(
            require_executable(&binary),
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));

        fs::remove_dir(&binary).unwrap();
        fs::write(&binary, b"not executable").unwrap();
        assert!(matches!(
            require_executable(&binary),
            Err(Error::Gate(message)) if message.contains("not executable")
        ));

        let target = temp.path().join("rivet-capture-target");
        fs::write(&target, b"executable evidence").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&target).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&target, permissions).unwrap();
        }
        fs::remove_file(&binary).unwrap();
        std::os::unix::fs::symlink(&target, &binary).unwrap();
        assert!(matches!(
            require_executable(&binary),
            Err(Error::Gate(message)) if message.contains("symlink")
        ));

        fs::remove_file(&binary).unwrap();
        fs::hard_link(&target, &binary).unwrap();
        assert!(matches!(
            require_executable(&binary),
            Err(Error::Gate(message)) if message.contains("hardlink")
        ));
    }

    #[test]
    fn absent_roots_are_unverified_but_existing_non_directories_fail() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Unverified(message)) if message.contains("root")
        ));

        fs::write(&paper_root, b"not a directory").unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("not a directory")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_roots_and_hardlinked_payloads_fail_closed() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        std::os::unix::fs::symlink(temp.path().join("missing"), &paper_root).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("symlink")
        ));

        fs::remove_file(&paper_root).unwrap();
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let seed = contract.seeds[0];
        let paper_payload = paper_root
            .join(seed.to_string())
            .join("chunk/overworld/0.0/0.0.nbt");
        let rivet_payload = rivet_root
            .join(seed.to_string())
            .join("chunk/overworld/0.0/0.0.nbt");
        fs::remove_file(&rivet_payload).unwrap();
        fs::hard_link(&paper_payload, &rivet_payload).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("hardlink")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn nested_payload_symlink_is_a_hard_failure() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);

        let seed = contract.seeds[0];
        let paper_payload = paper_root
            .join(seed.to_string())
            .join("chunk/overworld/0.0/0.0.nbt");
        let rivet_payload = rivet_root
            .join(seed.to_string())
            .join("chunk/overworld/0.0/0.0.nbt");
        fs::remove_file(&rivet_payload).unwrap();
        std::os::unix::fs::symlink(&paper_payload, &rivet_payload).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("symlink")
        ));
    }

    #[test]
    fn stale_wrong_seed_provenance_is_a_hard_failure() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let seed = contract.seeds[0];
        let rivet = temp.path().join("rivet").join(seed.to_string());
        write_seed_set(&temp.path().join("paper"), &contract, "paper", 42);
        write_seed_set(&temp.path().join("rivet"), &contract, "rivet", 42);
        let mut provenance: GeneratedProvenance =
            serde_json::from_str(&fs::read_to_string(rivet.join(PROVENANCE_BASENAME)).unwrap())
                .unwrap();
        provenance.seed_u64 ^= 1;
        provenance.seed_java_long = java_seed_long(provenance.seed_u64);
        write_provenance(&rivet, &provenance);
        let result = verify_synthetic_roots(
            &contract,
            temp.path().join("paper").as_path(),
            temp.path().join("rivet").as_path(),
        );
        assert!(matches!(result, Err(Error::Gate(_))));
    }

    #[test]
    fn lying_wrong_seed_payloads_name_every_expected_chunk() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        write_seed_set(&temp.path().join("paper"), &contract, "paper", 42);
        write_seed_set(&temp.path().join("rivet"), &contract, "rivet", 43);
        let result = verify_synthetic_roots(
            &contract,
            temp.path().join("paper").as_path(),
            temp.path().join("rivet").as_path(),
        );
        let error = result.expect_err("wrong-seed payload must fail");
        let Error::Gate(message) = error else {
            panic!("expected hard parity failure");
        };
        for coordinate in &contract.coordinates {
            assert!(
                message.contains(&format!("overworld/{}.{}", coordinate.x, coordinate.z)),
                "missing {}.{} in {message}",
                coordinate.x,
                coordinate.z
            );
        }
    }

    #[test]
    fn every_tamper_kind_is_exactly_one_named_mismatch() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        for kind in TamperKind::ALL {
            tamper_negative_roots(&contract, &paper_root, &rivet_root, Some(kind)).unwrap();
        }
    }

    #[test]
    fn self_diff_alias_is_a_hard_failure() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let shared_root = temp.path().join("shared");
        write_seed_set(&shared_root, &contract, "paper", 42);
        let result = verify_synthetic_roots(&contract, &shared_root, &shared_root);
        assert!(matches!(
            result,
            Err(Error::Gate(message)) if message.contains("self-diff")
        ));
    }

    #[test]
    fn missing_overworld_and_noncanonical_filenames_fail_with_distinct_verdicts() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let seed_root = rivet_root.join(contract.seeds[0].to_string());
        fs::remove_dir_all(seed_root.join("chunk/overworld")).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Unverified(message)) if message.contains("missing declared dimension")
        ));

        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let canonical = seed_root.join("chunk/overworld/0.0/0.0.nbt");
        let noncanonical = seed_root.join("chunk/overworld/0.0/+0.0.nbt");
        fs::rename(canonical, noncanonical).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &paper_root, &rivet_root),
            Err(Error::Gate(message)) if message.contains("canonical decimal spelling")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_symlink_aliases_are_rejected() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let real_parent = temp.path().join("real-parent");
        let real_paper = real_parent.join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&real_paper, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let alias_parent = temp.path().join("alias-parent");
        std::os::unix::fs::symlink(&real_parent, &alias_parent).unwrap();
        assert!(matches!(
            verify_synthetic_roots(&contract, &alias_parent.join("paper"), &rivet_root),
            Err(Error::Gate(message)) if message.contains("symlink component")
        ));
    }

    #[test]
    fn generated_full_outer_closure_requires_canonical_properties_file() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("generated-full");
        fs::create_dir_all(root.join("paper")).unwrap();
        fs::write(
            root.join(CONTRACT_BASENAME),
            format!("{}\n", serde_json::to_string_pretty(&contract).unwrap()),
        )
        .unwrap();
        let template =
            crate::crate_dir().join("fixtures/generated-full/server-normal-full.properties");
        fs::copy(&template, root.join("server-normal-full.properties")).unwrap();
        verify_fixture_outer_closure(&root).unwrap();

        fs::remove_file(root.join("server-normal-full.properties")).unwrap();
        assert!(matches!(
            verify_fixture_outer_closure(&root),
            Err(Error::Unverified(message)) if message.contains("server-normal-full.properties")
        ));

        fs::create_dir(root.join("server-normal-full.properties")).unwrap();
        assert!(matches!(
            verify_fixture_outer_closure(&root),
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));
    }

    #[test]
    fn malformed_extra_and_symlink_payloads_are_hard_failures() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let seed = contract.seeds[0];
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);
        let extra = rivet_root
            .join(seed.to_string())
            .join("chunk/overworld/0.0/99.99.nbt");
        fs::write(&extra, b"not nbt").unwrap();
        let result = verify_synthetic_roots(&contract, paper_root.as_path(), rivet_root.as_path());
        assert!(matches!(result, Err(Error::Gate(_))));
    }

    #[test]
    fn identical_worldgen_content_is_rejected_as_superflat_echo() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);

        let seed = contract.seeds[0];
        let seed_root = rivet_root.join(seed.to_string());
        let template = mutate::parse_payload(
            &fs::read(seed_root.join("chunk/overworld/0.0/0.0.nbt")).unwrap(),
        )
        .unwrap();
        for (index, coordinate) in contract.coordinates.iter().enumerate() {
            let path = seed_root.join(expected_payload_path(&contract, coordinate));
            let mut payload = template.clone();
            payload.put_int("xPos", coordinate.x);
            payload.put_int("zPos", coordinate.z);
            let bytes = mutate::encode_payload(&payload).unwrap();
            let bytes = if index % 2 == 0 {
                mutate::tamper(&bytes, TamperKind::NbtOrder).unwrap()
            } else {
                bytes
            };
            fs::write(&path, bytes).unwrap();
        }
        rebuild_manifest(&contract, seed, &seed_root).unwrap();

        let result = verify_synthetic_roots(&contract, &paper_root, &rivet_root);
        assert!(matches!(
            result,
            Err(Error::Gate(message)) if message.contains("superflat echo")
        ));
    }

    #[test]
    fn payload_parser_rejects_trailing_bytes_in_self_test_fixture() {
        let payload = mutate::fixture_full_payload(0, 0);
        let mut out = Vec::new();
        out.write_all(&payload).unwrap();
        out.push(0);
        assert!(mutate::parse_payload(&out).is_err());
    }
}
