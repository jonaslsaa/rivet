//! rivet-oracle — the M0/M2 differential-test harness runner.
//!
//! Milestone M0 is "harness green against vanilla itself (sanity)": the Java
//! Paper server boots, generates a fixed-seed world, and we capture a golden
//! fixture slice (chunk NBT + world metadata) that later milestones diff
//! Rivet's output against. M2 extends the same harness to the normal-overworld
//! generator (density / biome / surface semantic samples + none-compression
//! region chunks), per issue #51.
//!
//! The binary has several modes:
//!
//!   1. **default** — verify every committed fixture *kind* under `fixtures/`
//!      against its own `manifest.json` SHA-256s (M0 chunk slice,
//!      `worldgen/` semantic samples, `regions/overworld-normal/` region
//!      payloads). Each kind carries an independent manifest, so kinds can grow
//!      without a format migration.
//!   2. **`<dir>`** — verify a single fixtures dir against its manifest.
//!   3. **`verify`** — the one-command M0 sanity gate: boot a *fresh* Paper
//!      run in a clean scratch dir under `work/`, wait for `Done`, shut it
//!      down cleanly (SIGTERM), extract the deterministic chunk-NBT slice,
//!      and diff its SHA-256s against the committed baseline. Prints PASS
//!      ("green against vanilla itself") or FAIL (nonzero exit).
//!   4. **`verify --m2`** — the M2 region gate: same boot pipeline, but with
//!      the normal-overworld config (`fixtures/server-normal.properties`,
//!      `region-file-compression=none` per DECISIONS D13) and diffed against
//!      `fixtures/regions/overworld-normal`. Proves two fresh boots of the
//!      normal-overworld none-compression region capture match byte-for-byte.
//!   5. **`verify --expect-fail`** — the M0 negative control: the same
//!      pipeline, diffed against a deliberately corrupted *copy* of the
//!      baseline. Exits 0 only when the tampered chunk is detected and named —
//!      proving the pipeline is not vacuously green (see README). `--m2
//!      --expect-fail` runs the control against the region baseline.
//!   6. **`sample`** — regenerate the `worldgen/` semantic fixtures: run the
//!      Paper-side sampler (`scripts/run_worldgen_sampler.sh`) into
//!      `samples.json`, re-extract the Starlight light samples from the M0
//!      FULL superflat chunks (`scripts/extract_light_samples.py`), and rewrite
//!      `manifest.json`. Requires the materialized Paper runtime (see the
//!      scripts; no full server boot).
//!   7. **`regenerate`** — full regeneration of all fixture kinds: M0 chunk
//!      slice (boot + extract), M2 region payloads (boot with the normal
//!      config + `--chunks-only` extract), and the worldgen semantic samples.
//!      Sub-select with `--m0` / `--m2` / `--samples`.
//!
//! Note on determinism (see scripts/extract_fixtures.py): raw region files
//! are NOT byte-stable across boots (framing/timestamps), but the decompressed
//! chunk NBT payloads ARE (verified 432/432 M0 superflat and 408/408 normal
//! overworld across boots, seed 42). The `verify` gates therefore compare only
//! the chunk-NBT layer; level.dat and server.properties contain wall-clock
//! timestamps and are expected to differ across boots. The worldgen semantic
//! samples are emitted by the Java sampler and are byte-identical across boots
//! for a fixed seed + generator settings.
//!
//! Usage:
//!   cargo run -p rivet-oracle                          # verify all fixture kinds
//!   cargo run -p rivet-oracle -- <dir>                 # verify <dir> against its manifest
//!   cargo run -p rivet-oracle -- verify                # full M0 gate: boot -> extract -> pin-check -> diff
//!   cargo run -p rivet-oracle -- verify [dir]          # gate against a custom baseline dir
//!   cargo run -p rivet-oracle -- verify --m2 [dir]     # M2 region gate (normal-overworld none-compression)
//!   cargo run -p rivet-oracle -- verify --expect-fail [dir]
//!                                # M0 negative control: boot -> extract -> diff against a
//!                                # deliberately corrupted copy of the baseline; exits 0 only
//!                                # when the pipeline detects AND names the tamper
//!   cargo run -p rivet-oracle -- verify --m2 --expect-fail [dir]
//!                                # M2 negative control against the region baseline
//!   cargo run -p rivet-oracle -- sample                # regenerate worldgen/ semantic samples + manifest
//!   cargo run -p rivet-oracle -- regenerate            # regenerate all fixture kinds (--m0/--m2/--samples)
//!   RIVET_ORACLE_JAR=/path/jar.jar cargo run -p rivet-oracle -- verify
//!
//! Every gate mode enforces the Paper pin recorded in the relevant
//! `fixtures/**/manifest.json` (`paper: 26.2-DEV-main@0a99345`): after the
//! boot, the `Git-Commit` attribute of the server jar the paperclip actually
//! materialized and the JVM loaded (`work/verify/run/versions/26.2/paper-26.2.jar`)
//! must match the manifest pin. The pin is read from what actually boots —
//! never from a proxy build (co-located/`working/Paper` jars that can sit at a
//! different commit than the resolved paperclip). A stale, swapped, or
//! unverifiable Paper never passes silently (see gate.sh).

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Name of the paperclip bundler jar we boot through.
const PAPERCLIP_JAR: &str = "paper-paperclip-26.2.local-SNAPSHOT.jar";

/// How long to wait for the server to reach `Done (...)!` (covers the
/// paperclip first-boot materialization of ~160MB libraries + worldgen).
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
/// How long to wait for a clean shutdown after SIGTERM (the server's own
/// shutdown can block up to 60s on chunk I/O pools; observed ~3s).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(90);
/// Poll interval while watching the boot log / process exit.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// eula.txt content; the server refuses to boot without `eula=true`.
const EULA: &str = "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\neula=true\n";

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Manifest(String),
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    /// Orchestration failure (boot, extract, shutdown, config) — not a parity
    /// mismatch, an infrastructure problem.
    Gate(String),
    /// The fresh boot's chunk hashes differ from the committed baseline.
    Diff(ChunkDiff),
    /// The server jar that actually booted is not at the commit the committed
    /// golden baseline was captured against.
    PinMismatch {
        expected: String,
        actual: String,
    },
    /// The pinned Paper commit could not be confirmed (no manifest pin, or the
    /// materialized server jar has no Git-Commit attribute to inspect).
    PinUnavailable {
        reason: String,
    },
    /// The `verify --expect-fail` negative control failed: the boot -> extract
    /// -> pin-check -> diff pipeline did not detect (and name) the deliberately
    /// corrupted baseline chunk.
    NegativeControl {
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Manifest(m) => write!(f, "manifest error: {m}"),
            Error::HashMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "hash mismatch for {path}: expected {expected}, got {actual}"
            ),
            Error::Gate(m) => write!(f, "{m}"),
            Error::Diff(d) => write!(
                f,
                "chunk NBT parity FAIL: {} of {} matched",
                d.matched(),
                d.expected
            ),
            Error::PinMismatch { expected, actual } => write!(
                f,
                "Paper commit mismatch: the server jar that actually booted \
                 (work/verify/run/versions/26.2/paper-26.2.jar) carries Git-Commit {actual}, \
                 but the committed golden baseline is pinned to {expected}.\n\
                 The baseline must match the Paper it was captured against. Regenerate the \
                 fixtures against the pinned Paper and re-pin the manifest's `paper` field \
                 before relying on this gate — never fudge fixtures to pass."
            ),
            Error::PinUnavailable { reason } => write!(
                f,
                "Paper pin unavailable: {reason}.\n\
                 The oracle gate never passes silently when the pinned Paper commit \
                 cannot be confirmed — the resolved paperclip must be built from the pinned \
                 Paper (build working/Paper at 0a99345 into a paperclip, and materialize \
                 tools/rivet-oracle/work/jars/)."
            ),
            Error::NegativeControl { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// A single captured file as recorded in `manifest.json`.
#[derive(Debug, serde::Deserialize)]
struct Captured {
    path: String,
    sha256: String,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    dim: Option<String>,
}

/// The fixture manifest (subset of fields; unknown fields are ignored).
#[derive(Debug, serde::Deserialize)]
struct Manifest {
    #[serde(default)]
    format: u64,
    #[serde(default)]
    seed: Option<String>,
    #[serde(rename = "level-type", default)]
    level_type: Option<String>,
    #[serde(default)]
    paper: Option<String>,
    #[serde(rename = "chunk-count", default)]
    chunk_count: Option<u64>,
    #[serde(rename = "region-file-compression", default)]
    region_file_compression: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    captured: Vec<Captured>,
}

/// Result of diffing the fresh boot's chunk-NBT hashes against the baseline.
#[derive(Debug, Default)]
struct ChunkDiff {
    /// Number of chunk payloads in the baseline manifest.
    expected: usize,
    /// Number of chunk payloads in the fresh manifest.
    actual: usize,
    /// (path, expected_sha256, actual_sha256) for chunks whose hash differs.
    mismatched: Vec<(String, String, String)>,
    /// Chunk paths present in the baseline but absent from the fresh run.
    missing: Vec<String>,
    /// Chunk paths present in the fresh run but absent from the baseline.
    extra: Vec<String>,
}

impl ChunkDiff {
    fn is_clean(&self) -> bool {
        self.mismatched.is_empty() && self.missing.is_empty() && self.extra.is_empty()
    }
    fn matched(&self) -> usize {
        self.expected - self.mismatched.len() - self.missing.len()
    }
}

/// Which boot configuration a gate/negative-control run uses. M0 is the
/// superflat slice (`fixtures/server.properties`, full extract); M2 is the
/// normal-overworld none-compression region capture (`server-normal.properties`,
/// `--chunks-only` extract).
struct BootConfig {
    props_src: PathBuf,
    chunks_only: bool,
    title: &'static str,
    baseline: PathBuf,
}

fn m0_config() -> BootConfig {
    BootConfig {
        props_src: crate_dir().join("fixtures/server.properties"),
        chunks_only: false,
        title: "M0 sanity gate: green against vanilla itself (superflat, seed 42)",
        baseline: crate_dir().join("fixtures"),
    }
}

fn m2_config() -> BootConfig {
    BootConfig {
        props_src: crate_dir().join("fixtures/server-normal.properties"),
        chunks_only: true,
        title: "M2 region gate: normal-overworld none-compression region parity (seed 42)",
        baseline: crate_dir().join("fixtures/regions/overworld-normal"),
    }
}

/// `worldgen/` semantic-sample manifest, serialized in the exact committed
/// field order so regeneration is byte-identical (git-clean).
#[derive(serde::Serialize)]
struct WorldgenManifest<'a> {
    format: u64,
    paper: &'a str,
    seed: &'a str,
    #[serde(rename = "level-type")]
    level_type: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<WorldgenCaptured>,
}

#[derive(serde::Serialize)]
struct WorldgenCaptured {
    path: String,
    sha256: String,
    bytes: usize,
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let digest = sha2::Sha256::digest(data);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Read + structurally validate a manifest.json (format field must be 1).
fn load_manifest(dir: &Path) -> Result<Manifest, Error> {
    let manifest_path = dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| Error::Manifest(format!("cannot read {}: {e}", manifest_path.display())))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|e| Error::Manifest(format!("invalid manifest.json: {e}")))?;
    if manifest.format != 1 {
        return Err(Error::Manifest(format!(
            "unsupported manifest format {} (expected 1)",
            manifest.format
        )));
    }
    Ok(manifest)
}

/// Verify every captured file in `dir` matches its manifest SHA-256.
fn verify_fixtures(dir: &Path) -> Result<Manifest, Error> {
    let manifest = load_manifest(dir)?;
    for cap in &manifest.captured {
        let file = dir.join(&cap.path);
        let actual_bytes = fs::read(&file)
            .map_err(|e| Error::Manifest(format!("captured file {} missing: {e}", cap.path)))?;
        if cap.bytes != 0 && actual_bytes.len() as u64 != cap.bytes {
            return Err(Error::Manifest(format!(
                "captured file {} size mismatch: manifest {}, on disk {}",
                cap.path,
                cap.bytes,
                actual_bytes.len()
            )));
        }
        let actual = sha256_hex(&actual_bytes);
        if actual != cap.sha256 {
            return Err(Error::HashMismatch {
                path: cap.path.clone(),
                expected: cap.sha256.clone(),
                actual,
            });
        }
    }
    Ok(manifest)
}

/// Extract the pinned Paper commit from the manifest's `paper` provenance
/// string (`"26.2-DEV-main@0a99345"` -> `"0a99345"`). `None` when the manifest
/// carries no `@<commit>` pin (a broken/old manifest, never silently accepted).
fn parse_paper_pin(paper: Option<&str>) -> Option<String> {
    let paper = paper?;
    let (_, commit) = paper.rsplit_once('@')?;
    let commit = commit.trim();
    if commit.is_empty() {
        None
    } else {
        Some(commit.to_string())
    }
}

/// Extract `Git-Commit: <sha>` from a jar MANIFEST.MF text.
fn parse_manifest_commit(manifest_text: &str) -> Option<String> {
    manifest_text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("Git-Commit:")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// Read the `Git-Commit` attribute out of a compiled paper-server jar by
/// shelling out to `unzip -p` (the crate stays dependency-minimal; `unzip` is
/// a standard utility on the macOS/Linux dev machines this oracle runs on).
fn read_jar_git_commit(jar: &Path) -> Result<Option<String>, Error> {
    let out = Command::new("unzip")
        .arg("-p")
        .arg(jar)
        .arg("META-INF/MANIFEST.MF")
        .output()
        .map_err(|e| {
            Error::Gate(format!(
                "failed to run unzip {}: {e} (needed to read the Paper Git-Commit)",
                jar.display()
            ))
        })?;
    if !out.status.success() {
        // No MANIFEST.MF entry (e.g. a paperclip wrapper, not a compiled server).
        return Ok(None);
    }
    Ok(parse_manifest_commit(&String::from_utf8_lossy(&out.stdout)))
}

/// The server jar path the paperclip materializes at run dir on first boot
/// (its checksum is recorded in the paperclip's `META-INF/versions.list`).
fn materialized_server_jar(run_dir: &Path) -> PathBuf {
    run_dir.join("versions/26.2/paper-26.2.jar")
}

/// Verdict of comparing the pinned baseline commit against the commit of the
/// server jar that actually booted.
#[derive(Debug, PartialEq, Eq)]
enum PinVerdict {
    Match,
    Mismatch {
        expected: String,
        actual: String,
    },
    /// No manifest pin or no readable commit — never a silent pass.
    Unavailable {
        reason: String,
    },
}

fn classify_pin(expected: Option<String>, actual: Option<String>) -> PinVerdict {
    let Some(expected) = expected else {
        return PinVerdict::Unavailable {
            reason:
                "the committed baseline manifest has no pinned Paper revision (missing `paper` field or `@<commit>`)"
                    .into(),
        };
    };
    let Some(actual) = actual else {
        return PinVerdict::Unavailable {
            reason: "the materialized server jar (work/verify/run/versions/26.2/paper-26.2.jar) \
                 carries no Git-Commit manifest attribute"
                .into(),
        };
    };
    if expected == actual {
        PinVerdict::Match
    } else {
        PinVerdict::Mismatch { expected, actual }
    }
}

/// Verify the pinned baseline Paper commit (the baseline manifest `paper`
/// provenance) against the server jar that actually booted.
///
/// The source of truth is the materialized server jar the resolved paperclip
/// produced into the run dir and the JVM loaded. This is intentionally NOT read
/// from a co-located/`working/Paper` proxy jar: that proxy can sit at a
/// different commit than the resolved paperclip (see the RIVET_ORACLE_JAR /
/// stale `work/jars/` shadowing cases), and checking it would let a
/// wrong-commit boot pass green.
fn check_pin(baseline_dir: &Path, run_dir: &Path) -> Result<(), Error> {
    let manifest = load_manifest(baseline_dir)?;
    let expected = parse_paper_pin(manifest.paper.as_deref());
    let actual = read_jar_git_commit(&materialized_server_jar(run_dir))?;
    let actual_display = actual.clone().unwrap_or_else(|| "<none>".into());
    match classify_pin(expected, actual) {
        PinVerdict::Match => {
            println!(
                "   paper pin      : {} (baseline manifest provenance) — enforced (booted \
                 jar is Git-Commit {actual_display})",
                manifest.paper.as_deref().unwrap_or("?")
            );
            Ok(())
        }
        PinVerdict::Mismatch { expected, actual } => Err(Error::PinMismatch { expected, actual }),
        PinVerdict::Unavailable { reason } => Err(Error::PinUnavailable { reason }),
    }
}

/// Recursively copy a fixtures tree into `dst` (which is created). Used by the
/// negative control so the tamper never touches the committed fixtures.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), Error> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Group captured chunks by dimension for the summary.
fn summarize(manifest: &Manifest) {
    let mut dims: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &manifest.captured {
        if let Some(dim) = c.dim.as_deref() {
            *dims.entry(dim).or_default() += 1;
        }
    }

    println!("fixture summary");
    println!("===============");
    if let Some(kind) = &manifest.kind {
        println!("kind:                  {kind}");
    }
    if let Some(seed) = &manifest.seed {
        println!("seed:                  {seed}");
    }
    if let Some(lt) = &manifest.level_type {
        println!("level-type:            {lt}");
    }
    if let Some(paper) = &manifest.paper {
        println!("paper:                 {paper}");
    }
    if let Some(comp) = &manifest.region_file_compression {
        println!("region-file-compression: {comp}");
    }
    println!("format:                {}", manifest.format);
    println!("captured files:        {}", manifest.captured.len());
    if let Some(cc) = manifest.chunk_count {
        println!("chunk-count (manifest): {cc}");
    }
    if !dims.is_empty() {
        println!("chunks per dimension:");
        for (dim, n) in &dims {
            println!("  {dim:>12}: {n}");
        }
    }
}

fn verify_fixtures_dir(dir: &Path) -> Result<(), Error> {
    println!("verifying fixtures at {}", dir.display());
    if !dir.is_dir() {
        return Err(Error::Manifest(format!(
            "fixtures dir {} does not exist — run scripts/extract_fixtures.py first",
            dir.display()
        )));
    }
    let manifest = verify_fixtures(dir)?;
    println!(
        "OK: all {} captured files match manifest SHA-256s",
        manifest.captured.len()
    );
    summarize(&manifest);
    Ok(())
}

/// Every fixture *kind* that carries its own manifest under `fixtures/`: the
/// M0 root (`fixtures/manifest.json`) plus each subdir with a `manifest.json`
/// (recursive — `fixtures/worldgen/` and `fixtures/regions/overworld-normal/`
/// both qualify today, and kinds may nest arbitrarily). Kinds verify
/// independently and can grow without a format migration.
fn all_fixture_manifests() -> Vec<PathBuf> {
    let root = crate_dir().join("fixtures");
    let mut out = Vec::new();
    if root.join("manifest.json").is_file() {
        out.push(root.clone());
    }
    let mut walk = vec![root.clone()];
    while let Some(dir) = walk.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if path.join("manifest.json").is_file() {
                    out.push(path.clone());
                }
                walk.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The default (no-arg) mode: verify every committed fixture kind.
fn verify_all_fixture_kinds() -> Result<(), Error> {
    let kinds = all_fixture_manifests();
    if kinds.is_empty() {
        return Err(Error::Manifest(
            "no fixture manifests found under fixtures/ (run scripts/extract_fixtures.py first)"
                .into(),
        ));
    }
    println!("verifying all committed fixture kinds:");
    for d in &kinds {
        let rel = d
            .strip_prefix(crate_dir())
            .unwrap_or(d)
            .display()
            .to_string();
        println!("  - {rel}");
    }
    println!();
    let mut failed = 0;
    for d in &kinds {
        match verify_fixtures_dir(d) {
            Ok(()) => println!(),
            Err(e) => {
                eprintln!("rivet-oracle: {e}");
                eprintln!();
                failed += 1;
            }
        }
    }
    if failed != 0 {
        return Err(Error::Manifest(format!(
            "{failed} of {} fixture kinds failed verification",
            kinds.len()
        )));
    }
    println!(
        "PASS: all {} fixture kinds match their manifest SHA-256s",
        kinds.len()
    );
    Ok(())
}

/// Send a signal to `pid` via the POSIX `kill` utility.
///
/// Uses `kill` (a standard utility on macOS/Linux) rather than a signal crate
/// so this crate stays std-only-plus-{serde,serde_json,sha2} (per the README)
/// and no shared `Cargo.lock`/workspace changes are needed.
fn signal_process(pid: u32, signal: &str) -> io::Result<()> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "kill -{signal} {pid} exited with {status}"
        )))
    }
}

/// Locate the paperclip jar: `RIVET_ORACLE_JAR` env wins, then a local copy in
/// `work/jars/`, then copy it from `working/Paper` if present (main checkout).
fn ensure_jar() -> Result<PathBuf, Error> {
    if let Ok(p) = env::var("RIVET_ORACLE_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(Error::Gate(format!(
            "RIVET_ORACLE_JAR is set to {} but it is not a file",
            p.display()
        )));
    }
    let crate_root = crate_dir();
    let local = crate_root.join("work").join("jars").join(PAPERCLIP_JAR);
    if local.is_file() {
        return Ok(local);
    }
    let from_source = crate_root
        .join("../../working/Paper/paper-server/build/libs")
        .join(PAPERCLIP_JAR);
    if from_source.is_file() {
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from_source, &local)?;
        println!("copied {} -> {}", from_source.display(), local.display());
        return Ok(local);
    }
    Err(Error::Gate(format!(
        "Paper paperclip jar not found. Looked at {} and {}. \
         Copy it into work/jars/ or set RIVET_ORACLE_JAR.",
        local.display(),
        from_source.display()
    )))
}

/// Prepare a clean scratch run dir under `work/verify/`.
///
/// Paperclip materializes `libraries/`, `versions/`, `cache/` (~160MB) into
/// the run dir on first boot. When those already exist we reuse them (a re-run
/// boots in ~7s instead of ~30s) and wipe everything else so the world is
/// always fresh. `server_properties_src` is the committed exact config
/// (M0 superflat: `fixtures/server.properties`; M2 normal overworld:
/// `fixtures/server-normal.properties`), guaranteeing config parity by
/// construction.
fn prepare_run_dir(run_dir: &Path, server_properties_src: &Path) -> Result<(), Error> {
    let libs = run_dir.join("libraries");
    let reuse_libs = libs.is_dir()
        && fs::read_dir(&libs)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);

    if run_dir.exists() {
        if reuse_libs {
            for entry in fs::read_dir(run_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if matches!(name.as_str(), "libraries" | "versions" | "cache") {
                    continue;
                }
                let p = entry.path();
                if p.is_dir() {
                    fs::remove_dir_all(&p)?;
                } else {
                    fs::remove_file(&p)?;
                }
            }
        } else {
            fs::remove_dir_all(run_dir)?;
            fs::create_dir_all(run_dir)?;
        }
    } else {
        fs::create_dir_all(run_dir)?;
    }

    fs::copy(server_properties_src, run_dir.join("server.properties"))?;
    fs::write(run_dir.join("eula.txt"), EULA)?;
    Ok(())
}

/// Poll the boot log until the server prints `Done (...)! For help, ...`.
///
/// Returns the byte offset in the log at the moment Done was seen, so the
/// caller can later inspect only the *post-Done* tail for the clean-save
/// marker. Kills the child and errors on timeout or premature exit.
fn wait_for_done(child: &mut Child, log_path: &Path) -> Result<usize, Error> {
    let deadline = Instant::now() + BOOT_TIMEOUT;
    let pid = child.id();
    loop {
        if Instant::now() >= deadline {
            let _ = signal_process(pid, "KILL");
            let _ = child.wait();
            return Err(Error::Gate(format!(
                "timed out after {:?} waiting for the server to reach 'Done (...)!' — see {}",
                BOOT_TIMEOUT,
                log_path.display()
            )));
        }
        if let Some(status) = child.try_wait()? {
            return Err(Error::Gate(format!(
                "server process exited ({status}) before reaching 'Done' — see {}",
                log_path.display()
            )));
        }
        if let Ok(text) = fs::read_to_string(log_path)
            && text.contains("Done (")
            && text.contains("For help, type \"help\"")
        {
            let offset = fs::metadata(log_path)
                .map(|m| m.len() as usize)
                .unwrap_or(text.len());
            return Ok(offset);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Wait for `child` to exit, SIGKILLing after `timeout`.
fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return Ok(()),
            None => {
                if Instant::now() >= deadline {
                    let _ = signal_process(child.id(), "KILL");
                    child.wait()?;
                    return Err(Error::Gate(format!(
                        "server did not exit after SIGTERM within {timeout:?}; killed with SIGKILL"
                    )));
                }
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

/// Boot a fresh Paper run, wait for `Done`, then shut it down cleanly.
///
/// stdout+stderr are teed to `log_path`. The server's world save happens both
/// at `Done` and again on SIGTERM shutdown; we verify the post-Done log tail
/// contains `All dimensions are saved` so an unclean save is caught, not
/// silently diffed.
fn boot_and_shutdown(run_dir: &Path, log_path: &Path, jar: &Path) -> Result<(), Error> {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    let log_err = log_file.try_clone()?;
    let mut child = Command::new("java")
        .args(["-Xms512M", "-Xmx2G", "-jar"])
        .arg(jar)
        .arg("nogui")
        .current_dir(run_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| {
            Error::Gate(format!(
                "failed to spawn java: {e} (is a Java 25+ JRE on PATH?)"
            ))
        })?;
    let pid = child.id();

    let done_offset = wait_for_done(&mut child, log_path)?;
    println!("      server ready ('Done'); shutting down cleanly (SIGTERM)...");
    // Let trailing delayed-init / chunk I/O settle before stopping.
    thread::sleep(Duration::from_millis(1500));
    let _ = signal_process(pid, "TERM");
    wait_for_exit(&mut child, SHUTDOWN_TIMEOUT)?;

    let bytes = fs::read(log_path)?;
    let tail = if bytes.len() > done_offset {
        String::from_utf8_lossy(&bytes[done_offset..]).into_owned()
    } else {
        String::new()
    };
    if !tail.contains("All dimensions are saved") {
        return Err(Error::Gate(
            "server shut down without a clean save ('All dimensions are saved' missing from post-Done log tail)".into(),
        ));
    }
    Ok(())
}

/// Run `scripts/extract_fixtures.py` against a completed run's world dir.
///
/// We call the small, already-tested Python script as a subprocess rather than
/// porting its region-file parsing to Rust: it is deterministic, needs no new
/// Rust deps, and its JSON manifest is the same shape we already read. The M2
/// region capture passes `--chunks-only` so only the deterministic chunk-NBT
/// payloads (no level.dat / server.properties wall-clock copies) are emitted —
/// regeneration stays git-clean.
fn extract_fresh_fixtures(
    world_dir: &Path,
    out_dir: &Path,
    chunks_only: bool,
) -> Result<(), Error> {
    let script = crate_dir().join("scripts/extract_fixtures.py");
    let mut cmd = Command::new("python3");
    cmd.arg(&script).arg(world_dir).arg(out_dir);
    if chunks_only {
        cmd.arg("--chunks-only");
    }
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Gate(format!("failed to run python3 {}: {e}", script.display())))?;
    if !out.status.success() {
        return Err(Error::Gate(format!(
            "extract_fixtures.py failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Boot a fresh Paper run in `run_dir` and extract its deterministic chunk-NBT
/// slice into a temp dir. Returns the temp extraction dir (caller owns
/// cleanup). Shared by the `verify` gates and negative controls.
fn fresh_extraction(run_dir: &Path, jar: &Path, cfg: &BootConfig) -> Result<PathBuf, Error> {
    let log_path = run_dir.with_file_name("boot.log");
    prepare_run_dir(run_dir, &cfg.props_src)?;
    boot_and_shutdown(run_dir, &log_path, jar)?;
    let tmp = env::temp_dir().join(format!("rivet-oracle-verify-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    extract_fresh_fixtures(&run_dir.join("world"), &tmp, cfg.chunks_only)?;
    Ok(tmp)
}

/// Make a scratch copy of a baseline fixtures dir and corrupt one known chunk
/// payload (flip a byte) *and* that chunk's recorded SHA-256, so the copy is
/// internally consistent (a plausible but wrong baseline). Returns the
/// repo-relative path of the tampered chunk.
fn tamper_baseline_copy(baseline_dir: &Path, scratch: &Path) -> Result<String, Error> {
    copy_dir_recursive(baseline_dir, scratch)?;
    let mut root: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(scratch.join("manifest.json"))?)
            .map_err(|e| Error::Gate(format!("corrupted-copy manifest is unparsable: {e}")))?;
    let captured = root
        .get_mut("captured")
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| Error::Gate("corrupted-copy manifest has no `captured` list".into()))?;
    let idx = captured
        .iter()
        .position(|e| e.get("dim").is_some())
        .ok_or_else(|| Error::Gate("baseline has no chunk fixtures to tamper".into()))?;
    let rel = captured[idx]
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::Gate("chunk entry has no path".into()))?
        .to_string();

    let file = scratch.join(&rel);
    let payload = fs::read(&file)?;
    if payload.is_empty() {
        return Err(Error::Gate(format!(
            "chunk {rel} is empty — cannot tamper a payload to prove divergence"
        )));
    }
    let mut tampered = payload.clone();
    let i = (tampered.len() / 2).min(tampered.len().saturating_sub(1));
    tampered[i] ^= 0xFF;
    fs::write(&file, &tampered)?;

    captured[idx]["sha256"] = serde_json::Value::String(sha256_hex(&tampered));
    captured[idx]["bytes"] = serde_json::Value::Number(tampered.len().into());
    fs::write(
        scratch.join("manifest.json"),
        serde_json::to_string_pretty(&root)
            .map_err(|e| Error::Gate(format!("cannot serialize corrupted-copy manifest: {e}")))?,
    )?;
    Ok(rel)
}

/// The `--expect-fail` negative control passes only when the boot -> extract ->
/// diff pipeline detected *and named* the tampered chunk. A clean diff (false
/// negative — pipeline missed the tamper) or a divergence that does not name
/// the tampered chunk (e.g. an unrelated worldgen drift, or a boot/gate error
/// masquerading as divergence) must NOT satisfy the control.
fn negative_control_accepts(diff: &ChunkDiff, tampered_path: &str) -> bool {
    diff.mismatched.iter().any(|(p, _, _)| p == tampered_path)
}

/// `verify --expect-fail`: the end-to-end negative control for a boot gate.
///
/// Proves the full boot -> extract -> pin-check -> diff pipeline is not
/// vacuously green: it diffs a fresh Paper boot against a *deliberately
/// corrupted* copy of the committed baseline (never the committed fixtures
/// themselves) and exits 0 only when the divergence is detected and the
/// tampered chunk is named. Any other outcome — clean diff, unnamed
/// divergence, pin mismatch/unavailable, boot or extraction failure — exits
/// nonzero with a distinct message. The `cfg` selects which boot config
/// (M0 superflat or M2 normal-overworld) and which baseline the control runs
/// against.
fn run_verify_negative_control(cfg: &BootConfig) -> Result<(), Error> {
    let crate_root = crate_dir();
    let jar = ensure_jar()?;
    let run_dir = crate_root.join("work/verify/run");
    let baseline_dir = &cfg.baseline;

    println!("oracle negative control: verify --expect-fail");
    println!("   gate mode     : {}", cfg.title);
    println!("   baseline      : {}", baseline_dir.display());
    println!("   paperclip jar : {}", jar.display());
    println!();

    let scratch = env::temp_dir().join(format!("rivet-oracle-negcontrol-{}", std::process::id()));
    let tampered = tamper_baseline_copy(baseline_dir, &scratch)?;
    println!(
        "[0/4] baseline copied to {} and chunk {tampered} corrupted (byte flipped)",
        scratch.display()
    );

    println!(
        "[1/4] booting a fresh Paper run (scratch world in {})...",
        run_dir.display()
    );
    let tmp = fresh_extraction(&run_dir, &jar, cfg)?;
    println!("[2/4] world saved cleanly; extracted deterministic chunk slice.");

    // The control is meaningless against a stale/unverifiable Paper (the pin
    // check would already fail `verify`, so a nonzero here proves nothing).
    // Checked after the boot so the pin is read from the jar that actually ran.
    check_pin(baseline_dir, &run_dir)?;

    println!("[3/4] diffing fresh chunk-NBT hashes against the corrupted baseline...");
    let baseline = load_manifest(&scratch)?;
    let fresh = load_manifest(&tmp)?;
    let diff = diff_chunk_hashes(&baseline, &fresh);

    println!();
    if negative_control_accepts(&diff, &tampered) {
        println!(
            "PASS: the boot->extract->diff pipeline detected the tampered chunk {tampered} \
             ({} of {} chunks matched the corrupted baseline).",
            diff.matched(),
            diff.expected
        );
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&scratch);
        Ok(())
    } else if diff.is_clean() {
        let _ = fs::remove_dir_all(&scratch);
        Err(Error::NegativeControl {
            message: format!(
                "negative control FAILED: the pipeline reported ZERO divergence against a \
                 baseline whose chunk {tampered} was corrupted — the boot->extract->diff \
                 chain is vacuously green and cannot be trusted.\n\
                 fresh extraction (kept for inspection): {}",
                tmp.display()
            ),
        })
    } else {
        let named: Vec<String> = diff.mismatched.iter().map(|(p, _, _)| p.clone()).collect();
        let _ = fs::remove_dir_all(&scratch);
        Err(Error::NegativeControl {
            message: format!(
                "negative control FAILED: the pipeline diverged but did not name the \
                 tampered chunk {tampered} (mismatched: {named:?}) — the divergence was \
                 detected for the wrong reason.\n\
                 fresh extraction (kept for inspection): {}",
                tmp.display()
            ),
        })
    }
}

/// Compare the chunk-NBT payload hashes of a fresh boot against a baseline.
///
/// Only entries carrying a `dim` field are compared — those are the
/// deterministic decompressed chunk NBT payloads. level.dat / server.properties
/// are excluded (wall-clock timestamps).
fn diff_chunk_hashes(baseline: &Manifest, fresh: &Manifest) -> ChunkDiff {
    let mut b: BTreeMap<&str, &str> = BTreeMap::new();
    for c in &baseline.captured {
        if c.dim.is_some() {
            b.insert(&c.path, &c.sha256);
        }
    }
    let mut f: BTreeMap<&str, &str> = BTreeMap::new();
    for c in &fresh.captured {
        if c.dim.is_some() {
            f.insert(&c.path, &c.sha256);
        }
    }
    let mut d = ChunkDiff {
        expected: b.len(),
        actual: f.len(),
        ..Default::default()
    };
    for (path, want) in &b {
        match f.get(path) {
            Some(got) if got == want => {}
            Some(got) => {
                d.mismatched
                    .push(((*path).to_string(), (*want).to_string(), (*got).to_string()))
            }
            None => d.missing.push((*path).to_string()),
        }
    }
    for path in f.keys() {
        if !b.contains_key(path) {
            d.extra.push((*path).to_string());
        }
    }
    d
}

fn print_chunk_diff(diff: &ChunkDiff, baseline: &Manifest) {
    println!(
        "FAIL: {} of {} chunk NBT payloads match the committed golden baseline",
        diff.matched(),
        diff.expected
    );
    println!(
        "      baseline: seed {} / {} ({} chunk payloads); fresh run: {}",
        baseline.seed.as_deref().unwrap_or("?"),
        baseline.level_type.as_deref().unwrap_or("?"),
        diff.expected,
        diff.actual
    );
    if diff.mismatched.len() + diff.missing.len() + diff.extra.len() > 0 {
        println!();
    }
    for (path, want, got) in &diff.mismatched {
        println!("  mismatch   {path}");
        println!("    expected: {want}");
        println!("    actual:   {got}");
    }
    for p in &diff.missing {
        println!("  missing in fresh run: {p}");
    }
    for p in &diff.extra {
        println!("  extra in fresh run:   {p}");
    }
    println!();
    println!("A diff means this fresh Paper boot is NOT byte-identical to the committed golden");
    println!("baseline. Do not fudge fixtures — investigate (see work/verify/boot.log and the");
    println!("fresh extraction dir).");
}

/// The one-command sanity gate: boot -> extract -> pin-check -> diff -> verdict.
///
/// `cfg` selects the boot config + baseline: `m0_config()` is the superflat M0
/// gate; `m2_config()` is the normal-overworld none-compression region gate
/// (proving two fresh boots of the M2 capture match byte-for-byte).
fn run_verify_gate(cfg: &BootConfig) -> Result<(), Error> {
    let crate_root = crate_dir();
    let jar = ensure_jar()?;
    let run_dir = crate_root.join("work/verify/run");
    let baseline_dir = &cfg.baseline;

    println!("oracle gate");
    println!("   gate mode     : {}", cfg.title);
    println!("   baseline      : {}", baseline_dir.display());
    println!("   paperclip jar : {}", jar.display());
    println!();

    println!(
        "[1/4] booting a fresh Paper run (scratch world in {})...",
        run_dir.display()
    );
    let tmp = fresh_extraction(&run_dir, &jar, cfg)?;
    println!("[2/4] world saved cleanly; extracted deterministic chunk slice.");

    check_pin(baseline_dir, &run_dir)?;

    println!("[3/4] diffing fresh chunk-NBT hashes against the baseline...");
    let baseline = load_manifest(baseline_dir)?;
    let fresh = load_manifest(&tmp)?;
    let diff = diff_chunk_hashes(&baseline, &fresh);

    println!();
    if diff.is_clean() {
        println!(
            "PASS: {}/{} chunk NBT payloads are byte-identical to the committed golden baseline",
            diff.expected, diff.expected
        );
        println!(
            "      (seed {} / {}) — {}.",
            baseline.seed.as_deref().unwrap_or("?"),
            baseline.level_type.as_deref().unwrap_or("?"),
            cfg.title
        );
        let _ = fs::remove_dir_all(&tmp);
        Ok(())
    } else {
        print_chunk_diff(&diff, &baseline);
        println!("fresh extraction (kept for inspection): {}", tmp.display());
        Err(Error::Diff(diff))
    }
}

/// Rewrite `fixtures/worldgen/manifest.json` from the freshly generated
/// samples. The seed + Paper pin are read back out of `samples.json` (the
/// sampler records them) so the manifest always describes what was actually
/// generated. The manifest is serialized in the exact committed field order, so
/// regeneration is byte-identical (git-clean) given unchanged samples.
fn regenerate_worldgen_manifest(wg_dir: &Path) -> Result<(), Error> {
    let samples_path = wg_dir.join("samples.json");
    let samples: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&samples_path)
            .map_err(|e| Error::Manifest(format!("{} missing: {e}", samples_path.display())))?,
    )
    .map_err(|e| Error::Manifest(format!("{} unparsable: {e}", samples_path.display())))?;
    let seed = samples
        .get("seed")
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
        .unwrap_or(42)
        .to_string();
    let paper = samples
        .get("paper")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut captured = Vec::new();
    for name in ["samples.json", "light.json"] {
        let data = fs::read(wg_dir.join(name))
            .map_err(|e| Error::Manifest(format!("{name} missing: {e}")))?;
        captured.push(WorldgenCaptured {
            path: name.to_string(),
            sha256: sha256_hex(&data),
            bytes: data.len(),
        });
    }

    let manifest = WorldgenManifest {
        format: 1,
        paper: &paper,
        seed: &seed,
        level_type: "minecraft:normal",
        kind: "worldgen-samples",
        note: "semantic density/biome/surface samples (Paper-side sampler) + Starlight \
               light samples (M0 FULL superflat chunks). Deterministic across boots for \
               the pinned Paper + seed.",
        captured,
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize worldgen manifest: {e}")))?;
    text.push('\n');
    fs::write(wg_dir.join("manifest.json"), text)?;
    println!("rewrote {}", wg_dir.join("manifest.json").display());
    Ok(())
}

/// Regenerate the `worldgen/` semantic fixtures: run the Paper-side sampler
/// (samples.json), re-extract the Starlight light samples from the M0 FULL
/// superflat chunks (light.json), then rewrite the manifest. No full server
/// boot — only the materialized Paper runtime + the M0 chunk fixtures are
/// needed.
fn regenerate_samples() -> Result<(), Error> {
    let crate_root = crate_dir();
    let wg = crate_root.join("fixtures/worldgen");
    fs::create_dir_all(&wg)?;

    // 1. Paper-side semantic sampler -> fixtures/worldgen/samples.json
    let sampler = crate_root.join("scripts/run_worldgen_sampler.sh");
    let status = Command::new("bash")
        .arg(&sampler)
        .arg(&wg)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| Error::Gate(format!("failed to run {}: {e}", sampler.display())))?;
    if !status.success() {
        return Err(Error::Gate(format!(
            "run_worldgen_sampler.sh exited {status} — see its stderr"
        )));
    }

    // 2. Starlight light samples from the M0 FULL superflat chunk fixtures.
    let light = crate_root.join("scripts/extract_light_samples.py");
    let chunk_dir = crate_root.join("fixtures/chunk");
    let out = Command::new("python3")
        .arg(&light)
        .arg(&chunk_dir)
        .arg(wg.join("light.json"))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Gate(format!("failed to run python3 {}: {e}", light.display())))?;
    if !out.status.success() {
        return Err(Error::Gate(format!(
            "extract_light_samples.py failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    // 3. Rewrite the worldgen manifest from the fresh hashes.
    regenerate_worldgen_manifest(&wg)?;
    println!("regenerated worldgen semantic samples (samples.json, light.json, manifest.json)");
    Ok(())
}

/// Regenerate the M0 golden chunk slice: boot a fresh superflat run and extract
/// the deterministic chunk-NBT payloads (+ level.dat / server.properties /
/// manifest.json) straight into `fixtures/`. The gate's hash verification is
/// the safety net against a bad regeneration.
fn regenerate_m0() -> Result<(), Error> {
    let jar = ensure_jar()?;
    let run_dir = crate_dir().join("work/verify/run");
    let props = crate_dir().join("fixtures/server.properties");
    let log_path = run_dir.with_file_name("boot.log");
    prepare_run_dir(&run_dir, &props)?;
    boot_and_shutdown(&run_dir, &log_path, &jar)?;
    extract_fresh_fixtures(&run_dir.join("world"), &crate_dir().join("fixtures"), false)?;
    println!("regenerated M0 golden chunk slice under fixtures/");
    Ok(())
}

/// Regenerate the M2 normal-overworld none-compression region payloads: boot a
/// fresh run with `fixtures/server-normal.properties` (region-file-compression
/// = none) and extract the deterministic chunk-NBT slice with `--chunks-only`
/// into `fixtures/regions/overworld-normal/`.
fn regenerate_m2() -> Result<(), Error> {
    let jar = ensure_jar()?;
    let run_dir = crate_dir().join("work/verify/run");
    let props = crate_dir().join("fixtures/server-normal.properties");
    let log_path = run_dir.with_file_name("boot.log");
    prepare_run_dir(&run_dir, &props)?;
    boot_and_shutdown(&run_dir, &log_path, &jar)?;
    extract_fresh_fixtures(
        &run_dir.join("world"),
        &crate_dir().join("fixtures/regions/overworld-normal"),
        true,
    )?;
    println!(
        "regenerated M2 normal-overworld region payloads under fixtures/regions/overworld-normal/"
    );
    Ok(())
}

/// `regenerate`: full regeneration of every fixture kind (or a sub-selection
/// via `--m0` / `--m2` / `--samples`).
fn run_regenerate(only: &[&str]) -> Result<(), Error> {
    let m0 = only.is_empty() || only.contains(&"--m0");
    let m2 = only.is_empty() || only.contains(&"--m2");
    let samples = only.is_empty() || only.contains(&"--samples");
    for flag in only {
        if !matches!(*flag, "--m0" | "--m2" | "--samples") {
            return Err(Error::Gate(format!("unknown regenerate flag: {flag}")));
        }
    }
    if m0 {
        println!("==> regenerating M0 golden chunk slice");
        regenerate_m0()?;
    }
    if m2 {
        println!("==> regenerating M2 normal-overworld region payloads");
        regenerate_m2()?;
    }
    if samples {
        println!("==> regenerating worldgen semantic samples");
        regenerate_samples()?;
    }
    Ok(())
}

fn print_usage() {
    println!("rivet-oracle — the M0/M2 differential-test harness");
    println!();
    println!("USAGE:");
    println!(
        "  cargo run -p rivet-oracle                       verify ALL committed fixture kinds"
    );
    println!(
        "                                                     (M0 slice + worldgen/ + regions/overworld-normal/)"
    );
    println!(
        "  cargo run -p rivet-oracle -- <dir>          verify <dir> against its manifest.json"
    );
    println!(
        "  cargo run -p rivet-oracle -- verify         M0 gate: boot fresh Paper -> extract -> diff"
    );
    println!("  cargo run -p rivet-oracle -- verify --m2    M2 region gate (normal-overworld");
    println!(
        "                                                     none-compression region parity)"
    );
    println!("  cargo run -p rivet-oracle -- verify --expect-fail [dir]");
    println!(
        "                                             M0 negative control: diff against a corrupted"
    );
    println!(
        "                                             copy of the baseline; exits 0 only when the"
    );
    println!("                                             tampered chunk is detected AND named");
    println!("  cargo run -p rivet-oracle -- verify --m2 --expect-fail [dir]");
    println!(
        "                                             M2 negative control against the region baseline"
    );
    println!(
        "  cargo run -p rivet-oracle -- sample         regenerate worldgen/ semantic samples + manifest"
    );
    println!("  cargo run -p rivet-oracle -- regenerate     regenerate ALL fixture kinds");
    println!("                                             (sub-select: --m0 / --m2 / --samples)");
    println!();
    println!(
        "Every gate mode enforces the manifest's pinned Paper commit (fixtures/**/manifest.json"
    );
    println!(
        "paper: ...@<commit>) against the Git-Commit attribute of the server jar the paperclip"
    );
    println!(
        "actually materialized and booted (work/verify/run/versions/26.2/paper-26.2.jar); a stale"
    );
    println!("or unverifiable Paper fails loudly, never silently.");
    println!();
    println!("ENV:");
    println!("  RIVET_ORACLE_JAR   path to the paperclip jar");
    println!("                     (default: work/jars/, or copied from working/Paper/)");
}

fn run() -> Result<(), Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("verify") => {
            let mut m2 = false;
            let mut expect_fail = false;
            let mut rest: Vec<String> = Vec::new();
            for a in args.iter().skip(1) {
                match a.as_str() {
                    "--m2" => m2 = true,
                    "--expect-fail" => expect_fail = true,
                    other => rest.push(other.to_string()),
                }
            }
            let cfg = if m2 { m2_config() } else { m0_config() };
            // A custom baseline dir wins over the mode default.
            let cfg = if let Some(dir) = rest.first() {
                BootConfig {
                    baseline: PathBuf::from(dir),
                    ..cfg
                }
            } else {
                cfg
            };
            if expect_fail {
                run_verify_negative_control(&cfg)
            } else {
                run_verify_gate(&cfg)
            }
        }
        Some("sample") => regenerate_samples(),
        Some("regenerate") => {
            let flags: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
            run_regenerate(&flags)
        }
        Some("--help") | Some("-h") => {
            print_usage();
            Ok(())
        }
        _ => {
            let dir = args
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| crate_dir().join("fixtures"));
            if dir == crate_dir().join("fixtures") {
                verify_all_fixture_kinds()
            } else {
                verify_fixtures_dir(&dir)
            }
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("rivet-oracle: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// The committed M0 fixtures must verify clean against their manifest.
    #[test]
    fn committed_fixtures_verify() {
        let dir = fixtures_dir();
        if !dir.is_dir() {
            // Fixtures aren't checked out (or were pruned) — nothing to verify.
            return;
        }
        let manifest = verify_fixtures(&dir).expect("fixtures should match manifest");
        assert_eq!(manifest.format, 1);
        assert!(
            !manifest.captured.is_empty(),
            "manifest lists at least one file"
        );
        assert!(
            manifest.chunk_count.unwrap_or(0) > 0,
            "manifest records a nonzero chunk count"
        );
    }

    /// The committed `worldgen/` semantic-sample fixtures must verify clean and
    /// carry the pinned Paper provenance.
    #[test]
    fn worldgen_manifest_verify() {
        let dir = fixtures_dir().join("worldgen");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let manifest = verify_fixtures(&dir).expect("worldgen fixtures should match manifest");
        assert_eq!(manifest.kind.as_deref(), Some("worldgen-samples"));
        assert_eq!(
            parse_paper_pin(manifest.paper.as_deref()),
            Some("0a99345".into())
        );
        assert_eq!(manifest.captured.len(), 2);
        for cap in &manifest.captured {
            assert!(
                cap.path == "samples.json" || cap.path == "light.json",
                "unexpected worldgen capture {}",
                cap.path
            );
        }
    }

    /// The committed `regions/overworld-normal` none-compression region fixtures
    /// must verify clean: 408 chunk payloads across all three dimensions.
    #[test]
    fn regions_normal_manifest_verify() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let manifest = verify_fixtures(&dir).expect("region fixtures should match manifest");
        assert_eq!(manifest.chunk_count, Some(408));
        assert_eq!(
            manifest.region_file_compression.as_deref(),
            Some("none"),
            "D13: normal-overworld region captures use region-file-compression=none"
        );
        assert_eq!(manifest.level_type.as_deref(), Some("minecraft\\:normal"));
        let mut dims: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &manifest.captured {
            if let Some(d) = c.dim.as_deref() {
                *dims.entry(d).or_default() += 1;
            }
        }
        assert_eq!(dims.get("overworld"), Some(&120));
        assert_eq!(dims.get("the_nether"), Some(&144));
        assert_eq!(dims.get("the_end"), Some(&144));
    }

    /// The committed worldgen samples are the expected semantic shape: 25
    /// density + 22 biome + 16 surface entries at seed 42.
    #[test]
    fn worldgen_samples_shape() {
        let f = fixtures_dir().join("worldgen").join("samples.json");
        if !f.is_file() {
            return;
        }
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["seed"], 42);
        assert_eq!(v["format"], 1);
        assert_eq!(v["dimension"], "overworld");
        assert_eq!(v["generator"], "normal");
        assert!(v["density"].as_array().unwrap().len() >= 25);
        assert!(v["biome"].as_array().unwrap().len() >= 22);
        assert!(v["surface"].as_array().unwrap().len() >= 16);
    }

    /// Regenerating the worldgen manifest in Rust is byte-identical to the
    /// committed manifest (given unchanged samples) — regeneration is git-clean
    /// and the committed manifest is what the writer would produce.
    #[test]
    fn worldgen_manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("worldgen");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-wg-regen-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        for name in ["samples.json", "light.json"] {
            fs::copy(dir.join(name), scratch.join(name)).unwrap();
        }
        regenerate_worldgen_manifest(&scratch).unwrap();
        let committed = fs::read(dir.join("manifest.json")).unwrap();
        let regenerated = fs::read(scratch.join("manifest.json")).unwrap();
        assert_eq!(
            committed, regenerated,
            "regenerating the worldgen manifest must be byte-identical (git-clean)"
        );
        // And the regenerated manifest is self-consistent: it verifies its files.
        verify_fixtures(&scratch).unwrap();
        let _ = fs::remove_dir_all(&scratch);
    }

    /// Tampering with a captured file must be detected as a hash mismatch.
    ///
    /// Operates on a copy of the fixtures in a temp dir so the tamper never
    /// mutates the shared `fixtures/` (tests run in parallel and the committed
    /// verify test would race with the mutation).
    #[test]
    fn detects_hash_mismatch() {
        let dir = fixtures_dir();
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.is_file() {
            return;
        }
        let manifest: Manifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let first = manifest
            .captured
            .first()
            .expect("at least one captured file");
        if !dir.join(&first.path).is_file() {
            return;
        }

        // Copy the fixtures to a scratch dir and tamper there.
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-tamper-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        for c in &manifest.captured {
            let src = dir.join(&c.path);
            let dst = scratch.join(&c.path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(&src, &dst).unwrap();
        }

        let file = scratch.join(&first.path);
        let orig = fs::read(&file).unwrap();
        let mut tampered = orig.clone();
        tampered.push(0xFF);
        fs::write(&file, &tampered).unwrap();
        let result = verify_fixtures(&scratch);
        let _ = fs::remove_dir_all(&scratch);

        assert!(result.is_err(), "tampered fixture should fail verification");
        match result {
            Err(Error::HashMismatch { .. }) | Err(Error::Manifest(_)) => {}
            Err(other) => panic!("unexpected error kind: {other}"),
            Ok(_) => unreachable!(),
        }
    }

    /// Diffing the committed manifest against itself is clean and counts 432
    /// chunk payloads (the M0 golden slice).
    #[test]
    fn diff_chunk_hashes_clean_when_identical() {
        let dir = fixtures_dir();
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let m = load_manifest(&dir).unwrap();
        let d = diff_chunk_hashes(&m, &m);
        assert!(d.is_clean(), "identical manifests must diff clean");
        assert_eq!(d.expected, 432);
        assert_eq!(d.actual, 432);
    }

    /// Corrupting one chunk hash in a copy of the manifest is reported as a
    /// single mismatched chunk, not a PASS.
    #[test]
    fn diff_chunk_hashes_detects_tamper() {
        let dir = fixtures_dir();
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.is_file() {
            return;
        }
        let m = load_manifest(&dir).unwrap();
        let mut m2: Manifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let idx = m2
            .captured
            .iter()
            .position(|c| c.dim.is_some())
            .expect("manifest has chunk files");
        m2.captured[idx].sha256 = "0".repeat(64);
        let d = diff_chunk_hashes(&m, &m2);
        assert!(!d.is_clean());
        assert_eq!(d.mismatched.len(), 1);
        assert_eq!(d.missing.len(), 0);
        assert_eq!(d.extra.len(), 0);
        assert_eq!(d.mismatched[0].0, m2.captured[idx].path);
    }

    /// The manifest's `paper` provenance string carries the pinned commit.
    #[test]
    fn pin_provenance_match() {
        assert_eq!(
            parse_paper_pin(Some("26.2-DEV-main@0a99345")),
            Some("0a99345".into())
        );
        assert_eq!(
            parse_paper_pin(Some("26.2-DEV-main@abc1234")),
            Some("abc1234".into())
        );
        // A manifest without an `@<commit>` pin carries no enforceable commit.
        assert_eq!(parse_paper_pin(Some("26.2-DEV-main")), None);
        assert_eq!(parse_paper_pin(Some("26.2-DEV-main@")), None);
        assert_eq!(parse_paper_pin(None), None);
    }

    /// The worldgen sampler script honors the documented env overrides
    /// (`RIVET_PAPER_RUNTIME_JAR` / `RIVET_PAPER_LIBRARIES`): the classpath it
    /// builds for javac/java must use the overridden runtime jar and libraries,
    /// not the hardcoded `work/run` defaults. Runs the real script with stub
    /// `javac`/`java` on PATH that record their arguments.
    #[test]
    fn sampler_script_honors_runtime_env_overrides() {
        let script = crate_dir().join("scripts/run_worldgen_sampler.sh");
        if !script.is_file() {
            return; // script not present — nothing to exercise
        }

        // Fake materialized runtime: a paper-26.2.jar + one library jar.
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-sampler-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        let fake_jar = scratch.join("versions/26.2/paper-26.2.jar");
        fs::create_dir_all(fake_jar.parent().unwrap()).unwrap();
        fs::write(&fake_jar, b"fake").unwrap();
        let fake_lib_dir = scratch.join("libraries");
        fs::create_dir_all(&fake_lib_dir).unwrap();
        let fake_lib = fake_lib_dir.join("lib-1.jar");
        fs::write(&fake_lib, b"fake").unwrap();

        // Stub javac/java that append their args to a log, so the test can see
        // the classpath the script actually built.
        let stub_dir = scratch.join("bin");
        fs::create_dir_all(&stub_dir).unwrap();
        let log = scratch.join("args.log");
        let stub = format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" >> {}\n",
            log.display()
        );
        for name in ["javac", "java"] {
            let p = stub_dir.join(name);
            fs::write(&p, &stub).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&p).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&p, perms).unwrap();
            }
        }

        let out_dir = scratch.join("out");
        let status = Command::new("bash")
            .arg(&script)
            .arg(&out_dir)
            .env("RIVET_PAPER_RUNTIME_JAR", &fake_jar)
            .env("RIVET_PAPER_LIBRARIES", &fake_lib_dir)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    stub_dir.display(),
                    env::var("PATH").unwrap_or_default()
                ),
            )
            .status();

        let args = fs::read_to_string(&log).unwrap_or_default();
        let _ = fs::remove_dir_all(&scratch);

        assert!(status.is_ok(), "sampler script should run");
        let status = status.unwrap();
        assert!(
            status.success(),
            "sampler script should exit 0; recorded args:\n{args}"
        );
        assert!(
            args.contains(&fake_jar.display().to_string()),
            "classpath should use RIVET_PAPER_RUNTIME_JAR override; recorded args:\n{args}"
        );
        assert!(
            args.contains(&fake_lib.display().to_string()),
            "classpath should use RIVET_PAPER_LIBRARIES override; recorded args:\n{args}"
        );
    }

    /// A pinned commit matching the server jar's Git-Commit is a Match.
    #[test]
    fn pin_match_when_commits_agree() {
        assert_eq!(
            classify_pin(Some("0a99345".into()), Some("0a99345".into())),
            PinVerdict::Match
        );
    }

    /// A server jar at a different commit is a loud mismatch, never a pass.
    #[test]
    fn pin_mismatch_names_both_commits() {
        match classify_pin(Some("0a99345".into()), Some("deadbeef".into())) {
            PinVerdict::Mismatch { expected, actual } => {
                assert_eq!(expected, "0a99345");
                assert_eq!(actual, "deadbeef");
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    /// Missing pin OR missing commit are both Unavailable — a verify run that
    /// cannot confirm the pin must fail loudly, never pass silently.
    #[test]
    fn pin_unavailable_when_either_side_missing() {
        assert!(matches!(
            classify_pin(None, Some("0a99345".into())),
            PinVerdict::Unavailable { .. }
        ));
        assert!(matches!(
            classify_pin(Some("0a99345".into()), None),
            PinVerdict::Unavailable { .. }
        ));
        assert!(matches!(
            classify_pin(None, None),
            PinVerdict::Unavailable { .. }
        ));
    }

    /// `Git-Commit` extraction handles the exact MANIFEST.MF shape (trailing
    /// CRLF, leading space after the colon) and absent attributes.
    #[test]
    fn manifest_commit_extraction() {
        let mf = "Manifest-Version: 1.0\r\nGit-Commit: 0a99345\r\nSpecification-Version: 26.2\r\n";
        assert_eq!(parse_manifest_commit(mf), Some("0a99345".into()));
        assert_eq!(parse_manifest_commit("Manifest-Version: 1.0\r\n"), None);
        assert_eq!(parse_manifest_commit(""), None);
    }

    /// The negative control must NOT pass on a clean diff: a pipeline that
    /// reports zero divergence against a corrupted baseline is vacuously green
    /// and must be rejected (false-pass prevention).
    #[test]
    fn negative_control_rejects_clean_diff() {
        let clean = ChunkDiff::default();
        assert!(!negative_control_accepts(
            &clean,
            "chunk/overworld/0.0/0.0.nbt"
        ));
        // A diff naming only *other* chunks is divergence detected for the
        // wrong reason — the tampered chunk was not the one named.
        let wrong = ChunkDiff {
            expected: 432,
            actual: 432,
            mismatched: vec![(
                "chunk/overworld/0.0/1.1.nbt".into(),
                "deadbeef".into(),
                "cafebabe".into(),
            )],
            ..Default::default()
        };
        assert!(!negative_control_accepts(
            &wrong,
            "chunk/overworld/0.0/0.0.nbt"
        ));
    }

    /// The negative control accepts a diff that names the tampered chunk.
    #[test]
    fn negative_control_accepts_when_tampered_chunk_named() {
        let d = ChunkDiff {
            expected: 432,
            actual: 432,
            mismatched: vec![(
                "chunk/overworld/0.0/0.0.nbt".into(),
                "aaaa".into(),
                "bbbb".into(),
            )],
            ..Default::default()
        };
        assert!(negative_control_accepts(&d, "chunk/overworld/0.0/0.0.nbt"));
    }

    /// `tamper_baseline_copy` corrupts a copy of the committed fixtures (never
    /// the committed ones) and updates the copy's manifest consistently, so the
    /// copy is a *plausible but wrong* baseline: it verifies clean statically,
    /// but the boot->extract->diff pipeline (fresh boot reproduces the original
    /// chunk hashes) must surface exactly the tampered chunk as the one
    /// divergence.
    #[test]
    fn tamper_baseline_copy_detects_and_leaves_committed_intact() {
        let dir = fixtures_dir();
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.is_file() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-tamper2-{}", std::process::id()));
        let tampered_path = tamper_baseline_copy(&dir, &scratch).expect("copy+tamper succeeds");
        assert!(
            tampered_path.starts_with("chunk/"),
            "must tamper a chunk, got {tampered_path}"
        );

        // The corrupted copy is internally consistent — it passes a static
        // verify against its own manifest. The negative control catches the
        // divergence through the pipeline diff, not a trivially broken copy.
        verify_fixtures(&scratch).expect("corrupted copy is internally consistent");

        // The committed fixtures must still verify clean (never mutated).
        verify_fixtures(&dir).expect("committed fixtures must be untouched");

        // A deterministic fresh boot reproduces the committed (original) chunk
        // hashes, so diffing committed-vs-corrupted yields exactly the tampered
        // chunk as a single mismatch — the divergence the pipeline must name.
        let committed = load_manifest(&dir).unwrap();
        let corrupted = load_manifest(&scratch).unwrap();
        let d = diff_chunk_hashes(&committed, &corrupted);
        assert!(!d.is_clean(), "tamper must produce a divergence");
        assert_eq!(d.mismatched.len(), 1, "exactly one chunk diverged");
        assert_eq!(d.mismatched[0].0, tampered_path);

        let _ = fs::remove_dir_all(&scratch);
    }

    /// The same tamper mechanics work against the M2 region baseline: the
    /// corrupted copy is internally consistent, the committed fixtures stay
    /// clean, and the divergence names exactly the tampered region chunk.
    #[test]
    fn tamper_baseline_copy_detects_for_region_baseline() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-tamper3-{}", std::process::id()));
        let tampered_path = tamper_baseline_copy(&dir, &scratch).expect("copy+tamper succeeds");
        assert!(
            tampered_path.starts_with("chunk/"),
            "must tamper a chunk, got {tampered_path}"
        );
        verify_fixtures(&scratch).expect("corrupted region copy is internally consistent");
        verify_fixtures(&dir).expect("committed region fixtures must be untouched");
        let committed = load_manifest(&dir).unwrap();
        let corrupted = load_manifest(&scratch).unwrap();
        let d = diff_chunk_hashes(&committed, &corrupted);
        assert!(!d.is_clean(), "tamper must produce a divergence");
        assert_eq!(d.mismatched.len(), 1, "exactly one region chunk diverged");
        assert_eq!(d.mismatched[0].0, tampered_path);
        let _ = fs::remove_dir_all(&scratch);
    }
}
