//! The seed-42 LIGHT oracle checkpoint (`light`).
//!
//! The next oracle checkpoint BEFORE any Rivet LIGHT-status wiring: a focused
//! Paper 26.2 capture of one deterministic seed-42 overworld chunk grid forced
//! through real Starlight lighting, plus an exact Rivet-side verifier. It
//! reuses the generated-expected / features two-boot capture machinery
//! (`inject_forced_tickets` at level 33, `clear_region_files`,
//! `check_capture_pin`) but records stage-specific truth for the LIGHT step
//! itself: the per-section sky light nibbles, the derived sky-emptiness map,
//! and `light_correct`, so a future Rivet LIGHT-status port (the merged
//! `SkyStarLightEngine` / `SkyLightProvider` path) is checked against Paper
//! ground truth rather than against nothing.
//!
//! Status semantics: the Moonrise forced-ticket path can only serialize FULL.
//! A level-35 ticket (`ChunkLevel.byStatus(LIGHT)`) is `INACCESSIBLE` to
//! `fullStatus`, so `NewChunkHolder.processTicketLevelUpdate` never schedules
//! generation for it — a sub-FULL serialized capture is not reachable through
//! forced tickets at all. So the checkpoint captures at level 33
//! (`FORCED_TICKET_LEVEL`), which serializes as `minecraft:full`. That is
//! faithful to the LIGHT step because FULL serialization carries the
//! Starlight-computed light arrays: `ChunkLightTask.LightTask.getAsBoolean`
//! runs the fresh-chunk branch (`setLightCorrect(false); lightEngine
//! .lightChunk(fromChunk, emptySections); setLightCorrect(true)`), which is
//! exactly `StarLightInterface.lightChunk` → `SkyStarLightEngine.light` →
//! `lightChunk(lightAccess, chunk, true)`, and the resulting nibbles are what
//! `SaveUtil` persists. The verifier pins `EXPECTED_STATUS = minecraft:full`
//! and `light_correct = true` (status FULL is `isOrAfter(LIGHT)`, and the
//! save carries `isLightOn` + `starlight.light_version` 10).
//!
//! Coordinate set: a self-contained forced 5×5 grid at {18..22}² — far from
//! seed-42's spawn-area chunks (chunk (-2,0)), so no spawn-chunk influence —
//! with a committed interior 3×3 {19..21}². Every committed chunk's full
//! 1-radius block context and 2-radius emptiness context lies inside the
//! forced grid, so Paper's light for the interior is computed over exactly the
//! set this checkpoint commits, and Rivet reproduces it over the same set
//! (the engine's relaxed cache setup tolerates the missing 2-radius chunks on
//! the grid corners — Paper's own capture had the same holes).
//!
//! Fixture truth per committed chunk (decoded from the serialized FULL chunk
//! NBT):
//!   - `sky_nibbles`: the per-light-section `to_vanilla_nibble` byte views
//!     (the packet-visible `DataLayer` bytes; `None` for a null section),
//!     keyed by light-section y (minLightSection -5 ..= maxLightSection 20).
//!   - `sky_emptiness`: the per-world-section emptiness map, derived exactly
//!     like Paper's `StarLightEngine.getEmptySectionsForChunk` (absent or
//!     `hasOnlyAir` section → empty). The map is not persisted in NBT — this
//!     derivation is the engine's own fallback, so it is captured as truth.
//!   - `light_correct`, `status` (`minecraft:full`), `stored_pos`.
//!
//! The FULL 5×5's raw block NBT is committed alongside (`chunks/<x>.<z>.nbt`)
//! so the rivet-server engine differential can rebuild the exact context
//! Paper lit in: every chunk's sections are reconstructed into the server's
//! `StateId` space, every chunk gets its persisted light installed
//! (`reconstruct_lights(...).install(...)` → light_correct), and the committed
//! 3×3 is re-lit by the server's test-only parity helper — the per-neighbour
//! no-edge-checks path from Paper's `relightChunks`, whose neighbour-light pull
//! (`propagateNeighbourLevels`) reproduces the fixture's east-neighbour water
//! dampening at the boundary columns. The published sky nibbles and emptiness
//! map must then match the fixture truth byte-exact. The helper is absent from
//! production, so this oracle does not imply a served dynamic-relighting API.
//!
//! Honesty rules (mirror features/generated-expected, D8):
//!
//!   * capture returns `Error::Unverified` (exit 3) when the pinned Paper
//!     runtime is absent — it never writes an empty or fabricated manifest, and
//!     it removes a stale `--to` file first.
//!   * capture is a two-boot sequence (create boot, then a level-33 forced
//!     capture boot) that discards boot1's partial spawn-area chunks so the
//!     forced grid is generated from a blank chunk state (byte-deterministic).
//!   * verify returns `Error::Unverified` (exit 3) when the committed fixture
//!     tree is absent — never a silent green.
//!   * verify rejects a fixture that is not exactly the forced 5×5 grid with
//!     real light evidence: a chunk stuck below full, a `light_correct=false`
//!     chunk, a chunk with no non-null sky nibble, or a uniform-emptiness echo
//!     is refused loudly.
//!   * capture enforces provenance: after the capture boot, the materialized
//!     server jar's `Git-Commit` must match the pinned `0a99345` before any
//!     content is written.
//!   * the regenerate path validates a capture against the light contract
//!     BEFORE committing it, in addition to the twin-boot byte-identity proof
//!     (which covers the goldens AND all 25 raw chunk NBTs).
//!   * the tamper negative control proves a flipped byte in the golden fails
//!     verification (the manifest SHA-256 gate is not vacuous).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_registry::core::ChunkPos;
use rivet_util::DataOutputStream;
use rivet_world::chunk::storage::region_file_storage::{
    RegionFileStorage, get_region_file_coordinates,
};
use rivet_world::chunk::storage::region_storage_info::RegionStorageInfo;
use rivet_world::chunk::storage::section_reconstruction::{
    current_version_container_factory, reconstruct_sections,
};
use rivet_world::chunk::storage::serializable_chunk_data::{
    SerializableChunkData, parse_light_correct, parse_section_lights, reconstruct_lights,
};
use rivet_world::level::height_accessor::{self, LevelHeightAccessor, SimpleLevelHeightAccessor};
use rivet_world::level::level::overworld;

use crate::generated_expected::{
    capture_run_dir, check_capture_pin, clear_region_files, seed_properties,
};
use crate::loaded_world::section_predicates;
use crate::{CapturedFile, Error};

/// The fixture kind name (matches the manifest `kind` and the regenerate flag).
pub const KIND: &str = "light";
/// The committed golden filename.
pub const FIXTURE_BASENAME: &str = "light.json";
/// The pinned seed the checkpoint captures under (mirrors generated-expected).
pub const PINNED_SEED: i64 = 42;
/// The pinned Paper provenance the checkpoint is captured against.
pub const PINNED_PAPER: &str = "26.2-DEV-main@0a99345";
/// The serialized status a level-33 forced chunk reaches (see the module doc:
/// the forced-ticket path can only serialize FULL, and FULL serialization
/// carries the Starlight-computed light arrays — the LIGHT-stage output). The
/// verifier pins exactly this.
pub const EXPECTED_STATUS: &str = "minecraft:full";
/// The forced grid: a self-contained 5×5 {18..22}², far from seed-42's
/// spawn-area chunks at chunk (-2,0). Every committed chunk's full 1-radius
/// block context + 2-radius emptiness context is inside this grid; the 2-radius
/// holes on the grid corners are shared with Paper's own capture (relaxed).
const FORCE_MIN: i32 = 18;
const FORCE_MAX: i32 = 22;
/// The committed interior: the center 3×3 {19..21}².
const GRID_MIN: i32 = 19;
const GRID_MAX: i32 = 21;
/// The overworld's vertical extent (`minY=-64`, `height=384`).
const OVERWORLD_MIN_Y: i32 = -64;
const OVERWORLD_HEIGHT: i32 = 384;

/// The committed golden: the seed the grid was captured under plus the
/// per-chunk light truth (the generated-expected seed-provenance shape, PR
/// #595 — `seed` is both structurally bound and inside the SHA-256 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightGolden {
    /// The seed the golden content was generated under (the `--to` capture
    /// writes the actual seed; committed verification requires `PINNED_SEED`).
    pub seed: i64,
    /// A stable format marker.
    pub format: u32,
    /// Per-committed-chunk light truth keyed by `"<x>,<z>"`.
    pub chunks: BTreeMap<String, ChunkLightTruth>,
}

/// One committed chunk's LIGHT-stage truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkLightTruth {
    /// The chunk's internal `xPos`/`zPos`.
    pub stored_pos: [i32; 2],
    /// The serialized status — exactly `minecraft:full` for a level-33 forced
    /// chunk (see the module doc).
    pub status: String,
    /// Paper's `lightCorrect` predicate: status ≥ LIGHT + `isLightOn` +
    /// `starlight.light_version` == 10. True for every committed chunk.
    pub light_correct: bool,
    /// `minLightSection` — `getMinSectionY() - 1` (world section -5).
    pub min_light_section: i32,
    /// `maxLightSection` — `getMaxSectionY() + 1` (world section 20).
    pub max_light_section: i32,
    /// Per-light-section-y sky nibble bytes (`to_vanilla_nibble` views; `None`
    /// for a null section). Keyed by the light-section y so the differential
    /// compares directly against the engine's published `sky_nibbles`.
    pub sky_nibbles: BTreeMap<i32, Option<Vec<u8>>>,
    /// The per-world-section sky emptiness map, derived exactly like Paper's
    /// `getEmptySectionsForChunk` (absent or `hasOnlyAir` section → empty).
    pub sky_emptiness: Vec<bool>,
}

/// The committed grid coordinates, deterministically ordered (x-major).
pub(crate) fn committed_coordinates() -> Vec<(i32, i32)> {
    (GRID_MIN..=GRID_MAX)
        .flat_map(|x| (GRID_MIN..=GRID_MAX).map(move |z| (x, z)))
        .collect()
}

/// The forced grid coordinates, deterministically ordered (a superset of the
/// committed grid).
pub(crate) fn forced_coordinates() -> Vec<(i32, i32)> {
    (FORCE_MIN..=FORCE_MAX)
        .flat_map(|x| (FORCE_MIN..=FORCE_MAX).map(move |z| (x, z)))
        .collect()
}

/// A full capture: the committed light truth plus the raw serialized chunk NBT
/// of every forced chunk (the differential's input).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureOutcome {
    golden: LightGolden,
    /// `"<x>,<z>"` → serialized raw chunk NBT bytes, all 25 forced chunks.
    chunk_nbts: BTreeMap<String, Vec<u8>>,
}

/// Boot the pinned Paper on a fresh seed world, force the 5×5 grid through real
/// Starlight lighting (level-33 tickets → `minecraft:full`, whose serialization
/// carries the computed light arrays), and extract the deterministic light
/// truth for the committed 3×3 plus the raw NBT of all 25 forced chunks. A
/// missing Paper runtime is `Error::Unverified` — a missing prerequisite, never
/// a fabricated green.
fn capture_world(seed: i64) -> Result<CaptureOutcome, Error> {
    let jar = crate::ensure_jar().map_err(|e| {
        Error::Unverified(format!(
            "light capture needs the pinned Paper runtime: {e} \
             (boot the M0 fixture server once per tools/rivet-oracle/README.md, or set \
             RIVET_ORACLE_JAR); UNVERIFIED, never a fabricated manifest"
        ))
    })?;
    let props = seed_properties(seed, "light")?;
    let run_dir = capture_run_dir("light");
    let forced = forced_coordinates();

    // boot1 (create): a plain spawn boot creates the seed world (the world must
    // exist before the forced tickets can load). The world persists in
    // run_dir/world between the two boots (the generated-expected pattern).
    crate::prepare_run_dir(&run_dir, &props)?;
    let create_log = run_dir.with_file_name("boot-light-create.log");
    println!("      [boot1] creating the seed-{seed} normal-overworld world...");
    crate::boot_and_shutdown(&run_dir, &create_log, &jar)?;

    // Discard boot1's partial spawn-area chunks so boot2 generates the forced
    // grid from a blank chunk state (byte-deterministic), exactly like the
    // generated-expected / features handoffs.
    clear_region_files(&run_dir.join("world"), crate::OVERWORLD_DIM)?;

    // Inject level-33 forced tickets for the 5×5 grid, then boot2 loads those
    // persistent chunks and finishes each through Starlight lighting to FULL
    // (whose serialization carries the computed light arrays). Level 33 is
    // `ChunkLevel.byStatus(FULL)`; a higher level is `INACCESSIBLE` and the
    // Moonrise scheduler never generates it, so FULL is the forced path's
    // ceiling (and the LIGHT output rides on it).
    crate::inject_forced_tickets(
        &run_dir.join("world"),
        &forced,
        crate::OVERWORLD_DIM,
        crate::FORCED_TICKET_LEVEL,
    )?;
    let capture_log = run_dir.with_file_name("boot-light.log");
    println!("      [boot2] capturing the forced Starlight-lit chunks...");
    crate::boot_and_shutdown(&run_dir, &capture_log, &jar)?;
    crate::verify_forced_load(&capture_log, forced.len(), crate::OVERWORLD_DIM)?;

    // Provenance: the content about to be stamped `PINNED_PAPER` must have
    // actually been generated by that commit's server jar.
    check_capture_pin(&run_dir)?;

    extract_light(&run_dir.join("world"), seed)
}

/// Extract the light checkpoint from the captured world: decode the committed
/// 3×3's light truth from the serialized FULL chunk NBT, and keep the raw NBT
/// of all 25 forced chunks for the differential. Every read goes through the
/// read-only region storage; a forced chunk that is missing or below full is a
/// hard capture failure, never content to hand off.
fn extract_light(world_dir: &Path, seed: i64) -> Result<CaptureOutcome, Error> {
    let region_dir = crate::loaded_world::overworld_region_dir(world_dir);
    let mut region_files: Vec<(PathBuf, ChunkPos)> = fs::read_dir(&region_dir)
        .map_err(|e| Error::Gate(format!("reading region dir {}: {e}", region_dir.display())))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().map(|e| e == "mca").unwrap_or(false) {
                get_region_file_coordinates(&path).map(|coords| (path, coords))
            } else {
                None
            }
        })
        .collect();
    region_files.sort_by(|a, b| a.0.cmp(&b.0));
    if region_files.is_empty() {
        return Err(Error::Unverified(format!(
            "captured world {} has no overworld region files under {}",
            world_dir.display(),
            region_dir.display()
        )));
    }

    let storage_info = RegionStorageInfo::new(
        "seed42-light-capture".to_owned(),
        overworld(),
        "region".to_owned(),
        true,
    );
    let mut storage = RegionFileStorage::new_read_only(storage_info, region_dir.clone());
    let height = height_accessor::create(OVERWORLD_MIN_Y, OVERWORLD_HEIGHT);
    let factory = current_version_container_factory();
    let predicates = section_predicates();
    let min_section = height.get_min_section_y();
    let max_section = height.get_max_section_y();

    let forced = forced_coordinates();
    let committed = committed_coordinates();
    let committed_set: BTreeSet<(i32, i32)> = committed.iter().copied().collect();

    let mut chunk_nbts = BTreeMap::new();
    let mut chunks = BTreeMap::new();
    for (cx, cz) in &forced {
        let pos = ChunkPos::new(*cx, *cz);
        let key = format!("{cx},{cz}");
        let tag = storage
            .read(&pos)
            .map_err(|e| Error::Gate(format!("reading forced light chunk {pos}: {e}")))?
            .ok_or_else(|| {
                Error::Gate(format!(
                    "forced light chunk {key} is ABSENT from the captured world — the forced \
                 generation did not produce it; refusing to hand off a partial capture"
                ))
            })?;

        // The raw serialized bytes are the differential's exact input (the
        // rivet-server test reads them back with `nbt_io::read_unlimited`).
        // `LastUpdate` is stripped so the two twin boots serialize identically.
        let bytes = serialize_tag(&strip_volatile_fields(&tag));
        let data = SerializableChunkData::parse(height, &tag)
            .map_err(|e| Error::Gate(format!("parsing forced light chunk {pos}: {e}")))?
            .ok_or_else(|| {
                Error::Gate(format!(
                    "forced light chunk {key} has no Status — Paper drops it before DataVersion"
                ))
            })?;
        if data.status().serialization_name() != EXPECTED_STATUS {
            return Err(Error::Gate(format!(
                "forced light chunk {key} is {} (expected {EXPECTED_STATUS}) — the forced \
                 generation did not finish a full chunk; refusing to hand off a partial capture",
                data.status().serialization_name()
            )));
        }
        chunk_nbts.insert(key.clone(), bytes);

        if committed_set.contains(&(*cx, *cz)) {
            chunks.insert(key.clone(), light_truth_for(&tag, &data, &height, *cx, *cz));
        }
    }

    // Reconstruct the committed sections for the derived emptiness map — the
    // exact `getEmptySectionsForChunk` fallback (absent or hasOnlyAir → empty).
    let mut chunks_with_emptiness = BTreeMap::new();
    for (key, mut truth) in chunks {
        let cx = truth.stored_pos[0];
        let cz = truth.stored_pos[1];
        let pos = ChunkPos::new(cx, cz);
        let tag = storage
            .read(&pos)
            .map_err(|e| Error::Gate(format!("re-reading committed light chunk {pos}: {e}")))?
            .ok_or_else(|| {
                Error::Gate(format!("committed light chunk {key} vanished on re-read"))
            })?;
        let data = SerializableChunkData::parse(height, &tag)
            .map_err(|e| Error::Gate(format!("re-parsing committed light chunk {pos}: {e}")))?
            .ok_or_else(|| Error::Gate(format!("committed light chunk {key} lost Status")))?;
        let sections = reconstruct_sections(
            data.section_tags(),
            min_section,
            max_section,
            &factory,
            predicates,
        )
        .map_err(|e| Error::Gate(format!("reconstructing sections of {key}: {e}")))?;
        truth.sky_emptiness = sections
            .iter()
            .map(|s| s.as_ref().is_none_or(|s| s.has_only_air()))
            .collect();
        chunks_with_emptiness.insert(key, truth);
    }

    Ok(CaptureOutcome {
        golden: LightGolden {
            seed,
            format: 1,
            chunks: chunks_with_emptiness,
        },
        chunk_nbts,
    })
}

/// Decode one committed chunk's LIGHT truth from its serialized NBT: the
/// persisted Starlight sky nibbles (`reconstruct_lights` rebuilds each section's
/// nibble from its `starlight.skylight_state` INT) and Paper's `lightCorrect`
/// predicate (status FULL is `isOrAfter(LIGHT)`).
fn light_truth_for(
    tag: &CompoundTag,
    data: &SerializableChunkData,
    height: &SimpleLevelHeightAccessor,
    cx: i32,
    cz: i32,
) -> ChunkLightTruth {
    let light_correct = parse_light_correct(tag, true);
    let lights = parse_section_lights(tag);
    let reconstructed = reconstruct_lights(*height, &lights, light_correct, true);
    let min_light = height.get_min_section_y() - 1;
    let max_light = height.get_max_section_y() + 1;
    let mut sky_nibbles = BTreeMap::new();
    for (index, cy) in (min_light..=max_light).enumerate() {
        sky_nibbles.insert(
            cy,
            reconstructed.sky_nibbles[index]
                .to_vanilla_nibble()
                .map(|d| d.get_data()),
        );
    }
    ChunkLightTruth {
        stored_pos: [cx, cz],
        status: data.status().serialization_name().to_owned(),
        light_correct,
        min_light_section: min_light,
        max_light_section: max_light,
        sky_nibbles,
        sky_emptiness: Vec::new(), // filled by extract_light after section reconstruction
    }
}

/// Serialize a chunk `CompoundTag` to the raw NBT bytes the differential reads
/// back (`nbt_io::read_unlimited`).
fn serialize_tag(tag: &CompoundTag) -> Vec<u8> {
    let mut bytes = Vec::new();
    nbt_io::write(tag, &mut DataOutputStream::new(&mut bytes))
        .expect("serializing a chunk tag cannot fail on a Vec");
    bytes
}

/// Strip the chunk-level fields that are volatile between Paper boots from a
/// clone of a chunk tag, returning the deterministic serialization input.
///
/// Paper stamps `LastUpdate` (a game-tick counter) onto every serialized FULL
/// chunk; it differs between the two twin boots, so committing it would make the
/// checkpoint nondeterministic (regeneration would produce a different fixture).
/// The differential only consumes the light truth + block context, neither of
/// which reads `LastUpdate`. The twin-boot byte-identity gate in `run_probe`
/// remains the safety net: if any OTHER volatile field ever appears, the two
/// captures will differ and the gate refuses to commit — the strip list is never
/// silently extended.
fn strip_volatile_fields(tag: &CompoundTag) -> CompoundTag {
    let mut deterministic = tag.clone();
    deterministic.remove("LastUpdate");
    deterministic
}

/// Capture mode for the `--to <out>` invocation: the two-boot create +
/// forced-capture sequence, written as compact JSON to `to` (the committed
/// chunk NBTs stay with the fixture tree; `--to` is a single-file diagnostics
/// capture). A stale `to` file is removed first so a failed capture never
/// leaves a previous success behind.
pub fn capture_to(seed: i64, to: &Path) -> Result<(), Error> {
    if to.exists() {
        fs::remove_file(to)?;
    }
    let outcome = capture_world(seed)?;
    // Refuse a capture that does not meet the light contract — a bad capture
    // must never be handed off as the checkpoint's ground truth.
    validate_golden(&outcome.golden)?;
    let json = serde_json::to_string(&outcome.golden)
        .map_err(|e| Error::Gate(format!("serializing light golden: {e}")))?;
    if let Some(parent) = to.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(to, json.as_bytes())?;
    println!(
        "captured seed-{seed} light golden ({} Starlight-lit chunks) -> {}",
        outcome.golden.chunks.len(),
        to.display()
    );
    Ok(())
}

/// Parse + structurally validate the committed golden (`LightGolden` shape).
pub fn load(dir: &Path) -> Result<LightGolden, Error> {
    let path = dir.join(FIXTURE_BASENAME);
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let golden: LightGolden = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
    if golden.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported light format {} (expected 1)",
            golden.format
        )));
    }
    Ok(golden)
}

/// Assert the committed golden's provenance, manifest hashes, forced-grid
/// shape, per-chunk status/light-correct contract, chunk-NBT binding, and
/// LIGHT-stage non-vacuity.
pub fn verify_light(dir: &Path) -> Result<(), Error> {
    let manifest = crate::verify_fixtures(dir)?;
    // 0. The SHA-256 binding is load-bearing, not optional: verify_fixtures only
    //    checks the files the manifest DOES list, so a manifest with no captured
    //    entry for light.json would let a modified-but-still-valid golden pass
    //    with zero byte binding. Require exactly one non-empty entry.
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
            "light fixture not pinned to Paper 0a99345: {:?}",
            manifest.paper
        )));
    }
    // 2. Seed provenance, bound two ways (PR #595): the manifest's seed string
    //    AND the golden's self-described seed must both be the pinned seed.
    if manifest.seed.as_deref() != Some(&PINNED_SEED.to_string()) {
        return Err(Error::Manifest(format!(
            "light fixture seed {:?} != pinned seed {PINNED_SEED}",
            manifest.seed
        )));
    }
    let golden = load(dir)?;
    if golden.seed != PINNED_SEED {
        return Err(Error::Manifest(format!(
            "light golden self-describes seed {} != pinned seed {PINNED_SEED} — the captured \
             content was generated under a different seed; refusing a wrong-seed handoff",
            golden.seed
        )));
    }
    // 3. The differential's inputs — the 25 raw forced chunk NBTs — must be
    //    byte-bound in the manifest too (the rivet-server test rebuilds the
    //    exact context Paper lit in from them).
    for (cx, cz) in forced_coordinates() {
        let path = chunk_fixture_path(cx, cz);
        if !manifest.captured.iter().any(|c| c.path == path) {
            return Err(Error::Manifest(format!(
                "manifest does not bind the forced chunk NBT {path} — the differential \
                 cannot rebuild the context Paper lit in without it"
            )));
        }
    }

    validate_golden(&golden)?;
    Ok(())
}

/// The committed chunk-NBT fixture path for one forced coordinate.
pub(crate) fn chunk_fixture_path(cx: i32, cz: i32) -> String {
    format!("chunks/{cx}.{cz}.nbt")
}

/// Assert a captured golden meets the committed-grid shape, the per-chunk
/// status/light-correct contract, and the LIGHT-stage non-vacuity guarantees.
/// Shared by the committed-fixture verify and the `--to` capture path.
fn validate_golden(golden: &LightGolden) -> Result<(), Error> {
    let grid = committed_coordinates();
    let expected_keys: BTreeSet<String> = grid.iter().map(|(x, z)| format!("{x},{z}")).collect();
    let actual_keys: BTreeSet<String> = golden.chunks.keys().cloned().collect();
    if actual_keys != expected_keys {
        return Err(Error::Manifest(format!(
            "light chunks {} != committed grid {} — a capture that adds or drops committed \
             chunks is drift, not the checkpoint",
            format_set(&actual_keys),
            format_set(&expected_keys)
        )));
    }

    let forced = forced_coordinates();
    // Light sections span minSectionY-1 ..= maxSectionY+1. maxSectionY is
    // `(minY + height - 1) >> 4` (the top block, not one past it): -5 ..= 20
    // (26 light sections), and 24 world sections.
    let expected_light_min = OVERWORLD_MIN_Y / 16 - 1; // -5
    let expected_light_max = (OVERWORLD_MIN_Y + OVERWORLD_HEIGHT - 1) / 16 + 1; // 20
    let mut non_null_light_sections = 0usize;
    let mut non_uniform_light_sections = 0usize;
    let mut non_uniform_emptiness = 0usize;
    for (key, truth) in &golden.chunks {
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
        if truth.stored_pos != parsed {
            return Err(Error::Manifest(format!(
                "light chunk {key} has stored_pos {:?} — the chunk's internal xPos/zPos do \
                 not match its grid key; refusing a relabeled chunk",
                truth.stored_pos
            )));
        }
        // Stage-specific truth: the serialized status of a level-33 forced chunk
        // is exactly `minecraft:full`, and its light must have been computed and
        // persisted (`light_correct` — status ≥ LIGHT + `isLightOn` +
        // `starlight.light_version` 10). A chunk below full or without persisted
        // light is refused, never silently accepted.
        if truth.status != EXPECTED_STATUS {
            return Err(Error::Manifest(format!(
                "light chunk {key} is {} (expected {EXPECTED_STATUS}) — a chunk below full \
                 cannot be the LIGHT checkpoint (its serialization does not carry the \
                 computed light arrays)",
                truth.status
            )));
        }
        if !truth.light_correct {
            return Err(Error::Manifest(format!(
                "light chunk {key} is not light-correct — Paper did not persist Starlight \
                 light for it; the serialization carries no trustworthy light arrays"
            )));
        }
        if truth.min_light_section != expected_light_min
            || truth.max_light_section != expected_light_max
        {
            return Err(Error::Manifest(format!(
                "light chunk {key} covers light sections {}..={} (expected {expected_light_min}..=\
                 {expected_light_max}) — the vertical extent drifted from the overworld",
                truth.min_light_section, truth.max_light_section
            )));
        }
        let expected_sections = truth.max_light_section - truth.min_light_section + 1;
        if truth.sky_nibbles.len() != expected_sections as usize {
            return Err(Error::Manifest(format!(
                "light chunk {key} has {} sky-nibble entries (expected {expected_sections} \
                 light sections {expected_light_min}..={expected_light_max})",
                truth.sky_nibbles.len()
            )));
        }
        for (cy, nibble) in &truth.sky_nibbles {
            if *cy < truth.min_light_section || *cy > truth.max_light_section {
                return Err(Error::Manifest(format!(
                    "light chunk {key} sky nibble key {cy} is outside {expected_light_min}..=\
                     {expected_light_max}"
                )));
            }
            if let Some(bytes) = nibble {
                if bytes.len() != 2048 {
                    return Err(Error::Manifest(format!(
                        "light chunk {key} sky nibble at section {cy} is {} bytes, not the 2048 \
                         (16×16×8) DataLayer contract",
                        bytes.len()
                    )));
                }
                non_null_light_sections += 1;
                // A lit real chunk has a non-trivial sky nibble somewhere: not
                // uniformly 0 (nothing lit) and not uniformly 0xFF (no terrain
                // shadowing within the section).
                if bytes.iter().any(|&b| b != 0) && bytes.iter().any(|&b| b != 0xFF) {
                    non_uniform_light_sections += 1;
                }
            }
        }
        // The derived emptiness map must cover every world section (-4..=19,
        // 24 sections): one per in-bounds section entry in `sections`.
        let expected_world_sections =
            (OVERWORLD_MIN_Y + OVERWORLD_HEIGHT - 1) / 16 - OVERWORLD_MIN_Y / 16 + 1; // 24
        if truth.sky_emptiness.len() != expected_world_sections as usize {
            return Err(Error::Manifest(format!(
                "light chunk {key} sky emptiness map has {} entries (expected {expected_world_sections} \
                 world sections)",
                truth.sky_emptiness.len()
            )));
        }
        if truth.sky_emptiness.iter().any(|&empty| empty)
            && truth.sky_emptiness.iter().any(|&empty| !empty)
        {
            non_uniform_emptiness += 1;
        }
    }

    // 4. LIGHT-stage non-vacuity. A capture that cannot distinguish a
    //    Starlight-lit overworld chunk set from an empty or uniform floor is
    //    refused loudly.
    if non_null_light_sections == 0 {
        return Err(Error::Manifest(
            "no committed light chunk carries a non-null sky nibble — nothing was lit; a \
             capture with no sky light cannot be the LIGHT checkpoint"
                .into(),
        ));
    }
    if non_uniform_light_sections == 0 {
        return Err(Error::Manifest(
            "no committed light chunk carries a sky nibble with real terrain shadowing (all \
             non-null nibbles are uniformly 0 or uniformly 0xFF) — a superflat echo, not a \
             lit overworld chunk"
                .into(),
        ));
    }
    if non_uniform_emptiness == 0 {
        return Err(Error::Manifest(
            "no committed light chunk has a non-uniform sky emptiness map (all sections empty \
             or all non-empty) — the grid is not real overworld terrain"
                .into(),
        ));
    }
    // The committed grid must sit inside the forced grid (its own 1-radius
    // context is what Paper lit in), and every committed chunk's 1-radius must
    // be forced — otherwise the checkpoint captured a different light context.
    for (cx, cz) in &grid {
        for dx in -1..=1 {
            for dz in -1..=1 {
                if !forced.contains(&(cx + dx, cz + dz)) {
                    return Err(Error::Manifest(format!(
                        "committed chunk ({cx},{cz})'s 1-radius neighbour ({},{}) is not in the \
                         forced 5×5 grid — the captured light context is not self-contained",
                        cx + dx,
                        cz + dz
                    )));
                }
            }
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

/// `fixtures/light/manifest.json`, serialized in the exact committed field
/// order so regeneration is byte-identical (git-clean), mirroring the
/// generated-expected manifest convention.
#[derive(serde::Serialize)]
struct LightManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Write `fixtures/light/manifest.json` from the freshly generated golden and
/// chunk NBTs (byte-identical field order). The seed is READ BACK OUT OF THE
/// GOLDEN (PR #595) — regeneration never stamps a hardcoded 42.
pub fn regenerate_manifest(
    dir: &Path,
    golden: &LightGolden,
    chunk_nbts: &BTreeMap<String, Vec<u8>>,
) -> Result<(), Error> {
    let json = serde_json::to_string(golden)
        .map_err(|e| Error::Gate(format!("serializing light golden: {e}")))?;
    let seed_str = golden.seed.to_string();
    let mut captured = vec![CapturedFile {
        path: FIXTURE_BASENAME.to_string(),
        sha256: crate::sha256_hex(json.as_bytes()),
        bytes: json.len(),
    }];
    for (cx, cz) in forced_coordinates() {
        let key = format!("{cx},{cz}");
        let bytes = chunk_nbts.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "regenerating light manifest: chunk {key} NBT is absent"
            ))
        })?;
        captured.push(CapturedFile {
            path: chunk_fixture_path(cx, cz),
            sha256: crate::sha256_hex(bytes),
            bytes: bytes.len(),
        });
    }
    let manifest = LightManifest {
        format: 1,
        paper: PINNED_PAPER,
        seed: &seed_str,
        level_type: "minecraft:normal",
        kind: KIND,
        note: "Seed-42 LIGHT oracle checkpoint: per-section Starlight sky nibbles + derived \
               sky-emptiness map + light_correct for the committed 3x3 interior \
               {19..21}x{19..21} of a self-contained forced 5x5 grid {18..22}x{18..22}, \
               captured from the pinned Paper runtime by booting a fresh normal-overworld \
               world and force-generating the grid to level 33 (ChunkLevel.byStatus(FULL)), \
               serialized as minecraft:full. FULL is the forced path's ceiling (a level-35 \
               LIGHT ticket is INACCESSIBLE and never generates), and FULL serialization \
               carries the Starlight-computed light arrays (ChunkLightTask's fresh-chunk \
               branch runs lightChunk -> SkyStarLightEngine.light), so the persisted light \
               data IS the LIGHT-stage output this captures. sky_nibbles are the \
               to_vanilla_nibble byte views per light section (keyed by light-section y, \
               None for null sections); sky_emptiness is the per-world-section map derived \
               exactly like StarLightEngine.getEmptySectionsForChunk (absent or hasOnlyAir \
               -> empty). The raw NBT of all 25 forced chunks is committed under chunks/ \
               so the rivet-server engine differential can rebuild the exact context Paper \
               lit in. Non-vacuity: at least one committed chunk must carry a non-null sky \
               nibble with real terrain shadowing, and a non-uniform emptiness map. \
               Regenerate with `rivet-oracle regenerate --light` (twin-boot byte-identity \
               proof).",
        captured,
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize light manifest: {e}")))?;
    text.push('\n');
    fs::write(dir.join("manifest.json"), text)?;
    Ok(())
}

/// Twin-boot deterministic capture into the committed fixture tree (the
/// `regenerate --light` path). Requires the two independent Paper captures to
/// produce byte-identical goldens AND byte-identical raw chunk NBTs before
/// anything is committed — a nondeterministic pair is never committed.
pub fn run_probe(dir: &Path) -> Result<(), Error> {
    println!("[1/3] forced LIGHT capture A: fresh seed-42 Paper boot under the 1/1 pin...");
    let a = capture_world(PINNED_SEED)?;
    println!("[2/3] forced LIGHT capture B: fresh seed-42 Paper boot under the 1/1 pin...");
    let b = capture_world(PINNED_SEED)?;

    if a != b {
        return Err(Error::Gate(
            "light twin-boot byte-identity check failed — the two independent Paper captures \
             produced DIFFERENT light goldens or chunk NBTs; refusing to commit a \
             nondeterministic checkpoint"
                .into(),
        ));
    }
    // Validate the (byte-identical) capture against the light contract BEFORE
    // committing — two equally-wrong captures must be refused.
    validate_golden(&a.golden)?;

    println!("[3/3] byte-identical + contract-valid; writing the committed checkpoint...");
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("chunks"))?;
    let json = serde_json::to_string(&a.golden)
        .map_err(|e| Error::Gate(format!("serializing light golden: {e}")))?;
    fs::write(dir.join(FIXTURE_BASENAME), json.as_bytes())?;
    for (cx, cz) in forced_coordinates() {
        let key = format!("{cx},{cz}");
        let bytes = a.chunk_nbts.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "regenerating light checkpoint: chunk {key} NBT is absent"
            ))
        })?;
        fs::write(dir.join(chunk_fixture_path(cx, cz)), bytes)?;
    }
    regenerate_manifest(dir, &a.golden, &a.chunk_nbts)?;
    println!(
        "regenerated light seed-{PINNED_SEED} checkpoint under {} (twin-boot byte-identical; \
         {} Starlight-lit committed chunks + {} forced chunk NBTs)",
        dir.display(),
        a.golden.chunks.len(),
        a.chunk_nbts.len()
    );
    Ok(())
}

/// The tamper negative control: corrupt a committed bit pattern (flip one byte
/// of the golden JSON) and assert the verification FAILS — proving the
/// comparison is not vacuous. Operates on a scratch copy in the temp dir so the
/// committed fixtures are never mutated.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    let scratch = std::env::temp_dir().join(format!("rivet-oracle-light-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)
        .map_err(|e| Error::Gate(format!("cannot create scratch {}: {e}", scratch.display())))?;
    fs::create_dir_all(scratch.join("chunks"))
        .map_err(|e| Error::Gate(format!("cannot create scratch chunks dir: {e}")))?;
    fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME))?;
    fs::copy(dir.join("manifest.json"), scratch.join("manifest.json"))?;
    for (cx, cz) in forced_coordinates() {
        let path = chunk_fixture_path(cx, cz);
        fs::copy(dir.join(&path), scratch.join(&path))?;
    }
    let golden = scratch.join(FIXTURE_BASENAME);
    let original = fs::read(&golden)
        .map_err(|e| Error::Gate(format!("cannot read {}: {e}", golden.display())))?;
    let i = (original.len() / 2).min(original.len().saturating_sub(1));
    let mut tampered = original.clone();
    tampered[i] ^= 0xFF;
    fs::write(&golden, &tampered)?;
    let result = verify_light(&scratch);
    let _ = fs::remove_dir_all(&scratch);
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "light tamper was NOT detected — the comparison is vacuous".into(),
        }),
        Err(_) => Ok(()),
    }
}

/// The `light` subcommand:
///
///   cargo run -p rivet-oracle -- light <seed>            verify committed fixture
///   cargo run -p rivet-oracle -- light <seed> --to <out>  capture: boot Paper -> write <out>
///   cargo run -p rivet-oracle -- light <seed> --tamper    negative control
///
/// Verify mode is pinned to the committed seed-42 checkpoint (`<seed>` must be
/// 42); the `--to` capture path accepts any seed whose grid passes the light
/// contract. `--tamper` and `--to` are mutually exclusive.
pub fn run_cli(args: &[&str]) -> Result<(), Error> {
    let parsed = parse_cli(args)?;
    let dir = crate::crate_dir().join("fixtures/light");
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
            "light verify is pinned to seed {PINNED_SEED}; got {} — the committed checkpoint \
             only carries the seed-42 ground truth",
            parsed.seed
        )));
    }
    // Route through the same tri-state classification as the gate (see main.rs
    // `verify_light_step`): wholly absent -> UNVERIFIED (exit 3), partial/corrupt
    // -> hard failure, so the CLI and the gate cannot disagree.
    crate::verify_light_step(&dir)
}

/// Parsed `light` CLI arguments.
struct CliArgs {
    seed: i64,
    to: Option<PathBuf>,
    tamper: bool,
}

/// Parse the `light` arguments (everything after the subcommand name). A
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
                    return Err(Error::Gate("light --to requires a destination path".into()));
                };
                if path.starts_with('-') {
                    return Err(Error::Gate(
                        "light --to requires a destination path, not an option".into(),
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
                    return Err(Error::Gate("light takes exactly one seed".into()));
                }
                seed =
                    Some(other.parse().map_err(|_| {
                        Error::Gate(format!("light seed {other} is not an integer"))
                    })?);
                i += 1;
            }
            other
                if other.starts_with('-')
                    && !other.starts_with("--")
                    && other[1..].parse::<i64>().is_ok() =>
            {
                if seed.is_some() {
                    return Err(Error::Gate("light takes exactly one seed".into()));
                }
                seed =
                    Some(other.parse().map_err(|_| {
                        Error::Gate(format!("light seed {other} is not an integer"))
                    })?);
                i += 1;
            }
            other => {
                return Err(Error::Gate(format!("light: unknown option {other}")));
            }
        }
    }
    let seed = seed.ok_or_else(|| Error::Gate("light requires a seed".into()))?;
    if tamper && to.is_some() {
        return Err(Error::Gate(
            "light --tamper and --to are mutually exclusive".into(),
        ));
    }
    Ok(CliArgs { seed, to, tamper })
}

/// Load a committed light chunk NBT (raw uncompressed) — the helper the
/// tests use to sanity-check the fixture tree on disk.
#[cfg(test)]
fn load_chunk_nbt(dir: &Path, cx: i32, cz: i32) -> Result<CompoundTag, Error> {
    let path = dir.join(chunk_fixture_path(cx, cz));
    let bytes = fs::read(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let mut input = rivet_util::DataInputStream::new(std::io::Cursor::new(bytes));
    nbt_io::read_unlimited(&mut input)
        .map_err(|e| Error::Manifest(format!("cannot parse {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// The committed light golden is a load-bearing deliverable: a test that
    /// needs it must FAIL when it is absent, never silently return (D8).
    fn require_fixture(dir: &Path) {
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed light fixtures {} are ABSENT — the seed-42 LIGHT checkpoint \
                 and its gate cannot verify; restore them (git checkout) or this test is red, \
                 never silently skipped",
                dir.display()
            );
        }
    }

    #[test]
    fn committed_grid_is_the_three_by_three_interior() {
        let grid = committed_coordinates();
        assert_eq!(grid.len(), 9);
        assert!(grid.contains(&(19, 19)));
        assert!(grid.contains(&(20, 20)));
        assert!(grid.contains(&(21, 21)));
        assert!(!grid.contains(&(18, 18)));
        assert!(!grid.contains(&(22, 22)));
    }

    #[test]
    fn forced_grid_contains_every_committed_radius_one() {
        let forced = forced_coordinates();
        assert_eq!(forced.len(), 25);
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
    fn forced_grid_is_far_from_seed42_spawn() {
        // Seed 42's spawn-area chunks sit around chunk (-2,0); the forced grid
        // must not overlap them (spawn-chunk influence would couple the light
        // to boot1's partial world).
        let forced = forced_coordinates();
        for (cx, cz) in &forced {
            assert!(
                !(-4..=0).contains(cx) || !(-2..=2).contains(cz),
                "forced chunk ({cx},{cz}) overlaps the seed-42 spawn area"
            );
        }
    }

    #[test]
    fn committed_light_verifies() {
        let dir = fixtures_dir().join("light");
        require_fixture(&dir);
        verify_light(&dir).expect("committed light golden should verify");
    }

    #[test]
    fn committed_light_is_non_vacuous() {
        let dir = fixtures_dir().join("light");
        require_fixture(&dir);
        let golden = load(&dir).unwrap();
        assert_eq!(golden.seed, PINNED_SEED);
        assert_eq!(golden.format, 1);
        assert_eq!(golden.chunks.len(), 9);
        let mut non_null = 0usize;
        let mut non_uniform = 0usize;
        for (key, truth) in &golden.chunks {
            assert_eq!(truth.status, EXPECTED_STATUS, "chunk {key}");
            assert!(truth.light_correct, "chunk {key} must be light-correct");
            let parsed: Vec<i32> = key.split(',').map(|s| s.parse().unwrap()).collect();
            assert_eq!(
                truth.stored_pos,
                [parsed[0], parsed[1]],
                "chunk {key} pos sanity"
            );
            assert_eq!(truth.min_light_section, -5, "chunk {key}");
            assert_eq!(truth.max_light_section, 20, "chunk {key}");
            for (cy, nibble) in &truth.sky_nibbles {
                assert!((-5..=20).contains(cy), "chunk {key} section {cy}");
                if let Some(bytes) = nibble {
                    assert_eq!(bytes.len(), 2048, "chunk {key} section {cy}");
                    non_null += 1;
                    if bytes.iter().any(|&b| b != 0) && bytes.iter().any(|&b| b != 0xFF) {
                        non_uniform += 1;
                    }
                }
            }
        }
        assert!(
            non_null > 0,
            "no committed chunk carries a non-null sky nibble"
        );
        assert!(
            non_uniform > 0,
            "no committed chunk carries a sky nibble with real terrain shadowing"
        );
    }

    /// The default verify path must fail UNVERIFIED when the committed fixture
    /// tree is absent — never silently skip (D8).
    #[test]
    fn missing_fixture_tree_is_unverified() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-light-missing-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        let result = crate::verify_light_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    /// A PARTIAL light tree — only manifest.json, no light.json — is a corrupt
    /// checkpoint, NOT an absent prerequisite: it must hard-fail
    /// (Error::Manifest, exit 1), never classify as UNVERIFIED (exit 3).
    #[test]
    fn partial_fixture_tree_is_a_hard_failure() {
        for missing in ["manifest.json", FIXTURE_BASENAME] {
            let scratch = std::env::temp_dir().join(format!(
                "rivet-oracle-light-partial-{missing}-{}",
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
            let result = crate::verify_light_step(&scratch);
            let _ = fs::remove_dir_all(&scratch);
            assert!(
                matches!(result, Err(crate::Error::Manifest(_))),
                "a partial light tree missing {missing} must hard-fail (Error::Manifest), \
                 got {result:?}"
            );
        }
    }

    /// The SHA-256 binding is load-bearing, not optional: `verify_fixtures`
    /// only checks the files the manifest DOES list, so a manifest whose
    /// `captured` list is empty (or omits light.json) must be rejected even
    /// though the golden bytes themselves are untouched.
    #[test]
    fn manifest_without_captured_binding_is_rejected() {
        let dir = fixtures_dir().join("light");
        require_fixture(&dir);
        let golden = fs::read(dir.join(FIXTURE_BASENAME)).unwrap();
        for captured in [
            vec![],
            vec![serde_json::json!({
                "path": "other.json",
                "sha256": crate::sha256_hex(&golden),
                "bytes": golden.len(),
            })],
        ] {
            let scratch = std::env::temp_dir()
                .join(format!("rivet-oracle-light-nocap-{}", std::process::id()));
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
                "captured": captured,
            });
            fs::write(
                scratch.join("manifest.json"),
                serde_json::to_string(&manifest).unwrap(),
            )
            .unwrap();
            let result = crate::verify_light_step(&scratch);
            let _ = fs::remove_dir_all(&scratch);
            assert!(
                matches!(result, Err(crate::Error::Manifest(_))),
                "a manifest without the light.json SHA-256 binding must be rejected, got {result:?}"
            );
        }
    }

    /// A manifest that omits a forced chunk NBT binding must be rejected even
    /// though light.json itself is untouched — the differential's inputs are
    /// load-bearing.
    #[test]
    fn manifest_without_chunk_nbt_bindings_is_rejected() {
        let dir = fixtures_dir().join("light");
        require_fixture(&dir);
        let golden = fs::read(dir.join(FIXTURE_BASENAME)).unwrap();
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-light-nochunks-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join(FIXTURE_BASENAME), &golden).unwrap();
        // A manifest binding light.json but none of the 25 chunk NBTs.
        let manifest = serde_json::json!({
            "format": 1,
            "paper": PINNED_PAPER,
            "seed": "42",
            "level-type": "minecraft:normal",
            "kind": KIND,
            "captured": [{
                "path": FIXTURE_BASENAME,
                "sha256": crate::sha256_hex(&golden),
                "bytes": golden.len(),
            }],
        });
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let result = crate::verify_light_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "a manifest without the forced chunk NBT bindings must be rejected, got {result:?}"
        );
    }

    /// Every committed chunk's committed-NBT is present on disk and parses —
    /// the raw block input the rivet-server differential rebuilds.
    #[test]
    fn committed_chunk_nbts_parse() {
        let dir = fixtures_dir().join("light");
        require_fixture(&dir);
        let height = height_accessor::create(OVERWORLD_MIN_Y, OVERWORLD_HEIGHT);
        for (cx, cz) in forced_coordinates() {
            let tag = load_chunk_nbt(&dir, cx, cz).expect("committed chunk NBT readable");
            let data = SerializableChunkData::parse(height, &tag)
                .expect("committed chunk NBT parses")
                .expect("committed chunk NBT has Status");
            assert_eq!(
                data.status().serialization_name(),
                EXPECTED_STATUS,
                "committed chunk ({cx},{cz})"
            );
        }
    }
}
