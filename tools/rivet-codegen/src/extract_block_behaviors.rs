//! `rivet-codegen extract-block-behaviors` — dump the compact per-`StateId`
//! worldgen/heightmap/lighting behavior table from the real Paper 26.2
//! block-state registry (issue #228).
//!
//! The behaviors the worldgen/heightmap/lighting surfaces consume
//! (`isAir`, `blocksMotion`, `isSolidRender`, `canOcclude`,
//! `useShapeForLightOcclusion`, `propagatesSkylightDown`, `getLightDampening`,
//! `getLightEmission`, `isRandomlyTicking`, fluid emptiness, map color id) are
//! *state*-dependent — concrete blocks override the `BlockBehaviour`
//! defaults (e.g. `LeavesBlock.getLightDampening`, `RedstoneLampBlock`'s
//! lit-conditional light level, per-`waterlogged` map color). They cannot be
//! derived from `data/reports/blocks.json` (which carries only ids + property
//! shapes), so this helper (`src/java/BlockBehaviourProbe.java`) boots the real
//! registry, evaluates every one of the 32,366 states through its cached
//! accessors (the `Blocks` static init `initCache`s every state against
//! `EmptyBlockGetter`), and writes the run-length-encoded words the generator
//! bakes into `generated/block_behaviors.rs`.
//!
//! The dump is deterministic (verified byte-identical across independent runs),
//! so the committed fixture is the no-drift baseline. `probe-block-behaviors`
//! re-runs this against the live jar and requires byte-identity.
//!
//! Output: `data/block_behaviors.json`, consumed by `generate` (see
//! [`crate::block_behaviors`]).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::extract;

/// Canonical output path for the extracted behavior-table fixture.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/block_behaviors.json")
}

/// Compile + run `BlockBehaviourProbe` against the bundler classpath, writing
/// the fixture JSON to `output`, and return the helper's captured stdout (the
/// anchor lines + `PROBE OK`). Shared by `extract-block-behaviors` (which
/// echoes it) and the live probe (`probe-block-behaviors`, which asserts it +
/// byte-identity with the committed fixture).
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<String> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = cache.join("classpath");
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("blockbehaviourprobe");
    fs::create_dir_all(&helper_dir).context("create block-behaviors helper dir")?;
    let helper_src = include_str!("java/BlockBehaviourProbe.java");
    let helper_file = helper_dir.join("BlockBehaviourProbe.java");
    fs::write(&helper_file, helper_src).context("write BlockBehaviourProbe.java")?;
    extract::run_cmd(
        &javac,
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
        "compile BlockBehaviourProbe.java",
    )?;

    // Quiet log4j down so stdout only carries the probe's key=value lines.
    let log4j_off = cache.join("log4j2-off.xml");
    fs::write(
        &log4j_off,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#,
    )
    .context("write log4j2-off.xml")?;

    let classpath_arg = format!("{classpath}:{}", helper_dir.display());
    let log4j_arg = format!("-Dlog4j.configurationFile={}", log4j_off.display());
    let out = extract::run_cmd_capture(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "BlockBehaviourProbe",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
        ],
        "run BlockBehaviourProbe",
    )?;

    if !out.contains("PROBE OK") {
        anyhow::bail!(
            "BlockBehaviourProbe did not report PROBE OK — the live Paper state \
             behaviors disagree with the probe's invariants.\n{out}"
        );
    }
    Ok(out)
}

/// Resolve the server jar extracted from the exact bundler used by the probe.
/// Never substitute a separately materialized jar: an override bundler is the
/// source of both the runtime bytes and the recorded provenance.
fn source_jar(repo_root: &Path, bundler: &Path) -> Result<PathBuf> {
    let cache = repo_root.join("tools/rivet-codegen/.cache/classpath");
    let (_, rel) = extract::read_versions_list(bundler, &cache)?;
    let jar = cache.join("META-INF/versions").join(&rel);
    anyhow::ensure!(
        jar.is_file(),
        "probe server jar not found at {} after extracting {}",
        jar.display(),
        bundler.display()
    );
    Ok(jar)
}

/// Write `data/block_behaviors.manifest.json`: the source provenance (same
/// shape as the reports/biomes manifest) + the fixture's sha256, so the codegen
/// can pin the fixture to its source.
fn write_manifest(repo_root: &Path, output: &Path, bundler: &Path) -> Result<()> {
    let jar = source_jar(repo_root, bundler)?;
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
        generator: "BlockBehaviourProbe (Bootstrap + Block.BLOCK_STATE_REGISTRY)".to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write block_behaviors.manifest.json")?;
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
    // Echo the probe's anchor lines (the user-visible confirmation of the live
    // Paper load: state count, run count, anchor words).
    print!("{out}");
    println!(
        "Wrote block behavior table ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_jar_uses_override_runtime_not_materialized_jar() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("tools/rivet-codegen/.cache/classpath");
        let versions = cache.join("META-INF/versions");
        fs::create_dir_all(&versions).unwrap();
        fs::write(
            cache.join("META-INF/versions.list"),
            "sha1 26.2 server-26.2.jar\n",
        )
        .unwrap();
        let override_jar = root.path().join("override-paper-bundler.jar");
        fs::write(&override_jar, b"override bundler").unwrap();
        let override_server = versions.join("server-26.2.jar");
        fs::write(&override_server, b"override server").unwrap();

        let materialized = crate::reports::default_jar(root.path());
        fs::create_dir_all(materialized.parent().unwrap()).unwrap();
        fs::write(materialized, b"materialized server").unwrap();

        let selected = source_jar(root.path(), &override_jar).unwrap();
        assert_eq!(fs::read(selected).unwrap(), b"override server");
    }
}
