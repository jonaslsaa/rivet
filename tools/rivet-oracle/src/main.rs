//! rivet-oracle — the M0 differential-test harness runner.
//!
//! Milestone M0 is "harness green against vanilla itself (sanity)": the Java
//! Paper server boots, generates a fixed-seed world, and we capture a golden
//! fixture slice (chunk NBT + world metadata) that later milestones diff
//! Rivet's output against.
//!
//! The binary has two modes:
//!
//!   1. **default** — verify a fixtures dir against its `manifest.json`
//!      SHA-256s (the committed golden baseline).
//!   2. **`verify`** — the one-command M0 sanity gate: boot a *fresh* Paper
//!      run in a clean scratch dir under `work/`, wait for `Done`, shut it
//!      down cleanly (SIGTERM), extract the deterministic chunk-NBT slice,
//!      and diff its SHA-256s against the committed baseline. Prints PASS
//!      ("green against vanilla itself") or FAIL (nonzero exit).
//!
//! Note on determinism (see scripts/extract_fixtures.py): raw region files
//! are NOT byte-stable across boots (framing/timestamps), but the decompressed
//! chunk NBT payloads ARE (verified 432/432 across boots, seed 42, superflat).
//! The `verify` gate therefore compares only the chunk-NBT layer; level.dat
//! and server.properties contain wall-clock timestamps and are expected to
//! differ across boots.
//!
//! Usage:
//!   cargo run -p rivet-oracle                    # verify tools/rivet-oracle/fixtures
//!   cargo run -p rivet-oracle -- <dir>           # verify <dir> against its manifest
//!   cargo run -p rivet-oracle -- verify          # full M0 gate: boot -> extract -> diff
//!   cargo run -p rivet-oracle -- verify [dir]    # gate against a custom baseline dir
//!   RIVET_ORACLE_JAR=/path/jar.jar cargo run -p rivet-oracle -- verify

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

/// Group captured chunks by dimension for the summary.
fn summarize(manifest: &Manifest) {
    let mut dims: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &manifest.captured {
        if let Some(dim) = c.dim.as_deref() {
            *dims.entry(dim).or_default() += 1;
        }
    }

    println!("M0 oracle fixture summary");
    println!("========================");
    if let Some(seed) = &manifest.seed {
        println!("seed:                  {seed}");
    }
    if let Some(lt) = &manifest.level_type {
        println!("level-type:            {lt}");
    }
    if let Some(paper) = &manifest.paper {
        println!("paper:                 {paper}");
    }
    println!("format:                {}", manifest.format);
    println!("captured files:        {}", manifest.captured.len());
    println!(
        "chunk-count (manifest): {}",
        manifest.chunk_count.unwrap_or(0)
    );
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
/// always fresh. The committed `fixtures/server.properties` is the exact M0
/// config (seed 42, superflat, offline, same port/view-distance), guaranteeing
/// config parity by construction.
fn prepare_run_dir(run_dir: &Path) -> Result<(), Error> {
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

    fs::copy(
        crate_dir().join("fixtures/server.properties"),
        run_dir.join("server.properties"),
    )?;
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
/// Rust deps, and its JSON manifest is the same shape we already read.
fn extract_fresh_fixtures(world_dir: &Path, out_dir: &Path) -> Result<(), Error> {
    let script = crate_dir().join("scripts/extract_fixtures.py");
    let out = Command::new("python3")
        .arg(&script)
        .arg(world_dir)
        .arg(out_dir)
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

/// The one-command M0 sanity gate: boot -> extract -> diff -> verdict.
fn run_verify_gate(baseline_dir: &Path) -> Result<(), Error> {
    let crate_root = crate_dir();
    let jar = ensure_jar()?;
    let run_dir = crate_root.join("work/verify/run");
    let log_path = crate_root.join("work/verify/boot.log");
    let tmp = env::temp_dir().join(format!("rivet-oracle-verify-{}", std::process::id()));

    println!("M0 sanity gate: green against vanilla itself");
    println!("   baseline fixtures : {}", baseline_dir.display());
    println!("   paperclip jar     : {}", jar.display());
    println!();

    println!(
        "[1/4] booting a fresh Paper run (scratch world in {})...",
        run_dir.display()
    );
    prepare_run_dir(&run_dir)?;
    boot_and_shutdown(&run_dir, &log_path, &jar)?;

    println!("[2/4] world saved cleanly; extracting deterministic chunk slice...");
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    extract_fresh_fixtures(&run_dir.join("world"), &tmp)?;

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
            "      (seed {} / {}) — green against vanilla itself.",
            baseline.seed.as_deref().unwrap_or("?"),
            baseline.level_type.as_deref().unwrap_or("?")
        );
        let _ = fs::remove_dir_all(&tmp);
        Ok(())
    } else {
        print_chunk_diff(&diff, &baseline);
        println!("fresh extraction (kept for inspection): {}", tmp.display());
        Err(Error::Diff(diff))
    }
}

fn print_usage() {
    println!("rivet-oracle — the M0 differential-test harness");
    println!();
    println!("USAGE:");
    println!(
        "  cargo run -p rivet-oracle                  verify committed fixtures/ against manifest.json"
    );
    println!("  cargo run -p rivet-oracle -- <dir>         verify <dir> against its manifest.json");
    println!(
        "  cargo run -p rivet-oracle -- verify        M0 sanity gate: boot fresh Paper -> extract -> diff"
    );
    println!(
        "                                             (optional 2nd arg: baseline fixtures dir)"
    );
    println!();
    println!("ENV:");
    println!("  RIVET_ORACLE_JAR   path to the paperclip jar");
    println!("                     (default: work/jars/, or copied from working/Paper/)");
}

fn run() -> Result<(), Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("verify") => {
            let baseline = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| crate_dir().join("fixtures"));
            run_verify_gate(&baseline)
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
            verify_fixtures_dir(&dir)
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
}
