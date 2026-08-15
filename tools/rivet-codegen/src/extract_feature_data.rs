//! `rivet-codegen extract-feature-data` — dump the deterministic seed-42 feature
//! data foundation for the FEATURES checkpoint from a live Paper registry load
//! (`WorldgenFeatureDataExtractor.java`).
//!
//! The dump materializes:
//!   * `reachable_biomes` — the biome set that can drive FEATURES placement into
//!     the committed seed-42 grid {(3,3),(4,3),(3,4),(4,4)}: every chunk that can
//!     write into the grid (blockStateWriteRadius(1) writers = chunks 2..5) reads
//!     the biome map of its own 3x3 neighborhood, so the biome read set is chunks
//!     1..6. The biome source is sampled at every quart position and every Y quart
//!     (-64..319 blocks) because the depth parameter varies by Y.
//!   * `biomes` — the full `BiomeGenerationSettings` of EVERY overworld possible
//!     biome (the full `biomeSource.possibleBiomes()` list, 55 — the exact set
//!     Paper's FeatureSorter is built from, `ChunkGenerator.java` 97-100): id,
//!     carver identity names, and the per-step placed-feature lists (step
//!     order = `GenerationStep.Decoration` ordinal; holder-set order preserved).
//!   * `placed_features` / `configured_features` — the transitive closure of
//!     referenced registry entries, each stored as its full `RegistryOps`-encoded
//!     JSON (the datapack JSON shape: holder refs are strings, inline values are
//!     nested).
//!
//! The closure (what a future FEATURES port must be able to decode) starts from
//! every possible biome's direct per-step placed features — seeded from ALL 55
//! `biomes` step lists, not just the seed-42-reachable five — and grows to
//! fixpoint over the RegistryOps-encoded JSON of every configured feature: any
//! bare-string value that names a placed/configured registry entry is a holder
//! reference. Placed features are never walked for nested refs (their JSON is
//! only `{feature, placement}` — placement modifiers reference no features), so
//! the only placed refs come from configured-feature configs (e.g.
//! `random_selector` weights).
//!
//! Output: `data/feature_data.json` (+ `data/feature_data.manifest.json`
//! provenance), consumed by a later codegen slice that emits the Rust tables.
//! Requires the same runtime as `extract`: the bundler jar (`--bundler`, default
//! `working/Paper`), java + javac on PATH or JAVA_HOME, and unzip.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::extract;

/// The pinned Paper commit the fixture is captured against (the FEATURES oracle
/// pin; also recorded in `data/feature_data.manifest.json`'s `paper_git`).
pub const PAPER_PIN: &str = "26.2-DEV-main@0a99345";

/// Canonical output path for the extracted feature-data fixture.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/feature_data.json")
}

/// Compile + run `WorldgenFeatureDataExtractor` against the bundler classpath,
/// writing the fixture JSON to `output`, and return the helper's captured stdout
/// (the anchor lines). Shared by `extract-feature-data` (which echoes it) and
/// the live probe (`probe-feature-data`, which asserts byte-identity with the
/// committed fixture).
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<String> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = cache.join("classpath");
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("worldgenfeaturedataextractor");
    fs::create_dir_all(&helper_dir).context("create feature-data helper dir")?;
    let helper_src = include_str!("java/WorldgenFeatureDataExtractor.java");
    let helper_file = helper_dir.join("WorldgenFeatureDataExtractor.java");
    fs::write(&helper_file, helper_src).context("write WorldgenFeatureDataExtractor.java")?;
    extract::run_cmd(
        &javac,
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
        "compile WorldgenFeatureDataExtractor.java",
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
            "WorldgenFeatureDataExtractor",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
            "--paper",
            PAPER_PIN,
        ],
        "run WorldgenFeatureDataExtractor",
    )?;

    anyhow::ensure!(
        output.is_file(),
        "extract-feature-data finished but {} was not produced",
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

/// Write `data/feature_data.manifest.json`: the source provenance (same shape
/// as the reports manifest) + the fixture's sha256, so the codegen can pin the
/// fixture to its source.
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
        generator:
            "WorldgenFeatureDataExtractor (full overworld possible-biome + seed-42 feature closure)"
                .to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write feature_data.manifest.json")?;
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
    // Self-validate the fresh capture against the same contract the probe and
    // the (later) generation slice enforce: structure, order, closure, and the
    // manifest sha256 just written.
    crate::feature_data::validate(&output)?;
    // Echo the helper's anchor lines (the user-visible confirmation of the live
    // Paper load: reachable biome count, placed/configured feature counts).
    print!("{out}");
    println!(
        "Wrote seed-42 feature data ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}
