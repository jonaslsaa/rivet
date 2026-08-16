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

use anyhow::{Context, Result, anyhow, bail, ensure};

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

    let classpath_dir = bundler_cache_dir(&cache, &bundler)?;
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

    let helper_dir_arg = path_to_utf8(&helper_dir, "BlockDataExtractor output directory")?;
    let helper_file_arg = path_to_utf8(&helper_file, "BlockDataExtractor source")?;
    run_cmd(
        &javac,
        &["-cp", &classpath, "-d", helper_dir_arg, helper_file_arg],
        "compile BlockDataExtractor.java",
    )?;

    let helper_dir_arg = path_to_utf8(&helper_dir, "BlockDataExtractor classpath directory")?;
    let classpath_arg = format!("{classpath}:{helper_dir_arg}");
    let log4j_arg = format!(
        "-Dlog4j.configurationFile={}",
        path_to_utf8(&log4j_off, "log4j configuration")?
    );
    let output_arg = path_to_utf8(&output, "block registry output")?;
    run_cmd(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "BlockDataExtractor",
            "--output",
            output_arg,
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

/// Return the UTF-8 representation required by the Java and unzip command lines.
/// A hostile filesystem path must classify as UNVERIFIED, never panic.
pub(crate) fn path_to_utf8<'a>(path: &'a Path, label: &str) -> Result<&'a str> {
    path.to_str().with_context(|| {
        format!(
            "UNVERIFIED: {label} path is not valid UTF-8: {}",
            path.to_string_lossy()
        )
    })
}

/// Isolate every extracted classpath by the exact SHA-256 of its bundler.
pub(crate) fn bundler_cache_dir(cache_root: &Path, bundler: &Path) -> Result<PathBuf> {
    let bundler_sha = crate::reports::sha256_hex(
        &fs::read(bundler).with_context(|| format!("read bundler {}", bundler.display()))?,
    );
    let root = cache_root.join("classpath");
    match fs::symlink_metadata(&root) {
        Ok(metadata) => {
            ensure!(
                !metadata.file_type().is_symlink(),
                "UNVERIFIED: classpath cache root is a symlink: {}",
                root.display()
            );
            ensure!(
                metadata.file_type().is_dir(),
                "UNVERIFIED: classpath cache root is not a directory: {}",
                root.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        }
        Err(error) => {
            return Err(error).with_context(|| format!("inspect {}", root.display()));
        }
    }
    Ok(root.join(bundler_sha))
}

fn validate_namespace_root(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("UNVERIFIED: {label} is missing at {}", path.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "UNVERIFIED: {label} is a symlink: {}",
        path.display()
    );
    ensure!(
        metadata.file_type().is_dir(),
        "UNVERIFIED: {label} is not a directory: {}",
        path.display()
    );
    Ok(())
}

fn archive_entries(bundler: &Path) -> Result<Vec<String>> {
    let bundler_arg = path_to_utf8(bundler, "bundler jar")?;
    let output = Command::new("unzip")
        .args(["-Z1", bundler_arg])
        .output()
        .context("list bundler archive entries")?;
    if !output.status.success() {
        bail!("unzip -Z1 failed to list bundler archive entries");
    }
    let text = std::str::from_utf8(&output.stdout)
        .context("UNVERIFIED: bundler archive entry list is not UTF-8")?;
    Ok(text.lines().map(str::to_owned).collect())
}

fn validate_archive_path(path: &str, label: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
                    | std::path::Component::ParentDir
                    | std::path::Component::CurDir
            )
        })
    {
        bail!(
            "UNVERIFIED: {label} contains an unsafe relative path `{}`",
            path.display()
        );
    }
    Ok(())
}

fn validate_regular_file(path: &Path, root: &Path, label: &str) -> Result<()> {
    validate_namespace_root(root, "extracted bundler namespace")?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("UNVERIFIED: {label} is missing at {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "UNVERIFIED: {label} is not a regular file: {}",
            path.display()
        );
    }
    let root = fs::canonicalize(root)
        .with_context(|| format!("UNVERIFIED: canonicalize cache root {}", root.display()))?;
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("UNVERIFIED: canonicalize {label} {}", path.display()))?;
    if !canonical.starts_with(&root) {
        bail!(
            "UNVERIFIED: {label} escapes the extracted bundler namespace: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_directory(path: &Path, root: &Path, label: &str) -> Result<()> {
    validate_namespace_root(root, "extracted bundler namespace")?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("UNVERIFIED: {label} is missing at {}", path.display()))?;
    if !metadata.file_type().is_dir() {
        bail!("UNVERIFIED: {label} is not a directory: {}", path.display());
    }
    let root = fs::canonicalize(root)
        .with_context(|| format!("UNVERIFIED: canonicalize cache root {}", root.display()))?;
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("UNVERIFIED: canonicalize {label} {}", path.display()))?;
    if !canonical.starts_with(&root) {
        bail!(
            "UNVERIFIED: {label} escapes the extracted bundler namespace: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_relative_file(root: &Path, relative: &str, label: &str) -> Result<PathBuf> {
    validate_archive_path(relative, label)?;
    let path = root.join(relative);
    validate_regular_file(&path, root, label)?;
    Ok(path)
}

fn collect_jars(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let root = dir
        .parent()
        .and_then(Path::parent)
        .context("library directory has no extracted namespace")?;
    validate_directory(dir, root, "META-INF/libraries")?;
    for entry in
        fs::read_dir(dir).with_context(|| format!("read libraries dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            bail!("UNVERIFIED: library path is a symlink: {}", path.display());
        }
        if metadata.file_type().is_dir() {
            collect_jars(&path, out)?;
        } else if metadata.file_type().is_file() {
            if path.extension().is_some_and(|e| e == "jar") {
                validate_regular_file(&path, root, "library jar")?;
                out.push(path_to_utf8(&path, "library jar")?.to_owned());
            }
        } else {
            bail!(
                "UNVERIFIED: library path is not a regular file or directory: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn parse_versions_list(contents: &str, classpath_dir: &Path) -> Result<(String, String)> {
    let line = contents
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("empty META-INF/versions.list")?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 3 {
        bail!("invalid META-INF/versions.list entry: expected sha1 version path");
    }
    let version = fields[1].to_owned();
    let rel_path = fields[2].to_owned();
    let versions_root = classpath_dir.join("META-INF/versions");
    validate_directory(&versions_root, classpath_dir, "META-INF/versions")?;
    validate_relative_file(
        &versions_root,
        &rel_path,
        "server path in META-INF/versions.list",
    )?;
    Ok((version, rel_path))
}

fn validate_extracted_cache(classpath_dir: &Path, bundler_sha: &str) -> Result<()> {
    let marker = classpath_dir.join(".bundler.sha256");
    validate_regular_file(&marker, classpath_dir, "bundler cache marker")?;
    let cached = fs::read_to_string(&marker).context("read bundler cache marker")?;
    ensure!(
        cached.trim() == bundler_sha,
        "bundler cache marker SHA mismatch"
    );
    let versions_list = classpath_dir.join("META-INF/versions.list");
    validate_regular_file(&versions_list, classpath_dir, "META-INF/versions.list")?;
    let contents =
        fs::read_to_string(&versions_list).context("read extracted META-INF/versions.list")?;
    parse_versions_list(&contents, classpath_dir)?;
    let libraries = classpath_dir.join("META-INF/libraries");
    let mut jars = Vec::new();
    collect_jars(&libraries, &mut jars)?;
    ensure!(
        !jars.is_empty(),
        "no regular library jars in extracted bundler cache"
    );
    Ok(())
}

fn bundler_cache_matches(classpath_dir: &Path, bundler_sha: &str) -> bool {
    validate_extracted_cache(classpath_dir, bundler_sha).is_ok()
}

fn extract_bundler(bundler: &Path, classpath_dir: &Path) -> Result<()> {
    let bundler_sha = crate::reports::sha256_hex(&fs::read(bundler).context("read bundler jar")?);
    if let Ok(metadata) = fs::symlink_metadata(classpath_dir) {
        ensure!(
            !metadata.file_type().is_symlink(),
            "UNVERIFIED: bundler cache namespace is a symlink: {}",
            classpath_dir.display()
        );
    }
    if bundler_cache_matches(classpath_dir, &bundler_sha) {
        return Ok(());
    }

    let entries = archive_entries(bundler)?;
    let has_versions_list = entries
        .iter()
        .any(|entry| entry == "META-INF/versions.list");
    ensure!(
        has_versions_list,
        "UNVERIFIED: bundler lacks META-INF/versions.list"
    );
    for entry in &entries {
        if entry == "META-INF/versions.list"
            || entry.starts_with("META-INF/versions/")
            || entry.starts_with("META-INF/libraries/")
        {
            validate_archive_path(entry, "bundler archive entry")?;
        }
    }
    ensure!(
        entries
            .iter()
            .any(|entry| entry.starts_with("META-INF/versions/") && !entry.ends_with('/')),
        "UNVERIFIED: bundler has no server version entry"
    );
    ensure!(
        entries
            .iter()
            .any(|entry| entry.starts_with("META-INF/libraries/") && entry.ends_with(".jar")),
        "UNVERIFIED: bundler has no library jars"
    );

    let parent = classpath_dir
        .parent()
        .context("bundler cache namespace has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), bundler_sha));
    if let Ok(metadata) = fs::symlink_metadata(&tmp) {
        ensure!(
            !metadata.file_type().is_symlink(),
            "UNVERIFIED: temporary bundler extraction path is a symlink: {}",
            tmp.display()
        );
        fs::remove_dir_all(&tmp).with_context(|| format!("clear {}", tmp.display()))?;
    }
    fs::create_dir_all(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let bundler_arg = path_to_utf8(bundler, "bundler jar")?;
    let tmp_arg = path_to_utf8(&tmp, "bundler extraction directory")?;
    let mut unzip_args = vec!["-o", "-q", bundler_arg, "-d", tmp_arg];
    if entries.iter().any(|entry| entry == "META-INF/MANIFEST.MF") {
        unzip_args.push("META-INF/MANIFEST.MF");
    }
    if entries.iter().any(|entry| entry == "META-INF/main-class") {
        unzip_args.push("META-INF/main-class");
    }
    unzip_args.extend([
        "META-INF/versions.list",
        "META-INF/versions/*",
        "META-INF/libraries/*",
    ]);
    run_cmd(
        &PathBuf::from("unzip"),
        &unzip_args,
        "extract server + libraries from bundler jar",
    )?;
    fs::write(tmp.join(".bundler.sha256"), &bundler_sha)
        .context("record bundler cache identity")?;
    if let Err(error) = validate_extracted_cache(&tmp, &bundler_sha) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(error);
    }

    if bundler_cache_matches(classpath_dir, &bundler_sha) {
        fs::remove_dir_all(&tmp).ok();
        return Ok(());
    }
    match fs::symlink_metadata(classpath_dir) {
        Ok(metadata) => {
            ensure!(
                !metadata.file_type().is_symlink(),
                "UNVERIFIED: bundler cache namespace became a symlink: {}",
                classpath_dir.display()
            );
            ensure!(
                metadata.file_type().is_dir(),
                "UNVERIFIED: bundler cache namespace is not a directory: {}",
                classpath_dir.display()
            );
            fs::remove_dir_all(classpath_dir).with_context(|| {
                format!("clear stale bundler cache {}", classpath_dir.display())
            })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("inspect stale bundler cache {}", classpath_dir.display())
            });
        }
    }
    fs::rename(&tmp, classpath_dir).with_context(|| {
        format!(
            "atomically install bundler cache {} -> {}",
            tmp.display(),
            classpath_dir.display()
        )
    })?;
    Ok(())
}

pub(crate) fn read_versions_list(bundler: &Path, classpath_dir: &Path) -> Result<(String, String)> {
    let bundler_sha = crate::reports::sha256_hex(&fs::read(bundler).context("read bundler jar")?);
    ensure!(
        bundler_cache_matches(classpath_dir, &bundler_sha),
        "UNVERIFIED: extracted bundler cache is missing, stale, or unsafe"
    );
    let marker = classpath_dir.join("META-INF/versions.list");
    let contents = fs::read_to_string(&marker)
        .with_context(|| format!("read extracted {}", marker.display()))?;
    parse_versions_list(&contents, classpath_dir)
}

/// Full classpath: server jar + every library jar under META-INF/libraries.
fn build_classpath(classpath_dir: &Path, server_jar: &Path) -> Result<String> {
    validate_regular_file(server_jar, classpath_dir, "selected server jar")?;
    let libs = classpath_dir.join("META-INF/libraries");
    let mut jars = vec![path_to_utf8(server_jar, "selected server jar")?.to_owned()];
    collect_jars(&libs, &mut jars)?;
    if jars.len() <= 1 {
        bail!("no library jars found under {}", libs.display());
    }
    Ok(jars.join(":"))
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
    let classpath_dir = bundler_cache_dir(&cache, bundler)?;
    extract_bundler(bundler, &classpath_dir)?;

    let (_, server_jar_rel) = read_versions_list(bundler, &classpath_dir)?;
    let versions_root = classpath_dir.join("META-INF/versions");
    let server_jar = validate_relative_file(
        &versions_root,
        &server_jar_rel,
        "selected server jar from META-INF/versions.list",
    )?;
    ensure!(
        server_jar.starts_with(&classpath_dir),
        "UNVERIFIED: selected server jar escaped bundler cache namespace"
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
    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = bundler_cache_dir(&cache, bundler)?;
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
        let bundler = root.path().join("bundler.jar");
        write_test_bundler(&bundler, b"server");
        let cache = root.path().join("cache");
        extract_bundler(&bundler, &cache).unwrap();
        let sha = crate::reports::sha256_hex(&fs::read(&bundler).unwrap());
        assert!(!bundler_cache_matches(&cache, "sha-a"));
        assert!(bundler_cache_matches(&cache, &sha));
    }

    #[test]
    fn server_jar_selection_ignores_conflicting_materialized_jar() {
        let root = tempfile::tempdir().unwrap();
        let bundler = root.path().join("override-bundler.jar");
        write_test_bundler(&bundler, b"override server");
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

    #[test]
    fn bundlers_use_distinct_hash_namespaces() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let bundler_a = root.path().join("bundler-a.jar");
        let bundler_b = root.path().join("bundler-b.jar");
        write_test_bundler(&bundler_a, b"server A");
        write_test_bundler(&bundler_b, b"server B");

        let cache_a = bundler_cache_dir(&cache, &bundler_a).unwrap();
        let cache_b = bundler_cache_dir(&cache, &bundler_b).unwrap();
        assert_ne!(cache_a, cache_b);
        extract_bundler(&bundler_a, &cache_a).unwrap();
        extract_bundler(&bundler_b, &cache_b).unwrap();
        assert_eq!(
            fs::read(cache_a.join("META-INF/versions/26.2/paper-26.2.jar")).unwrap(),
            b"server A"
        );
        assert_eq!(
            fs::read(cache_b.join("META-INF/versions/26.2/paper-26.2.jar")).unwrap(),
            b"server B"
        );
    }

    #[test]
    fn versions_list_rejects_parent_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("META-INF/versions/26.2")).unwrap();
        fs::write(
            root.path().join("META-INF/versions/26.2/paper-26.2.jar"),
            b"server",
        )
        .unwrap();
        let error = parse_versions_list("sha1 26.2 ../outside.jar\n", root.path()).unwrap_err();
        assert!(
            error.to_string().contains("unsafe relative path"),
            "got: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn classpath_cache_root_symlink_is_unverified() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let bundler = root.path().join("bundler.jar");
        write_test_bundler(&bundler, b"server");
        let cache = root.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, cache.join("classpath")).unwrap();

        let error = bundler_cache_dir(&cache, &bundler).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("classpath cache root is a symlink"),
            "got: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn classpath_cache_namespace_symlink_is_unverified() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let bundler = root.path().join("bundler.jar");
        write_test_bundler(&bundler, b"server");
        let cache = root.path().join("cache");
        let classpath = bundler_cache_dir(&cache, &bundler).unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, &classpath).unwrap();

        let error = extract_bundler(&bundler, &classpath).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("bundler cache namespace is a symlink"),
            "got: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_server_symlink_is_unverified() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let versions = root.path().join("META-INF/versions/26.2");
        fs::create_dir_all(&versions).unwrap();
        fs::write(root.path().join("outside.jar"), b"outside").unwrap();
        symlink(
            root.path().join("outside.jar"),
            versions.join("paper-26.2.jar"),
        )
        .unwrap();
        let error = validate_relative_file(
            &root.path().join("META-INF/versions"),
            "26.2/paper-26.2.jar",
            "selected server jar",
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("not a regular file"),
            "got: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_command_path_is_unverified() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', 0xff]));
        let error = path_to_utf8(&path, "bundler jar").unwrap_err();
        assert!(
            error.to_string().contains("not valid UTF-8"),
            "got: {error}"
        );
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
        let path_arg = path.to_string_lossy().into_owned();
        let status = Command::new("zip")
            .current_dir(&stage)
            .args(["-q", "-r", &path_arg, "META-INF"])
            .status()
            .unwrap();
        assert!(status.success());
    }
}
