//! The #54 chunk-hash manifest: `HashManifest` (serde) + build/coverage logic.
//!
//! A `HashManifest` records every chunk's xxh3_64 (and sha256, for cross-check)
//! over its **raw serialized Level-compound payload**, plus the provenance that
//! makes the digest meaningful:
//!
//! - `hash-algorithm: "xxh3_64"` — the digest family (guards variant drift);
//! - `seed` — the world seed the capture was generated under;
//! - `level-type` / `region-file-compression: "none"` — the generator + region
//!   framing that produced the payloads;
//! - `paper` — the pinned Paper commit (provenance drift threat 9);
//! - `chunk-concurrency` — the pinned 1/1 worker/I-O threads (#266);
//! - `corpus-version` — which corpus revision the target digest table follows;
//! - `hash-scope: "payload-only"` — the digest is over the decompressed chunk
//!   NBT, region framing (offset tables, timestamps, sector padding) excluded.
//!
//! `build_from_payloads` reads committed `.nbt` fixtures with the rivet-nbt
//! codec and **stamps `status` from the root `Status` string** — a chunk is
//! never assumed FULL; it is recorded as whatever status its payload says. The
//! comparator compares only FULL entries, and non-FULL entries are recorded and
//! reported (the committed-fixture trap: the M2 region capture has exactly 2
//! genuine FULL chunks today — the_nether/0.0 and the_end/0.0; overworld has
//! zero). Coverage is always reported against the corpus, never assumed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::corpus;
use crate::hash::xxh3_64_hex;
use crate::mutate::parse_payload;

/// Algorithm string recorded in every manifest (the gate self-check pins the
/// exact xxh3_64 vectors).
pub const HASH_ALGORITHM: &str = "xxh3_64";
/// Digest scope: raw serialized payload bytes, region framing excluded.
pub const HASH_SCOPE: &str = "payload-only";
/// The committed M2 region capture's fixed world seed — the seed the 408
/// payloads in `fixtures/regions/overworld-normal` were generated under. It is
/// the *working* seed for the single committed capture and is distinct from
/// `corpus::corpus_seeds()`: those are the pinned #175 sweep targets a future
/// live generation must reach. Every rebuilt hash manifest records whichever
/// seed its fixture tree was generated under (read from the source manifest),
/// never a magic literal.
pub const CAPTURE_SEED: &str = "42";
/// Corpus revision this digest table follows (bump on a seed/coordinate
/// corpus change so stale tables refuse to compare).
pub const CORPUS_VERSION: &str = "v1";
/// The Paper commit the committed Paper manifest was captured against.
pub const PAPER_PIN: &str = "0a99345";

/// Evidence caps are part of the verifier contract.  A producer cannot make
/// the controller allocate unbounded memory by declaring an enormous payload
/// or by repeating coordinates indefinitely.
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PAYLOAD_COUNT: usize = 8192;
pub const MAX_TOTAL_PAYLOAD_BYTES: usize = 512 * 1024 * 1024;

/// A single chunk's digests in a `HashManifest`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChunkHashEntry {
    pub dim: String,
    pub region: String,
    pub cx: i32,
    pub cz: i32,
    /// The chunk's root `Status` string as recorded in its payload (stamped,
    /// never assumed).
    pub status: String,
    /// Raw serialized payload length.
    pub bytes: usize,
    pub xxh3_64: String,
    pub sha256: String,
    /// Canonical (order-insensitive) digest — triage only.
    pub xxh3_64_canonical: String,
}

impl ChunkHashEntry {
    /// Whether this entry is a FULL chunk (root `Status` == `minecraft:full`).
    pub fn is_full(&self) -> bool {
        is_full_status(&self.status)
    }
}

/// Normalized FULL predicate over the root `Status` string. `build_from_payloads`
/// stamps the raw root `Status`, so every consumer — build, coverage, the
/// comparator — decides FULL through this one function, never a raw string
/// compare that could silently drop chunks and turn the diff vacuously green.
///
/// Paper's `ChunkStatus.CODEC` is `BuiltInRegistries.CHUNK_STATUS.byNameCodec()`,
/// whose `toString` is `namespace + ":" + path`; the FULL status is registered
/// as `register("full", ...)`, so Paper always serializes `minecraft:full` and
/// never a bare `full`. A bare `full` therefore never comes from Paper — it is
/// an off-spec serialization and must be refused (a malformed tree must fail
/// loudly, never be silently treated as FULL and compared against Paper).
fn is_full_status(status: &str) -> bool {
    status == "minecraft:full"
}

/// Reject a manifest whose entries carry a duplicate (dim, cx, cz). The
/// verifier derives entries from an exact filename closure, but generic hash
/// callers may provide a manifest-shaped source with mixed statuses. Checking
/// every status prevents a non-FULL duplicate from becoming a later FULL entry
/// through a producer-supplied manifest edit.
pub fn reject_duplicate_coordinates(entries: &[ChunkHashEntry]) -> Result<(), String> {
    let mut seen: std::collections::HashSet<(&str, i32, i32)> = std::collections::HashSet::new();
    for e in entries {
        if !seen.insert((e.dim.as_str(), e.cx, e.cz)) {
            return Err(format!(
                "duplicate chunk coordinate at {}/{}: a malformed capture tree must be rejected, \
                 never silently deduplicated",
                e.dim,
                fmt_coord(e.cx, e.cz)
            ));
        }
    }
    Ok(())
}

/// Compatibility name for callers that only care about the FULL subset. The
/// implementation intentionally checks all entries now.
pub fn reject_duplicate_full(entries: &[ChunkHashEntry]) -> Result<(), String> {
    reject_duplicate_coordinates(entries)
}

/// `<cx>.<cz>` coordinate display used in manifest errors.
fn fmt_coord(cx: i32, cz: i32) -> String {
    format!("{cx}.{cz}")
}

/// A `HashManifest` (the #54 format). Serialized in committed field order so a
/// rebuild is byte-identical.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HashManifest {
    pub format: u64,
    #[serde(rename = "hash-algorithm")]
    pub hash_algorithm: String,
    #[serde(rename = "hash-scope")]
    pub hash_scope: String,
    #[serde(rename = "corpus-version")]
    pub corpus_version: String,
    pub seed: String,
    #[serde(rename = "level-type")]
    pub level_type: String,
    #[serde(rename = "region-file-compression")]
    pub region_file_compression: String,
    pub paper: String,
    #[serde(rename = "chunk-concurrency")]
    pub chunk_concurrency: Concurrency,
    #[serde(rename = "chunk-count")]
    pub chunk_count: usize,
    #[serde(rename = "full-count")]
    pub full_count: usize,
    pub entries: Vec<ChunkHashEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    #[serde(rename = "worker-threads")]
    pub worker_threads: u32,
    #[serde(rename = "io-threads")]
    pub io_threads: u32,
}

impl HashManifest {
    /// Recorded provenance (seed + algorithm + concurrency + paper + scope +
    /// compression + corpus revision) used by the comparator to refuse
    /// comparing manifests of different provenance (threat 9).
    pub fn provenance(&self) -> Provenance {
        Provenance {
            seed: self.seed.clone(),
            hash_algorithm: self.hash_algorithm.clone(),
            hash_scope: self.hash_scope.clone(),
            corpus_version: self.corpus_version.clone(),
            region_file_compression: self.region_file_compression.clone(),
            paper: self.paper.clone(),
            concurrency: self.chunk_concurrency,
            level_type: self.level_type.clone(),
        }
    }

    /// Look up the FULL entry for (dim, cx, cz).
    pub fn full_entry(&self, dim: &str, cx: i32, cz: i32) -> Option<&ChunkHashEntry> {
        self.entries
            .iter()
            .find(|e| e.dim == dim && e.cx == cx && e.cz == cz && e.is_full())
    }
}

/// The provenance identity a diff is only valid within (a subset of
/// `HashManifest` fields — the ones that make digests comparable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub seed: String,
    pub hash_algorithm: String,
    pub hash_scope: String,
    pub corpus_version: String,
    pub region_file_compression: String,
    pub paper: String,
    pub concurrency: Concurrency,
    pub level_type: String,
}

impl Provenance {
    /// Serialize to a stable, human-readable identity string for diff output.
    pub fn describe(&self) -> String {
        format!(
            "seed={} algorithm={} scope={} corpus-version={} compression={} paper={} \
             concurrency={}/{} level-type={}",
            self.seed,
            self.hash_algorithm,
            self.hash_scope,
            self.corpus_version,
            self.region_file_compression,
            self.paper,
            self.concurrency.worker_threads,
            self.concurrency.io_threads,
            self.level_type
        )
    }
}

/// The capture provenance that makes a digest table meaningful, read out of a
/// source region manifest (the same `Manifest` struct `extract_fixtures.py`
/// writes): the world seed, level-type, region-file-compression, and corpus
/// version the payloads were generated under. A hash manifest must record
/// exactly what its source capture recorded — a digest table stamped with a
/// hardcoded compression or corpus revision would silently compare worlds
/// generated under different region framing. `None` fields (a broken or
/// pre-provenance source manifest) are never invented: `build_from_payloads`
/// falls back to the same constants the committed captures use. The world seed
/// is deliberately *not* part of the provenance struct: it names the payloads
/// themselves and flows through `build_from_payloads_with`'s `seed` argument
/// (read from the source capture), so carrying it twice could only drift.
#[derive(Debug, Clone)]
pub struct CaptureProvenance {
    pub level_type: String,
    pub region_file_compression: String,
    pub corpus_version: String,
}

impl Default for CaptureProvenance {
    fn default() -> Self {
        CaptureProvenance {
            level_type: "minecraft\\:normal".to_string(),
            region_file_compression: "none".to_string(),
            corpus_version: CORPUS_VERSION.to_string(),
        }
    }
}

impl CaptureProvenance {
    /// Build the provenance from a source region capture's manifest fields
    /// (the ones `extract_fixtures.py` writes). `None` fields fall back to the
    /// same defaults `build_from_payloads` always used, so a pre-provenance or
    /// broken source never invents values.
    pub fn from_region_manifest(
        level_type: Option<&str>,
        region_file_compression: Option<&str>,
    ) -> CaptureProvenance {
        CaptureProvenance {
            level_type: inherited_level_type(level_type),
            region_file_compression: inherited_compression(region_file_compression),
            corpus_version: CORPUS_VERSION.to_string(),
        }
    }
}

/// `level-type` inherited from a source capture, or the default when absent.
pub fn inherited_level_type(level_type: Option<&str>) -> String {
    level_type
        .filter(|s| !s.is_empty())
        .unwrap_or("minecraft\\:normal")
        .to_string()
}

/// `region-file-compression` inherited from a source capture, or the default
/// (`none`) when absent.
pub fn inherited_compression(compression: Option<&str>) -> String {
    compression
        .filter(|s| !s.is_empty())
        .unwrap_or("none")
        .to_string()
}

/// Build a `HashManifest` from a fixtures tree laid out exactly like the
/// region capture: `chunk/<dim>/<region>/<cx>.<cz>.nbt`. Reads each payload
/// with the rivet-nbt codec, stamps its root `Status`, and records FULL vs
/// non-FULL (threat 2: status is stamped, never assumed).
///
/// `seed` and `level_type` name the world the payloads were generated under
/// (read from the source capture, never magic literals); the hash manifest's
/// `region_file_compression` and `corpus_version` are inherited from
/// `CaptureProvenance` so a digest table always records the framing + corpus
/// revision its source capture used.
///
/// Test-only convenience wrapper (production callers thread explicit provenance
/// through `build_from_payloads_with`): builds under the default provenance.
#[cfg(test)]
pub fn build_from_payloads(
    dir: &Path,
    seed: &str,
    level_type: &str,
) -> Result<HashManifest, String> {
    build_from_payloads_with(dir, seed, level_type, &CaptureProvenance::default())
}

/// Like `build_from_payloads`, but with full capture provenance from a source
/// manifest (`CaptureProvenance::from_region_manifest`). Every FULL payload is
/// additionally checked for the structure a FULL chunk must carry (root
/// `structures`, final heightmaps, `isLightOn`, starlight version — issue #51),
/// so a chunk stamped FULL that Paper did not actually finish is a hard error,
/// never silently compared.
/// Derive a manifest from the raw payload tree, never from a producer-supplied
/// digest table. The optional `manifest.json` at the root is metadata only; all
/// payload names, coordinates, statuses, lengths, xxh3 digests, SHA-256
/// digests, and canonical digests come from the bytes acquired here.
pub fn build_from_raw_tree_with(
    dir: &Path,
    seed: &str,
    level_type: &str,
    provenance: &CaptureProvenance,
) -> Result<HashManifest, String> {
    let chunk_dir = dir.join("chunk");
    let metadata = std::fs::symlink_metadata(&chunk_dir)
        .map_err(|e| format!("cannot inspect {}: {e}", chunk_dir.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "{} is not a regular chunk directory",
            chunk_dir.display()
        ));
    }
    let mut payloads = Vec::new();
    let mut total_payload_bytes = 0usize;
    for dim_entry in sorted_entries(&chunk_dir)? {
        reject_tree_entry(&dim_entry, "dimension")?;
        let dim_path = dim_entry.path();
        if !dim_entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", dim_path.display()))?
            .is_dir()
        {
            return Err(format!(
                "non-directory entry under chunk/: {}",
                dim_path.display()
            ));
        }
        let dim = dim_entry.file_name().to_string_lossy().into_owned();
        for region_entry in sorted_entries(&dim_path)? {
            reject_tree_entry(&region_entry, "region")?;
            let region_path = region_entry.path();
            if !region_entry
                .file_type()
                .map_err(|e| format!("cannot inspect {}: {e}", region_path.display()))?
                .is_dir()
            {
                return Err(format!(
                    "non-directory entry under {}",
                    region_path.display()
                ));
            }
            let region = region_entry.file_name().to_string_lossy().into_owned();
            for file_entry in sorted_entries(&region_path)? {
                reject_tree_entry(&file_entry, "payload")?;
                let path = file_entry.path();
                if !file_entry
                    .file_type()
                    .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
                    .is_file()
                {
                    return Err(format!("non-file payload entry {}", path.display()));
                }
                if path.extension().and_then(|e| e.to_str()) != Some("nbt") {
                    return Err(format!("extra non-NBT payload entry {}", path.display()));
                }
                let (cx, cz) = parse_chunk_filename(&path)?;
                let expected_region = format!("{}.{}", cx.div_euclid(32), cz.div_euclid(32));
                if region != expected_region {
                    return Err(format!(
                        "payload {} is in region {}, expected {}",
                        path.display(),
                        region,
                        expected_region
                    ));
                }
                if payloads.len() >= MAX_PAYLOAD_COUNT {
                    return Err(format!(
                        "raw payload tree {} exceeds the {}-entry cap",
                        chunk_dir.display(),
                        MAX_PAYLOAD_COUNT
                    ));
                }
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
                if metadata.len() > MAX_PAYLOAD_BYTES as u64 {
                    return Err(format!(
                        "payload {} is {} bytes, above the {}-byte cap",
                        path.display(),
                        metadata.len(),
                        MAX_PAYLOAD_BYTES
                    ));
                }
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                total_payload_bytes =
                    total_payload_bytes
                        .checked_add(bytes.len())
                        .ok_or_else(|| {
                            "total payload byte count overflowed the verifier cap".to_string()
                        })?;
                if total_payload_bytes > MAX_TOTAL_PAYLOAD_BYTES {
                    return Err(format!(
                        "raw payload tree {} is {} bytes, above the {}-byte cap",
                        chunk_dir.display(),
                        total_payload_bytes,
                        MAX_TOTAL_PAYLOAD_BYTES
                    ));
                }
                payloads.push(PayloadBytes {
                    dim: dim.clone(),
                    region: region.clone(),
                    cx,
                    cz,
                    bytes,
                });
            }
        }
    }
    if payloads.is_empty() {
        return Err(format!(
            "raw payload tree {} contains no .nbt payloads",
            chunk_dir.display()
        ));
    }
    build_from_payload_bytes_with(&payloads, seed, level_type, provenance)
}

fn sorted_entries(path: &Path) -> Result<Vec<std::fs::DirEntry>, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot enumerate {}: {e}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn reject_tree_entry(entry: &std::fs::DirEntry, kind: &str) -> Result<(), String> {
    let path = entry.path();
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|e| format!("cannot inspect {kind} {}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{kind} {} is a symlink", path.display()));
    }
    #[cfg(unix)]
    if metadata.file_type().is_file() {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(format!("{kind} {} is a hardlink", path.display()));
        }
    }
    Ok(())
}

/// Payload bytes captured from one stable file descriptor. The generated
/// FULL verifier uses this representation after opening each payload with
/// `RESOLVE_NO_SYMLINKS`; hashing the bytes here avoids reopening the path after
/// metadata validation.
#[derive(Debug, Clone)]
pub struct PayloadBytes {
    pub dim: String,
    pub region: String,
    pub cx: i32,
    pub cz: i32,
    pub bytes: Vec<u8>,
}

pub fn build_from_payloads_with(
    dir: &Path,
    seed: &str,
    level_type: &str,
    provenance: &CaptureProvenance,
) -> Result<HashManifest, String> {
    let chunk_dir = dir.join("chunk");
    let mut payloads = Vec::new();
    let mut total_payload_bytes = 0usize;

    if chunk_dir.is_dir() {
        let mut dims: Vec<PathBuf> = std::fs::read_dir(&chunk_dir)
            .map_err(|e| format!("cannot read {}: {e}", chunk_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        dims.sort();
        for dim_dir in dims {
            let dim = dim_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut regions: Vec<PathBuf> = std::fs::read_dir(&dim_dir)
                .map_err(|e| format!("cannot read {}: {e}", dim_dir.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            regions.sort();
            for region_dir in regions {
                let region = region_dir
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut files: Vec<PathBuf> = std::fs::read_dir(&region_dir)
                    .map_err(|e| format!("cannot read {}: {e}", region_dir.display()))?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().map(|e| e == "nbt").unwrap_or(false))
                    .collect();
                files.sort();
                for file in files {
                    if payloads.len() >= MAX_PAYLOAD_COUNT {
                        return Err(format!(
                            "raw payload tree {} exceeds the {}-entry cap",
                            chunk_dir.display(),
                            MAX_PAYLOAD_COUNT
                        ));
                    }
                    let (cx, cz) = parse_chunk_filename(&file)?;
                    let metadata = std::fs::symlink_metadata(&file)
                        .map_err(|e| format!("cannot inspect {}: {e}", file.display()))?;
                    if metadata.len() > MAX_PAYLOAD_BYTES as u64 {
                        return Err(format!(
                            "payload {} is {} bytes, above the {}-byte cap",
                            file.display(),
                            metadata.len(),
                            MAX_PAYLOAD_BYTES
                        ));
                    }
                    let bytes = std::fs::read(&file)
                        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                    total_payload_bytes =
                        total_payload_bytes
                            .checked_add(bytes.len())
                            .ok_or_else(|| {
                                "total payload byte count overflowed the verifier cap".to_string()
                            })?;
                    if total_payload_bytes > MAX_TOTAL_PAYLOAD_BYTES {
                        return Err(format!(
                            "raw payload tree {} is {} bytes, above the {}-byte cap",
                            chunk_dir.display(),
                            total_payload_bytes,
                            MAX_TOTAL_PAYLOAD_BYTES
                        ));
                    }
                    payloads.push(PayloadBytes {
                        dim: dim.clone(),
                        region: region.clone(),
                        cx,
                        cz,
                        bytes,
                    });
                }
            }
        }
    }

    build_from_payload_bytes_with(&payloads, seed, level_type, provenance)
}

/// Build a manifest from already-captured payload bytes.  Callers that acquire
/// evidence through stable file descriptors must use this entry point rather
/// than `build_from_payloads_with`, which intentionally retains its path-based
/// fixture-tree API for the older generic oracle modes.
pub fn build_from_payload_bytes_with(
    payloads: &[PayloadBytes],
    seed: &str,
    level_type: &str,
    provenance: &CaptureProvenance,
) -> Result<HashManifest, String> {
    if payloads.len() > MAX_PAYLOAD_COUNT {
        return Err(format!(
            "payload count {} exceeds verifier cap {}",
            payloads.len(),
            MAX_PAYLOAD_COUNT
        ));
    }
    let total_bytes = payloads.iter().try_fold(0usize, |total, payload| {
        if payload.bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(format!(
                "payload {}/{}/{}.{} is {} bytes, above the {}-byte cap",
                payload.dim,
                payload.region,
                payload.cx,
                payload.cz,
                payload.bytes.len(),
                MAX_PAYLOAD_BYTES
            ));
        }
        total
            .checked_add(payload.bytes.len())
            .ok_or_else(|| "total payload byte count overflowed the verifier cap".to_string())
    })?;
    if total_bytes > MAX_TOTAL_PAYLOAD_BYTES {
        return Err(format!(
            "payload tree is {} bytes, above the {}-byte cap",
            total_bytes, MAX_TOTAL_PAYLOAD_BYTES
        ));
    }

    let mut entries = Vec::with_capacity(payloads.len());
    let mut full_count = 0usize;

    for payload in payloads {
        let expected_region = format!(
            "{}.{}",
            payload.cx.div_euclid(32),
            payload.cz.div_euclid(32)
        );
        if payload.region != expected_region {
            return Err(format!(
                "payload {}/{}/{}.{} is in region {}, expected {}",
                payload.dim,
                payload.region,
                payload.cx,
                payload.cz,
                payload.region,
                expected_region
            ));
        }
        let compound = parse_payload(&payload.bytes)?;
        let stored_coordinates = (compound.get_int("xPos"), compound.get_int("zPos"));
        if stored_coordinates != (Some(payload.cx), Some(payload.cz)) {
            return Err(format!(
                "payload {}/{}/{}.{} stores xPos/zPos {:?}, expected ({}, {})",
                payload.dim,
                payload.region,
                payload.cx,
                payload.cz,
                stored_coordinates,
                payload.cx,
                payload.cz
            ));
        }
        let status = compound
            .get_string("Status")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        if status == "full" {
            return Err(format!(
                "chunk {}/{}/{}.{} has bare root Status `full`: Paper's \
                 `ChunkStatus.CODEC` always serializes the FULL status as \
                 `minecraft:full`, so a bare `full` is an off-spec tree and must \
                 be refused loudly, never silently recorded as non-FULL",
                payload.dim, payload.region, payload.cx, payload.cz
            ));
        }
        if is_full_status(&status) {
            full_count += 1;
            validate_full_payload(&compound, &payload.dim, payload.cx, payload.cz)?;
        }
        entries.push(ChunkHashEntry {
            dim: payload.dim.clone(),
            region: payload.region.clone(),
            cx: payload.cx,
            cz: payload.cz,
            status,
            bytes: payload.bytes.len(),
            xxh3_64: xxh3_64_hex(&payload.bytes),
            sha256: crate::sha256_hex(&payload.bytes),
            xxh3_64_canonical: crate::semantic_hash::canonical_xxh3_64(&compound)?,
        });
    }

    // Deterministic ordering (dim, region, cx, cz) so the committed manifest is
    // byte-stable across rebuilds.
    entries.sort_by(|a, b| {
        (a.dim.as_str(), a.region.as_str(), a.cx, a.cz).cmp(&(
            b.dim.as_str(),
            b.region.as_str(),
            b.cx,
            b.cz,
        ))
    });

    reject_duplicate_full(&entries)?;

    Ok(HashManifest {
        format: 1,
        hash_algorithm: HASH_ALGORITHM.to_string(),
        hash_scope: HASH_SCOPE.to_string(),
        corpus_version: provenance.corpus_version.clone(),
        seed: seed.to_string(),
        level_type: level_type.to_string(),
        region_file_compression: provenance.region_file_compression.clone(),
        paper: format!("26.2-DEV-main@{PAPER_PIN}"),
        chunk_concurrency: Concurrency {
            worker_threads: 1,
            io_threads: 1,
        },
        chunk_count: entries.len(),
        full_count,
        entries,
    })
}

/// `starlight.light_version` written by a light-correct FULL chunk
/// (`SaveUtil.STARLIGHT_LIGHT_VERSION`, 10).
const STARLIGHT_LIGHT_VERSION: i32 = 10;
const CURRENT_DATA_VERSION: i32 = 4903;

/// The packed length of a FINAL heightmap as `long[]`. A heightmap is a
/// `SimpleBitStorage` over 256 entries at 9 bits each: `valuesPerLong =
/// floor(64 / 9) = 7`, so `requiredLength = ceil(256 / 7) = 37` longs. It is
/// *not* `ceil(256 * 9 / 64) = 36` — the per-long packing rounds the divisor
/// before dividing, which is exactly why the 37 is pinned here (verified
/// against the pinned Paper's `SimpleBitStorage` and every committed FULL
/// payload) rather than recomputed inline and silently "simplified" to 36.
const FINAL_HEIGHTMAP_LONGS: usize = 37;

/// The root keys a chunk stamped `minecraft:full` must carry (per the
/// SerializableChunkData spec §2/§5/§6, verified against the live superflat
/// Paper FULL captures): `structures` (starts + References), the four FINAL
/// heightmaps (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`,
/// `MOTION_BLOCKING_NO_LEAVES`) as 37-long arrays, `isLightOn`, and the
/// `starlight.light_version` state.
///
/// `isLightOn` and `starlight.light_version` are **required**, not optional:
/// `SerializableChunkData`'s `lightCorrect` (spec §6) is true exactly when the
/// chunk's status is at-or-after `LIGHT` and `isLightOn` is non-null and
/// `starlight.light_version == 10`, and a chunk that has reached `minecraft:full`
/// writes both (isLightOn then clobbered to false). Every genuine FULL payload
/// captured — the M2 seed-42 spawn boot's the_nether/0.0 and the_end/0.0, and
/// the corpus-forced superflat capture's 8 per dimension across
/// overworld/nether/end — carries both, so a FULL chunk missing either was not
/// finished by Paper and must never be silently compared.
fn validate_full_payload(
    compound: &rivet_nbt::compound_tag::CompoundTag,
    dim: &str,
    cx: i32,
    cz: i32,
) -> Result<(), String> {
    let at = format!("{dim}/{cx}.{cz}");

    if compound.get_string("Status").map(String::as_str) != Some("minecraft:full") {
        return Err(format!(
            "chunk {at} stamped FULL but root Status is not minecraft:full (bare `full` is \
             never written by Paper's `ChunkStatus.CODEC`; status/spec drift)"
        ));
    }
    let mut missing: Vec<&'static str> = Vec::new();
    match compound.get_int("DataVersion") {
        Some(version) if version == CURRENT_DATA_VERSION => {}
        Some(version) => {
            return Err(format!(
                "chunk {at} has DataVersion {version}, expected pinned Minecraft 26.2 value {CURRENT_DATA_VERSION}"
            ));
        }
        None => missing.push("DataVersion"),
    }
    let expected_min_section_y = match dim {
        "overworld" => -4,
        "the_nether" | "the_end" => 0,
        _ => {
            return Err(format!(
                "chunk {at} is in an unsupported dimension for FULL validation"
            ));
        }
    };
    match compound.get_int("yPos") {
        Some(y) if y == expected_min_section_y => {}
        Some(y) => {
            return Err(format!(
                "chunk {at} has yPos {y}, expected dimension minSectionY {expected_min_section_y}"
            ));
        }
        None => missing.push("yPos"),
    }
    match compound.tags.get("structures") {
        Some(rivet_nbt::tag::Tag::Compound(structures)) => {
            // SerializableChunkData always emits both structure sub-compounds
            // for a FULL LevelChunk.  Merely carrying an empty `structures`
            // marker would let a hand-built/synthetic payload masquerade as a
            // finished Paper chunk.
            if !matches!(
                structures.tags.get("starts"),
                Some(rivet_nbt::tag::Tag::Compound(_))
            ) {
                missing.push("structures.starts");
            }
            if !matches!(
                structures.tags.get("References"),
                Some(rivet_nbt::tag::Tag::Compound(_))
            ) {
                missing.push("structures.References");
            }
        }
        _ => missing.push("structures"),
    }
    // lightCorrect (spec §6) is the gate for isLightOn/starlight.light_version
    // being written at all: a FULL chunk without them was not light-correct.
    if compound.get_byte("isLightOn").is_none() {
        missing.push("isLightOn");
    }
    match compound.get_int("starlight.light_version") {
        Some(ver) if ver == STARLIGHT_LIGHT_VERSION => {}
        Some(ver) => {
            return Err(format!(
                "chunk {at} stamped FULL but starlight.light_version is {ver}, expected \
                 {STARLIGHT_LIGHT_VERSION} — the light engine did not finalize this chunk"
            ));
        }
        None => missing.push("starlight.light_version"),
    }
    let final_heightmaps = [
        "OCEAN_FLOOR",
        "WORLD_SURFACE",
        "MOTION_BLOCKING",
        "MOTION_BLOCKING_NO_LEAVES",
    ];
    let heightmaps = compound
        .get_compound("Heightmaps")
        .ok_or_else(|| format!("chunk {at} has no Heightmaps compound"))?;
    for key in final_heightmaps {
        match heightmaps.tags.get(key) {
            Some(rivet_nbt::tag::Tag::LongArray(arr)) => {
                if arr.data.len() != FINAL_HEIGHTMAP_LONGS {
                    return Err(format!(
                        "chunk {at} heightmap {key} has {} longs, expected \
                         {FINAL_HEIGHTMAP_LONGS} (256 entries packed into longs)",
                        arr.data.len()
                    ));
                }
            }
            _ => missing.push(key),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "chunk {at} stamped FULL but missing FULL-time structure fields: {} — a chunk \
             Paper did not actually finish to FULL, refusing to compare it",
            missing.join(", ")
        ));
    }
    // Paper always writes a sections ListTag. Every emitted section is a
    // compound with a unique, light-bounded Y. Every interior section carries
    // codec-shaped block-state and biome compounds; each optional boundary is
    // emitted only when it carries light state. Light arrays, when present, are
    // exact DataLayers.
    let sections = compound
        .get_list("sections")
        .ok_or_else(|| format!("chunk {at} has no sections ListTag"))?;
    if sections.list.is_empty() {
        return Err(format!("chunk {at} has an empty sections ListTag"));
    }
    let max_section_y = if dim == "overworld" { 19 } else { 15 };
    // Paper/Starlight serializes the light neighborhood too: WorldUtil's
    // light-section range is one section below the block range through one
    // section above it. `SerializableChunkData.copyOf` emits every interior
    // block section, while either boundary section is emitted only when its
    // light state exists. Thus the exact accepted closure is all interior Ys
    // plus optional light-only boundary Ys; arbitrary subsets and a single
    // section are not Paper FULL payloads.
    let light_min_section_y = expected_min_section_y - 1;
    let light_max_section_y = max_section_y + 1;
    let mut section_ys = std::collections::HashSet::new();
    let mut previous_section_y = None;
    for sec_tag in &sections.list {
        let rivet_nbt::tag::Tag::Compound(sec) = sec_tag else {
            return Err(format!(
                "chunk {at} sections contains a non-compound element"
            ));
        };
        let Some(y) = sec.get_byte("Y") else {
            return Err(format!("chunk {at} section is missing byte Y"));
        };
        let y = i32::from(y);
        if y < light_min_section_y || y > light_max_section_y {
            return Err(format!(
                "chunk {at} section Y {y} is outside Paper light-section bounds {light_min_section_y}..={light_max_section_y}"
            ));
        }
        if !section_ys.insert(y) {
            return Err(format!("chunk {at} contains duplicate section Y {y}"));
        }
        if let Some(previous) = previous_section_y
            && y <= previous
        {
            return Err(format!(
                "chunk {at} sections are not in Paper's ascending Y order: {previous} then {y}"
            ));
        }
        previous_section_y = Some(y);
        let in_block_bounds = y >= expected_min_section_y && y <= max_section_y;
        let has_block_states = sec.tags.contains_key("block_states");
        let has_biomes = sec.tags.contains_key("biomes");
        if in_block_bounds {
            validate_codec_container(sec, &at, y, "block_states", true)?;
            validate_codec_container(sec, &at, y, "biomes", false)?;
        } else if has_block_states || has_biomes {
            return Err(format!(
                "chunk {at} boundary section Y {y} carries block_states/biomes; Paper light-only boundary sections must not carry block or biome containers"
            ));
        }
        for key in ["SkyLight", "BlockLight"] {
            if let Some(tag) = sec.tags.get(key) {
                let rivet_nbt::tag::Tag::ByteArray(arr) = tag else {
                    return Err(format!(
                        "chunk {at} section {key} has the wrong NBT type; Paper expects a ByteArray"
                    ));
                };
                if arr.data.len() != 2048 {
                    return Err(format!(
                        "chunk {at} section {key} has {} bytes, expected 2048 \
                         (16x16x16 nibble array)",
                        arr.data.len()
                    ));
                }
            }
        }
        for key in ["starlight.blocklight_state", "starlight.skylight_state"] {
            if let Some(tag) = sec.tags.get(key) {
                let rivet_nbt::tag::Tag::Int(state) = tag else {
                    return Err(format!(
                        "chunk {at} section {key} has the wrong NBT type; Paper expects an Int"
                    ));
                };
                if state.value <= 0 {
                    return Err(format!(
                        "chunk {at} section {key} has state {}; Paper only emits positive light states",
                        state.value
                    ));
                }
            }
        }
        if !in_block_bounds
            && ![
                "SkyLight",
                "BlockLight",
                "starlight.blocklight_state",
                "starlight.skylight_state",
            ]
            .iter()
            .any(|key| sec.tags.contains_key(*key))
        {
            return Err(format!(
                "chunk {at} boundary section Y {y} has no light state; Paper emits a boundary only when a light nibble or positive Starlight state exists"
            ));
        }
    }
    for y in expected_min_section_y..=max_section_y {
        if !section_ys.contains(&y) {
            return Err(format!(
                "chunk {at} is missing required Paper block section Y {y}; FULL section closure is {expected_min_section_y}..={max_section_y} plus optional boundary light sections"
            ));
        }
    }
    Ok(())
}

/// Validate the NBT shape accepted by Paper's PalettedContainer codec. The
/// codec is a compound with a non-empty palette and an optional packed LONG
/// stream (`data`); single-value palettes omit `data`, while non-singleton
/// palettes carry exactly the SimpleBitStorage packed length.
fn validate_codec_container(
    section: &rivet_nbt::compound_tag::CompoundTag,
    at: &str,
    y: i32,
    key: &str,
    block_states: bool,
) -> Result<(), String> {
    let Some(tag) = section.tags.get(key) else {
        return Err(format!("chunk {at} section Y {y} has no {key} compound"));
    };
    let rivet_nbt::tag::Tag::Compound(container) = tag else {
        return Err(format!(
            "chunk {at} section Y {y} {key} has the wrong NBT type; Paper expects a Compound"
        ));
    };
    let Some(palette_tag) = container.tags.get("palette") else {
        return Err(format!(
            "chunk {at} section Y {y} {key} has no palette ListTag"
        ));
    };
    let rivet_nbt::tag::Tag::List(palette) = palette_tag else {
        return Err(format!(
            "chunk {at} section Y {y} {key}.palette has the wrong NBT type; Paper expects a List"
        ));
    };
    if palette.list.is_empty() {
        return Err(format!("chunk {at} section Y {y} {key} palette is empty"));
    }
    for (index, entry) in palette.list.iter().enumerate() {
        if block_states {
            let rivet_nbt::tag::Tag::Compound(state) = entry else {
                return Err(format!(
                    "chunk {at} section Y {y} block_states.palette entry {index} is not a Compound"
                ));
            };
            match state.tags.get("Name") {
                Some(rivet_nbt::tag::Tag::String(name)) if !name.value.is_empty() => {}
                Some(_) => {
                    return Err(format!(
                        "chunk {at} section Y {y} block_states.palette entry {index} Name is not a non-empty String"
                    ));
                }
                None => {
                    return Err(format!(
                        "chunk {at} section Y {y} block_states.palette entry {index} has no Name"
                    ));
                }
            }
            if let Some(properties) = state.tags.get("Properties") {
                let rivet_nbt::tag::Tag::Compound(properties) = properties else {
                    return Err(format!(
                        "chunk {at} section Y {y} block_states.palette entry {index} Properties is not a Compound"
                    ));
                };
                if properties
                    .tags
                    .values()
                    .any(|value| !matches!(value, rivet_nbt::tag::Tag::String(_)))
                {
                    return Err(format!(
                        "chunk {at} section Y {y} block_states.palette entry {index} Properties contains a non-String value"
                    ));
                }
            }
        } else if !matches!(entry, rivet_nbt::tag::Tag::String(name) if !name.value.is_empty()) {
            return Err(format!(
                "chunk {at} section Y {y} biomes.palette entry {index} is not a non-empty String"
            ));
        }
    }

    // This is Paper's Strategy/Configuration table, not a generic ceil-log2
    // approximation. Blocks reserve four local bits through sixteen states and
    // switch to the global configuration at nine bits. Biomes switch at four.
    // SimpleBitStorage stores floor(64 / bits) values in each long.
    let palette_size = palette.list.len();
    let entry_count: usize = if block_states { 4096 } else { 64 };
    let bits = if palette_size == 1 {
        0
    } else {
        let minimum = ceil_log2(palette_size);
        if block_states {
            minimum.max(4)
        } else {
            minimum
        }
    };
    if bits > 32 {
        return Err(format!(
            "chunk {at} section Y {y} {key} palette requires unsupported {bits}-bit storage"
        ));
    }
    let expected_longs = if bits == 0 {
        0
    } else {
        let values_per_long = 64usize.checked_div(bits).ok_or_else(|| {
            format!("chunk {at} section Y {y} {key} has no values-per-long for {bits} bits")
        })?;
        entry_count.div_ceil(values_per_long)
    };
    let Some(data_tag) = container.tags.get("data") else {
        return if bits == 0 {
            Ok(())
        } else {
            Err(format!(
                "chunk {at} section Y {y} {key} palette has {palette_size} entries but no packed data"
            ))
        };
    };
    let rivet_nbt::tag::Tag::LongArray(data) = data_tag else {
        return Err(format!(
            "chunk {at} section Y {y} {key}.data has the wrong NBT type; Paper expects a LongArray"
        ));
    };
    if bits == 0 {
        return Err(format!(
            "chunk {at} section Y {y} {key} single-value palette must omit packed data, got {} longs",
            data.data.len()
        ));
    }
    if data.data.len() != expected_longs {
        return Err(format!(
            "chunk {at} section Y {y} {key} packed data has {} longs, expected {expected_longs}",
            data.data.len()
        ));
    }

    // Decode every logical entry.  Checking only the packed length accepts a
    // p2 container whose data contains index 2 (or a global p257 container
    // containing an index outside its palette), which Paper rejects while
    // unpacking through Palette.valueFor.
    let values_per_long = 64 / bits;
    let mask = (1u64 << bits) - 1;
    for index in 0..entry_count {
        let word = index / values_per_long;
        let shift = (index % values_per_long) * bits;
        let value = ((data.data[word] as u64) >> shift) & mask;
        if value >= palette_size as u64 {
            return Err(format!(
                "chunk {at} section Y {y} {key} data index {index} decodes to palette index {value}, outside palette size {palette_size}"
            ));
        }
    }
    Ok(())
}

fn ceil_log2(value: usize) -> usize {
    if value <= 1 {
        0
    } else {
        (usize::BITS - (value - 1).leading_zeros()) as usize
    }
}

/// Parse `<cx>.<cz>.nbt` from a fixture path.
fn parse_chunk_filename(path: &Path) -> Result<(i32, i32), String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| format!("cannot parse chunk name from {}", path.display()))?;
    let (cx, cz) = stem
        .split_once('.')
        .ok_or_else(|| format!("chunk name {stem} is not <cx>.<cz>"))?;
    let cx_value = cx
        .parse::<i32>()
        .map_err(|e| format!("bad cx in {stem}: {e}"))?;
    let cz_value = cz
        .parse::<i32>()
        .map_err(|e| format!("bad cz in {stem}: {e}"))?;
    if cx != cx_value.to_string() || cz != cz_value.to_string() {
        return Err(format!(
            "chunk name {stem} has noncanonical coordinate spelling"
        ));
    }
    Ok((cx_value, cz_value))
}

/// Coverage of `manifest`'s FULL chunks against the corpus (threat 4: coverage
/// is reported, never assumed). `extra` = FULL entries not in the corpus sweep.
///
/// The sweep target is **seeds × coordinates** (the #175 matrix): a green sweep
/// needs a FULL chunk at every corpus coordinate, generated under every corpus
/// seed. A manifest records a single world seed, so at most one seed's row of
/// coordinates can be present — a manifest under an off-corpus seed covers zero
/// cells and its FULL entries are all "extra" (the whole manifest is outside the
/// sweep). Coverage is informational today (nothing gates on it); a future
/// multi-seed green decision compares one seed-pair at a time and must reach
/// complete coverage per seed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub expected: usize,
    pub present: usize,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl Coverage {
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Compute coverage of a manifest's FULL chunks against the corpus
/// (seeds × coordinates). A (seed, coordinate) cell is "present" when the
/// manifest's seed is a corpus seed AND it has a FULL entry at that coordinate
/// (any dimension — the corpus names coordinates, not dimensions). The manifest's
/// seed must be a corpus seed for any cell to count: an off-corpus seed (like the
/// committed capture's working seed 42) covers none of the sweep.
pub fn coverage(manifest: &HashManifest, corpus: &corpus::Corpus) -> Coverage {
    let expected = corpus.seeds.len() * corpus.coordinates.len();
    let seed = manifest.seed.parse::<u64>().ok();
    let seed_idx = seed.and_then(|s| corpus.seeds.iter().position(|c| *c == s));
    let mut missing = Vec::new();
    let mut extra = Vec::new();
    let mut present = 0usize;

    let Some(si) = seed_idx else {
        // Off-corpus seed: no sweep cell can be satisfied; every FULL entry is
        // outside the corpus (the manifest was generated under a seed the sweep
        // does not target).
        missing = corpus
            .seeds
            .iter()
            .flat_map(|s| {
                corpus
                    .coordinates
                    .iter()
                    .map(move |c| format!("{s}@({},{})", c.0, c.1))
            })
            .collect();
        for e in &manifest.entries {
            if e.is_full() {
                extra.push(format!("{}/{}.{}.{}", e.dim, e.region, e.cx, e.cz));
            }
        }
        return Coverage {
            expected,
            present,
            missing,
            extra,
        };
    };

    let seed = corpus.seeds[si];
    let mut covered: BTreeMap<(i32, i32), bool> = BTreeMap::new();
    for c in &corpus.coordinates {
        covered.insert(*c, false);
    }
    for e in &manifest.entries {
        if e.is_full()
            && let Some(found) = covered.get_mut(&(e.cx, e.cz))
            && !*found
        {
            *found = true;
            present += 1;
        }
    }
    for (coord, found) in &covered {
        if !*found {
            missing.push(format!("{seed}@({},{})", coord.0, coord.1));
        }
    }
    // FULL entries at coordinates outside the corpus are over-generation.
    for e in &manifest.entries {
        if e.is_full() && !covered.contains_key(&(e.cx, e.cz)) {
            extra.push(format!("{}/{}.{}.{}", e.dim, e.region, e.cx, e.cz));
        }
    }
    Coverage {
        expected,
        present,
        missing,
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::Corpus;

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn sample_manifest(entries: Vec<ChunkHashEntry>) -> HashManifest {
        sample_manifest_seeded(entries, "5207638315753790570")
    }

    fn sample_manifest_seeded(entries: Vec<ChunkHashEntry>, seed: &str) -> HashManifest {
        let full_count = entries.iter().filter(|e| e.is_full()).count();
        HashManifest {
            format: 1,
            hash_algorithm: HASH_ALGORITHM.to_string(),
            hash_scope: HASH_SCOPE.to_string(),
            corpus_version: CORPUS_VERSION.to_string(),
            seed: seed.to_string(),
            level_type: "minecraft\\:normal".to_string(),
            region_file_compression: "none".to_string(),
            paper: "26.2-DEV-main@0a99345".to_string(),
            chunk_concurrency: Concurrency {
                worker_threads: 1,
                io_threads: 1,
            },
            chunk_count: entries.len(),
            full_count,
            entries,
        }
    }

    fn full_entry(dim: &str, cx: i32, cz: i32) -> ChunkHashEntry {
        ChunkHashEntry {
            dim: dim.to_string(),
            region: "0.0".to_string(),
            cx,
            cz,
            status: "minecraft:full".to_string(),
            bytes: 10,
            xxh3_64: "0".repeat(16),
            sha256: "0".repeat(64),
            xxh3_64_canonical: "0".repeat(16),
        }
    }

    /// The committed M2 region capture stamps exactly the true FULL counts
    /// (0 overworld, 1 each nether/end) — the load-bearing fixture-trap guard.
    /// This test is *load-bearing*: if the committed region fixtures are ever
    /// pruned or not checked out, it must FAIL (never silently return and leave
    /// the fixture-trap guard unverified) — the `else` branch panics instead of
    /// skipping, per the project fixture rule (D8: never weaken/delete fixtures
    /// to go green; a missing load-bearing fixture is a hard failure).
    #[test]
    fn committed_region_payloads_stamp_true_full_counts() {
        let dir = crate_dir().join("fixtures/regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed region fixtures {} are ABSENT — the load-bearing FULL \
                 fixture-trap guard cannot verify; restore them (git checkout) or this \
                 test is red, never silently skipped",
                dir.display()
            );
        }
        let m = build_from_payloads(&dir, "42", "minecraft\\:normal").unwrap();
        assert_eq!(m.chunk_count, 408);
        assert_eq!(m.full_count, 2);
        let by_dim: BTreeMap<&str, usize> =
            m.entries
                .iter()
                .map(|e| e.dim.as_str())
                .fold(BTreeMap::new(), |mut acc, d| {
                    *acc.entry(d).or_default() += 1;
                    acc
                });
        assert_eq!(by_dim.get("overworld").copied().unwrap_or(0), 120);
        let nether_full = m
            .entries
            .iter()
            .filter(|e| e.dim == "the_nether" && e.is_full())
            .count();
        let end_full = m
            .entries
            .iter()
            .filter(|e| e.dim == "the_end" && e.is_full())
            .count();
        assert_eq!(nether_full, 1);
        assert_eq!(end_full, 1);
        // Overworld has zero FULL chunks — the #175 premise is stamped, not
        // papered over.
        let ow_full = m
            .entries
            .iter()
            .filter(|e| e.dim == "overworld" && e.is_full())
            .count();
        assert_eq!(ow_full, 0);
    }

    #[test]
    fn build_is_deterministic() {
        let dir = crate_dir().join("fixtures/regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let a = build_from_payloads(&dir, "42", "minecraft\\:normal").unwrap();
        let b = build_from_payloads(&dir, "42", "minecraft\\:normal").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn coverage_reports_missing_and_extra() {
        let corpus = Corpus {
            seeds: vec![42],
            coordinates: vec![(0, 0), (15, 15)],
        };
        // A manifest under the corpus seed 42: (0,0) present, (15,15) missing.
        let m = sample_manifest_seeded(vec![full_entry("the_nether", 0, 0)], "42");
        let cov = coverage(&m, &corpus);
        assert_eq!(cov.expected, 2);
        assert_eq!(cov.present, 1);
        assert_eq!(cov.missing, vec!["42@(15,15)".to_string()]);
        assert!(!cov.is_complete());

        // A FULL entry outside the corpus is "extra".
        let m2 = sample_manifest_seeded(vec![full_entry("the_nether", 7, 7)], "42");
        let cov2 = coverage(&m2, &corpus);
        assert_eq!(cov2.present, 0);
        assert_eq!(cov2.extra, vec!["the_nether/0.0.7.7".to_string()]);
    }

    /// A manifest under an off-corpus seed (the committed capture's working seed
    /// 42 when it is not a corpus seed) covers zero sweep cells — the whole
    /// sweep is missing and every FULL entry is outside the corpus. This is the
    /// honest answer the seed-aware sweep gives for a capture that was not
    /// generated under a pinned corpus seed.
    #[test]
    fn coverage_off_corpus_seed_covers_nothing() {
        let corpus = Corpus {
            seeds: vec![5207638315753790570],
            coordinates: vec![(0, 0), (15, 15)],
        };
        let m = sample_manifest_seeded(vec![full_entry("the_nether", 0, 0)], "42");
        let cov = coverage(&m, &corpus);
        assert_eq!(cov.expected, 2);
        assert_eq!(cov.present, 0, "seed 42 is not a corpus seed");
        assert_eq!(
            cov.missing,
            vec![
                "5207638315753790570@(0,0)".to_string(),
                "5207638315753790570@(15,15)".to_string()
            ]
        );
        assert_eq!(cov.extra, vec!["the_nether/0.0.0.0".to_string()]);
        assert!(!cov.is_complete());
    }

    #[test]
    fn provenance_refuses_mismatch() {
        let a = sample_manifest(vec![]);
        let mut b = a.clone();
        b.seed = "different-seed".to_string();
        assert_ne!(a.provenance(), b.provenance());
    }

    /// Provenance is a tuple over seed/algorithm/scope/corpus/compression/
    /// paper/concurrency/level-type: any field drift refuses the diff. The
    /// comparator compares `HashManifest::provenance()` whole (main.rs
    /// `run_diff`), so a manifest recorded under a different region compression,
    /// corpus revision, or level-type must never compare green against another —
    /// a digest table stamped with a hardcoded "none"/"v1"/"normal" would
    /// silently compare worlds generated under different framing (issue #51).
    #[test]
    fn provenance_refuses_compression_corpus_and_level_drift() {
        let base = sample_manifest(vec![]);

        let mut compression = base.clone();
        compression.region_file_compression = "deflate".to_string();
        assert_ne!(base.provenance(), compression.provenance());

        let mut corpus = base.clone();
        corpus.corpus_version = "v2".to_string();
        assert_ne!(base.provenance(), corpus.provenance());

        let mut level = base.clone();
        level.level_type = "minecraft\\:flat".to_string();
        assert_ne!(base.provenance(), level.provenance());
    }

    /// The committed superflat status-FULL region capture (issue #51) is the
    /// deliverable: a genuine corpus-forced Paper capture under corpus seed 0
    /// (5207638315753790570) of the four regions around the origin. The
    /// two-boot ticket-injection capture (`regenerate --full`) loads level-33
    /// `minecraft:forced` tickets for every corpus coordinate in all three
    /// dimensions, so all 8 corpus coordinates are stamped `minecraft:full` per
    /// dimension — 24 FULL chunks (8 coords × 3 dims), zero outside the corpus.
    /// This test pins the acceptance: corpus coverage is 8 present / 24 missing
    /// (seed 0's row fully owned; the other 3 seeds' rows are unreachable from a
    /// single-seed manifest) / 0 extra. The level-type/compression are inherited
    /// from the source capture.
    #[test]
    fn committed_superflat_full_capture_covers_corpus_seed_zero() {
        let dir = crate_dir().join("fixtures/regions/superflat-full");
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed superflat FULL fixtures {} are ABSENT — the #51 corpus-forced \
                 FULL capture is a load-bearing deliverable; this test must FAIL, never \
                 silently skip (restore them or the SHA-256 superflat gate is unverified)",
                dir.display()
            );
        }
        let prov = CaptureProvenance::from_region_manifest(Some("minecraft\\:flat"), Some("none"));
        let m = build_from_payloads_with(&dir, "5207638315753790570", "minecraft\\:flat", &prov)
            .expect("committed superflat FULL payloads build and FULL-validate");
        assert_eq!(m.level_type, "minecraft\\:flat");
        assert_eq!(m.region_file_compression, "none");
        assert_eq!(m.corpus_version, CORPUS_VERSION);

        // The corpus-forced capture spans the four origin regions per dimension
        // (r.0.0, r.-1.-1, r.-1.0, r.0.-1).
        assert_eq!(
            m.full_count, 24,
            "all 8 corpus coordinates per dimension × 3 dimensions are status-FULL"
        );

        let cov = coverage(&m, &Corpus::from_committed());
        assert_eq!(cov.expected, 4 * 8, "corpus = 4 seeds × 8 coordinates");
        assert_eq!(
            cov.present, 8,
            "every corpus coordinate of the recorded seed (seed 0) is FULL"
        );
        // `missing` enumerates only the recorded seed's row (the other seeds'
        // rows are never reachable from a single-seed manifest): seed 0's 8
        // coordinates are all present, so its row is complete. The 24 uncovered
        // sweep cells are the other 3 seeds' rows, which this manifest can
        // never own — covered by `expected - present` below.
        assert_eq!(cov.missing.len(), 0, "seed 0's row is fully owned");
        assert!(
            cov.is_complete(),
            "seed 0's row is complete (8/8 coordinates FULL)"
        );
        assert_eq!(
            cov.extra,
            Vec::<String>::new(),
            "no FULL chunk exists outside the corpus coordinates"
        );
        assert_eq!(cov.expected - cov.present, 24);
    }

    /// The FULL-structure validator accepts a genuine FULL payload and rejects
    /// each shape that would let an unfinished chunk be silently compared as if
    /// Paper had finished it (issue #51): no `structures` compound, a FINAL
    /// heightmap missing or not 37-long, a section light array not 2048 bytes,
    /// and a non-FULL status stamped FULL.
    #[test]
    fn validate_full_payload_accepts_genuine_full_and_rejects_shapes() {
        use crate::mutate::{fixture_full_payload, parse_payload};
        use rivet_nbt::byte_array_tag::ByteArrayTag;
        use rivet_nbt::int_tag::IntTag;
        use rivet_nbt::long_array_tag::LongArrayTag;
        use rivet_nbt::tag::Tag;

        let compound = parse_payload(&fixture_full_payload(0, 0)).unwrap();
        validate_full_payload(&compound, "overworld", 0, 0).expect("genuine FULL validates");

        let mut no_data_version = compound.clone();
        no_data_version.tags.swap_remove("DataVersion");
        assert!(validate_full_payload(&no_data_version, "overworld", 0, 0).is_err());

        let mut wrong_data_version = compound.clone();
        wrong_data_version.put_int("DataVersion", CURRENT_DATA_VERSION - 1);
        assert!(validate_full_payload(&wrong_data_version, "overworld", 0, 0).is_err());

        let mut wrong_y_pos = compound.clone();
        wrong_y_pos.put_int("yPos", -3);
        assert!(validate_full_payload(&wrong_y_pos, "overworld", 0, 0).is_err());

        let mut no_sections = compound.clone();
        no_sections.tags.swap_remove("sections");
        assert!(validate_full_payload(&no_sections, "overworld", 0, 0).is_err());

        let mut no_structures = compound.clone();
        no_structures.tags.swap_remove("structures");
        assert!(validate_full_payload(&no_structures, "overworld", 0, 0).is_err());

        let mut no_structure_starts = compound.clone();
        no_structure_starts
            .get_compound_or_empty_mut("structures")
            .tags
            .swap_remove("starts");
        assert!(validate_full_payload(&no_structure_starts, "overworld", 0, 0).is_err());

        let mut no_structure_references = compound.clone();
        no_structure_references
            .get_compound_or_empty_mut("structures")
            .tags
            .swap_remove("References");
        assert!(validate_full_payload(&no_structure_references, "overworld", 0, 0).is_err());

        let mut short_hm = compound.clone();
        short_hm
            .get_compound_or_empty_mut("Heightmaps")
            .tags
            .insert(
                "OCEAN_FLOOR".to_string(),
                Tag::LongArray(LongArrayTag::new(vec![0; 5])),
            );
        assert!(validate_full_payload(&short_hm, "overworld", 0, 0).is_err());

        let mut missing_hm = compound.clone();
        missing_hm
            .get_compound_or_empty_mut("Heightmaps")
            .tags
            .swap_remove("MOTION_BLOCKING_NO_LEAVES");
        assert!(validate_full_payload(&missing_hm, "overworld", 0, 0).is_err());

        let mut bad_light = compound.clone();
        let sections = bad_light.get_list_or_empty_mut("sections");
        if let Tag::Compound(sec) = &mut sections.list[0] {
            sec.tags.insert(
                "SkyLight".to_string(),
                Tag::ByteArray(ByteArrayTag::new(vec![0i8; 10])),
            );
        }
        assert!(validate_full_payload(&bad_light, "overworld", 0, 0).is_err());

        for key in ["SkyLight", "BlockLight"] {
            let mut wrong_light_type = compound.clone();
            let sections = wrong_light_type.get_list_or_empty_mut("sections");
            if let Tag::Compound(sec) = &mut sections.list[0] {
                sec.tags.insert(key.to_string(), Tag::Int(IntTag::new(1)));
            }
            assert!(
                validate_full_payload(&wrong_light_type, "overworld", 0, 0).is_err(),
                "present {key} with a non-ByteArray NBT type must be rejected"
            );
        }

        let block_section = compound
            .get_list("sections")
            .unwrap()
            .list
            .iter()
            .position(|tag| {
                matches!(
                    tag,
                    Tag::Compound(section) if section.tags.contains_key("block_states")
                )
            })
            .unwrap();
        let mut wrong_codec_container = compound.clone();
        if let Tag::Compound(section) =
            &mut wrong_codec_container.get_list_or_empty_mut("sections").list[block_section]
        {
            section
                .tags
                .insert("block_states".to_string(), Tag::Int(IntTag::new(1)));
        }
        assert!(validate_full_payload(&wrong_codec_container, "overworld", 0, 0).is_err());

        let mut wrong_codec_data_type = compound.clone();
        if let Tag::Compound(section) =
            &mut wrong_codec_data_type.get_list_or_empty_mut("sections").list[block_section]
        {
            let states = section.get_compound_or_empty_mut("block_states");
            states
                .tags
                .insert("data".to_string(), Tag::Int(IntTag::new(1)));
        }
        assert!(validate_full_payload(&wrong_codec_data_type, "overworld", 0, 0).is_err());

        let mut missing_codec_data = compound.clone();
        if let Tag::Compound(section) =
            &mut missing_codec_data.get_list_or_empty_mut("sections").list[block_section]
        {
            section
                .get_compound_or_empty_mut("block_states")
                .tags
                .swap_remove("data");
        }
        assert!(validate_full_payload(&missing_codec_data, "overworld", 0, 0).is_err());

        let mut non_compound_section = compound.clone();
        non_compound_section.get_list_or_empty_mut("sections").list[0] =
            Tag::String(rivet_nbt::string_tag::StringTag::value_of("bad".into()));
        assert!(validate_full_payload(&non_compound_section, "overworld", 0, 0).is_err());

        let mut no_section_y = compound.clone();
        if let Tag::Compound(section) = &mut no_section_y.get_list_or_empty_mut("sections").list[0]
        {
            section.tags.swap_remove("Y");
        }
        assert!(validate_full_payload(&no_section_y, "overworld", 0, 0).is_err());

        let mut duplicate_section_y = compound.clone();
        let duplicate = duplicate_section_y.get_list_or_empty_mut("sections").list[0].clone();
        duplicate_section_y
            .get_list_or_empty_mut("sections")
            .list
            .push(duplicate);
        assert!(validate_full_payload(&duplicate_section_y, "overworld", 0, 0).is_err());

        let mut no_biomes = compound.clone();
        let block_section = no_biomes
            .get_list("sections")
            .unwrap()
            .list
            .iter()
            .position(|tag| {
                matches!(
                    tag,
                    Tag::Compound(section) if section.tags.contains_key("block_states")
                )
            })
            .unwrap();
        if let Tag::Compound(section) =
            &mut no_biomes.get_list_or_empty_mut("sections").list[block_section]
        {
            section.tags.swap_remove("biomes");
        }
        assert!(validate_full_payload(&no_biomes, "overworld", 0, 0).is_err());

        let mut no_light_flag = compound.clone();
        no_light_flag.tags.swap_remove("isLightOn");
        assert!(
            validate_full_payload(&no_light_flag, "overworld", 0, 0).is_err(),
            "a FULL chunk without isLightOn was not light-correct and must be refused"
        );

        let mut no_starlight = compound.clone();
        no_starlight.tags.swap_remove("starlight.light_version");
        assert!(
            validate_full_payload(&no_starlight, "overworld", 0, 0).is_err(),
            "a FULL chunk without starlight.light_version must be refused"
        );

        let mut stale_starlight = compound.clone();
        stale_starlight.put_int("starlight.light_version", 9);
        assert!(
            validate_full_payload(&stale_starlight, "overworld", 0, 0).is_err(),
            "a FULL chunk with a stale starlight.light_version (not {STARLIGHT_LIGHT_VERSION}) \
             must be refused"
        );

        let mut not_full = compound.clone();
        not_full.put_string("Status", "minecraft:structure_starts");
        assert!(validate_full_payload(&not_full, "overworld", 0, 0).is_err());

        let mut bare_full = compound.clone();
        bare_full.put_string("Status", "full");
        assert!(
            validate_full_payload(&bare_full, "overworld", 0, 0).is_err(),
            "bare `full` is never written by Paper's ChunkStatus.CODEC (namespace:path) and \
             must be refused as FULL, not compared against Paper"
        );
    }

    #[test]
    fn full_validator_rejects_single_section_and_empty_boundary_evidence() {
        use rivet_nbt::compound_tag::CompoundTag;
        use rivet_nbt::tag::Tag;

        let compound = parse_payload(&crate::mutate::fixture_full_payload(0, 0)).unwrap();
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
        let mut single_section = compound.clone();
        single_section.get_list_or_empty_mut("sections").list = vec![interior];
        let error = validate_full_payload(&single_section, "overworld", 0, 0)
            .expect_err("one section cannot represent Paper's FULL closure");
        assert!(
            error.contains("missing required Paper block section"),
            "single-section rejection must name the missing closure: {error}"
        );

        let mut empty_boundary = compound.clone();
        if let Tag::Compound(section) =
            &mut empty_boundary.get_list_or_empty_mut("sections").list[0]
        {
            section.tags.clear();
            section.put_byte("Y", -5);
        }
        let error = validate_full_payload(&empty_boundary, "overworld", 0, 0)
            .expect_err("Paper never emits an empty boundary section");
        assert!(
            error.contains("boundary section Y -5") && error.contains("no light state"),
            "empty-boundary rejection must name the missing light state: {error}"
        );

        let mut upper_boundary = compound;
        let mut section = CompoundTag::new();
        section.put_byte("Y", 20);
        section.put_byte_array("SkyLight", vec![0i8; 2048]);
        upper_boundary
            .get_list_or_empty_mut("sections")
            .list
            .push(Tag::Compound(section));
        validate_full_payload(&upper_boundary, "overworld", 0, 0)
            .expect("a light-bearing upper boundary is part of Paper's accepted closure");

        let mut reversed = upper_boundary;
        reversed.get_list_or_empty_mut("sections").list.reverse();
        let error = validate_full_payload(&reversed, "overworld", 0, 0)
            .expect_err("Paper writes section data in ascending Y order");
        assert!(
            error.contains("ascending Y order"),
            "reversed sections must be rejected as malformed Paper evidence: {error}"
        );
    }

    /// A bare `full` root Status is never FULL: Paper serializes the FULL status
    /// as `minecraft:full` (`register("full", ...)` + `Identifier.toString` =
    /// `namespace:path`), so `is_full_status` accepts only the namespaced form.
    ///
    /// Two guards, both load-bearing:
    /// 1. `is_full` classifies a bare `full` as non-FULL, so it is never compared
    ///    against Paper.
    /// 2. `build_from_payloads` **refuses** a tree whose payload carries bare
    ///    `full` — the off-spec serialization is a loud hard error, never a
    ///    silently 0-FULL manifest (which would surface later only as an obscure
    ///    UNVERIFIED/one-sided FAIL instead of naming the malformed tree).
    #[test]
    fn bare_full_status_is_refused_at_build() {
        let e = full_entry("the_nether", 0, 0);
        assert!(e.is_full(), "namespaced minecraft:full is FULL");
        let mut bare = e;
        bare.status = "full".to_string();
        assert!(
            !bare.is_full(),
            "bare `full` is not Paper's serialized FULL status"
        );

        // A tree whose only payload carries bare `full` must be refused loudly.
        use crate::mutate::{encode_payload, fixture_full_payload, parse_payload};
        let mut compound = parse_payload(&fixture_full_payload(0, 0)).unwrap();
        compound.put_string("Status", "full");
        let bytes = encode_payload(&compound).unwrap();

        let dir = std::env::temp_dir().join(format!(
            "rivet-oracle-hash-bare-full-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        let chunk = dir.join("chunk/the_nether/0.0/0.0.nbt");
        std::fs::create_dir_all(chunk.parent().unwrap()).unwrap();
        std::fs::write(&chunk, &bytes).unwrap();

        let err = build_from_payloads(&dir, "42", "minecraft\\:normal")
            .expect_err("bare `full` in a payload tree must be refused");
        assert!(
            err.contains("bare root Status `full`"),
            "error names the bare `full`: {err}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A duplicate coordinate in a built manifest must be a hard error, not
    /// a silent dedup. This applies to every status, because a malformed
    /// non-FULL duplicate must not be able to become FULL through metadata edits.
    #[test]
    fn duplicate_coordinate_is_rejected_for_every_status() {
        let a = full_entry("the_nether", 0, 0);
        let mut b = full_entry("the_nether", 0, 0);
        b.xxh3_64 = "1".repeat(16); // a genuinely different payload at the same coord
        let err = reject_duplicate_full(&[a, b]).expect_err("duplicate FULL must be rejected");
        assert!(
            err.contains("duplicate chunk coordinate"),
            "error names the duplicate: {err}"
        );
        // Non-FULL duplicates at the same coordinate are malformed too.
        let mut c = full_entry("overworld", 5, 5);
        c.status = "minecraft:biomes".to_string();
        let mut d = full_entry("overworld", 5, 5);
        d.status = "minecraft:carvers".to_string();
        let err = reject_duplicate_full(&[c, d]).expect_err("all statuses share coordinates");
        assert!(err.contains("duplicate chunk coordinate"));
    }

    #[test]
    fn payload_bytes_bind_region_and_nbt_coordinates() {
        let provenance = CaptureProvenance::default();
        let bytes = crate::mutate::fixture_full_payload(0, 0);
        let wrong_region = [PayloadBytes {
            dim: "the_nether".to_string(),
            region: "1.0".to_string(),
            cx: 0,
            cz: 0,
            bytes: bytes.clone(),
        }];
        let error =
            build_from_payload_bytes_with(&wrong_region, "42", "minecraft\\:normal", &provenance)
                .expect_err("a payload in the wrong Anvil region must fail");
        assert!(error.contains("expected 0.0"), "{error}");

        let wrong_coordinates = [PayloadBytes {
            dim: "the_nether".to_string(),
            region: "0.0".to_string(),
            cx: 1,
            cz: 0,
            bytes,
        }];
        let error = build_from_payload_bytes_with(
            &wrong_coordinates,
            "42",
            "minecraft\\:normal",
            &provenance,
        )
        .expect_err("payload NBT coordinates must bind to the filename");
        assert!(error.contains("stores xPos/zPos"), "{error}");
    }
}
