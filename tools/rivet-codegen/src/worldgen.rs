//! `rivet-codegen generate` worldgen half — consume the deterministic
//! `data/worldgen.json` fixture (produced by `extract-worldgen`, see
//! [`crate::extract_worldgen`]) and emit `generated/worldgen.rs` (issue #354).
//!
//! The emitted table holds the three surfaces #177/#178 consume:
//!
//! - `NOISE_BY_NAME` / `NOISE_BY_ID` / `NOISE_AMPLITUDES` — the
//!   `minecraft:worldgen/noise` registry (`NormalNoise.NoiseParameters`), dense
//!   `0..n`. Each entry carries `first_octave` and the ordered `amplitudes`.
//!   Noise ids are assigned at runtime from a `TreeMap<Identifier, Resource>`
//!   sorted by `Identifier` compareTo (path first, then namespace) — id 0 is
//!   `minecraft:aquifer_barrier`, alphabetical.
//! - `BIOME_CLIMATE` — the per-biome climate configuration
//!   (`Biome.ClimateSettings`: temperature, downfall, has_precipitation,
//!   temperature_modifier) keyed by the `minecraft:worldgen/biome` name, the
//!   same element set + ids as `generated/biomes.rs`.
//! - `BIOME_SOURCE_PARAMETER_POINTS` — the multi-noise biome-source preset
//!   parameter points (`MultiNoiseBiomeSourceParameterList.knownPresets`):
//!   overworld (`OverworldBiomeBuilder`) + nether (the inline list). Each point
//!   is `(biome, {temperature, humidity, continentalness, erosion, depth,
//!   weirdness} min/max, offset)` with the parameter spans as the *quantized
//!   longs* (`Climate.quantizeCoord`, i.e. `(long)(coord * 10000.0F)`) exactly
//!   as stored in the runtime `Climate.ParameterPoint` — no float round-trip.
//!   Point order is the builder's fixed value order (never sorted): the R-tree
//!   `findValue` tie-breaks on it, so it is part of the climate semantics.
//!
//! Determinism: element tables are re-ordered by id (the fixture is read
//! order-insensitively, never trusted by key order), presets are sorted by id,
//! and each point list preserves the builder order. Regeneration is
//! byte-idempotent.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// Ground-truth anchors a live Paper 26.2 load must reproduce. The Java probe
/// asserts these against the running JVM; the codegen asserts them against the
/// fixture, so a fixture from a different jar or a hand-edited one fails
/// generation.
const ANCHORS: &[(&str, usize)] = &[
    ("minecraft:worldgen/noise", 63), // noise registry size
    ("minecraft:worldgen/biome", 66), // biome registry size
    ("minecraft:nether", 5),          // multi-noise nether preset point count
    ("minecraft:overworld", 7594),    // multi-noise overworld preset point count
];

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/worldgen.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One validated noise registry entry.
#[derive(Debug)]
struct Noise {
    name: String,
    id: u16,
    first_octave: i8,
    amplitudes: Vec<f64>,
}

/// One validated biome climate entry.
#[derive(Debug)]
struct BiomeClimate {
    name: String,
    id: u16,
    temperature: f32,
    downfall: f32,
    has_precipitation: bool,
    temperature_modifier: String,
}

/// A quantized parameter span, as stored in the runtime `Climate.Parameter`.
#[derive(Debug)]
struct Span {
    min: i64,
    max: i64,
}

/// One validated multi-noise preset parameter point.
#[derive(Debug)]
struct ParameterPoint {
    biome: String,
    temperature: Span,
    humidity: Span,
    continentalness: Span,
    erosion: Span,
    depth: Span,
    weirdness: Span,
    offset: i64,
}

/// A validated preset's point list, in the builder's fixed value order.
#[derive(Debug)]
struct Preset {
    id: String,
    points: Vec<ParameterPoint>,
}

pub fn run(input_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let input = match input_flag {
        Some(p) => p.to_path_buf(),
        None => default_input(&repo_root),
    };
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let json = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", input.display()))?;
    let (noises, climates, presets, _probe) = validate(&root)?;
    // Cross-check the biome climate surface against the already-emitted
    // `minecraft:worldgen/biome` element table (issue #49) so the two fixtures
    // cannot drift apart.
    let biome_ids = read_biome_ids(&repo_root)?;
    check_biome_surface_matches(&climates, &biome_ids)?;
    let source = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("worldgen.rs"),
        render(&noises, &climates, &presets, &source),
    )
    .context("write generated/worldgen.rs")?;

    let point_total: usize = presets.iter().map(|p| p.points.len()).sum();
    println!(
        "Wrote {} noises + {} biome climates + {} presets / {} parameter points -> {}",
        noises.len(),
        climates.len(),
        presets.len(),
        point_total,
        output.display()
    );
    Ok(())
}

/// The counts the extractor recorded for its live Paper load (see the `probe`
/// object in the fixture). `validate` checks the recorded values against the
/// parsed structures and the anchors.
#[derive(Debug)]
struct ProbeCounts {
    noise_count: usize,
    biome_count: usize,
    preset_count: usize,
    nether_point_count: usize,
    overworld_point_count: usize,
}

/// The fully-validated fixture tables (the value every validation entry point
/// returns).
type Validated = (Vec<Noise>, Vec<BiomeClimate>, Vec<Preset>, ProbeCounts);

/// Full validation for the committed fixture: structural validation + the
/// live-Paper anchor counts.
fn validate(root: &Value) -> Result<Validated> {
    let (noises, climates, presets, probe_actual) = validate_structural(root)?;

    // Anchor check: table sizes + point counts must match the live-Paper ground
    // truth (a fixture from a different jar fails here).
    if noises.len() != ANCHORS[0].1 {
        bail!(
            "anchor drift: `minecraft:worldgen/noise` has {} elements but a live Paper 26.2 load has {}",
            noises.len(),
            ANCHORS[0].1
        );
    }
    if climates.len() != ANCHORS[1].1 {
        bail!(
            "anchor drift: `minecraft:worldgen/biome` has {} climate entries but a live Paper 26.2 load has {}",
            climates.len(),
            ANCHORS[1].1
        );
    }
    for preset in &presets {
        let expected = ANCHORS
            .iter()
            .find(|(id, _)| id == &preset.id)
            .map(|(_, n)| *n);
        if let Some(expected) = expected
            && preset.points.len() != expected
        {
            bail!(
                "anchor drift: preset `{}` has {} points but a live Paper 26.2 load has {expected}",
                preset.id,
                preset.points.len()
            );
        }
    }

    Ok((noises, climates, presets, probe_actual))
}

/// Structural validation (independent of the live-Paper anchors): malformed
/// names, sparse/non-contiguous/duplicate ids, malformed values, unknown
/// presets, and the probe-count internal consistency.
fn validate_structural(root: &Value) -> Result<Validated> {
    let object = root
        .as_object()
        .context("worldgen.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "generator"
                | "minecraft_version"
                | "protocol_version"
                | "world_version"
                | "noise"
                | "biomes"
                | "presets"
                | "probe"
        ) {
            bail!("worldgen.json has unexpected top-level field `{field}`");
        }
    }
    let probe = object
        .get("probe")
        .and_then(Value::as_object)
        .context("worldgen.json is missing the `probe` object")?;
    for field in probe.keys() {
        if !matches!(
            field.as_str(),
            "noise_count"
                | "biome_count"
                | "preset_count"
                | "nether_point_count"
                | "overworld_point_count"
        ) {
            bail!("worldgen.json `probe` has unexpected field `{field}`");
        }
    }
    let probe_counts: HashMap<&str, usize> = probe
        .iter()
        .map(|(k, v)| {
            v.as_u64()
                .map(|n| (k.as_str(), n as usize))
                .with_context(|| format!("worldgen.json `probe.{k}` is not a count"))
        })
        .collect::<Result<_>>()?;
    let _mc = object
        .get("minecraft_version")
        .and_then(Value::as_str)
        .context("worldgen.json is missing `minecraft_version`")?;
    for (field, min) in [("protocol_version", 0u64), ("world_version", 0u64)] {
        match object.get(field).and_then(Value::as_u64) {
            Some(v) if v >= min => {}
            Some(_) => bail!("worldgen.json `{field}` is out of range"),
            None => bail!("worldgen.json is missing `{field}`"),
        }
    }

    let noises = validate_noises(&object["noise"])?;
    let climates = validate_biomes(&object["biomes"])?;
    let presets = validate_presets(&object["presets"])?;

    // The probe counts recorded by the extractor must match the parsed
    // structures (internal consistency — a hand-edited fixture that bumps one
    // count without the other, or drifts from the tables, fails here).
    let probe_actual = ProbeCounts {
        noise_count: noises.len(),
        biome_count: climates.len(),
        preset_count: presets.len(),
        nether_point_count: presets
            .iter()
            .find(|p| p.id == "minecraft:nether")
            .map(|p| p.points.len())
            .unwrap_or(0),
        overworld_point_count: presets
            .iter()
            .find(|p| p.id == "minecraft:overworld")
            .map(|p| p.points.len())
            .unwrap_or(0),
    };
    let expected_probe = [
        ("noise_count", probe_actual.noise_count),
        ("biome_count", probe_actual.biome_count),
        ("preset_count", probe_actual.preset_count),
        ("nether_point_count", probe_actual.nether_point_count),
        ("overworld_point_count", probe_actual.overworld_point_count),
    ];
    for (key, actual) in expected_probe {
        match probe_counts.get(key) {
            Some(&v) if v == actual => {}
            Some(&v) => bail!(
                "worldgen.json `probe.{key}` is {v} but the fixture has {actual} (hand-edited fixture?)"
            ),
            None => bail!("worldgen.json `probe` is missing `{key}`"),
        }
    }

    Ok((noises, climates, presets, probe_actual))
}

/// The noise registry element table: `name -> { id, firstOctave, amplitudes[] }`.
fn validate_noises(value: &Value) -> Result<Vec<Noise>> {
    let obj = value
        .as_object()
        .context("`noise` element table must be a JSON object")?;
    let mut noises = Vec::with_capacity(obj.len());
    for (name, entry) in obj {
        crate::registries::validate_name("minecraft:worldgen/noise", name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("noise `{name}` entry must be a JSON object"))?;
        for field in entry.keys() {
            if !matches!(field.as_str(), "id" | "firstOctave" | "amplitudes") {
                bail!("noise `{name}` has unexpected field `{field}`");
            }
        }
        let id = parse_id("minecraft:worldgen/noise", name, &entry["id"])?;
        let first_octave = match entry.get("firstOctave").and_then(Value::as_i64) {
            Some(v) if i8::try_from(v).is_ok() => v as i8,
            _ => bail!("noise `{name}` `firstOctave` is not a valid i8"),
        };
        let amplitudes = entry
            .get("amplitudes")
            .and_then(Value::as_array)
            .with_context(|| format!("noise `{name}` `amplitudes` must be an array"))?;
        if amplitudes.is_empty() {
            bail!("noise `{name}` has empty `amplitudes`");
        }
        let mut vals = Vec::with_capacity(amplitudes.len());
        for (i, a) in amplitudes.iter().enumerate() {
            match a.as_f64() {
                Some(v) if v.is_finite() => vals.push(v),
                _ => bail!("noise `{name}` `amplitudes[{i}]` is not a finite number"),
            }
        }
        noises.push(Noise {
            name: name.clone(),
            id,
            first_octave,
            amplitudes: vals,
        });
    }
    noises.sort_unstable_by_key(|n| n.id);
    check_dense_bijection("minecraft:worldgen/noise", &noises)?;
    Ok(noises)
}

fn parse_id(registry: &str, name: &str, id: &Value) -> Result<u16> {
    let id = match id {
        Value::Number(n) => n.as_u64().with_context(|| {
            format!("element `{name}` in `{registry}` has a non-integer id ({n})")
        })?,
        _ => bail!("element `{name}` in `{registry}` has a non-numeric id"),
    };
    u16::try_from(id).with_context(|| {
        format!("element `{name}` in `{registry}` has an id outside the u16 range")
    })
}

/// Shared surface of the noise/biome element-table entries (dense id + unique
/// name), so the dense-bijection check can be written once.
trait DenseEntry {
    fn id(&self) -> u16;
    fn name(&self) -> &str;
}

impl DenseEntry for Noise {
    fn id(&self) -> u16 {
        self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}

impl DenseEntry for BiomeClimate {
    fn id(&self) -> u16 {
        self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}

/// Dense `0..n` id space with a unique name per id and a unique id per name.
fn check_dense_bijection(registry: &str, entries: &[impl DenseEntry]) -> Result<()> {
    for (i, e) in entries.iter().enumerate() {
        if e.id() as usize != i {
            bail!(
                "`{registry}` element ids are not contiguous 0..{}: expected {i} at index {i}, got {}",
                entries.len(),
                e.id()
            );
        }
    }
    let mut names: Vec<&str> = entries.iter().map(DenseEntry::name).collect();
    names.sort_unstable();
    for pair in names.windows(2) {
        if pair[0] == pair[1] {
            bail!("duplicate element name `{}` in `{registry}`", pair[0]);
        }
    }
    Ok(())
}

/// The biome climate table: `name -> { id, temperature, downfall, ... }`.
fn validate_biomes(value: &Value) -> Result<Vec<BiomeClimate>> {
    let obj = value
        .as_object()
        .context("`biomes` element table must be a JSON object")?;
    let mut climates = Vec::with_capacity(obj.len());
    for (name, entry) in obj {
        crate::registries::validate_name("minecraft:worldgen/biome", name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("biome `{name}` entry must be a JSON object"))?;
        for field in entry.keys() {
            if !matches!(
                field.as_str(),
                "id" | "temperature" | "downfall" | "has_precipitation" | "temperature_modifier"
            ) {
                bail!("biome `{name}` has unexpected field `{field}`");
            }
        }
        let id = parse_id("minecraft:worldgen/biome", name, &entry["id"])?;
        let temperature = parse_float(name, "temperature", &entry["temperature"])?;
        let downfall = parse_float(name, "downfall", &entry["downfall"])?;
        let has_precipitation = entry
            .get("has_precipitation")
            .and_then(Value::as_bool)
            .with_context(|| format!("biome `{name}` `has_precipitation` is not a boolean"))?;
        let temperature_modifier = entry
            .get("temperature_modifier")
            .and_then(Value::as_str)
            .with_context(|| format!("biome `{name}` `temperature_modifier` is not a string"))?;
        if !matches!(temperature_modifier, "none" | "frozen") {
            bail!(
                "biome `{name}` has unknown `temperature_modifier` `{temperature_modifier}`"
            );
        }
        climates.push(BiomeClimate {
            name: name.clone(),
            id,
            temperature,
            downfall,
            has_precipitation,
            temperature_modifier: temperature_modifier.to_string(),
        });
    }
    climates.sort_unstable_by_key(|c| c.id);
    check_dense_bijection("minecraft:worldgen/biome", &climates)?;
    Ok(climates)
}

/// Read a `f32` field. Values like `0.4F` are exact in `f64`/`f32`; a value
/// that is not finite (or not representable in `f32`) fails here.
fn parse_float(name: &str, field: &str, value: &Value) -> Result<f32> {
    let v = value
        .as_f64()
        .with_context(|| format!("biome `{name}` `{field}` is not a finite number"))?;
    if !v.is_finite() {
        bail!("biome `{name}` `{field}` is not finite");
    }
    let v32 = v as f32;
    if !v32.is_finite() {
        bail!("biome `{name}` `{field}` is outside the f32 range");
    }
    Ok(v32)
}

/// The multi-noise preset parameter points: `preset id -> [ {biome, spans, offset} ]`.
fn validate_presets(value: &Value) -> Result<Vec<Preset>> {
    let obj = value
        .as_object()
        .context("`presets` must be a JSON object")?;
    let mut presets = Vec::with_capacity(obj.len());
    for (id, points_value) in obj {
        crate::registries::validate_name("preset", id)?;
        let points = points_value
            .as_array()
            .with_context(|| format!("preset `{id}` must be an array"))?;
        if points.is_empty() {
            bail!("preset `{id}` has no points");
        }
        let mut parsed = Vec::with_capacity(points.len());
        for (i, point) in points.iter().enumerate() {
            parsed.push(validate_parameter_point(id, i, point)?);
        }
        presets.push(Preset { id: id.clone(), points: parsed });
    }
    presets.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    // Only the two known presets exist; a third would be a stale/mutated
    // fixture the anchors could not cover.
    let known: [&str; 2] = ["minecraft:nether", "minecraft:overworld"];
    if presets.len() != known.len() {
        bail!(
            "worldgen.json has {} presets but a live Paper 26.2 load has {}",
            presets.len(),
            known.len()
        );
    }
    for (p, expected) in presets.iter().zip(known.iter()) {
        if p.id != *expected {
            bail!(
                "worldgen.json preset `{}` is not the expected `{expected}` (unknown preset)",
                p.id
            );
        }
    }
    Ok(presets)
}

fn validate_parameter_point(preset: &str, idx: usize, point: &Value) -> Result<ParameterPoint> {
    let obj = point
        .as_object()
        .with_context(|| format!("preset `{preset}` point {idx} must be a JSON object"))?;
    for field in obj.keys() {
        if !matches!(
            field.as_str(),
            "biome"
                | "temperature"
                | "humidity"
                | "continentalness"
                | "erosion"
                | "depth"
                | "weirdness"
                | "offset"
        ) {
            bail!("preset `{preset}` point {idx} has unexpected field `{field}`");
        }
    }
    let biome = obj
        .get("biome")
        .and_then(Value::as_str)
        .with_context(|| format!("preset `{preset}` point {idx} is missing `biome`"))?;
    crate::registries::validate_name("minecraft:worldgen/biome", biome)?;
    let temperature = validate_span(preset, idx, "temperature", &obj["temperature"])?;
    let humidity = validate_span(preset, idx, "humidity", &obj["humidity"])?;
    let continentalness = validate_span(preset, idx, "continentalness", &obj["continentalness"])?;
    let erosion = validate_span(preset, idx, "erosion", &obj["erosion"])?;
    let depth = validate_span(preset, idx, "depth", &obj["depth"])?;
    let weirdness = validate_span(preset, idx, "weirdness", &obj["weirdness"])?;
    let offset = match obj.get("offset").and_then(Value::as_i64) {
        Some(v) => v,
        _ => bail!("preset `{preset}` point {idx} `offset` is not an integer"),
    };
    Ok(ParameterPoint {
        biome: biome.to_string(),
        temperature,
        humidity,
        continentalness,
        erosion,
        depth,
        weirdness,
        offset,
    })
}

fn validate_span(preset: &str, idx: usize, name: &str, value: &Value) -> Result<Span> {
    let obj = value
        .as_object()
        .with_context(|| format!("preset `{preset}` point {idx} `{name}` must be an object"))?;
    for field in obj.keys() {
        if !matches!(field.as_str(), "min" | "max") {
            bail!("preset `{preset}` point {idx} `{name}` has unexpected field `{field}`");
        }
    }
    let min = obj
        .get("min")
        .and_then(Value::as_i64)
        .with_context(|| format!("preset `{preset}` point {idx} `{name}.min` is not an integer"))?;
    let max = obj
        .get("max")
        .and_then(Value::as_i64)
        .with_context(|| format!("preset `{preset}` point {idx} `{name}.max` is not an integer"))?;
    if min > max {
        bail!("preset `{preset}` point {idx} `{name}` has min {min} > max {max}");
    }
    Ok(Span { min, max })
}

/// Read the already-emitted `minecraft:worldgen/biome` element table from
/// `generated/biomes.rs`'s fixture so the two surfaces cannot drift.
fn read_biome_ids(repo_root: &Path) -> Result<HashMap<String, u16>> {
    let path = repo_root.join(crate::biomes_tags::default_input(repo_root));
    let json = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", path.display()))?;
    let biomes = root
        .get("biomes")
        .and_then(Value::as_object)
        .context("biomes_tags.json is missing `biomes`")?;
    let mut out = HashMap::with_capacity(biomes.len());
    for (name, id) in biomes {
        let id = id
            .as_u64()
            .with_context(|| format!("biome `{name}` in biomes_tags.json has a non-integer id"))?;
        out.insert(name.clone(), u16::try_from(id).context("biome id out of u16 range")?);
    }
    Ok(out)
}

/// The biome climate surface and the #49 biome element table must agree on name
/// set + ids (two independent captures of the same registry).
fn check_biome_surface_matches(climates: &[BiomeClimate], biome_ids: &HashMap<String, u16>) -> Result<()> {
    if climates.len() != biome_ids.len() {
        bail!(
            "worldgen.json has {} biome climate entries but biomes_tags.json has {} biome ids",
            climates.len(),
            biome_ids.len()
        );
    }
    for c in climates {
        match biome_ids.get(&c.name) {
            Some(&id) if id == c.id => {}
            Some(&id) => bail!(
                "biome climate/table id mismatch for `{}`: worldgen.json {id} vs biomes_tags.json {id}",
                c.name
            ),
            None => bail!(
                "biome climate `{}` is absent from the biomes_tags.json biome table",
                c.name
            ),
        }
    }
    Ok(())
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/worldgen.manifest.json`, and the emitted header
/// carries that provenance (jar identity + MC/proto/world versions).
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("worldgen.manifest.json"))
        .with_context(|| format!("{} has no parent dir", input.display()))?;
    let manifest_json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "read {} (expected next to the pinned fixture)",
            manifest_path.display()
        )
    })?;
    let manifest: FixtureManifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let actual = crate::reports::sha256_hex(&bytes);
    if actual != manifest.file.sha256 {
        bail!(
            "worldgen.json does not match its provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen extract-worldgen` to refresh the pinned fixture",
            manifest.file.sha256,
            actual
        );
    }
    Ok(manifest.source)
}

#[derive(serde::Deserialize)]
struct FixtureManifest {
    source: SourceProvenance,
    file: FixtureFile,
}

#[derive(serde::Deserialize)]
struct FixtureFile {
    sha256: String,
}

/// Render `generated/worldgen.rs`.
fn render(
    noises: &[Noise],
    climates: &[BiomeClimate],
    presets: &[Preset],
    source: &SourceProvenance,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/worldgen.json\n\
         // (live Paper registry load via WorldgenDataExtractor; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/worldgen.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// Worldgen data surfaces #177/#178 consume (issue #354): the noise registry\n\
         // (`NormalNoise.NoiseParameters`), the per-biome climate configuration\n\
         // (`Biome.ClimateSettings`), and the multi-noise biome-source preset parameter\n\
         // points (`MultiNoiseBiomeSourceParameterList.knownPresets`). All three are\n\
         // datapack-loaded or hardcoded in Paper — the values here are extracted from a\n\
         // live Paper 26.2 load, never hand-typed. Parameter spans are the quantized\n\
         // longs (`Climate.quantizeCoord`, i.e. `(long)(coord * 10000.0F)`) exactly as\n\
         // stored in the runtime `Climate.ParameterPoint`.\n\n",
    );

    out.push_str(
        "/// A quantized parameter span, exactly as stored in the runtime\n\
         /// `Climate.Parameter`: `min`/`max` are `(long)(coord * 10000.0F)`. A point\n\
         /// span matches when the target's quantized coordinate lies in `[min, max]`.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct ParameterPoint {\n\
         \x20   pub biome: &'static str,\n\
         \x20   pub temperature: (i64, i64),\n\
         \x20   pub humidity: (i64, i64),\n\
         \x20   pub continentalness: (i64, i64),\n\
         \x20   pub erosion: (i64, i64),\n\
         \x20   pub depth: (i64, i64),\n\
         \x20   pub weirdness: (i64, i64),\n\
         \x20   pub offset: i64,\n\
         }\n\n\
         /// Per-biome climate configuration (`Biome.ClimateSettings`), as a plain data\n\
         /// struct so the climate table can be read directly by the biome source.\n\
         #[derive(Clone, Copy, Debug, PartialEq)]\n\
         pub struct BiomeClimate {\n\
         \x20   pub id: u16,\n\
         \x20   pub temperature: f32,\n\
         \x20   pub downfall: f32,\n\
         \x20   pub has_precipitation: bool,\n\
         \x20   pub temperature_modifier: &'static str,\n\
         }\n\n",
    );

    out.push_str(&render_noises(noises));
    out.push_str(&render_biome_climates(climates));
    out.push_str(&render_presets(presets));
    out
}

fn render_noises(noises: &[Noise]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "/// `minecraft:worldgen/noise` — noise name -> registry id (dense `0..{}`).\n\
         /// Noise ids are assigned at runtime from a `TreeMap<Identifier, Resource>`\n\
         /// sorted by `Identifier` compareTo (path first, then namespace) — id 0 is\n\
         /// `minecraft:aquifer_barrier`, alphabetical.\n",
        noises.len()
    ));
    out.push_str("pub static NOISE_BY_NAME: phf::Map<&'static str, u16> = phf::phf_map! {\n");
    for n in noises {
        out.push_str(&format!("    {:?} => {}u16,\n", n.name, n.id));
    }
    out.push_str("};\n\n");

    out.push_str(
        "/// `minecraft:worldgen/noise` — noise names indexed by registry id (id == index).\n",
    );
    out.push_str("pub static NOISE_BY_ID: &[&str] = &[\n");
    for n in noises {
        out.push_str(&format!("    {:?}, // {}\n", n.name, n.id));
    }
    out.push_str("];\n\n");

    out.push_str(
        "/// `minecraft:worldgen/noise` — per-noise `(first_octave, &[amplitudes])` indexed\n\
         /// by registry id (id == index). `first_octave` is the `NormalNoise` first octave\n\
         /// (`-` for a finer octave); amplitudes are the ordered octave amplitudes.\n",
    );
    out.push_str("pub static NOISE_AMPLITUDES: &[(i8, &[f64])] = &[\n");
    for n in noises {
        let amps = n
            .amplitudes
            .iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("    ({}, &[{amps}]), // {}\n", n.first_octave, n.name));
    }
    out.push_str("];\n\n");

    out
}

fn render_biome_climates(climates: &[BiomeClimate]) -> String {
    let mut out = String::new();
    out.push_str(
        "/// Per-biome climate configuration (`Biome.ClimateSettings`): temperature,\n\
         /// downfall, `has_precipitation`, and `temperature_modifier` (none|frozen), keyed\n\
         /// by the `minecraft:worldgen/biome` name. Same element set + ids as\n\
         /// `BIOME_BY_NAME`/`BIOME_BY_ID` in biomes.rs (a live-load cross-check pins this).\n",
    );
    out.push_str("pub static BIOME_CLIMATE: phf::Map<&'static str, BiomeClimate> = phf::phf_map! {\n");
    for c in climates {
        let precip = if c.has_precipitation { "true" } else { "false" };
        out.push_str(&format!(
            "    {:?} => BiomeClimate {{ id: {}u16, temperature: {}, downfall: {}, has_precipitation: {precip}, temperature_modifier: {:?} }},\n",
            c.name,
            c.id,
            render_f32(c.temperature),
            render_f32(c.downfall),
            c.temperature_modifier
        ));
    }
    out.push_str("};\n\n");

    out.push_str(
        "/// Per-biome climate configuration, in registry-id order (id == index).\n",
    );
    out.push_str("pub static BIOME_CLIMATE_BY_ID: &[BiomeClimate] = &[\n");
    for c in climates {
        let precip = if c.has_precipitation { "true" } else { "false" };
        out.push_str(&format!(
            "    BiomeClimate {{ id: {}u16, temperature: {}, downfall: {}, has_precipitation: {precip}, temperature_modifier: {:?} }}, // {}\n",
            c.id,
            render_f32(c.temperature),
            render_f32(c.downfall),
            c.temperature_modifier,
            c.name
        ));
    }
    out.push_str("];\n\n");

    out
}

/// Render an `f32` literal losslessly. `f32` values extracted from a Paper
/// float round-trip through the fixture JSON as the shortest string that
/// round-trips, so `{v:?}` (Rust's float `Debug`, shortest-roundtrip and
/// always decimal-pointed) reproduces the exact bits and reads as a float
/// literal — `Display` would emit `2` for `2.0f32`, an integer literal.
fn render_f32(v: f32) -> String {
    format!("{v:?}")
}

fn render_presets(presets: &[Preset]) -> String {
    let mut out = String::new();
    for preset in presets {
        out.push_str(&render_preset(preset));
    }
    out
}

fn render_preset(preset: &Preset) -> String {
    let const_name = preset_const_name(&preset.id);
    let mut out = String::new();
    out.push_str(&format!(
        "/// The `{}` multi-noise biome-source preset parameter points\n\
         /// (`MultiNoiseBiomeSourceParameterList.knownPresets`), in the builder's fixed\n\
         /// value order (never sorted — the R-tree `findValue` tie-breaks on it).\n",
        preset.id
    ));
    out.push_str(&format!("pub static {const_name}: &[ParameterPoint] = &[\n"));
    for p in &preset.points {
        out.push_str(&format!(
            "    ParameterPoint {{\n\
             \x20       biome: {:?},\n\
             \x20       temperature: ({}, {}),\n\
             \x20       humidity: ({}, {}),\n\
             \x20       continentalness: ({}, {}),\n\
             \x20       erosion: ({}, {}),\n\
             \x20       depth: ({}, {}),\n\
             \x20       weirdness: ({}, {}),\n\
             \x20       offset: {},\n\
             \x20   }},\n",
            p.biome,
            p.temperature.min,
            p.temperature.max,
            p.humidity.min,
            p.humidity.max,
            p.continentalness.min,
            p.continentalness.max,
            p.erosion.min,
            p.erosion.max,
            p.depth.min,
            p.depth.max,
            p.weirdness.min,
            p.weirdness.max,
            p.offset
        ));
    }
    out.push_str("];\n\n");
    out
}

/// `minecraft:overworld` -> `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS`;
/// `minecraft:nether` -> `NETHER_BIOME_SOURCE_PARAMETER_POINTS`.
fn preset_const_name(id: &str) -> String {
    let path = id.rsplit_once(':').map(|(_, p)| p).unwrap_or(id);
    format!("{}_BIOME_SOURCE_PARAMETER_POINTS", path.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal fixture that satisfies the structural checks but not the anchor
    /// cross-checks (which only the real fixture passes).
    fn fixture() -> Value {
        serde_json::json!({
            "generator": "WorldgenDataExtractor",
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "noise": {
                "minecraft:aquifer_barrier": { "id": 0, "firstOctave": -3, "amplitudes": [1.0] },
                "minecraft:badlands_pillar": { "id": 1, "firstOctave": -2, "amplitudes": [1.0, 1.0] }
            },
            "biomes": {
                "minecraft:badlands": { "id": 0, "temperature": 2.0, "downfall": 0.0, "has_precipitation": false, "temperature_modifier": "none" },
                "minecraft:bamboo_jungle": { "id": 1, "temperature": 0.95, "downfall": 0.9, "has_precipitation": true, "temperature_modifier": "frozen" }
            },
            "presets": {
                "minecraft:nether": [
                    { "biome": "minecraft:nether_wastes", "temperature": { "min": 0, "max": 0 }, "humidity": { "min": 0, "max": 0 }, "continentalness": { "min": 0, "max": 0 }, "erosion": { "min": 0, "max": 0 }, "depth": { "min": 0, "max": 0 }, "weirdness": { "min": 0, "max": 0 }, "offset": 0 }
                ],
                "minecraft:overworld": [
                    { "biome": "minecraft:mushroom_fields", "temperature": { "min": -10000, "max": 10000 }, "humidity": { "min": -10000, "max": 10000 }, "continentalness": { "min": -12000, "max": -10500 }, "erosion": { "min": -10000, "max": 10000 }, "depth": { "min": 0, "max": 0 }, "weirdness": { "min": -10000, "max": 10000 }, "offset": 0 }
                ]
            },
            "probe": {
                "noise_count": 2,
                "biome_count": 2,
                "preset_count": 2,
                "nether_point_count": 1,
                "overworld_point_count": 1
            }
        })
    }

    /// The minimal fixture satisfies the structural checks but not the anchor
    /// cross-checks (which only the real fixture passes). So the mutation tests
    /// drive `validate_structural`, and the anchor paths get their own tests.
    fn structural(v: &Value) -> (Vec<Noise>, Vec<BiomeClimate>, Vec<Preset>, ProbeCounts) {
        validate_structural(v).unwrap()
    }

    #[test]
    fn minimal_fixture_is_valid() {
        let (noises, climates, presets, probe) = structural(&fixture());
        assert_eq!(noises.len(), 2);
        assert_eq!(climates.len(), 2);
        assert_eq!(presets.len(), 2);
        assert_eq!(probe.noise_count, 2);
        // Noise entries round-trip in id order.
        assert_eq!(noises[0].name, "minecraft:aquifer_barrier");
        assert_eq!(noises[0].first_octave, -3);
        assert_eq!(noises[0].amplitudes, vec![1.0]);
        // Presets are sorted by id.
        assert_eq!(presets[0].id, "minecraft:nether");
        assert_eq!(presets[1].id, "minecraft:overworld");
    }

    #[test]
    fn unknown_preset_is_rejected() {
        let mut v = fixture();
        v["presets"]["minecraft:the_end"] = serde_json::json!([
            { "biome": "minecraft:the_end", "temperature": { "min": 0, "max": 0 }, "humidity": { "min": 0, "max": 0 }, "continentalness": { "min": 0, "max": 0 }, "erosion": { "min": 0, "max": 0 }, "depth": { "min": 0, "max": 0 }, "weirdness": { "min": 0, "max": 0 }, "offset": 0 }
        ]);
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("has 3 presets but a live Paper 26.2 load has 2"),
            "got: {err}"
        );
    }

    #[test]
    fn preset_span_min_over_max_is_rejected() {
        let mut v = fixture();
        v["presets"]["minecraft:nether"][0]["temperature"]["min"] = serde_json::json!(1);
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("min 1 > max 0"), "got: {err}");
    }

    #[test]
    fn unknown_temperature_modifier_is_rejected() {
        let mut v = fixture();
        v["biomes"]["minecraft:badlands"]["temperature_modifier"] =
            serde_json::json!("nope");
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("unknown `temperature_modifier`"), "got: {err}");
    }

    #[test]
    fn sparse_noise_ids_are_rejected() {
        let mut v = fixture();
        v["noise"]["minecraft:sparse"] =
            serde_json::json!({ "id": 3, "firstOctave": -1, "amplitudes": [1.0] });
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn point_biome_must_be_valid_name() {
        let mut v = fixture();
        v["presets"]["minecraft:nether"][0]["biome"] = serde_json::json!("no_namespace");
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("not a namespaced identifier"),
            "got: {err}"
        );
    }

    #[test]
    fn anchor_drift_fails() {
        // The real fixture alone passes the anchors; a structurally-valid
        // minimal fixture (2 noises vs the live 63) must fail `validate`.
        let err = validate(&fixture()).unwrap_err();
        assert!(err.to_string().contains("anchor drift"), "got: {err}");
    }

    #[test]
    fn cross_check_detects_climate_table_mismatch() {
        let climates = vec![BiomeClimate {
            name: "minecraft:badlands".to_string(),
            id: 0,
            temperature: 2.0,
            downfall: 0.0,
            has_precipitation: false,
            temperature_modifier: "none".to_string(),
        }];
        let mut ids = HashMap::new();
        ids.insert("minecraft:badlands".to_string(), 1u16); // id drift
        let err = check_biome_surface_matches(&climates, &ids).unwrap_err();
        assert!(err.to_string().contains("id mismatch"), "got: {err}");

        let mut renamed = HashMap::new();
        renamed.insert("minecraft:other".to_string(), 0u16);
        let err = check_biome_surface_matches(&climates, &renamed).unwrap_err();
        assert!(err.to_string().contains("absent from"), "got: {err}");
    }

    #[test]
    fn preset_const_naming() {
        assert_eq!(
            preset_const_name("minecraft:overworld"),
            "OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS"
        );
        assert_eq!(
            preset_const_name("minecraft:nether"),
            "NETHER_BIOME_SOURCE_PARAMETER_POINTS"
        );
    }

    #[test]
    fn render_f32_round_trips() {
        for v in [0.0f32, 0.4, 0.95, 2.0, 1.5, -0.5, 0.375] {
            let s = render_f32(v);
            let parsed: f32 = s.parse().unwrap();
            assert_eq!(parsed.to_bits(), v.to_bits(), "round-trip failed for {v}");
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let (noises, climates, presets, _) = structural(&fixture());
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let first = render(&noises, &climates, &presets, &source);
        let second = render(&noises, &climates, &presets, &source);
        assert_eq!(first, second);
        assert!(first.contains("MC 26.2, protocol 776, world 4903"));
        assert!(first.contains("NOISE_BY_NAME"));
        assert!(first.contains("\"minecraft:aquifer_barrier\" => 0u16"));
        assert!(first.contains("NOISE_AMPLITUDES"));
        assert!(first.contains("(-3, &[1.0])"));
        assert!(first.contains("BIOME_CLIMATE"));
        assert!(first.contains("temperature_modifier: \"frozen\""));
        assert!(first.contains("NETHER_BIOME_SOURCE_PARAMETER_POINTS"));
        assert!(first.contains("temperature: (-10000, 10000)"));
        assert!(first.contains("biome: \"minecraft:nether_wastes\""));
    }
}
