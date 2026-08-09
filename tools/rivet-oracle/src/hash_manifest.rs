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
    /// Recorded provenance (seed + algorithm + concurrency + paper + scope)
    /// used by the comparator to refuse comparing manifests of different
    /// provenance (threat 9).
    pub fn provenance(&self) -> Provenance {
        Provenance {
            seed: self.seed.clone(),
            hash_algorithm: self.hash_algorithm.clone(),
            hash_scope: self.hash_scope.clone(),
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
    pub paper: String,
    pub concurrency: Concurrency,
    pub level_type: String,
}

impl Provenance {
    /// Serialize to a stable, human-readable identity string for diff output.
    pub fn describe(&self) -> String {
        format!(
            "seed={} algorithm={} scope={} paper={} concurrency={}/{} level-type={}",
            self.seed,
            self.hash_algorithm,
            self.hash_scope,
            self.paper,
            self.concurrency.worker_threads,
            self.concurrency.io_threads,
            self.level_type
        )
    }
}

/// Build a `HashManifest` from a fixtures tree laid out exactly like the
/// region capture: `chunk/<dim>/<region>/<cx>.<cz>.nbt`. Reads each payload
/// with the rivet-nbt codec, stamps its root `Status`, and records FULL vs
/// non-FULL (threat 2: status is stamped, never assumed).
pub fn build_from_payloads(
    dir: &Path,
    seed: &str,
    level_type: &str,
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
        corpus_version: CORPUS_VERSION.to_string(),
        seed: seed.to_string(),
        level_type: level_type.to_string(),
        region_file_compression: "none".to_string(),
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
}
