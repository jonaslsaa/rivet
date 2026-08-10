//! `rivet-codegen extract-worldgen` — dump the deterministic MC 26.2 worldgen
//! noise registry, the per-biome climate configuration, and the multi-noise
//! biome-source preset parameter points from a live Paper registry load
//! (issue #354), following the #49 extract-biomes-tags pattern.
//!
//! The noise + biome registries are datapack-loaded (WORLDGEN_REGISTRIES), so
//! their ids are assigned at runtime by `ResourceManagerRegistryLoadTask` from a
//! `TreeMap<Identifier, Resource>` sorted by `Identifier` compareTo (path first,
//! then namespace) — id 0 is `minecraft:aquifer_barrier` for noise and
//! `minecraft:badlands` for biomes, alphabetical. The multi-noise preset
//! parameter points are hardcoded in
//! `MultiNoiseBiomeSourceParameterList.Preset` (overworld ->
//! `OverworldBiomeBuilder`, nether -> the inline list), so they are read through
//! the public static `knownPresets()` rather than invented or copied by hand.
//!
//! The helper (`src/java/WorldgenDataExtractor.java`) reproduces the same load
//! sequence as `BiomeTagExtractor`: vanilla pack source -> STATIC layer ->
//! `TagLoader.loadTagsForExistingRegistries` -> `buildUpdatedLookups` ->
//! `RegistryDataLoader.load(WORLDGEN_REGISTRIES)`. The dump is deterministic
//! (verified byte-identical across independent runs), so the committed fixture
//! is the no-drift baseline.
//!
//! Output: `data/worldgen.json`, consumed by `generate` (see
//! [`crate::worldgen`]).
//!
//! Requires the same runtime as `extract`: the bundler jar (`--bundler`,
//! default `working/Paper`), java + javac on PATH or JAVA_HOME, and unzip.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::extract;

/// Canonical output path for the extracted worldgen fixture.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/worldgen.json")
}

/// Compile + run `WorldgenDataExtractor` against the bundler classpath, writing
/// the fixture JSON to `output`, and return the helper's captured stdout (the
/// anchor lines + `PROBE OK`). Shared by `extract-worldgen` (which echoes it)
/// and the live probe (`probe-worldgen`, which asserts it + byte-identity with
/// the committed fixture).
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<String> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = cache.join("classpath");
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("worldgenextractor");
    fs::create_dir_all(&helper_dir).context("create worldgen helper dir")?;
    let helper_src = include_str!("java/WorldgenDataExtractor.java");
    let helper_file = helper_dir.join("WorldgenDataExtractor.java");
    fs::write(&helper_file, helper_src).context("write WorldgenDataExtractor.java")?;
    extract::run_cmd(
        &javac,
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
        "compile WorldgenDataExtractor.java",
    )?;

    // Quiet log4j down so stdout only carries the probe's key=value lines.
    let log4j_off = cache.join("log4j2-off.xml");
    if !log4j_off.is_file() {
        fs::write(
            &log4j_off,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#,
        )
        .context("write log4j2-off.xml")?;
    }

    let classpath_arg = format!("{classpath}:{}", helper_dir.display());
    let log4j_arg = format!("-Dlog4j.configurationFile={}", log4j_off.display());
    let out = extract::run_cmd_capture(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "WorldgenDataExtractor",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
        ],
        "run WorldgenDataExtractor",
    )?;

    anyhow::ensure!(
        output.is_file(),
        "extract-worldgen finished but {} was not produced",
        output.display()
    );
    Ok(out)
}

/// Resolve the source jar for provenance: `--bundler` points at the bundler,
/// but the pinned source identity is the materialized server jar (the same one
/// the report provenance records). Falls back to the bundler's extracted server
/// jar when no materialized run exists.
fn source_jar(repo_root: &Path, bundler: &Path) -> PathBuf {
    let materialized = crate::reports::default_jar(repo_root);
    if materialized.is_file() {
        return materialized;
    }
    // Fall back to the server jar extracted from the bundler (same bytes as the
    // materialized run when built from the same Paper tree).
    let cache = repo_root.join("tools/rivet-codegen/.cache/classpath");
    if let Ok((_, rel)) = extract::read_versions_list(bundler, &cache) {
        let candidate = cache.join("META-INF/versions").join(&rel);
        if candidate.is_file() {
            return candidate;
        }
    }
    materialized
}

/// Write `data/worldgen.manifest.json`: the source provenance (same shape as
/// the reports/biomes manifest) + the fixture's sha256, so the codegen can pin
/// the fixture to its source.
fn write_manifest(repo_root: &Path, output: &Path, bundler: &Path) -> Result<()> {
    let jar = source_jar(repo_root, bundler);
    if !jar.is_file() {
        // No jar to record provenance for (e.g. a test-only extraction); skip.
        return Ok(());
    }
    let mut source = crate::reports::capture_source(&jar, repo_root)?;
    // Record the canonical repo-relative source location (same as the reports
    // manifest) even when the bytes came from the bundler's extracted server jar
    // — the sha256 is the load-bearing identity; the path is context only.
    let canonical = crate::reports::default_jar(repo_root)
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source.jar.clone());
    source.jar = canonical;
    let bytes = fs::read(output).with_context(|| format!("read {}", output.display()))?;
    let manifest = FixtureManifest {
        generator: "WorldgenDataExtractor (Bootstrap + RegistryDataLoader + knownPresets)".to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write worldgen.manifest.json")?;
    Ok(())
}

#[derive(serde::Serialize)]
struct FixtureManifest {
    generator: String,
    source: crate::reports::SourceProvenance,
    file: FixtureFile,
}

#[derive(serde::Serialize)]
struct FixtureFile {
    bytes: u64,
    sha256: String,
}

pub fn run(bundler_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = extract::find_repo_root()?;
    let bundler = match bundler_flag {
        Some(p) => p.to_path_buf(),
        None => extract::default_bundler(&repo_root),
    };
    anyhow::ensure!(
        bundler.is_file(),
        "bundler jar not found at {} — pass --bundler or build Paper first (working/Paper/paper-server/build/libs)",
        bundler.display()
    );
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };
    if let Some(dir) = output.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }

    let out = run_extractor(&repo_root, &bundler, &output)?;
    write_manifest(&repo_root, &output, &bundler)?;
    // Echo the helper's anchor lines (the user-visible confirmation of the
    // live Paper load: noise count, biome count, preset point counts).
    print!("{out}");
    println!(
        "Wrote worldgen noise registry + biome climate + preset points ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}
