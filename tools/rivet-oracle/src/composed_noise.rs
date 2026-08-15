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
//! flipped byte (the manifest SHA-256 gate). `--sample` regeneration
//! (`run_probe`) validates the freshly captured output with the same semantic
//! gate before rewriting the manifest, so a garbage probe output is never
//! committed as the new golden.

use crate::chunk_level::{ChunkLevelConsts, ChunkPyramid, by_status, status_around_full_chunk};
use crate::{CapturedFile, Error, sha256_hex};
use rivet_world::chunk::status::ChunkStatus;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
    if raw.is_empty() {
        return Err(Error::Manifest(format!(
            "composed-noise golden {} is EMPTY — restore the committed fixture \
             (git checkout), it is corrupt",
            path.display()
        )));
    }
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

/// Verify the committed composed-noise golden: the manifest hash gate first
/// (SHA-256s + kind + pinned Paper provenance), then the fixture's own
/// semantics (`verify_semantics`). This is the NOISE checkpoint gate.
pub fn verify_composed_noise(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    // 1. The manifest hash gate: the golden bytes must match the committed
    //    SHA-256 — a flipped byte is drift, not the probe's output. The manifest
    //    must also carry the composed-noise kind + pinned Paper 0a99345,
    //    agreeing with the golden's own paper string.
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
    // 2. Then the fixture's own semantics.
    verify_semantics(&fixture)?;
    Ok(())
}

/// Semantically validate a composed-noise fixture: the pinned provenance
/// (seed/dimension/generator/level-type/noise-settings/Paper pin), the computed
/// (not assumed) FULL_CHUNK_STEP reachability, the #175 matrix shape, and that
/// every value round-trips to its raw IEEE-754 bits exactly (and is finite).
/// Shared by `verify_composed_noise` (after the hash gate) and `capture` (before
/// the manifest rewrite), so a semantically invalid fresh capture can never
/// become the committed golden.
fn verify_semantics(fixture: &ComposedNoise) -> Result<(), Error> {
    // 1. Provenance. The golden is pinned to seed 42 and to Paper 0a99345; a
    //    fixture regenerated under a different seed or commit is drift, not the
    //    pinned golden.
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
    // The capture's own Paper pin — a probe run against a different commit is
    // drift even when its reachability and values still match.
    if crate::parse_paper_pin(Some(&fixture.paper)).as_deref() != Some("0a99345") {
        return Err(Error::Manifest(format!(
            "composed-noise fixture not pinned to Paper 0a99345: {:?}",
            fixture.paper
        )));
    }
    // 2. The computed (not assumed) reachability.
    verify_full_chunk_step(fixture)?;
    // 3. Structural + exact value assertions. In the verify path the golden is
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
    verify_value_bits(fixture)?;
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
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ComposedNoiseMode {
    Verify,
    Tamper,
    Sample,
}

/// Parse the composed-noise subcommand flags into a mode. Unknown flags are a
/// usage error — a typo'd `--sampple` used to fall through to the verify branch
/// and exit 0 after verifying, so strictness here prevents a silent misread of
/// the intended mode. `--tamper` and `--sample` are mutually exclusive (repeats
/// of the same flag are fine). Sole `--help`/`-h` is intercepted by the
/// dispatcher; in any combination it is a hard usage error here (the
/// subcommand accepts no positional arguments, unlike `verify`).
pub fn parse_mode(flags: &[&str]) -> Result<ComposedNoiseMode, Error> {
    let mut mode: Option<ComposedNoiseMode> = None;
    for flag in flags {
        let m = match *flag {
            "--tamper" => ComposedNoiseMode::Tamper,
            "--sample" => ComposedNoiseMode::Sample,
            other => {
                return Err(Error::Gate(format!(
                    "composed-noise takes only --tamper/--sample, got {other}"
                )));
            }
        };
        match mode {
            None => mode = Some(m),
            Some(prev) if prev == m => {}
            Some(_) => {
                return Err(Error::Gate(
                    "composed-noise --tamper and --sample are mutually exclusive".into(),
                ));
            }
        }
    }
    Ok(mode.unwrap_or(ComposedNoiseMode::Verify))
}

/// The committed composed-noise tree state, classified by which files exist:
/// `Absent` (neither file — the regenerate case), `Partial` (exactly one — a
/// broken checked-in tree), or `Present` (both). `Present` means present, not
/// valid: a zero-byte golden still hard-fails in `load`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FixtureState {
    Absent,
    Partial,
    Present,
}

/// Classify the committed composed-noise tree by which of `manifest.json` /
/// the golden file exist.
pub fn fixture_state(dir: &Path) -> FixtureState {
    match (
        dir.join("manifest.json").is_file(),
        dir.join(FIXTURE_BASENAME).is_file(),
    ) {
        (false, false) => FixtureState::Absent,
        (true, true) => FixtureState::Present,
        _ => FixtureState::Partial,
    }
}

/// Assert the composed-noise tree is not wholly absent. `FixtureState::Absent`
/// is a missing prerequisite — `Error::Unverified` (exit 3), the regenerate
/// case; a partial or corrupt tree (manifest-only, golden-only, zero-byte
/// golden) is a broken checked-in fixture and hard-fails (exit 1) once the
/// comparison runs. `--sample` regeneration does not route through this guard,
/// since it writes the golden from the Paper runtime.
pub fn require_fixture_tree(dir: &Path) -> Result<(), Error> {
    if fixture_state(dir) == FixtureState::Absent {
        return Err(Error::Unverified(format!(
            "composed-noise fixtures {} are ABSENT — the seed-42 golden and its \
             NOISE-checkpoint gate cannot verify (git checkout or regenerate via \
             `composed-noise --sample`); refusing to pass green without them",
            dir.display()
        )));
    }
    Ok(())
}

/// Flip one digit of a sample `bits` value — the first `"bits": ` field's
/// number, skipping its optional sign and leading digit (so the tampered file
/// stays valid JSON — no leading zero). A digit flipped to the adjacent digit
/// keeps the file valid UTF-8 and parseable, so the tamper is caught by the
/// raw-bytes SHA-256 hash mismatch, exercising the hash comparison rather than
/// a decode failure. Returns a hard `Error::Manifest` if there is no
/// `"bits": ` field to flip (the control cannot run, so it must not pass
/// vacuously).
fn flip_sample_bits_digit(golden: &mut [u8]) -> Result<(), Error> {
    const MARKER: &[u8] = b"\"bits\": ";
    let start = golden
        .windows(MARKER.len())
        .position(|w| w == MARKER)
        .ok_or_else(|| {
            Error::Manifest(
                "composed-noise golden has no \"bits\": field to flip for the tamper control"
                    .into(),
            )
        })?
        + MARKER.len();
    let mut idx = start;
    if golden.get(idx) == Some(&b'-') {
        idx += 1;
    }
    // Skip the leading digit: flipping it could produce a leading zero, which
    // is invalid JSON.
    idx += 1;
    let flip = golden[idx..]
        .iter()
        .position(|b| b.is_ascii_digit())
        .map(|d| idx + d)
        .ok_or_else(|| {
            Error::Manifest("composed-noise golden has no flip-able digit after \"bits\":".into())
        })?;
    golden[flip] ^= 0x01;
    Ok(())
}

/// Classify the verification result of a tampered scratch copy. The expected
/// tamper signature is a SHA-256 hash mismatch on the golden itself
/// (`Error::HashMismatch` with `path == FIXTURE_BASENAME`): that alone is
/// detection (Ok). A green result means the tamper was not detected
/// (`Error::NegativeControl` — vacuous). Any other error — a missing captured
/// file, a hash mismatch on another path, a decode failure — is unrelated to
/// the flip and propagates, never masquerading as a detected tamper.
fn classify_tamper_result(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Ok(()) => Err(Error::NegativeControl {
            message: "composed-noise tamper was NOT detected — the comparison is vacuous".into(),
        }),
        Err(Error::HashMismatch { path, .. }) if path == FIXTURE_BASENAME => Ok(()),
        Err(other) => Err(other),
    }
}

/// The tamper negative control: corrupt a committed bit pattern (flip one digit
/// of a sample `bits` value) and assert the verification FAILS — proving the
/// comparison is not vacuous (a green is impossible with tampered goldens).
///
/// The control is load-bearing: it first verifies the committed golden is
/// green, so a broken baseline is reported (exit 1) rather than passing
/// vacuously. An absent or partial tree is a hard `Error::Gate` (exit 1) — the
/// control cannot run vacuous. An empty golden is a hard `Error::Manifest`.
///
/// It tampers a `tempfile` scratch copy, so the committed fixtures are never
/// mutated and the scratch dir is removed on every path when the `TempDir`
/// drops. Only the expected tamper signature — a SHA-256 hash mismatch on the
/// golden itself — counts as detection; any unrelated scratch verification
/// failure propagates rather than masquerading as a detected tamper.
pub fn tamper_negative_control(dir: &Path) -> Result<(), Error> {
    match fixture_state(dir) {
        FixtureState::Present => {}
        FixtureState::Absent => {
            return Err(Error::Gate(format!(
                "composed-noise fixtures {} are ABSENT — the tamper negative control \
                 cannot run without the committed golden (git checkout or regenerate \
                 via `composed-noise --sample`); refusing a vacuous pass",
                dir.display()
            )));
        }
        FixtureState::Partial => {
            return Err(Error::Gate(format!(
                "composed-noise fixtures {} are INCOMPLETE (manifest or golden missing) — \
                 the tamper negative control cannot run without the committed golden \
                 (git checkout); refusing a vacuous pass",
                dir.display()
            )));
        }
    }
    // The baseline must be green before it is tampered; otherwise any scratch
    // failure would be credited to the flip and the control would pass
    // vacuously on an already-broken golden.
    verify_composed_noise(dir)?;
    let scratch = tempfile::tempdir()
        .map_err(|e| Error::Gate(format!("cannot create composed-noise scratch dir: {e}")))?;
    // Scratch setup I/O: any failure propagates — it is an infrastructure
    // error, never a tamper detection.
    fs::copy(
        dir.join("manifest.json"),
        scratch.path().join("manifest.json"),
    )?;
    let mut tampered = fs::read(dir.join(FIXTURE_BASENAME))?;
    if tampered.is_empty() {
        return Err(Error::Manifest(format!(
            "composed-noise golden {} is EMPTY — the tamper negative control needs a \
             payload to flip; restore the committed fixture (git checkout), it is corrupt",
            dir.join(FIXTURE_BASENAME).display()
        )));
    }
    flip_sample_bits_digit(&mut tampered)?;
    fs::write(scratch.path().join(FIXTURE_BASENAME), &tampered)?;
    // Only the golden's own hash mismatch is the tamper signature; any other
    // scratch failure propagates via classify_tamper_result.
    classify_tamper_result(verify_composed_noise(scratch.path()))
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
/// capture it: validate the fresh output's semantics and only then rewrite the
/// manifest. Requires the materialized pinned Paper runtime (or the env
/// overrides).
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
    capture(dir)
}

/// Commit a freshly generated composed-noise golden: validate its semantics
/// BEFORE the manifest rewrite hashes/commits it. `regenerate_manifest` derives
/// the manifest hash from the golden bytes, so without this gate it would commit
/// garbage — a probe run against the wrong seed/generator/Paper, a NaN density,
/// a broken #175 shape, or drifted reachability must never become the new
/// committed golden.
fn capture(dir: &Path) -> Result<(), Error> {
    let fixture = load(dir)?;
    verify_semantics(&fixture)?;
    regenerate_manifest(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// A unique scratch dir per test, auto-removed on drop (`TempDir`), so
    /// parallel tests never collide and never leak.
    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("rivet-oracle-cn-{tag}-"))
            .tempdir()
            .expect("create scratch tempdir")
    }

    /// The committed composed-noise golden is a load-bearing deliverable: a test
    /// that needs it must FAIL when it is absent, never silently return (D8:
    /// never weaken/delete fixtures to go green; a missing load-bearing fixture
    /// is a hard failure).
    fn require_fixture(dir: &std::path::Path) {
        require_fixture_tree(dir).unwrap_or_else(|e| {
            panic!(
                "committed composed-noise fixtures at {} are unusable: {e}",
                dir.display()
            )
        });
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
        let result = crate::verify_composed_noise_step(dir.path());
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "expected Error::Unverified (exit 3), got {result:?}"
        );
    }

    #[test]
    fn gate_path_classifies_fully_absent_composed_noise_tree_as_unverified() {
        let root = scratch("gate");
        fs::write(
            root.path().join("manifest.json"),
            r#"{"format":1,"captured":[]}"#,
        )
        .unwrap();
        // No composed-noise dir at all: a fully absent golden tree is the
        // "regenerate the fixture" case — UNVERIFIED (exit 3).
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "gate must classify a fully absent composed-noise tree as UNVERIFIED (exit 3), got {result:?}"
        );
    }

    #[test]
    fn gate_path_classifies_partial_composed_noise_tree_as_hard_fail() {
        let root = scratch("gate-partial");
        fs::write(
            root.path().join("manifest.json"),
            r#"{"format":1,"captured":[]}"#,
        )
        .unwrap();
        fs::create_dir_all(root.path().join("composed-noise")).unwrap();
        fs::write(root.path().join("composed-noise/manifest.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "a manifest-present/golden-missing tree is a corrupt checked-in fixture and must hard-FAIL (exit 1), got {result:?}"
        );
    }

    #[test]
    fn gate_path_surfaces_other_kind_corruption_before_missing_composed_noise() {
        let root = scratch("gate-corrupt");
        // A corrupt generic kind: the manifest references a missing file, so
        // the generic hash loop hard-fails it before composed-noise is reached.
        fs::write(
            root.path().join("manifest.json"),
            r#"{"format":1,"captured":[{"path":"missing.bin","sha256":"00"}]}"#,
        )
        .unwrap();
        fs::create_dir_all(root.path().join("composed-noise")).unwrap();
        fs::write(root.path().join("composed-noise/manifest.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "other-kind corruption must surface as a hard FAIL (exit 1), not be masked by missing composed-noise: {result:?}"
        );
    }

    #[test]
    fn gate_path_load_bearing_goldens_only_root_is_verified() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let ge_src = fixtures_dir().join("generated-expected");
        assert!(
            ge_src.join("manifest.json").is_file()
                && ge_src
                    .join(crate::generated_expected::FIXTURE_BASENAME)
                    .is_file(),
            "committed generated-expected fixtures at {} are unusable",
            ge_src.display()
        );
        let fe_src = fixtures_dir().join("features");
        assert!(
            fe_src.join("manifest.json").is_file()
                && fe_src.join(crate::features::FIXTURE_BASENAME).is_file(),
            "committed features fixtures at {} are unusable",
            fe_src.display()
        );
        let root = scratch("gate-cn-only");
        fs::create_dir_all(root.path().join("composed-noise")).unwrap();
        fs::copy(
            src.join("manifest.json"),
            root.path().join("composed-noise/manifest.json"),
        )
        .unwrap();
        fs::copy(
            src.join(FIXTURE_BASENAME),
            root.path().join("composed-noise/composed-noise.json"),
        )
        .unwrap();
        // generated-expected is equally load-bearing (PR #563/#595): carry both
        // goldens so the gate's positive path exercises the full composed
        // contract — neither golden alone may claim an overall green.
        fs::create_dir_all(root.path().join("generated-expected")).unwrap();
        fs::copy(
            ge_src.join("manifest.json"),
            root.path().join("generated-expected/manifest.json"),
        )
        .unwrap();
        fs::copy(
            ge_src.join(crate::generated_expected::FIXTURE_BASENAME),
            root.path()
                .join("generated-expected/generated-expected.json"),
        )
        .unwrap();
        // The seed-42 FEATURES checkpoint is the newest load-bearing golden
        // (PR #175/#232): carry it too, or the gate's positive path fails with
        // UNVERIFIED on the missing features tree.
        fs::create_dir_all(root.path().join("features")).unwrap();
        fs::copy(
            fe_src.join("manifest.json"),
            root.path().join("features/manifest.json"),
        )
        .unwrap();
        fs::copy(
            fe_src.join(crate::features::FIXTURE_BASENAME),
            root.path().join("features/features.json"),
        )
        .unwrap();
        // The seed-42 LIGHT-stage checkpoint is load-bearing too (PR #184): carry
        // its full tree — manifest, light.json, and the 25 forced chunk NBTs —
        // since the manifest binds every captured file by SHA-256.
        let li_src = fixtures_dir().join("light");
        assert!(
            li_src.join("manifest.json").is_file()
                && li_src.join(crate::light_stage::FIXTURE_BASENAME).is_file(),
            "committed light fixtures at {} are unusable",
            li_src.display()
        );
        fs::create_dir_all(root.path().join("light/chunks")).unwrap();
        fs::copy(
            li_src.join("manifest.json"),
            root.path().join("light/manifest.json"),
        )
        .unwrap();
        fs::copy(
            li_src.join(crate::light_stage::FIXTURE_BASENAME),
            root.path().join("light/light.json"),
        )
        .unwrap();
        for (cx, cz) in crate::light_stage::forced_coordinates() {
            let name = crate::light_stage::chunk_fixture_path(cx, cz);
            fs::copy(li_src.join(&name), root.path().join("light").join(&name)).unwrap();
        }
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            result.is_ok(),
            "goldens-only root must verify green: {result:?}"
        );
    }

    #[test]
    fn gate_path_composed_noise_only_root_absent_is_unverified() {
        let root = scratch("gate-cn-only-absent");
        // A root with no generic kinds and no composed-noise tree at all: the
        // load-bearing composed-noise golden is wholly absent, so the gate is
        // UNVERIFIED (exit 3) — the missing-prerequisite classification.
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Unverified(_))),
            "composed-noise-only root with a wholly absent tree must be UNVERIFIED (exit 3), got {result:?}"
        );
    }

    #[test]
    fn gate_path_composed_noise_only_root_partial_tree_is_hard_fail() {
        let root = scratch("gate-cn-only-partial");
        fs::create_dir_all(root.path().join("composed-noise")).unwrap();
        fs::write(root.path().join("composed-noise/manifest.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "composed-noise-only root with a manifest-present/golden-missing tree must hard-FAIL (exit 1), got {result:?}"
        );
    }

    #[test]
    fn gate_path_composed_noise_only_root_golden_only_is_hard_fail() {
        let root = scratch("gate-cn-only-golden");
        fs::create_dir_all(root.path().join("composed-noise")).unwrap();
        fs::write(root.path().join("composed-noise/composed-noise.json"), "{}").unwrap();
        let result = crate::verify_all_fixture_kinds_from(root.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "composed-noise-only root with a golden-only tree is a corrupt checked-in fixture and must hard-FAIL (exit 1), got {result:?}"
        );
    }

    /// A golden-only tree (golden present, manifest missing) is a partial
    /// tree, not a whole-tree absence: the verify path hard-fails (exit 1).
    #[test]
    fn verify_golden_only_tree_is_a_hard_error() {
        let dir = scratch("verify-golden-only");
        fs::write(dir.path().join(FIXTURE_BASENAME), "{}").unwrap();
        let result = crate::verify_composed_noise_step(dir.path());
        assert!(
            !matches!(result, Err(crate::Error::Unverified(_))),
            "golden-only tree must not be UNVERIFIED: {result:?}"
        );
    }

    #[test]
    fn verify_empty_golden_is_a_hard_error() {
        let dir = scratch("verify-empty");
        fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        fs::write(dir.path().join(FIXTURE_BASENAME), "").unwrap();
        let result = verify_composed_noise(dir.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "empty golden must hard-fail (exit 1), not UNVERIFIED: {result:?}"
        );
    }

    #[test]
    fn tamper_absent_tree_is_a_hard_error() {
        let dir = scratch("tamper-absent");
        // No manifest and no golden: wholly absent.
        let result = tamper_negative_control(dir.path());
        assert!(
            matches!(result, Err(crate::Error::Gate(_))),
            "absent tamper tree must hard-FAIL (exit 1), not UNVERIFIED: {result:?}"
        );
    }

    #[test]
    fn tamper_partial_tree_message_names_incomplete() {
        let dir = scratch("tamper-partial");
        fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        let result = tamper_negative_control(dir.path());
        let msg = result
            .expect_err("partial tamper tree must hard-FAIL (exit 1)")
            .to_string();
        assert!(
            msg.contains("INCOMPLETE"),
            "partial tree must be reported as INCOMPLETE, not ABSENT: {msg}"
        );
    }

    #[test]
    fn tamper_empty_golden_is_a_hard_error() {
        let dir = scratch("tamper-empty");
        fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        fs::write(dir.path().join(FIXTURE_BASENAME), "").unwrap();
        let result = tamper_negative_control(dir.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "empty golden must hard-fail, not panic: {result:?}"
        );
    }

    /// A committed golden that is already broken (wrong seed) must make the
    /// tamper control hard-fail at the baseline pre-verify — never pass
    /// vacuously by crediting the flip for a pre-existing failure.
    #[test]
    fn tamper_reports_broken_baseline_instead_of_passing_vacuously() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let dir = scratch("tamper-broken");
        fs::copy(src.join("manifest.json"), dir.path().join("manifest.json")).unwrap();
        let raw = fs::read_to_string(src.join(FIXTURE_BASENAME)).unwrap();
        let corrupt = raw.replace("\"seed\": 42,", "\"seed\": 43,");
        fs::write(dir.path().join(FIXTURE_BASENAME), corrupt).unwrap();
        let result = tamper_negative_control(dir.path());
        assert!(
            !matches!(result, Ok(())),
            "a broken baseline must not pass the tamper control vacuously: {result:?}"
        );
    }

    /// The expected tamper signature — a SHA-256 hash mismatch on the golden
    /// itself — counts as detection.
    #[test]
    fn classify_tamper_detection_on_golden_hash_mismatch() {
        let result = classify_tamper_result(Err(Error::HashMismatch {
            path: FIXTURE_BASENAME.into(),
            expected: "e".into(),
            actual: "a".into(),
        }));
        assert!(
            result.is_ok(),
            "golden hash mismatch must count as detection: {result:?}"
        );
    }

    /// A green tampered copy means the comparison did not notice the tamper —
    /// vacuous.
    #[test]
    fn classify_tamper_green_is_vacuous() {
        let result = classify_tamper_result(Ok(()));
        assert!(
            matches!(result, Err(Error::NegativeControl { .. })),
            "a green tampered copy is vacuous: {result:?}"
        );
    }

    /// Any failure unrelated to the flipped golden must propagate, never
    /// masquerade as a detected tamper: a manifest that gained a captured file
    /// (missing on disk), a hash mismatch on another path, or an
    /// infrastructure/verification error.
    #[test]
    fn classify_tamper_propagates_unrelated_failures() {
        assert!(matches!(
            classify_tamper_result(Err(Error::Manifest("captured file x missing".into()))),
            Err(Error::Manifest(_))
        ));
        assert!(matches!(
            classify_tamper_result(Err(Error::HashMismatch {
                path: "other.json".into(),
                expected: "e".into(),
                actual: "a".into(),
            })),
            Err(Error::HashMismatch { path, .. }) if path == "other.json"
        ));
        assert!(matches!(
            classify_tamper_result(Err(Error::Unverified("no prereq".into()))),
            Err(Error::Unverified(_))
        ));
    }

    /// In the full flow, an unrelated scratch verification failure must
    /// propagate, never be reported as a detected tamper. A committed tree
    /// whose manifest also captures a second file (present on disk, so the
    /// baseline verifies) is tampered: the scratch copy omits that second file
    /// (tamper copies only the manifest + golden), so scratch verification
    /// fails with "captured file ... missing" before the golden hash is even
    /// reached — and the control must surface that Manifest error, not Ok.
    #[test]
    fn tamper_propagates_unrelated_scratch_failure() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let dir = scratch("tamper-unrelated");
        fs::copy(
            src.join(FIXTURE_BASENAME),
            dir.path().join(FIXTURE_BASENAME),
        )
        .unwrap();
        // A second captured file, present in the committed tree (baseline
        // verifies) and ordered BEFORE the golden so the scratch hash loop hits
        // it (missing) first.
        fs::write(dir.path().join("extra.bin"), b"extra").unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(src.join("manifest.json")).unwrap()).unwrap();
        let golden_entry = manifest["captured"][0].clone();
        manifest["captured"] = serde_json::json!([
            { "path": "extra.bin", "sha256": sha256_hex(b"extra"), "bytes": 5 },
            golden_entry
        ]);
        fs::write(
            dir.path().join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        // Baseline verify must pass (both files present).
        verify_composed_noise(dir.path()).expect("baseline with the extra file must verify");
        // The control must hard-fail with the missing extra.bin error — NOT
        // report detection (Ok).
        let result = tamper_negative_control(dir.path());
        assert!(
            matches!(result, Err(crate::Error::Manifest(_))),
            "an unrelated scratch failure must propagate, not pass as a detected tamper: {result:?}"
        );
    }

    /// The flip must keep the golden valid UTF-8 JSON so the tamper is caught
    /// by the hash gate, not a decode failure: flipping one digit of a sample
    /// `bits` value changes the raw bytes while staying parseable.
    #[test]
    fn flip_sample_bits_digit_keeps_golden_parseable() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let mut bytes = fs::read(src.join(FIXTURE_BASENAME)).unwrap();
        flip_sample_bits_digit(&mut bytes).unwrap();
        // Still valid UTF-8 JSON, and parseable to the same structure.
        let text = std::str::from_utf8(&bytes).expect("flip must keep valid UTF-8");
        let v: serde_json::Value = serde_json::from_str(text).expect("flip must keep valid JSON");
        assert_eq!(v["seed"], 42, "flip must not touch a pinned header field");
        // The raw bytes changed (the hash gate will catch it).
        assert_ne!(bytes, fs::read(src.join(FIXTURE_BASENAME)).unwrap());
    }

    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        let scratch = scratch("regen");
        fs::copy(
            dir.join(FIXTURE_BASENAME),
            scratch.path().join(FIXTURE_BASENAME),
        )
        .unwrap();
        regenerate_manifest(scratch.path()).unwrap();
        let committed = fs::read(dir.join("manifest.json")).unwrap();
        let regenerated = fs::read(scratch.path().join("manifest.json")).unwrap();
        assert_eq!(
            committed, regenerated,
            "regenerating the composed-noise manifest must be byte-identical (git-clean)"
        );
        // The regenerated manifest is self-consistent: it verifies its files.
        crate::verify_fixtures(scratch.path()).unwrap();
    }

    #[test]
    fn tamper_negative_control_detects_corruption() {
        let dir = fixtures_dir().join("composed-noise");
        require_fixture(&dir);
        tamper_negative_control(&dir).expect("tamper must be detected");
    }

    /// Write a composed-noise golden derived from the committed fixture with one
    /// hostile mutation into a fresh scratch dir (no manifest). A `capture`
    /// failure on it is then provably the semantic gate — the manifest is only
    /// written when the output validates, so its absence is proof of rejection.
    fn write_hostile_golden<F: FnOnce(&mut ComposedNoise)>(
        tag: &str,
        mutate: F,
    ) -> tempfile::TempDir {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let dir = scratch(tag);
        let mut fixture = load(&src).unwrap();
        mutate(&mut fixture);
        fs::write(
            dir.path().join(FIXTURE_BASENAME),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
        dir
    }

    /// A fresh capture that fails the semantic gate must never write a manifest:
    /// `regenerate_manifest` (which derives the hash from the golden) is only
    /// reached for valid output. A hostile NaN density is the value_bits gate.
    #[test]
    fn capture_rejects_nan_density_without_rewriting_manifest() {
        let dir = write_hostile_golden("capture-nan", |f| {
            f.density[0].density.value = serde_json::json!("NaN");
        });
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "NaN density must be rejected, got {result:?}"
        );
        assert!(
            !dir.path().join("manifest.json").exists(),
            "capture must not write a manifest for a NaN-density golden"
        );
    }

    /// A wrong seed is drift, not the pinned golden: the semantic gate rejects
    /// it, and an already-committed manifest is left byte-identical — never
    /// clobbered with a hash of the invalid output.
    #[test]
    fn capture_rejects_wrong_seed_without_clobbering_committed_manifest() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let dir = scratch("capture-seed");
        fs::copy(src.join("manifest.json"), dir.path().join("manifest.json")).unwrap();
        let mut fixture = load(&src).unwrap();
        fixture.seed = 43;
        fs::write(
            dir.path().join(FIXTURE_BASENAME),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
        let committed = fs::read(src.join("manifest.json")).unwrap();
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "wrong-seed output must be rejected, got {result:?}"
        );
        assert_eq!(
            fs::read(dir.path().join("manifest.json")).unwrap(),
            committed,
            "capture must not clobber the committed manifest with a wrong-seed golden"
        );
    }

    /// A broken #175 matrix shape (a missing density row) must fail capture.
    #[test]
    fn capture_rejects_broken_175_matrix_without_rewriting_manifest() {
        let dir = write_hostile_golden("capture-shape", |f| {
            f.density.truncate(79);
        });
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "truncated density matrix must be rejected, got {result:?}"
        );
        assert!(!dir.path().join("manifest.json").exists());
    }

    /// Drifted FULL_CHUNK_STEP reachability (a probe against a different Paper)
    /// must fail capture.
    #[test]
    fn capture_rejects_wrong_full_chunk_step_without_rewriting_manifest() {
        let dir = write_hostile_golden("capture-step", |f| {
            f.full_chunk_step.level = 32;
        });
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "drifted reachability must be rejected, got {result:?}"
        );
        assert!(!dir.path().join("manifest.json").exists());
    }

    /// A capture against a non-pinned generator is drift, not the overworld
    /// NOISE golden: the provenance gate rejects it before the manifest rewrite.
    #[test]
    fn capture_rejects_wrong_generator_without_rewriting_manifest() {
        let dir = write_hostile_golden("capture-generator", |f| {
            f.generator = "amplified".into();
        });
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "non-normal generator must be rejected, got {result:?}"
        );
        assert!(!dir.path().join("manifest.json").exists());
    }

    /// A capture recorded against a different Paper commit is drift, not the
    /// pinned golden: the provenance gate rejects it before the manifest rewrite.
    #[test]
    fn capture_rejects_wrong_paper_pin_without_rewriting_manifest() {
        let dir = write_hostile_golden("capture-paper", |f| {
            f.paper = "26.2-DEV-main@badbeef".into();
        });
        let result = capture(dir.path());
        assert!(
            matches!(result, Err(Error::Manifest(_))),
            "non-pinned Paper commit must be rejected, got {result:?}"
        );
        assert!(!dir.path().join("manifest.json").exists());
    }

    /// The happy path without a live Java probe: capturing the committed golden
    /// (a valid fresh output) rewrites the manifest byte-identical to the
    /// committed one (git-clean), and the captured tree verifies green.
    #[test]
    fn capture_commits_valid_golden_with_byte_identical_manifest() {
        let src = fixtures_dir().join("composed-noise");
        require_fixture(&src);
        let dir = scratch("capture-valid");
        fs::copy(
            src.join(FIXTURE_BASENAME),
            dir.path().join(FIXTURE_BASENAME),
        )
        .unwrap();
        capture(dir.path()).expect("a valid fresh golden must be capturable");
        let committed = fs::read(src.join("manifest.json")).unwrap();
        let written = fs::read(dir.path().join("manifest.json")).unwrap();
        assert_eq!(written, committed);
        verify_composed_noise(dir.path()).expect("the captured tree must verify green");
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
