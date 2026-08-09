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

/// A single chunk's digests in a `HashManifest`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
/// stamps the raw root `Status` (e.g. `minecraft:full`), so every consumer —
/// build, coverage, the comparator — decides FULL through this one function,
/// never a raw string compare that could silently drop chunks and turn the diff
/// vacuously green.
fn is_full_status(status: &str) -> bool {
    status == "minecraft:full" || status == "full"
}

/// A `HashManifest` (the #54 format). Serialized in committed field order so a
/// rebuild is byte-identical.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
pub fn build_from_payloads_with(
    dir: &Path,
    seed: &str,
    level_type: &str,
    provenance: &CaptureProvenance,
) -> Result<HashManifest, String> {
    let chunk_dir = dir.join("chunk");
    let mut entries = Vec::new();
    let mut full_count = 0usize;

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
                    let (cx, cz) = parse_chunk_filename(&file)?;
                    let bytes = std::fs::read(&file)
                        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                    let compound = parse_payload(&bytes)?;
                    let status = compound
                        .get_string("Status")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    if is_full_status(&status) {
                        full_count += 1;
                        validate_full_payload(&compound, &dim, cx, cz)?;
                    }
                    entries.push(ChunkHashEntry {
                        dim: dim.clone(),
                        region: region.clone(),
                        cx,
                        cz,
                        status: status.clone(),
                        bytes: bytes.len(),
                        xxh3_64: xxh3_64_hex(&bytes),
                        sha256: crate::sha256_hex(&bytes),
                        xxh3_64_canonical: crate::semantic_hash::canonical_xxh3_64(&compound)?,
                    });
                }
            }
        }
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

    if compound.get_string("Status").map(String::as_str) != Some("minecraft:full")
        && compound.get_string("Status").map(String::as_str) != Some("full")
    {
        return Err(format!(
            "chunk {at} stamped FULL but root Status is not minecraft:full — status/spec drift"
        ));
    }
    let mut missing: Vec<&'static str> = Vec::new();
    if !matches!(
        compound.tags.get("structures"),
        Some(rivet_nbt::tag::Tag::Compound(_))
    ) {
        missing.push("structures");
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
                if arr.data.len() != 37 {
                    return Err(format!(
                        "chunk {at} heightmap {key} has {} longs, expected 37 (256 entries packed)",
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
    // Section-local light shape (spec §5): every `SkyLight`/`BlockLight` byte
    // array on a section is exactly 2048 bytes (16x16x16 nibbles). A wrong
    // length is a hard IllegalArgumentException in Java (`DataLayer`) — the
    // validator refuses to compare a chunk whose light data is malformed.
    if let Some(sections) = compound.get_list("sections") {
        for sec in &sections.list {
            let rivet_nbt::tag::Tag::Compound(sec) = sec else {
                continue;
            };
            for key in ["SkyLight", "BlockLight"] {
                if let Some(rivet_nbt::tag::Tag::ByteArray(arr)) = sec.tags.get(key)
                    && arr.data.len() != 2048
                {
                    return Err(format!(
                        "chunk {at} section {key} has {} bytes, expected 2048 \
                         (16x16x16 nibble array)",
                        arr.data.len()
                    ));
                }
            }
        }
    }
    Ok(())
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
    Ok((
        cx.parse().map_err(|e| format!("bad cx in {stem}: {e}"))?,
        cz.parse().map_err(|e| format!("bad cz in {stem}: {e}"))?,
    ))
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
    #[test]
    fn committed_region_payloads_stamp_true_full_counts() {
        let dir = crate_dir().join("fixtures/regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
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
            return;
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
        use rivet_nbt::long_array_tag::LongArrayTag;
        use rivet_nbt::tag::Tag;

        let compound = parse_payload(&fixture_full_payload(0, 0)).unwrap();
        validate_full_payload(&compound, "overworld", 0, 0).expect("genuine FULL validates");

        let mut no_structures = compound.clone();
        no_structures.tags.swap_remove("structures");
        assert!(validate_full_payload(&no_structures, "overworld", 0, 0).is_err());

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
    }
}
