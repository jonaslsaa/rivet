//! The generated-world seed-42 ground-truth handoff (`generated-expected <seed>`).
//!
//! PR #563's generated-world acceptance compares the seed-42 content a
//! `rivet-server --seed 42` serves against a Paper-captured reference — never
//! against nothing, and never against a superflat fallback. This module builds
//! that reference: it boots the pinned Paper 26.2 runtime on a fresh seed-42
//! normal-overworld world, force-generates a fixed spawn-area grid to
//! `minecraft:full` (the same forced-ticket mechanism issue #51 uses for the
//! corpus), extracts the deterministic per-chunk sample contract via
//! `loaded_world::extract_world`, and commits the result as
//! `fixtures/generated-expected/generated-expected.json`.
//!
//! The per-chunk contract is exactly what the generated acceptance compares
//! (`compare_generated_content` in the PR): 16×16 `surface`/`bedrock`/
//! `below_feet` arrays indexed row-major `z*16+x`, sampled at the chunk center
//! offset (8,8) → index `8*16+8`. `surface` is the highest non-air block in the
//! column, which the extractor computes across the full column height; because
//! the overworld's ceiling is `WORLD_CEILING_Y=320`, this equals exactly what
//! the acceptance observes scanning down from y=320 to `BELOW_BEDROCK_Y`.
//! `bedrock` is the block at `y=-60` and `below_feet` the block at `y=-61`. The
//! extractor (already tested in `loaded_world.rs`) produces exactly this shape
//! from the region files; the verify path here pins the provenance, the
//! manifest hashes, the forced-grid shape, and the anti-superflat sample
//! contract.
//!
//! Honesty rules (D8: never fabricate expected content, never silently fall
//! back to superflat):
//!
//!   * capture (`--to <out>`) returns `Error::Unverified` (exit 3) when the
//!     pinned Paper runtime is absent — it never writes an empty or fabricated
//!     manifest, and it removes a stale `--to` file first so a failed capture
//!     cannot leave a previous success behind.
//!   * capture is a two-boot sequence (a create boot, then a forced-ticket
//!     capture boot) that discards boot1's partial spawn-area chunks so the
//!     forced grid is generated from a blank chunk state (byte-deterministic).
//!   * verify returns `Error::Unverified` (exit 3) when the committed fixture
//!     tree is absent — never a silent green.
//!   * verify rejects a fixture that is not exactly the forced grid of FULL
//!     chunks with real generated variety: a superflat echo (all-air bedrock
//!     plane at -60, uniform surface arrays, a tiny distinct-block set) is
//!     refused loudly, so the handoff can never be gamed into a vacuous pass.
//!   * capture enforces provenance: after the capture boot, the materialized
//!     server jar's `Git-Commit` must match the pinned `0a99345` before any
//!     content is written — a wrong-commit jar (a `RIVET_ORACLE_JAR` override or
//!     a stale `work/jars/` paperclip) is refused, never stamped with the pinned
//!     provenance.
//!   * the regenerate path validates a capture against the anti-superflat
//!     contract BEFORE committing it, in addition to the twin-boot byte-identity
//!     proof — two equally-wrong captures are refused, never committed.
//!   * the tamper negative control proves a flipped byte in the golden fails
//!     verification (the manifest SHA-256 gate is not vacuous).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::loaded_world::{self, WorldManifest};
use crate::{CapturedFile, Error};

/// The fixture kind name (matches the manifest `kind` and the regenerate flag).
pub const KIND: &str = "generated-expected";
/// The committed golden filename.
pub const FIXTURE_BASENAME: &str = "generated-expected.json";
/// The pinned handoff seed (PR #563 pins the runner to this seed).
pub const PINNED_SEED: i64 = 42;
/// The pinned Paper provenance the handoff is captured against.
pub const PINNED_PAPER: &str = "26.2-DEV-main@0a99345";
/// The overworld world-ceiling Y the acceptance samples down from (mirrors the
/// client's `WORLD_CEILING_Y` and the extractor's full-height scan).
pub const WORLD_CEILING_Y: i32 = 320;
/// The bedrock-plane sample Y (mirrors the extractor's `BEDROCK_Y`).
pub const BEDROCK_Y: i32 = -60;
/// One block below the bedrock plane (mirrors the extractor's
/// `BELOW_BEDROCK_Y`).
pub const BELOW_BEDROCK_Y: i32 = -61;
/// The chunk-center sample index in the 16×16 row-major arrays (`z*16+x` at the
/// client's (8,8) center offset).
pub const CENTER_INDEX: usize = 8 * 16 + 8;
/// The committed spawn-area grid: chunk coordinates -4..=4 in both axes (81
/// chunks, 128×128 blocks), covering Paper's seed-42 spawn (chunk -2,0) and the
/// acceptance's 5×5 sample grid (x=-4..0, z=-2..2) with margin.
pub const GRID_MIN: i32 = -4;
pub const GRID_MAX: i32 = 4;
/// The FORCED generation grid: -6..=6 in both axes (169 chunks). The committed
/// grid is a strict interior subset so every committed chunk's 8 neighbors are
/// also forced FULL — the buffer that keeps border-tree placement deterministic.
pub const FORCE_GRID_MIN: i32 = -6;
pub const FORCE_GRID_MAX: i32 = 6;

/// The committed grid coordinates, deterministically ordered (x-major).
pub fn grid_coordinates() -> Vec<(i32, i32)> {
    (GRID_MIN..=GRID_MAX)
        .flat_map(|x| (GRID_MIN..=GRID_MAX).map(move |z| (x, z)))
        .collect()
}

/// The forced grid coordinates (a superset of the committed grid), ordered the
/// same way.
pub fn forced_coordinates() -> Vec<(i32, i32)> {
    (FORCE_GRID_MIN..=FORCE_GRID_MAX)
        .flat_map(|x| (FORCE_GRID_MIN..=FORCE_GRID_MAX).map(move |z| (x, z)))
        .collect()
}

/// The capture boot's `server-port` setting: `0` asks Paper to bind an
/// OS-assigned free port. The capture is headless (no client joins,
/// `enable-status=false`), so the bound port is never addressed; a fixed
/// isolated port would risk colliding with another worktree's concurrent
/// tooling, and the shared 25599 is the serialized release gate's port. Binding
/// port 0 keeps a concurrent strict gate (or any other server) on any port from
/// colliding with the capture boot.
const CAPTURE_SERVER_PORT: &str = "0";

/// Rewrite the committed M2 `server.properties` text for a capture: pin `level-
/// seed` to `seed` and set `server-port` to `CAPTURE_SERVER_PORT` (`0` → an
/// OS-assigned free port), so the capture never binds the shared oracle port
/// 25599 (the serialized release gate's port) or any fixed port another
/// worktree might use. Returns `Error::Gate` when either line is absent — the
/// capture config must not be silently derived from a config that lacks the
/// lines it depends on.
fn rewrite_properties(text: &str, seed: i64) -> Result<String, Error> {
    let seed_line = format!("level-seed={seed}");
    let mut rewritten = String::new();
    let mut replaced_seed = false;
    let mut replaced_port = false;
    for line in text.lines() {
        if line.starts_with("level-seed=") {
            rewritten.push_str(&seed_line);
            replaced_seed = true;
        } else if line.starts_with("server-port=") {
            rewritten.push_str(&format!("server-port={CAPTURE_SERVER_PORT}"));
            replaced_port = true;
        } else {
            rewritten.push_str(line);
        }
        rewritten.push('\n');
    }
    if !replaced_seed {
        return Err(Error::Gate(
            "server-normal.properties has no level-seed line to rewrite".into(),
        ));
    }
    if !replaced_port {
        return Err(Error::Gate(
            "server-normal.properties has no server-port line to rewrite".into(),
        ));
    }
    Ok(rewritten)
}

/// Write a seed-customized `server.properties` (the committed
/// `server-normal.properties` with `level-seed` rewritten and `server-port`
/// isolated from the shared 25599) into the capture work dir, so a capture can
/// generate any seed while seed 42 stays byte-identical to the committed config.
/// Returns the temp properties path.
fn seed_properties(seed: i64) -> Result<PathBuf, Error> {
    let src = crate::crate_dir().join("fixtures/server-normal.properties");
    let text = fs::read_to_string(&src).map_err(|e| {
        Error::Gate(format!(
            "cannot read {} to build the capture config: {e}",
            src.display()
        ))
    })?;
    let rewritten = rewrite_properties(&text, seed)?;
    let dir = crate::crate_dir().join("work/generated-expected");
    fs::create_dir_all(&dir)?;
    let path = dir.join("server.properties");
    fs::write(&path, rewritten)?;
    Ok(path)
}

/// The capture's dedicated run dir — a scratch space isolated from the shared
/// `work/verify/run` the oracle gates (M0/M2/FULL) boot in, so a capture can
/// never wipe or be wiped by a concurrent gate run.
fn capture_run_dir() -> PathBuf {
    crate::crate_dir().join("work/generated-expected/run")
}

/// Confirm the server jar the capture actually booted carries the pinned Paper
/// commit (`0a99345`, the commit part of `PINNED_PAPER`).
///
/// The captured content is stamped with `PINNED_PAPER` provenance in the
/// committed manifest, so a boot from a different commit (a `RIVET_ORACLE_JAR`
/// override or a stale `work/jars/` paperclip) must be refused — otherwise a
/// wrong-commit capture would be handed off as the pinned ground truth, which is
/// fabricated provenance (D8). Mirrors the gate's `check_pin`: the source of
/// truth is the materialized server jar the paperclip produced into the run dir
/// and the JVM actually loaded.
fn check_capture_pin(run_dir: &Path) -> Result<(), Error> {
    let expected = crate::parse_paper_pin(Some(PINNED_PAPER)).ok_or_else(|| {
        Error::Gate("PINNED_PAPER carries no @<commit> pin to verify the capture against".into())
    })?;
    let jar = crate::materialized_server_jar(run_dir);
    let actual = crate::read_jar_git_commit(&jar)?;
    // Reuse the gate's already-tested classification (main.rs `classify_pin`):
    // Match / Mismatch / Unavailable, where Unavailable is never a silent pass.
    match crate::classify_pin(Some(expected), actual) {
        crate::PinVerdict::Match => Ok(()),
        crate::PinVerdict::Mismatch { expected, actual } => {
            Err(Error::PinMismatch { expected, actual })
        }
        crate::PinVerdict::Unavailable { reason } => Err(Error::PinUnavailable {
            reason: format!(
                "{reason} — the generated-expected capture's content would be stamped with \
                 {PINNED_PAPER} provenance; refusing to fabricate it"
            ),
        }),
    }
}

/// Boot the pinned Paper on a fresh seed world, force-generate the spawn grid
/// to FULL, and extract the deterministic per-chunk sample contract.
///
/// This is the single-capture workhorse for both the PR `--to` path (the same
/// two-boot create + forced-capture sequence) and the committed-fixture
/// regenerate path (twin two-boot captures, byte-compared). A missing Paper
/// runtime is `Error::Unverified` — a missing prerequisite, never a fabricated
/// green.
fn capture_world(seed: i64) -> Result<WorldManifest, Error> {
    let jar = crate::ensure_jar().map_err(|e| {
        Error::Unverified(format!(
            "generated-expected capture needs the pinned Paper runtime: {e} \
             (boot the M0 fixture server once per tools/rivet-oracle/README.md, or set \
             RIVET_ORACLE_JAR); UNVERIFIED, never a fabricated manifest"
        ))
    })?;
    let props = seed_properties(seed)?;
    let run_dir = capture_run_dir();
    // The capture's runtime is self-contained in this dedicated dir: the first
    // boot's `prepare_run_dir` materializes libraries/versions/cache here, and
    // later boots and captures reuse them. We deliberately never symlink/copy
    // from the shared `work/verify/run` (the dir the oracle gates boot in): a
    // concurrent gate may `remove_dir_all` it or write to its `cache/` mid-boot,
    // which would dangle a link or hand the capture a half-written runtime.
    let grid = grid_coordinates();
    let forced = forced_coordinates();

    // boot1 (create): a plain spawn boot creates the seed world (the world must
    // exist before the forced tickets can load). The world persists in
    // run_dir/world between the two boots (the exact `full_forced_extraction`
    // pattern).
    crate::prepare_run_dir(&run_dir, &props)?;
    let create_log = run_dir.with_file_name("boot-generated-create.log");
    println!("      [boot1] creating the seed-{seed} normal-overworld world...");
    crate::boot_and_shutdown(&run_dir, &create_log, &jar)?;

    // Discard boot1's partial spawn-area chunks: Paper's create boot saves a
    // ring of partially-generated chunks around spawn (some already carrying
    // trees from an interrupted FEATURES pass), and regenerating THOSE to FULL
    // in boot2 is not byte-deterministic (border-tree placement races with the
    // leftover partial state). Deleting the region files leaves the world
    // metadata (level.dat, seed, spawn) intact and lets boot2 generate the
    // forced grid from a blank chunk state — which IS byte-deterministic
    // (verified across independent boots). The forced grid is a superset of the
    // committed grid so every committed chunk's neighbors are forced FULL too.
    clear_region_files(&run_dir.join("world"))?;

    // Inject level-33 forced tickets for the spawn grid into every dimension,
    // then boot2 loads those persistent chunks and finishes them to FULL.
    crate::inject_forced_tickets(&run_dir.join("world"), &forced)?;
    let capture_log = run_dir.with_file_name("boot-generated.log");
    println!("      [boot2] capturing the forced FULL spawn grid...");
    crate::boot_and_shutdown(&run_dir, &capture_log, &jar)?;
    crate::verify_forced_load(&capture_log, forced.len())?;

    // Provenance: the content this capture is about to be stamped with
    // `PINNED_PAPER` in the committed manifest must have actually been generated
    // by that commit's server jar (the materialized jar the JVM loaded). A
    // different jar (wrong RIVET_ORACLE_JAR / stale work/jars override) is
    // fabricated provenance and is refused before anything is written.
    check_capture_pin(&run_dir)?;

    let manifest = loaded_world::extract_world(&run_dir.join("world")).map_err(|e| match e {
        loaded_world::ExtractError::Unverified(m) => Error::Unverified(m),
        loaded_world::ExtractError::Gate(m) => Error::Gate(m),
        loaded_world::ExtractError::Io(io) => Error::Io(io),
    })?;
    filter_to_grid(&manifest, &grid)
}

/// Delete every `.mca` region/entities/poi file under a world root's three
/// dimension dirs, leaving the world metadata (level.dat, seed, spawn, injected
/// ticket data) intact. Used to discard a create boot's partial spawn-area
/// chunks so the capture boot regenerates the forced grid from a blank chunk
/// state. The entities/poi files carry no block content (spawn-limits are 0),
/// but they are boot1 leftovers that do not participate in the regenerated
/// chunk state — clearing them keeps the capture's blank chunk state complete.
fn clear_region_files(world_dir: &Path) -> Result<(), Error> {
    let mut cleared = 0usize;
    for dim in ["overworld", "the_nether", "the_end"] {
        let base = world_dir.join("dimensions/minecraft").join(dim);
        for sub in ["region", "entities", "poi"] {
            let dir = base.join(sub);
            if !dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.extension().map(|e| e == "mca").unwrap_or(false) {
                    fs::remove_file(&path)?;
                    cleared += 1;
                }
            }
        }
    }
    println!(
        "      discarded boot1 partial chunks ({cleared} region/entities/poi files cleared; \
         the capture boot regenerates the forced grid from a blank chunk state)"
    );
    Ok(())
}

/// Keep only the forced-grid chunks and require every one to be `minecraft:full`
/// — a grid chunk that did not reach FULL is a capture failure (the forced
/// generation did not run), never content to hand off.
fn filter_to_grid(manifest: &WorldManifest, grid: &[(i32, i32)]) -> Result<WorldManifest, Error> {
    let mut chunks = BTreeMap::new();
    for (cx, cz) in grid {
        let key = format!("{cx},{cz}");
        let fp = manifest.chunks.get(&key).ok_or_else(|| {
            Error::Gate(format!(
                "forced grid chunk {key} is ABSENT from the extracted world — the forced \
                 generation did not produce it; refusing to hand off a partial capture"
            ))
        })?;
        if fp.status != "minecraft:full" {
            return Err(Error::Gate(format!(
                "forced grid chunk {key} is {} (not minecraft:full) — the forced generation \
                 did not reach FULL; refusing to hand off a partial capture",
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

/// Capture mode for the PR `--to <out>` invocation: the two-boot create +
/// forced-capture sequence (`capture_world`), written as compact JSON to `to`.
/// A stale `to` file is removed first so a failed capture never leaves a
/// previous success behind.
pub fn capture_to(seed: i64, to: &Path) -> Result<(), Error> {
    if to.exists() {
        fs::remove_file(to)?;
    }
    let manifest = capture_world(seed)?;
    // Refuse a capture that does not meet the anti-superflat sample contract —
    // a bad capture must never be handed to the acceptance as ground truth.
    validate_world(&manifest)?;
    let json = serde_json::to_string(&manifest)
        .map_err(|e| Error::Gate(format!("serializing generated-expected manifest: {e}")))?;
    if let Some(parent) = to.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(to, json.as_bytes())?;
    println!(
        "captured seed-{seed} generated-expected manifest ({} FULL grid chunks) -> {}",
        manifest.chunks.len(),
        to.display()
    );
    Ok(())
}

/// Parse + structurally validate the committed golden (`WorldManifest` shape).
pub fn load(dir: &Path) -> Result<WorldManifest, Error> {
    let path = dir.join(FIXTURE_BASENAME);
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let world: WorldManifest = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
    if world.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported generated-expected format {} (expected 1)",
            world.format
        )));
    }
    Ok(world)
}

/// Verify the committed generated-expected golden. Failing with
/// `Error::Unverified` (exit 3) when the fixture tree is absent rather than
/// silently skipping it (D8).
pub fn verify_generated_expected_step(dir: &Path) -> Result<(), Error> {
    if !dir.join("manifest.json").is_file() || !dir.join(FIXTURE_BASENAME).is_file() {
        return Err(Error::Unverified(format!(
            "generated-expected fixtures {} are ABSENT (need both manifest.json and \
             {FIXTURE_BASENAME}) — the seed-42 ground-truth handoff and its per-chunk gate \
             cannot verify (git checkout or regenerate via --generated-expected); refusing to \
             pass green without them",
            dir.display()
        )));
    }
    verify_generated_expected(dir)
}

/// Assert the committed golden's provenance, manifest hashes, forced-grid shape,
/// per-chunk sample contract, and anti-superflat guarantees.
fn verify_generated_expected(dir: &Path) -> Result<(), Error> {
    let manifest = crate::verify_fixtures(dir)?;
    // 1. Provenance: the handoff is pinned to seed 42 and Paper 0a99345; a
    //    fixture regenerated under a different seed or commit is drift.
    if manifest.kind.as_deref() != Some(KIND) {
        return Err(Error::Manifest(format!(
            "expected kind {KIND}, got {:?}",
            manifest.kind
        )));
    }
    if crate::parse_paper_pin(manifest.paper.as_deref()).as_deref() != Some("0a99345") {
        return Err(Error::Manifest(format!(
            "generated-expected fixture not pinned to Paper 0a99345: {:?}",
            manifest.paper
        )));
    }
    if manifest.seed.as_deref() != Some("42") {
        return Err(Error::Manifest(format!(
            "generated-expected fixture seed {:?} != pinned seed 42",
            manifest.seed
        )));
    }

    validate_world(&load(dir)?)?;
    Ok(())
}

/// Assert a captured world meets the committed-grid shape, the per-chunk sample
/// contract, and the anti-superflat guarantees. Shared by the committed-fixture
/// verify and the `--to` capture path (a capture that cannot distinguish a real
/// seed-42 world from a flat floor is refused, never handed off).
fn validate_world(world: &WorldManifest) -> Result<(), Error> {
    let grid = grid_coordinates();
    let expected_keys: BTreeSet<String> = grid.iter().map(|(x, z)| format!("{x},{z}")).collect();
    let actual_keys: BTreeSet<String> = world.chunks.keys().cloned().collect();
    if actual_keys != expected_keys {
        return Err(Error::Manifest(format!(
            "generated-expected chunks {} != forced grid {} — a capture that adds or drops \
             spawn-grid chunks is drift, not the pinned handoff",
            format_set(&actual_keys),
            format_set(&expected_keys)
        )));
    }

    // 2. Shape + sample contract. Every grid chunk must be FULL with no
    //    #519-uncarried capability flags, and every sample array must be the
    //    16×16 row-major `z*16+x` contract (256 entries) the acceptance indexes
    //    at the chunk center.
    let mut distinct_union: BTreeSet<&str> = BTreeSet::new();
    let mut surface_patterns: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut below_patterns: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut bedrock_non_air = 0usize;
    let mut total_columns = 0usize;
    for (key, fp) in &world.chunks {
        if fp.status != "minecraft:full" {
            return Err(Error::Manifest(format!(
                "generated-expected chunk {key} is {} (not minecraft:full) — the handoff only \
                 carries FULL ground truth",
                fp.status
            )));
        }
        // The chunk's stored xPos/zPos must match the grid key — a relabeled or
        // fabricated chunk is refused, not silently accepted as ground truth.
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
                "generated-expected chunk {key} has stored_pos {:?} — the chunk's internal \
                 xPos/zPos do not match its grid key; refusing a relabeled chunk",
                fp.stored_pos
            )));
        }
        if !fp.capability_flags.is_empty() {
            return Err(Error::Manifest(format!(
                "generated-expected chunk {key} carries #519-uncarried capability flags \
                 {flags:?} — the acceptance would be UNVERIFIED, not green",
                flags = fp.capability_flags
            )));
        }
        // The per-chunk sample contract: 16×16 row-major `z*16+x` arrays sampled
        // at the acceptance's vertical scan — surface = highest non-air block in
        // [`BELOW_BEDROCK_Y`, `WORLD_CEILING_Y`], bedrock = the block at
        // `BEDROCK_Y`, below_feet = the block at `BELOW_BEDROCK_Y`. A short
        // array is a malformed handoff the acceptance would compare against a
        // partial column.
        for (field, arr) in [
            ("surface", &fp.surface),
            ("bedrock", &fp.bedrock),
            ("below_feet", &fp.below_feet),
        ] {
            if arr.len() != 256 {
                return Err(Error::Manifest(format!(
                    "generated-expected chunk {key} {field} array is {} entries, not the 16×16 \
                     (z*16+x) contract the acceptance indexes at the chunk center",
                    arr.len()
                )));
            }
        }
        distinct_union.extend(fp.distinct.iter().map(String::as_str));
        surface_patterns.insert(fp.surface.clone());
        below_patterns.insert(fp.below_feet.clone());
        bedrock_non_air += fp.bedrock.iter().filter(|b| *b != "minecraft:air").count();
        total_columns += 256;
    }

    // 3. Anti-superflat: the handoff must be genuine generated terrain, not a
    //    repeated superflat floor and not a fabricated uniform fixture. A
    //    superflat world's FULL chunks share one surface pattern and one
    //    below-feet pattern, a 3-block distinct set, and an all-air bedrock
    //    plane at y=-60 (its 4 layers end at y=-61); the generated overworld
    //    has varied surface/below-feet patterns, 75+ distinct blocks, and a
    //    dense deepslate/bedrock floor at y=-60.
    if distinct_union.len() < 3 {
        return Err(Error::Manifest(format!(
            "generated-expected has only {} distinct block names — a superflat echo, not \
             generated terrain; refusing to hand off content that cannot distinguish a real \
             seed-42 world from a flat floor",
            distinct_union.len()
        )));
    }
    if surface_patterns.len() < 2 {
        return Err(Error::Manifest(
            "all generated-expected FULL chunks share an identical surface array — a repeated \
             superflat floor, not generated terrain"
                .into(),
        ));
    }
    if below_patterns.len() < 2 {
        return Err(Error::Manifest(format!(
            "all generated-expected FULL chunks share an identical below_feet array \
             (y={BELOW_BEDROCK_Y}) — a uniform floor, not depth sampled into the generated \
             overworld"
        )));
    }
    if bedrock_non_air * 2 <= total_columns {
        return Err(Error::Manifest(format!(
            "generated-expected bedrock plane (y={BEDROCK_Y}) is non-air in only \
             {bedrock_non_air}/{total_columns} columns — a superflat floor has no bedrock at \
             y={BEDROCK_Y}; refusing to hand off content that cannot distinguish depth into the \
             generated overworld floor"
        )));
    }

    // 4. Sample contract: the acceptance samples at the chunk center offset
    //    (8,8); every committed column must carry a real value there (a missing
    //    center would be a vacuous air-vs-air compare). The extractor records
    //    the center like any other column, so a 256-length array guarantees it;
    //    pin the non-vacuous requirement explicitly.
    for (key, fp) in &world.chunks {
        let surface = &fp.surface[CENTER_INDEX];
        if surface == "minecraft:air" {
            return Err(Error::Manifest(format!(
                "generated-expected chunk {key} has an air surface at the chunk-center sample \
                 point (index {CENTER_INDEX}, center offset 8,8) — the acceptance scans \
                 [{BELOW_BEDROCK_Y}..={WORLD_CEILING_Y}] for that column and would observe \
                 terrain, so an all-air center cannot be a genuine terrain column"
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

/// `fixtures/generated-expected/manifest.json`, serialized in the exact committed
/// field order so regeneration is byte-identical (git-clean), mirroring the
/// composed-noise/worldgen manifest convention.
#[derive(serde::Serialize)]
struct GeneratedExpectedManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Write `fixtures/generated-expected/manifest.json` from the freshly generated
/// golden (byte-identical field order).
pub fn regenerate_manifest(dir: &Path) -> Result<(), Error> {
    let data = fs::read(dir.join(FIXTURE_BASENAME))?;
    let manifest = GeneratedExpectedManifest {
        format: 1,
        paper: PINNED_PAPER,
        seed: &PINNED_SEED.to_string(),
        level_type: "minecraft:normal",
        kind: KIND,
        note: "Seed-42 generated-world ground-truth handoff (PR #563): the per-chunk \
               surface/bedrock/below_feet sample contract for the forced spawn grid \
               (-4..=4 in both axes, 81 minecraft:full chunks) captured from the pinned \
               Paper runtime by booting a fresh normal-overworld world and force-generating \
               the grid to FULL. Arrays are 16x16 row-major z*16+x; surface is the highest \
               non-air block, bedrock the block at y=-60, below_feet at y=-61, up to \
               WORLD_CEILING_Y=320; the acceptance samples at the chunk center offset (8,8). \
               Regenerate with `rivet-oracle regenerate --generated-expected` (twin-boot \
               byte-identity proof).",
        captured: vec![CapturedFile {
            path: FIXTURE_BASENAME.to_string(),
            sha256: crate::sha256_hex(&data),
            bytes: data.len(),
        }],
    };
    let mut text = serde_json::to_string_pretty(&manifest).map_err(|e| {
        Error::Manifest(format!("cannot serialize generated-expected manifest: {e}"))
    })?;
    text.push('\n');
    fs::write(dir.join("manifest.json"), text)?;
    Ok(())
}

/// Twin-boot deterministic capture into the committed fixture tree (the
/// `regenerate --generated-expected` path). Requires the two independent Paper
/// captures to produce byte-identical world manifests before anything is
/// committed — a nondeterministic pair is never committed (mirrors
/// `regenerate_m2`).
pub fn run_probe(dir: &Path) -> Result<(), Error> {
    println!("[1/3] forced-grid capture A: fresh seed-42 Paper boot under the 1/1 pin...");
    let a = capture_world(PINNED_SEED)?;
    println!("[2/3] forced-grid capture B: fresh seed-42 Paper boot under the 1/1 pin...");
    let b = capture_world(PINNED_SEED)?;

    if a != b {
        return Err(Error::Gate(
            "generated-expected twin-boot byte-identity check failed — the two independent \
             Paper captures produced DIFFERENT world manifests; refusing to commit a \
             nondeterministic handoff (the seed-42 world generation is not byte-deterministic)"
                .into(),
        ));
    }
    // Validate the (byte-identical) capture against the anti-superflat contract
    // BEFORE committing — two equally-wrong captures (e.g. both a superflat echo
    // from a config or code drift) must be refused, not committed as ground
    // truth.
    validate_world(&a)?;

    println!("[3/3] byte-identical + contract-valid; writing the committed handoff...");
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string(&a)
        .map_err(|e| Error::Gate(format!("serializing generated-expected manifest: {e}")))?;
    fs::write(dir.join(FIXTURE_BASENAME), json.as_bytes())?;
    regenerate_manifest(dir)?;
    println!(
        "regenerated generated-expected seed-42 handoff under {} (twin-boot byte-identical; \
         {} FULL grid chunks)",
        dir.display(),
        a.chunks.len()
    );
    Ok(())
}

/// The tamper negative control: corrupt a committed bit pattern (flip one byte
/// of the golden JSON) and assert the verification FAILS — proving the
/// comparison is not vacuous. Operates on a scratch copy in the temp dir so the
/// committed fixtures are never mutated.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    let scratch = std::env::temp_dir().join(format!(
        "rivet-oracle-generated-expected-{}",
        std::process::id()
    ));
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
    let result = verify_generated_expected(&scratch);
    let _ = fs::remove_dir_all(&scratch);
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "generated-expected tamper was NOT detected — the comparison is vacuous"
                .into(),
        }),
        Err(_) => Ok(()),
    }
}

/// The `generated-expected` subcommand:
///
///   cargo run -p rivet-oracle -- generated-expected <seed>            verify committed fixture
///   cargo run -p rivet-oracle -- generated-expected <seed> --to <out>  capture: boot Paper -> write <out>
///   cargo run -p rivet-oracle -- generated-expected <seed> --tamper    negative control
///
/// Verify mode is pinned to the committed seed-42 handoff (`<seed>` must be 42);
/// the `--to` capture path accepts any seed. `--tamper` and `--to` are mutually
/// exclusive.
///
/// `args` is everything after the subcommand name (the dispatch strips
/// `"generated-expected"` and the program name), so `args` is `[<seed>, ...]`.
pub fn run_cli(args: &[&str]) -> Result<(), Error> {
    let parsed = parse_cli(args)?;
    let dir = crate::crate_dir().join("fixtures/generated-expected");
    if parsed.tamper {
        return tamper_negative_control(&dir);
    }
    if let Some(to) = parsed.to {
        return capture_to(parsed.seed, &to);
    }
    // Verify mode only understands the committed seed-42 handoff. A different
    // seed is a usage error (the committed fixture is pinned to 42), not a
    // silent verify of the wrong reference.
    if parsed.seed != PINNED_SEED {
        return Err(Error::Gate(format!(
            "generated-expected verify is pinned to seed {PINNED_SEED}; got {} — the \
             committed handoff only carries the seed-42 ground truth",
            parsed.seed
        )));
    }
    verify_generated_expected_step(&dir)?;
    println!(
        "PASS: generated-expected seed-42 golden verified (pinned Paper 0a99345 provenance, \
         manifest hash, forced-grid per-chunk sample contract)"
    );
    Ok(())
}

/// Parsed `generated-expected` CLI arguments.
struct CliArgs {
    seed: i64,
    to: Option<PathBuf>,
    tamper: bool,
}

/// Parse the `generated-expected` arguments (everything after the subcommand
/// name). A malformed seed or unknown option is a usage error — `Error::Gate`,
/// never `Error::Unverified`.
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
                        "generated-expected --to requires a destination path".into(),
                    ));
                };
                to = Some(PathBuf::from(path));
                i += 2;
            }
            "--tamper" => {
                tamper = true;
                i += 1;
            }
            other if !other.starts_with('-') => {
                if seed.is_some() {
                    return Err(Error::Gate(
                        "generated-expected takes exactly one seed".into(),
                    ));
                }
                seed = Some(other.parse().map_err(|_| {
                    Error::Gate(format!("generated-expected seed {other} is not an integer"))
                })?);
                i += 1;
            }
            // A negative seed (`-5`) must be read as a seed, not an unknown
            // option — the `--to` capture accepts any i64 seed. A `--`-prefixed
            // token is always an option (unknown ones are refused below); only a
            // single-dash token whose tail is an integer is a negative seed.
            other
                if other.starts_with('-')
                    && !other.starts_with("--")
                    && other[1..].parse::<i64>().is_ok() =>
            {
                if seed.is_some() {
                    return Err(Error::Gate(
                        "generated-expected takes exactly one seed".into(),
                    ));
                }
                seed = Some(other.parse().map_err(|_| {
                    Error::Gate(format!("generated-expected seed {other} is not an integer"))
                })?);
                i += 1;
            }
            other => {
                return Err(Error::Gate(format!(
                    "generated-expected: unknown option {other}"
                )));
            }
        }
    }
    let seed = seed.ok_or_else(|| Error::Gate("generated-expected requires a seed".into()))?;
    if tamper && to.is_some() {
        return Err(Error::Gate(
            "generated-expected --tamper and --to are mutually exclusive".into(),
        ));
    }
    Ok(CliArgs { seed, to, tamper })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaded_world::ChunkFingerprint;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// The committed generated-expected golden is a load-bearing deliverable: a
    /// test that needs it must FAIL when it is absent, never silently return
    /// (D8: never weaken/delete fixtures to go green).
    fn require_fixture(dir: &Path) {
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed generated-expected fixtures {} are ABSENT — the seed-42 handoff \
                 and its gate cannot verify; restore them (git checkout) or this test is red, \
                 never silently skipped",
                dir.display()
            );
        }
    }

    #[test]
    fn grid_coordinates_is_the_81_chunk_spawn_grid() {
        let grid = grid_coordinates();
        assert_eq!(grid.len(), 81);
        assert!(grid.contains(&(0, 0)));
        assert!(grid.contains(&(-4, -4)));
        assert!(grid.contains(&(4, 4)));
        assert!(!grid.contains(&(-5, 0)));
        assert!(!grid.contains(&(0, 5)));
    }

    /// The capture config must isolate the capture boot from the shared oracle
    /// port: the committed `server-normal.properties` serves the M2 gate on
    /// 25599, so a capture that reuses it concurrently collides with the
    /// serialized release gate. `rewrite_properties` pins the seed and sets
    /// `server-port=0` so Paper binds an OS-assigned free port (the headless
    /// capture never addresses it).
    #[test]
    fn capture_properties_rewrite_seed_and_isolate_port() {
        let src = fixtures_dir().join("server-normal.properties");
        let text = fs::read_to_string(&src).unwrap();
        let rewritten = rewrite_properties(&text, 42).unwrap();
        assert!(
            rewritten.contains("level-seed=42\n"),
            "seed not pinned in the capture config"
        );
        assert!(
            !rewritten.contains("level-seed=0\n"),
            "unexpected seed in the capture config"
        );
        assert!(
            rewritten.contains("server-port=0\n"),
            "capture server-port must be 0 (OS-assigned free port)"
        );
        assert!(
            !rewritten.contains("server-port=25599"),
            "the capture must not bind the shared oracle port 25599"
        );
        // The rewrite is the committed config plus exactly the seed/port lines —
        // nothing else drifts.
        assert_eq!(rewritten.lines().count(), text.lines().count());
        assert_eq!(
            rewritten
                .lines()
                .filter(|l| l.starts_with("level-seed="))
                .count(),
            1
        );
        assert_eq!(
            rewritten
                .lines()
                .filter(|l| l.starts_with("server-port="))
                .count(),
            1
        );
    }

    /// A config missing the lines the capture depends on is refused loudly —
    /// the capture config is never silently derived from an unexpected source.
    #[test]
    fn capture_properties_rewrite_refuses_missing_lines() {
        assert!(matches!(
            rewrite_properties("# no seed, no port\n", 42),
            Err(crate::Error::Gate(_))
        ));
        assert!(matches!(
            rewrite_properties("level-seed=42\n", 42),
            Err(crate::Error::Gate(_))
        ));
    }

    #[test]
    fn committed_generated_expected_verifies() {
        let dir = fixtures_dir().join("generated-expected");
        require_fixture(&dir);
        verify_generated_expected(&dir).expect("committed generated-expected golden should verify");
    }

    #[test]
    fn committed_generated_expected_is_non_vacuous() {
        let dir = fixtures_dir().join("generated-expected");
        require_fixture(&dir);
        let world = load(&dir).unwrap();
        assert_eq!(world.format, 1);
        assert_eq!(world.chunks.len(), 81);
        // Every chunk is FULL with a 16×16 row-major contract.
        for (key, fp) in &world.chunks {
            assert_eq!(fp.status, "minecraft:full", "chunk {key}");
            assert!(fp.capability_flags.is_empty(), "chunk {key}");
            assert_eq!(fp.surface.len(), 256, "chunk {key}");
            assert_eq!(fp.bedrock.len(), 256, "chunk {key}");
            assert_eq!(fp.below_feet.len(), 256, "chunk {key}");
        }
        // The pinned grid is covered, including the seed-42 spawn area.
        let keys: BTreeSet<&String> = world.chunks.keys().collect();
        assert!(keys.contains(&"0,0".to_string()));
        assert!(keys.contains(&"-2,0".to_string()));
        assert!(keys.contains(&"4,4".to_string()));
        assert!(keys.contains(&"-4,-4".to_string()));
        // Genuine generated variety, not a superflat echo.
        let distinct: BTreeSet<&str> = world
            .chunks
            .values()
            .flat_map(|fp| fp.distinct.iter().map(String::as_str))
            .collect();
        assert!(distinct.len() >= 3, "only {distinct:?}");
        let patterns: BTreeSet<Vec<String>> =
            world.chunks.values().map(|fp| fp.surface.clone()).collect();
        assert!(patterns.len() >= 2);
        // The chunk-center sample point (8,8) carries real terrain.
        for (key, fp) in &world.chunks {
            assert_ne!(
                fp.surface[CENTER_INDEX], "minecraft:air",
                "chunk {key} center surface"
            );
        }
    }

    /// The default verify path must fail UNVERIFIED when the committed fixture
    /// tree is absent — never silently skip (D8).
    #[test]
    fn missing_fixture_tree_is_unverified() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-ge-missing-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        let result = verify_generated_expected_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    /// A PARTIALLY absent fixture tree (manifest present, golden deleted) is
    /// still UNVERIFIED (exit 3), not a FAIL — the runner must not misclassify a
    /// missing prereq as a comparison failure.
    #[test]
    fn partially_absent_fixture_tree_is_unverified() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-ge-half-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("manifest.json"), b"{}").unwrap();
        let result = verify_generated_expected_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3) for a missing golden, got {result:?}"
        );
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("generated-expected");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    /// Write `world` into a scratch dir under a valid hash-gated manifest and run
    /// the full verify against it. Returns the verify error (the caller asserts
    /// it is a rejection). A valid manifest is essential: a failure must come
    /// from the anti-superflat contract, never from the hash gate.
    fn verify_scratch_world(world: &WorldManifest, tag: &str) -> crate::Error {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-ge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string(&world).unwrap(),
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
        let result = verify_generated_expected(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        result.expect_err("the handoff must be rejected by the anti-superflat contract")
    }

    /// A uniform grid of identical FULL chunks (one surface pattern, one
    /// below-feet pattern, a 3-block distinct set).
    fn uniform_chunks(bedrock_plane: &str) -> WorldManifest {
        let mut chunks = BTreeMap::new();
        for (x, z) in grid_coordinates() {
            let surface = vec!["minecraft:grass_block".to_owned(); 256];
            let bedrock = vec![bedrock_plane.to_owned(); 256];
            let below = vec!["minecraft:dirt".to_owned(); 256];
            chunks.insert(
                format!("{x},{z}"),
                ChunkFingerprint {
                    status: "minecraft:full".to_owned(),
                    stored_pos: [x, z],
                    capability_flags: vec![],
                    distinct: vec![
                        "minecraft:bedrock".to_owned(),
                        "minecraft:dirt".to_owned(),
                        "minecraft:grass_block".to_owned(),
                    ],
                    surface,
                    bedrock,
                    below_feet: below,
                    distinct_state_ids: 3,
                    section_count: 1,
                },
            );
        }
        WorldManifest {
            format: 1,
            overworld_region: "dimensions/minecraft/overworld/region".to_owned(),
            chunks,
        }
    }

    /// A superflat-shaped handoff must be rejected: an all-air bedrock plane at
    /// y=-60, a single repeated surface pattern, and a 3-block distinct set is
    /// the flat-floor echo the generated-world acceptance must never pass
    /// against.
    #[test]
    fn superflat_fixture_is_rejected() {
        let mut chunks = BTreeMap::new();
        for (x, z) in grid_coordinates() {
            let mut surface = vec!["minecraft:grass_block".to_owned(); 256];
            let mut bedrock = vec!["minecraft:air".to_owned(); 256];
            let mut below = vec!["minecraft:dirt".to_owned(); 256];
            // One non-air bedrock entry so the fixture is not degenerate, but
            // the plane is still ~0% non-air — the superflat signature.
            bedrock[0] = "minecraft:bedrock".to_owned();
            surface[136] = "minecraft:grass_block".to_owned();
            below[136] = "minecraft:dirt".to_owned();
            chunks.insert(
                format!("{x},{z}"),
                ChunkFingerprint {
                    status: "minecraft:full".to_owned(),
                    stored_pos: [x, z],
                    capability_flags: vec![],
                    distinct: vec![
                        "minecraft:bedrock".to_owned(),
                        "minecraft:dirt".to_owned(),
                        "minecraft:grass_block".to_owned(),
                    ],
                    surface,
                    bedrock,
                    below_feet: below,
                    distinct_state_ids: 3,
                    section_count: 1,
                },
            );
        }
        let world = WorldManifest {
            format: 1,
            overworld_region: "dimensions/minecraft/overworld/region".to_owned(),
            chunks,
        };
        let err = verify_scratch_world(&world, "superflat");
        let msg = err.to_string();
        assert!(
            msg.contains("bedrock") || msg.contains("superflat"),
            "unexpected error: {msg}"
        );
    }

    /// A uniform grid with a fully non-air bedrock plane (so the bedrock-plane
    /// density check alone would pass) must still be rejected: the repeated
    /// surface pattern and uniform below-feet array are the superflat signature
    /// the generated-world acceptance must never pass against. This proves the
    /// anti-superflat contract does not rest on the bedrock-plane check alone.
    #[test]
    fn uniform_bedrock_floor_is_rejected() {
        let err = verify_scratch_world(&uniform_chunks("minecraft:bedrock"), "bedrock-floor");
        let msg = err.to_string();
        assert!(
            msg.contains("surface array") || msg.contains("below_feet array"),
            "unexpected error: {msg}"
        );
    }

    /// A uniform grid whose surface arrays vary per chunk (fake variety) but
    /// whose below_feet arrays are uniform must still be rejected — the
    /// below-feet (y=-61) depth evidence is an independent anti-superflat
    /// dimension.
    #[test]
    fn uniform_below_floor_is_rejected() {
        let mut world = uniform_chunks("minecraft:bedrock");
        for (i, (_, fp)) in world.chunks.iter_mut().enumerate() {
            let variant = if i % 2 == 0 {
                "minecraft:grass_block"
            } else {
                "minecraft:sand"
            };
            fp.surface = vec![variant.to_owned(); 256];
        }
        let err = verify_scratch_world(&world, "below-floor");
        let msg = err.to_string();
        assert!(msg.contains("below_feet array"), "unexpected error: {msg}");
    }

    /// A relabeled chunk — one whose internal `stored_pos` does not match its
    /// grid key — must be rejected even when every other chunk is the genuine
    /// committed golden. The stored_pos/key agreement is what stops a chunk
    /// captured elsewhere from being relabeled into a real grid slot, so it must
    /// be covered by its own negative test (regression: the check could silently
    /// rot without one).
    #[test]
    fn relabeled_chunk_is_rejected() {
        let dir = fixtures_dir().join("generated-expected");
        require_fixture(&dir);
        let mut world = load(&dir).unwrap();
        // Relabel the genuine (0,0) chunk as if it were (1,0): the chunk's
        // internal xPos/zPos no longer match its grid key.
        let fp = world.chunks.get_mut("0,0").expect("grid chunk 0,0");
        fp.stored_pos = [1, 0];
        let err = verify_scratch_world(&world, "relabeled");
        let msg = err.to_string();
        assert!(
            msg.contains("stored_pos") && msg.contains("relabeled"),
            "unexpected error: {msg}"
        );
    }

    /// The capture's provenance pin is the commit part of PINNED_PAPER — the
    /// committed handoff is only honest if the booted server jar is at 0a99345
    /// (this is what `check_capture_pin` enforces against the materialized jar).
    #[test]
    fn pinned_paper_commit_is_the_capture_provenance_pin() {
        assert_eq!(
            crate::parse_paper_pin(Some(PINNED_PAPER)).as_deref(),
            Some("0a99345")
        );
    }

    /// Regenerating the manifest in Rust is byte-identical to the committed
    /// manifest (given an unchanged golden) — regeneration is git-clean.
    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("generated-expected");
        require_fixture(&dir);
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-ge-regen-{}", std::process::id()));
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
            "regenerating the generated-expected manifest must be byte-identical (git-clean)"
        );
        crate::verify_fixtures(&scratch).unwrap();
        let _ = fs::remove_dir_all(&scratch);
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

        // Negative seeds are valid for the --to capture path (a leading `-` must
        // not be mistaken for an unknown option).
        let parsed = parse_cli(&["-5", "--to", "/tmp/out.json"]).unwrap();
        assert_eq!(parsed.seed, -5);
        assert_eq!(parsed.to, Some(PathBuf::from("/tmp/out.json")));
    }

    #[test]
    fn cli_rejects_malformed_args_as_gate() {
        // No seed -> Gate (usage), never Unverified.
        assert!(matches!(parse_cli(&[]), Err(crate::Error::Gate(_))));
        // Non-integer seed -> Gate.
        assert!(matches!(parse_cli(&["abc"]), Err(crate::Error::Gate(_))));
        // Two seeds -> Gate.
        assert!(matches!(
            parse_cli(&["42", "43"]),
            Err(crate::Error::Gate(_))
        ));
        // Missing --to value -> Gate.
        assert!(matches!(
            parse_cli(&["42", "--to"]),
            Err(crate::Error::Gate(_))
        ));
        // Unknown option -> Gate.
        assert!(matches!(
            parse_cli(&["42", "--bogus"]),
            Err(crate::Error::Gate(_))
        ));
        // A `--`-prefixed token is never a negative seed — unknown option -> Gate.
        assert!(matches!(parse_cli(&["--5"]), Err(crate::Error::Gate(_))));
        // --tamper and --to together are refused (mutually exclusive modes).
        assert!(matches!(
            parse_cli(&["42", "--tamper", "--to", "/tmp/x.json"]),
            Err(crate::Error::Gate(_))
        ));
    }

    /// Verify mode is pinned to seed 42: a different seed is a usage error, not
    /// a silent verify of the wrong reference. (The --to capture path still
    /// accepts any seed.)
    #[test]
    fn verify_mode_rejects_non_42_seed() {
        // run_cli's verify branch refuses a non-42 seed without touching the
        // committed fixture.
        let err = run_cli(&["999"]).expect_err("verify must refuse a non-42 seed");
        assert!(matches!(err, crate::Error::Gate(_)));
        let msg = err.to_string();
        assert!(msg.contains("seed 42"), "unexpected error: {msg}");
    }
}
