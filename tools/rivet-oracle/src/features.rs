//! The seed-42 FEATURES oracle checkpoint (`features`).
//!
//! The next oracle checkpoint BEFORE any Rivet FEATURES implementation: a
//! focused Paper 26.2 capture of one deterministic seed-42 overworld chunk set
//! forced through the FEATURES decoration status, plus an exact Rivet-side
//! verifier. It reuses the generated-expected two-boot capture machinery
//! (`inject_forced_tickets` at level 33, `clear_region_files`, `check_capture_pin`)
//! but records stage-specific truth for the decoration step itself, so a future
//! Rivet FEATURES port is checked against Paper ground truth rather than against
//! nothing.
//!
//! Status semantics (the design consequence that shapes the verifier): the
//! Moonrise forced-ticket path can only serialize FULL. A level-34 ticket
//! (`ChunkLevel.byStatus(FEATURES)`) is `INACCESSIBLE` to `fullStatus`, so
//! `NewChunkHolder.processTicketLevelUpdate` never schedules generation for it —
//! a sub-FULL serialized capture is not reachable through forced tickets at all
//! (that path is server-boot-only and always requests FULL). So the checkpoint
//! captures at level 33, which serializes as `minecraft:full`. That is faithful
//! to the FEATURES step because FEATURES is the LAST block-mutating status: the
//! InitializeLight/Light steps only compute light arrays and never touch block
//! data, so a FULL serialization's block states ARE the FEATURES-decoration
//! output. The verifier pins `EXPECTED_STATUS = minecraft:full` and, since a
//! FULL chunk folds no `status:` flag into `capability_flags` (only non-FULL
//! statuses do), expects exactly the empty flag set.
//!
//! Coordinate set: the committed 2×2 grid {(3,3),(4,3),(3,4),(4,4)} around the
//! tree-bearing chunk (4,4). The FULL golden (generated-expected) shows (4,4)
//! decorated with dark-oak/oak logs and leaves, (3,4) as a water column, and
//! (4,3) as a grass/sand edge — a set that is non-vacuous for biome decoration.
//! The forced grid is the 4×4 neighborhood {2..=5}×{2..=5}, so every committed
//! chunk's 3×3 WorldGenRegion window (the radius-1 carvers dependency of the
//! FEATURES step) is forced too — the buffer that keeps border-tree placement
//! deterministic. The forced chunks are all held at level 33 (FULL), which is
//! past FEATURES, so a center chunk's decoration inputs (neighbor
//! heightmaps/surface, already settled by carvers) are complete before its
//! FEATURES task runs.
//!
//! The per-chunk contract reuses the loaded-world fingerprint: 16×16 row-major
//! `z*16+x` `surface`/`bedrock`/`below_feet` arrays, the sorted distinct block
//! set, distinct-state-id count, and section count, plus the chunk status and
//! capability flags. Non-vacuity: the committed chunk (4,4) must carry tree
//! blocks in its surface (FEATURES placed them; a pre-features carvers capture
//! has none), the union distinct set must exceed a pre-features floor, and the
//! bedrock plane must be depth-sampled — a capture that cannot distinguish a
//! decorated seed-42 chunk from an undecorated one is refused loudly.
//!
//! Honesty rules (mirror generated-expected, D8: never fabricate expected
//! content, never silently fall back to a superflat/undecorated capture):
//!
//!   * capture returns `Error::Unverified` (exit 3) when the pinned Paper
//!     runtime is absent — it never writes an empty or fabricated manifest, and
//!     it removes a stale `--to` file first.
//!   * capture is a two-boot sequence (create boot, then a level-33 forced
//!     capture boot) that discards boot1's partial spawn-area chunks so the
//!     forced grid is generated from a blank chunk state (byte-deterministic).
//!   * verify returns `Error::Unverified` (exit 3) when the committed fixture
//!     tree is absent — never a silent green.
//!   * verify rejects a fixture that is not exactly the forced FULL grid with
//!     real decoration evidence: a chunk stuck below full, a grid with no tree
//!     blocks, or an all-sand flat echo is refused loudly.
//!   * capture enforces provenance: after the capture boot, the materialized
//!     server jar's `Git-Commit` must match the pinned `0a99345` before any
//!     content is written.
//!   * the regenerate path validates a capture against the decoration contract
//!     BEFORE committing it, in addition to the twin-boot byte-identity proof.
//!   * the tamper negative control proves a flipped byte in the golden fails
//!     verification (the manifest SHA-256 gate is not vacuous).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::generated_expected::{
    capture_run_dir, check_capture_pin, clear_region_files, seed_properties,
};
use crate::loaded_world::{self, WorldManifest};
use crate::{CapturedFile, Error};

/// The fixture kind name (matches the manifest `kind` and the regenerate flag).
pub const KIND: &str = "features";
/// The committed golden filename.
pub const FIXTURE_BASENAME: &str = "features.json";
/// The pinned seed the checkpoint captures under (mirrors generated-expected).
pub const PINNED_SEED: i64 = 42;
/// The pinned Paper provenance the checkpoint is captured against.
pub const PINNED_PAPER: &str = "26.2-DEV-main@0a99345";
/// The serialized status a level-33 forced chunk reaches (see the module doc:
/// the forced-ticket path can only serialize FULL, and FEATURES is the last
/// block-mutating status, so FULL block data IS the decorated output). The
/// verifier pins exactly this.
pub const EXPECTED_STATUS: &str = "minecraft:full";
/// The chunk-center sample index in the 16×16 row-major arrays (`z*16+x` at the
/// center offset (8,8)).
const CENTER_INDEX: usize = 8 * 16 + 8;
/// The committed grid: the 2×2 southwest quadrant around the tree-bearing chunk
/// (4,4), a strict interior subset of the forced grid.
const GRID_MIN: i32 = 3;
const GRID_MAX: i32 = 4;
/// The forced grid: the full 4×4 neighborhood of the committed 2×2, so every
/// committed chunk's 3×3 WorldGenRegion window is forced at level 33.
const FORCE_MIN: i32 = 2;
const FORCE_MAX: i32 = 5;
/// The FEATURES-decoration evidence: tree blocks a pre-features (carvers)
/// chunk can never contain. A committed surface carrying any of these proves
/// the FEATURES step ran and placed vegetation.
const TREE_BLOCKS: &[&str] = &[
    "minecraft:oak_log",
    "minecraft:oak_leaves",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_leaves",
    "minecraft:azalea",
    "minecraft:flowering_azalea",
];
/// The union-distinct floor: a decorated normal-overworld chunk set carries
/// 30+ distinct blocks ((4,4) alone has 38 in the FULL golden); a pre-features
/// carvers set stays near the stone/sand/water/gravel floor. Anything below
/// this floor is an undecorated (or superflat-echo) capture.
const MIN_DISTINCT_UNION: usize = 15;

/// The committed golden: the per-chunk `WorldManifest` wrapped with the seed it
/// was actually captured under (the generated-expected seed-provenance shape,
/// PR #595). The `seed` field sits at the top level (`#[serde(flatten)]` keeps
/// the world fields at top level too), so it is both structurally bound —
/// verification reads it back and requires `PINNED_SEED` — and hash-bound — it
/// is inside the bytes the manifest SHA-256 covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturesGolden {
    /// The seed the golden content was generated under (the `--to` capture
    /// writes the actual seed; committed verification requires `PINNED_SEED`).
    pub seed: i64,
    #[serde(flatten)]
    pub world: WorldManifest,
}

/// The committed grid coordinates, deterministically ordered (x-major).
fn committed_coordinates() -> Vec<(i32, i32)> {
    (GRID_MIN..=GRID_MAX)
        .flat_map(|x| (GRID_MIN..=GRID_MAX).map(move |z| (x, z)))
        .collect()
}

/// The forced grid coordinates (a superset of the committed grid), ordered the
/// same way.
fn forced_coordinates() -> Vec<(i32, i32)> {
    (FORCE_MIN..=FORCE_MAX)
        .flat_map(|x| (FORCE_MIN..=FORCE_MAX).map(move |z| (x, z)))
        .collect()
}

/// Boot the pinned Paper on a fresh seed world, force the committed grid's
/// neighborhood through the FEATURES decoration (level-33 tickets → `minecraft:full`,
/// whose block data is the FEATURES-decoration output), and extract the
/// deterministic per-chunk fingerprint. A missing Paper runtime is
/// `Error::Unverified` — a missing prerequisite, never a fabricated green.
fn capture_world(seed: i64) -> Result<WorldManifest, Error> {
    let jar = crate::ensure_jar().map_err(|e| {
        Error::Unverified(format!(
            "features capture needs the pinned Paper runtime: {e} \
             (boot the M0 fixture server once per tools/rivet-oracle/README.md, or set \
             RIVET_ORACLE_JAR); UNVERIFIED, never a fabricated manifest"
        ))
    })?;
    let props = seed_properties(seed, "features")?;
    let run_dir = capture_run_dir("features");
    let committed = committed_coordinates();
    let forced = forced_coordinates();

    // boot1 (create): a plain spawn boot creates the seed world (the world must
    // exist before the forced tickets can load). The world persists in
    // run_dir/world between the two boots (the generated-expected pattern).
    crate::prepare_run_dir(&run_dir, &props)?;
    let create_log = run_dir.with_file_name("boot-features-create.log");
    println!("      [boot1] creating the seed-{seed} normal-overworld world...");
    crate::boot_and_shutdown(&run_dir, &create_log, &jar)?;

    // Discard boot1's partial spawn-area chunks so boot2 generates the forced
    // grid from a blank chunk state (byte-deterministic), exactly like the
    // generated-expected handoff.
    clear_region_files(&run_dir.join("world"), crate::OVERWORLD_DIM)?;

    // Inject level-33 forced tickets for the committed grid's neighborhood,
    // then boot2 loads those persistent chunks and finishes each through the
    // FEATURES decoration to FULL (whose block data is the decoration output).
    // Only the overworld is forced and verified (the checkpoint never consults
    // nether/end). Level 33 is `ChunkLevel.byStatus(FULL)`; a higher level is
    // `INACCESSIBLE` and the Moonrise scheduler never generates it, so FULL is
    // the forced path's ceiling.
    crate::inject_forced_tickets(
        &run_dir.join("world"),
        &forced,
        crate::OVERWORLD_DIM,
        crate::FORCED_TICKET_LEVEL,
    )?;
    let capture_log = run_dir.with_file_name("boot-features.log");
    println!("      [boot2] capturing the forced FEATURES-decoration chunks...");
    crate::boot_and_shutdown(&run_dir, &capture_log, &jar)?;
    crate::verify_forced_load(&capture_log, forced.len(), crate::OVERWORLD_DIM)?;

    // Provenance: the content about to be stamped `PINNED_PAPER` must have
    // actually been generated by that commit's server jar.
    check_capture_pin(&run_dir)?;

    let manifest = loaded_world::extract_world(&run_dir.join("world")).map_err(|e| match e {
        loaded_world::ExtractError::Unverified(m) => Error::Unverified(m),
        loaded_world::ExtractError::Gate(m) => Error::Gate(m),
        loaded_world::ExtractError::Io(io) => Error::Io(io),
    })?;
    filter_to_committed(&manifest, &committed)
}

/// Keep only the committed-grid chunks and require every one to be at the
/// expected status — a grid chunk that did not reach the features-past status
/// is a capture failure (the forced generation did not run), never content to
/// hand off.
fn filter_to_committed(
    manifest: &WorldManifest,
    grid: &[(i32, i32)],
) -> Result<WorldManifest, Error> {
    let mut chunks = BTreeMap::new();
    for (cx, cz) in grid {
        let key = format!("{cx},{cz}");
        let fp = manifest.chunks.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "forced features chunk {key} is ABSENT from the extracted world — the forced \
                 generation did not produce it; refusing to hand off a partial capture"
            ))
        })?;
        if fp.status != EXPECTED_STATUS {
            return Err(Error::Gate(format!(
                "forced features chunk {key} is {} (expected {EXPECTED_STATUS}) — the forced \
                 generation did not finish a full chunk; refusing to hand off a partial capture",
                fp.status
            )));
        }
        chunks.insert(key, fp.clone());
    }
    Ok(WorldManifest {
        format: manifest.format,
        overworld_region: manifest.overworld_region.clone(),
        chunks,
    })
}

/// Capture mode for the `--to <out>` invocation: the two-boot create +
/// forced-capture sequence (`capture_world`), written as compact JSON to `to`
/// wrapped in a `FeaturesGolden` that records the actual captured `seed`. A
/// stale `to` file is removed first so a failed capture never leaves a previous
/// success behind.
pub fn capture_to(seed: i64, to: &Path) -> Result<(), Error> {
    if to.exists() {
        fs::remove_file(to)?;
    }
    let world = capture_world(seed)?;
    // Refuse a capture that does not meet the decoration contract — a bad
    // capture must never be handed off as the checkpoint's ground truth.
    validate_world(&world)?;
    let golden = FeaturesGolden { seed, world };
    let json = serde_json::to_string(&golden)
        .map_err(|e| Error::Gate(format!("serializing features golden: {e}")))?;
    if let Some(parent) = to.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(to, json.as_bytes())?;
    println!(
        "captured seed-{seed} features golden ({} FEATURES-decoration chunks) -> {}",
        golden.world.chunks.len(),
        to.display()
    );
    Ok(())
}

/// Parse + structurally validate the committed golden (`FeaturesGolden` shape).
pub fn load(dir: &Path) -> Result<FeaturesGolden, Error> {
    let path = dir.join(FIXTURE_BASENAME);
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let golden: FeaturesGolden = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
    if golden.world.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported features format {} (expected 1)",
            golden.world.format
        )));
    }
    Ok(golden)
}

/// Assert the committed golden's provenance, manifest hashes, forced-grid shape,
/// per-chunk status/flag contract, and FEATURES-decoration non-vacuity.
pub fn verify_features(dir: &Path) -> Result<(), Error> {
    let manifest = crate::verify_fixtures(dir)?;
    // 1. Provenance: the checkpoint is pinned to seed 42 and Paper 0a99345; a
    //    fixture regenerated under a different seed or commit is drift.
    if manifest.kind.as_deref() != Some(KIND) {
        return Err(Error::Manifest(format!(
            "expected kind {KIND}, got {:?}",
            manifest.kind
        )));
    }
    if crate::parse_paper_pin(manifest.paper.as_deref()).as_deref() != Some("0a99345") {
        return Err(Error::Manifest(format!(
            "features fixture not pinned to Paper 0a99345: {:?}",
            manifest.paper
        )));
    }
    // 2. Seed provenance, bound two ways (PR #595): the manifest's seed string
    //    AND the golden's self-described seed must both be the pinned seed.
    if manifest.seed.as_deref() != Some(&PINNED_SEED.to_string()) {
        return Err(Error::Manifest(format!(
            "features fixture seed {:?} != pinned seed {PINNED_SEED}",
            manifest.seed
        )));
    }
    let golden = load(dir)?;
    if golden.seed != PINNED_SEED {
        return Err(Error::Manifest(format!(
            "features golden self-describes seed {} != pinned seed {PINNED_SEED} — the captured \
             content was generated under a different seed; refusing a wrong-seed handoff",
            golden.seed
        )));
    }

    validate_world(&golden.world)?;
    Ok(())
}

/// Assert a captured world meets the committed-grid shape, the per-chunk
/// status/flag contract, and the FEATURES-decoration non-vacuity guarantees.
/// Shared by the committed-fixture verify and the `--to` capture path.
fn validate_world(world: &WorldManifest) -> Result<(), Error> {
    let grid = committed_coordinates();
    let expected_keys: BTreeSet<String> = grid.iter().map(|(x, z)| format!("{x},{z}")).collect();
    let actual_keys: BTreeSet<String> = world.chunks.keys().cloned().collect();
    if actual_keys != expected_keys {
        return Err(Error::Manifest(format!(
            "features chunks {} != forced grid {} — a capture that adds or drops committed \
             chunks is drift, not the checkpoint",
            format_set(&actual_keys),
            format_set(&expected_keys)
        )));
    }

    let mut distinct_union: BTreeSet<&str> = BTreeSet::new();
    let mut surface_patterns: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut tree_columns = 0usize;
    let mut bedrock_non_air = 0usize;
    let mut total_columns = 0usize;
    for (key, fp) in &world.chunks {
        // The chunk's stored xPos/zPos must match the grid key — a relabeled or
        // fabricated chunk is refused.
        let parsed: [i32; 2] = {
            let mut parts = key.split(',');
            [
                parts
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(i32::MAX),
                parts
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(i32::MAX),
            ]
        };
        if fp.stored_pos != parsed {
            return Err(Error::Manifest(format!(
                "features chunk {key} has stored_pos {:?} — the chunk's internal xPos/zPos do \
                 not match its grid key; refusing a relabeled chunk",
                fp.stored_pos
            )));
        }
        // Stage-specific truth: the serialized status of a level-33 forced chunk
        // is exactly `minecraft:full`, and a FULL chunk folds no `status:` flag
        // into capability_flags (only non-FULL statuses do — loaded_world.rs
        // pushes `status:<name>` only when status != Full). Any deviation — a
        // sub-FULL status (the forced generation stopped short of FULL, hence
        // short of the decoration) or an uncarried flag — is refused, never
        // silently accepted.
        if fp.status != EXPECTED_STATUS {
            return Err(Error::Manifest(format!(
                "features chunk {key} is {} (expected {EXPECTED_STATUS}) — a chunk below full \
                 cannot be the FEATURES checkpoint (its block data is not the decorated \
                 output)",
                fp.status
            )));
        }
        if !fp.capability_flags.is_empty() {
            return Err(Error::Manifest(format!(
                "features chunk {key} carries capability flags {flags:?} — expected none (a \
                 FULL chunk has no status flag and the entities/#519 boundary is the only \
                 remaining uncarried surface); an unexpected flag is a capture the checkpoint \
                 cannot vouch for",
                flags = fp.capability_flags
            )));
        }
        // The per-chunk sample contract: 16×16 row-major `z*16+x` arrays. A short
        // array is a malformed handoff.
        for (field, arr) in [
            ("surface", &fp.surface),
            ("bedrock", &fp.bedrock),
            ("below_feet", &fp.below_feet),
        ] {
            if arr.len() != 256 {
                return Err(Error::Manifest(format!(
                    "features chunk {key} {field} array is {} entries, not the 16×16 (z*16+x) \
                     contract",
                    arr.len()
                )));
            }
        }
        distinct_union.extend(fp.distinct.iter().map(String::as_str));
        surface_patterns.insert(fp.surface.clone());
        tree_columns += fp
            .surface
            .iter()
            .filter(|b| TREE_BLOCKS.contains(&b.as_str()))
            .count();
        bedrock_non_air += fp.bedrock.iter().filter(|b| *b != "minecraft:air").count();
        total_columns += 256;
    }

    // 3. Decoration non-vacuity: the FEATURES step placed trees. A pre-features
    //    (carvers) capture has no tree block anywhere in its surface; a
    //    decorated seed-42 overworld chunk does. A capture with no tree columns
    //    cannot distinguish the decoration step from nothing, so it is refused.
    if tree_columns == 0 {
        return Err(Error::Manifest(
            "no committed FEATURES chunk carries a tree block in its surface — a pre-features \
             (carvers) capture, not a decorated one; refusing to hand off content that cannot \
             distinguish the FEATURES step from nothing"
                .into(),
        ));
    }
    // The tree-bearing chunk itself (the FULL golden proves (4,4) is decorated):
    // its surface must carry a tree block AND real terrain at the chunk-center
    // sample point.
    let tree_chunk = world.chunks.get("4,4").expect("committed grid has 4,4");
    if !tree_chunk
        .surface
        .iter()
        .any(|b| TREE_BLOCKS.contains(&b.as_str()))
    {
        return Err(Error::Manifest(
            "committed chunk 4,4 (the tree-bearing target the FULL golden shows decorated) \
             carries no tree block in its surface — the decoration evidence is absent"
                .into(),
        ));
    }
    if tree_chunk.surface[CENTER_INDEX] == "minecraft:air" {
        return Err(Error::Manifest(
            "committed chunk 4,4 has an air surface at the chunk-center sample point (index \
             {CENTER_INDEX}, center offset 8,8) — an all-air center cannot be a genuine \
             decorated terrain column"
                .into(),
        ));
    }
    // The union distinct set must clear the pre-features floor (real overworld
    // variety, not a superflat echo and not an undecorated stone/sand floor).
    if distinct_union.len() < MIN_DISTINCT_UNION {
        return Err(Error::Manifest(format!(
            "features has only {} distinct block names — below the {MIN_DISTINCT_UNION} \
             pre-features floor; a superflat echo or an undecorated capture, not decorated \
             overworld terrain",
            distinct_union.len()
        )));
    }
    // Surface variety across the grid + a depth-sampled bedrock plane: the same
    // anti-superflat dimensions the generated-expected handoff enforces.
    if surface_patterns.len() < 2 {
        return Err(Error::Manifest(
            "all committed FEATURES chunks share an identical surface array — a repeated flat \
             floor, not generated terrain"
                .into(),
        ));
    }
    if bedrock_non_air * 2 <= total_columns {
        return Err(Error::Manifest(format!(
            "features bedrock plane (y=-60) is non-air in only {bedrock_non_air}/{total_columns} \
             columns — a superflat floor has no bedrock at y=-60; refusing to hand off content \
             that cannot distinguish depth into the generated overworld floor"
        )));
    }
    Ok(())
}

fn format_set(set: &BTreeSet<String>) -> String {
    if set.len() <= 4 {
        set.iter().cloned().collect::<Vec<_>>().join(",")
    } else {
        format!(
            "{} entries ({}..{})",
            set.len(),
            set.first().unwrap(),
            set.last().unwrap()
        )
    }
}

/// `fixtures/features/manifest.json`, serialized in the exact committed field
/// order so regeneration is byte-identical (git-clean), mirroring the
/// generated-expected manifest convention.
#[derive(serde::Serialize)]
struct FeaturesManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Write `fixtures/features/manifest.json` from the freshly generated golden
/// (byte-identical field order). The seed is READ BACK OUT OF THE GOLDEN (PR
/// #595) — regeneration never stamps a hardcoded 42.
pub fn regenerate_manifest(dir: &Path) -> Result<(), Error> {
    let data = fs::read(dir.join(FIXTURE_BASENAME))?;
    let golden: FeaturesGolden = serde_json::from_slice(&data)
        .map_err(|e| Error::Manifest(format!("cannot read seed from {FIXTURE_BASENAME}: {e}")))?;
    let seed_str = golden.seed.to_string();
    let manifest = FeaturesManifest {
        format: 1,
        paper: PINNED_PAPER,
        seed: &seed_str,
        level_type: "minecraft:normal",
        kind: KIND,
        note: "Seed-42 FEATURES oracle checkpoint (PR #175/#232): the per-chunk \
               surface/bedrock/below_feet fingerprint for the committed 2x2 grid \
               {(3,3),(4,3),(3,4),(4,4)} captured from the pinned Paper runtime by booting a \
               fresh normal-overworld world and force-generating the 4x4 neighborhood \
               {2..5}x{2..5} to level 33 (ChunkLevel.byStatus(FULL)), serialized as \
               minecraft:full. FULL is the forced path's ceiling (a level-34 ticket is \
               INACCESSIBLE and never generates), and FEATURES is the last block-mutating \
               status, so a FULL serialization's block data IS the FEATURES-decoration output \
               this captures. Arrays are 16x16 row-major z*16+x; surface is the highest \
               non-air block, bedrock at y=-60, below_feet at y=-61. Non-vacuity: chunk (4,4) \
               must carry tree blocks in its surface (the decoration evidence). Regenerate with \
               `rivet-oracle regenerate --features` (twin-boot byte-identity proof).",
        captured: vec![CapturedFile {
            path: FIXTURE_BASENAME.to_string(),
            sha256: crate::sha256_hex(&data),
            bytes: data.len(),
        }],
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize features manifest: {e}")))?;
    text.push('\n');
    fs::write(dir.join("manifest.json"), text)?;
    Ok(())
}

/// Twin-boot deterministic capture into the committed fixture tree (the
/// `regenerate --features` path). Requires the two independent Paper captures
/// to produce byte-identical world manifests before anything is committed — a
/// nondeterministic pair is never committed.
pub fn run_probe(dir: &Path) -> Result<(), Error> {
    println!("[1/3] forced FEATURES capture A: fresh seed-42 Paper boot under the 1/1 pin...");
    let a = capture_world(PINNED_SEED)?;
    println!("[2/3] forced FEATURES capture B: fresh seed-42 Paper boot under the 1/1 pin...");
    let b = capture_world(PINNED_SEED)?;

    if a != b {
        return Err(Error::Gate(
            "features twin-boot byte-identity check failed — the two independent Paper \
             captures produced DIFFERENT world manifests; refusing to commit a nondeterministic \
             checkpoint"
                .into(),
        ));
    }
    // Validate the (byte-identical) capture against the decoration contract
    // BEFORE committing — two equally-wrong captures must be refused.
    validate_world(&a)?;

    println!("[3/3] byte-identical + contract-valid; writing the committed checkpoint...");
    fs::create_dir_all(dir)?;
    let golden = FeaturesGolden {
        seed: PINNED_SEED,
        world: a,
    };
    let json = serde_json::to_string(&golden)
        .map_err(|e| Error::Gate(format!("serializing features golden: {e}")))?;
    fs::write(dir.join(FIXTURE_BASENAME), json.as_bytes())?;
    regenerate_manifest(dir)?;
    println!(
        "regenerated features seed-{PINNED_SEED} checkpoint under {} (twin-boot byte-identical; \
         {} FEATURES-decoration chunks)",
        dir.display(),
        golden.world.chunks.len()
    );
    Ok(())
}

/// The tamper negative control: corrupt a committed bit pattern (flip one byte
/// of the golden JSON) and assert the verification FAILS — proving the
/// comparison is not vacuous. Operates on a scratch copy in the temp dir so the
/// committed fixtures are never mutated.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    let scratch =
        std::env::temp_dir().join(format!("rivet-oracle-features-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)
        .map_err(|e| Error::Gate(format!("cannot create scratch {}: {e}", scratch.display())))?;
    fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME))?;
    fs::copy(dir.join("manifest.json"), scratch.join("manifest.json"))?;
    let golden = scratch.join(FIXTURE_BASENAME);
    let original = fs::read(&golden)
        .map_err(|e| Error::Gate(format!("cannot read {}: {e}", golden.display())))?;
    let i = (original.len() / 2).min(original.len().saturating_sub(1));
    let mut tampered = original.clone();
    tampered[i] ^= 0xFF;
    fs::write(&golden, &tampered)?;
    let result = verify_features(&scratch);
    let _ = fs::remove_dir_all(&scratch);
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "features tamper was NOT detected — the comparison is vacuous".into(),
        }),
        Err(_) => Ok(()),
    }
}

/// The `features` subcommand:
///
///   cargo run -p rivet-oracle -- features <seed>            verify committed fixture
///   cargo run -p rivet-oracle -- features <seed> --to <out>  capture: boot Paper -> write <out>
///   cargo run -p rivet-oracle -- features <seed> --tamper    negative control
///
/// Verify mode is pinned to the committed seed-42 checkpoint (`<seed>` must be
/// 42); the `--to` capture path accepts any seed whose grid passes the
/// decoration contract. `--tamper` and `--to` are mutually exclusive.
pub fn run_cli(args: &[&str]) -> Result<(), Error> {
    let parsed = parse_cli(args)?;
    let dir = crate::crate_dir().join("fixtures/features");
    if parsed.tamper {
        return tamper_negative_control(&dir);
    }
    if let Some(to) = parsed.to {
        return capture_to(parsed.seed, &to);
    }
    // Verify mode only understands the committed seed-42 checkpoint. A different
    // seed is a usage error (the committed fixture is pinned to 42), not a
    // silent verify of the wrong reference.
    if parsed.seed != PINNED_SEED {
        return Err(Error::Gate(format!(
            "features verify is pinned to seed {PINNED_SEED}; got {} — the committed \
             checkpoint only carries the seed-42 ground truth",
            parsed.seed
        )));
    }
    verify_features(&dir)?;
    println!(
        "PASS: features seed-42 FEATURES checkpoint verified (pinned Paper 0a99345 provenance, \
         manifest hash, forced-grid per-chunk status/decoration-sample contract, non-vacuity)"
    );
    Ok(())
}

/// Parsed `features` CLI arguments.
struct CliArgs {
    seed: i64,
    to: Option<PathBuf>,
    tamper: bool,
}

/// Parse the `features` arguments (everything after the subcommand name). A
/// malformed seed or unknown option is a usage error — `Error::Gate`, never
/// `Error::Unverified`.
fn parse_cli(rest: &[&str]) -> Result<CliArgs, Error> {
    let mut seed: Option<i64> = None;
    let mut to: Option<PathBuf> = None;
    let mut tamper = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--to" => {
                let Some(path) = rest.get(i + 1) else {
                    return Err(Error::Gate(
                        "features --to requires a destination path".into(),
                    ));
                };
                // An option token is never a destination: `--to --tamper` is a
                // usage error, not a capture written to a file named `--tamper`.
                if path.starts_with('-') {
                    return Err(Error::Gate(
                        "features --to requires a destination path, not an option".into(),
                    ));
                }
                to = Some(PathBuf::from(path));
                i += 2;
            }
            "--tamper" => {
                tamper = true;
                i += 1;
            }
            other if !other.starts_with('-') => {
                if seed.is_some() {
                    return Err(Error::Gate("features takes exactly one seed".into()));
                }
                seed = Some(other.parse().map_err(|_| {
                    Error::Gate(format!("features seed {other} is not an integer"))
                })?);
                i += 1;
            }
            // A negative seed (`-5`) must be read as a seed, not an unknown
            // option — the `--to` capture accepts any i64 seed (the decoration
            // contract still refuses an undecorated grid).
            other
                if other.starts_with('-')
                    && !other.starts_with("--")
                    && other[1..].parse::<i64>().is_ok() =>
            {
                if seed.is_some() {
                    return Err(Error::Gate("features takes exactly one seed".into()));
                }
                seed = Some(other.parse().map_err(|_| {
                    Error::Gate(format!("features seed {other} is not an integer"))
                })?);
                i += 1;
            }
            other => {
                return Err(Error::Gate(format!("features: unknown option {other}")));
            }
        }
    }
    let seed = seed.ok_or_else(|| Error::Gate("features requires a seed".into()))?;
    if tamper && to.is_some() {
        return Err(Error::Gate(
            "features --tamper and --to are mutually exclusive".into(),
        ));
    }
    Ok(CliArgs { seed, to, tamper })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// The committed features golden is a load-bearing deliverable: a test that
    /// needs it must FAIL when it is absent, never silently return (D8).
    fn require_fixture(dir: &Path) {
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed features fixtures {} are ABSENT — the seed-42 FEATURES checkpoint \
                 and its gate cannot verify; restore them (git checkout) or this test is red, \
                 never silently skipped",
                dir.display()
            );
        }
    }

    #[test]
    fn committed_grid_is_the_two_by_two_decoration_set() {
        let grid = committed_coordinates();
        assert_eq!(grid.len(), 4);
        assert!(grid.contains(&(3, 3)));
        assert!(grid.contains(&(4, 3)));
        assert!(grid.contains(&(3, 4)));
        assert!(grid.contains(&(4, 4)));
        assert!(!grid.contains(&(2, 2)));
        assert!(!grid.contains(&(5, 5)));
    }

    #[test]
    fn forced_grid_is_the_full_neighborhood_of_the_committed_grid() {
        let committed = committed_coordinates();
        let forced = forced_coordinates();
        // Every committed chunk's 3x3 WorldGenRegion window is forced: all 8
        // neighbors of every committed chunk are in the forced grid.
        for (cx, cz) in &committed {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    assert!(
                        forced.contains(&(cx + dx, cz + dz)),
                        "neighbor ({},{}) of committed chunk ({cx},{cz}) is not forced",
                        cx + dx,
                        cz + dz
                    );
                }
            }
        }
    }

    #[test]
    fn committed_generated_expected_shows_the_4_4_chunk_is_tree_bearing() {
        // Cross-fixture grounding: the FULL golden's chunk (4,4) is decorated
        // (the very reason this checkpoint's target is non-vacuous). If the
        // committed golden ever loses that, this checkpoint's premise is gone.
        let dir = fixtures_dir().join("generated-expected");
        assert!(
            dir.join("manifest.json").is_file(),
            "generated-expected fixtures are ABSENT — the FEATURES checkpoint's decoration \
             grounding cannot be checked"
        );
        let golden = crate::generated_expected::load(&dir).expect("generated-expected loads");
        let fp = golden
            .world
            .chunks
            .get("4,4")
            .expect("generated-expected grid has 4,4");
        assert!(
            fp.surface.iter().any(|b| TREE_BLOCKS.contains(&b.as_str())),
            "the FULL golden's chunk 4,4 is not tree-bearing — the FEATURES checkpoint target \
             would be vacuous"
        );
    }

    #[test]
    fn committed_features_verifies() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        verify_features(&dir).expect("committed features golden should verify");
    }

    #[test]
    fn committed_features_is_non_vacuous() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden = load(&dir).unwrap();
        assert_eq!(golden.seed, PINNED_SEED);
        let world = &golden.world;
        assert_eq!(world.format, 1);
        assert_eq!(world.chunks.len(), 4);
        for (key, fp) in &world.chunks {
            assert_eq!(fp.status, EXPECTED_STATUS, "chunk {key}");
            assert!(
                fp.capability_flags.is_empty(),
                "chunk {key} must carry no capability flags (a FULL chunk has no status flag)"
            );
            assert_eq!(fp.surface.len(), 256, "chunk {key}");
            assert_eq!(fp.bedrock.len(), 256, "chunk {key}");
            assert_eq!(fp.below_feet.len(), 256, "chunk {key}");
            assert!(fp.section_count >= 1, "chunk {key}");
            assert!(fp.distinct_state_ids >= 1, "chunk {key}");
        }
        // The tree-bearing target is decorated (the FEATURES evidence).
        let tree_chunk = world.chunks.get("4,4").unwrap();
        assert!(
            tree_chunk
                .surface
                .iter()
                .any(|b| TREE_BLOCKS.contains(&b.as_str())),
            "chunk 4,4 carries no tree block in its surface"
        );
        assert_ne!(tree_chunk.surface[CENTER_INDEX], "minecraft:air");
        // Genuine overworld variety, not a superflat echo.
        let distinct: BTreeSet<&str> = world
            .chunks
            .values()
            .flat_map(|fp| fp.distinct.iter().map(String::as_str))
            .collect();
        assert!(distinct.len() >= MIN_DISTINCT_UNION, "only {distinct:?}");
    }

    /// The default verify path must fail UNVERIFIED when the committed fixture
    /// tree is absent — never silently skip (D8).
    #[test]
    fn missing_fixture_tree_is_unverified() {
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-missing-{}",
            std::process::id()
        ));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        let result = crate::verify_features_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    /// A wrong-seed golden — content generated under a seed OTHER than the pinned
    /// 42 — must be refused even when its manifest SHA-256 is freshly rebuilt
    /// around it (PR #595).
    #[test]
    fn wrong_seed_golden_is_rejected_even_with_fresh_hash() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        golden.seed = 999;

        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-wrongseed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string(&golden).unwrap(),
        )
        .unwrap();
        let data = fs::read(scratch.join(FIXTURE_BASENAME)).unwrap();
        let manifest = serde_json::json!({
            "format": 1,
            "paper": PINNED_PAPER,
            "seed": "42",
            "level-type": "minecraft:normal",
            "kind": KIND,
            "note": "test",
            "captured": [{ "path": FIXTURE_BASENAME, "sha256": crate::sha256_hex(&data), "bytes": data.len() }],
        });
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let result = verify_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("a wrong-seed golden must be refused by the seed gate");
        let msg = err.to_string();
        assert!(
            msg.contains("self-describes seed") && msg.contains("999"),
            "unexpected error: {msg}"
        );
    }

    /// Regenerating the manifest in Rust is byte-identical to the committed
    /// manifest (given an unchanged golden) — regeneration is git-clean.
    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-regen-{}",
            std::process::id()
        ));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME)).unwrap();
        regenerate_manifest(&scratch).unwrap();
        let committed = fs::read(dir.join("manifest.json")).unwrap();
        let regenerated = fs::read(scratch.join("manifest.json")).unwrap();
        assert_eq!(
            committed, regenerated,
            "regenerating the features manifest must be byte-identical (git-clean)"
        );
        crate::verify_fixtures(&scratch).unwrap();
        let _ = fs::remove_dir_all(&scratch);
    }

    /// `regenerate_manifest` reads the seed OUT of the golden rather than
    /// stamping `PINNED_SEED` (PR #595).
    #[test]
    fn regenerate_manifest_reads_seed_from_golden() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-regen-seed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        let mut golden = load(&dir).unwrap();
        golden.seed = 999;
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string(&golden).unwrap(),
        )
        .unwrap();
        regenerate_manifest(&scratch).unwrap();
        let text = fs::read_to_string(scratch.join("manifest.json")).unwrap();
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            text.contains("\"seed\": \"999\""),
            "regenerated manifest must carry the golden's actual seed, got: {text}"
        );
    }

    /// Write `world` into a scratch dir under a valid hash-gated manifest and
    /// run the full verify against it. Returns the verify error (the caller
    /// asserts it is a rejection).
    fn verify_scratch_world(world: &WorldManifest, tag: &str) -> crate::Error {
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        let golden = FeaturesGolden {
            seed: PINNED_SEED,
            world: world.clone(),
        };
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string(&golden).unwrap(),
        )
        .unwrap();
        let data = fs::read(scratch.join(FIXTURE_BASENAME)).unwrap();
        let manifest = serde_json::json!({
            "format": 1,
            "paper": PINNED_PAPER,
            "seed": "42",
            "level-type": "minecraft:normal",
            "kind": KIND,
            "note": "test",
            "captured": [{ "path": FIXTURE_BASENAME, "sha256": crate::sha256_hex(&data), "bytes": data.len() }],
        });
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let result = verify_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        result.expect_err("the checkpoint must be rejected by the decoration contract")
    }

    /// A WorldManifest with the committed 2x2 grid at the given status; each
    /// chunk carries an `initial`-distinct-block set so only the status/tree
    /// assertions discriminate. A FULL chunk folds no `status:` flag into
    /// capability_flags (loaded_world.rs pushes it only for non-FULL statuses),
    /// so the helper mirrors that: the flag is set only below full.
    fn grid_world(status: &str, tree_surface: bool) -> WorldManifest {
        let mut chunks = BTreeMap::new();
        for (x, z) in committed_coordinates() {
            let mut surface = vec!["minecraft:sand".to_owned(); 256];
            let mut distinct = vec![
                "minecraft:air".to_owned(),
                "minecraft:sand".to_owned(),
                "minecraft:stone".to_owned(),
                "minecraft:water".to_owned(),
                "minecraft:gravel".to_owned(),
                "minecraft:deepslate".to_owned(),
                "minecraft:bedrock".to_owned(),
                "minecraft:diorite".to_owned(),
                "minecraft:andesite".to_owned(),
                "minecraft:granite".to_owned(),
                "minecraft:tuff".to_owned(),
                "minecraft:dirt".to_owned(),
                "minecraft:grass_block".to_owned(),
                "minecraft:short_grass".to_owned(),
                "minecraft:moss_block".to_owned(),
                "minecraft:cave_vines".to_owned(),
            ];
            if tree_surface {
                surface[136] = "minecraft:oak_log".to_owned();
                distinct.push("minecraft:oak_log".to_owned());
                distinct.push("minecraft:oak_leaves".to_owned());
                distinct.push("minecraft:dark_oak_leaves".to_owned());
            }
            chunks.insert(
                format!("{x},{z}"),
                crate::loaded_world::ChunkFingerprint {
                    status: status.to_owned(),
                    stored_pos: [x, z],
                    capability_flags: if status == "minecraft:full" {
                        Vec::new()
                    } else {
                        vec![format!("status:{status}")]
                    },
                    distinct,
                    surface,
                    bedrock: vec!["minecraft:bedrock".to_owned(); 256],
                    below_feet: vec!["minecraft:stone".to_owned(); 256],
                    distinct_state_ids: 8,
                    section_count: 2,
                },
            );
        }
        WorldManifest {
            format: 1,
            overworld_region: "dimensions/minecraft/overworld/region".to_owned(),
            chunks,
        }
    }

    /// A capture whose chunks never reached FULL must be rejected even when the
    /// block content looks decorated — the status is stage-specific truth the
    /// checkpoint pins exactly (a sub-FULL serialization's block data is not
    /// the FEATURES-decoration output).
    #[test]
    fn pre_features_status_is_rejected() {
        let world = grid_world("minecraft:carvers", true);
        let err = verify_scratch_world(&world, "pre-features");
        let msg = err.to_string();
        assert!(
            msg.contains("carvers") && msg.contains("minecraft:full"),
            "unexpected: {msg}"
        );
    }

    /// A capture at the right status but with no tree blocks anywhere must be
    /// rejected — no decoration evidence means the FEATURES step cannot be
    /// distinguished from nothing.
    #[test]
    fn undecorated_grid_is_rejected() {
        let world = grid_world(EXPECTED_STATUS, false);
        let err = verify_scratch_world(&world, "undecorated");
        let msg = err.to_string();
        assert!(msg.contains("tree block"), "unexpected: {msg}");
    }

    /// A relabeled chunk — one whose internal `stored_pos` does not match its
    /// grid key — must be rejected even when every other chunk is the genuine
    /// committed golden.
    #[test]
    fn relabeled_chunk_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let fp = golden.world.chunks.get_mut("3,3").expect("grid chunk 3,3");
        fp.stored_pos = [4, 3];
        let err = verify_scratch_world(&golden.world, "relabeled");
        let msg = err.to_string();
        assert!(
            msg.contains("stored_pos") && msg.contains("relabeled"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn cli_parses_seed_to_and_tamper() {
        let parsed = parse_cli(&["42", "--to", "/tmp/out.json"]).unwrap();
        assert_eq!(parsed.seed, 42);
        assert_eq!(parsed.to, Some(PathBuf::from("/tmp/out.json")));
        assert!(!parsed.tamper);

        let parsed = parse_cli(&["42", "--tamper"]).unwrap();
        assert_eq!(parsed.seed, 42);
        assert!(parsed.tamper);

        let parsed = parse_cli(&["42"]).unwrap();
        assert_eq!(parsed.seed, 42);
        assert!(parsed.to.is_none());

        let parsed = parse_cli(&["-5", "--to", "/tmp/out.json"]).unwrap();
        assert_eq!(parsed.seed, -5);
        assert_eq!(parsed.to, Some(PathBuf::from("/tmp/out.json")));
    }

    #[test]
    fn cli_rejects_malformed_args_as_gate() {
        assert!(matches!(parse_cli(&[]), Err(crate::Error::Gate(_))));
        assert!(matches!(parse_cli(&["abc"]), Err(crate::Error::Gate(_))));
        assert!(matches!(
            parse_cli(&["42", "43"]),
            Err(crate::Error::Gate(_))
        ));
        assert!(matches!(
            parse_cli(&["42", "--to"]),
            Err(crate::Error::Gate(_))
        ));
        assert!(matches!(
            parse_cli(&["42", "--to", "--tamper"]),
            Err(crate::Error::Gate(_))
        ));
        assert!(matches!(
            parse_cli(&["42", "--bogus"]),
            Err(crate::Error::Gate(_))
        ));
        assert!(matches!(parse_cli(&["--5"]), Err(crate::Error::Gate(_))));
        assert!(matches!(
            parse_cli(&["42", "--tamper", "--to", "/tmp/x.json"]),
            Err(crate::Error::Gate(_))
        ));
    }

    /// Verify mode is pinned to seed 42: a different seed is a usage error, not
    /// a silent verify of the wrong reference.
    #[test]
    fn verify_mode_rejects_non_42_seed() {
        let err = run_cli(&["999"]).expect_err("verify must refuse a non-42 seed");
        assert!(matches!(err, crate::Error::Gate(_)));
        let msg = err.to_string();
        assert!(msg.contains("seed 42"), "unexpected error: {msg}");
    }
}
