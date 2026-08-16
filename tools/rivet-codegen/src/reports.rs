//! `rivet-codegen reports` — capture the canonical vanilla data reports by
//! running the real `net.minecraft.data.Main --reports` datagen against the
//! materialized Paper 26.2 server jar, then pin them with provenance.
//!
//! The three report generators we need (`net.minecraft.data.info.PacketReport`,
//! `RegistryDumpReport`, `BlockListReport`) are registered in
//! `net.minecraft.data.Main.addServerDefinitionProviders` under the `--reports`
//! flag and each write into `<output>/reports/`:
//!
//! - `packets.json` — every protocol/flow packet name -> `protocol_id`, in the
//!   exact `addPacket` registration order of the `*Protocols.TEMPLATE`
//!   definitions. This is the canonical enumeration that `IdDispatchCodec`
//!   assigns ids from, so it is the oracle for `rivet-protocol`'s packet-ID
//!   tables.
//! - `registries.json` — every `BuiltInRegistries` registry with numeric
//!   protocol ids; `Bootstrap.bootStrap()` runs in `DataGenerator` static init,
//!   so this is a fully-populated real-server dump.
//! - `blocks.json` — per-block ordered state properties, all state ids, the
//!   default marker, and the `BlockTypes.CODEC` definition.
//!
//! We run the vanilla entrypoint directly (no custom extraction logic), copy
//! the three JSON artifacts byte-for-byte into `data/reports/`, and record the
//! source provenance (jar identity, Paper git commit, MC/protocol/world
//! version) in `manifest.json`. The datagen output is deterministic (verified
//! byte-identical across independent runs), so the committed fixtures are the
//! no-drift baseline.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// The three canonical report artifacts (as produced under `reports/` by
/// `Main --reports`). Order is fixed so the committed layout is deterministic.
pub const REPORT_FILES: &[&str] = &["packets.json", "registries.json", "blocks.json"];

const LOG4J_OFF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#;

/// Every committed Paper-derived fixture in this repository is captured from
/// this exact clean checkout, never from an arbitrary current HEAD.
pub(crate) const PINNED_PAPER_COMMIT: &str = "0a993450f129c4942c2a9ed45ba047412b4667cf";
pub(crate) const PINNED_SERVER_JAR_SHA256: &str =
    "e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda";
pub(crate) const PINNED_JOIN_CAPTURE_SHA256: &str =
    "e78a673617a1eefc8029c43b69bf3cbe7d1f2b6fcf65e3b333f5c85e25f1c533";
pub(crate) const PINNED_MINECRAFT_VERSION: &str = "26.2";
pub(crate) const PINNED_PROTOCOL_VERSION: u32 = 776;
pub(crate) const PINNED_WORLD_VERSION: u32 = 4903;

/// Default pinned source: the materialized server jar created by the oracle's
/// first boot (`tools/rivet-oracle work/run/versions/26.2/`). `pub(crate)` so
/// the biomes+tags half ([`crate::extract_biomes_tags`]) records the same
/// canonical source identity.
pub(crate) fn default_jar(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar")
}

/// `--jar` flag wins, then `RIVET_CODEGEN_JAR` (mirrors `RIVET_ORACLE_JAR` for
/// the gitignored oracle `work/`, which is absent from committed checkouts —
/// e.g. pointing at the primary repo from a worktree), then the default.
fn resolve_jar(jar_flag: Option<&Path>, repo_root: &Path) -> Result<PathBuf> {
    if let Some(p) = jar_flag {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("RIVET_CODEGEN_JAR") {
        let path = PathBuf::from(p);
        if !path.is_file() {
            bail!(
                "RIVET_CODEGEN_JAR is set to {} but it is not a file",
                path.display()
            );
        }
        return Ok(path);
    }
    let default = default_jar(repo_root);
    ensure!(
        default.is_file(),
        "materialized server jar not found at {} — boot the oracle once (cargo run -p rivet-oracle -- verify), set RIVET_CODEGEN_JAR, or pass --jar",
        default.display()
    );
    Ok(default)
}

/// Where the pinned reports + provenance manifest are committed.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/reports")
}

pub fn run(jar_flag: Option<&Path>, output_flag: Option<&Path>, verify: bool) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let jar = resolve_jar(jar_flag, &repo_root)?;
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let (java, _) = crate::extract::resolve_java()?;
    let classpath = build_classpath(&jar)?;
    let cache = repo_root.join("tools/rivet-codegen/.cache/reports");

    if verify {
        verify_fixture_provenance(&jar, &output.join("manifest.json"), &repo_root)?;
        return verify_no_drift(&jar, &output, &java, &classpath, &cache);
    }
    capture(&jar, &output, &java, &classpath, &cache, &repo_root)
}

/// Regenerate the pinned reports into `output/` and refresh `manifest.json`.
fn capture(
    jar: &Path,
    output: &Path,
    java: &Path,
    classpath: &str,
    cache: &Path,
    repo_root: &Path,
) -> Result<()> {
    let scratch = cache.join(format!("capture-{}", std::process::id()));
    fs::create_dir_all(&scratch).context("create capture scratch dir")?;

    let reports_dir = run_datagen(java, classpath, &scratch, cache)?;
    let source = capture_source(jar, repo_root)?;
    println!(
        "Capturing {} reports from {}",
        REPORT_FILES.len(),
        jar.display()
    );

    fs::create_dir_all(output).context("create reports output dir")?;
    let mut entries = Vec::new();
    for name in REPORT_FILES {
        let src = reports_dir.join(name);
        let dst = output.join(name);
        let bytes = fs::read(&src).with_context(|| format!("read {}", src.display()))?;
        let previous = fs::read(&dst).ok();
        fs::write(&dst, &bytes).with_context(|| format!("write {}", dst.display()))?;
        let sha = sha256_hex(&bytes);
        let status = match &previous {
            Some(old) if *old == bytes => "unchanged",
            Some(_) => "updated",
            None => "new",
        };
        println!(
            "  {} {:<18} {} bytes (sha256 {})",
            status,
            name,
            bytes.len(),
            &sha[..16]
        );
        entries.push(ReportEntry {
            path: name.to_string(),
            bytes: bytes.len() as u64,
            sha256: sha,
        });
    }

    let manifest = ProvenanceManifest {
        format: 1,
        generator: "net.minecraft.data.Main --reports".to_string(),
        source,
        reports: entries,
    };
    let manifest_json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(output.join("manifest.json"), &manifest_json).context("write manifest.json")?;

    let _ = fs::remove_dir_all(&scratch);
    println!(
        "Captured {} reports (MC {}, protocol {}, world {}) from {}",
        REPORT_FILES.len(),
        manifest.source.minecraft_version,
        manifest.source.protocol_version,
        manifest.source.world_version,
        jar.display()
    );
    println!("  source sha256: {}", manifest.source.jar_sha256);
    println!("  provenance -> {}", output.join("manifest.json").display());
    println!("  verify no-drift with: rivet-codegen reports --verify");
    Ok(())
}

/// No-drift gate: run the datagen twice (proves cross-run determinism), require
/// both runs' output to be byte-identical to the committed fixtures, and check
/// the committed files still match the manifest's recorded hashes.
fn verify_no_drift(
    jar: &Path,
    output: &Path,
    java: &Path,
    classpath: &str,
    cache: &Path,
) -> Result<()> {
    let manifest_path = output.join("manifest.json");
    ensure!(
        manifest_path.is_file(),
        "no committed reports at {} — run `rivet-codegen reports` first",
        output.display()
    );

    let run_a = cache.join("verify-a");
    let run_b = cache.join("verify-b");
    let dir_a = run_datagen(java, classpath, &run_a, cache)?;
    let dir_b = run_datagen(java, classpath, &run_b, cache)?;

    let mut drift: Vec<String> = Vec::new();
    for name in REPORT_FILES {
        let a = fs::read(dir_a.join(name))
            .with_context(|| format!("read {}", dir_a.join(name).display()))?;
        let b = fs::read(dir_b.join(name))
            .with_context(|| format!("read {}", dir_b.join(name).display()))?;
        if a != b {
            drift.push(format!(
                "{name}: two fresh --reports runs differ (not deterministic)"
            ));
            continue;
        }
        let committed = fs::read(output.join(name))
            .with_context(|| format!("read committed {}", output.join(name).display()))?;
        if a != committed {
            drift.push(format!(
                "{name}: fresh --reports run differs from the committed fixture"
            ));
        }
    }
    if !drift.is_empty() {
        bail!("REPORT DRIFT DETECTED:\n  {}", drift.join("\n  "));
    }

    let manifest: ProvenanceManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).context("read manifest.json")?)
            .context("parse manifest.json")?;
    for entry in &manifest.reports {
        let committed = fs::read(output.join(&entry.path)).context("read committed report")?;
        let actual = sha256_hex(&committed);
        if actual != entry.sha256 || committed.len() as u64 != entry.bytes {
            bail!(
                "committed {} does not match manifest (manifest sha256 {} / {} bytes, actual sha256 {} / {} bytes)",
                entry.path,
                entry.sha256,
                entry.bytes,
                actual,
                committed.len()
            );
        }
    }

    // Source provenance change is a warning, not a drift failure: if the jar
    // changed but the reports are byte-identical the fixtures are still
    // canonical; the manifest should be refreshed by a `reports` run.
    let jar_sha = sha256_hex(&fs::read(jar).context("read source jar")?);
    if manifest.source.jar_sha256 != jar_sha {
        eprintln!(
            "note: source jar sha256 changed (committed {} -> {}) — rerun `rivet-codegen reports` to refresh provenance",
            manifest.source.jar_sha256, jar_sha
        );
    }

    let _ = fs::remove_dir_all(&run_a);
    let _ = fs::remove_dir_all(&run_b);
    println!(
        "reports verified: {}/{} byte-identical to a fresh --reports run (two independent runs identical)",
        REPORT_FILES.len(),
        REPORT_FILES.len()
    );
    Ok(())
}

/// Run `net.minecraft.data.Main --reports --output <scratch>` on the full server
/// classpath and return the `reports/` subdir holding the three JSONs.
fn run_datagen(java: &Path, classpath: &str, scratch: &Path, cache: &Path) -> Result<PathBuf> {
    // Start from a clean scratch: a crashed prior run (or an aborted verify)
    // must not leave stale report files behind that could be misread as fresh.
    if scratch.exists() {
        fs::remove_dir_all(scratch).with_context(|| format!("clear {}", scratch.display()))?;
    }
    fs::create_dir_all(scratch).with_context(|| format!("create {}", scratch.display()))?;
    let log4j_off = cache.join("log4j2-off.xml");
    if !log4j_off.is_file() {
        fs::write(&log4j_off, LOG4J_OFF).context("write log4j2-off.xml")?;
    }

    let scratch_arg = crate::extract::path_to_utf8(scratch, "reports datagen output")?;
    let log4j_arg = format!(
        "-Dlog4j.configurationFile={}",
        crate::extract::path_to_utf8(&log4j_off, "log4j configuration")?
    );
    let status = Command::new(java)
        .args(["-Xms512M", "-Xmx2G"])
        .arg("-cp")
        .arg(classpath)
        .arg("--enable-native-access=ALL-UNNAMED")
        .arg(log4j_arg)
        .args(["net.minecraft.data.Main", "--reports", "--output"])
        .arg(scratch_arg)
        .status()
        .with_context(|| format!("spawn {java:?} for net.minecraft.data.Main --reports"))?;
    ensure!(
        status.success(),
        "datagen (net.minecraft.data.Main --reports) failed with {status}"
    );

    let reports_dir = scratch.join("reports");
    for name in REPORT_FILES {
        ensure!(
            reports_dir.join(name).is_file(),
            "datagen finished but {} was not produced",
            reports_dir.join(name).display()
        );
    }
    Ok(reports_dir)
}

/// Full classpath: the materialized server jar first, then every library jar
/// under the sibling `libraries/` dir, sorted for a deterministic ordering.
fn build_classpath(jar: &Path) -> Result<String> {
    let libs = libraries_dir(jar);
    ensure!(
        libs.is_dir(),
        "libraries dir not found at {} — the materialized run is incomplete",
        libs.display()
    );
    let mut jars = vec![jar.display().to_string()];
    collect_jars(&libs, &mut jars)?;
    if jars.len() <= 1 {
        bail!("no library jars found under {}", libs.display());
    }
    Ok(jars.join(":"))
}

/// The `libraries/` dir sits next to the materialized run dir: the jar lives at
/// `<run>/versions/<mc>/paper-<mc>.jar`, so libraries is parent^3.
fn libraries_dir(jar: &Path) -> PathBuf {
    jar.parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|p| p.join("libraries"))
        .unwrap_or_else(|| PathBuf::from("libraries"))
}

fn collect_jars(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let mut jars = Vec::new();
    collect_jars_into(dir, &mut jars)?;
    jars.sort();
    out.extend(jars);
    Ok(())
}

fn collect_jars_into(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_jars_into(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "jar") {
            out.push(path.display().to_string());
        }
    }
    Ok(())
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let digest = sha2::Sha256::digest(data);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProvenanceManifest {
    format: u64,
    /// The vanilla entrypoint that produced the reports.
    generator: String,
    pub(crate) source: SourceProvenance,
    pub(crate) reports: Vec<ReportEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SourceProvenance {
    /// The jar path passed/used at capture time (repo-relative when default).
    pub(crate) jar: String,
    pub(crate) jar_sha256: String,
    /// Paper commit the jar was built from. Legacy manifests remain
    /// deserializable, but generation rejects an absent or non-pinned value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) paper_git: Option<String>,
    pub(crate) minecraft_version: String,
    pub(crate) protocol_version: u32,
    pub(crate) world_version: u32,
}

/// Reject fixture provenance that merely repeats a self-supplied source label.
/// Generated tables are tied to one of the exact server-jar or canonical-join
/// sources and the Paper checkout pinned by this repository.
pub(crate) fn verify_pinned_source(source: &SourceProvenance) -> Result<()> {
    ensure!(
        source.jar_sha256 == PINNED_SERVER_JAR_SHA256
            || source.jar_sha256 == PINNED_JOIN_CAPTURE_SHA256,
        "UNVERIFIED: fixture source SHA {} is not a pinned source",
        source.jar_sha256
    );
    ensure!(
        source.paper_git.as_deref() == Some(PINNED_PAPER_COMMIT),
        "UNVERIFIED: fixture Paper commit {:?} is not the pinned {}",
        source.paper_git,
        PINNED_PAPER_COMMIT
    );
    ensure!(
        source.minecraft_version == PINNED_MINECRAFT_VERSION,
        "UNVERIFIED: fixture Minecraft version {} is not the pinned {}",
        source.minecraft_version,
        PINNED_MINECRAFT_VERSION
    );
    ensure!(
        source.protocol_version == PINNED_PROTOCOL_VERSION,
        "UNVERIFIED: fixture protocol version {} is not the pinned {}",
        source.protocol_version,
        PINNED_PROTOCOL_VERSION
    );
    ensure!(
        source.world_version == PINNED_WORLD_VERSION,
        "UNVERIFIED: fixture world version {} is not the pinned {}",
        source.world_version,
        PINNED_WORLD_VERSION
    );
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ReportEntry {
    pub(crate) path: String,
    bytes: u64,
    pub(crate) sha256: String,
}

/// Build the source provenance for `jar` from its `version.json`, manifest,
/// and sha256. `pub(crate)` so the biomes+tags half
/// ([`crate::extract_biomes_tags`]) pins its fixture to the same source
/// identity.
pub(crate) fn capture_source(jar: &Path, repo_root: &Path) -> Result<SourceProvenance> {
    let version = read_version_json(jar)?;
    let jar_sha256 = sha256_hex(&fs::read(jar).context("read source jar")?);
    let paper_git = Some(read_paper_git(jar, repo_root)?);
    let jar_path = render_jar_path(jar, repo_root)?;
    Ok(SourceProvenance {
        jar: jar_path,
        jar_sha256,
        paper_git,
        minecraft_version: version.id,
        protocol_version: version.protocol_version,
        world_version: version.world_version,
    })
}

/// Repo-relative rendering of the source jar so the committed manifest is
/// machine-independent. The oracle materialization lives under the gitignored
/// `tools/rivet-oracle/work/`, so it never exists in any committed checkout;
/// when a `--jar` override points at the standard materialization from outside
/// the checkout (e.g. the primary repo from a worktree), we still record the
/// canonical repo-relative location. The jar's identity is the sha256, so the
/// path is provenance context only.
fn render_jar_path(jar: &Path, repo_root: &Path) -> Result<String> {
    let canonical_jar = fs::canonicalize(jar).unwrap_or_else(|_| jar.to_path_buf());
    let canonical_root = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    if let Ok(rel) = canonical_jar.strip_prefix(&canonical_root) {
        return Ok(crate::extract::path_to_utf8(rel, "source jar provenance")?.to_owned());
    }
    let default = default_jar(repo_root);
    let relative = default
        .strip_prefix(repo_root)
        .context("UNVERIFIED: default source jar is outside the Rivet checkout")?;
    Ok(crate::extract::path_to_utf8(relative, "default source jar provenance")?.to_owned())
}

/// Read `version.json` straight out of the jar — the authoritative MC build
/// metadata (id, protocol_version, world_version), not a hardcoded constant.
fn read_version_json(jar: &Path) -> Result<VersionJson> {
    let jar = utf8_path(jar, "Paper jar")?;
    let out = Command::new("unzip")
        .args(["-p", jar, "version.json"])
        .output()
        .context("read version.json from source jar")?;
    ensure!(out.status.success(), "unzip -p version.json failed");
    serde_json::from_slice(&out.stdout).context("parse version.json")
}

fn utf8_path<'a>(path: &'a Path, label: &str) -> Result<&'a str> {
    path.to_str().with_context(|| {
        format!(
            "UNVERIFIED: {label} path is not valid UTF-8: {}",
            path.display()
        )
    })
}

/// Locate the checkout that is allowed to establish Paper provenance. A
/// worktree must use the primary checkout's `working/Paper`; a missing checkout
/// is UNVERIFIED rather than an opportunity to trust arbitrary current HEAD.
fn pinned_paper_checkout(repo_root: &Path) -> Result<PathBuf> {
    let direct = repo_root.join("working/Paper");
    if direct.is_dir() {
        return Ok(direct);
    }
    let repo_root_arg = utf8_path(repo_root, "Rivet checkout")?;
    let out = Command::new("git")
        .args(["-C", repo_root_arg, "rev-parse", "--git-common-dir"])
        .output()
        .context("UNVERIFIED: resolve Rivet git common directory")?;
    ensure!(
        out.status.success(),
        "UNVERIFIED: git cannot resolve the Rivet git common directory from {}",
        repo_root.display()
    );
    let common_dir = std::str::from_utf8(&out.stdout)
        .context("UNVERIFIED: Rivet git common directory is not UTF-8")?
        .trim();
    ensure!(
        !common_dir.is_empty(),
        "UNVERIFIED: git returned an empty Rivet common directory"
    );
    let common_dir = PathBuf::from(common_dir);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        repo_root.join(common_dir)
    };
    let common_dir = fs::canonicalize(&common_dir).with_context(|| {
        format!(
            "UNVERIFIED: canonicalize Rivet git common directory {}",
            common_dir.display()
        )
    })?;
    let primary = common_dir.parent().with_context(|| {
        format!(
            "UNVERIFIED: Rivet git common directory {} has no primary checkout",
            common_dir.display()
        )
    })?;
    let paper = primary.join("working/Paper");
    ensure!(
        paper.is_dir(),
        "UNVERIFIED: pinned Paper checkout not found at {} (primary Rivet checkout {})",
        paper.display(),
        primary.display()
    );
    Ok(paper)
}

fn read_paper_git(jar: &Path, repo_root: &Path) -> Result<String> {
    let jar_arg = utf8_path(jar, "Paper jar")?;
    let manifest = Command::new("unzip")
        .args(["-p", jar_arg, "META-INF/MANIFEST.MF"])
        .output()
        .with_context(|| format!("UNVERIFIED: read Paper manifest from {}", jar.display()))?;
    ensure!(
        manifest.status.success(),
        "UNVERIFIED: Paper jar {} has no readable META-INF/MANIFEST.MF",
        jar.display()
    );
    let jar_commit = parse_paper_git_manifest(&manifest.stdout)?;

    let paper = pinned_paper_checkout(repo_root)?;
    let paper_arg = utf8_path(&paper, "Paper checkout")?;
    let out = Command::new("git")
        .args(["-C", paper_arg, "rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("UNVERIFIED: read Paper git HEAD in {}", paper.display()))?;
    ensure!(
        out.status.success(),
        "UNVERIFIED: Paper checkout at {} has no readable git HEAD",
        paper.display()
    );
    let checkout_commit = std::str::from_utf8(&out.stdout)
        .context("UNVERIFIED: Paper git HEAD is not UTF-8")?
        .trim()
        .to_owned();
    ensure!(
        !checkout_commit.is_empty(),
        "UNVERIFIED: Paper git HEAD was empty"
    );

    let status = Command::new("git")
        .args([
            "-C",
            paper_arg,
            "status",
            "--porcelain",
            "--untracked-files=all",
        ])
        .output()
        .with_context(|| format!("UNVERIFIED: inspect Paper checkout {}", paper.display()))?;
    ensure!(
        status.status.success(),
        "UNVERIFIED: cannot inspect Paper checkout status"
    );
    let dirty = std::str::from_utf8(&status.stdout)
        .context("UNVERIFIED: Paper checkout status is not UTF-8")?;
    ensure!(
        dirty.trim().is_empty(),
        "UNVERIFIED: Paper checkout {} is dirty; provenance requires a clean checkout",
        paper.display()
    );
    resolve_paper_git(jar, &jar_commit, &checkout_commit)
}

fn resolve_paper_git(jar: &Path, jar_commit: &str, checkout_commit: &str) -> Result<String> {
    ensure!(
        PINNED_PAPER_COMMIT.starts_with(jar_commit),
        "UNVERIFIED: Paper jar {} reports Git-Commit {}, not the pinned Paper commit {}",
        jar.display(),
        jar_commit,
        PINNED_PAPER_COMMIT
    );
    ensure!(
        checkout_commit == PINNED_PAPER_COMMIT,
        "UNVERIFIED: Paper checkout HEAD is {}, expected exact pinned commit {}",
        checkout_commit,
        PINNED_PAPER_COMMIT
    );
    Ok(PINNED_PAPER_COMMIT.to_string())
}

fn parse_paper_git_manifest(manifest: &[u8]) -> Result<String> {
    let manifest =
        std::str::from_utf8(manifest).context("UNVERIFIED: Paper manifest is not UTF-8")?;
    let commit = manifest
        .lines()
        .find_map(|line| line.strip_prefix("Git-Commit:").map(str::trim))
        .context("UNVERIFIED: Paper manifest has no Git-Commit field")?;
    ensure!(
        (7..=40).contains(&commit.len()) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "UNVERIFIED: Paper manifest Git-Commit field is malformed: {commit:?}"
    );
    Ok(commit.to_string())
}

/// Verify that a live probe's exact embedded server jar is the source recorded
/// by its sibling committed fixture manifest. Fixture bytes alone are not
/// provenance: a byte-different jar with behaviorally identical output must fail.
pub(crate) fn verify_fixture_provenance(
    jar: &Path,
    manifest_path: &Path,
    repo_root: &Path,
) -> Result<()> {
    let raw = fs::read_to_string(manifest_path).with_context(|| {
        format!(
            "UNVERIFIED: read fixture provenance {}",
            manifest_path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).with_context(|| {
        format!(
            "UNVERIFIED: parse fixture provenance {}",
            manifest_path.display()
        )
    })?;
    let source = value
        .get("source")
        .cloned()
        .context("UNVERIFIED: fixture provenance has no source object")?;
    let expected: SourceProvenance =
        serde_json::from_value(source).context("UNVERIFIED: parse fixture source provenance")?;
    verify_pinned_source(&expected)?;

    let actual_sha = sha256_hex(&fs::read(jar).context("read live server jar")?);
    ensure!(
        actual_sha == expected.jar_sha256,
        "UNVERIFIED: live server jar SHA {} does not match fixture provenance {}",
        actual_sha,
        expected.jar_sha256
    );
    let actual = capture_source(jar, repo_root)?;
    ensure!(
        actual.jar_sha256 == expected.jar_sha256,
        "UNVERIFIED: live server jar SHA {} does not match fixture provenance {}",
        actual.jar_sha256,
        expected.jar_sha256
    );
    ensure!(
        actual.paper_git == expected.paper_git,
        "UNVERIFIED: live server jar Git-Commit {:?} does not match fixture provenance {:?}",
        actual.paper_git,
        expected.paper_git
    );
    verify_pinned_source(&actual)
}

#[derive(Debug, Deserialize)]
struct VersionJson {
    id: String,
    protocol_version: u32,
    world_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn report_files_are_the_canonical_three() {
        assert_eq!(
            REPORT_FILES,
            &["packets.json", "registries.json", "blocks.json"]
        );
    }

    #[test]
    fn fixture_source_must_match_exact_pins() {
        let mut source = SourceProvenance {
            jar: "paper-26.2.jar".into(),
            jar_sha256: PINNED_SERVER_JAR_SHA256.into(),
            paper_git: Some(PINNED_PAPER_COMMIT.into()),
            minecraft_version: PINNED_MINECRAFT_VERSION.into(),
            protocol_version: PINNED_PROTOCOL_VERSION,
            world_version: PINNED_WORLD_VERSION,
        };
        verify_pinned_source(&source).unwrap();

        source.paper_git = Some("deadbeef".into());
        let error = verify_pinned_source(&source).unwrap_err();
        assert!(error.to_string().contains("Paper commit"), "got: {error}");

        source.paper_git = Some(PINNED_PAPER_COMMIT.into());
        source.jar_sha256 = "deadbeef".into();
        let error = verify_pinned_source(&source).unwrap_err();
        assert!(error.to_string().contains("source SHA"), "got: {error}");
    }

    #[test]
    fn libraries_dir_is_sibling_of_materialized_run() {
        let jar = Path::new("/run/versions/26.2/paper-26.2.jar");
        assert_eq!(libraries_dir(jar), Path::new("/run/libraries"));
    }

    #[test]
    fn render_jar_path_relativizes_inside_repo() {
        let repo = Path::new("/repo");
        let jar = Path::new("/repo/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar");
        assert_eq!(
            render_jar_path(jar, repo).unwrap(),
            "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
        );
    }

    #[test]
    fn render_jar_path_falls_back_to_default_for_outside_jar() {
        // A --jar override pointing at the primary repo from a worktree checkout
        // is not under the worktree root; the manifest must still record the
        // canonical repo-relative location (machine-independent).
        let repo = Path::new("/worktree");
        let outside =
            Path::new("/primary/repo/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar");
        assert_eq!(
            render_jar_path(outside, repo).unwrap(),
            "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
        );
    }

    #[test]
    fn jar_manifest_commit_is_expanded_only_after_checkout_match() {
        let jar = Path::new("/tmp/paper-server.jar");
        let checkout = "0a993450f129c4942c2a9ed45ba047412b4667cf";
        assert_eq!(
            resolve_paper_git(jar, "0a99345", checkout).unwrap(),
            checkout
        );
    }

    #[test]
    fn jar_checkout_mismatch_is_unverified() {
        let jar = Path::new("/tmp/paper-server.jar");
        let result = resolve_paper_git(jar, "0a99345", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        assert!(result.is_err());
    }

    #[test]
    fn paper_manifest_requires_a_valid_git_commit() {
        assert!(parse_paper_git_manifest(b"Manifest-Version: 1.0\r\n").is_err());
        assert!(parse_paper_git_manifest(b"Git-Commit: \r\n").is_err());
        assert!(parse_paper_git_manifest(b"Git-Commit: not-a-commit\r\n").is_err());
        assert_eq!(
            parse_paper_git_manifest(b"Manifest-Version: 1.0\r\nGit-Commit: 0a99345\r\n").unwrap(),
            "0a99345"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_jar_path_returns_unverified_error() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let jar = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        let error = read_paper_git(&jar, Path::new("/repo")).unwrap_err();
        assert!(error.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn version_json_parses_paper_26_2_snapshot() {
        let raw = r#"{
          "id": "26.2",
          "name": "26.2",
          "world_version": 4903,
          "series_id": "main",
          "protocol_version": 776,
          "pack_version": { "resource_major": 88, "resource_minor": 0, "data_major": 107, "data_minor": 1 },
          "build_time": "2026-06-16T12:01:27+00:00",
          "java_component": "java-runtime-epsilon",
          "java_version": 25,
          "stable": true,
          "use_editor": false
        }"#;
        let v: VersionJson = serde_json::from_str(raw).unwrap();
        assert_eq!(v.id, "26.2");
        assert_eq!(v.protocol_version, 776);
        assert_eq!(v.world_version, 4903);
    }

    #[test]
    fn provenance_manifest_round_trips() {
        let m = ProvenanceManifest {
            format: 1,
            generator: "net.minecraft.data.Main --reports".into(),
            source: SourceProvenance {
                jar: "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar".into(),
                jar_sha256: "ab".repeat(32),
                paper_git: Some("0a993450f129c4942c2a9ed45ba047412b4667cf".into()),
                minecraft_version: "26.2".into(),
                protocol_version: 776,
                world_version: 4903,
            },
            reports: vec![
                ReportEntry {
                    path: "packets.json".into(),
                    bytes: 18734,
                    sha256: "aa".repeat(32),
                },
                ReportEntry {
                    path: "registries.json".into(),
                    bytes: 529716,
                    sha256: "bb".repeat(32),
                },
                ReportEntry {
                    path: "blocks.json".into(),
                    bytes: 6807038,
                    sha256: "cc".repeat(32),
                },
            ],
        };
        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: ProvenanceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source.minecraft_version, "26.2");
        assert_eq!(back.source.protocol_version, 776);
        assert_eq!(back.reports.len(), 3);
        assert_eq!(back.reports[2].path, "blocks.json");
        assert!(json.contains("net.minecraft.data.Main --reports"));
    }

    /// Full determinism check against the real materialized jar. Requires the
    /// oracle to have been booted once (`tools/rivet-oracle/work/run`) and the
    /// jar reachable via the default path or `RIVET_CODEGEN_JAR`. Runs the
    /// vanilla datagen twice and asserts byte-identity of all three reports.
    /// Opt-in via `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires the materialized paper-26.2.jar (tools/rivet-oracle/work/run/versions/26.2)"]
    fn datagen_reports_are_byte_stable_across_runs() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let jar = resolve_jar(None, &repo_root)
            .expect("materialized jar: boot the oracle or set RIVET_CODEGEN_JAR");
        let (java, _) = crate::extract::resolve_java().unwrap();
        let classpath = build_classpath(&jar).unwrap();
        let cache = repo_root.join("tools/rivet-codegen/.cache/reports");
        let a = run_datagen(&java, &classpath, &cache.join("itest-a"), &cache).unwrap();
        let b = run_datagen(&java, &classpath, &cache.join("itest-b"), &cache).unwrap();
        for name in REPORT_FILES {
            let x = fs::read(a.join(name)).unwrap();
            let y = fs::read(b.join(name)).unwrap();
            assert_eq!(x, y, "{name} differs between two independent datagen runs");
        }
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }
}
