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

/// Default pinned source: the materialized server jar created by the oracle's
/// first boot (`tools/rivet-oracle work/run/versions/26.2/`).
pub fn default_jar(repo_root: &Path) -> PathBuf {
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

    let status = Command::new(java)
        .args(["-Xms512M", "-Xmx2G"])
        .arg("-cp")
        .arg(classpath)
        .arg("--enable-native-access=ALL-UNNAMED")
        .arg(format!("-Dlog4j.configurationFile={}", log4j_off.display()))
        .args(["net.minecraft.data.Main", "--reports", "--output"])
        .arg(scratch.to_str().unwrap())
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
    jar: String,
    pub(crate) jar_sha256: String,
    /// Paper commit the jar was built from (best-effort; `working/` may be a
    /// plain checkout without git metadata).
    #[serde(skip_serializing_if = "Option::is_none")]
    paper_git: Option<String>,
    pub(crate) minecraft_version: String,
    pub(crate) protocol_version: u32,
    pub(crate) world_version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ReportEntry {
    pub(crate) path: String,
    bytes: u64,
    pub(crate) sha256: String,
}

fn capture_source(jar: &Path, repo_root: &Path) -> Result<SourceProvenance> {
    let version = read_version_json(jar)?;
    let jar_sha256 = sha256_hex(&fs::read(jar).context("read source jar")?);
    let paper_git = read_paper_git(repo_root);
    let jar_path = render_jar_path(jar, repo_root);
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
fn render_jar_path(jar: &Path, repo_root: &Path) -> String {
    let canonical_jar = fs::canonicalize(jar).unwrap_or_else(|_| jar.to_path_buf());
    let canonical_root = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    if let Ok(rel) = canonical_jar.strip_prefix(&canonical_root) {
        return rel.display().to_string();
    }
    default_jar(repo_root)
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| canonical_jar.display().to_string())
}

/// Read `version.json` straight out of the jar — the authoritative MC build
/// metadata (id, protocol_version, world_version), not a hardcoded constant.
fn read_version_json(jar: &Path) -> Result<VersionJson> {
    let out = Command::new("unzip")
        .arg("-p")
        .arg(jar)
        .arg("version.json")
        .output()
        .context("read version.json from source jar")?;
    ensure!(out.status.success(), "unzip -p version.json failed");
    serde_json::from_slice(&out.stdout).context("parse version.json")
}

/// Best-effort Paper git commit (the jar is built from `working/Paper`).
fn read_paper_git(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args([
            "-C",
            repo_root.join("working/Paper").to_str().unwrap(),
            "rev-parse",
            "HEAD",
        ])
        .output()
        .ok()?;
    if out.status.success() {
        let commit = String::from_utf8_lossy(&out.stdout);
        Some(commit.trim().to_string())
    } else {
        None
    }
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
    fn libraries_dir_is_sibling_of_materialized_run() {
        let jar = Path::new("/run/versions/26.2/paper-26.2.jar");
        assert_eq!(libraries_dir(jar), Path::new("/run/libraries"));
    }

    #[test]
    fn render_jar_path_relativizes_inside_repo() {
        let repo = Path::new("/repo");
        let jar = Path::new("/repo/tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar");
        assert_eq!(
            render_jar_path(jar, repo),
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
            render_jar_path(outside, repo),
            "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
        );
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
