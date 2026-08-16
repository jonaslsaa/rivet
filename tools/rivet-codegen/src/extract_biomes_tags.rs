//! `rivet-codegen extract-biomes-tags` — dump the deterministic MC 26.2 biome id
//! table + the tag network-serialization content from a real Paper registry load
//! (issue #49), mirroring `extract`'s Java-helper-against-the-server-classpath
//! pattern.
//!
//! The biome registry is datapack-loaded (not in `BuiltInRegistries`), so its ids
//! are assigned at runtime by `ResourceManagerRegistryLoadTask` from a
//! `TreeMap<Identifier, Resource>` sorted by `Identifier` compareTo (path first,
//! then namespace) — id 0 is `minecraft:badlands`, alphabetical. The tag content
//! is exactly what `TagNetworkSerialization.serializeTagsToNetwork` emits for the
//! `ClientboundUpdateTagsPacket` (WORLDGEN networkable + STATIC registries with a
//! bound tag, each mapped to tag-location -> element ids in tag-file order).
//!
//! The helper (`src/java/BiomeTagExtractor.java`) reproduces `WorldLoader.load`:
//! vanilla pack source -> STATIC layer -> `TagLoader.loadTagsForExistingRegistries`
//! -> `buildUpdatedLookups` -> `RegistryDataLoader.load(WORLDGEN_REGISTRIES)` ->
//! `replaceFrom(WORLDGEN)` -> static `PendingTags.apply` ->
//! `serializeTagsToNetwork`. The dump is deterministic (verified byte-identical
//! across independent runs), so the committed fixture is the no-drift baseline.
//!
//! Output: `data/biomes_tags.json`, consumed by `generate` (see
//! [`crate::biomes_tags`]).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::extract;

/// Canonical output path for the extracted biome + tag fixture.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/biomes_tags.json")
}

/// Compile + run `BiomeTagExtractor` against the bundler classpath, writing the
/// fixture JSON to `output`, and return the helper's captured stdout (the
/// anchor lines + `PROBE OK`). Shared by `extract-biomes-tags` (which echoes it)
/// and the live probe (`probe-biomes-tags`, which asserts it + byte-identity
/// with the committed fixture).
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<String> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = cache.join("classpath");
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("biometagextractor");
    fs::create_dir_all(&helper_dir).context("create biomes-tags helper dir")?;
    let helper_src = include_str!("java/BiomeTagExtractor.java");
    let helper_file = helper_dir.join("BiomeTagExtractor.java");
    fs::write(&helper_file, helper_src).context("write BiomeTagExtractor.java")?;
    extract::run_cmd(
        &javac,
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
        "compile BiomeTagExtractor.java",
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
    // Capture the helper's stdout (the anchor lines + `PROBE OK`) so the live
    // probe can assert them; the JSON fixture itself is written to `output`.
    let out = extract::run_cmd_capture(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "BiomeTagExtractor",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
        ],
        "run BiomeTagExtractor",
    )?;

    anyhow::ensure!(
        output.is_file(),
        "extract-biomes-tags finished but {} was not produced",
        output.display()
    );
    Ok(out)
}

/// Write `data/biomes_tags.manifest.json`: the source provenance (same shape as
/// the reports manifest) + the fixture's sha256, so the codegen can pin the
/// fixture to its source.
fn write_manifest(repo_root: &Path, output: &Path, bundler: &Path) -> Result<()> {
    let jar = extract::server_jar_for_bundler(repo_root, bundler)?;
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
        generator: "BiomeTagExtractor (Bootstrap + RegistryDataLoader + TagNetworkSerialization)"
            .to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write biomes_tags.manifest.json")?;
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
    // live Paper load: biome count, tag-registry count, total tags).
    print!("{out}");
    println!(
        "Wrote biome id table + tag network content ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}
