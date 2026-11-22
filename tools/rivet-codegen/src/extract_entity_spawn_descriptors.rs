//! `rivet-codegen extract-entity-spawn-descriptors` — boot Paper 26.2 and
//! extract the exact set of CREATURE `EntityType` spawn descriptors reachable
//! from overworld `possibleBiomes()` `MobSpawnSettings` into
//! `data/entity_spawn_descriptors.json` (+ sibling provenance manifest).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::extract;

/// Paper commit pin recorded in `data/entity_spawn_descriptors.json` and
/// verified by `probe-entity-spawn-descriptors` and `generate`.
pub(crate) const PAPER_PIN: &str = "26.2-DEV-main@0a99345";

/// Canonical generator label embedded in both the fixture JSON and manifest.
pub(crate) const GENERATOR_NAME: &str =
    "EntitySpawnDescriptorExtractor (possible-biome CREATURE entity spawn descriptors)";

pub(crate) fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/entity_spawn_descriptors.json")
}

/// Compile and run `EntitySpawnDescriptorExtractor.java` against the real
/// Paper 26.2 bundler jar, writing the deterministic JSON fixture to `output`.
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<String> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;
    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = extract::bundler_cache_dir(&cache, bundler)?;
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("entity-spawn-descriptor-extractor");
    fs::create_dir_all(&helper_dir).context("create entity-spawn-descriptor-extractor dir")?;
    let helper_src = include_str!("java/EntitySpawnDescriptorExtractor.java");
    let helper_java = helper_dir.join("EntitySpawnDescriptorExtractor.java");
    fs::write(&helper_java, helper_src).context("write EntitySpawnDescriptorExtractor.java")?;

    let sep = if cfg!(windows) { ";" } else { ":" };

    let compile = Command::new(&javac)
        .args(["-cp", &classpath, "-d"])
        .arg(&helper_dir)
        .arg(&helper_java)
        .output()
        .with_context(|| format!("failed to invoke {}", javac.display()))?;
    if !compile.status.success() {
        bail!(
            "javac failed on EntitySpawnDescriptorExtractor.java:\n{}\n{}",
            String::from_utf8_lossy(&compile.stdout),
            String::from_utf8_lossy(&compile.stderr),
        );
    }

    let log4j_xml = helper_dir.join("log4j2.xml");
    fs::write(
        &log4j_xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Configuration status=\"OFF\">\n\
           <Appenders><Null name=\"Null\"/></Appenders>\n\
           <Loggers><Root level=\"OFF\"><AppenderRef ref=\"Null\"/></Root></Loggers>\n\
         </Configuration>\n",
    )
    .context("write quiet log4j2.xml")?;

    let run_cp = format!("{}{sep}{classpath}", helper_dir.display());
    let log4j_arg = format!("-Dlog4j2.configurationFile={}", log4j_xml.display());
    let output_arg = output
        .to_str()
        .context("output path is not valid UTF-8")?
        .to_string();

    let run = Command::new(&java)
        .args([
            "-cp",
            &run_cp,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "EntitySpawnDescriptorExtractor",
            "--output",
            &output_arg,
            "--version",
            &version,
            "--paper",
            PAPER_PIN,
        ])
        .output()
        .with_context(|| format!("failed to invoke {}", java.display()))?;

    let out = String::from_utf8_lossy(&run.stdout).into_owned();
    let err = String::from_utf8_lossy(&run.stderr).into_owned();
    if !run.status.success() {
        bail!(
            "EntitySpawnDescriptorExtractor exited with {}:\nstdout:\n{out}\nstderr:\n{err}",
            run.status,
        );
    }
    anyhow::ensure!(
        output.is_file(),
        "extract-entity-spawn-descriptors finished but {} was not produced",
        output.display()
    );
    Ok(out)
}

/// Write `data/entity_spawn_descriptors.manifest.json`: source provenance
/// (same shape as `feature_data.manifest.json`) + fixture byte count & sha256.
fn write_manifest(repo_root: &Path, output: &Path, bundler: &Path) -> Result<()> {
    let jar = extract::server_jar_for_bundler(repo_root, bundler)?;
    let mut source = crate::reports::capture_source(&jar, repo_root)?;
    let canonical = crate::reports::default_jar(repo_root)
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source.jar.clone());
    source.jar = canonical;
    let bytes = fs::read(output).with_context(|| format!("read {}", output.display()))?;
    let manifest = FixtureManifest {
        generator: GENERATOR_NAME.to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write entity_spawn_descriptors.manifest.json")?;
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
    // Self-validate against schema, cross-checks, and manifest sha256.
    // Note: if this is the first extraction from a newly-built Paper jar whose
    // SHA is not yet pinned in `reports.rs`, callers can pin the SHA from the
    // generated manifest and re-run `probe-entity-spawn-descriptors`.
    if let Err(err) = crate::entity_spawn_descriptors::validate(&repo_root, &output) {
        eprintln!("warning: post-extract validation reported: {err:#}");
        return Err(err);
    }
    print!("{out}");
    println!(
        "Wrote CREATURE entity spawn descriptors ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}
