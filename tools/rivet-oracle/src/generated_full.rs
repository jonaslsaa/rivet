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
//!
//! Stable evidence acquisition is intentionally Linux-only: it uses `openat2`
//! with `RESOLVE_NO_SYMLINKS`, plus `/proc/self/fd` for the opened Paper jar.
//! There is no insecure portable-Unix fallback; non-Linux callers receive an
//! explicit unsupported-platform failure. Linux x86_64 is the primary tested
//! target, while other Linux architectures use the same kernel contract.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rivet_harness_common::server::{self as harness_server, ChildServer};
use rivet_nbt::compound_tag::CompoundTag;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::corpus;
#[cfg(test)]
use crate::hash;
use crate::hash_manifest::{self, CaptureProvenance, HashManifest};
use crate::mutate::{self, TamperKind};
use crate::{
    FORCED_TICKET_LEVEL, KIND_FULL, OVERWORLD_DIM, extract_fresh_fixtures, inject_forced_tickets,
    normalize_last_update_tree, parse_boot_thread_counts, prepare_run_dir, rehash_captured,
    verify_forced_load,
};

pub const KIND: &str = "generated-full";
pub const CONTRACT_BASENAME: &str = "contract.json";
#[cfg(test)]
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
#[cfg(test)]
const EXPECTED_PAPER_TICKET_LEVEL: u32 = 33;
#[cfg(test)]
const EXPECTED_PAPER_SOURCE_PAYLOADS: usize = 2764;
#[cfg(test)]
const EXPECTED_PAPER_SOURCE_FULL: usize = 10;
#[cfg(test)]
const EXPECTED_PAPER_SAVE_COMPLETION: &str =
    "SIGTERM+All dimensions are saved+RegionFile-I/O shutdown/read-back";
const MAX_EVIDENCE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PAYLOAD_FILE_BYTES: u64 = hash_manifest::MAX_PAYLOAD_BYTES as u64;
const MAX_EVIDENCE_ENTRIES: usize = hash_manifest::MAX_PAYLOAD_COUNT;

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
pub struct SharedContract {
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

/// Internal name retained only for the parser/test surface. Production replay
/// takes a [`SharedContract`] and never accepts a caller-supplied evidence root.
pub type GeneratedContract = SharedContract;

/// A payload digest derived by the verifier from immutable bytes. Producer
/// manifests may describe these values for diagnostics, but never supply them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawPayloadDigest {
    pub dimension: String,
    pub region: String,
    pub x: i32,
    pub z: i32,
    pub bytes: usize,
    pub xxh3_64: String,
    pub sha256: String,
}

/// Controller-observed Paper evidence. This schema intentionally contains
/// Paper-only identity and lifecycle fields; no Rivet identity is present to
/// compare or accidentally bless as equal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PaperObserved {
    pub schema: String,
    pub seed: u64,
    pub root: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(rename = "paper-jar-sha256")]
    pub paper_jar_sha256: String,
    #[serde(rename = "paper-config-sha256")]
    pub paper_config_sha256: String,
    #[serde(rename = "paper-revision-identity-sha256")]
    pub paper_revision_identity_sha256: String,
    pub pid: u32,
    #[serde(rename = "started-unix-nanos")]
    pub started_unix_nanos: u128,
    #[serde(rename = "ready-count")]
    pub ready_count: u32,
    #[serde(rename = "stopped-unix-nanos")]
    pub stopped_unix_nanos: u128,
    #[serde(rename = "exit-code")]
    pub exit_code: i32,
    #[serde(rename = "raw-log-sha256")]
    pub raw_log_sha256: String,
    #[serde(rename = "payload-digests")]
    pub payload_digests: Vec<RawPayloadDigest>,
    #[serde(rename = "producer-manifest-sha256")]
    pub producer_manifest_sha256: Option<String>,
}

/// Controller-observed Rivet evidence. This schema is source-disjoint from
/// [`PaperObserved`], so a producer cannot claim Paper's identity fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RivetObserved {
    pub schema: String,
    pub seed: u64,
    pub root: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(rename = "rivet-executable-sha256")]
    pub rivet_executable_sha256: String,
    #[serde(rename = "rivet-config-sha256")]
    pub rivet_config_sha256: String,
    #[serde(rename = "rivet-revision-identity-sha256")]
    pub rivet_revision_identity_sha256: String,
    pub pid: u32,
    #[serde(rename = "started-unix-nanos")]
    pub started_unix_nanos: u128,
    #[serde(rename = "ready-count")]
    pub ready_count: u32,
    #[serde(rename = "stopped-unix-nanos")]
    pub stopped_unix_nanos: u128,
    #[serde(rename = "exit-code")]
    pub exit_code: i32,
    #[serde(rename = "raw-log-sha256")]
    pub raw_log_sha256: String,
    #[serde(rename = "payload-digests")]
    pub payload_digests: Vec<RawPayloadDigest>,
    #[serde(rename = "producer-manifest-sha256")]
    pub producer_manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplayRecord {
    pub schema: String,
    pub nonce: String,
    #[serde(rename = "controller-root")]
    pub controller_root: String,
    #[serde(rename = "paper-root")]
    pub paper_root: String,
    #[serde(rename = "rivet-root")]
    pub rivet_root: String,
    pub lifecycle: String,
    #[serde(rename = "paper-observed")]
    pub paper_observed: Vec<PaperObserved>,
    #[serde(rename = "rivet-observed")]
    pub rivet_observed: Vec<RivetObserved>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LatestReplay {
    schema: String,
    nonce: String,
    #[serde(rename = "replay-root")]
    replay_root: String,
}

#[derive(Debug, Clone)]
struct ReplaySnapshotEvidence {
    root: PathBuf,
    manifest: HashManifest,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
#[derive(Debug, Clone)]
struct VerifiedSide {
    #[cfg_attr(not(test), allow(dead_code))]
    provenance: GeneratedProvenance,
    manifest: HashManifest,
}

#[derive(Debug, Clone)]
struct Mismatch {
    #[cfg_attr(not(test), allow(dead_code))]
    seed: u64,
    coordinate: (i32, i32),
    expected: String,
    actual: String,
    #[cfg_attr(not(test), allow(dead_code))]
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
            rivet_producer_binary: "work/generated-full/artifacts/rivet-generated-full".to_string(),
            rivet_capture_config: "work/generated-full/artifacts/rivet-generated-full-config.json"
                .to_string(),
        },
    }
}

/// Load and structurally validate the generated-FULL contract.
pub fn load_contract(path: &Path) -> Result<GeneratedContract, Error> {
    let raw_bytes = read_stable_file_capped(
        path,
        "generated-full contract",
        crate::json::MAX_JSON_BYTES as u64,
    )?;
    std::str::from_utf8(&raw_bytes).map_err(|e| {
        Error::Gate(format!(
            "generated-full contract {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    let contract: GeneratedContract = crate::json::from_slice(&raw_bytes).map_err(|e| {
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

#[derive(Debug)]
struct StagedFile {
    path: PathBuf,
    /// Descriptor held open on the staged file for the whole replay: a later
    /// path swap cannot redirect any consumer away from attested contents.
    #[allow(dead_code)]
    file: fs::File,
    /// Contents read exactly once through the stable descriptor at staging
    /// time; later consumers never reopen anything by pathname.
    #[allow(dead_code)]
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct StagedArtifacts {
    paper_jar: StagedFile,
    paper_properties: StagedFile,
    rivet_binary: StagedFile,
    rivet_config: StagedFile,
    identity: ArtifactIdentity,
}

const REPLAY_SCHEMA: &str = "generated-full-replay-v1";
const LATEST_REPLAY_SCHEMA: &str = "generated-full-latest-replay-v1";
const LATEST_REPLAY_BASENAME: &str = "latest-replay.json";
const PAPER_READY_TIMEOUT: Duration = Duration::from_secs(180);
const PRODUCER_READY_TIMEOUT: Duration = Duration::from_secs(30);
const PROCESS_POLL: Duration = Duration::from_millis(100);
const PAPER_READY_MARKER: &str = "Done (";
const RIVET_READY_MARKER: &str = "RIVET_GENERATED_FULL_READY";
const PAPER_CLEAN_SAVE_MARKER: &str = "All dimensions are saved";

#[derive(Debug, Clone, Copy)]
enum ExpectedLifecycle {
    PaperClean,
    ProducerExit(i32),
}

fn process_exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .or_else(|| status.signal().map(|signal| 128 + signal))
            .unwrap_or(-1)
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(-1)
    }
}

fn validate_expected_lifecycle(
    pid: u32,
    exit_code: i32,
    log_bytes: &[u8],
    ready_offset: usize,
    expected_lifecycle: ExpectedLifecycle,
) -> Result<(), Error> {
    match expected_lifecycle {
        ExpectedLifecycle::PaperClean => {
            // Paper's clean shutdown is SIGTERM-driven. On Unix a direct
            // child reports signal 15 (rendered as the conventional 143),
            // while wrappers may return 143 and some Paper builds return 0.
            // The save marker is the authoritative lifecycle witness; an
            // arbitrary successful-looking producer exit is never accepted.
            if !matches!(exit_code, 0 | 143) {
                return Err(Error::Gate(format!(
                    "Paper process {pid} exited with code {exit_code}; clean SIGTERM lifecycle requires exit 0 or 143"
                )));
            }
            let post_ready = String::from_utf8_lossy(
                log_bytes
                    .get(ready_offset.min(log_bytes.len())..)
                    .unwrap_or(&[]),
            );
            if !post_ready.contains(PAPER_CLEAN_SAVE_MARKER) {
                return Err(Error::Gate(format!(
                    "Paper process {pid} exited {exit_code} without the clean-save marker {PAPER_CLEAN_SAVE_MARKER:?} after READY"
                )));
            }
        }
        ExpectedLifecycle::ProducerExit(expected) => {
            if exit_code == 4
                && let Some(message) = String::from_utf8_lossy(log_bytes).lines().find_map(|line| {
                    line.strip_prefix("RIVET_GENERATED_FULL_BLOCKED:")
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                        .map(str::to_string)
                })
            {
                return Err(Error::Blocked(message));
            }
            if exit_code != expected {
                return Err(Error::Gate(format!(
                    "generated-full producer {pid} exited with code {exit_code}; expected dedicated exit {expected}"
                )));
            }
        }
    }
    Ok(())
}

fn now_unix_nanos() -> Result<u128, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| Error::Gate(format!("system clock is before UNIX_EPOCH: {error}")))
}

fn controller_environment() -> std::collections::BTreeMap<String, String> {
    ["PATH", "JAVA_HOME", "HOME", "LANG", "LC_ALL", "TZ"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

fn apply_controller_environment(
    command: &mut Command,
) -> std::collections::BTreeMap<String, String> {
    let environment = controller_environment();
    command.env_clear();
    command.envs(&environment);
    environment
}

#[derive(Debug)]
struct ProcessObservation {
    pid: u32,
    started_unix_nanos: u128,
    stopped_unix_nanos: u128,
    exit_code: i32,
    ready_count: u32,
    raw_log_sha256: String,
    argv: Vec<String>,
    cwd: String,
    env: std::collections::BTreeMap<String, String>,
}

fn process_observation(
    command: &mut Command,
    argv: Vec<String>,
    cwd: &Path,
    log_path: &Path,
    ready_marker: &str,
    timeout: Duration,
    expected_lifecycle: ExpectedLifecycle,
) -> Result<ProcessObservation, Error> {
    if log_path.exists() {
        return Err(Error::Gate(format!(
            "generated-full process log {} already exists; launched evidence is not fresh",
            log_path.display()
        )));
    }
    let env = apply_controller_environment(command);
    let started_unix_nanos = now_unix_nanos()?;
    let mut child = ChildServer::spawn(command, log_path).map_err(|error| match error {
        harness_server::Error::Io(error) => {
            Error::Gate(format!("cannot launch generated-full producer: {error}"))
        }
        harness_server::Error::Unverified(message) | harness_server::Error::Gate(message) => {
            Error::Gate(message)
        }
    })?;
    let pid = child.id();
    child
        .wait_ready("generated-full producer", timeout, PROCESS_POLL, |text| {
            text.contains(ready_marker)
        })
        .map_err(|error| {
            let blocked = fs::read_to_string(log_path).ok().and_then(|text| {
                text.lines().find_map(|line| {
                    line.strip_prefix("RIVET_GENERATED_FULL_BLOCKED:")
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                        .map(str::to_string)
                })
            });
            if let Some(message) = blocked {
                return Error::Blocked(message);
            }
            match error {
                harness_server::Error::Io(error) => Error::Gate(format!(
                    "generated-full producer {pid} lifecycle I/O failed before READY: {error}"
                )),
                harness_server::Error::Unverified(message)
                | harness_server::Error::Gate(message) => Error::Gate(format!(
                    "generated-full producer {pid} failed before READY: {message}"
                )),
            }
        })?;
    let status = child
        .shutdown(timeout, PROCESS_POLL)
        .map_err(|error| match error {
            harness_server::Error::Io(error) => Error::Gate(format!(
                "generated-full producer {pid} shutdown I/O failed: {error}"
            )),
            harness_server::Error::Unverified(message) | harness_server::Error::Gate(message) => {
                Error::Gate(message)
            }
        })?;
    let stopped_unix_nanos = now_unix_nanos()?;
    let log_file = fs::OpenOptions::new()
        .read(true)
        .open(log_path)
        .map_err(|error| {
            Error::Gate(format!(
                "generated-full producer log {} cannot reopen: {error}",
                log_path.display()
            ))
        })?;
    log_file.sync_all().map_err(|error| {
        Error::Gate(format!(
            "generated-full producer log {} cannot fsync: {error}",
            log_path.display()
        ))
    })?;
    drop(log_file);
    let log_bytes = fs::read(log_path).map_err(|error| {
        Error::Gate(format!(
            "generated-full producer log {} cannot be read: {error}",
            log_path.display()
        ))
    })?;
    let ready_count = String::from_utf8_lossy(&log_bytes)
        .matches(ready_marker)
        .count() as u32;
    if ready_count != 1 {
        return Err(Error::Gate(format!(
            "generated-full producer {pid} emitted {ready_count} {ready_marker:?} markers; exactly one is required"
        )));
    }
    let exit_code = process_exit_code(status);
    validate_expected_lifecycle(
        pid,
        exit_code,
        &log_bytes,
        child.ready_offset(),
        expected_lifecycle,
    )?;
    Ok(ProcessObservation {
        pid,
        started_unix_nanos,
        stopped_unix_nanos,
        exit_code,
        ready_count,
        raw_log_sha256: crate::sha256_hex(&log_bytes),
        argv,
        cwd: cwd.display().to_string(),
        env,
    })
}

fn clear_latest_replay(base: &Path) -> Result<(), Error> {
    let path = base.join(LATEST_REPLAY_BASENAME);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Error::Gate(format!(
                    "generated-full latest replay pointer {} is a symlink",
                    path.display()
                )));
            }
            reject_hardlink_metadata(&metadata, &path, "generated-full latest replay pointer")?;
            fs::remove_file(&path).map_err(Error::Io)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Gate(format!(
            "generated-full latest replay pointer {} cannot be inspected: {error}",
            path.display()
        ))),
    }
}

fn publish_latest_replay(base: &Path, nonce: &str, replay_root: &Path) -> Result<(), Error> {
    let path = base.join(LATEST_REPLAY_BASENAME);
    let record = LatestReplay {
        schema: LATEST_REPLAY_SCHEMA.to_string(),
        nonce: nonce.to_string(),
        replay_root: replay_root.display().to_string(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
        Error::Gate(format!(
            "cannot serialize generated-full latest replay pointer: {error}"
        ))
    })?;
    write_fresh_file(&path, &bytes, "latest replay pointer")?;
    Ok(())
}

fn load_latest_replay(base: &Path) -> Result<ReplayRecord, Error> {
    let pointer_path = base.join(LATEST_REPLAY_BASENAME);
    let bytes = read_stable_file_capped(
        &pointer_path,
        "generated-full latest replay pointer",
        crate::json::MAX_JSON_BYTES as u64,
    )?;
    let pointer: LatestReplay = crate::json::from_slice(&bytes).map_err(|error| {
        Error::Gate(format!(
            "generated-full latest replay pointer {} is malformed: {error}",
            pointer_path.display()
        ))
    })?;
    if pointer.schema != LATEST_REPLAY_SCHEMA || pointer.nonce.is_empty() {
        return Err(Error::Gate(format!(
            "generated-full latest replay pointer {} has an invalid schema or nonce",
            pointer_path.display()
        )));
    }
    let expected_root = base.join(format!("replay-{}", pointer.nonce));
    if Path::new(&pointer.replay_root) != expected_root {
        return Err(Error::Gate(format!(
            "generated-full latest replay pointer {} names an unexpected replay root",
            pointer_path.display()
        )));
    }
    reject_symlink_components(&expected_root, "generated-full retained replay")?;
    if !is_real_dir(&expected_root) {
        return Err(Error::Gate(format!(
            "generated-full retained replay root {} is absent or not a directory",
            expected_root.display()
        )));
    }
    for side in ["paper", "rivet"] {
        let side_root = expected_root.join(side);
        let metadata = fs::symlink_metadata(&side_root).map_err(|error| {
            Error::Gate(format!(
                "generated-full retained replay {side} root {} is absent or cannot be inspected: {error}",
                side_root.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(Error::Gate(format!(
                "generated-full retained replay {side} root {} is not a regular directory",
                side_root.display()
            )));
        }
        reject_hardlink_metadata(
            &metadata,
            &side_root,
            "generated-full retained replay side root",
        )?;
    }
    let record_path = expected_root.join("replay.json");
    let record_bytes = read_stable_file_capped(
        &record_path,
        "generated-full retained replay record",
        crate::json::MAX_JSON_BYTES as u64,
    )
    .map_err(|error| match error {
        Error::Unverified(message) => Error::Gate(format!(
            "generated-full retained replay record {} is absent or partial: {message}",
            record_path.display()
        )),
        other => other,
    })?;
    let record: ReplayRecord = crate::json::from_slice(&record_bytes).map_err(|error| {
        Error::Gate(format!(
            "generated-full retained replay record {} is malformed: {error}",
            record_path.display()
        ))
    })?;
    if record.schema != REPLAY_SCHEMA
        || record.lifecycle != "completed"
        || record.nonce != pointer.nonce
        || record.controller_root != expected_root.display().to_string()
    {
        return Err(Error::Gate(format!(
            "generated-full retained replay record {} is incomplete or not owned by this pointer",
            record_path.display()
        )));
    }
    Ok(record)
}

/// Bind a retained observation path to the exact verifier-owned location that
/// the fresh replay writes for its side, seed, and capture boot.  The replay
/// record is untrusted input once `--tamper` is invoked: checking only that a
/// path is somewhere below `controller_root` would let a tampered record swap
/// Paper/Rivet sides or point at another seed's clean snapshot.
fn bind_replay_snapshot_root(
    record: &ReplayRecord,
    root: &Path,
    seed: u64,
    side: &str,
) -> Result<Option<u8>, Error> {
    let controller_root = Path::new(&record.controller_root);
    let (recorded_side_root, expected_side_root) = match side {
        "paper" => (&record.paper_root, controller_root.join("paper")),
        "rivet" => (&record.rivet_root, controller_root.join("rivet")),
        other => {
            return Err(Error::Gate(format!(
                "generated-full retained replay has unknown observation side {other:?}"
            )));
        }
    };
    if Path::new(recorded_side_root) != expected_side_root {
        return Err(Error::Gate(format!(
            "generated-full retained replay {side} root {} is not the controller-owned {} root {}",
            recorded_side_root,
            side,
            expected_side_root.display()
        )));
    }

    let seed_root = expected_side_root.join(seed.to_string());
    let (expected_root, paper_boot) = match side {
        "paper" => {
            if root.parent() != Some(seed_root.as_path()) {
                return Err(Error::Gate(format!(
                    "generated-full retained Paper seed-{seed} snapshot {} is not directly under {}",
                    root.display(),
                    seed_root.display()
                )));
            }
            let name = root.file_name().and_then(|value| value.to_str()).ok_or_else(|| {
                Error::Gate(format!(
                    "generated-full retained Paper seed-{seed} snapshot {} has no canonical UTF-8 name",
                    root.display()
                ))
            })?;
            let suffix = name.strip_prefix("snapshot-").ok_or_else(|| {
                Error::Gate(format!(
                    "generated-full retained Paper seed-{seed} snapshot {name:?} has an unexpected name; expected snapshot-1 or snapshot-2"
                ))
            })?;
            let boot = suffix.parse::<u8>().map_err(|_| {
                Error::Gate(format!(
                    "generated-full retained Paper seed-{seed} snapshot {name:?} has a non-numeric boot name"
                ))
            })?;
            if !(1..=2).contains(&boot) || name != format!("snapshot-{boot}") {
                return Err(Error::Gate(format!(
                    "generated-full retained Paper seed-{seed} snapshot {name:?} has an unexpected boot name; expected snapshot-1 or snapshot-2"
                )));
            }
            (seed_root.join(name), Some(boot))
        }
        "rivet" => (seed_root.join("snapshot"), None),
        _ => unreachable!("side was matched above"),
    };
    if root != expected_root {
        return Err(Error::Gate(format!(
            "generated-full retained {side} seed-{seed} snapshot {} is not the expected verifier-owned path {}",
            root.display(),
            expected_root.display()
        )));
    }
    Ok(paper_boot)
}

/// Validate every observation root in a retained replay record before hashing
/// any snapshot.  This rejects side/seed swaps, duplicate roots, skipped or
/// unexpected Paper boot names, and extra seeds that the record could otherwise
/// hide behind a valid in-root snapshot.
fn validate_replay_observation_layout(
    record: &ReplayRecord,
    contract: &GeneratedContract,
) -> Result<(), Error> {
    let expected_seeds = contract.seeds.iter().copied().collect::<BTreeSet<_>>();
    if expected_seeds.is_empty() {
        return Err(Error::Gate(
            "generated-full retained replay cannot validate an empty seed contract".into(),
        ));
    }
    let max_paper_observations = expected_seeds.len().checked_mul(2).ok_or_else(|| {
        Error::Gate("generated-full retained Paper observation cap overflowed".into())
    })?;
    if record.paper_observed.len() > max_paper_observations {
        return Err(Error::Gate(format!(
            "generated-full retained replay has {} Paper observations, above the {max_paper_observations}-observation cap",
            record.paper_observed.len()
        )));
    }
    if record.rivet_observed.len() > expected_seeds.len() {
        return Err(Error::Gate(format!(
            "generated-full retained replay has {} Rivet observations, above the {}-observation cap",
            record.rivet_observed.len(),
            expected_seeds.len()
        )));
    }
    let mut seen_roots = BTreeSet::new();
    let mut paper_boots = std::collections::BTreeMap::<u64, BTreeSet<u8>>::new();

    for observation in &record.paper_observed {
        if !expected_seeds.contains(&observation.seed) {
            return Err(Error::Gate(format!(
                "generated-full retained replay has an unexpected Paper seed {}",
                observation.seed
            )));
        }
        let root = PathBuf::from(&observation.root);
        let Some(boot) = bind_replay_snapshot_root(record, &root, observation.seed, "paper")?
        else {
            unreachable!("Paper observations always carry a boot number")
        };
        if !seen_roots.insert(root.clone()) {
            return Err(Error::Gate(format!(
                "generated-full retained replay reuses observation root {}",
                root.display()
            )));
        }
        paper_boots
            .entry(observation.seed)
            .or_default()
            .insert(boot);
    }

    let mut rivet_seeds = BTreeSet::new();
    for observation in &record.rivet_observed {
        if !expected_seeds.contains(&observation.seed) {
            return Err(Error::Gate(format!(
                "generated-full retained replay has an unexpected Rivet seed {}",
                observation.seed
            )));
        }
        let root = PathBuf::from(&observation.root);
        if bind_replay_snapshot_root(record, &root, observation.seed, "rivet")?.is_some() {
            unreachable!("Rivet observations never carry a Paper boot number")
        }
        if !seen_roots.insert(root.clone()) {
            return Err(Error::Gate(format!(
                "generated-full retained replay reuses observation root {}",
                root.display()
            )));
        }
        if !rivet_seeds.insert(observation.seed) {
            return Err(Error::Gate(format!(
                "generated-full retained replay has duplicate Rivet observations for seed {}",
                observation.seed
            )));
        }
    }

    let mut capture_count = None;
    for &seed in &expected_seeds {
        let Some(boots) = paper_boots.get(&seed) else {
            return Err(Error::Gate(format!(
                "generated-full retained replay is partial: no Paper capture observation for seed {seed}"
            )));
        };
        let Some(&last) = boots.iter().next_back() else {
            unreachable!("an inserted Paper seed always has a boot")
        };
        let expected = (1..=last).collect::<BTreeSet<_>>();
        if boots != &expected {
            return Err(Error::Gate(format!(
                "generated-full retained Paper seed-{seed} capture boots {boots:?} are not a contiguous sequence starting at snapshot-1"
            )));
        }
        if !matches!(boots.len(), 1 | 2) {
            return Err(Error::Gate(format!(
                "generated-full retained Paper seed-{seed} has {} capture boots; expected one ordinary or two refresh captures",
                boots.len()
            )));
        }
        if capture_count
            .replace(boots.len())
            .is_some_and(|count| count != boots.len())
        {
            return Err(Error::Gate(
                "generated-full retained replay has inconsistent Paper capture counts across seeds"
                    .into(),
            ));
        }
        if !rivet_seeds.contains(&seed) {
            return Err(Error::Gate(format!(
                "generated-full retained replay is partial: no Rivet observation for seed {seed}"
            )));
        }
    }
    Ok(())
}

fn allocate_replay_root() -> Result<(String, PathBuf), Error> {
    let base = crate::crate_dir().join("work/generated-full");
    fs::create_dir_all(&base).map_err(|error| {
        Error::Gate(format!(
            "cannot create replay base {}: {error}",
            base.display()
        ))
    })?;
    reject_symlink_components(&base, "generated-full replay base")?;
    clear_latest_replay(&base)?;
    for attempt in 0..8u32 {
        let now = now_unix_nanos()?;
        let nonce = format!("{:x}-{}-{}", now, std::process::id(), attempt);
        let root = base.join(format!("replay-{nonce}"));
        match fs::create_dir(&root) {
            Ok(()) => {
                fs::create_dir(root.join("paper"))?;
                fs::create_dir(root.join("rivet"))?;
                fs::create_dir(root.join("logs"))?;
                return Ok((nonce, root));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(Error::Gate(format!(
                    "cannot allocate fresh replay root {}: {error}",
                    root.display()
                )));
            }
        }
    }
    Err(Error::Gate(
        "could not allocate a collision-free generated-full replay nonce".into(),
    ))
}

fn snapshot_tree(src: &Path, dst: &Path) -> Result<(), Error> {
    snapshot_tree_with_limits(
        src,
        dst,
        MAX_EVIDENCE_ENTRIES,
        hash_manifest::MAX_TOTAL_PAYLOAD_BYTES as u64,
    )
}

#[cfg(test)]
fn snapshot_tree_with_limits_for_test(
    src: &Path,
    dst: &Path,
    max_entries: usize,
    max_bytes: u64,
) -> Result<(), Error> {
    snapshot_tree_with_limits(src, dst, max_entries, max_bytes)
}

fn snapshot_tree_with_limits(
    src: &Path,
    dst: &Path,
    max_entries: usize,
    max_bytes: u64,
) -> Result<(), Error> {
    reject_symlink_components(src, "generated-full producer output")?;
    if !src.exists() {
        return Err(Error::Gate(format!(
            "generated-full producer completed but output root {} is absent; launched evidence is failed",
            src.display()
        )));
    }
    if !is_real_dir(src) {
        return Err(Error::Gate(format!(
            "generated-full producer output {} is not a directory",
            src.display()
        )));
    }
    if dst.exists() {
        return Err(Error::Gate(format!(
            "generated-full verifier snapshot {} already exists; refusing to reuse evidence",
            dst.display()
        )));
    }
    fs::create_dir_all(dst)?;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf(), 0usize)];
    let mut aggregate_entries = 0usize;
    let mut aggregate_bytes = 0u64;
    while let Some((from, to, depth)) = stack.pop() {
        if depth > 8 {
            return Err(Error::Gate(format!(
                "generated-full output tree {} is too deep",
                from.display()
            )));
        }
        let mut directory_entries = 0usize;
        for entry in fs::read_dir(&from)? {
            let entry = entry?;
            directory_entries = directory_entries.checked_add(1).ok_or_else(|| {
                Error::Gate("generated-full output directory entry count overflowed".into())
            })?;
            if directory_entries > max_entries {
                return Err(Error::Gate(format!(
                    "generated-full output directory {} exceeds the {}-entry cap",
                    from.display(),
                    max_entries
                )));
            }
            aggregate_entries = aggregate_entries.checked_add(1).ok_or_else(|| {
                Error::Gate("generated-full producer snapshot entry count overflowed".into())
            })?;
            if aggregate_entries > max_entries {
                return Err(Error::Gate(format!(
                    "generated-full producer snapshot exceeds the {max_entries}-entry aggregate cap"
                )));
            }
            let source = entry.path();
            let target = to.join(entry.file_name());
            reject_symlink(&source, "generated-full producer output")?;
            reject_hardlink(&source, "generated-full producer output")?;
            if is_real_dir(&source) {
                fs::create_dir(&target)?;
                stack.push((source, target, depth + 1));
            } else if is_regular_file(&source) {
                let cap = if source.extension().and_then(|ext| ext.to_str()) == Some("nbt") {
                    MAX_PAYLOAD_FILE_BYTES
                } else {
                    MAX_EVIDENCE_FILE_BYTES
                };
                let bytes =
                    read_stable_file_capped(&source, "generated-full producer output", cap)?;
                aggregate_bytes =
                    aggregate_bytes
                        .checked_add(bytes.len() as u64)
                        .ok_or_else(|| {
                            Error::Gate(
                                "generated-full producer snapshot byte count overflowed".into(),
                            )
                        })?;
                if aggregate_bytes > max_bytes {
                    return Err(Error::Gate(format!(
                        "generated-full producer snapshot exceeds the {max_bytes}-byte aggregate cap"
                    )));
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                std::io::Write::write_all(&mut file, &bytes)?;
                file.sync_all()?;
                drop(file);
            } else {
                return Err(Error::Gate(format!(
                    "generated-full producer output {} is not a regular file or directory",
                    source.display()
                )));
            }
        }
    }
    Ok(())
}

fn open_stable_directory(path: &Path, what: &str) -> Result<fs::File, Error> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{CWD, Mode, OFlags, ResolveFlags, openat2};
        let file = openat2(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map(fs::File::from)
        .map_err(std::io::Error::from)
        .map_err(|error| {
            Error::Gate(format!(
                "{what} {} cannot be opened as a fresh regular directory without following links: {error}",
                path.display()
            ))
        })?;
        let metadata = file.metadata().map_err(|error| {
            Error::Gate(format!(
                "{what} {} cannot be inspected through its opened directory descriptor: {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(Error::Gate(format!(
                "{what} {} is not a regular directory",
                path.display()
            )));
        }
        reject_hardlink_metadata(&metadata, path, what)?;
        Ok(file)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, what);
        Err(Error::Gate(
            "descriptor-relative generated-full output validation requires Linux openat2".into(),
        ))
    }
}

fn require_fresh_output_root(path: &Path, what: &str) -> Result<(), Error> {
    reject_symlink_components(path.parent().unwrap_or_else(|| Path::new(".")), what)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => Err(Error::Gate(format!(
            "{what} {} already exists as {}; producer output roots must be fresh and absent",
            path.display(),
            if metadata.file_type().is_symlink() {
                "a symlink"
            } else if metadata.file_type().is_dir() {
                "a directory"
            } else {
                "an existing entry"
            }
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Gate(format!(
            "{what} {} cannot be checked for freshness: {error}",
            path.display()
        ))),
    }
}

fn write_seed_config(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
) -> Result<PathBuf, Error> {
    let root_descriptor = open_stable_directory(root, "generated-full seed-config root")?;
    let config = serde_json::json!({
        "seed-u64": seed,
        "seed-java-long": java_seed_long(seed),
        "level-type": contract.level_type,
        "dimension": &contract.dimension,
        "region-file-compression": contract.region_file_compression,
        "status": &contract.status,
        "stage": &contract.stage,
    });
    let bytes = serde_json::to_vec_pretty(&config).map_err(|error| {
        Error::Gate(format!(
            "cannot serialize generated-full seed config: {error}"
        ))
    })?;
    #[cfg(target_os = "linux")]
    let mut file = {
        use rustix::fs::{Mode, OFlags, openat};
        use std::os::fd::AsFd;
        openat(
            root_descriptor.as_fd(),
            SEED_CONFIG_BASENAME,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o644),
        )
        .map(fs::File::from)
        .map_err(std::io::Error::from)
        .map_err(Error::Io)?
    };
    #[cfg(not(target_os = "linux"))]
    let mut file = {
        let _ = &mut root_descriptor;
        return Err(Error::Gate(
            "descriptor-relative generated-full seed-config writes require Linux openat2".into(),
        ));
    };
    file.write_all(&bytes).map_err(Error::Io)?;
    file.write_all(b"\n").map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    Ok(root.join(SEED_CONFIG_BASENAME))
}

fn write_fresh_file(path: &Path, bytes: &[u8], what: &str) -> Result<(), Error> {
    if path.exists() {
        return Err(Error::Gate(format!(
            "generated-full {what} {} already exists; refusing to reuse it",
            path.display()
        )));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Error::Io)?;
    file.write_all(bytes).map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    Ok(())
}

fn open_staged_file(
    path: PathBuf,
    what: &str,
    expected_sha256: &str,
    cap: u64,
) -> Result<StagedFile, Error> {
    let mut file = open_stable_regular(&path, what)?;
    let bytes = read_opened_file_capped(&mut file, &path, what, cap)?;
    let sha256 = crate::sha256_hex(&bytes);
    if sha256 != expected_sha256 {
        return Err(Error::Gate(format!(
            "{what} {} changed while staging; expected SHA-256 {expected_sha256}, got {sha256}",
            path.display()
        )));
    }
    Ok(StagedFile { path, file, bytes })
}

fn retain_overworld_only(root: &Path) -> Result<(), Error> {
    let chunk_root = root.join("chunk");
    let entries = fs::read_dir(&chunk_root)
        .map_err(|error| Error::Gate(format!("cannot read {}: {error}", chunk_root.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Io)?;
    for entry in entries {
        let path = entry.path();
        reject_symlink(&path, "generated-full extracted dimension")?;
        if entry.file_name() != "overworld" {
            if !is_real_dir(&path) {
                return Err(Error::Gate(format!(
                    "generated-full extracted dimension {} is not a directory",
                    path.display()
                )));
            }
            fs::remove_dir_all(path).map_err(Error::Io)?;
        }
    }
    Ok(())
}

/// Prune an extracted Paper capture tree to the contract's exact payload
/// closure. The `--all-regions` extraction dumps every saved chunk in the four
/// origin-adjacent regions — including the spawn-area chunks every genuine
/// boot saves — while the contract covers only the eight forced corpus
/// coordinates. Without this prune the verifier's exact payload-closure check
/// would refuse genuine evidence as having "extra paths". Every pruned file
/// was produced by the same boot, so dropping it cannot hide divergence: the
/// remaining bytes are still compared in full against the Rivet side.
fn prune_to_contract_closure(root: &Path, contract: &GeneratedContract) -> Result<(), Error> {
    let keep = expected_paths(contract);
    let dim_root = root.join("chunk").join(&contract.dimension);
    for region_entry in fs::read_dir(&dim_root)
        .map_err(|error| Error::Gate(format!("cannot read {}: {error}", dim_root.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Io)?
    {
        let region_path = region_entry.path();
        reject_symlink(&region_path, "generated-full extracted region")?;
        if !is_real_dir(&region_path) {
            return Err(Error::Gate(format!(
                "generated-full extracted region {} is not a directory",
                region_path.display()
            )));
        }
        for file_entry in fs::read_dir(&region_path)
            .map_err(|error| {
                Error::Gate(format!("cannot read {}: {error}", region_path.display()))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::Io)?
        {
            let path = file_entry.path();
            reject_symlink(&path, "generated-full extracted payload")?;
            if !keep.contains(
                path.strip_prefix(root)
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .as_str(),
            ) {
                fs::remove_file(&path).map_err(Error::Io)?;
            }
        }
        if fs::read_dir(&region_path)
            .map_err(|error| {
                Error::Gate(format!("cannot read {}: {error}", region_path.display()))
            })?
            .next()
            .is_none()
        {
            fs::remove_dir(&region_path).map_err(Error::Io)?;
        }
    }
    Ok(())
}

fn check_snapshot_closure(root: &Path) -> Result<(), Error> {
    let expected = BTreeSet::from([
        "chunk".to_string(),
        SEED_CONFIG_BASENAME.to_string(),
        MANIFEST_BASENAME.to_string(),
    ]);
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let path = entry.path();
        reject_symlink(&path, "generated-full snapshot root")?;
        reject_hardlink(&path, "generated-full snapshot root")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !expected.contains(name.as_str()) {
            return Err(Error::Gate(format!(
                "generated-full snapshot {} has extra entry {name:?}",
                root.display()
            )));
        }
        if name == "chunk" {
            if !is_real_dir(&path) {
                return Err(Error::Gate(format!(
                    "generated-full snapshot {} chunk is not a directory",
                    root.display()
                )));
            }
        } else if !is_regular_file(&path) {
            return Err(Error::Gate(format!(
                "generated-full snapshot {} entry {name:?} is not a regular file",
                root.display()
            )));
        }
        actual.insert(name);
    }
    if actual != expected {
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(Error::Gate(format!(
            "generated-full snapshot {} has partial closure; missing declared entries {missing:?}",
            root.display()
        )));
    }
    Ok(())
}

fn raw_manifest_from_snapshot(
    contract: &GeneratedContract,
    root: &Path,
    seed: u64,
    side: &str,
) -> Result<HashManifest, Error> {
    check_snapshot_closure(root)?;
    let discovered =
        discover_payloads(root, contract, seed, side).map_err(|error| match error {
            Error::Unverified(message) => Error::Gate(format!(
                "generated-full {side} seed-{seed} launched output is incomplete: {message}"
            )),
            other => other,
        })?;
    for payload in &discovered {
        let compound = mutate::parse_payload(&payload.bytes).map_err(|error| {
            Error::Gate(format!(
                "generated-full {side} seed-{seed} payload {} is malformed: {error}",
                payload.path.display()
            ))
        })?;
        if (compound.get_int("xPos"), compound.get_int("zPos"))
            != (Some(payload.cx), Some(payload.cz))
        {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} payload {} xPos/zPos do not bind to filename",
                payload.path.display()
            )));
        }
        require_canonical_last_update(&compound, &payload.path)?;
    }
    let payloads = discovered
        .iter()
        .map(|payload| hash_manifest::PayloadBytes {
            dim: contract.dimension.clone(),
            region: region_for(payload.cx, payload.cz),
            cx: payload.cx,
            cz: payload.cz,
            bytes: payload.bytes.clone(),
        })
        .collect::<Vec<_>>();
    let capture = CaptureProvenance {
        level_type: contract.level_type.clone(),
        region_file_compression: contract.region_file_compression.clone(),
        corpus_version: contract.corpus_version.clone(),
    };
    let manifest = hash_manifest::build_from_payload_bytes_with(
        &payloads,
        &seed.to_string(),
        &contract.level_type,
        &capture,
    )
    .map_err(|error| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} raw payload validation failed: {error}"
        ))
    })?;
    if manifest.full_count != contract.coordinates.len()
        || manifest.entries.len() != contract.coordinates.len()
    {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} produced {} FULL payloads, expected {}; launched evidence is not a complete FULL replay",
            manifest.full_count,
            contract.coordinates.len()
        )));
    }
    Ok(manifest)
}

fn raw_payload_digests(manifest: &HashManifest) -> Vec<RawPayloadDigest> {
    manifest
        .entries
        .iter()
        .map(|entry| RawPayloadDigest {
            dimension: entry.dim.clone(),
            region: entry.region.clone(),
            x: entry.cx,
            z: entry.cz,
            bytes: entry.bytes,
            xxh3_64: entry.xxh3_64.clone(),
            sha256: entry.sha256.clone(),
        })
        .collect()
}

fn paper_observed(
    seed: u64,
    root: &Path,
    identity: &ArtifactIdentity,
    process: ProcessObservation,
    manifest: &HashManifest,
    revision_identity_sha256: &str,
    config_sha256: &str,
) -> PaperObserved {
    PaperObserved {
        schema: "paper-observed-v1".to_string(),
        seed,
        root: root.display().to_string(),
        argv: process.argv,
        cwd: process.cwd,
        env: process.env,
        paper_jar_sha256: identity.materialized_jar_sha256.clone(),
        paper_config_sha256: config_sha256.to_string(),
        paper_revision_identity_sha256: revision_identity_sha256.to_string(),
        pid: process.pid,
        started_unix_nanos: process.started_unix_nanos,
        ready_count: process.ready_count,
        stopped_unix_nanos: process.stopped_unix_nanos,
        exit_code: process.exit_code,
        raw_log_sha256: process.raw_log_sha256,
        payload_digests: raw_payload_digests(manifest),
        producer_manifest_sha256: None,
    }
}

fn rivet_observed(
    seed: u64,
    root: &Path,
    identity: &ArtifactIdentity,
    process: ProcessObservation,
    manifest: &HashManifest,
    revision_identity_sha256: &str,
) -> RivetObserved {
    RivetObserved {
        schema: "rivet-observed-v1".to_string(),
        seed,
        root: root.display().to_string(),
        argv: process.argv,
        cwd: process.cwd,
        env: process.env,
        rivet_executable_sha256: identity.capture_binary_sha256.clone(),
        rivet_config_sha256: identity.capture_config_sha256.clone(),
        rivet_revision_identity_sha256: revision_identity_sha256.to_string(),
        pid: process.pid,
        started_unix_nanos: process.started_unix_nanos,
        ready_count: process.ready_count,
        stopped_unix_nanos: process.stopped_unix_nanos,
        exit_code: process.exit_code,
        raw_log_sha256: process.raw_log_sha256,
        payload_digests: raw_payload_digests(manifest),
        producer_manifest_sha256: None,
    }
}

fn write_replay_record(root: &Path, record: &ReplayRecord) -> Result<(), Error> {
    let path = root.join("replay.json");
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
        Error::Gate(format!(
            "cannot serialize generated-full replay record: {error}"
        ))
    })?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Error::Io)?;
    file.write_all(&bytes).map_err(Error::Io)?;
    file.write_all(b"\n").map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    Ok(())
}

fn run_fresh_replay(contract: &GeneratedContract, artifacts: &ArtifactInputs) -> Result<(), Error> {
    run_fresh_replay_with_paper_boots(contract, artifacts, 2)
}

fn run_fresh_replay_with_paper_boots(
    contract: &GeneratedContract,
    artifacts: &ArtifactInputs,
    paper_boots: u8,
) -> Result<(), Error> {
    if !(2..=3).contains(&paper_boots) {
        return Err(Error::Gate("generated-full replay requires two ordinary Paper boots or three explicit determinism-refresh boots per seed".into()));
    }
    let (nonce, replay_root) = allocate_replay_root()?;
    let inputs = replay_root.join("inputs");
    fs::create_dir(&inputs).map_err(Error::Io)?;
    let staged = artifacts.stage(&replay_root)?;
    let identity = &staged.identity;
    let paper_revision_identity_sha256 = crate::sha256_hex(identity.paper_commit.as_bytes());
    let rivet_revision_identity_sha256 = crate::sha256_hex(identity.rivet_commit.as_bytes());
    // Post-attestation reads consume the staged descriptors' captured bytes:
    // nothing is reopened by pathname, so a path swap between staging and use
    // cannot substitute different contents.
    let staged_paper_properties = &staged.paper_properties.bytes;
    if crate::sha256_hex(staged_paper_properties) != identity.paper_config_template_sha256 {
        return Err(Error::Gate(
            "generated-full staged Paper capture properties changed after attestation".into(),
        ));
    }
    let staged_rivet_config = &staged.rivet_config.bytes;
    if crate::sha256_hex(staged_rivet_config) != identity.capture_config_sha256 {
        return Err(Error::Gate(
            "generated-full staged Rivet capture config changed after attestation".into(),
        ));
    }
    let mut paper_observations = Vec::new();
    let mut rivet_observations = Vec::new();

    for &seed in &contract.seeds {
        let seed_text = seed.to_string();
        let paper_seed_root = replay_root.join("paper").join(&seed_text);
        let rivet_seed_root = replay_root.join("rivet").join(&seed_text);
        let paper_run = paper_seed_root.join("run");
        let rivet_output = rivet_seed_root.join("output");
        fs::create_dir_all(&paper_seed_root).map_err(Error::Io)?;
        fs::create_dir_all(&rivet_seed_root).map_err(Error::Io)?;
        let seed_input_root = inputs.join(&seed_text);
        fs::create_dir(&seed_input_root).map_err(Error::Io)?;
        let _seed_config = write_seed_config(&seed_input_root, contract, seed)?;
        let paper_properties = inputs.join(format!("paper-{seed}.properties"));
        let paper_config = rewrite_paper_properties_for_seed(staged_paper_properties, seed)?;
        let paper_config_sha256 = crate::sha256_hex(&paper_config);
        write_fresh_file(
            &paper_properties,
            &paper_config,
            "generated-full Paper seed properties",
        )?;
        let rivet_config = &staged.rivet_config;

        prepare_run_dir(&paper_run, &paper_properties)?;
        let mut paper_captures = Vec::new();
        for boot in 0..paper_boots {
            let paper_log = replay_root
                .join("logs")
                .join(format!("paper-{seed}-{boot}.log"));
            let paper_argv = vec![
                "java".to_string(),
                "-Xms512M".to_string(),
                "-Xmx2G".to_string(),
                "-jar".to_string(),
                staged.paper_jar.path.display().to_string(),
                "nogui".to_string(),
            ];
            let mut paper_command = Command::new("java");
            paper_command
                .args(["-Xms512M", "-Xmx2G", "-jar"])
                .arg(&staged.paper_jar.path)
                .arg("nogui")
                .current_dir(&paper_run);
            let paper_process = process_observation(
                &mut paper_command,
                paper_argv,
                &paper_run,
                &paper_log,
                PAPER_READY_MARKER,
                PAPER_READY_TIMEOUT,
                ExpectedLifecycle::PaperClean,
            )?;
            if boot == 0 {
                let coordinates = contract
                    .coordinates
                    .iter()
                    .map(|coordinate| (coordinate.x, coordinate.z))
                    .collect::<Vec<_>>();
                inject_forced_tickets(
                    &paper_run.join("world"),
                    &coordinates,
                    OVERWORLD_DIM,
                    FORCED_TICKET_LEVEL,
                )?;
                continue;
            }

            // Every post-injection boot is a capture boot. Keep each raw tree
            // and derive its manifest independently; refresh-determinism must
            // prove that no earlier Paper capture diverged before Rivet is
            // allowed into the parity comparison.
            let paper_output = paper_seed_root.join(format!("output-{boot}"));
            let observed = parse_boot_thread_counts(&String::from_utf8_lossy(&read_stable_file(
                &paper_log,
                "generated-full Paper raw log",
            )?))
            .map(|(worker_threads, io_threads)| crate::ChunkConcurrency {
                worker_threads,
                io_threads,
            });
            verify_forced_load(&paper_log, contract.coordinates.len(), OVERWORLD_DIM)?;
            require_fresh_output_root(&paper_output, "generated-full Paper output root")?;
            extract_fresh_fixtures(
                &paper_run.join("world"),
                &paper_output,
                true,
                KIND_FULL,
                observed,
            )?;
            normalize_last_update_tree(&paper_output)?;
            retain_overworld_only(&paper_output)?;
            prune_to_contract_closure(&paper_output, contract)?;
            rehash_captured(&paper_output)?;
            write_seed_config(&paper_output, contract, seed)?;
            let paper_snapshot = paper_seed_root.join(format!("snapshot-{boot}"));
            snapshot_tree(&paper_output, &paper_snapshot)?;
            let paper_manifest =
                raw_manifest_from_snapshot(contract, &paper_snapshot, seed, "paper")?;
            paper_captures.push((boot, paper_manifest, (paper_process, paper_snapshot)));
        }
        if paper_captures.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full Paper seed {seed} produced no post-injection capture boots"
            )));
        }
        let capture_manifests = paper_captures
            .iter()
            .map(|(boot, manifest, _)| (*boot, manifest.clone()))
            .collect::<Vec<_>>();
        let paper_manifest = verify_capture_determinism(seed, &capture_manifests)?;
        for (_, paper_manifest, (paper_process, paper_snapshot)) in paper_captures {
            paper_observations.push(paper_observed(
                seed,
                &paper_snapshot,
                identity,
                paper_process,
                &paper_manifest,
                &paper_revision_identity_sha256,
                &paper_config_sha256,
            ));
        }

        require_fresh_output_root(&rivet_output, "generated-full Rivet producer output root")?;
        let rivet_log = replay_root.join("logs").join(format!("rivet-{seed}.log"));
        let coordinates_json = serde_json::to_string(&contract.coordinates).map_err(|error| {
            Error::Gate(format!(
                "cannot serialize generated-full coordinates: {error}"
            ))
        })?;
        let rivet_argv = vec![
            staged.rivet_binary.path.display().to_string(),
            "--generated-full".to_string(),
            "--seed".to_string(),
            seed_text.clone(),
            "--coordinates".to_string(),
            coordinates_json.clone(),
            "--config".to_string(),
            rivet_config.path.display().to_string(),
            "--output".to_string(),
            rivet_output.display().to_string(),
            "--nonce".to_string(),
            nonce.clone(),
        ];
        let mut rivet_command = Command::new(&staged.rivet_binary.path);
        rivet_command
            .arg("--generated-full")
            .arg("--seed")
            .arg(&seed_text)
            .arg("--coordinates")
            .arg(&coordinates_json)
            .arg("--config")
            .arg(&rivet_config.path)
            .arg("--output")
            .arg(&rivet_output)
            .arg("--nonce")
            .arg(&nonce)
            .current_dir(&replay_root);
        let rivet_process = process_observation(
            &mut rivet_command,
            rivet_argv,
            &replay_root,
            &rivet_log,
            RIVET_READY_MARKER,
            PRODUCER_READY_TIMEOUT,
            ExpectedLifecycle::ProducerExit(0),
        )?;
        write_seed_config(&rivet_output, contract, seed)?;
        let rivet_snapshot = rivet_seed_root.join("snapshot");
        snapshot_tree(&rivet_output, &rivet_snapshot)?;
        let rivet_manifest = raw_manifest_from_snapshot(contract, &rivet_snapshot, seed, "rivet")?;
        rivet_observations.push(rivet_observed(
            seed,
            &rivet_snapshot,
            identity,
            rivet_process,
            &rivet_manifest,
            &rivet_revision_identity_sha256,
        ));
        let mismatches = manifest_mismatches(contract, seed, &paper_manifest, &rivet_manifest);
        if !mismatches.is_empty() {
            let details = mismatches
                .iter()
                .map(|mismatch| {
                    format!(
                        "overworld/{}.{} expected {} got {}",
                        mismatch.coordinate.0,
                        mismatch.coordinate.1,
                        mismatch.expected,
                        mismatch.actual
                    )
                })
                .collect::<Vec<_>>();
            return Err(Error::Gate(format!(
                "generated-full parity FAIL after fresh replay for seed {seed}: {} divergent payloads\n{}",
                mismatches.len(),
                details.join("\n")
            )));
        }
    }

    let record = ReplayRecord {
        schema: REPLAY_SCHEMA.to_string(),
        nonce,
        controller_root: replay_root.display().to_string(),
        paper_root: replay_root.join("paper").display().to_string(),
        rivet_root: replay_root.join("rivet").display().to_string(),
        lifecycle: "completed".to_string(),
        paper_observed: paper_observations,
        rivet_observed: rivet_observations,
    };
    write_replay_record(&replay_root, &record)?;
    publish_latest_replay(
        &crate::crate_dir().join("work/generated-full"),
        &record.nonce,
        &replay_root,
    )?;
    println!(
        "PARITY_VERIFIED: generated-full fresh replay matched ({} seeds, {} coordinates, {} Paper boots, {} Rivet runs)",
        contract.seeds.len(),
        contract.coordinates.len(),
        contract.seeds.len() * (paper_boots as usize - 1),
        contract.seeds.len()
    );
    Ok(())
}

fn verify_capture_determinism(
    seed: u64,
    captures: &[(u8, HashManifest)],
) -> Result<HashManifest, Error> {
    let Some((first_boot, first_manifest)) = captures.first() else {
        return Err(Error::Gate(format!(
            "generated-full Paper seed {seed} produced no capture-boot evidence"
        )));
    };
    for (boot, manifest) in captures.iter().skip(1) {
        if manifest != first_manifest {
            return Err(Error::Gate(format!(
                "generated-full Paper determinism FAIL for seed {seed}: capture boots {first_boot} and {boot} differ in verifier-derived byte/manifest observations"
            )));
        }
    }
    Ok(first_manifest.clone())
}

fn replay_snapshot_manifest(
    record: &ReplayRecord,
    observation_root: &str,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
) -> Result<ReplaySnapshotEvidence, Error> {
    let replay_root = Path::new(&record.controller_root);
    let root = PathBuf::from(observation_root);
    bind_replay_snapshot_root(record, &root, seed, side)?;
    reject_symlink_components(&root, "generated-full retained replay snapshot")?;
    let root_descriptor = open_stable_directory(&root, "generated-full retained replay snapshot")?;
    let root_metadata = root_descriptor.metadata().map_err(|error| {
        Error::Gate(format!(
            "generated-full retained replay snapshot {} cannot be inspected through its opened directory descriptor: {error}",
            root.display()
        ))
    })?;
    let canonical_root = root.canonicalize().map_err(|error| {
        Error::Gate(format!(
            "generated-full retained replay snapshot {} cannot be resolved: {error}",
            root.display()
        ))
    })?;
    let canonical_replay = replay_root.canonicalize().map_err(|error| {
        Error::Gate(format!(
            "generated-full retained replay root {} cannot be resolved: {error}",
            replay_root.display()
        ))
    })?;
    if !canonical_root.starts_with(&canonical_replay) || canonical_root == canonical_replay {
        return Err(Error::Gate(format!(
            "generated-full retained {side} seed-{seed} snapshot {} escapes the controller-owned replay root",
            root.display()
        )));
    }
    let manifest = raw_manifest_from_snapshot(contract, &root, seed, side)?;
    verify_directory_path_matches_descriptor(
        &root,
        &root_metadata,
        "generated-full retained replay snapshot",
    )?;
    Ok(ReplaySnapshotEvidence { root, manifest })
}

fn validate_replay_observation(
    record: &ReplayRecord,
    observation: &PaperObserved,
    contract: &GeneratedContract,
    side: &str,
) -> Result<ReplaySnapshotEvidence, Error> {
    let evidence =
        replay_snapshot_manifest(record, &observation.root, contract, observation.seed, side)?;
    if observation.schema != "paper-observed-v1"
        || observation.ready_count != 1
        || observation.exit_code != 0 && observation.exit_code != 143
        || observation.payload_digests != raw_payload_digests(&evidence.manifest)
    {
        return Err(Error::Gate(format!(
            "generated-full retained {side} seed-{} observation does not match verifier-derived snapshot evidence",
            observation.seed
        )));
    }
    Ok(evidence)
}

fn validate_rivet_observation(
    record: &ReplayRecord,
    observation: &RivetObserved,
    contract: &GeneratedContract,
) -> Result<ReplaySnapshotEvidence, Error> {
    let evidence = replay_snapshot_manifest(
        record,
        &observation.root,
        contract,
        observation.seed,
        "rivet",
    )?;
    if observation.schema != "rivet-observed-v1"
        || observation.ready_count != 1
        || observation.exit_code != 0
        || observation.payload_digests != raw_payload_digests(&evidence.manifest)
    {
        return Err(Error::Gate(format!(
            "generated-full retained Rivet seed-{} observation does not match verifier-derived snapshot evidence",
            observation.seed
        )));
    }
    Ok(evidence)
}

fn run_tamper_from_latest(
    contract: &GeneratedContract,
    selected: Option<TamperKind>,
) -> Result<(), Error> {
    let base = crate::crate_dir().join("work/generated-full");
    let record = load_latest_replay(&base).map_err(|error| match error {
        Error::Unverified(message) => Error::Blocked(message),
        other => other,
    })?;
    let expected_paper_root = Path::new(&record.controller_root).join("paper");
    let expected_rivet_root = Path::new(&record.controller_root).join("rivet");
    if record.paper_root != expected_paper_root.display().to_string()
        || record.rivet_root != expected_rivet_root.display().to_string()
    {
        return Err(Error::Gate(
            "generated-full retained replay record has untrusted side roots".into(),
        ));
    }
    reject_symlink_components(&expected_paper_root, "generated-full retained Paper root")?;
    reject_symlink_components(&expected_rivet_root, "generated-full retained Rivet root")?;
    validate_replay_observation_layout(&record, contract)?;
    let kinds = selected.map_or_else(|| TamperKind::ALL.to_vec(), |kind| vec![kind]);
    let mut paper_by_seed: std::collections::BTreeMap<u64, Vec<ReplaySnapshotEvidence>> =
        Default::default();
    for observation in &record.paper_observed {
        let value = validate_replay_observation(&record, observation, contract, "paper")?;
        paper_by_seed
            .entry(observation.seed)
            .or_default()
            .push(value);
    }
    let mut rivet_by_seed: std::collections::BTreeMap<u64, ReplaySnapshotEvidence> =
        Default::default();
    for observation in &record.rivet_observed {
        if rivet_by_seed
            .insert(
                observation.seed,
                validate_rivet_observation(&record, observation, contract)?,
            )
            .is_some()
        {
            return Err(Error::Gate(format!(
                "generated-full retained replay has duplicate Rivet observations for seed {}",
                observation.seed
            )));
        }
    }
    for &seed in &contract.seeds {
        let papers = paper_by_seed.get(&seed).ok_or_else(|| {
            Error::Blocked(format!(
                "generated-full retained replay has no Paper capture prerequisite for seed {seed}"
            ))
        })?;
        let rivet = rivet_by_seed.get(&seed).ok_or_else(|| {
            Error::Blocked(format!(
                "generated-full retained replay has no Rivet capture prerequisite for seed {seed}"
            ))
        })?;
        let paper_manifest = &papers[0].manifest;
        for other in papers.iter().skip(1) {
            if other.manifest != *paper_manifest {
                return Err(Error::Gate(format!(
                    "generated-full Paper determinism FAIL for seed {seed}: retained capture boots diverge"
                )));
            }
        }
        let mismatches = manifest_mismatches(contract, seed, paper_manifest, &rivet.manifest);
        if !mismatches.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full retained replay is not parity-clean for seed {seed}; tamper controls cannot run before replay parity"
            )));
        }
        if seed != contract.seeds[0] {
            continue;
        }
        let target = contract.coordinates.first().ok_or_else(|| {
            Error::Gate("generated-full contract has no tamper target coordinate".into())
        })?;
        let target_relative = format!(
            "chunk/{}/{}/{}.{}.nbt",
            contract.dimension,
            region_for(target.x, target.z),
            target.x,
            target.z
        );
        for &kind in &kinds {
            let scratch = tempfile::tempdir().map_err(|error| {
                Error::Gate(format!(
                    "generated-full tamper scratch directory failed: {error}"
                ))
            })?;
            let scratch_root = scratch.path().join("rivet");
            snapshot_tree(&rivet.root, &scratch_root)?;
            let scratch_target = scratch_root.join(&target_relative);
            let bytes = read_stable_file(&scratch_target, "generated-full tamper target")?;
            let tampered = mutate::tamper(&bytes, kind).map_err(Error::Gate)?;
            fs::write(&scratch_target, tampered).map_err(Error::Io)?;
            match raw_manifest_from_snapshot(contract, &scratch_root, seed, "rivet") {
                Err(Error::Gate(message)) if kind == TamperKind::LastUpdate => {
                    if !message.contains("LastUpdate") {
                        return Err(Error::NegativeControl { message });
                    }
                }
                Err(error) => return Err(error),
                Ok(tampered_manifest) => {
                    let mismatches =
                        manifest_mismatches(contract, seed, paper_manifest, &tampered_manifest);
                    if mismatches.len() != 1 || mismatches[0].coordinate != target.tuple() {
                        return Err(Error::NegativeControl {
                            message: format!(
                                "generated-full {} tamper was not the only mismatch at overworld/{}.{}",
                                kind.cli_name(),
                                target.x,
                                target.z
                            ),
                        });
                    }
                }
            }
            println!(
                "tamper control PASS: {} changed only seed={} overworld/{}.{}",
                kind.cli_name(),
                seed,
                target.x,
                target.z
            );
        }
    }
    Ok(())
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

    fn stage(&self, replay_root: &Path) -> Result<StagedArtifacts, Error> {
        let artifacts_root = replay_root.join("artifacts");
        fs::create_dir(&artifacts_root).map_err(Error::Io)?;
        let inputs_root = replay_root.join("inputs");
        let staged_paper_jar = artifacts_root.join("paper-26.2.jar");
        let staged_paper_properties = artifacts_root.join("server-normal-full.properties");
        let staged_rivet_binary = artifacts_root.join("rivet-generated-full");
        let staged_rivet_config = inputs_root.join("rivet-generated-full-config.json");

        let mut paper_jar = open_stable_regular(
            &self.paper_jar,
            "generated-full expected Paper materialized jar",
        )?;
        let paper_jar_bytes = read_opened_file_capped(
            &mut paper_jar,
            &self.paper_jar,
            "generated-full Paper materialized jar",
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        if paper_jar_bytes.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full Paper materialized jar {} is empty; an existing artifact is malformed",
                self.paper_jar.display()
            )));
        }
        let paper_commit = read_jar_git_commit_opened(&paper_jar)
            .map_err(|error| {
                Error::Gate(format!(
                    "generated-full Paper jar attestation failed for existing artifact: {error}"
                ))
            })?
            .ok_or_else(|| {
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
        write_fresh_file(
            &staged_paper_jar,
            &paper_jar_bytes,
            "staged Paper materialized jar",
        )?;
        drop(paper_jar);

        let mut rivet_binary =
            open_stable_regular(&self.rivet_binary, "generated-full Rivet producer binary")?;
        let rivet_binary_metadata = rivet_binary.metadata().map_err(|error| {
            Error::Gate(format!(
                "Rivet producer binary {} cannot be inspected through its opened descriptor: {error}",
                self.rivet_binary.display()
            ))
        })?;
        #[cfg(unix)]
        let rivet_binary_mode = {
            use std::os::unix::fs::PermissionsExt;
            if rivet_binary_metadata.permissions().mode() & 0o111 == 0 {
                return Err(Error::Gate(format!(
                    "Rivet producer binary {} is not executable; existing artifact is malformed",
                    self.rivet_binary.display()
                )));
            }
            rivet_binary_metadata.permissions().mode()
        };
        let rivet_binary_bytes = read_opened_file_capped(
            &mut rivet_binary,
            &self.rivet_binary,
            "Rivet producer binary",
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        if rivet_binary_bytes.is_empty() {
            return Err(Error::Gate(format!(
                "Rivet producer binary {} is empty; an existing artifact is malformed",
                self.rivet_binary.display()
            )));
        }
        write_fresh_file(
            &staged_rivet_binary,
            &rivet_binary_bytes,
            "staged Rivet producer binary",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::symlink_metadata(&staged_rivet_binary)
                .map_err(Error::Io)?
                .permissions();
            permissions.set_mode(rivet_binary_mode);
            fs::set_permissions(&staged_rivet_binary, permissions).map_err(Error::Io)?;
        }
        drop(rivet_binary);

        let mut rivet_config =
            open_stable_regular(&self.rivet_config, "generated-full Rivet capture config")?;
        let rivet_config_bytes = read_opened_file_capped(
            &mut rivet_config,
            &self.rivet_config,
            "generated-full Rivet capture config",
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        if rivet_config_bytes.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full Rivet capture config {} is empty; an existing artifact is malformed",
                self.rivet_config.display()
            )));
        }
        write_fresh_file(
            &staged_rivet_config,
            &rivet_config_bytes,
            "staged Rivet capture config",
        )?;
        drop(rivet_config);

        let paper_config_template = read_canonical_paper_properties(&self.paper_properties)?;
        write_fresh_file(
            &staged_paper_properties,
            &paper_config_template,
            "staged Paper capture properties",
        )?;
        let paper_jar_sha256 = crate::sha256_hex(&paper_jar_bytes);
        let paper_properties_sha256 = crate::sha256_hex(&paper_config_template);
        let rivet_binary_sha256 = crate::sha256_hex(&rivet_binary_bytes);
        let rivet_config_sha256 = crate::sha256_hex(&rivet_config_bytes);
        let paper_jar = open_staged_file(
            staged_paper_jar,
            "generated-full staged Paper materialized jar",
            &paper_jar_sha256,
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        let paper_properties = open_staged_file(
            staged_paper_properties,
            "generated-full staged Paper capture properties",
            &paper_properties_sha256,
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        let rivet_binary = open_staged_file(
            staged_rivet_binary,
            "generated-full staged Rivet producer binary",
            &rivet_binary_sha256,
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        let rivet_config = open_staged_file(
            staged_rivet_config,
            "generated-full staged Rivet capture config",
            &rivet_config_sha256,
            MAX_EVIDENCE_FILE_BYTES,
        )?;
        Ok(StagedArtifacts {
            paper_jar,
            paper_properties,
            rivet_binary,
            rivet_config,
            identity: ArtifactIdentity {
                paper_commit: hash_manifest::PAPER_PIN.to_string(),
                materialized_jar_sha256: paper_jar_sha256,
                paper_config_template_sha256: paper_properties_sha256,
                paper_config_template,
                rivet_commit: self.rivet_commit.clone(),
                capture_binary_sha256: rivet_binary_sha256,
                capture_config_sha256: rivet_config_sha256,
            },
        })
    }
}

/// Verify every declared seed using the default source-disjoint tree layout.
pub fn verify_default() -> Result<(), Error> {
    let fixture_root = crate::crate_dir().join("fixtures/generated-full");
    verify_fixture_outer_closure(&fixture_root)?;
    let contract = load_contract(&fixture_root.join(CONTRACT_BASENAME))?;
    let artifacts = ArtifactInputs::from_contract(&contract)?;
    run_fresh_replay(&contract, &artifacts)
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

#[cfg(test)]
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
    // The fixture directory is contract/config only.  Paper and Rivet output
    // are allocated by the verifier under a fresh replay nonce; a committed
    // or caller-selected side tree is never accepted as production evidence.
    let expected = BTreeSet::from([
        CONTRACT_BASENAME.to_string(),
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
        }
        actual.insert(name);
    }
    if actual != expected {
        let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(Error::Gate(format!(
            "generated-full fixture root has partial closure; missing declared entries {missing:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
fn canonical_side_path(path: &Path) -> Result<PathBuf, Error> {
    reject_symlink_components(path, "generated-full side path")?;
    path.canonicalize().map_err(|e| {
        Error::Unverified(format!(
            "generated-full side {} is absent or cannot be resolved: {e}",
            path.display()
        ))
    })
}

#[cfg(test)]
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
        return Err(Error::Gate(format!(
            "generated-full {side} root has partial closure; missing declared seed directories {missing:?}",
        )));
    }
    Ok(())
}

#[cfg(test)]
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
    // A seed directory that exists but is missing any handoff member is partial
    // evidence, not an absent prerequisite. Inspect the exact root closure before
    // opening the members so missing files cannot be downgraded to UNVERIFIED.
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
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial closure; missing declared root entries {missing:?}",
        )));
    }
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
    let manifest_path = root.join(MANIFEST_BASENAME);
    reject_symlink_components(&manifest_path, "generated-full manifest")?;
    reject_symlink_if_present(&manifest_path, "generated-full manifest")?;
    if !manifest_path.exists() {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial evidence: no {MANIFEST_BASENAME}",
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
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial evidence: no FULL payloads",
        )));
    }

    // Check each payload's internal position before deriving hashes. A
    // relabeled file must not become a valid coordinate merely because its
    // filename is in the expected closure. LastUpdate is the only
    // canonicalization invariant for this harness: captures must serialize an
    // explicit root `LastUpdate` long with value 0, and no other normalization
    // is applied before byte-level comparison.
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

#[cfg(test)]
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

#[cfg(test)]
fn read_seed_config(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
) -> Result<String, Error> {
    let path = root.join(SEED_CONFIG_BASENAME);
    let bytes = read_stable_file_capped(
        &path,
        "generated-full seed config",
        crate::json::MAX_JSON_BYTES as u64,
    )?;
    let config: SeedConfig = crate::json::from_slice(&bytes).map_err(|e| {
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

#[cfg(test)]
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
    let raw = read_stable_file_capped(
        &path,
        "generated-full provenance",
        crate::json::MAX_JSON_BYTES as u64,
    )?;
    let raw = std::str::from_utf8(&raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full {side} seed-{seed} provenance {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    let provenance: GeneratedProvenance = crate::json::from_slice(raw.as_bytes()).map_err(|e| {
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

fn discover_payloads(
    root: &Path,
    contract: &GeneratedContract,
    seed: u64,
    side: &str,
) -> Result<Vec<PayloadFile>, Error> {
    let chunk_root = root.join("chunk");
    // A broken chunk symlink is malformed existing output; a missing chunk tree
    // below an existing seed handoff is partial evidence and must FAIL.
    reject_symlink_components(&chunk_root, "generated-full chunk root")?;
    reject_symlink_if_present(&chunk_root, "generated-full chunk root")?;
    if !chunk_root.exists() {
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial evidence: no chunk/ payload tree",
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

    let mut total_payload_bytes = 0u64;
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
                let bytes = read_stable_file_capped(
                    &path,
                    "generated-full nested payload",
                    MAX_PAYLOAD_FILE_BYTES,
                )?;
                if discovered.len() >= MAX_EVIDENCE_ENTRIES {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} has more than {MAX_EVIDENCE_ENTRIES} payload entries"
                    )));
                }
                total_payload_bytes = total_payload_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| {
                        Error::Gate(format!(
                            "generated-full {side} seed-{seed} payload byte count overflowed"
                        ))
                    })?;
                if total_payload_bytes > hash_manifest::MAX_TOTAL_PAYLOAD_BYTES as u64 {
                    return Err(Error::Gate(format!(
                        "generated-full {side} seed-{seed} payloads exceed the {}-byte cap",
                        hash_manifest::MAX_TOTAL_PAYLOAD_BYTES
                    )));
                }
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
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial closure: missing declared dimension {}",
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
        // A missing region is an incomplete producer output. Extra regions
        // were rejected above as malformed artifacts; this branch names the
        // missing closure as a hard parity failure.
        let missing = expected_regions
            .difference(&actual_region_refs)
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(Error::Gate(format!(
                "generated-full {side} seed-{seed} has partial closure: missing declared regions {missing:?}",
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
        return Err(Error::Gate(format!(
            "generated-full {side} seed-{seed} has partial payload closure; missing declared paths: {}",
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
/// or link-substitution window. `O_NONBLOCK` ensures a FIFO cannot stall before
/// its descriptor metadata identifies it as a forbidden nonregular artifact.
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

#[cfg(unix)]
fn stable_metadata_fingerprint(
    metadata: &fs::Metadata,
) -> (u64, u64, u64, u32, u64, i64, i64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (
        metadata.len(),
        metadata.dev(),
        metadata.ino(),
        metadata.mode(),
        metadata.nlink(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

#[cfg(not(unix))]
fn stable_metadata_fingerprint(metadata: &fs::Metadata) -> (u64, bool, Option<SystemTime>) {
    (
        metadata.len(),
        metadata.permissions().readonly(),
        metadata.modified().ok(),
    )
}

fn verify_path_matches_descriptor(
    path: &Path,
    descriptor_metadata: &fs::Metadata,
    what: &str,
) -> Result<(), Error> {
    let path_metadata = fs::symlink_metadata(path).map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be re-stat'ed after descriptor acquisition: {error}",
            path.display()
        ))
    })?;
    if path_metadata.file_type().is_symlink() {
        return Err(Error::Gate(format!(
            "{what} {} was replaced by a symlink while being read",
            path.display()
        )));
    }
    if !path_metadata.file_type().is_file()
        || stable_metadata_fingerprint(&path_metadata)
            != stable_metadata_fingerprint(descriptor_metadata)
    {
        return Err(Error::Gate(format!(
            "{what} {} was replaced or changed while being read; descriptor and pathname metadata differ",
            path.display()
        )));
    }
    Ok(())
}

fn verify_directory_path_matches_descriptor(
    path: &Path,
    descriptor_metadata: &fs::Metadata,
    what: &str,
) -> Result<(), Error> {
    let path_metadata = fs::symlink_metadata(path).map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be re-stat'ed after descriptor acquisition: {error}",
            path.display()
        ))
    })?;
    if path_metadata.file_type().is_symlink()
        || !path_metadata.file_type().is_dir()
        || stable_metadata_fingerprint(&path_metadata)
            != stable_metadata_fingerprint(descriptor_metadata)
    {
        return Err(Error::Gate(format!(
            "{what} {} was replaced or changed while being read; descriptor and pathname metadata differ",
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
    verify_path_matches_descriptor(path, &metadata, what)?;
    Ok(file)
}

/// Acquire, type-check, hardlink-check, and consume one regular evidence file
/// through the same opened descriptor. Missing files retain the existing
/// UNVERIFIED classification; every present nonregular/link/error state is a
/// hard failure. The descriptor size is checked before allocation.
fn read_stable_file(path: &Path, what: &str) -> Result<Vec<u8>, Error> {
    read_stable_file_capped(path, what, MAX_EVIDENCE_FILE_BYTES)
}

pub(crate) fn read_stable_file_capped(path: &Path, what: &str, cap: u64) -> Result<Vec<u8>, Error> {
    let mut file = open_stable_regular(path, what)?;
    read_opened_file_capped(&mut file, path, what, cap)
}

fn read_opened_file_capped(
    file: &mut fs::File,
    path: &Path,
    what: &str,
    cap: u64,
) -> Result<Vec<u8>, Error> {
    let initial = file.metadata().map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be inspected through its opened descriptor: {error}",
            path.display()
        ))
    })?;
    verify_path_matches_descriptor(path, &initial, what)?;
    let initial_len = initial.len();
    if initial_len > cap {
        return Err(Error::Gate(format!(
            "{what} {} is {} bytes, above the {}-byte cap",
            path.display(),
            initial_len,
            cap
        )));
    }
    let mut bytes = vec![0; initial_len as usize];
    file.read_exact(&mut bytes).map_err(|error| {
        Error::Gate(format!(
            "{what} {} was truncated while being read at its initial {}-byte length: {error}",
            path.display(),
            initial_len
        ))
    })?;
    let final_descriptor = file.metadata().map_err(|error| {
        Error::Gate(format!(
            "{what} {} cannot be re-inspected through its opened descriptor: {error}",
            path.display()
        ))
    })?;
    if stable_metadata_fingerprint(&final_descriptor) != stable_metadata_fingerprint(&initial)
        || final_descriptor.len() != initial_len
    {
        return Err(Error::Gate(format!(
            "{what} {} changed while being read; descriptor metadata was not stable",
            path.display()
        )));
    }
    verify_path_matches_descriptor(path, &final_descriptor, what)?;
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
/// closure contract.  Hardlink checks are available on Unix; stable evidence
/// consumption itself remains explicitly Linux-only (see `open_stable_read`).
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

#[cfg(test)]
fn require_executable(path: &Path) -> Result<(), Error> {
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
    let bytes = read_opened_file_capped(
        &mut file,
        path,
        "generated-full Rivet producer binary",
        MAX_EVIDENCE_FILE_BYTES,
    )?;
    if bytes.is_empty() {
        return Err(Error::Gate(format!(
            "Rivet producer binary {} is empty; existing artifact is malformed",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
fn read_manifest(path: &Path) -> Result<HashManifest, Error> {
    let raw = read_stable_file_capped(
        path,
        "generated-full manifest",
        crate::json::MAX_JSON_BYTES as u64,
    )?;
    let raw = std::str::from_utf8(&raw).map_err(|e| {
        Error::Gate(format!(
            "generated-full manifest {} is not UTF-8: {e}",
            path.display()
        ))
    })?;
    crate::json::from_slice(raw.as_bytes()).map_err(|e| {
        Error::Gate(format!(
            "generated-full manifest {} is malformed: {e}",
            path.display()
        ))
    })
}

#[cfg(test)]
fn compare_seed(
    contract: &GeneratedContract,
    seed: u64,
    paper: &VerifiedSide,
    rivet: &VerifiedSide,
) -> Result<(), Error> {
    // Producer identities are intentionally side-disjoint. The controller
    // attests each executable independently; comparing a Paper jar hash with a
    // Rivet binary hash would turn equality into a false proof of parity.
    if paper.provenance.seed_u64 != rivet.provenance.seed_u64
        || paper.provenance.seed_java_long != rivet.provenance.seed_java_long
        || paper.provenance.level_type != rivet.provenance.level_type
        || paper.provenance.dimension != rivet.provenance.dimension
        || paper.provenance.region_file_compression != rivet.provenance.region_file_compression
        || paper.provenance.corpus_version != rivet.provenance.corpus_version
        || paper.provenance.status != rivet.provenance.status
        || paper.provenance.stage != rivet.provenance.stage
        || paper.provenance.regions != rivet.provenance.regions
        || paper.provenance.coordinates != rivet.provenance.coordinates
        || paper.provenance.chunk_concurrency != rivet.provenance.chunk_concurrency
        || paper.provenance.normalization_rule != rivet.provenance.normalization_rule
        || paper.provenance.seed_config_sha256 != rivet.provenance.seed_config_sha256
    {
        return Err(Error::Gate(format!(
            "generated-full seed {seed} Paper/Rivet shared seed/config/corpus/coordinate contract differs"
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

#[cfg(test)]
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

#[cfg(test)]
fn expected_payload_path(contract: &GeneratedContract, coord: &Coordinate) -> PathBuf {
    PathBuf::from(format!(
        "chunk/{}/{}/{}.{}.nbt",
        contract.dimension,
        region_for(coord.x, coord.z),
        coord.x,
        coord.z
    ))
}

#[cfg(test)]
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
    let mut tamper = None;
    let mut refresh_determinism = false;
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
            "--refresh-determinism" => {
                refresh_determinism = true;
                i += 1;
            }
            "--paper" | "--rivet" => {
                return Err(Error::Gate(
                    "generated-full production verification controls its own fresh Paper/Rivet roots; arbitrary evidence roots are forbidden".into(),
                ));
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
    if let Some(selected) = tamper {
        return run_tamper_from_latest(&contract, selected);
    }
    let artifacts = ArtifactInputs::from_contract(&contract)?;
    if refresh_determinism {
        run_fresh_replay_with_paper_boots(&contract, &artifacts, 3)
    } else {
        run_fresh_replay(&contract, &artifacts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_contract() -> GeneratedContract {
        canonical_contract()
    }

    /// A genuine `--all-regions` Paper extraction carries every saved chunk in
    /// the four origin-adjacent regions, not just the eight forced corpus
    /// coordinates. The prune must reduce that tree to the exact contract
    /// closure (dropping off-contract payloads and emptying non-covered
    /// regions) while keeping every on-contract payload byte-identical.
    #[test]
    fn prune_to_contract_closure_keeps_only_contract_payloads() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("output");
        // Contract payloads plus genuine-boot extras: spawn-area chunks and a
        // whole region directory the contract does not cover.
        for coordinate in &contract.coordinates {
            let region = region_for(coordinate.x, coordinate.z);
            let dir = root.join("chunk").join(&contract.dimension).join(&region);
            fs::create_dir_all(&dir).unwrap();
            let payload = mutate::fixture_full_payload_with_seed(coordinate.x, coordinate.z, 1);
            fs::write(
                dir.join(format!("{}.{}.nbt", coordinate.x, coordinate.z)),
                payload.clone(),
            )
            .unwrap();
        }
        let dim_root = root.join("chunk").join(&contract.dimension);
        let spawn_dir = dim_root.join("0.0");
        fs::write(spawn_dir.join("4.7.nbt"), b"spawn-area extra chunk").unwrap();
        fs::write(spawn_dir.join("-5.12.nbt"), b"spawn-area extra chunk 2").unwrap();
        let other_region = dim_root.join("1.0");
        fs::create_dir_all(&other_region).unwrap();
        fs::write(other_region.join("32.0.nbt"), b"outside declared closure").unwrap();

        prune_to_contract_closure(&root, &contract).unwrap();

        for coordinate in &contract.coordinates {
            let path = dim_root
                .join(region_for(coordinate.x, coordinate.z))
                .join(format!("{}.{}.nbt", coordinate.x, coordinate.z));
            assert_eq!(
                fs::read(&path).unwrap(),
                mutate::fixture_full_payload_with_seed(coordinate.x, coordinate.z, 1),
                "on-contract payload {path:?} must survive the prune unchanged"
            );
        }
        assert!(!spawn_dir.join("4.7.nbt").exists());
        assert!(!spawn_dir.join("-5.12.nbt").exists());
        assert!(!other_region.exists(), "emptied region dir is removed");
    }

    /// A malformed extraction must still fail loudly: the prune runs before
    /// discovery, but it never deletes or rewrites an on-contract payload, so
    /// tampered bytes still reach the verifier's exactly-one-mismatch gate.
    #[test]
    fn prune_preserves_on_contract_bytes_for_verifier_discrimination() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("output");
        let target = contract.coordinates.first().unwrap().clone();
        let dir = root
            .join("chunk")
            .join(&contract.dimension)
            .join(region_for(target.x, target.z));
        fs::create_dir_all(&dir).unwrap();
        let original = mutate::fixture_full_payload_with_seed(target.x, target.z, 1);
        fs::write(
            dir.join(format!("{}.{}.nbt", target.x, target.z)),
            &original,
        )
        .unwrap();
        fs::write(dir.join("9.9.nbt"), b"extra").unwrap();

        prune_to_contract_closure(&root, &contract).unwrap();

        assert_eq!(
            fs::read(dir.join(format!("{}.{}.nbt", target.x, target.z))).unwrap(),
            original,
            "the prune must be byte-preserving for kept payloads"
        );
        assert!(!dir.join("9.9.nbt").exists());
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
    fn nested_snapshot_entry_and_byte_caps_are_aggregate() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("left")).unwrap();
        fs::create_dir_all(source.join("right")).unwrap();
        fs::write(source.join("left/payload"), b"abc").unwrap();
        fs::write(source.join("right/payload"), b"def").unwrap();

        let entry_result = snapshot_tree_with_limits_for_test(
            &source,
            &temp.path().join("entry-snapshot"),
            3,
            100,
        );
        assert!(
            matches!(entry_result, Err(Error::Gate(message)) if message.contains("aggregate cap"))
        );

        let byte_result =
            snapshot_tree_with_limits_for_test(&source, &temp.path().join("byte-snapshot"), 100, 5);
        assert!(
            matches!(byte_result, Err(Error::Gate(message)) if message.contains("aggregate cap"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn producer_output_root_requires_fresh_directory_and_cannot_write_external_tree() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        require_fresh_output_root(&output, "test producer output").unwrap();
        fs::create_dir(&output).unwrap();
        assert!(matches!(
            require_fresh_output_root(&output, "test producer output"),
            Err(Error::Gate(message)) if message.contains("already exists")
        ));

        let external = temp.path().join("external");
        fs::create_dir(&external).unwrap();
        fs::remove_dir(&output).unwrap();
        std::os::unix::fs::symlink(&external, &output).unwrap();
        let result = write_seed_config(&output, &contract, 42);
        assert!(
            matches!(result, Err(Error::Gate(message)) if message.contains("without following links"))
        );
        assert!(!external.join(SEED_CONFIG_BASENAME).exists());
    }

    #[test]
    fn lifecycle_accepts_only_clean_paper_or_strict_producer_outcomes() {
        validate_expected_lifecycle(
            7,
            143,
            b"READY\nAll dimensions are saved\n",
            0,
            ExpectedLifecycle::PaperClean,
        )
        .unwrap();
        validate_expected_lifecycle(
            7,
            0,
            b"READY\nAll dimensions are saved\n",
            0,
            ExpectedLifecycle::PaperClean,
        )
        .unwrap();
        assert!(matches!(
            validate_expected_lifecycle(7, 1, b"READY\nAll dimensions are saved\n", 0, ExpectedLifecycle::PaperClean),
            Err(Error::Gate(message)) if message.contains("exit 0 or 143")
        ));
        assert!(matches!(
            validate_expected_lifecycle(7, 143, b"READY\n", 0, ExpectedLifecycle::PaperClean),
            Err(Error::Gate(message)) if message.contains("clean-save marker")
        ));
        assert!(matches!(
            validate_expected_lifecycle(8, 4, b"RIVET_GENERATED_FULL_BLOCKED: not wired\n", 0, ExpectedLifecycle::ProducerExit(0)),
            Err(Error::Blocked(message)) if message.contains("not wired")
        ));
        assert!(matches!(
            validate_expected_lifecycle(8, 143, b"READY\n", 0, ExpectedLifecycle::ProducerExit(0)),
            Err(Error::Gate(message)) if message.contains("expected dedicated exit 0")
        ));
    }

    #[test]
    fn replay_schema_names_revision_identity_digests_without_source_aliases() {
        let observation = PaperObserved {
            schema: "paper-observed-v1".into(),
            seed: 42,
            root: "/tmp/paper".into(),
            argv: Vec::new(),
            cwd: "/tmp".into(),
            env: Default::default(),
            paper_jar_sha256: "0".repeat(64),
            paper_config_sha256: "1".repeat(64),
            paper_revision_identity_sha256: "2".repeat(64),
            pid: 1,
            started_unix_nanos: 1,
            ready_count: 1,
            stopped_unix_nanos: 2,
            exit_code: 143,
            raw_log_sha256: "3".repeat(64),
            payload_digests: Vec::new(),
            producer_manifest_sha256: None,
        };
        let value = serde_json::to_value(observation).unwrap();
        let object = value.as_object().unwrap();
        assert!(object.contains_key("paper-revision-identity-sha256"));
        assert!(!object.contains_key("paper-source-sha256"));
    }

    #[test]
    fn equal_capture_boot_manifests_pass_determinism_refresh() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join("rivet")
            .join(contract.seeds[0].to_string());
        write_tree(&root, &contract, "rivet", contract.seeds[0], 42);
        let manifest = read_manifest(&root.join(MANIFEST_BASENAME)).unwrap();
        let result =
            verify_capture_determinism(42, &[(1, manifest.clone()), (2, manifest.clone())]);
        assert_eq!(result.unwrap(), manifest);
    }

    #[test]
    fn divergent_capture_boot_manifests_fail_determinism_refresh() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join("rivet")
            .join(contract.seeds[0].to_string());
        write_tree(&root, &contract, "rivet", contract.seeds[0], 42);
        let first = read_manifest(&root.join(MANIFEST_BASENAME)).unwrap();
        let mut second = first.clone();
        second.entries[0].sha256 = "f".repeat(64);
        let error = verify_capture_determinism(42, &[(1, first), (2, second)]).unwrap_err();
        assert!(error.to_string().contains("capture boots 1 and 2"));
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[test]
    fn stable_evidence_platform_contract_is_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("evidence");
        fs::write(&path, b"stable evidence").unwrap();
        let result = open_stable_read(&path);

        #[cfg(target_os = "linux")]
        {
            let file = result.expect("Linux stable evidence uses openat2");
            assert!(file.metadata().unwrap().file_type().is_file());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let error = result.expect_err("non-Linux stable evidence is explicit unsupported");
            assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            assert!(error.to_string().contains("Linux openat2"));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn paper_clean_lifecycle_accepts_sigterm_143_with_save_marker() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("paper.log");
        let mut command = Command::new("sh");
        command.arg("-c").arg(
            "trap 'echo All dimensions are saved; exit 143' TERM; echo 'Done (0s)!'; while :; do sleep 0.01; done",
        );
        let observation = process_observation(
            &mut command,
            vec!["sh".into(), "-c".into()],
            temp.path(),
            &log,
            "Done (",
            Duration::from_secs(2),
            ExpectedLifecycle::PaperClean,
        )
        .unwrap();
        assert_eq!(observation.exit_code, 143);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dedicated_producer_exit_4_is_blocked_not_clean_failure() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("producer.log");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("echo 'RIVET_GENERATED_FULL_BLOCKED: FULL pipeline unavailable'; exit 4");
        let error = process_observation(
            &mut command,
            vec!["sh".into(), "-c".into()],
            temp.path(),
            &log,
            RIVET_READY_MARKER,
            Duration::from_secs(2),
            ExpectedLifecycle::ProducerExit(0),
        )
        .unwrap_err();
        assert!(
            matches!(error, Error::Blocked(message) if message.contains("FULL pipeline unavailable"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifo_contract_is_a_nonregular_hard_failure_without_blocking() {
        use rustix::fs::{CWD, Mode, mkfifoat};

        let temp = tempfile::tempdir().unwrap();
        let contract_path = temp.path().join(CONTRACT_BASENAME);
        mkfifoat(CWD, &contract_path, Mode::from_raw_mode(0o600)).unwrap();
        assert!(matches!(
            load_contract(&contract_path),
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifo_evidence_fails_fast_as_nonregular() {
        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("evidence");
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
        )
        .unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(read_stable_file(&fifo, "generated-full FIFO evidence"))
                .unwrap();
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("opening a FIFO must not block while checking evidence metadata");
        worker.join().unwrap();
        assert!(matches!(
            result,
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn producer_output_symlink_cannot_write_external_tree() {
        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let output = temp.path().join("producer-output");
        std::os::unix::fs::symlink(external.path(), &output).unwrap();
        assert!(matches!(
            require_fresh_output_root(&output, "generated-full producer output root"),
            Err(Error::Gate(message)) if message.contains("already exists")
        ));
        assert!(write_seed_config(&output, &contract, contract.seeds[0]).is_err());
        assert!(!external.path().join(SEED_CONFIG_BASENAME).exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn snapshot_tree_enforces_aggregate_entry_cap_across_nested_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("left")).unwrap();
        fs::create_dir_all(source.join("right")).unwrap();
        for (dir, offset) in [("left", 0usize), ("right", MAX_EVIDENCE_ENTRIES / 2)] {
            for index in 0..(MAX_EVIDENCE_ENTRIES / 2 + 1) {
                fs::write(source.join(dir).join(format!("{index}").as_str()), [0u8])
                    .unwrap_or_else(|error| panic!("write {} {offset}: {error}", index));
            }
        }
        let error = snapshot_tree(&source, &destination).unwrap_err();
        assert!(error.to_string().contains("aggregate cap"));
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn symmetric_malformed_single_section_evidence_is_rejected() {
        use rivet_nbt::tag::Tag;

        let contract = test_contract();
        let temp = tempfile::tempdir().unwrap();
        let paper_root = temp.path().join("paper");
        let rivet_root = temp.path().join("rivet");
        write_seed_set(&paper_root, &contract, "paper", 42);
        write_seed_set(&rivet_root, &contract, "rivet", 42);

        let seed = contract.seeds[0];
        let relative = expected_payload_path(&contract, &contract.coordinates[0]);
        for root in [&paper_root, &rivet_root] {
            let path = root.join(seed.to_string()).join(&relative);
            let payload = fs::read(&path).unwrap();
            let compound = mutate::parse_payload(&payload).unwrap();
            let interior = compound
                .get_list("sections")
                .unwrap()
                .list
                .iter()
                .find(|tag| {
                    matches!(
                        tag,
                        Tag::Compound(section) if section.tags.contains_key("block_states")
                    )
                })
                .cloned()
                .unwrap();
            let mut malformed = compound;
            malformed.get_list_or_empty_mut("sections").list = vec![interior];
            fs::write(&path, mutate::encode_payload(&malformed).unwrap()).unwrap();
        }

        let error = verify_synthetic_roots(&contract, &paper_root, &rivet_root)
            .expect_err("identical malformed Paper/Rivet evidence must not compare green");
        assert!(
            matches!(error, Error::Gate(ref message) if message.contains("missing required Paper block section")),
            "symmetric malformed evidence must fail in schema validation: {error}"
        );
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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
            Err(Error::Gate(message)) if message.contains("missing declared dimension")
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
        fs::create_dir_all(&root).unwrap();
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
            Err(Error::Gate(message)) if message.contains("server-normal-full.properties")
        ));

        fs::create_dir(root.join("server-normal-full.properties")).unwrap();
        assert!(matches!(
            verify_fixture_outer_closure(&root),
            Err(Error::Gate(message)) if message.contains("not a regular file")
        ));
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn identical_worldgen_content_is_not_a_parity_proof() {
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
            Err(Error::Gate(message)) if message.contains("parity FAIL")
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

    /// Counterfactual: if the verifier reopened an identity-checked artifact by
    /// pathname, an attacker could swap the path to different bytes between
    /// hashing and use and the replay would silently execute the substitute.
    /// Staging must capture the attested bytes once through the stable
    /// descriptor so every later consumer sees exactly what was hashed.
    #[test]
    fn staged_artifact_path_swap_after_staging_cannot_change_consumed_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("artifact");
        fs::write(&original, b"attested artifact bytes").unwrap();
        let sha256 = crate::sha256_hex(b"attested artifact bytes");

        // Stage through the real acquisition path: stable open, capped read,
        // hash binding against the expected digest.
        let staged = open_staged_file(
            original.clone(),
            "generated-full staged counterfactual artifact",
            &sha256,
            MAX_EVIDENCE_FILE_BYTES,
        )
        .unwrap();
        assert_eq!(staged.bytes, b"attested artifact bytes");

        // Path-swap attack: replace the staged pathname with different bytes
        // (and even with a symlink to yet another file) after identity was
        // bound. The captured bytes must be untouched — no reopen happens.
        fs::remove_file(&original).unwrap();
        fs::write(&original, b"substituted bytes").unwrap();
        assert_eq!(staged.bytes, b"attested artifact bytes");

        let decoy = temp.path().join("decoy");
        fs::write(&decoy, b"decoy bytes").unwrap();
        fs::remove_file(&original).unwrap();
        std::os::unix::fs::symlink(&decoy, &original).unwrap();
        assert_eq!(staged.bytes, b"attested artifact bytes");
    }

    /// Counterfactual: a staged file whose on-path contents diverge from the
    /// attestation digest must fail staging outright instead of executing or
    /// comparing against unattested material.
    #[cfg(target_os = "linux")]
    #[test]
    fn open_staged_file_fails_when_staged_contents_diverge_from_attestation() {
        let temp = tempfile::tempdir().unwrap();
        let staged = temp.path().join("staged-artifact");
        fs::write(&staged, b"not what was attested").unwrap();

        let result = open_staged_file(
            staged,
            "generated-full staged divergent artifact",
            &crate::sha256_hex(b"attested"),
            MAX_EVIDENCE_FILE_BYTES,
        );
        assert!(
            matches!(result, Err(Error::Gate(message)) if message.contains("changed while staging"))
        );
    }

    /// Counterfactual for the stable-read primitive itself: swapping the
    /// pathname onto different bytes between descriptor acquisition and the
    /// post-read re-stat must be detected as a path/descriptor mismatch, not
    /// silently consumed as evidence of the original file.
    #[cfg(target_os = "linux")]
    #[test]
    fn stable_read_rejects_pathname_swapped_to_different_file_mid_flight() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("swappable");
        fs::write(&target, b"genuine evidence").unwrap();

        // Acquire the descriptor exactly as open_stable_regular does, then
        // swap the pathname before verify_path_matches_descriptor would run.
        let mut file = open_stable_regular(&target, "generated-full swap probe").unwrap();
        fs::remove_file(&target).unwrap();
        fs::write(&target, b"tampered evidence").unwrap();
        let result = read_opened_file_capped(&mut file, &target, "generated-full swap probe", 1024);
        assert!(matches!(
            result,
            Err(Error::Gate(message))
                if message.contains("replaced or changed") || message.contains("was truncated")
                    || message.contains("changed while being read")
        ));
    }
}
