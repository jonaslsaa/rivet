//! The composed-noise golden comparison slice: a non-vacuous seed-42 golden
//! comparison against the pinned Paper overworld generator's NOISE checkpoint
//! (`mc.world.level.levelgen`), the piece the generated-world scoreboard needs
//! before generated chunks exist.
//!
//! `fixtures/composed-noise/composed-noise.json` is captured by
//! `scripts/run_composed_noise_probe.sh` (running `ComposedNoiseProbe` against
//! the pinned Paper 0a99345 runtime) and records, at the #175 chunk-coordinate
//! matrix expressed as block columns:
//!
//!   * the router climate fields (temperature/vegetation/continents/erosion/
//!     depth/ridges) and the float-cast weirdness + folded `peaksAndValleys`,
//!   * the interpolated final density (`getInterpolatedNoiseValue` — the value
//!     `doFill` uses to place blocks), the raw `finalDensity` router field, and
//!     `preliminarySurfaceLevel`,
//!   * every value as BOTH the round-tripping JSON double AND the raw IEEE-754
//!     bit pattern (`Double.doubleToLongBits` / `Float.floatToIntBits`), so a
//!     Rust port asserts `f64::to_bits` exactly — never a tolerant compare.
//!
//! The verify command asserts the committed golden's provenance (Paper pin +
//! manifest SHA-256s), its FULL_CHUNK_STEP reachability (computed by
//! `chunk_level`, not assumed), the #175 matrix shape, and that every value
//! round-trips to its raw IEEE-754 bits exactly; then it prints the
//! BIOMES→NOISE→SURFACE→CARVERS→FEATURES→LIGHT→FULL status/provenance
//! scoreboard. The tamper negative control proves the comparison detects a
//! flipped byte (the manifest SHA-256 gate).

use crate::chunk_level::{ChunkLevelConsts, ChunkPyramid, by_status, status_around_full_chunk};
use crate::{CapturedFile, Error, sha256_hex};
use rivet_world::chunk::status::ChunkStatus;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The checkpoint ladder the scoreboard reports — the statuses a generated
/// world must prove green in order (each later checkpoint depends on all
/// earlier ones).
pub const CHECKPOINTS: &[ChunkStatus] = &[
    ChunkStatus::Biomes,
    ChunkStatus::Noise,
    ChunkStatus::Surface,
    ChunkStatus::Carvers,
    ChunkStatus::Features,
    ChunkStatus::Light,
    ChunkStatus::Full,
];

const KIND: &str = "composed-noise";
const FIXTURE_BASENAME: &str = "composed-noise.json";

/// Scratch-dir uniquifier for `tamper_negative_control` (tests run in parallel
/// threads within one process, so pid alone would collide).
static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The vertical slice the probe samples per column (`DENSITY_YS` in
/// `ComposedNoiseProbe.java`).
const DENSITY_YS: [i64; 10] = [-60, -40, -20, 0, 20, 40, 60, 80, 100, 120];

/// A captured bit-pattern entry (double via `Double.doubleToLongBits`).
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct BitSample {
    pub bits: i64,
    pub value: serde_json::Value,
}

/// One climate column sample.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct ClimateSample {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub cx: i64,
    pub cz: i64,
    pub temperature: BitSample,
    pub vegetation: BitSample,
    pub continents: BitSample,
    pub erosion: BitSample,
    pub depth: BitSample,
    pub ridges: BitSample,
    pub weirdness: BitSample,
    #[serde(rename = "peaksAndValleys")]
    pub peaks_and_valleys: BitSample,
}

/// One density column sample.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct DensitySample {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub cx: i64,
    pub cz: i64,
    pub density: BitSample,
    #[serde(rename = "finalDensity")]
    pub final_density: BitSample,
    #[serde(rename = "preliminarySurfaceLevel")]
    pub preliminary_surface_level: BitSample,
}

/// The parsed composed-noise fixture.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct ComposedNoise {
    pub seed: i64,
    pub paper: String,
    pub dimension: String,
    pub generator: String,
    #[serde(rename = "level-type")]
    pub level_type: String,
    #[serde(rename = "noise-settings")]
    pub noise_settings: String,
    pub format: i64,
    #[serde(rename = "full-chunk-step")]
    pub full_chunk_step: FullChunkStep,
    pub climate: Vec<ClimateSample>,
    pub density: Vec<DensitySample>,
}

/// The FULL_CHUNK_STEP reachability captured from live Paper.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct FullChunkStep {
    pub level: i64,
    #[serde(rename = "accumulated-radius")]
    pub accumulated_radius: i64,
    #[serde(rename = "max-level")]
    pub max_level: i64,
    #[serde(rename = "by-distance")]
    pub by_distance: Vec<ByDistance>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct ByDistance {
    pub distance: i64,
    pub status: String,
}

/// A scoreboard row: the checkpoint status, its measured reachability level
/// (`ChunkLevel.byStatus`), and the number of committed fixture sample rows
/// (density rows, or climate rows for BIOMES) a green checkpoint would assert.
/// Each density row carries 3 bit-patterns (density/finalDensity/
/// preliminarySurfaceLevel); each climate row carries 8.
#[derive(Debug)]
pub struct ScoreboardRow {
    pub status: ChunkStatus,
    pub level: i64,
    pub entries: usize,
}

/// Parse + structurally validate the committed composed-noise fixture.
pub fn load(dir: &Path) -> Result<ComposedNoise, Error> {
    let path = dir.join(FIXTURE_BASENAME);
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", path.display())))?;
    let v: ComposedNoise = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid {FIXTURE_BASENAME}: {e}")))?;
    if v.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported composed-noise format {} (expected 1)",
            v.format
        )));
    }
    Ok(v)
}

/// The generated-world scoreboard: every checkpoint, its reachability level,
/// and how many committed fixture sample rows a green checkpoint would assert.
/// Printed by verify; the checkpoint statuses are asserted to be a faithful
/// port (the FULL_CHUNK_STEP reachability and byStatus levels come from
/// `chunk_level`, never from the fixture).
pub fn scoreboard() -> Vec<ScoreboardRow> {
    let step = ChunkPyramid::generation_pyramid()
        .get_step_to(ChunkStatus::Full)
        .clone();
    let mut rows: Vec<ScoreboardRow> = Vec::new();
    for status in CHECKPOINTS {
        let level = by_status(&step, *status) as i64;
        // A checkpoint asserts the fixture sample rows that cover its value
        // leaves: NOISE asserts all 80 density + 8 climate rows (each density
        // row carries density/finalDensity/preliminarySurfaceLevel; each
        // climate row the 8 climate fields). BIOMES/SURFACE/etc. reach the same
        // columns at their own level. For a not-yet-wired checkpoint the count
        // is the number that WOULD be asserted — the scoreboard is
        // forward-looking.
        let entries = match status {
            ChunkStatus::Biomes => 8,
            ChunkStatus::Noise => 88,
            ChunkStatus::Surface => 80,
            ChunkStatus::Carvers => 80,
            ChunkStatus::Features => 80,
            ChunkStatus::Light => 80,
            ChunkStatus::Full => 80,
            _ => 0,
        };
        rows.push(ScoreboardRow {
            status: *status,
            level,
            entries,
        });
    }
    rows
}

/// Print the status/provenance scoreboard.
pub fn print_scoreboard() {
    println!();
    println!("generated-world status/provenance scoreboard");
    println!("=============================================");
    println!(
        "{:<20} {:>6} {:>10}   coverage",
        "checkpoint", "level", "samples"
    );
    let step = ChunkPyramid::generation_pyramid()
        .get_step_to(ChunkStatus::Full)
        .clone();
    for row in scoreboard() {
        // A checkpoint is "captured" by the committed golden when its value
        // leaves are present in the fixture today (hash-gated + structurally
        // verified). Only NOISE (the composed-noise slice) and BIOMES (the
        // climate column) are captured now; the rest are the planned later
        // checkpoints, which will assert Rust-computed values against the
        // same committed bit patterns.
        let captured = matches!(row.status, ChunkStatus::Biomes | ChunkStatus::Noise);
        println!(
            "{:<20} {:>6} {:>10}   {}",
            row.status.serialization_name(),
            row.level,
            row.entries,
            if captured { "captured" } else { "not-wired" }
        );
    }
    println!();
    println!(
        "FULL_CHUNK_STEP reachability (computed, not assumed): level {} radius {} max {}",
        ChunkLevelConsts::FULL_CHUNK_LEVEL,
        step.accumulated_dependencies.get_radius(),
        ChunkLevelConsts::FULL_CHUNK_LEVEL as i64
            + step.accumulated_dependencies.get_radius() as i64
    );
    let mut dist = BTreeMap::new();
    for d in 0..=step.accumulated_dependencies.get_radius() {
        dist.entry(status_around_full_chunk(&step, d).serialization_name())
            .or_insert_with(Vec::new)
            .push(d);
    }
    for (status, distances) in dist {
        println!("  distance(s) {distances:?} serialize {status}");
    }
}

/// Assert the committed golden's FULL_CHUNK_STEP reachability + byStatus levels
/// equal the faithful port (`chunk_level`). This is what proves the harness
/// computes reachability rather than assuming one status per forced level.
pub fn verify_full_chunk_step(fixture: &ComposedNoise) -> Result<(), Error> {
    let step = ChunkPyramid::generation_pyramid()
        .get_step_to(ChunkStatus::Full)
        .clone();
    if fixture.full_chunk_step.level != ChunkLevelConsts::FULL_CHUNK_LEVEL as i64 {
        return Err(Error::Manifest(format!(
            "full-chunk-step.level {} != Paper FULL_CHUNK_LEVEL 33",
            fixture.full_chunk_step.level
        )));
    }
    if fixture.full_chunk_step.accumulated_radius
        != step.accumulated_dependencies.get_radius() as i64
    {
        return Err(Error::Manifest(format!(
            "full-chunk-step.accumulated-radius {} != port {}",
            fixture.full_chunk_step.accumulated_radius,
            step.accumulated_dependencies.get_radius()
        )));
    }
    if fixture.full_chunk_step.max_level
        != (ChunkLevelConsts::FULL_CHUNK_LEVEL as i64
            + step.accumulated_dependencies.get_radius() as i64)
    {
        return Err(Error::Manifest(format!(
            "full-chunk-step.max-level {} != port {}",
            fixture.full_chunk_step.max_level,
            ChunkLevelConsts::FULL_CHUNK_LEVEL as i64
                + step.accumulated_dependencies.get_radius() as i64
        )));
    }
    let mut captured: Vec<(i64, &str)> = fixture
        .full_chunk_step
        .by_distance
        .iter()
        .map(|d| (d.distance, d.status.as_str()))
        .collect();
    captured.sort_by_key(|(d, _)| *d);
    let mut expected: Vec<(usize, String)> = (0..=step.accumulated_dependencies.get_radius())
        .map(|d| {
            (
                d,
                status_around_full_chunk(&step, d)
                    .serialization_name()
                    .to_string(),
            )
        })
        .collect();
    expected.sort_by_key(|(d, _)| *d);
    if captured.len() != expected.len() {
        return Err(Error::Manifest(format!(
            "full-chunk-step.by-distance has {} entries, port has {}",
            captured.len(),
            expected.len()
        )));
    }
    for ((cd, cs), (ed, es)) in captured.iter().zip(expected.iter()) {
        if *cd != *ed as i64 || *cs != es.as_str() {
            return Err(Error::Manifest(format!(
                "full-chunk-step by-distance[{ed}] mismatch: captured {cs} (at {cd}), port {es}"
            )));
        }
    }
    // The byStatus reachability: each checkpoint's serialization level.
    for status in CHECKPOINTS {
        let level = by_status(&step, *status) as i64;
        // Sanity: FULL at 33. (STRUCTURE_STARTS' 44 is covered by the by-distance
        // comparison above — it reaches out at distances 4..=11 — so it needs no
        // separate check here.)
        if *status == ChunkStatus::Full && level != 33 {
            return Err(Error::Manifest(format!(
                "byStatus(FULL) = {level}, expected 33"
            )));
        }
    }
    Ok(())
}

/// Verify the committed composed-noise golden: the manifest hashes, the pinned
/// provenance, the FULL_CHUNK_STEP reachability, and the bit-exact entries
/// (each captured `bits` must be present; the `value` round-trips). This is the
/// NOISE checkpoint gate.
pub fn verify_composed_noise(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    // 1. Provenance + manifest hashes. The golden is pinned to seed 42 and to
    //    Paper 0a99345; a fixture regenerated under a different seed or commit
    //    is drift, not the pinned golden.
    if fixture.seed != 42 {
        return Err(Error::Manifest(format!(
            "composed-noise seed {} != pinned seed 42",
            fixture.seed
        )));
    }
    // The golden is the overworld NOISE checkpoint — the generator identity
    // must be pinned too, not just the seed/commit. A fixture regenerated
    // against a different generator is drift, not the pinned golden.
    if fixture.dimension != "overworld" {
        return Err(Error::Manifest(format!(
            "composed-noise dimension {} != overworld",
            fixture.dimension
        )));
    }
    if fixture.generator != "normal" {
        return Err(Error::Manifest(format!(
            "composed-noise generator {} != normal",
            fixture.generator
        )));
    }
    if fixture.level_type != "minecraft:normal" {
        return Err(Error::Manifest(format!(
            "composed-noise level-type {} != minecraft:normal",
            fixture.level_type
        )));
    }
    if fixture.noise_settings != "minecraft:overworld" {
        return Err(Error::Manifest(format!(
            "composed-noise noise-settings {} != minecraft:overworld",
            fixture.noise_settings
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
            "composed-noise fixture not pinned to Paper 0a99345: {:?}",
            manifest.paper
        )));
    }
    if fixture.paper != manifest.paper.as_deref().unwrap_or("") {
        return Err(Error::Manifest(format!(
            "fixture paper {} != manifest paper {:?}",
            fixture.paper, manifest.paper
        )));
    }
    // 2. The computed (not assumed) reachability.
    verify_full_chunk_step(&fixture)?;
    // 3. Structural + exact value assertions. The committed fixture is
    //    hash-gated (verify_fixtures above), so the bytes are the probe's exact
    //    output; here we pin the shape (the #175 matrix, the density Y slice)
    //    and that every value round-trips to its raw IEEE-754 bits exactly —
    //    the assertion a Rust port would make (`f64::from_bits(bits)`), made
    //    against the fixture's own self-consistency.
    if fixture.climate.len() != 8 {
        return Err(Error::Manifest(format!(
            "composed-noise has {} climate entries, expected 8 (#175 matrix)",
            fixture.climate.len()
        )));
    }
    if fixture.density.len() != 80 {
        return Err(Error::Manifest(format!(
            "composed-noise has {} density entries, expected 80 (8 columns x 10 y)",
            fixture.density.len()
        )));
    }
    let mut seen_columns: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for s in &fixture.climate {
        let key = (s.cx, s.cz);
        *seen_columns.entry(key).or_default() += 1;
    }
    if seen_columns.len() != 8 {
        return Err(Error::Manifest(format!(
            "composed-noise climate covers {} distinct #175 columns, expected 8",
            seen_columns.len()
        )));
    }
    // Every density column samples the full 10-y vertical slice the probe
    // records (DENSITY_YS), exactly once per y.
    let mut density_by_column: BTreeMap<(i64, i64), Vec<i64>> = BTreeMap::new();
    for s in &fixture.density {
        let key = (s.cx, s.cz);
        if !seen_columns.contains_key(&key) {
            return Err(Error::Manifest(format!(
                "density column ({},{}) not in the #175 matrix",
                s.cx, s.cz
            )));
        }
        density_by_column.entry(key).or_default().push(s.y);
    }
    for key in seen_columns.keys() {
        let mut ys = density_by_column
            .get(key)
            .ok_or_else(|| {
                Error::Manifest(format!(
                    "density column ({},{}) has no entries",
                    key.0, key.1
                ))
            })?
            .clone();
        ys.sort_unstable();
        if ys != DENSITY_YS.to_vec() {
            return Err(Error::Manifest(format!(
                "density column ({},{}) ys {ys:?} != expected {DENSITY_YS:?}",
                key.0, key.1
            )));
        }
    }
    // 4. Every entry's `value` must round-trip to its `bits` exactly (density:
    //    f64::from_bits; climate: the float-cast f32::from_bits widened), and
    //    density must never be NaN — a NaN density would be un-placeable
    //    garbage, not a golden.
    verify_value_bits(&fixture)?;
    Ok(())
}

/// Assert every committed entry's `value` round-trips to its raw IEEE-754
/// `bits` exactly, and that density entries are finite.
fn verify_value_bits(fixture: &ComposedNoise) -> Result<(), Error> {
    for (ci, s) in fixture.climate.iter().enumerate() {
        for (name, sample) in [
            ("temperature", &s.temperature),
            ("vegetation", &s.vegetation),
            ("continents", &s.continents),
            ("erosion", &s.erosion),
            ("depth", &s.depth),
            ("ridges", &s.ridges),
            ("weirdness", &s.weirdness),
            ("peaksAndValleys", &s.peaks_and_valleys),
        ] {
            // Climate fields are float-cast (`(double) f` in the probe), so
            // the JSON value must equal the widened f32 the bits encode. Like
            // density, a non-numeric value (the probe's "NaN" string, or JSON
            // null) is a malformed golden — never a legal skip.
            let v = sample.value.as_f64().ok_or_else(|| {
                Error::Manifest(format!(
                    "climate[{ci}] {name} is NaN — climate goldens must be finite"
                ))
            })?;
            let rt = f32::from_bits(sample.bits as u32) as f64;
            if rt != v {
                return Err(Error::Manifest(format!(
                    "climate[{ci}] {name} value {v} != f32::from_bits({}) {rt}",
                    sample.bits
                )));
            }
        }
    }
    for (di, s) in fixture.density.iter().enumerate() {
        for (name, sample) in [
            ("density", &s.density),
            ("finalDensity", &s.final_density),
            ("preliminarySurfaceLevel", &s.preliminary_surface_level),
        ] {
            let v = sample.value.as_f64().ok_or_else(|| {
                Error::Manifest(format!(
                    "density[{di}] {name} is NaN — density goldens must be finite"
                ))
            })?;
            let rt = f64::from_bits(sample.bits as u64);
            if rt != v {
                return Err(Error::Manifest(format!(
                    "density[{di}] {name} value {v} != f64::from_bits({}) {rt}",
                    sample.bits
                )));
            }
        }
    }
    Ok(())
}

/// The composed-noise subcommand mode selected by its `--tamper` / `--sample`
/// flags (default: verify + scoreboard).
#[derive(Debug, Clone, Copy)]
pub enum ComposedNoiseMode {
    Verify,
    Tamper,
    Sample,
}

/// Parse the composed-noise subcommand flags into a mode. Unknown flags are a
/// usage error — before, a typo'd `--sampple` fell through to the verify
/// branch and exited 0 after verifying, silently misreading the intended mode.
/// `--tamper` and `--sample` are mutually exclusive, mirroring the `verify`
/// subcommand's `--m2`/`--full` handling. Sole `--help`/`-h` is intercepted by
/// the dispatcher before this runs; in any combination it is a hard usage
/// error here.
pub fn parse_mode(flags: &[&str]) -> Result<ComposedNoiseMode, Error> {
    let mut tamper = false;
    let mut sample = false;
    for flag in flags {
        match *flag {
            "--tamper" => tamper = true,
            "--sample" => sample = true,
            other => {
                return Err(Error::Gate(format!(
                    "composed-noise takes only --tamper/--sample, got {other}"
                )));
            }
        }
    }
    match (tamper, sample) {
        (true, true) => Err(Error::Gate(
            "composed-noise --tamper and --sample are mutually exclusive".into(),
        )),
        (true, false) => Ok(ComposedNoiseMode::Tamper),
        (false, true) => Ok(ComposedNoiseMode::Sample),
        (false, false) => Ok(ComposedNoiseMode::Verify),
    }
}

/// Assert the composed-noise golden tree is present (manifest AND golden file).
/// A missing golden — whole tree absent, or manifest present but the golden
/// file missing — is a missing prerequisite, `Error::Unverified` (exit 3), not
/// a hard FAIL: the comparison cannot run against an absent golden, so
/// "unverified" is the honest classification. A present-but-corrupt golden
/// (empty, unparsable, hash-mismatched) fails hard (exit 1) once the
/// comparison runs. `--sample` regeneration does not route through this guard,
/// since it writes the golden from the Paper runtime.
pub fn require_fixture_tree(dir: &Path) -> Result<(), Error> {
    if !dir.join("manifest.json").is_file() || !dir.join(FIXTURE_BASENAME).is_file() {
        return Err(Error::Unverified(format!(
            "composed-noise fixtures {} are ABSENT — the seed-42 golden and its \
             NOISE-checkpoint gate cannot verify (git checkout or regenerate via \
             `composed-noise --sample`); refusing to pass green without them",
            dir.display()
        )));
    }
    Ok(())
}

/// The tamper negative control: corrupt a committed bit pattern (flip one byte
/// of the JSON) and assert the verification FAILS — proving the comparison is
/// not vacuous (a green is impossible with tampered goldens).
///
/// The control is load-bearing: when it cannot run — the committed golden is
/// absent (nothing to tamper) or empty (nothing to flip) — that is a hard FAIL
/// (`Error::Gate`/`Error::Manifest`, exit 1), unlike the verify path whose
/// missing golden is a missing prerequisite (exit 3).
///
/// It operates on a scratch copy in the temp dir so the committed fixtures are
/// never mutated — a panic or `?` early-return can never leave
/// `fixtures/composed-noise/` corrupted. The scratch dir is unique per
/// invocation (pid + counter), so parallel test calls never share one.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    if !dir.join("manifest.json").is_file() || !dir.join(FIXTURE_BASENAME).is_file() {
        return Err(Error::Gate(format!(
            "composed-noise fixtures {} are ABSENT — the tamper negative control \
             cannot run without the committed golden (git checkout or regenerate \
             via `composed-noise --sample`); refusing a vacuous pass",
            dir.display()
        )));
    }
    let scratch = std::env::temp_dir().join(format!(
        "rivet-oracle-composed-noise-{}-{}",
        std::process::id(),
        SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)
        .map_err(|e| Error::Gate(format!("cannot create scratch {}: {e}", scratch.display())))?;
    // Read the committed golden once, flip one byte in place, and write the
    // tampered copy; the manifest is copied verbatim so verification runs
    // against a hash-gated tree.
    fs::copy(dir.join("manifest.json"), scratch.join("manifest.json"))?;
    let mut tampered = fs::read(dir.join(FIXTURE_BASENAME))?;
    if tampered.is_empty() {
        let _ = fs::remove_dir_all(&scratch);
        return Err(Error::Manifest(format!(
            "composed-noise golden {} is EMPTY — a byte flip needs a payload; \
             restore the committed fixture (git checkout), it is corrupt",
            dir.join(FIXTURE_BASENAME).display()
        )));
    }
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0xFF;
    fs::write(scratch.join(FIXTURE_BASENAME), &tampered)?;
    let result = verify_composed_noise(&scratch);
    // Discard the scratch copy; the committed fixtures are untouched.
    let _ = fs::remove_dir_all(&scratch);
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "composed-noise tamper was NOT detected — the comparison is vacuous".into(),
        }),
        Err(_) => Ok(()),
    }
}

/// `fixtures/composed-noise/manifest.json`, serialized in the exact committed
/// field order so regeneration is byte-identical (git-clean), mirroring the
/// worldgen manifest convention.
#[derive(serde::Serialize)]
struct ComposedNoiseManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Write `fixtures/composed-noise/manifest.json` from the freshly generated
/// fixture (byte-identical field order, like the worldgen manifest).
pub fn regenerate_manifest(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    let data = fs::read(dir.join(FIXTURE_BASENAME))?;
    let manifest = ComposedNoiseManifest {
        format: 1,
        paper: &fixture.paper,
        seed: &fixture.seed.to_string(),
        level_type: &fixture.level_type,
        kind: KIND,
        note: "Bit-exact seed-42 composed-noise goldens for the overworld NOISE \
               checkpoint (mc.world.level.levelgen): router climate fields, \
               float-cast weirdness + peaksAndValleys, interpolated final density, \
               raw finalDensity, preliminarySurfaceLevel — each as the JSON double \
               AND the raw IEEE-754 bits (Double.doubleToLongBits / \
               Float.floatToIntBits) at the #175 chunk-coordinate matrix. Plus \
               Paper's live FULL_CHUNK_STEP reachability. Captured from the pinned \
               Paper runtime via tools/rivet-oracle/src/java/ComposedNoiseProbe.java; \
               regenerate with scripts/run_composed_noise_probe.sh.",
        captured: vec![CapturedFile {
            path: FIXTURE_BASENAME.to_string(),
            sha256: sha256_hex(&data),
            bytes: data.len(),
        }],
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize composed-noise manifest: {e}")))?;
    text.push('\n');
    fs::write(dir.join("manifest.json"), text)?;
    Ok(())
}

/// Run the Paper-side probe into `dir` (regenerating composed-noise.json), then
/// rewrite the manifest. Requires the materialized pinned Paper runtime (or the
/// env overrides).
pub fn run_probe(dir: &Path) -> Result<(), Error> {
    let crate_root = crate::crate_dir();
    let script = crate_root.join("scripts/run_composed_noise_probe.sh");
    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::Gate(format!("failed to run {}: {e}", script.display())))?;
    if !status.success() {
        return Err(Error::Gate(format!(
            "run_composed_noise_probe.sh exited {status} — see its stderr"
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

    /// A unique scratch dir per test (pid + counter, so parallel tests in one
    /// process never collide with each other or with the tamper scratch copy).
    fn scratch(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rivet-oracle-cn-{tag}-{}-{}",
            std::process::id(),
            SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The committed composed-noise golden is a load-bearing deliverable: a test
    /// that needs it must FAIL when it is absent, never silently return (D8:
    /// never weaken/delete fixtures to go green; a missing load-bearing fixture
    /// is a hard failure).
    fn require_fixture(dir: &std::path::Path) {
        require_fixture_tree(dir).expect(
            "committed composed-noise fixtures are ABSENT — the seed-42 golden and its \
             NOISE-checkpoint gate cannot verify; restore them (git checkout) or this \
             test is red, never silently skipped",
        );
    }

    #[test]
    fn committed_composed_noise_verifies() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        verify_composed_noise(&dir).expect("committed composed-noise golden should verify");
    }

    #[test]
    fn committed_composed_noise_is_non_vacuous() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        let fixture = load(&dir).unwrap();
        assert_eq!(fixture.seed, 42, "seed-42 golden");
        assert_eq!(fixture.paper, "26.2-DEV-main@0a99345");
        assert_eq!(fixture.noise_settings, "minecraft:overworld");
        assert_eq!(fixture.climate.len(), 8);
        assert_eq!(fixture.density.len(), 80);
        // Every committed entry has a real bit pattern (never NaN/-0 tricks).
        for s in &fixture.density {
            for (name, sample) in [
                ("density", &s.density),
                ("finalDensity", &s.final_density),
                ("preliminarySurfaceLevel", &s.preliminary_surface_level),
            ] {
                assert_ne!(
                    sample.value.as_str(),
                    Some("NaN"),
                    "{name} at ({},{},{}) must not be NaN",
                    s.x,
                    s.y,
                    s.z
                );
                assert_ne!(sample.bits, i64::MAX, "{name} sentinel");
            }
        }
        for s in &fixture.climate {
            for (name, sample) in [
                ("temperature", &s.temperature),
                ("vegetation", &s.vegetation),
                ("continents", &s.continents),
                ("erosion", &s.erosion),
                ("depth", &s.depth),
                ("ridges", &s.ridges),
                ("weirdness", &s.weirdness),
                ("peaksAndValleys", &s.peaks_and_valleys),
            ] {
                assert_ne!(
                    sample.value.as_str(),
                    Some("NaN"),
                    "{name} at ({},{})",
                    s.x,
                    s.z
                );
            }
        }
        // The #175 matrix is covered: the block origin of every corpus chunk.
        let columns: BTreeMap<(i64, i64), usize> =
            fixture.climate.iter().map(|s| ((s.cx, s.cz), 1)).collect();
        assert_eq!(columns.len(), 8);
        assert!(columns.contains_key(&(0, 0)));
        assert!(columns.contains_key(&(31, 31)));
        assert!(columns.contains_key(&(-31, -31)));
    }

    /// A climate entry with a NaN value (the probe's "NaN" string) must be
    /// rejected like a NaN density — the golden must be finite, and a NaN
    /// climate field is a malformed fixture, not a legal skip.
    #[test]
    fn climate_nan_value_is_rejected() {
        let finite = |bits: i64, value: f64| BitSample {
            bits,
            value: serde_json::json!(value),
        };
        let fixture = ComposedNoise {
            seed: 42,
            paper: "26.2-DEV-main@0a99345".into(),
            dimension: "overworld".into(),
            generator: "normal".into(),
            level_type: "minecraft:normal".into(),
            noise_settings: "minecraft:overworld".into(),
            format: 1,
            full_chunk_step: FullChunkStep {
                level: 33,
                accumulated_radius: 11,
                max_level: 44,
                by_distance: vec![],
            },
            climate: vec![ClimateSample {
                x: 0,
                y: -60,
                z: 0,
                cx: 0,
                cz: 0,
                temperature: BitSample {
                    bits: f32::NAN.to_bits() as i64,
                    value: serde_json::json!("NaN"),
                },
                vegetation: finite(0x3F800000, 1.0),
                continents: finite(0x3F800000, 1.0),
                erosion: finite(0x3F800000, 1.0),
                depth: finite(0x3F800000, 1.0),
                ridges: finite(0x3F800000, 1.0),
                weirdness: finite(0x3F800000, 1.0),
                peaks_and_valleys: finite(0x3F800000, 1.0),
            }],
            density: vec![],
        };
        let err = verify_value_bits(&fixture).expect_err("NaN climate must be rejected");
        // The error names the offending field.
        let msg = err.to_string();
        assert!(msg.contains("is NaN"), "unexpected error: {msg}");
    }

    #[test]
    fn missing_fixture_tree_is_unverified() {
        let dir = scratch("missing");
        fs::create_dir_all(&dir).unwrap();
        let result = crate::verify_composed_noise_step(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    #[test]
    fn gate_path_classifies_missing_composed_noise_golden_as_unverified() {
        let root = scratch("gate");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("manifest.json"), r#"{"format":1,"captured":[]}"#).unwrap();
        fs::create_dir_all(root.join("composed-noise")).unwrap();
        fs::write(root.join("composed-noise/manifest.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(&root);
        let _ = fs::remove_dir_all(&root);
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "gate must classify a missing composed-noise golden as UNVERIFIED (exit 3), got {result:?}"
        );
    }

    #[test]
    fn gate_path_surfaces_other_kind_corruption_before_missing_composed_noise() {
        let root = scratch("gate-corrupt");
        fs::create_dir_all(&root).unwrap();
        // A corrupt generic kind: the manifest references a missing file, so
        // the generic hash loop hard-fails it before composed-noise is reached.
        fs::write(
            root.join("manifest.json"),
            r#"{"format":1,"captured":[{"path":"missing.bin","sha256":"00"}]}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("composed-noise")).unwrap();
        fs::write(root.join("composed-noise/manifest.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(&root);
        let _ = fs::remove_dir_all(&root);
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "other-kind corruption must surface as a hard FAIL (exit 1), not be masked by missing composed-noise: {result:?}"
        );
    }

    #[test]
    fn tamper_absent_tree_is_a_hard_error() {
        let dir = scratch("tamper-absent");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), "{}").unwrap();
        let result = tamper_negative_control(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(
            matches!(result, Err(crate::Error::Gate(_))),
            "absent tamper tree must hard-FAIL (exit 1), not UNVERIFIED: {result:?}"
        );
    }

    #[test]
    fn tamper_empty_golden_is_a_hard_error() {
        let dir = scratch("tamper-empty");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), "{}").unwrap();
        fs::write(dir.join(FIXTURE_BASENAME), "").unwrap();
        let result = tamper_negative_control(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "empty golden must hard-fail, not panic: {result:?}"
        );
    }

    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        let scratch = scratch("regen");
        fs::create_dir_all(&scratch).unwrap();
        fs::copy(dir.join(FIXTURE_BASENAME), scratch.join(FIXTURE_BASENAME)).unwrap();
        regenerate_manifest(&scratch).unwrap();
        let committed = fs::read(dir.join("manifest.json")).unwrap();
        let regenerated = fs::read(scratch.join("manifest.json")).unwrap();
        assert_eq!(
            committed, regenerated,
            "regenerating the composed-noise manifest must be byte-identical (git-clean)"
        );
        // The regenerated manifest is self-consistent: it verifies its files.
        crate::verify_fixtures(&scratch).unwrap();
        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    #[test]
    fn scoreboard_lists_all_checkpoints() {
        let rows = scoreboard();
        let statuses: Vec<ChunkStatus> = rows.iter().map(|r| r.status).collect();
        assert_eq!(
            statuses,
            vec![
                ChunkStatus::Biomes,
                ChunkStatus::Noise,
                ChunkStatus::Surface,
                ChunkStatus::Carvers,
                ChunkStatus::Features,
                ChunkStatus::Light,
                ChunkStatus::Full,
            ]
        );
        // The reachability levels (not assumed one-per-status): the generated-
        // world scoreboard must reproduce Paper's non-monotonic reachability.
        let levels: Vec<i64> = rows.iter().map(|r| r.level).collect();
        assert_eq!(levels, vec![36, 35, 35, 35, 34, 33, 33]);
    }

    #[test]
    fn parse_mode_maps_no_flags_to_verify() {
        assert!(matches!(parse_mode(&[]), Ok(ComposedNoiseMode::Verify)));
    }

    #[test]
    fn parse_mode_maps_each_flag() {
        assert!(matches!(
            parse_mode(&["--tamper"]),
            Ok(ComposedNoiseMode::Tamper)
        ));
        assert!(matches!(
            parse_mode(&["--sample"]),
            Ok(ComposedNoiseMode::Sample)
        ));
    }

    /// A typo'd flag must be a usage error, never a silent fallback to verify
    /// (previously `--sampple` exited 0 after verifying — a silent misread of
    /// the intended mode).
    #[test]
    fn parse_mode_rejects_unknown_flags() {
        let err = parse_mode(&["--sampple"]).expect_err("unknown flag must be rejected");
        assert!(err.to_string().contains("--sampple"), "unexpected: {err}");
        assert!(parse_mode(&["--tamper", "--nope"]).is_err());
    }

    #[test]
    fn parse_mode_rejects_tamper_and_sample_together() {
        let err = parse_mode(&["--tamper", "--sample"]).expect_err("mutually exclusive");
        assert!(
            err.to_string().contains("mutually exclusive"),
            "unexpected: {err}"
        );
    }

    /// `--help` is valid only as the sole argument (intercepted by the
    /// dispatcher); in any combination with a mode or another flag it is a hard
    /// usage error, so a `--tamper --help` never silently skips the control.
    #[test]
    fn parse_mode_rejects_help_in_combination() {
        assert!(parse_mode(&["--help"]).is_err());
        assert!(parse_mode(&["--tamper", "--help"]).is_err());
        assert!(parse_mode(&["--help", "--bogus"]).is_err());
    }
}
