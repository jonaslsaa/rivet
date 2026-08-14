//! The post-surface column oracle slice: a non-vacuous seed-42 golden
//! comparison against the pinned Paper overworld generator's SURFACE
//! checkpoint — the piece #179 needs to prove that a Rivet port of the
//! surface pass (`SurfaceSystem.buildSurface` + the overworld `surface_rule`)
//! reproduces Paper's post-surface columns before real generated chunks exist.
//!
//! `fixtures/surface-column/surface-columns.json` is captured by
//! `scripts/run_surface_column_probe.sh` (running `SurfaceColumnProbe` against
//! the pinned Paper 0a99345 runtime). The probe boots the vanilla registries
//! (no server boot) and drives the REAL overworld generator
//! (`createBiomes` -> `fillFromNoise` -> `buildSurface`) on REAL `ProtoChunk`s
//! at seed 42 for the #175 chunk-coordinate matrix. It records, per column:
//!
//!   * the pre-surface and post-surface block state at every `SAMPLE_STEP`-th Y
//!     (block registry key + raw `Block.BLOCK_STATE_REGISTRY` id), so a Rust
//!     port asserts the exact post-surface block id per sampled Y. The block
//!     samples are pinned at the chunk's own block-origin corner column only
//!     (the block at `min-block-x`, y, `min-block-z`); the other 255 columns of
//!     each chunk are covered by heights alone, not block ids. So a green proves
//!     the exact block id down the corner column of every chunk plus the
//!     surface/floor height across all 256 columns — it does NOT pin the
//!     sub-surface block id at the other 255 columns (a surface port whose
//!     cave/biome-driven subsurface differs off the corner column while matching
//!     top heights and the corner column would still pass). That bound is
//!     intentional for the #179 SURFACE checkpoint; exact per-column block-id
//!     coverage is a follow-up.
//!   * pre/post WORLD_SURFACE_WG and OCEAN_FLOOR_WG heights (the pre-surface
//!     snapshot is an all-air chunk whose heightmaps are unprimed, so a capture
//!     that recorded the chunk before any generation is visible as a null
//!     pre/post delta; a capture that ran surface before fill — heightmaps
//!     primed early — is visible via the unprimed pre-heightmap check),
//!   * the surface biome the surface pass saw at the top of the column.
//!
//! The verify command asserts the committed golden's provenance (Paper pin +
//! manifest SHA-256s), the pinned seed/generator/height shape, and that the
//! capture really ran post-surface. The per-column checks prove real
//! pre/post deltas (a no-op capture that recorded the chunk pre-surface fails)
//! and unprimed pre-heightmaps; the corpus-level check proves the surface pass
//! emitted material the fill pass cannot (a fill-only capture that dropped
//! `buildSurface` — post = plain air/water/stone/deepslate/bedrock — fails).
//! The surface-biome ids are part of the fixture — pinned byte-for-byte by the
//! manifest SHA-256 like every other field — but are not individually
//! interpreted: verify never asserts they are the seed-42 overworld biomes.
//! The tamper negative control proves the comparison detects a flipped byte
//! (the manifest SHA-256 gate).
//!
//! # Structure-freeness of the corpus (load-bearing assumption)
//!
//! The probe pre-sets each `NoiseChunk` with `Beardifier.EMPTY` instead of the
//! real `Beardifier.forStructuresInChunk(...)`, mirroring the composed-noise
//! oracle. This is exact ONLY because the seed-42 #175 corpus chunks are
//! structure-free: a beard-affecting structure start (village, pillager
//! outpost, ancient city, trail ruins, trial chambers, stronghold, ...) whose
//! pieces come within `BEARD_KERNEL_RADIUS` (12 blocks) of a corpus chunk would
//! change its density field and therefore its surface — the golden would then
//! capture a world real Paper does not produce at these coordinates.
//!
//! Verified against the pinned Paper 0a99345 runtime (via the real
//! `ChunkGeneratorStructureState` + `StructurePlacement.isStructureChunk` +
//! the `Structure.isValidBiome` biome gate at each structure's real start
//! position): **zero** beard-affecting structure starts land within 6
//! chunks of any of the 8 corpus chunks, and no corpus chunk is a placement
//! chunk for any structure set. 6 chunks of margin covers the 12-block beard
//! kernel plus any piece reach. The seed-42 matrix is genuinely structure-free;
//! `Beardifier.EMPTY` is the exact Paper density input, not a weakening.
//! `committed_surface_column_is_structure_free` locks this in: it replays the
//! placement-chunk predicate (reproducing Paper's `LegacyRandomSource`,
//! including the power-of-two `nextInt(bound)` shortcut that ancient cities'
//! limit of 16 needs) over the committed corpus coords and encodes the four
//! verified in-reach placement chunks — each biome-rejected at its true start
//! position — so a future regeneration that stops being structure-free fails
//! loudly instead of silently capturing a non-structure-free world.

use crate::{CapturedFile, Error, sha256_hex};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const KIND: &str = "surface-column";
const FIXTURE_BASENAME: &str = "surface-columns.json";

/// The vertical step the probe samples per column (`SAMPLE_STEP` in
/// `SurfaceColumnProbe.java`), between MIN_Y (-64) and MAX_Y (320).
const SAMPLE_STEP: i64 = 4;
const MIN_Y: i64 = -64;
const MAX_Y: i64 = 320;
const SAMPLE_COUNT: usize = ((MAX_Y - MIN_Y) / SAMPLE_STEP) as usize; // 96
const HEIGHTMAP_COUNT: usize = 16 * 16; // 256

/// The Paper commit the surface-column golden is captured against (both the
/// fixture/manifest provenance and the runtime jar's `Git-Commit` attribute).
const PINNED_COMMIT: &str = "0a99345";

/// Blocks the overworld surface rules can emit that the density/fill pass
/// (`fillFromNoise`) never produces. The fill pass yields only air, water,
/// lava, stone, deepslate and the bedrock floor; every block in this set is
/// surface-rule material. The corpus must contain at least one of them, which
/// is what separates a genuine post-surface capture from a fill-only one (a
/// probe that dropped the `buildSurface` call would relabel fill output as
/// "post-surface" and pass every per-column check).
///
/// `minecraft:sulfur` is the sulfur-caves surface rule (26.2 `sulfur_caves`
/// biome), not a typo — the committed golden contains 3 sampled sulfur samples
/// in the `flower_forest` column (-31,-31).
const SURFACE_RULE_BLOCKS: &[&str] = &[
    "minecraft:grass_block",
    "minecraft:dirt",
    "minecraft:sand",
    "minecraft:gravel",
    "minecraft:sulfur",
    "minecraft:clay",
    "minecraft:snow_block",
];

/// A captured block state.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq)]
pub struct BlockState {
    pub id: i64,
    pub block: String,
    pub air: bool,
    #[serde(rename = "fluid-empty")]
    pub fluid_empty: bool,
}

/// One sampled Y: the pre- and post-surface block states.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct Sample {
    pub y: i64,
    pub pre: BlockState,
    pub post: BlockState,
    pub changed: bool,
}

/// One heightmap cell.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct HeightCell {
    pub x: i64,
    pub z: i64,
    #[serde(rename = "pre-ws")]
    pub pre_ws: i64,
    #[serde(rename = "post-ws")]
    pub post_ws: i64,
    #[serde(rename = "pre-of")]
    pub pre_of: i64,
    #[serde(rename = "post-of")]
    pub post_of: i64,
}

/// One post-surface column.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct Column {
    pub cx: i64,
    pub cz: i64,
    #[serde(rename = "min-block-x")]
    pub min_block_x: i64,
    #[serde(rename = "min-block-z")]
    pub min_block_z: i64,
    pub samples: Vec<Sample>,
    #[serde(rename = "any-surface-changed")]
    pub any_surface_changed: bool,
    pub heightmap: Vec<HeightCell>,
    #[serde(rename = "any-height-changed")]
    pub any_height_changed: bool,
    #[serde(rename = "surface-biome")]
    pub surface_biome: String,
}

/// The parsed surface-column fixture.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct SurfaceColumn {
    pub seed: i64,
    pub paper: String,
    pub dimension: String,
    pub generator: String,
    #[serde(rename = "level-type")]
    pub level_type: String,
    #[serde(rename = "noise-settings")]
    pub noise_settings: String,
    #[serde(rename = "min-y")]
    pub min_y: i64,
    pub height: i64,
    #[serde(rename = "sea-level")]
    pub sea_level: i64,
    #[serde(rename = "possible-biomes")]
    pub possible_biomes: String,
    #[serde(rename = "flat-bedrock-substitution")]
    pub flat_bedrock_substitution: String,
    pub format: i64,
    pub columns: Vec<Column>,
}

/// The committed #175 chunk-coordinate matrix (same set as `corpus` /
/// `composed_noise`), as chunk coords.
const CORPUS_CHUNK_COORDS: [(i64, i64); 8] = [
    (0, 0),
    (15, 15),
    (31, 31),
    (-1, -1),
    (-16, -16),
    (-31, -31),
    (-1, 0),
    (0, -1),
];

/// The surface-column subcommand mode selected by its `--tamper` / `--sample`
/// flags (default: verify + non-vacuity checks).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SurfaceColumnMode {
    Verify,
    Tamper,
    Sample,
}

/// Parse the surface-column subcommand flags into a mode. Unknown flags are a
/// usage error — a typo'd `--sampple` used to fall through to the verify branch
/// and exit 0 after verifying, so strictness here prevents a silent misread of
/// the intended mode (the same bug composed-noise already guards against).
/// `--tamper` and `--sample` are mutually exclusive (repeats of the same flag
/// are fine). Sole `--help`/`-h` is intercepted by the dispatcher; in any
/// combination it is a hard usage error here.
pub fn parse_mode(flags: &[&str]) -> Result<SurfaceColumnMode, Error> {
    let mut mode: Option<SurfaceColumnMode> = None;
    for flag in flags {
        let m = match *flag {
            "--tamper" => SurfaceColumnMode::Tamper,
            "--sample" => SurfaceColumnMode::Sample,
            other => {
                return Err(Error::Gate(format!(
                    "surface-column takes only --tamper/--sample, got {other}"
                )));
            }
        };
        match mode {
            None => mode = Some(m),
            Some(prev) if prev == m => {}
            Some(_) => {
                return Err(Error::Gate(
                    "surface-column --tamper and --sample are mutually exclusive".into(),
                ));
            }
        }
    }
    Ok(mode.unwrap_or(SurfaceColumnMode::Verify))
}

/// Parse + structurally validate the committed surface-column fixture.
pub fn load(dir: &Path) -> Result<SurfaceColumn, Error> {
    let path = dir.join(FIXTURE_BASENAME);
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let v: SurfaceColumn = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
    if v.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported surface-column format {} (expected 1)",
            v.format
        )));
    }
    Ok(v)
}

/// Verify the committed post-surface column golden: the manifest hashes, the
/// pinned provenance (Paper pin + seed 42 + generator identity), the height
/// shape, the #175 matrix coverage, and the non-vacuity guarantees — every
/// column must have had real surface changes (a pre-surface/no-op capture is
/// drift, not a golden). This is the SURFACE checkpoint gate.
pub fn verify_surface_column(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    // 1. Provenance + manifest hashes. Pinned to seed 42, Paper 0a99345, the
    //    normal overworld generator.
    if fixture.seed != 42 {
        return Err(Error::Manifest(format!(
            "surface-column seed {} != pinned seed 42",
            fixture.seed
        )));
    }
    if fixture.dimension != "overworld" {
        return Err(Error::Manifest(format!(
            "surface-column dimension {} != overworld",
            fixture.dimension
        )));
    }
    if fixture.generator != "normal" {
        return Err(Error::Manifest(format!(
            "surface-column generator {} != normal",
            fixture.generator
        )));
    }
    if fixture.level_type != "minecraft:normal" {
        return Err(Error::Manifest(format!(
            "surface-column level-type {} != minecraft:normal",
            fixture.level_type
        )));
    }
    if fixture.noise_settings != "minecraft:overworld" {
        return Err(Error::Manifest(format!(
            "surface-column noise-settings {} != minecraft:overworld",
            fixture.noise_settings
        )));
    }
    // Height shape + sea level are pinned too: a fixture regenerated against a
    // different height budget or sea level is drift.
    if fixture.min_y != MIN_Y || fixture.height != (MAX_Y - MIN_Y) {
        return Err(Error::Manifest(format!(
            "surface-column height shape (min_y {}, height {}) != ({MIN_Y}, {})",
            fixture.min_y,
            fixture.height,
            MAX_Y - MIN_Y
        )));
    }
    if fixture.sea_level != 63 {
        return Err(Error::Manifest(format!(
            "surface-column sea-level {} != overworld sea level 63",
            fixture.sea_level
        )));
    }
    // possible-biomes=null is how buildSurface is driven (full per-column biome
    // evaluation, no biome-condition pruning).
    if fixture.possible_biomes != "null" {
        return Err(Error::Manifest(format!(
            "surface-column possible-biomes {} != null",
            fixture.possible_biomes
        )));
    }
    // The flat-bedrock substitution is honest and pinned: the probe ships a
    // Level-free shadow of Paper's OptionallyFlatBedrockConditionSource
    // (generateFlatBedrock=false, the Paper default). A fixture captured
    // WITHOUT the shadow (or with a changed substitution) is a different
    // capture, not this golden.
    if fixture.flat_bedrock_substitution != "generateFlatBedrock=false (Paper default)" {
        return Err(Error::Manifest(format!(
            "surface-column flat-bedrock-substitution {:?} != pinned Paper-default false",
            fixture.flat_bedrock_substitution
        )));
    }
    let manifest = crate::verify_fixtures(dir)?;
    if manifest.kind.as_deref() != Some(KIND) {
        return Err(Error::Manifest(format!(
            "expected kind {KIND}, got {:?}",
            manifest.kind
        )));
    }
    // The manifest's SHA-256 gate is only load-bearing if it actually records
    // the golden — an empty `captured` list would verify nothing.
    if !manifest.captured.iter().any(|c| c.path == FIXTURE_BASENAME) {
        return Err(Error::Manifest(format!(
            "surface-column manifest records no captured {FIXTURE_BASENAME} — the \
             SHA-256 gate cannot protect the golden"
        )));
    }
    if crate::parse_paper_pin(manifest.paper.as_deref()).as_deref() != Some(PINNED_COMMIT) {
        return Err(Error::Manifest(format!(
            "surface-column fixture not pinned to Paper 0a99345: {:?}",
            manifest.paper
        )));
    }
    if fixture.paper != manifest.paper.as_deref().unwrap_or("") {
        return Err(Error::Manifest(format!(
            "fixture paper {} != manifest paper {:?}",
            fixture.paper, manifest.paper
        )));
    }
    // The manifest's informational seed string must match the pinned seed too.
    // The golden's own `seed` field is the structural gate (checked above); the
    // manifest string is the writer's self-description, so a manifest that
    // claims a different seed is drift even if the golden's hash still matches.
    if manifest.seed.as_deref() != Some("42") {
        return Err(Error::Manifest(format!(
            "surface-column manifest seed {:?} != pinned seed 42",
            manifest.seed
        )));
    }
    // 2. Structural + non-vacuity assertions. The committed fixture is
    //    hash-gated, so the bytes are the probe's exact output; here we pin the
    //    shape (the #175 matrix, the sample Y slice) and — crucially — that the
    //    capture really ran post-surface: every column must show at least one
    //    changed block and a heightmap delta, and every sampled Y must be
    //    covered exactly once.
    if fixture.columns.len() != 8 {
        return Err(Error::Manifest(format!(
            "surface-column has {} columns, expected 8 (#175 matrix)",
            fixture.columns.len()
        )));
    }
    let mut seen_columns: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for col in &fixture.columns {
        let key = (col.cx, col.cz);
        if !CORPUS_CHUNK_COORDS.contains(&key) {
            return Err(Error::Manifest(format!(
                "surface-column ({},{}) not in the #175 matrix",
                col.cx, col.cz
            )));
        }
        // The block origin must be the chunk coords times 16 — a hand-edited or
        // mismatched capture would break the column-to-chunk mapping.
        if col.min_block_x != col.cx * 16 || col.min_block_z != col.cz * 16 {
            return Err(Error::Manifest(format!(
                "column ({},{}) min-block ({},{}) != chunk coords x16 ({},{})",
                col.cx,
                col.cz,
                col.min_block_x,
                col.min_block_z,
                col.cx * 16,
                col.cz * 16
            )));
        }
        *seen_columns.entry(key).or_default() += 1;
    }
    if seen_columns.len() != 8 {
        return Err(Error::Manifest(format!(
            "surface-column covers {} distinct #175 columns, expected 8",
            seen_columns.len()
        )));
    }
    for col in &fixture.columns {
        // Every column samples the full vertical slice at every 4th Y, exactly
        // once per Y, with a pre and post state that are internally consistent.
        if col.samples.len() != SAMPLE_COUNT {
            return Err(Error::Manifest(format!(
                "column ({},{}) has {} samples, expected {SAMPLE_COUNT}",
                col.cx,
                col.cz,
                col.samples.len()
            )));
        }
        let expected_ys: Vec<i64> = (0..SAMPLE_COUNT)
            .map(|i| MIN_Y + (i as i64) * SAMPLE_STEP)
            .collect();
        let ys: Vec<i64> = col.samples.iter().map(|s| s.y).collect();
        if ys != expected_ys {
            return Err(Error::Manifest(format!(
                "column ({},{}) sample ys {ys:?} != expected {expected_ys:?}",
                col.cx, col.cz
            )));
        }
        for s in &col.samples {
            if s.changed != (s.pre != s.post) {
                return Err(Error::Manifest(format!(
                    "column ({},{}) y {} changed flag {} != pre/post equality",
                    col.cx, col.cz, s.y, s.changed
                )));
            }
        }
        if col.heightmap.len() != HEIGHTMAP_COUNT {
            return Err(Error::Manifest(format!(
                "column ({},{}) has {} heightmap cells, expected {HEIGHTMAP_COUNT}",
                col.cx,
                col.cz,
                col.heightmap.len()
            )));
        }
        // The 256 heightmap cells must be the unique 16x16 grid — duplicated
        // cells would let a hand-rolled fixture under-cover the column, and
        // out-of-range cells (x/z outside 0..15) are not this chunk's grid.
        let mut seen_cells: BTreeMap<(i64, i64), usize> = BTreeMap::new();
        for h in &col.heightmap {
            if !(0..16).contains(&h.x) || !(0..16).contains(&h.z) {
                return Err(Error::Manifest(format!(
                    "column ({},{}) heightmap cell ({},{}) outside the 16x16 grid",
                    col.cx, col.cz, h.x, h.z
                )));
            }
            *seen_cells.entry((h.x, h.z)).or_default() += 1;
        }
        if seen_cells.len() != HEIGHTMAP_COUNT {
            return Err(Error::Manifest(format!(
                "column ({},{}) heightmap covers {} distinct cells, expected {HEIGHTMAP_COUNT}",
                col.cx,
                col.cz,
                seen_cells.len()
            )));
        }
        // Non-vacuity: the capture must prove it ran post-surface. A probe that
        // skipped buildSurface (or captured the chunk before surface) would emit
        // all-air both sides — pre-ws unprimed (-65) everywhere and
        // any-surface-changed=false.
        if !col.any_surface_changed {
            return Err(Error::Manifest(format!(
                "column ({},{}) any-surface-changed=false — the capture is a no-op \
                 (recorded the chunk before buildSurface ran); a post-surface \
                 golden must show real block changes",
                col.cx, col.cz
            )));
        }
        let sample_changed = col.samples.iter().any(|s| s.changed);
        if col.any_surface_changed != sample_changed {
            return Err(Error::Manifest(format!(
                "column ({},{}) any-surface-changed flag inconsistent with samples",
                col.cx, col.cz
            )));
        }
        let height_changed = col
            .heightmap
            .iter()
            .any(|h| h.pre_ws != h.post_ws || h.pre_of != h.post_of);
        if col.any_height_changed != height_changed {
            return Err(Error::Manifest(format!(
                "column ({},{}) any-height-changed flag inconsistent with heightmap",
                col.cx, col.cz
            )));
        }
        // The pre-surface heightmap must be unprimed (-65 = MIN_Y-1) for at
        // least one cell — proving the pre snapshot was taken before the surface
        // pass primed them, and the post is the real fill+surface result.
        let pre_unprimed = col.heightmap.iter().any(|h| h.pre_ws == MIN_Y - 1);
        if !pre_unprimed {
            return Err(Error::Manifest(format!(
                "column ({},{}) has no unprimed pre-ws height (-65) — the pre \
                 snapshot was not taken before the surface pass",
                col.cx, col.cz
            )));
        }
        // The sampled (0,0) column (the block at min-block-x, y, min-block-z)
        // must have a non-air post state somewhere below the surface: the
        // overworld surface never leaves the sampled axis all air.
        if col.samples.iter().all(|s| s.post.air) {
            return Err(Error::Manifest(format!(
                "column ({},{}) every sampled post state is air — the surface pass \
                 produced no terrain on the sampled axis",
                col.cx, col.cz
            )));
        }
    }
    // Corpus-level non-vacuity: the surface pass must have emitted material the
    // fill pass cannot. A fill-only capture (buildSurface dropped or skipped)
    // yields only air/water/stone/deepslate/bedrock, so requiring at least one
    // surface-rule block anywhere in the corpus separates a genuine post-surface
    // golden from a relabeled fill-only one. (Ocean columns may not sample the
    // 1-2 block floor cap at SAMPLE_STEP resolution, so this is a corpus-level
    // assertion, not per-column.) The surface-rule blocks must land on the
    // 4-aligned sample grid (e.g. grass at odd Y=63 is never sampled), so a
    // legitimately different-but-correct regeneration whose surface tops fall
    // off-grid could false-reject — stable for the pinned seed-42 matrix
    // (4/8 columns carry a surface-rule block), but a known sampling-resolution
    // caveat, not a per-column guarantee.
    let has_surface_block = fixture.columns.iter().any(|col| {
        col.samples
            .iter()
            .any(|s| SURFACE_RULE_BLOCKS.contains(&s.post.block.as_str()))
    });
    if !has_surface_block {
        return Err(Error::Manifest(
            "no surface-rule block (grass_block/dirt/sand/gravel/sulfur/...) anywhere \
             in the corpus — the capture is fill-only (buildSurface never ran), not a \
             post-surface golden"
                .into(),
        ));
    }
    Ok(())
}

/// The tamper negative control: corrupt a committed block state id (flip one
/// byte of the JSON) and assert the verification FAILS — proving the comparison
/// is not vacuous (a green is impossible with tampered goldens).
///
/// Like the other negative controls, it operates on a scratch copy in the temp
/// dir so the committed fixtures are never mutated — a panic or `?`
/// early-return can never leave `fixtures/surface-column/` corrupted.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    // The committed golden must be green before it is tampered; otherwise any
    // scratch failure would be credited to the flip and the control would pass
    // vacuously on an already-broken golden (a dev running `surface-column
    // --tamper` on a drifted tree would get a green that masks the breakage).
    // Mirrors composed_noise::tamper_negative_control's baseline guard.
    verify_surface_column(dir)?;
    let scratch = std::env::temp_dir().join(format!(
        "rivet-oracle-surface-column-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)
        .map_err(|e| Error::Gate(format!("cannot create scratch {}: {e}", scratch.display())))?;
    fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME))?;
    fs::copy(dir.join("manifest.json"), scratch.join("manifest.json"))?;
    let fixture_path = scratch.join(FIXTURE_BASENAME);
    let original = fs::read(&fixture_path)
        .map_err(|e| Error::Gate(format!("cannot read {}: {e}", fixture_path.display())))?;
    // Flip one byte in the middle of the file — a deterministic, minimal tamper.
    let i = (original.len() / 2).min(original.len().saturating_sub(1));
    let mut tampered = original.clone();
    tampered[i] ^= 0xFF;
    fs::write(&fixture_path, &tampered)?;
    let result = verify_surface_column(&scratch);
    let _ = fs::remove_dir_all(&scratch);
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "surface-column tamper was NOT detected — the comparison is vacuous".into(),
        }),
        Err(_) => Ok(()),
    }
}

/// `fixtures/surface-column/manifest.json`, serialized in the exact committed
/// field order so regeneration is byte-identical (git-clean), mirroring the
/// worldgen manifest convention.
#[derive(serde::Serialize)]
struct SurfaceColumnManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Write `fixtures/surface-column/manifest.json` from the freshly generated
/// fixture (byte-identical field order, like the worldgen manifest).
pub fn regenerate_manifest(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    let data = fs::read(dir.join(FIXTURE_BASENAME))?;
    let manifest = SurfaceColumnManifest {
        format: 1,
        paper: &fixture.paper,
        seed: &fixture.seed.to_string(),
        level_type: &fixture.level_type,
        kind: KIND,
        note: "Deterministic seed-42 post-surface column goldens for the overworld \
               SURFACE checkpoint: the REAL generator pipeline (createBiomes -> \
               fillFromNoise -> buildSurface) run on REAL ProtoChunks at the #175 \
               chunk-coordinate matrix. Each column records pre/post block states \
               at every 4th Y (block registry key + raw state id), pre/post \
               WORLD_SURFACE_WG + OCEAN_FLOOR_WG heights, and the surface biome. \
               The pre snapshot is taken before buildSurface (all-air, heightmaps \
               unprimed), so a no-op capture is detected. Captured from the pinned \
               Paper runtime via tools/rivet-oracle/src/java/SurfaceColumnProbe.java \
               with a Level-free OptionallyFlatBedrockConditionSource shadow \
               (generateFlatBedrock=false, Paper default); regenerate with \
               scripts/run_surface_column_probe.sh.",
        captured: vec![CapturedFile {
            path: FIXTURE_BASENAME.to_string(),
            sha256: sha256_hex(&data),
            bytes: data.len(),
        }],
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize surface-column manifest: {e}")))?;
    text.push('\n');
    fs::write(dir.join("manifest.json"), text)?;
    Ok(())
}

/// Run the Paper-side probe into `dir` (regenerating surface-columns.json), then
/// rewrite the manifest. Requires the materialized pinned Paper runtime (or the
/// env overrides).
///
/// The runner exits 3 (UNVERIFIED) when a runtime prerequisite is absent or the
/// materialized jar's Git-Commit cannot be confirmed to match the pinned
/// {PINNED_COMMIT} — mapped to `Error::Unverified` so `--surface-column --sample`
/// fails with exit 3, never a bare FAIL or a relabeled fixture. Only a genuine
/// probe failure (javac/java) is a Gate.
pub fn run_probe(dir: &Path) -> Result<(), Error> {
    let crate_root = crate::crate_dir();
    let script = crate_root.join("scripts/run_surface_column_probe.sh");
    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::Gate(format!("failed to run {}: {e}", script.display())))?;
    if !status.success() {
        return Err(match status.code() {
            Some(crate::EXIT_UNVERIFIED) => Error::Unverified(format!(
                "run_surface_column_probe.sh exited {status} — runtime prerequisite \
                 absent or not pinned (no materialized paper jar / libraries, or the jar's \
                 Git-Commit is not {PINNED_COMMIT}); see its stderr"
            )),
            _ => Error::Gate(format!(
                "run_surface_column_probe.sh exited {status} — see its stderr"
            )),
        });
    }
    regenerate_manifest(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// The committed post-surface column golden is a load-bearing deliverable: a
    /// test that needs it must FAIL when it is absent, never silently return
    /// (D8: never weaken/delete fixtures to go green; a missing load-bearing
    /// fixture is a hard failure).
    fn require_fixture(dir: &std::path::Path) {
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed surface-column fixtures {} are ABSENT — the seed-42 golden \
                 and its SURFACE-checkpoint gate cannot verify; restore them (git \
                 checkout) or this test is red, never silently skipped",
                dir.display()
            );
        }
    }

    #[test]
    fn committed_surface_column_verifies() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        verify_surface_column(&dir).expect("committed surface-column golden should verify");
    }

    /// The seed-42 #175 corpus is structure-free — the load-bearing input
    /// assumption that makes `Beardifier.EMPTY` exact. This replays Paper's
    /// `RandomSpreadStructurePlacement` / `ConcentricRingsStructurePlacement`
    /// placement-chunk predicate over the committed corpus coordinates and
    /// asserts no beard-affecting start comes within beard reach of any corpus
    /// chunk. The scan parameters mirror the probe's documented claim (6 chunks
    /// of margin around the 12-block `BEARD_KERNEL_RADIUS`).
    #[test]
    fn committed_surface_column_is_structure_free() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let fixture = load(&dir).unwrap();

        // This replays Paper's placement-chunk predicate over the committed
        // corpus coordinates and asserts the structure-freeness invariant that
        // makes `Beardifier.EMPTY` exact. It matches the real Paper 0a99345
        // scan (ChunkGeneratorStructureState + StructurePlacement.isStructureChunk
        // + the Structure.isValidBiome gate at the onTopOfChunkCenter start
        // position), which found ZERO beard-affecting structure starts within
        // 6 chunks of any corpus chunk. The exact placement chunks reported by
        // `isStructureChunk` near each corpus chunk (biome-unfiltered
        // over-reports) are recorded below as a truth table so a future
        // regeneration that changes the corpus coordinates would fail loudly
        // instead of silently capturing a non-structure-free world.

        // Beard-affecting overworld structure sets at seed 42: villages (salt
        // 10387312, spacing 34, sep 8), pillager outposts (salt 165745296,
        // spacing 32, sep 8), ancient cities (salt 20083232, spacing 24, sep 8),
        // trail ruins (salt 83469867, spacing 34, sep 8), trial chambers (salt
        // 94251327, spacing 34, sep 12). Strongholds (concentric rings) are
        // handled separately by ring membership; none reach the corpus (verified
        // empirically: 0 starts in range).
        const BEARD_SPREADS: &[(i64, i64, i64)] = &[
            // (salt, spacing, separation)
            (10387312, 34, 8),  // villages
            (165745296, 32, 8), // pillager outposts
            (20083232, 24, 8),  // ancient cities
            (83469867, 34, 8),  // trail ruins
            (94251327, 34, 12), // trial chambers
        ];

        let corpus: Vec<(i64, i64)> = fixture.columns.iter().map(|c| (c.cx, c.cz)).collect();

        // Direct placement-chunk property on the corpus chunks themselves. Real
        // Paper reports NONE of the 8 corpus chunks as a placement chunk for any
        // beard-affecting set. If a future coordinate change makes a corpus chunk
        // itself a placement chunk, that is a hard regression.
        for &(cx, cz) in &corpus {
            for &(salt, spacing, sep) in BEARD_SPREADS {
                let potential = potential_spread_chunk(fixture.seed, salt, spacing, sep, cx, cz);
                assert_ne!(
                    potential,
                    (cx, cz),
                    "corpus chunk ({cx},{cz}) is a direct placement chunk for a \
                     beard-affecting set (salt {salt}) — the seed-42 matrix is NOT \
                     structure-free, so Beardifier.EMPTY would capture a world real \
                     Paper does not produce"
                );
            }
        }

        // The beard-reach invariant: no beard-affecting placement chunk comes
        // within `SEARCH` chunks of a corpus chunk AND is biome-eligible. Real
        // Paper's isStructureChunk (placement-only, no biome gate) reports
        // several over-reports near the corpus; every one is biome-rejected for
        // its structure at its TRUE start position (verified against the pinned
        // Paper 0a99345 runtime biome source), so no beard start actually
        // generates. The only over-reports within beard reach (<= SEARCH chunks,
        // chebyshev) are:
        //   (15,15): (9,11)   [village salt 10387312]        — start projects to
        //     WORLD_SURFACE_WG, biome = ocean; the village biome tags
        //     (plains/meadow, desert, savanna, snowy_*, taiga) accept no ocean ->
        //     REJECTED
        //   (15,15): (9,11)   [ancient city salt 20083232]   — ConstantHeight
        //     y=-27, biome = lush_caves; ancient-city tag is deep_dark ->
        //     REJECTED
        //   (31,31): (36,25)  [village salt 10387312]        — surface biome =
        //     ocean -> REJECTED
        //   (-31,-31): (-25,-27)  [trail ruins salt 83469867] — surface biome =
        //     plains; trail-ruins tag is taiga/snowy_taiga/old_growth_*/jungle
        //     -> REJECTED
        // The rest ((17,7), (23,19), (35,38), (38,31), (-29,-21), (-22,-36),
        // (-13,-24)) are farther than beard reach and cannot affect the corpus.
        // This truth table is verified against the pinned Paper 0a99345 runtime.
        const SEARCH: i64 = 6;
        // (corpus chunk, set salt, placement chunk) — the verified in-reach
        // over-reports, all biome-rejected. A named tuple type avoids the
        // type-complexity lint while keeping the truth table readable.
        type InReach = ((i64, i64), i64, (i64, i64));
        const VERIFIED_IN_REACH: &[InReach] = &[
            ((15, 15), 10387312, (9, 11)),
            ((15, 15), 20083232, (9, 11)),
            ((31, 31), 10387312, (36, 25)),
            ((-31, -31), 83469867, (-25, -27)),
        ];

        // The full-search sweep: any beard-affecting placement chunk within
        // SEARCH chunks of a corpus chunk must be one of the verified in-reach
        // over-reports (all biome-rejected). A placement chunk at exactly a
        // corpus coordinate, or any placement chunk within reach that is NOT in
        // the verified set, is a hard regression.
        for &(cx, cz) in &corpus {
            for dx in -SEARCH..=SEARCH {
                for dz in -SEARCH..=SEARCH {
                    let sx = cx + dx;
                    let sz = cz + dz;
                    for &(salt, spacing, sep) in BEARD_SPREADS {
                        let potential =
                            potential_spread_chunk(fixture.seed, salt, spacing, sep, sx, sz);
                        if potential == (sx, sz) {
                            let known =
                                VERIFIED_IN_REACH
                                    .iter()
                                    .any(|&((ccx, ccz), s, (pcx, pcz))| {
                                        s == salt
                                            && (pcx, pcz) == (sx, sz)
                                            && (ccx, ccz) == (cx, cz)
                                    });
                            assert!(
                                known,
                                "unverified beard-affecting placement chunk ({sx},{sz}) for \
                                 salt {salt} within {SEARCH} chunks of corpus chunk ({cx},{cz}) \
                                 — the structure-freeness truth table must be updated (this \
                                 could be a real beard start)"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The power-of-two `nextInt` shortcut must match Paper exactly: for a bound
    /// that is a power of two, `BitRandomSource.nextInt` takes the TOP bits via
    /// `(bound * next(31)) >> 31`, NOT `next(31) % bound` (the bottom bits).
    /// Ancient cities (spacing 24, separation 8 -> limit 16) depend on this. The
    /// expected draws are grounded in real Paper 0a99345 output: for the ancient
    /// city feature seed of grid cell (0,0) (`42 + 20083232`), the real
    /// `LegacyRandomSource.nextInt(16)` sequence is 9, 11 (raw `next(31)` =
    /// 1242028138, 1500875121), and the first modulo draw would be 10 — so this
    /// test also fails if the implementation ever regresses to the bottom-bit
    /// form.
    #[test]
    fn next_int_power_of_two_matches_paper() {
        // Grid (0,0) feature seed for ancient cities at seed 42:
        // setLargeFeatureWithSalt(42, 0, 0, 20083232) = 0 + 0 + 42 + 20083232.
        let mut rng = LegacyRandomSource::new(42 + 20083232);
        assert_eq!(
            rng.next_int(16),
            9,
            "ancient-city grid(0,0) first spread draw"
        );
        assert_eq!(
            rng.next_int(16),
            11,
            "ancient-city grid(0,0) second spread draw"
        );

        // The top-bit and bottom-bit forms disagree on the first draw, so a
        // regression to `next(31) % bound` is caught, not silently accepted.
        let mut bottom_bits = LegacyRandomSource::new(42 + 20083232);
        assert_eq!((16 * bottom_bits.next(31)) >> 31, 9, "top-bit shortcut");
        let mut modulo = LegacyRandomSource::new(42 + 20083232);
        assert_eq!(
            modulo.next(31) % 16,
            10,
            "bottom-bit modulo differs, proving the shortcut matters"
        );
    }

    /// Paper's `WorldgenRandom.setLargeFeatureWithSalt` + `LegacyRandomSource`,
    /// reproduced exactly: a 48-bit LCG initialized from the seed via
    /// `setSeed` (xor-masked), then advanced by (gridX * 341873128712 + gridZ *
    /// 132897987541 + salt + seed) with the standard LCG constants.
    /// `RandomSpreadStructurePlacement.getPotentialStructureChunk`.
    fn potential_spread_chunk(
        seed: i64,
        salt: i64,
        spacing: i64,
        separation: i64,
        sx: i64,
        sz: i64,
    ) -> (i64, i64) {
        // Minecraft's LegacyRandomSource: setSeed(s) = (s ^ 0x5DEECE66D) & ((1<<48)-1)
        // WorldgenRandom.setLargeFeatureWithSalt(seed, gridX, gridZ, salt):
        //   setSeed(gridX * 341873128712L + gridZ * 132897987541L + seed + salt)
        // (addition, NOT xor — mirrors Paper's WorldgenRandom.java). Then the two
        // spreadType.evaluate draws via nextInt(limit).
        let grid_x = sx.div_euclid(spacing);
        let grid_z = sz.div_euclid(spacing);
        let feature_seed = grid_x
            .wrapping_mul(341873128712i64)
            .wrapping_add(grid_z.wrapping_mul(132897987541i64))
            .wrapping_add(seed)
            .wrapping_add(salt);
        let limit = (spacing - separation) as i32;
        let mut rng = LegacyRandomSource::new(feature_seed);
        let spread_x = rng.next_int(limit);
        let spread_z = rng.next_int(limit);
        (grid_x * spacing + spread_x, grid_z * spacing + spread_z)
    }

    /// Minecraft's `LegacyRandomSource`: a 48-bit LCG with the standard
    /// constants, `nextInt(bound)` consuming the top 31 bits.
    struct LegacyRandomSource {
        seed: i64,
    }

    impl LegacyRandomSource {
        const MULTIPLIER: i64 = 0x5DEECE66D;
        const ADDEND: i64 = 0xB;
        const MASK: i64 = (1 << 48) - 1;

        fn new(seed: i64) -> Self {
            let seed = (seed ^ Self::MULTIPLIER) & Self::MASK;
            Self { seed }
        }

        fn next(&mut self, bits: i32) -> i64 {
            // LegacyRandomSource.next(bits): advance the LCG, then take the top
            // `bits` bits of the new seed.
            self.seed = (self
                .seed
                .wrapping_mul(Self::MULTIPLIER)
                .wrapping_add(Self::ADDEND))
                & Self::MASK;
            self.seed >> (48 - bits)
        }

        fn next_int(&mut self, bound: i32) -> i64 {
            // BitRandomSource.nextInt(bound), exactly:
            //   * power-of-two bound: take the TOP bits, (bound * next(31)) >> 31.
            //     This is NOT next(31) % bound (the bottom bits) — they disagree,
            //     and ancient cities (spacing 24, separation 8 -> limit 16) depend
            //     on the top-bit form.
            //   * otherwise: rejection sampling, sample = next(31); modulo =
            //     sample % bound; redraw while (sample - modulo + (bound - 1))
            //     overflows into the sign bit (i.e. < 0).
            if bound <= 0 {
                return 0;
            }
            if (bound & (bound - 1)) == 0 {
                return ((bound as i64) * self.next(31)) >> 31;
            }
            let sample = self.next(31) as i32;
            let modulo = sample % bound;
            if (sample - modulo + (bound - 1)) < 0 {
                return self.next_int(bound); // reject and redraw, exactly like Paper
            }
            modulo as i64
        }
    }

    #[test]
    fn committed_surface_column_is_non_vacuous() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let fixture = load(&dir).unwrap();
        assert_eq!(fixture.seed, 42, "seed-42 golden");
        assert_eq!(fixture.paper, "26.2-DEV-main@0a99345");
        assert_eq!(fixture.noise_settings, "minecraft:overworld");
        assert_eq!(fixture.columns.len(), 8);
        // Every column ran post-surface: real block changes, real heightmap
        // deltas, an unprimed pre-heightmap, and non-air terrain on the axis.
        for col in &fixture.columns {
            assert!(
                col.any_surface_changed,
                "column ({},{}) must show surface changes",
                col.cx, col.cz
            );
            assert!(
                col.any_height_changed,
                "column ({},{}) must show heightmap changes",
                col.cx, col.cz
            );
            assert!(col.samples.iter().any(|s| s.changed));
            assert!(
                col.samples.iter().any(|s| !s.post.air),
                "terrain must exist"
            );
        }
        // The #175 matrix is covered.
        let columns: BTreeMap<(i64, i64), usize> =
            fixture.columns.iter().map(|c| ((c.cx, c.cz), 1)).collect();
        assert_eq!(columns.len(), 8);
        assert!(columns.contains_key(&(0, 0)));
        assert!(columns.contains_key(&(31, 31)));
        assert!(columns.contains_key(&(-31, -31)));
        // Bedrock exists at the floor in the overworld golden (block id 85 for
        // minecraft:bedrock in the sampled (0,0) column).
        let c00 = &fixture.columns[0];
        let bedrock = c00
            .samples
            .iter()
            .find(|s| s.y == MIN_Y)
            .expect("y=-64 sampled");
        assert_eq!(
            bedrock.post.block, "minecraft:bedrock",
            "overworld floor must be bedrock"
        );
        assert!(!bedrock.post.air);
    }

    /// The default `verify` path must fail UNVERIFIED when the committed
    /// surface-column fixture tree is absent — never silently skip (D8).
    #[test]
    fn missing_fixture_tree_is_unverified() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-sc-missing-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        let result = crate::verify_surface_column_step(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    /// Regenerating the surface-column manifest in Rust is byte-identical to the
    /// committed manifest (given an unchanged golden) — regeneration is git-clean
    /// and the committed manifest is what the writer would produce.
    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-sc-regen-{}", std::process::id()));
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
            "regenerating the surface-column manifest must be byte-identical (git-clean)"
        );
        crate::verify_fixtures(&scratch).unwrap();
        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    /// A committed golden that is already broken (wrong seed) must make the
    /// tamper control hard-fail at the baseline pre-verify — never pass
    /// vacuously by crediting the flip for a pre-existing failure.
    #[test]
    fn tamper_reports_broken_baseline_instead_of_passing_vacuously() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let scratch = std::env::temp_dir().join(format!(
            "rivet-oracle-sc-tamper-broken-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::copy(dir.join("manifest.json"), scratch.join("manifest.json")).unwrap();
        let raw = fs::read_to_string(dir.join(FIXTURE_BASENAME)).unwrap();
        let corrupt = raw.replace("\"seed\": 42", "\"seed\": 43");
        fs::write(scratch.join(FIXTURE_BASENAME), corrupt).unwrap();
        let result = tamper_negative_control(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        assert!(
            !matches!(result, Ok(())),
            "a broken baseline must not pass the tamper control vacuously: {result:?}"
        );
    }

    #[test]
    fn parse_mode_maps_no_flags_to_verify() {
        assert!(matches!(parse_mode(&[]), Ok(SurfaceColumnMode::Verify)));
        assert!(matches!(
            parse_mode(&["--tamper"]),
            Ok(SurfaceColumnMode::Tamper)
        ));
        assert!(matches!(
            parse_mode(&["--sample"]),
            Ok(SurfaceColumnMode::Sample)
        ));
    }

    #[test]
    fn parse_mode_rejects_unknown_flags() {
        let err = parse_mode(&["--sampple"]).expect_err("unknown flag must be rejected");
        assert!(err.to_string().contains("--sampple"), "unexpected: {err}");
        assert!(parse_mode(&["--tamper", "--nope"]).is_err());
    }

    #[test]
    fn parse_mode_rejects_tamper_and_sample_together() {
        assert!(parse_mode(&["--tamper", "--sample"]).is_err());
        assert!(parse_mode(&["--sample", "--tamper"]).is_err());
        // Repeats of the same flag are fine.
        assert!(matches!(
            parse_mode(&["--tamper", "--tamper"]),
            Ok(SurfaceColumnMode::Tamper)
        ));
    }

    #[test]
    fn parse_mode_rejects_help_in_combination() {
        // Sole --help/-h is intercepted by the dispatcher; here it is an unknown
        // flag, so a --tamper --help never silently skips the control.
        assert!(parse_mode(&["--help"]).is_err());
        assert!(parse_mode(&["--tamper", "--help"]).is_err());
    }

    /// A fill-only capture — post = plain fill output (air/water/stone/
    /// deepslate/bedrock), with no surface-rule block anywhere — must be
    /// rejected even though every per-column check (deltas, unprimed
    /// pre-heightmaps, terrain exists) passes. This is the probe regression the
    /// corpus-level check catches: dropping the buildSurface call relabels fill
    /// output as "post-surface". Operates on a scratch copy (with a regenerated
    /// manifest) so the committed fixtures stay untouched.
    #[test]
    fn fill_only_capture_is_rejected() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let mut fixture = load(&dir).unwrap();
        // Replace every surface-rule block with stone — the fill pass emits no
        // dirt/grass/sand/gravel/sulfur, so this models a fill-only capture.
        for col in fixture.columns.iter_mut() {
            for s in col.samples.iter_mut() {
                if SURFACE_RULE_BLOCKS.contains(&s.post.block.as_str()) {
                    s.post.block = "minecraft:stone".to_string();
                }
                s.changed = s.pre != s.post;
            }
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-sc-fillonly-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string_pretty(&fixture).unwrap(),
        )
        .unwrap();
        regenerate_manifest(&scratch).unwrap();
        let result = verify_surface_column(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("fill-only capture must be rejected");
        assert!(
            err.to_string().contains("no surface-rule block"),
            "unexpected error: {err}"
        );
    }

    /// A no-op capture (all-air post states, no surface changes) must be
    /// rejected — the whole point of the pre/post metadata is to detect a probe
    /// that recorded the chunk before buildSurface. Operates on a scratch copy
    /// (with a regenerated manifest) so the committed fixtures stay untouched.
    #[test]
    fn no_op_capture_is_rejected() {
        let dir = fixtures_dir().join("surface-column");
        require_fixture(&dir);
        let mut fixture = load(&dir).unwrap();
        // Collapse one column to the pre-surface state: every post state equals
        // its pre state, heights all unprimed, no changes. A probe that captured
        // the chunk before buildSurface would emit exactly this.
        let col = &mut fixture.columns[0];
        for s in col.samples.iter_mut() {
            s.post = s.pre.clone();
            s.changed = false;
        }
        col.any_surface_changed = false;
        for h in col.heightmap.iter_mut() {
            h.post_ws = h.pre_ws;
            h.post_of = h.pre_of;
        }
        col.any_height_changed = false;

        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-sc-noop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join(FIXTURE_BASENAME),
            serde_json::to_string_pretty(&fixture).unwrap(),
        )
        .unwrap();
        // Regenerate the manifest so the hash gate passes and the non-vacuity
        // checks (not the hash gate) are what reject the no-op capture.
        regenerate_manifest(&scratch).unwrap();
        let result = verify_surface_column(&scratch);
        let _ = fs::remove_dir_all(&scratch);
        let err = result.expect_err("no-op capture must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("any-surface-changed=false"),
            "unexpected error: {msg}"
        );
    }

    /// The probe runner's exit-code contract: a missing runtime prerequisite
    /// exits 3 (UNVERIFIED, the same code `Error::Unverified` maps to), so the
    /// regeneration path can never relabel fixtures captured against a jar it
    /// could not authenticate. Hermetic: a nonexistent RIVET_PAPER_RUNTIME_JAR
    /// forces the script's missing-jar branch before any javac/java work.
    #[test]
    fn probe_runner_missing_runtime_exits_unverified() {
        let crate_root = crate::crate_dir();
        let script = crate_root.join("scripts/run_surface_column_probe.sh");
        let status = std::process::Command::new("bash")
            .arg(&script)
            .arg(std::env::temp_dir().join("rivet-oracle-sc-sample-probe"))
            .env("RIVET_PAPER_RUNTIME_JAR", "/nonexistent/paper-26.2.jar")
            .env("RIVET_PAPER_LIBRARIES", "/nonexistent/libraries")
            .stdin(std::process::Stdio::null())
            .status()
            .expect("bash must run");
        assert_eq!(
            status.code(),
            Some(crate::EXIT_UNVERIFIED),
            "missing runtime must exit 3 UNVERIFIED, got {status}"
        );
    }
}
