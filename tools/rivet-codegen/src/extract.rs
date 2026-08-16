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

use anyhow::{Context, Result, anyhow, bail};

/// Canonical output path for the extracted block-state registry.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/block_states.json")
}

/// Default bundler jar produced by a Paper `build` in the working tree.
pub(crate) fn default_bundler(repo_root: &Path) -> PathBuf {
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
    let server_jar = classpath_dir
        .join("META-INF/versions")
        .join(&server_jar_rel);
    anyhow::ensure!(
        server_jar.is_file(),
        "server jar not found at {}",
        server_jar.display()
    );

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
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
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
        serde_json::from_str::<crate::model::BlockRegistry>(&fs::read_to_string(&output)?)?
            .blocks
            .len(),
        version,
        output.display()
    );
    Ok(())
}

/// Locate the repo root (the dir whose Cargo.toml declares the `[workspace]`).
pub(crate) fn find_repo_root() -> Result<PathBuf> {
    find_repo_root_from(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// Search upward from `start` for the dir whose Cargo.toml declares the real
/// `[workspace]` — one with members/excludes. A standalone `[workspace]` marker
/// (rivet-codegen's own manifest, an empty table used to root cargo in nested
/// worktrees) does not count, so this keeps walking up to the repository root.
pub(crate) fn find_repo_root_from(start: PathBuf) -> Result<PathBuf> {
    let mut dir = start.as_path();
    loop {
        let workspace_toml = dir.join("Cargo.toml");
        if workspace_toml.is_file()
            && fs::read_to_string(&workspace_toml)
                .map(|t| t.contains("[workspace]") && t.contains("members"))
                .unwrap_or(false)
        {
            return Ok(dir.to_path_buf());
        }
        dir = dir
            .parent()
            .ok_or_else(|| anyhow!("could not find rivet repo root from {}", start.display()))?;
    }
}

fn extract_bundler(bundler: &Path, classpath_dir: &Path) -> Result<()> {
    let marker = classpath_dir.join("META-INF/versions.list");
    let source_marker = classpath_dir.join(".bundler.sha256");
    let bundler_sha = crate::reports::sha256_hex(&fs::read(bundler).context("read bundler jar")?);
    let cache_matches = bundler_cache_matches(classpath_dir, &bundler_sha);
    if marker.is_file() && cache_matches {
        return Ok(());
    }

    if classpath_dir.is_dir() {
        fs::remove_dir_all(classpath_dir)
            .with_context(|| format!("clear stale bundler cache {}", classpath_dir.display()))?;
    }
    fs::create_dir_all(classpath_dir)
        .with_context(|| format!("create {}", classpath_dir.display()))?;
    run_cmd(
        &PathBuf::from("unzip"),
        &[
            "-o",
            "-q",
            bundler.to_str().unwrap(),
            "-d",
            classpath_dir.to_str().unwrap(),
            "META-INF/versions/*",
            "META-INF/libraries/*",
        ],
        "extract server + libraries from bundler jar",
    )?;
    fs::write(source_marker, bundler_sha).context("record bundler cache identity")
}

fn bundler_cache_matches(classpath_dir: &Path, bundler_sha: &str) -> bool {
    fs::read_to_string(classpath_dir.join(".bundler.sha256"))
        .map(|cached| cached.trim() == bundler_sha)
        .unwrap_or(false)
}

pub(crate) fn read_versions_list(bundler: &Path, classpath_dir: &Path) -> Result<(String, String)> {
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

    let line = contents
        .lines()
        .next()
        .context("empty META-INF/versions.list")?;
    let mut fields = line.split_whitespace();
    let _sha1 = fields.next().context("missing sha1 in versions.list")?;
    let version = fields
        .next()
        .context("missing version in versions.list")?
        .to_string();
    let rel_path = fields
        .next()
        .context("missing server path in versions.list")?
        .to_string();
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

pub(crate) fn server_jar_for_bundler(repo_root: &Path, bundler: &Path) -> Result<PathBuf> {
    let cache = repo_root.join("tools/rivet-codegen/.cache");
    fs::create_dir_all(&cache).context("create codegen cache dir")?;
    let classpath_dir = cache.join("classpath");
    extract_bundler(bundler, &classpath_dir)?;

    let (_, server_jar_rel) = read_versions_list(bundler, &classpath_dir)?;
    let server_jar = classpath_dir
        .join("META-INF/versions")
        .join(&server_jar_rel);
    anyhow::ensure!(
        server_jar.is_file(),
        "server jar not found at {} after extracting {}",
        server_jar.display(),
        bundler.display()
    );
    Ok(server_jar)
}

/// Unpack the bundler classpath + resolve java/javac, shared by `extract` and
/// `mth-gen`. Returns (classpath, java, javac).
pub(crate) fn prepare_runtime(
    repo_root: &Path,
    bundler: &Path,
) -> Result<(String, PathBuf, PathBuf)> {
    let server_jar = server_jar_for_bundler(repo_root, bundler)?;
    let classpath_dir = repo_root.join("tools/rivet-codegen/.cache/classpath");
    let classpath = build_classpath(&classpath_dir, &server_jar)?;
    let (java, javac) = resolve_java()?;
    Ok((classpath, java, javac))
}

pub(crate) fn run_cmd(program: &Path, args: &[&str], what: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("spawn {program:?} for {what}"))?;
    if !status.success() {
        bail!("{what} failed with {status}");
    }
    Ok(())
}

/// Capture stdout of a command (used by mth-gen to read the oracle's printed
/// tables/vectors).
pub(crate) fn run_cmd_capture(program: &Path, args: &[&str], what: &str) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("spawn {program:?} for {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed with {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundler_cache_identity_rejects_conflicting_override() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".bundler.sha256"), "sha-b\n").unwrap();
        assert!(!bundler_cache_matches(root.path(), "sha-a"));
        assert!(bundler_cache_matches(root.path(), "sha-b"));
    }

    #[test]
    fn server_jar_selection_ignores_conflicting_materialized_jar() {
        let root = tempfile::tempdir().unwrap();
        let bundler = root.path().join("override-bundler.jar");
        fs::write(&bundler, b"override bundler").unwrap();
        let cache = root.path().join("tools/rivet-codegen/.cache/classpath");
        let versions = cache.join("META-INF/versions/26.2");
        fs::create_dir_all(&versions).unwrap();
        fs::write(
            cache.join(".bundler.sha256"),
            crate::reports::sha256_hex(b"override bundler"),
        )
        .unwrap();
        fs::write(
            cache.join("META-INF/versions.list"),
            "sha1 26.2 26.2/paper-26.2.jar\n",
        )
        .unwrap();
        fs::write(versions.join("paper-26.2.jar"), b"override server").unwrap();

        let materialized = crate::reports::default_jar(root.path());
        fs::create_dir_all(materialized.parent().unwrap()).unwrap();
        fs::write(materialized, b"materialized server").unwrap();

        let selected = server_jar_for_bundler(root.path(), &bundler).unwrap();
        assert_eq!(fs::read(selected).unwrap(), b"override server");
    }

    #[test]
    fn bundler_hash_wins_when_override_mtime_is_not_newer() {
        use std::time::{Duration, SystemTime};

        for age in [Duration::ZERO, Duration::from_secs(60)] {
            let root = tempfile::tempdir().unwrap();
            let cache = root.path().join("classpath");
            let bundler_a = root.path().join("bundler-a.jar");
            let bundler_b = root.path().join("bundler-b.jar");
            write_test_bundler(&bundler_a, b"server A");
            extract_bundler(&bundler_a, &cache).unwrap();
            let marker_mtime = fs::metadata(cache.join("META-INF/versions/26.2/paper-26.2.jar"))
                .unwrap()
                .modified()
                .unwrap();

            write_test_bundler(&bundler_b, b"server B");
            let bundler_b_mtime = marker_mtime
                .checked_sub(age)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            fs::File::open(&bundler_b)
                .unwrap()
                .set_modified(bundler_b_mtime)
                .unwrap();
            extract_bundler(&bundler_b, &cache).unwrap();

            let selected = cache.join("META-INF/versions/26.2/paper-26.2.jar");
            assert_eq!(fs::read(selected).unwrap(), b"server B");
        }
    }

    fn write_test_bundler(path: &Path, server: &[u8]) {
        let stage = path.with_extension("stage");
        fs::create_dir_all(stage.join("META-INF/versions/26.2")).unwrap();
        fs::write(
            stage.join("META-INF/versions.list"),
            "sha1 26.2 26.2/paper-26.2.jar\n",
        )
        .unwrap();
        fs::write(stage.join("META-INF/versions/26.2/paper-26.2.jar"), server).unwrap();
        fs::create_dir_all(stage.join("META-INF/libraries")).unwrap();
        fs::write(stage.join("META-INF/libraries/placeholder.jar"), b"library").unwrap();
        let status = Command::new("zip")
            .current_dir(&stage)
            .args(["-q", "-r", path.to_str().unwrap(), "META-INF"])
            .status()
            .unwrap();
        assert!(status.success());
    }
}
