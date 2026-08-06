//! `rivet-codegen extract` — pull the vanilla block registry + block states out
//! of the real Paper 26.2 bundler jar.
//!
//! The block registry is not stored as JSON anywhere in the jar, so this shells
//! out to a small Java helper (`java/BlockDataExtractor.java`) that runs inside
//! the full server classpath and dumps `BuiltInRegistries.BLOCK` (numeric ids,
//! names, per-block state properties and their ordered value sets).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

/// Canonical output path for the extracted block-state registry.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/block_states.json")
}

/// Default bundler jar produced by a Paper `build` in the working tree.
fn default_bundler(repo_root: &Path) -> PathBuf {
    repo_root.join("working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar")
}

pub fn run(bundler_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = find_repo_root()?;
    let bundler = match bundler_flag {
        Some(p) => p.to_path_buf(),
        None => default_bundler(&repo_root),
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

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    fs::create_dir_all(&cache).context("create codegen cache dir")?;

    let classpath_dir = cache.join("classpath");
    extract_bundler(&bundler, &classpath_dir)?;

    let (version, server_jar_rel) = read_versions_list(&bundler, &classpath_dir)?;
    let server_jar = classpath_dir.join("META-INF/versions").join(&server_jar_rel);
    anyhow::ensure!(server_jar.is_file(), "server jar not found at {}", server_jar.display());

    let classpath = build_classpath(&classpath_dir, &server_jar)?;

    let (java, javac) = resolve_java()?;
    let helper_dir = cache.join("helper");
    fs::create_dir_all(&helper_dir)?;
    let helper_src = include_str!("java/BlockDataExtractor.java");
    let helper_file = helper_dir.join("BlockDataExtractor.java");
    fs::write(&helper_file, helper_src).context("write BlockDataExtractor.java")?;

    // Quiet log4j down so stdout only carries our "extracted N blocks" line.
    let log4j_off = cache.join("log4j2-off.xml");
    fs::write(
        &log4j_off,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#,
    )
    .context("write log4j2-off.xml")?;

    run_cmd(
        &javac,
        &["-cp", &classpath, "-d", helper_dir.to_str().unwrap(), helper_file.to_str().unwrap()],
        "compile BlockDataExtractor.java",
    )?;

    let classpath_arg = format!("{classpath}:{}", helper_dir.display());
    let log4j_arg = format!("-Dlog4j.configurationFile={}", log4j_off.display());
    run_cmd(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "BlockDataExtractor",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
        ],
        "run BlockDataExtractor",
    )?;

    anyhow::ensure!(
        output.is_file(),
        "extract finished but {} was not produced",
        output.display()
    );

    println!(
        "Wrote block registry ({} blocks, MC {}) to {}",
        serde_json::from_str::<crate::model::BlockRegistry>(&fs::read_to_string(&output)?)?.blocks.len(),
        version,
        output.display()
    );
    Ok(())
}

/// Locate the repo root (the dir whose Cargo.toml declares the `[workspace]`).
pub(crate) fn find_repo_root() -> Result<PathBuf> {
    // Anchor at the source dir of this binary: <repo>/tools/rivet-codegen.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = manifest_dir.as_path();
    loop {
        let workspace_toml = dir.join("Cargo.toml");
        if workspace_toml.is_file()
            && fs::read_to_string(&workspace_toml)
                .map(|t| t.contains("[workspace]"))
                .unwrap_or(false)
        {
            return Ok(dir.to_path_buf());
        }
        dir = dir
            .parent()
            .ok_or_else(|| anyhow!("could not find rivet repo root from {}", manifest_dir.display()))?;
    }
}

fn extract_bundler(bundler: &Path, classpath_dir: &Path) -> Result<()> {
    let marker = classpath_dir.join("META-INF/versions.list");
    let need_extract = !marker.is_file()
        || fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .zip(fs::metadata(bundler).and_then(|m| m.modified()).ok())
            .map(|(marker_mtime, bundler_mtime)| bundler_mtime > marker_mtime)
            .unwrap_or(true);
    if !need_extract {
        return Ok(());
    }

    fs::create_dir_all(classpath_dir).with_context(|| format!("create {}", classpath_dir.display()))?;
    run_cmd(
        &PathBuf::from("unzip"),
        &["-o", "-q", bundler.to_str().unwrap(), "-d", classpath_dir.to_str().unwrap(), "META-INF/versions/*", "META-INF/libraries/*"],
        "extract server + libraries from bundler jar",
    )
}

fn read_versions_list(bundler: &Path, classpath_dir: &Path) -> Result<(String, String)> {
    let marker = classpath_dir.join("META-INF/versions.list");
    let contents = if marker.is_file() {
        fs::read_to_string(&marker)?
    } else {
        // Marker may be from a previous run; re-read from the jar to be safe.
        let out = Command::new("unzip")
            .arg("-p")
            .arg(bundler)
            .arg("META-INF/versions.list")
            .output()
            .context("read META-INF/versions.list from bundler")?;
        if !out.status.success() {
            bail!("unzip -p failed to read META-INF/versions.list");
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let line = contents.lines().next().context("empty META-INF/versions.list")?;
    let mut fields = line.split_whitespace();
    let _sha1 = fields.next().context("missing sha1 in versions.list")?;
    let version = fields.next().context("missing version in versions.list")?.to_string();
    let rel_path = fields.next().context("missing server path in versions.list")?.to_string();
    Ok((version, rel_path))
}

/// Full classpath: server jar + every library jar under META-INF/libraries.
fn build_classpath(classpath_dir: &Path, server_jar: &Path) -> Result<String> {
    let mut jars: Vec<String> = vec![server_jar.display().to_string()];
    let libs = classpath_dir.join("META-INF/libraries");
    if libs.is_dir() {
        collect_jars(&libs, &mut jars)?;
    }
    if jars.len() <= 1 {
        bail!("no library jars found under {}", libs.display());
    }
    Ok(jars.join(":"))
}

fn collect_jars(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).context("read libraries dir")? {
        let path = entry?.path();
        if path.is_dir() {
            collect_jars(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "jar") {
            out.push(path.display().to_string());
        }
    }
    Ok(())
}

pub(crate) fn resolve_java() -> Result<(PathBuf, PathBuf)> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let bin = Path::new(&home).join("bin");
        let java = bin.join("java");
        let javac = bin.join("javac");
        if java.is_file() && javac.is_file() {
            return Ok((java, javac));
        }
    }
    Ok((PathBuf::from("java"), PathBuf::from("javac")))
}

fn run_cmd(program: &Path, args: &[&str], what: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("spawn {program:?} for {what}"))?;
    if !status.success() {
        bail!("{what} failed with {status}");
    }
    Ok(())
}
