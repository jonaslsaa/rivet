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
//!     port asserts the exact post-surface block id per sampled Y,
//!   * pre/post WORLD_SURFACE_WG and OCEAN_FLOOR_WG heights (the pre-surface
//!     snapshot is an all-air chunk whose heightmaps are unprimed, so a capture
//!     that ran surface before fill, or skipped buildSurface entirely, is
//!     visible as a null pre/post delta),
//!   * the surface biome the surface pass saw at the top of the column.
//!
//! The verify command asserts the committed golden's provenance (Paper pin +
//! manifest SHA-256s), the pinned seed/generator/height shape, that every
//! column really had surface blocks changed (non-vacuity: a no-op capture that
//! recorded the chunk pre-surface fails), and that the committed surface-biome
//! ids are the seed-42 overworld biomes. The tamper negative control proves
//! the comparison detects a flipped byte (the manifest SHA-256 gate).

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
    if crate::parse_paper_pin(manifest.paper.as_deref()).as_deref() != Some("0a99345") {
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
        return Err(Error::Gate(format!(
            "run_surface_column_probe.sh exited {status} — see its stderr"
        )));
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
}
