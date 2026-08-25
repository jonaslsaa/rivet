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
//! tree-bearing chunk (4,4) — a compact, decoration-dense interior slice of the
//! established generated-expected handoff (whose committed grid is {-4..=4}).
//! The FORCED grid is generated-expected's {-6..=6}² regime, not a smaller local
//! neighborhood. That matters because the FEATURES step is declared with
//! `blockStateWriteRadius(1)` (ChunkPyramid): a chunk's decoration writes one
//! chunk into each neighbor, and a border tree observed inside a committed chunk
//! on its east/south edge is actually placed by the neighbor chunk's own FEATURES
//! pass. Whether that neighbor's pass runs (and with what surrounding context) is
//! a function of the forced-grid boundary, so a small pad captures a different
//! edge regime than the canonical golden: the committed chunks then diverge from
//! generated-expected at the very x=14/15, z=14/15 spill columns. Forcing the
//! identical {-6..=6}² grid as the established golden makes every committed
//! chunk's transitive decoration context (the writers into it, their radius-1
//! windows, and the spill across the grid edges) byte-identical to that golden,
//! so the committed chunks ARE the canonical seed-42 FEATURES output — and the
//! verifier enforces that by cross-checking each committed chunk against
//! generated-expected at the same coordinates. The forced chunks are all held at
//! level 33 (FULL), past FEATURES, so a center chunk's decoration inputs
//! (neighbor heightmaps/surface, already settled by carvers) are complete before
//! its FEATURES task runs.
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
//! The checkpoint ALSO pins the leaf features this checkpoint exists to cover,
//! through a features-only observation layer (`feature_observations` on the
//! golden, kept OUT of the shared `WorldManifest` so generated-expected's
//! cross-check byte-identity is untouched): the positional occurrences of
//! `magma_block` (UnderwaterMagmaFeature, PR #644) and `glow_lichen`
//! (MultifaceGrowthFeature/`glow_lichen`, PR #645). `surface`/`bedrock`/
//! `below_feet` do not locate these — magma sits on the ocean floor below the
//! surface water and glow_lichen attaches in the water column/caves — so they
//! are recorded as `{block, index (z*16+x), y}` observations. Validation
//! requires a `magma_block` in a submerged column (`surface[index] == water`),
//! the pinned UnderwaterMagma ocean-floor signature, and at least one
//! `glow_lichen`. Tamper negatives
//! prove removing either set, or relocating magma off the ocean floor, is
//! detected. This is feature-leaf evidence only; it does not claim full
//! block-volume or FULL parity for the generated world.
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
//!   * verify (and the regenerate path) cross-check every committed chunk
//!     against the generated-expected golden at the same coordinates: the
//!     committed FEATURES output must be byte-identical to the canonical
//!     handoff, or the checkpoint has silently regressed to a capture-regime
//!     artifact. The generated-expected fixture is a committed repo file; its
//!     absence (or a divergent chunk) is damage, never a silent skip.
//!   * the tamper negative control proves a flipped byte in the golden fails
//!     verification (the manifest SHA-256 gate is not vacuous).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::generated_expected::{
    capture_run_dir, check_capture_pin, clear_region_files, seed_properties,
};
use crate::loaded_world::{self, WorldManifest};

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
/// (4,4), a strict interior subset of the generated-expected committed grid.
const GRID_MIN: i32 = 3;
const GRID_MAX: i32 = 4;
/// The forced grid: generated-expected's {-6..=6}² regime (aliased so the two
/// captures cannot drift apart). See the module doc: the FEATURES step's
/// `blockStateWriteRadius(1)` makes a chunk's border trees spill from its
/// neighbors, and only forcing the same grid as the established golden makes the
/// committed chunks byte-identical to it.
const FORCE_MIN: i32 = crate::generated_expected::FORCE_GRID_MIN;
const FORCE_MAX: i32 = crate::generated_expected::FORCE_GRID_MAX;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeaturesGolden {
    /// The seed the golden content was generated under (the `--to` capture
    /// writes the actual seed; committed verification requires `PINNED_SEED`).
    pub seed: i64,
    #[serde(flatten)]
    pub world: WorldManifest,
    /// The positional occurrences of the FEATURES-palette blocks
    /// (`magma_block` = UnderwaterMagmaFeature, `glow_lichen` =
    /// MultifaceGrowthFeature), keyed by committed chunk `"<x>,<z>"`. This is
    /// a features-only evidence layer: the shared `WorldManifest` fingerprint
    /// (surface/bedrock/below_feet) does not locate these because they sit
    /// below the surface water / in the water column, so the committed
    /// checkpoint pins them here. The field is required: an older golden without
    /// this evidence is not a valid checkpoint, and the decoration contract also
    /// refuses a capture without either leaf's observations.
    pub feature_observations: BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeaturesGoldenWire {
    seed: i64,
    format: u32,
    overworld_region: String,
    chunks: BTreeMap<String, loaded_world::ChunkFingerprint>,
    feature_observations: BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
}

impl<'de> Deserialize<'de> for FeaturesGolden {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = FeaturesGoldenWire::deserialize(deserializer)?;
        Ok(Self {
            seed: wire.seed,
            world: WorldManifest {
                format: wire.format,
                overworld_region: wire.overworld_region,
                chunks: wire.chunks,
            },
            feature_observations: wire.feature_observations,
        })
    }
}

/// The committed grid coordinates, deterministically ordered (x-major).
fn committed_coordinates() -> Vec<(i32, i32)> {
    (GRID_MIN..=GRID_MAX)
        .flat_map(|x| (GRID_MIN..=GRID_MAX).map(move |z| (x, z)))
        .collect()
}

/// The forced grid coordinates: generated-expected's {-6..=6}² regime, ordered
/// identically (a superset of the committed grid). Generating the committed
/// chunks under the exact same forced-grid context as the established golden is
/// what makes them the canonical seed-42 FEATURES output.
fn forced_coordinates() -> Vec<(i32, i32)> {
    (FORCE_MIN..=FORCE_MAX)
        .flat_map(|x| (FORCE_MIN..=FORCE_MAX).map(move |z| (x, z)))
        .collect()
}

/// Boot the pinned Paper on a fresh seed world, force the committed grid's
/// neighborhood through the FEATURES decoration (level-33 tickets → `minecraft:full`,
/// whose block data is the FEATURES-decoration output), and extract the
/// deterministic per-chunk fingerprint plus the feature-palette observations. A
/// missing Paper runtime is `Error::Unverified` — a missing prerequisite, never
/// a fabricated green.
fn capture_world(
    seed: i64,
) -> Result<
    (
        WorldManifest,
        BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
    ),
    Error,
> {
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
    let observations = loaded_world::extract_feature_observations(&run_dir.join("world")).map_err(
        |e| match e {
            loaded_world::ExtractError::Unverified(m) => Error::Unverified(m),
            loaded_world::ExtractError::Gate(m) => Error::Gate(m),
            loaded_world::ExtractError::Io(io) => Error::Io(io),
        },
    )?;
    let world = filter_to_committed(&manifest, &committed)?;
    let obs = filter_observations_to_committed(&observations, &committed);
    Ok((world, obs))
}

/// Keep only the committed-grid chunks' feature observations.
fn filter_observations_to_committed(
    observations: &BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
    grid: &[(i32, i32)],
) -> BTreeMap<String, Vec<loaded_world::FeatureObservation>> {
    let mut out = BTreeMap::new();
    for (cx, cz) in grid {
        if let Some(obs) = observations.get(&format!("{cx},{cz}")) {
            out.insert(format!("{cx},{cz}"), obs.clone());
        }
    }
    out
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
    let (world, observations) = capture_world(seed)?;
    // Refuse a capture that does not meet the decoration contract — a bad
    // capture must never be handed off as the checkpoint's ground truth.
    validate_world(&world)?;
    validate_observations(&world, &observations)?;
    let golden = FeaturesGolden {
        seed,
        world,
        feature_observations: observations,
    };
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
    crate::reject_duplicate_json_keys(raw.as_bytes())
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
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

/// Parse the features manifest with its exact schema. The generic fixture
/// verifier intentionally accepts several historical manifest shapes; this
/// checkpoint does not, because provenance is part of the evidence.
fn load_features_manifest(dir: &Path) -> Result<FeaturesManifest, Error> {
    let path = dir.join("manifest.json");
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    crate::reject_duplicate_json_keys(raw.as_bytes())
        .map_err(|e| Error::Manifest(format!("invalid features manifest schema: {e}")))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid features manifest schema: {e}")))
}

/// Assert the committed golden's provenance, manifest hashes, forced-grid shape,
/// per-chunk status/flag contract, and FEATURES-decoration non-vacuity.
pub fn verify_features(dir: &Path) -> Result<(), Error> {
    let strict_manifest = load_features_manifest(dir)?;
    if strict_manifest.format != 1
        || strict_manifest.paper != PINNED_PAPER
        || strict_manifest.seed != PINNED_SEED.to_string()
        || strict_manifest.level_type != "minecraft:normal"
        || strict_manifest.kind != KIND
    {
        return Err(Error::Manifest(format!(
            "features manifest provenance must be exactly format=1, paper={PINNED_PAPER}, seed={PINNED_SEED}, level-type=minecraft:normal, kind={KIND}; got format={}, paper={:?}, seed={:?}, level-type={:?}, kind={:?}",
            strict_manifest.format,
            strict_manifest.paper,
            strict_manifest.seed,
            strict_manifest.level_type,
            strict_manifest.kind
        )));
    }
    let manifest = crate::verify_fixtures(dir)?;
    // 0. The SHA-256 binding is load-bearing, not optional: verify_fixtures only
    //    checks the files the manifest DOES list, so a manifest with no captured
    //    entry (or one that omits features.json) would let a modified-but-still-
    //    valid golden pass with zero byte binding. Require exactly one non-empty
    //    captured entry naming the golden.
    crate::require_single_captured(&manifest, FIXTURE_BASENAME)?;
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
    validate_observations(&golden.world, &golden.feature_observations)?;
    validate_canonical_observations(&golden.feature_observations)?;
    cross_check_generated_expected(&golden.world)?;
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

/// The block the UnderwaterMagmaFeature places.
const MAGMA_BLOCK: &str = "minecraft:magma_block";
/// The block MultifaceGrowthFeature places for the `glow_lichen` configured
/// feature.
const GLOW_LICHEN_BLOCK: &str = "minecraft:glow_lichen";

/// Assert the feature-palette observations pin both the leaf features this
/// checkpoint exists to cover: UnderwaterMagmaFeature (`magma_block`) and
/// MultifaceGrowthFeature (`glow_lichen`). This is the non-vacuity for PRs
/// #644/#645 — a capture whose feature blocks cannot be attributed to them (or
/// that carries none) is refused, never handed off as ground truth.
///
/// Underwater-magma signature: the observed `magma_block` must sit in a
/// submerged column, whose `surface` entry is water. That pins the Paper
/// observation to an ocean-floor placement without making an implementation
/// claim about every possible magma producer.
///
/// Glow-lichen evidence is the positional observation of the configured
/// `glow_lichen` feature's output; this checkpoint does not claim full feature
/// dispatch or generated-world parity.
pub fn validate_observations(
    world: &WorldManifest,
    observations: &BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
) -> Result<(), Error> {
    // Observations are scoped to the committed grid. Rejecting an unknown key
    // prevents a tampered fixture from hiding evidence in an ignored chunk.
    for key in observations.keys() {
        if !world.chunks.contains_key(key) {
            return Err(Error::Manifest(format!(
                "features observation key {key} is not a committed chunk — refusing evidence outside the checkpoint grid"
            )));
        }
    }

    // Central diagnostics: nothing to count below unless a structurally valid
    // observation exists.
    let mut magma_count = 0usize;
    let mut submerged_magma = 0usize;
    let mut lichen_count = 0usize;
    for (key, fp) in &world.chunks {
        let Some(obs) = observations.get(key) else {
            continue;
        };
        let mut positions = BTreeSet::new();
        for o in obs {
            if o.index >= 16 * 16 {
                return Err(Error::Manifest(format!(
                    "features observation in chunk {key} has column index {} outside the z*16+x 16x16 contract",
                    o.index
                )));
            }
            if !(-64..=319).contains(&o.y) {
                return Err(Error::Manifest(format!(
                    "features observation in chunk {key} has y={} outside the captured overworld section bounds [-64,319]",
                    o.y
                )));
            }
            if !positions.insert((o.index, o.y)) {
                return Err(Error::Manifest(format!(
                    "features observation in chunk {key} duplicates payload position index={}, y={}",
                    o.index, o.y
                )));
            }
            match o.block.as_str() {
                MAGMA_BLOCK => {
                    magma_count += 1;
                    // The column's surface (highest non-air) must be water: the
                    // magma sits on the floor of a submerged column.
                    if fp
                        .surface
                        .get(o.index)
                        .is_some_and(|surface| surface == "minecraft:water")
                    {
                        submerged_magma += 1;
                    }
                }
                GLOW_LICHEN_BLOCK => lichen_count += 1,
                other => {
                    return Err(Error::Manifest(format!(
                        "features observation in chunk {key} contains unsupported block {other:?}"
                    )));
                }
            }
        }
    }

    if magma_count == 0 {
        return Err(Error::Manifest(
            "no committed FEATURES chunk observes a magma_block — UnderwaterMagmaFeature's \
             ocean-floor placement is absent; the #644 leaf is vacuous"
                .into(),
        ));
    }
    if submerged_magma == 0 {
        return Err(Error::Manifest(
            "no observed magma_block sits in a submerged column (surface=water) — the \
             UnderwaterMagmaFeature ocean-floor signature is absent"
                .into(),
        ));
    }
    if lichen_count == 0 {
        return Err(Error::Manifest(
            "no committed FEATURES chunk observes a glow_lichen — MultifaceGrowthFeature's \
             glow_lichen placement is absent; the #645 leaf is vacuous"
                .into(),
        ));
    }
    Ok(())
}

/// The exact positional feature tuples captured from the pinned seed-42 Paper
/// payload. The generic observation contract above proves the shape and leaf
/// non-vacuity; this second gate binds the observations to the canonical payload
/// geometry so a tampered fixture cannot relocate blocks to another water column
/// or recompute a local count/hash and still pass.
fn canonical_observations() -> BTreeMap<&'static str, Vec<(&'static str, usize, i32)>> {
    BTreeMap::from([
        (
            "3,3",
            vec![
                (GLOW_LICHEN_BLOCK, 114, -33),
                (GLOW_LICHEN_BLOCK, 212, -5),
                (GLOW_LICHEN_BLOCK, 143, 15),
                (GLOW_LICHEN_BLOCK, 125, 16),
                (GLOW_LICHEN_BLOCK, 148, 30),
                (GLOW_LICHEN_BLOCK, 2, 50),
            ],
        ),
        (
            "3,4",
            vec![
                (GLOW_LICHEN_BLOCK, 150, 8),
                (GLOW_LICHEN_BLOCK, 151, 12),
                (GLOW_LICHEN_BLOCK, 151, 13),
                (GLOW_LICHEN_BLOCK, 139, 29),
                (GLOW_LICHEN_BLOCK, 156, 29),
                (MAGMA_BLOCK, 3, 7),
                (MAGMA_BLOCK, 4, 7),
                (MAGMA_BLOCK, 5, 7),
                (MAGMA_BLOCK, 20, 7),
                (MAGMA_BLOCK, 21, 8),
            ],
        ),
        (
            "4,3",
            vec![
                (GLOW_LICHEN_BLOCK, 151, -51),
                (GLOW_LICHEN_BLOCK, 159, -50),
                (GLOW_LICHEN_BLOCK, 174, -50),
                (GLOW_LICHEN_BLOCK, 216, -43),
                (GLOW_LICHEN_BLOCK, 198, -1),
                (GLOW_LICHEN_BLOCK, 128, 15),
            ],
        ),
    ])
}

fn validate_canonical_observations(
    observations: &BTreeMap<String, Vec<loaded_world::FeatureObservation>>,
) -> Result<(), Error> {
    let expected = canonical_observations();
    let actual: BTreeMap<String, Vec<(&str, usize, i32)>> = observations
        .iter()
        .map(|(key, values)| {
            let mut tuples: Vec<_> = values
                .iter()
                .map(|o| (o.block.as_str(), o.index, o.y))
                .collect();
            tuples.sort_unstable();
            (key.clone(), tuples)
        })
        .collect();
    let expected: BTreeMap<String, Vec<(&str, usize, i32)>> = expected
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_unstable();
            (key.to_owned(), values)
        })
        .collect();
    if actual != expected {
        return Err(Error::Manifest(format!(
            "features observations diverge from the canonical seed-42 payload geometry: expected {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictGeneratedExpectedManifest {
    format: u64,
    paper: String,
    seed: String,
    #[serde(rename = "level-type")]
    level_type: String,
    kind: String,
    #[serde(rename = "note")]
    _note: String,
    captured: Vec<StrictCapturedFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictCapturedFile {
    path: String,
    sha256: String,
    bytes: u64,
}

/// Load and fully verify the generated-expected dependency for the FEATURES
/// cross-check.
///
/// This dependency is distinct from the FEATURES fixture tree: either missing
/// file is partial/tampered handoff damage, not an absent prerequisite. Check
/// both before invoking the dependency verifier so its loader-level
/// `Unverified` classification cannot leak through this checkpoint as exit 3.
fn load_generated_expected_for_features(
    dir: &Path,
) -> Result<crate::generated_expected::GoldenWorld, Error> {
    let manifest = dir.join("manifest.json");
    let golden = dir.join(crate::generated_expected::FIXTURE_BASENAME);
    if !manifest.is_file() || !golden.is_file() {
        return Err(Error::Manifest(format!(
            "features cross-check needs a complete generated-expected golden in {} (both \
             manifest.json and {}) — a missing dependency file is partial/tampered fixture damage, \
             not an absent prerequisite; refusing to classify it as UNVERIFIED",
            dir.display(),
            crate::generated_expected::FIXTURE_BASENAME
        )));
    }

    let raw_manifest = fs::read_to_string(&manifest)?;
    crate::reject_duplicate_json_keys(raw_manifest.as_bytes()).map_err(|e| {
        Error::Manifest(format!("invalid generated-expected manifest.json: {e}"))
    })?;
    let strict: StrictGeneratedExpectedManifest = serde_json::from_str(&raw_manifest)
        .map_err(|e| Error::Manifest(format!("invalid generated-expected manifest.json: {e}")))?;
    if strict.format != 1 {
        return Err(Error::Manifest(format!(
            "generated-expected manifest format {} != 1",
            strict.format
        )));
    }
    if strict.paper != crate::generated_expected::PINNED_PAPER {
        return Err(Error::Manifest(format!(
            "generated-expected dependency paper {:?} != pinned {}",
            strict.paper,
            crate::generated_expected::PINNED_PAPER
        )));
    }
    if strict.seed != crate::generated_expected::PINNED_SEED.to_string() {
        return Err(Error::Manifest(format!(
            "generated-expected dependency seed {:?} != pinned {}",
            strict.seed,
            crate::generated_expected::PINNED_SEED
        )));
    }
    if strict.level_type != "minecraft:normal" {
        return Err(Error::Manifest(format!(
            "generated-expected dependency level-type {:?} is not minecraft:normal — the FEATURES \
             checkpoint requires the normal-overworld ground-truth handoff",
            strict.level_type
        )));
    }
    if strict.kind != crate::generated_expected::KIND {
        return Err(Error::Manifest(format!(
            "generated-expected dependency kind {:?} != {}",
            strict.kind,
            crate::generated_expected::KIND
        )));
    }
    crate::require_single_captured(
        &crate::load_manifest(dir)?,
        crate::generated_expected::FIXTURE_BASENAME,
    )?;
    let captured = strict.captured.first().ok_or_else(|| {
        Error::Manifest(format!(
            "generated-expected manifest must bind exactly one captured entry for {}",
            crate::generated_expected::FIXTURE_BASENAME
        ))
    })?;
    if strict.captured.len() != 1
        || captured.path != crate::generated_expected::FIXTURE_BASENAME
        || captured.sha256.is_empty()
        || captured.bytes == 0
    {
        return Err(Error::Manifest(format!(
            "generated-expected manifest must bind exactly one non-empty captured entry for {}",
            crate::generated_expected::FIXTURE_BASENAME
        )));
    }

    crate::generated_expected::verify_generated_expected_step(dir)?;
    crate::generated_expected::load(dir)
}

/// Cross-check every committed chunk against the generated-expected golden at
/// the same coordinates. This is the byte-identity that makes the FEATURES
/// checkpoint the canonical seed-42 decoration rather than a capture-regime
/// artifact: the FEATURES step's `blockStateWriteRadius(1)` lets border
/// decoration spill across chunk edges, so the committed chunks must match the
/// established FULL handoff at every sampled field (surface, bedrock,
/// below_feet, distinct set, status flags). The generated-expected fixture is a
/// committed repo file; an absent one (or a divergent chunk) is damage, never a
/// silent skip.
fn cross_check_generated_expected(world: &WorldManifest) -> Result<(), Error> {
    let dir = crate::crate_dir().join("fixtures/generated-expected");
    cross_check_generated_expected_in(world, &dir)
}

fn cross_check_generated_expected_in(world: &WorldManifest, dir: &Path) -> Result<(), Error> {
    // Do not use a merely parseable or partial generated-expected tree as an
    // oracle. Its exact files, SHA, Paper/normal-overworld provenance, seed,
    // forced-grid shape, and anti-superflat contract must pass first.
    let golden = load_generated_expected_for_features(dir)?;
    for (key, fp) in &world.chunks {
        let expected = golden.world.chunks.get(key).ok_or_else(|| {
            Error::Manifest(format!(
                "features chunk {key} is ABSENT from the generated-expected golden — the \
                 committed grids must stay aligned"
            ))
        })?;
        if fp != expected {
            return Err(Error::Manifest(format!(
                "features chunk {key} diverges from the generated-expected golden at the same \
                 coordinates — the FEATURES capture is not the canonical seed-42 decoration \
                 (a capture-regime artifact); the forced-grid context changed, or the golden \
                 drifted"
            )));
        }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeaturesManifest {
    format: u64,
    paper: String,
    seed: String,
    #[serde(rename = "level-type")]
    level_type: String,
    kind: String,
    note: String,
    captured: Vec<FeaturesCapturedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeaturesCapturedFile {
    path: String,
    sha256: String,
    bytes: usize,
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
        paper: PINNED_PAPER.to_owned(),
        seed: seed_str,
        level_type: "minecraft:normal".to_owned(),
        kind: KIND.to_owned(),
        note: "Seed-42 FEATURES oracle checkpoint (PR #175/#232): the per-chunk \
               surface/bedrock/below_feet fingerprint for the committed 2x2 grid \
               {(3,3),(4,3),(3,4),(4,4)} captured from the pinned Paper runtime by booting a \
               fresh normal-overworld world and force-generating generated-expected's {-6..6}x\
               {-6..6} forced grid to level 33 (ChunkLevel.byStatus(FULL)), serialized as \
               minecraft:full. FULL is the forced path's ceiling (a level-34 ticket is \
               INACCESSIBLE and never generates), and FEATURES is the last block-mutating \
               status, so a FULL serialization's block data IS the FEATURES-decoration output \
               this captures. The forced grid is generated-expected's regime because the \
               FEATURES step writes one chunk into each neighbor (blockStateWriteRadius(1)): \
               only the same forced-grid context makes the committed chunks byte-identical to \
               the canonical golden, which the verifier cross-checks chunk-for-chunk. Arrays \
               are 16x16 row-major z*16+x; surface is the highest non-air block, bedrock at \
               y=-60, below_feet at y=-61. Non-vacuity: chunk (4,4) must carry tree blocks in \
               its surface (the decoration evidence). The checkpoint also records a \
               feature-only observation layer (feature_observations): the positional \
               occurrences of magma_block (UnderwaterMagmaFeature, PR #644) and glow_lichen \
               (MultifaceGrowthFeature/glow_lichen, PR #645); verification requires a \
               submerged-column magma_block (the pinned ocean-floor signature) and at least one \
               glow_lichen. This is feature-leaf evidence only, not full generated-world parity. \
               Regenerate with `rivet-oracle regenerate --features` (twin-boot byte-identity proof)."
            .to_owned(),
        captured: vec![FeaturesCapturedFile {
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
    let (world_a, obs_a) = capture_world(PINNED_SEED)?;
    println!("[2/3] forced FEATURES capture B: fresh seed-42 Paper boot under the 1/1 pin...");
    let (world_b, obs_b) = capture_world(PINNED_SEED)?;

    if (&world_a, &obs_a) != (&world_b, &obs_b) {
        return Err(Error::Gate(
            "features twin-boot byte-identity check failed — the two independent Paper \
             captures produced DIFFERENT world manifests; refusing to commit a nondeterministic \
             checkpoint"
                .into(),
        ));
    }
    // Validate the (byte-identical) capture against the decoration contract
    // AND the canonical-golden cross-check BEFORE committing — two
    // equally-wrong captures must be refused.
    validate_world(&world_a)?;
    validate_observations(&world_a, &obs_a)?;
    validate_canonical_observations(&obs_a)?;
    cross_check_generated_expected(&world_a)?;

    println!("[3/3] byte-identical + contract-valid; writing the committed checkpoint...");
    fs::create_dir_all(dir)?;
    let golden = FeaturesGolden {
        seed: PINNED_SEED,
        world: world_a,
        feature_observations: obs_a,
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
    // Route through the same tri-state classification as the gate (PR #175):
    // wholly absent -> UNVERIFIED (exit 3), partial/corrupt -> hard failure, so
    // the CLI and the gate cannot disagree about an absent/partial tree.
    crate::verify_features_step(&dir)
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
    use crate::generated_expected;

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

    fn scratch_generated_expected(tag: &str) -> PathBuf {
        let source = fixtures_dir().join("generated-expected");
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-generated-expected-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        for filename in ["manifest.json", generated_expected::FIXTURE_BASENAME] {
            fs::copy(source.join(filename), scratch.join(filename)).unwrap();
        }
        scratch
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
    fn forced_grid_is_the_generated_expected_regime() {
        // The FEATURES step writes one chunk into each neighbor
        // (`blockStateWriteRadius(1)`), so a committed chunk's border trees spill
        // from its neighbors and the forced-grid context decides the captured
        // content. The forced grid must therefore be generated-expected's
        // {-6..=6} regime, not a smaller local pad — otherwise the committed
        // chunks diverge from the canonical golden at the spill columns.
        let forced = forced_coordinates();
        let ge_forced = crate::generated_expected::forced_coordinates();
        assert_eq!(
            forced, ge_forced,
            "the features forced grid must equal generated-expected's"
        );
        let committed = committed_coordinates();
        for (cx, cz) in &committed {
            for dx in -1..=1 {
                for dz in -1..=1 {
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
    fn committed_features_match_generated_expected_at_every_chunk() {
        // The committed FEATURES chunks must be byte-identical to the canonical
        // generated-expected golden at the same coordinates — a divergent capture
        // is a capture-regime artifact, not Paper ground truth.
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden = load(&dir).unwrap();
        cross_check_generated_expected(&golden.world).expect(
            "committed features chunks must match generated-expected at every sampled field",
        );
    }

    #[test]
    fn partial_generated_expected_dependency_is_a_hard_failure() {
        let source = fixtures_dir().join("generated-expected/manifest.json");
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-generated-expected-partial-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::copy(source, scratch.join("manifest.json")).unwrap();

        let result = load_generated_expected_for_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err(
            "a generated-expected manifest without its golden must hard-fail the FEATURES verifier",
        );
        assert!(
            matches!(&err, crate::Error::Manifest(_)),
            "partial generated-expected dependency must be Error::Manifest, got {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("generated-expected.json"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("partial/tampered"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn generated_expected_dependency_requires_exact_captured_binding() {
        let source = fixtures_dir().join("generated-expected/manifest.json");
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
        let captured = original["captured"][0].clone();
        let cases = [
            ("empty-captured", serde_json::json!([])),
            (
                "relabeled-captured",
                serde_json::json!([{
                    "path": "other.json",
                    "sha256": captured["sha256"].clone(),
                    "bytes": captured["bytes"].clone(),
                }]),
            ),
        ];
        for (tag, captured) in cases {
            let scratch = scratch_generated_expected(tag);
            let mut manifest = original.clone();
            manifest["captured"] = captured;
            fs::write(
                scratch.join("manifest.json"),
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();

            let result = load_generated_expected_for_features(&scratch);
            let _ = fs::remove_dir_all(&scratch);
            let err = result.expect_err("generated-expected binding tamper must hard-fail");
            assert!(
                matches!(&err, crate::Error::Manifest(_)),
                "tampered generated-expected binding must be Error::Manifest, got {err:?}"
            );
            assert!(
                err.to_string().contains("exactly one captured entry"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn generated_expected_dependency_requires_normal_level_type() {
        let scratch = scratch_generated_expected("flat-level-type");
        let manifest_path = scratch.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["level-type"] = serde_json::json!("minecraft:flat");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = load_generated_expected_for_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("flat generated-expected level type must hard-fail");
        assert!(
            matches!(&err, crate::Error::Manifest(_)),
            "flat generated-expected level type must be Error::Manifest, got {err:?}"
        );
        assert!(
            err.to_string().contains("not minecraft:normal"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn generated_expected_tamper_outside_features_grid_is_rejected() {
        let scratch = scratch_generated_expected("outside-features-grid");
        let golden_path = scratch.join(generated_expected::FIXTURE_BASENAME);
        let mut golden = generated_expected::load(&scratch).unwrap();
        let fingerprint = golden
            .world
            .chunks
            .get_mut("0,0")
            .expect("generated-expected includes an outside-grid chunk");
        fingerprint.surface[0] = if fingerprint.surface[0] == "minecraft:water" {
            "minecraft:stone".into()
        } else {
            "minecraft:water".into()
        };
        fs::write(&golden_path, serde_json::to_vec(&golden).unwrap()).unwrap();

        let features = load(&fixtures_dir().join("features")).unwrap();
        let result = cross_check_generated_expected_in(&features.world, &scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(&result, Err(crate::Error::HashMismatch { .. })),
            "outside-grid generated-expected tamper must fail its dependency hash gate, got {result:?}"
        );
    }

    #[test]
    fn generated_expected_dependency_rejects_unknown_manifest_fields() {
        let scratch = scratch_generated_expected("unknown-manifest-field");
        let manifest_path = scratch.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["unexpected"] = serde_json::json!(true);
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = load_generated_expected_for_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("unknown generated-expected manifest field must hard-fail");
        assert!(
            matches!(&err, crate::Error::Manifest(_)),
            "unknown generated-expected field must be Error::Manifest, got {err:?}"
        );
        assert!(
            err.to_string().contains("unknown field"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn generated_expected_dependency_rejects_unknown_golden_fields_after_rehash() {
        let scratch = scratch_generated_expected("unknown-golden-field");
        let golden_path = scratch.join(generated_expected::FIXTURE_BASENAME);
        let mut golden: serde_json::Value =
            serde_json::from_slice(&fs::read(&golden_path).unwrap()).unwrap();
        golden["unexpected_top_level"] = serde_json::json!(true);
        let bytes = serde_json::to_vec(&golden).unwrap();
        fs::write(&golden_path, &bytes).unwrap();

        let manifest_path = scratch.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["captured"][0]["sha256"] = serde_json::Value::String(crate::sha256_hex(&bytes));
        manifest["captured"][0]["bytes"] = serde_json::Value::from(bytes.len());
        fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

        let result = load_generated_expected_for_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err(
            "an unknown generated-expected golden field must fail even after rehash",
        );
        assert!(
            matches!(&err, crate::Error::Manifest(_)),
            "unknown generated-expected golden field must be Error::Manifest, got {err:?}"
        );
        assert!(
            err.to_string().contains("unknown field"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn generated_expected_dependency_requires_pinned_paper() {
        let scratch = scratch_generated_expected("wrong-paper");
        let manifest_path = scratch.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["paper"] = serde_json::json!("26.2-DEV-main@wrong");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = load_generated_expected_for_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("wrong generated-expected paper pin must hard-fail");
        assert!(
            matches!(&err, crate::Error::Manifest(_)),
            "wrong generated-expected paper must be Error::Manifest, got {err:?}"
        );
        assert!(err.to_string().contains("paper"), "unexpected error: {err}");
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

    /// A PARTIAL features tree — only manifest.json, no features.json — is a
    /// corrupt checkpoint, NOT an absent prerequisite: it must hard-fail
    /// (Error::Manifest, exit 1), never classify as UNVERIFIED (exit 3). The
    /// half-present tree's verifier cannot be trusted, so it reads as damage,
    /// not as "prerequisites unavailable" (D8 tri-state contract).
    #[test]
    fn partial_fixture_tree_is_a_hard_failure() {
        for missing in ["manifest.json", FIXTURE_BASENAME] {
            let scratch = std::env::temp_dir().join(format!(
                "rivet-oracle-features-partial-{missing}-{}",
                std::process::id()
            ));
            if scratch.exists() {
                fs::remove_dir_all(&scratch).unwrap();
            }
            fs::create_dir_all(&scratch).unwrap();
            let present = if missing == "manifest.json" {
                FIXTURE_BASENAME
            } else {
                "manifest.json"
            };
            fs::write(scratch.join(present), b"{}").unwrap();
            let result = crate::verify_features_step(&scratch);
            let _ = fs::remove_dir_all(&scratch);
            assert!(
                matches!(result, Err(crate::Error::Manifest(_))),
                "a partial features tree missing {missing} must hard-fail (Error::Manifest), \
                 got {result:?}"
            );
        }
    }

    /// The SHA-256 binding is load-bearing, not optional: `verify_fixtures`
    /// only checks the files the manifest DOES list, so a manifest whose
    /// `captured` list is empty (or binds a different file instead of
    /// features.json) must be rejected even though the golden bytes themselves
    /// are untouched — otherwise a modified golden could pass with no byte
    /// binding at all.
    #[test]
    fn manifest_without_captured_binding_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden = fs::read(dir.join(FIXTURE_BASENAME)).unwrap();
        // Case 1: an empty `captured` list — verify_fixtures accepts it
        // (nothing to check), so the require_single_captured gate must refuse.
        // Case 2: a manifest that binds ONLY a relabeled copy (other.json, a
        // byte-identical features.json under a different name) — verify_fixtures
        // passes its hash, yet features.json itself has no binding.
        let relabeled = vec![serde_json::json!({
            "path": "other.json",
            "sha256": crate::sha256_hex(&golden),
            "bytes": golden.len(),
        })];
        for captured in [vec![], relabeled] {
            let scratch = std::env::temp_dir().join(format!(
                "rivet-oracle-features-nocap-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).unwrap();
            fs::write(scratch.join(FIXTURE_BASENAME), &golden).unwrap();
            fs::write(scratch.join("other.json"), &golden).unwrap();
            let manifest = serde_json::json!({
                "format": 1,
                "paper": PINNED_PAPER,
                "seed": "42",
                "level-type": "minecraft:normal",
                "kind": KIND,
                "note": "test",
                "captured": captured,
            });
            fs::write(
                scratch.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            let result = verify_features(&scratch);
            let _ = fs::remove_dir_all(&scratch);
            let err = result
                .expect_err("a manifest without a features.json captured entry must be refused");
            assert!(
                err.to_string()
                    .contains("must bind exactly one captured entry"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn feature_manifest_requires_normal_level_type_and_exact_schema() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        for level_type in [None, Some("minecraft:flat")] {
            let scratch = std::env::temp_dir().join(format!(
                "rivet-oracle-features-level-type-{}-{}",
                if level_type.is_some() {
                    "flat"
                } else {
                    "missing"
                },
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).unwrap();
            fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME)).unwrap();
            let mut manifest: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap())
                    .unwrap();
            if let Some(level_type) = level_type {
                manifest["level-type"] = serde_json::Value::String(level_type.to_owned());
            } else {
                manifest.as_object_mut().unwrap().remove("level-type");
            }
            fs::write(
                scratch.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            let err = verify_features(&scratch)
                .expect_err("features provenance must reject missing/non-normal level-type");
            let _ = fs::remove_dir_all(&scratch);
            assert!(
                err.to_string().contains("provenance") || err.to_string().contains("schema"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    /// The committed features golden must still verify with the new
    /// observation layer present (redundant with `committed_features_verifies`,
    /// but pins the full golden including observations round-trips through
    /// verify).
    #[test]
    fn committed_features_observations_verify() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden = load(&dir).unwrap();
        assert!(
            !golden.feature_observations.is_empty(),
            "the committed golden must carry feature-palette observations"
        );
        verify_features(&dir).expect("committed features golden with observations verifies");
    }

    /// Removing every magma_block observation must fail verification — the
    /// UnderwaterMagmaFeature evidence is not optional.
    #[test]
    fn removing_magma_observations_is_detected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let mut stripped = BTreeMap::new();
        for (k, mut v) in golden.feature_observations.clone() {
            v.retain(|o| o.block != MAGMA_BLOCK);
            if !v.is_empty() {
                stripped.insert(k, v);
            }
        }
        // Guard: the committed golden actually has magma observations to strip.
        assert_ne!(
            stripped, golden.feature_observations,
            "the committed golden must carry magma observations for this tamper test to be real"
        );
        golden.feature_observations = stripped;
        let err = verify_scratch_golden(&golden, "no-magma")
            .expect_err("a golden with no magma observations must be refused");
        assert!(
            err.to_string().contains("magma_block"),
            "unexpected error: {err}"
        );
    }

    /// Removing every glow_lichen observation must fail verification — the
    /// MultifaceGrowthFeature (glow_lichen) evidence is not optional.
    #[test]
    fn removing_glow_lichen_observations_is_detected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let mut stripped = BTreeMap::new();
        for (k, mut v) in golden.feature_observations.clone() {
            v.retain(|o| o.block != GLOW_LICHEN_BLOCK);
            if !v.is_empty() {
                stripped.insert(k, v);
            }
        }
        assert_ne!(
            stripped, golden.feature_observations,
            "the committed golden must carry glow_lichen observations for this tamper test to be real"
        );
        golden.feature_observations = stripped;
        let err = verify_scratch_golden(&golden, "no-lichen")
            .expect_err("a golden with no glow_lichen observations must be refused");
        assert!(
            err.to_string().contains("glow_lichen"),
            "unexpected error: {err}"
        );
    }

    /// Relocating every observed magma_block to a non-submerged (dry) column must
    /// fail verification — the UnderwaterMagmaFeature modulation of the ocean
    /// floor, not a stray magma block in stone, is the pinned signature.
    #[test]
    fn magma_moved_out_of_water_is_detected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden = load(&dir).unwrap();
        // Guard: at least one submerged magma observation exists (it does),
        // so this negative control cannot pass vacuously.
        assert!(
            golden
                .feature_observations
                .iter()
                .any(|(key, observations)| {
                    let fp = golden.world.chunks.get(key).unwrap();
                    observations.iter().any(|o| {
                        o.block == MAGMA_BLOCK
                            && fp
                                .surface
                                .get(o.index)
                                .is_some_and(|surface| surface == "minecraft:water")
                    })
                }),
            "committed golden must carry submerged magma observations"
        );
        let mut tampered = golden.feature_observations.clone();
        // Move every magma observation into a column whose surface is NOT water
        // (dry). The disambiguation then refuses: no submerged magma remains.
        for v in tampered.values_mut() {
            for o in v.iter_mut() {
                if o.block == MAGMA_BLOCK {
                    // Find a dry column (surface != water) to relocate to.
                    let fp = golden.world.chunks.get("3,4").unwrap();
                    o.index = fp
                        .surface
                        .iter()
                        .position(|s| s != "minecraft:water")
                        .expect("3,4 has dry columns");
                }
            }
        }
        assert_ne!(
            tampered, golden.feature_observations,
            "the relocation must actually change the magma observations"
        );
        let mut g2 = golden.clone();
        g2.feature_observations = tampered;
        let err = verify_scratch_golden(&g2, "magma-dry")
            .expect_err("a golden with no submerged magma must be refused");
        assert!(
            err.to_string().contains("submerged") || err.to_string().contains("duplicates"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_feature_golden_key_is_rejected_after_rehash() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FIXTURE_BASENAME)).unwrap()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        let mut tampered = golden;
        tampered["unknown_top_level"] = serde_json::Value::Bool(true);
        let err = verify_scratch_values(tampered, manifest, "unknown-golden")
            .expect_err("unknown feature golden keys must fail even after rehash");
        assert!(
            err.to_string().contains("unknown field"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_feature_observation_key_is_rejected_after_rehash() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FIXTURE_BASENAME)).unwrap()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        golden["feature_observations"]["3,3"][0]["unknown_observation_field"] =
            serde_json::Value::Bool(true);
        let err = verify_scratch_values(golden, manifest, "unknown-observation")
            .expect_err("unknown observation keys must fail even after rehash");
        assert!(
            err.to_string().contains("unknown field"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_feature_chunk_key_with_canonical_last_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FIXTURE_BASENAME)).unwrap()).unwrap();
        let mut altered = golden["chunks"]["3,3"].clone();
        let original = altered["surface"][0].clone();
        altered["surface"][0] = if original == serde_json::json!("minecraft:air") {
            serde_json::json!("minecraft:stone")
        } else {
            serde_json::json!("minecraft:air")
        };
        let raw = duplicate_nested_object_entry(&golden, "chunks", "3,3", &altered);
        let err = verify_scratch_raw(raw, "duplicate-chunk-key")
            .expect_err("duplicate chunk keys must fail even with canonical value last");
        assert!(
            err.to_string().contains("duplicate JSON object key")
                && err.to_string().contains("3,3"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_feature_observation_key_with_canonical_last_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FIXTURE_BASENAME)).unwrap()).unwrap();
        let mut altered = golden["feature_observations"]["3,3"].clone();
        altered.as_array_mut().unwrap()[0]["y"] = serde_json::json!(999);
        let raw = duplicate_nested_object_entry(
            &golden,
            "feature_observations",
            "3,3",
            &altered,
        );
        let err = verify_scratch_raw(raw, "duplicate-observation-key")
            .expect_err("duplicate observation keys must fail even with canonical value last");
        assert!(
            err.to_string().contains("duplicate JSON object key")
                && err.to_string().contains("3,3"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn feature_manifest_unknown_or_missing_provenance_key_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let golden: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FIXTURE_BASENAME)).unwrap()).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        manifest["unknown_provenance"] = serde_json::Value::Bool(true);
        let err = verify_scratch_values(golden.clone(), manifest, "unknown-manifest")
            .expect_err("unknown manifest provenance must fail");
        assert!(
            err.to_string().contains("unknown field"),
            "unexpected error: {err}"
        );

        let mut missing: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        missing.as_object_mut().unwrap().remove("level-type");
        let err = verify_scratch_values(golden, missing, "missing-level-type")
            .expect_err("missing level-type provenance must fail");
        assert!(
            err.to_string().contains("missing field"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_feature_observation_is_rejected_after_rehash() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let duplicate = golden.feature_observations["3,3"][0].clone();
        golden
            .feature_observations
            .get_mut("3,3")
            .unwrap()
            .push(duplicate);
        let err = verify_scratch_golden(&golden, "duplicate-observation")
            .expect_err("duplicate payload positions must fail");
        assert!(
            err.to_string().contains("duplicates"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn generated_expected_seed_tamper_with_stale_manifest_is_rejected() {
        let features_dir = fixtures_dir().join("features");
        let generated_dir = fixtures_dir().join("generated-expected");
        require_fixture(&features_dir);
        assert!(generated_dir.join("manifest.json").is_file());
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-stale-generated-expected-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::copy(
            generated_dir.join("manifest.json"),
            scratch.join("manifest.json"),
        )
        .unwrap();
        let mut generated: serde_json::Value = serde_json::from_slice(
            &fs::read(generated_dir.join(generated_expected::FIXTURE_BASENAME)).unwrap(),
        )
        .unwrap();
        generated["seed"] = serde_json::Value::from(999);
        fs::write(
            scratch.join(generated_expected::FIXTURE_BASENAME),
            serde_json::to_vec(&generated).unwrap(),
        )
        .unwrap();
        let features = load(&features_dir).unwrap();
        let result = cross_check_generated_expected_in(&features.world, &scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("seed 42 -> 999 with stale generated manifest must fail");
        assert!(
            err.to_string().contains("hash mismatch") || err.to_string().contains("size mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn relocated_or_recomputed_feature_position_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let observation = golden
            .feature_observations
            .get_mut("3,3")
            .and_then(|values| values.first_mut())
            .expect("canonical glow_lichen observation");
        observation.index = 20;
        observation.y = 123;
        let err = verify_scratch_golden(&golden, "relocated-position")
            .expect_err("a relocated observation must not pass by surface/count checks");
        assert!(
            err.to_string().contains("bounds") || err.to_string().contains("canonical"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_feature_position_is_rejected() {
        let dir = fixtures_dir().join("features");
        require_fixture(&dir);
        let mut golden = load(&dir).unwrap();
        let duplicate = golden
            .feature_observations
            .get("3,3")
            .and_then(|values| values.first())
            .cloned()
            .expect("canonical feature observation");
        golden
            .feature_observations
            .get_mut("3,3")
            .unwrap()
            .push(duplicate);
        let err = verify_scratch_golden(&golden, "duplicate-position")
            .expect_err("duplicate feature positions must be refused");
        assert!(
            err.to_string().contains("duplicates"),
            "unexpected error: {err}"
        );
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

    /// Write a full `FeaturesGolden` into a scratch dir under a valid
    /// hash-gated manifest and run the full verify against it. Returns the
    /// verify result (callers assert success or a specific rejection).
    fn verify_scratch_golden(golden: &FeaturesGolden, tag: &str) -> Result<(), crate::Error> {
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string(golden).unwrap(),
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
        result
    }

    /// Verify arbitrary JSON after rebuilding its captured SHA. Negative tests
    /// use this so failures are attributable to strict schema/provenance gates,
    /// not the manifest hash.
    fn verify_scratch_values(
        golden: serde_json::Value,
        mut manifest: serde_json::Value,
        tag: &str,
    ) -> Result<(), crate::Error> {
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-values-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        let bytes = serde_json::to_vec(&golden).unwrap();
        fs::write(scratch.join(FIXTURE_BASENAME), &bytes).unwrap();
        manifest["captured"][0]["sha256"] = serde_json::Value::String(crate::sha256_hex(&bytes));
        manifest["captured"][0]["bytes"] = serde_json::Value::from(bytes.len());
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let result = verify_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        result
    }

    /// Build a fixture JSON object with one nested object key repeated. The
    /// altered value is first and the canonical value is last, matching the
    /// tamper that ordinary BTreeMap deserialization would silently accept.
    fn duplicate_nested_object_entry(
        golden: &serde_json::Value,
        outer_key: &str,
        inner_key: &str,
        first_value: &serde_json::Value,
    ) -> String {
        let root = golden.as_object().unwrap();
        let mut root_entries = Vec::new();
        for (key, value) in root {
            let key_json = serde_json::to_string(key).unwrap();
            if key != outer_key {
                root_entries.push(format!(
                    "{key_json}:{}",
                    serde_json::to_string(value).unwrap()
                ));
                continue;
            }
            let nested = value.as_object().unwrap();
            let mut nested_entries = Vec::new();
            for (nested_key, canonical) in nested {
                let nested_key_json = serde_json::to_string(nested_key).unwrap();
                if nested_key == inner_key {
                    nested_entries.push(format!(
                        "{nested_key_json}:{}",
                        serde_json::to_string(first_value).unwrap()
                    ));
                }
                nested_entries.push(format!(
                    "{nested_key_json}:{}",
                    serde_json::to_string(canonical).unwrap()
                ));
            }
            root_entries.push(format!(
                "{key_json}:{{{}}}",
                nested_entries.join(",")
            ));
        }
        format!("{{{}}}", root_entries.join(","))
    }

    /// Verify raw fixture JSON after rebuilding its captured SHA. Unlike
    /// `verify_scratch_values`, this preserves duplicate object keys so the
    /// duplicate-key detector itself is exercised rather than serde's map
    /// overwrite behavior.
    fn verify_scratch_raw(raw: String, tag: &str) -> Result<(), crate::Error> {
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-features-raw-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        let bytes = raw.as_bytes();
        fs::write(scratch.join(FIXTURE_BASENAME), bytes).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(fixtures_dir().join("features/manifest.json")).unwrap(),
        )
        .unwrap();
        manifest["captured"][0]["sha256"] = serde_json::Value::String(crate::sha256_hex(bytes));
        manifest["captured"][0]["bytes"] = serde_json::Value::from(bytes.len());
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let result = verify_features(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        result
    }

    /// Write `world` into a scratch dir and run the full verify against it,
    /// expecting rejection (the shared schema-degradation tests). No feature
    /// observations are attached, so a world that passes `validate_world` but
    /// carries no feature-palette evidence is rejected by the observation
    /// contract rather than handed off.
    fn verify_scratch_world(world: &WorldManifest, tag: &str) -> crate::Error {
        let golden = FeaturesGolden {
            seed: PINNED_SEED,
            world: world.clone(),
            feature_observations: BTreeMap::new(),
        };
        verify_scratch_golden(&golden, tag)
            .expect_err("the checkpoint must be rejected by the decoration contract")
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
