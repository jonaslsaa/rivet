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
//!      payloads, the text component-JSON corpus, and the `spline/`/`seq/`
//!      value-leaf goldens). Each kind carries an independent manifest, so kinds
//!      can grow without a format migration.
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
//!   6. **`verify --full`** — the M2 FULL region gate (issue #51): same boot
//!      pipeline with the superflat config (`fixtures/server-full.properties`,
//!      `region-file-compression=none` per DECISIONS D13, corpus seed 0),
//!      diffed against `fixtures/regions/superflat-full` — the corpus-forced,
//!      twin-boot-captured status-FULL region payloads. The capture injects
//!      level-33 forced tickets for every corpus coordinate into each
//!      dimension, so all 8 corpus coordinates per dimension reach
//!      `minecraft:full`; LastUpdate is normalized to 0 (save-clock artifact).
//!      `--full --expect-fail` runs the control.
//!   7. **`sample`** — regenerate the `worldgen/` semantic fixtures: run the
//!      Paper-side sampler (`scripts/run_worldgen_sampler.sh`) into
//!      `samples.json`, re-extract the Starlight light samples from the M0
//!      FULL superflat chunks (`scripts/extract_light_samples.py`), and rewrite
//!      `manifest.json`. Requires the materialized Paper runtime (see the
//!      scripts; no full server boot).
//!   8. **`regenerate`** — full regeneration of all fixture kinds: M0 chunk
//!      slice (boot + extract), M2 region payloads (twin-boot: two independent
//!      fresh normal-overworld boots whose extracted payloads must be
//!      byte-identical before anything is committed), the FULL superflat region
//!      payloads (also twin-boot, issue #51), the worldgen semantic samples, and
//!      the text component-JSON corpus (Paper reference-oracle capture, issue
//!      #98). Sub-select with `--m0` / `--m2` / `--full` / `--samples` /
//!      `--text`. The `spline/` and `seq/` value-leaf goldens are regenerated
//!      by `scripts/run_spline_probe.sh` / `scripts/run_seq_probe.sh`
//!      (script-driven, no boot; not `regenerate` modes), so bare `regenerate`
//!      never refreshes them — re-run those scripts after a Paper re-pin.
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
//! Chunk-generation concurrency is pinned for byte-determinism (issue #266):
//! every oracle boot copies `fixtures/paper-global.yml` into the run dir's
//! `config/`, which sets `chunk-system` to exactly 1 worker / 1 I/O thread.
//! `boot_and_shutdown` refuses a boot whose log does not confirm that pin
//! (missing/ineffective config), the `verify` gates enforce provenance drift
//! between the baseline manifest and the run, and M2 region captures record
//! their concurrency as `chunk-concurrency` manifest provenance. Which
//! manifests are concurrency-sensitive region captures is decided by the
//! explicit `kind` field (`kind: "m2"`, stamped by `regenerate`), never
//! inferred from the level-type/compression strings — so a regenerated M0
//! manifest (`kind: "m0"`) can never be misclassified as a region capture that
//! requires the chunk-concurrency provenance (issue #266).
//!
//! Entity spawning is also suppressed for deterministic chunks (issue #266):
//! every boot copies `fixtures/paper-world-defaults.yml` into the run dir's
//! `config/`, which caps every `entities.spawning.spawn-limits.*` category at 0
//! so no mob spawns into the save window and into the captured chunk NBT. MC
//! 26.2 removed the vanilla `spawn-monsters`/`spawn-animals`/`spawn-npcs`
//! server.properties keys (DedicatedServerProperties reads none of them), so
//! this is the effective mechanism — the same one `rivet-capture` uses.
//!
//! Usage:
//!   cargo run -p rivet-oracle                          # verify all fixture kinds
//!   cargo run -p rivet-oracle -- <dir>                 # verify <dir> against its manifest
//!   cargo run -p rivet-oracle -- verify                # full M0 gate: boot -> extract -> pin-check -> diff
//!   cargo run -p rivet-oracle -- verify [dir]          # gate against a custom baseline dir
//!   cargo run -p rivet-oracle -- verify --m2 [dir]     # M2 region gate (normal-overworld none-compression)
//!   cargo run -p rivet-oracle -- verify --full [dir]   # M2 FULL region gate (superflat status-FULL capture, issue #51)
//!   cargo run -p rivet-oracle -- verify --expect-fail [dir]
//!                                # M0 negative control: boot -> extract -> diff against a
//!                                # deliberately corrupted copy of the baseline; exits 0 only
//!                                # when the pipeline detects AND names the tamper
//!   cargo run -p rivet-oracle -- verify --m2 --expect-fail [dir]
//!                                # M2 negative control against the region baseline
//!   cargo run -p rivet-oracle -- verify --full --expect-fail [dir]
//!                                # M2 FULL negative control against the superflat region baseline
//!   cargo run -p rivet-oracle -- sample                # regenerate worldgen/ semantic samples + manifest
//!   cargo run -p rivet-oracle -- regenerate            # regenerate all fixture kinds
//!                                                      # (sub-select: --m0/--m2/--full/--samples/--text)
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

mod chunk_level;
mod composed_noise;
mod corpus;
mod hash;
mod hash_manifest;
mod loaded_world;
mod mutate;
mod semantic_hash;

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_util::{DataInputStream, DataOutputStream};

use crate::mutate::TamperKind;

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
    /// The #54 hash stage could not complete honestly (missing/empty payload
    /// input, or a malformed capture tree) — maps to exit 3 UNVERIFIED, never a
    /// fabricated green.
    Unverified(String),
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
            Error::Unverified(m) => write!(f, "{m}"),
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
#[derive(Debug, Clone, serde::Deserialize)]
struct Captured {
    path: String,
    sha256: String,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    dim: Option<String>,
}

/// Recorded chunk-generation concurrency provenance in a manifest: the
/// effective Moonrise worker/I-O thread counts the capture ran under (issue
/// #266). A byte-identity oracle is only well-posed at 1 worker / 1 I-O thread,
/// so any recorded concurrency other than the pin is provenance drift.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
struct ChunkConcurrency {
    #[serde(rename = "worker-threads")]
    worker_threads: u32,
    #[serde(rename = "io-threads")]
    io_threads: u32,
}

impl ChunkConcurrency {
    /// The serialized-worldgen pin: exactly one worker and one I/O thread.
    const PINNED: ChunkConcurrency = ChunkConcurrency {
        worker_threads: 1,
        io_threads: 1,
    };

    fn is_pinned(self) -> bool {
        self == Self::PINNED
    }
}

/// Explicit capture-kind provenance written into regenerated manifests
/// (issue #266). The `kind` field is authoritative for the M0 vs M2
/// chunk-concurrency gate: `m0` declares the flat superflat slice (never
/// concurrency-sensitive, never requires chunk-concurrency provenance), `m2`
/// declares the normal-overworld none-compression region capture (always
/// concurrency-sensitive, MUST record the pinned 1/1 provenance), `full`
/// declares the superflat status-FULL region capture (issue #51 — also
/// concurrency-sensitive, MUST record the pinned 1/1 provenance). Regeneration
/// stamps these so classification never depends on inferring the capture from
/// the level-type/compression strings.
const KIND_M0: &str = "m0";
const KIND_M2: &str = "m2";
const KIND_FULL: &str = "full";

/// The fixture manifest (subset of fields; unknown fields are ignored).
#[derive(Debug, Clone, serde::Deserialize)]
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
    #[serde(rename = "chunk-concurrency", default)]
    chunk_concurrency: Option<ChunkConcurrency>,
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
/// `--chunks-only` extract); FULL is the superflat status-FULL region capture
/// under corpus seed 0 (`server-full.properties`, `--chunks-only` extract,
/// issue #51).
struct BootConfig {
    props_src: PathBuf,
    chunks_only: bool,
    /// Explicit capture-kind stamped into the regenerated manifest (issue #266):
    /// `KIND_M0` for the superflat slice, `KIND_M2` for the normal-overworld
    /// region capture, `KIND_FULL` for the superflat status-FULL capture.
    /// Kept authoritative so the chunk-concurrency provenance gate never
    /// depends on inferring the capture from config strings.
    kind: &'static str,
    title: &'static str,
    baseline: PathBuf,
}

fn m0_config() -> BootConfig {
    BootConfig {
        props_src: crate_dir().join("fixtures/server.properties"),
        chunks_only: false,
        kind: KIND_M0,
        title: "M0 sanity gate: green against vanilla itself (superflat, seed 42)",
        baseline: crate_dir().join("fixtures"),
    }
}

fn m2_config() -> BootConfig {
    BootConfig {
        props_src: crate_dir().join("fixtures/server-normal.properties"),
        chunks_only: true,
        kind: KIND_M2,
        title: "M2 region gate: normal-overworld none-compression region parity (seed 42)",
        baseline: crate_dir().join("fixtures/regions/overworld-normal"),
    }
}

fn full_config() -> BootConfig {
    BootConfig {
        props_src: crate_dir().join("fixtures/server-full.properties"),
        chunks_only: true,
        kind: KIND_FULL,
        title: "M2 FULL gate: superflat status-FULL region capture (corpus-forced, seed 0)",
        baseline: crate_dir().join("fixtures/regions/superflat-full"),
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
    captured: Vec<CapturedFile>,
}

/// A captured file entry in a fixture manifest: relative path + SHA-256 + byte
/// count, verified by `verify_fixtures`. Shared by the worldgen and text kinds.
#[derive(serde::Serialize)]
struct CapturedFile {
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

    // Chunk-concurrency provenance (issue #266): a normal-overworld region
    // capture MUST record the pinned 1 worker / 1 I-O thread it was generated
    // under, and any recorded concurrency anywhere must be that pin. A missing
    // provenance on such a capture, or any non-pinned value, is drift — the
    // fixtures were made under unknown concurrency and cannot be trusted.
    if is_region_capture(&manifest) {
        match manifest.chunk_concurrency {
            Some(cc) if cc.is_pinned() => {}
            Some(cc) => {
                return Err(Error::Manifest(format!(
                    "manifest records chunk-concurrency {}/{} — the oracle pin is exactly 1/1 \
                     (issue #266); this manifest drifted from the pinned provenance",
                    cc.worker_threads, cc.io_threads
                )));
            }
            None => {
                return Err(Error::Manifest(
                    "region-capture manifest records no chunk-concurrency provenance — it must \
                     declare the pinned 1/1 worker/I-O threads it was generated under (issue #266)"
                        .into(),
                ));
            }
        }
    } else if let Some(cc) = manifest.chunk_concurrency
        && !cc.is_pinned()
    {
        return Err(Error::Manifest(format!(
            "manifest records chunk-concurrency {}/{} — the oracle pin is exactly 1/1 \
             (issue #266); this manifest drifted from the pinned provenance",
            cc.worker_threads, cc.io_threads
        )));
    }
    Ok(manifest)
}

/// True for the concurrency-sensitive M2 region chunk capture — the only
/// capture whose byte identity depends on the pinned chunk-concurrency
/// provenance (issue #266).
///
/// Classification is **authoritative by explicit `kind`**: a regenerated M2
/// manifest carries `kind: "m2"` and a regenerated M0 carries `kind: "m0"`
/// (`regenerate` stamps both), so the decision never depends on inferring the
/// capture from the level-type/compression strings. A manifest with any other
/// (or missing) kind is *not* a region capture.
///
/// The inferential check is kept ONLY as a backward-compatible fallback for the
/// two already-committed manifests captured before the `kind` field existed
/// (M0 flat root and M2 normal-overworld region). It is a strict, named
/// fallback, never a silent skip: a kind-less none-compression
/// normal-overworld capture with chunks is still a region capture and still
/// requires the pinned provenance. The explicit kind wins whenever present, so
/// a future change to Paper's level-type string (or escaping) can never make a
/// kinded M2 manifest silently provenance-free.
fn is_region_capture(manifest: &Manifest) -> bool {
    if let Some(kind) = manifest.kind.as_deref() {
        return kind == KIND_M2 || kind == KIND_FULL;
    }
    manifest.level_type.as_deref() == Some("minecraft\\:normal")
        && manifest.region_file_compression.as_deref() == Some("none")
        && manifest.chunk_count.unwrap_or(0) > 0
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

/// Parse the Moonrise worker/I-O thread counts out of a boot log.
///
/// Paper logs exactly one line, from `MoonriseCommon.adjustWorkerThreads`:
/// `[MoonriseCommon] Paper is using N worker threads, M I/O threads`. Returns
/// `(worker, io)` when exactly one such line is present; `None` when the line
/// is absent or the log is ambiguous (two pin lines = something is wrong).
fn parse_boot_thread_counts(log_text: &str) -> Option<(u32, u32)> {
    let mut counts = None;
    for line in log_text.lines() {
        let Some((_, rest)) = line.split_once(" is using ") else {
            continue;
        };
        let Some((worker, io)) = rest.split_once(" worker threads, ") else {
            continue;
        };
        let Some(io) = io.split_once(" I/O threads") else {
            continue;
        };
        let Ok(worker) = worker.trim().parse::<u32>() else {
            continue;
        };
        let Ok(io) = io.0.trim().parse::<u32>() else {
            continue;
        };
        if counts.is_some() {
            return None; // more than one pin line — ambiguous, refuse.
        }
        counts = Some((worker, io));
    }
    counts
}

/// Enforce that the boot log confirms the pinned chunk concurrency: exactly
/// one `MoonriseCommon` line reporting 1 worker thread and 1 I/O thread.
///
/// This is the "fail loudly unless the boot logs confirm the pin" guarantee:
/// a missing config (Paper falls back to `-1` → `cores/2` workers) or a wrong
/// config (e.g. `worker-threads: 2`) is a hard error, never a skip or pass,
/// because without the pin the captured world is not byte-deterministic.
fn check_boot_thread_pin(log_text: &str) -> Result<(), Error> {
    match parse_boot_thread_counts(log_text) {
        Some((1, 1)) => Ok(()),
        Some((workers, io)) => Err(Error::Gate(format!(
            "chunk-concurrency pin NOT enforced: the boot log reports {workers} worker threads / {io} I/O \
             threads, but the oracle requires exactly 1 / 1 (issue #266). Check that \
             fixtures/paper-global.yml was copied into the run dir's config/ and sets \
             chunk-system.worker-threads=1 and chunk-system.io-threads=1."
        ))),
        None => Err(Error::Gate(
            "chunk-concurrency pin NOT confirmed: the boot log has no Moonrise worker/I-O thread line \
             ('Paper is using N worker threads, M I/O threads'). The pinned config \
             (fixtures/paper-global.yml) is missing or ineffective — refuse to treat this run as \
             byte-deterministic (issue #266).".into(),
        )),
    }
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

/// Verify the chunk-concurrency provenance of the baseline against the boot
/// that actually ran (issue #266).
///
/// The baseline manifest records the pinned concurrency its fixtures were
/// captured under; the fresh boot's log reports what it actually ran with.
/// They must agree — a baseline captured under unknown or wrong concurrency
/// (or a fresh boot that drifted off the pin) is not a byte-determinism
/// comparison. The strict requirement for region captures lives in
/// `verify_fixtures`; this is the per-run side of the drift check.
fn check_concurrency_provenance(baseline_dir: &Path, log_path: &Path) -> Result<(), Error> {
    let manifest = load_manifest(baseline_dir)?;
    let recorded = manifest.chunk_concurrency;
    let log_text = fs::read(log_path)
        .map_err(|e| Error::Gate(format!("cannot read boot log {}: {e}", log_path.display())))?;
    let log_text = String::from_utf8_lossy(&log_text);
    let observed = parse_boot_thread_counts(&log_text);

    let recorded = match recorded {
        Some(cc) if cc.is_pinned() => cc,
        Some(cc) => {
            return Err(Error::Manifest(format!(
                "baseline records chunk-concurrency {}/{} — the oracle pin is exactly 1/1 \
                 (issue #266); regenerate the fixtures under the pin",
                cc.worker_threads, cc.io_threads
            )));
        }
        None if is_region_capture(&manifest) => {
            return Err(Error::Manifest(
                "baseline is a region capture but records no chunk-concurrency provenance \
                 (issue #266); regenerate it under the pin"
                    .into(),
            ));
        }
        None => ChunkConcurrency::PINNED,
    };

    let observed = match observed {
        Some((w, i)) => ChunkConcurrency {
            worker_threads: w,
            io_threads: i,
        },
        None => {
            return Err(Error::Gate(
                "cannot confirm the concurrency the fresh boot ran with — no Moonrise \
                 worker/I-O thread line in the boot log (issue #266)"
                    .into(),
            ));
        }
    };

    if recorded != observed {
        return Err(Error::Gate(format!(
            "chunk-concurrency provenance drift: baseline recorded {}/{} but this boot ran \
             {}/{} (issue #266)",
            recorded.worker_threads,
            recorded.io_threads,
            observed.worker_threads,
            observed.io_threads
        )));
    }
    println!(
        "   chunk concurrency: {} / {} (baseline provenance) — enforced (boot log)",
        observed.worker_threads, observed.io_threads
    );
    Ok(())
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
    // The composed-noise golden comparison: beyond the manifest hashes, assert
    // the NOISE-checkpoint goldens (provenance, FULL_CHUNK_STEP reachability,
    // non-vacuous #175 matrix) and print the status/provenance scoreboard. The
    // committed seed-42 golden is a load-bearing deliverable — if the fixture
    // tree is absent this is UNVERIFIED (exit 3), never a silent green (D8:
    // never weaken/delete fixtures to go green).
    verify_composed_noise_step(&crate_dir().join("fixtures/composed-noise"))?;
    Ok(())
}

/// Verify the committed composed-noise golden; the absent-golden exit-3
/// contract lives in `composed_noise::require_fixture_tree`.
fn verify_composed_noise_step(dir: &Path) -> Result<(), Error> {
    composed_noise::require_fixture_tree(dir)?;
    composed_noise::verify_composed_noise(dir)?;
    println!(
        "PASS: composed-noise seed-42 golden verified (pinned Paper 0a99345 provenance, reachability, value↔bits round-trip)"
    );
    composed_noise::print_scoreboard();
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

    // Pin chunk-gen concurrency for deterministic oracle boots (issue #266):
    // every boot runs under the committed fixtures/paper-global.yml, which sets
    // chunk-system to exactly 1 worker / 1 I/O thread. Paper rewrites this file
    // into the run dir with its full defaults on first boot; the committed
    // source is re-copied every prepare so a stale run-dir config never
    // survives to the next boot.
    let config_dir = run_dir.join("config");
    fs::create_dir_all(&config_dir)?;
    let global_src = crate_dir().join("fixtures/paper-global.yml");
    if !global_src.is_file() {
        return Err(Error::Gate(format!(
            "pinned Paper global config {} missing — every oracle boot must run under \
             chunk-system io-threads=1 / worker-threads=1 (issue #266)",
            global_src.display()
        )));
    }
    fs::copy(&global_src, config_dir.join("paper-global.yml"))?;

    // Pin entity spawning off for deterministic chunk payloads (issue #266): a
    // mob that spawns into the save window would serialize into the captured
    // chunks' 'Entities' tag, adding nondeterminism no normalization can remove.
    // MC 26.2 removed the vanilla spawn-animals/monsters/npcs server.properties
    // keys (DedicatedServerProperties reads none of them), so the effective
    // switch is Paper's world-defaults spawn-limits — every category capped at
    // 0, the same mechanism rivet-capture uses. Copied before every boot so a
    // stale run-dir config (Paper rewrites it with its full defaults) never
    // survives to the next run.
    let defaults_src = crate_dir().join("fixtures/paper-world-defaults.yml");
    if !defaults_src.is_file() {
        return Err(Error::Gate(format!(
            "pinned Paper world-defaults config {} missing — every oracle boot must run under \
             spawn-limits 0 so no entity spawns into the capture window (issue #266)",
            defaults_src.display()
        )));
    }
    fs::copy(&defaults_src, config_dir.join("paper-world-defaults.yml"))?;
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
/// silently diffed. Finally, the whole boot log is checked to confirm the
/// pinned chunk concurrency (issue #266): every oracle boot must run under
/// exactly 1 worker / 1 I/O thread, or the captured world is not
/// byte-deterministic and the run is refused.
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

    // Confirm the pinned chunk concurrency in the FULL boot log (the pin line
    // is printed during startup, before `Done`).
    check_boot_thread_pin(&String::from_utf8_lossy(&bytes))?;
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
///
/// `kind` is the explicit capture-kind provenance stamped into the manifest
/// (`KIND_M0` for the flat superflat slice, `KIND_M2` for the normal-overworld
/// region capture, issue #266). `observed` is the chunk concurrency the boot
/// actually ran with (parsed from the boot log): for M2 region captures it is
/// recorded as `chunk-concurrency` provenance; M0 never records it.
fn extract_fresh_fixtures(
    world_dir: &Path,
    out_dir: &Path,
    chunks_only: bool,
    kind: &str,
    observed: Option<ChunkConcurrency>,
) -> Result<(), Error> {
    let script = crate_dir().join("scripts/extract_fixtures.py");
    let mut cmd = Command::new("python3");
    cmd.arg(&script).arg(world_dir).arg(out_dir);
    if chunks_only {
        cmd.arg("--chunks-only");
    }
    // The FULL capture spans the four regions around the origin (issue #51) so
    // every corpus coordinate — positive, negative, and the x/z=31 seams —
    // lands in a captured region file. M0/M2 stay on the single spawn region.
    // The corpus coordinates are forced to `minecraft:full` by the two-boot
    // ticket-injection capture (`full_forced_extraction`), never left to a
    // spawn boot.
    if kind == KIND_FULL {
        cmd.arg("--all-regions");
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

    let observed = if chunks_only {
        Some(observed.ok_or_else(|| {
            Error::Gate(
                "M2/FULL region extraction needs the boot's observed chunk concurrency to \
                 record provenance (issue #266)"
                    .into(),
            )
        })?)
    } else {
        None
    };
    inject_manifest_metadata(out_dir, kind, observed)
}

/// Record capture-kind + (for M2) `chunk-concurrency` provenance into a
/// manifest (issue #266).
///
/// `extract_fixtures.py` writes the manifest (alphabetically sorted keys,
/// `indent=2`, trailing newline); we add the provenance fields with the same
/// formatting so the file stays git-clean under identical input.
fn inject_manifest_metadata(
    dir: &Path,
    kind: &str,
    observed: Option<ChunkConcurrency>,
) -> Result<(), Error> {
    let manifest_path = dir.join("manifest.json");
    let mut root: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).map_err(|e| {
            Error::Gate(format!(
                "cannot read {} to record provenance: {e}",
                manifest_path.display()
            ))
        })?)
        .map_err(|e| {
            Error::Gate(format!(
                "manifest {} unparsable: {e}",
                manifest_path.display()
            ))
        })?;
    root["kind"] = serde_json::Value::String(kind.to_string());
    if let Some(observed) = observed {
        root["chunk-concurrency"] = serde_json::json!({
            "worker-threads": observed.worker_threads,
            "io-threads": observed.io_threads,
        });
    }
    let mut text = serde_json::to_string_pretty(&root)
        .map_err(|e| Error::Gate(format!("cannot serialize manifest: {e}")))?;
    text.push('\n');
    fs::write(&manifest_path, text)?;
    Ok(())
}

/// The Moonrise FULL-chunk ticket level (`FULL_CHUNK_LEVEL`): a forced chunk at
/// this level is generated through `minecraft:full` WITHOUT entity ticking, so
/// its serialized payload is content-deterministic. Level 31 (`ENTITY_TICKING`)
/// would tick the chunk and serialize nondeterministic entity state into the
/// payloads — the forced FULL capture must stay at 33.
const FORCED_TICKET_LEVEL: i32 = 33;
/// `ticks_left = Long.MIN_VALUE` — a forced ticket never counts down, so it
/// holds the chunk persistent for the whole boot.
const FORCED_TICKET_TICKS_LEFT: i64 = i64::MIN;
/// The world data version written into `chunk_tickets.dat`, pinned alongside
/// the Paper pin: `SharedConstants.getCurrentVersion()` reports
/// `new DataVersion(4903, "main")` for MC 26.2 (`DetectedVersion`). A SavedData
/// written with a data version the pinned Paper has since moved past would be
/// run through the `SAVED_DATA_FORCED_CHUNKS` datafix on load — or misparsed —
/// so a drift that stops the forced chunks from loading is caught loudly by
/// `verify_forced_load`, never silently.
const FORCED_TICKET_DATA_VERSION: i32 = 4903;

/// The three dimensions + their Moonrise SavedData subpaths. Every dimension
/// (incl. the overworld) stores its `chunk_tickets.dat` SavedData under
/// `dimensions/minecraft/<dim>/data/minecraft/` (not the legacy per-world root).
const TICKET_DIMS: &[(&str, &str)] = &[
    ("overworld", "dimensions/minecraft/overworld"),
    ("the_nether", "dimensions/minecraft/the_nether"),
    ("the_end", "dimensions/minecraft/the_end"),
];

/// Write a level-33 `minecraft:forced` ticket for every corpus coordinate into
/// each dimension's `chunk_tickets.dat` (gzip NBT), mirroring the Moonrise
/// `TicketStorage`/`Ticket` codec:
///
///   `{DataVersion:int, data:{tickets:[{type:"minecraft:forced",
///     chunk_pos:int_array[x,z], level:int, ticks_left:long}]}}`
///
/// Written between boot1 (world create) and boot2 (capture) so boot2 loads
/// "8 persistent chunks" per dimension and finishes every corpus coordinate to
/// `minecraft:full` — the corpus-forced generation that a spawn boot never
/// reaches (issue #51).
fn inject_forced_tickets(world_dir: &Path) -> Result<(), Error> {
    let mut tickets = Vec::new();
    for (cx, cz) in crate::corpus::COORDINATES {
        let mut ticket = CompoundTag::new();
        ticket.put_string("type", "minecraft:forced");
        ticket.put_int_array("chunk_pos", vec![*cx, *cz]);
        ticket.put_int("level", FORCED_TICKET_LEVEL);
        ticket.put_long("ticks_left", FORCED_TICKET_TICKS_LEFT);
        tickets.push(Tag::Compound(ticket));
    }
    let mut data = CompoundTag::new();
    data.put(
        "tickets".to_string(),
        Tag::List(ListTag::with_list(tickets)),
    );
    let mut root = CompoundTag::new();
    root.put_int("DataVersion", FORCED_TICKET_DATA_VERSION);
    root.put("data".to_string(), Tag::Compound(data));

    for (_, sub) in TICKET_DIMS {
        let dir = world_dir.join(sub).join("data/minecraft");
        fs::create_dir_all(&dir)?;
        let path = dir.join("chunk_tickets.dat");
        let mut out = fs::File::create(&path)?;
        nbt_io::write_compressed(&root, &mut out).map_err(|e| {
            Error::Gate(format!(
                "cannot write forced tickets {}: {e}",
                path.display()
            ))
        })?;
    }
    println!(
        "      injected level-{FORCED_TICKET_LEVEL} forced tickets for {} corpus coordinates x {} dimensions",
        crate::corpus::COORDINATES.len(),
        TICKET_DIMS.len()
    );
    Ok(())
}

/// Rewrite a serialized chunk payload so its root `LastUpdate` (the save-clock
/// game-time long) is 0, matching the committed M0 FULL convention. The
/// save-clock long varies by a few ticks across forced-capture boots (the save
/// lands on a nondeterministic tick), so the twin-boot byte-identity proof
/// normalizes it before comparing: `LastUpdate` is a save-clock artifact, not
/// worldgen content. Returns `None` when the payload has no `LastUpdate` (a
/// partial chunk that never records one — left untouched by the tree pass).
///
/// Note the gate this feeds is a *determinism* proof between two Paper captures
/// (both re-encoded through the rivet-nbt serializer), not a byte-parity check
/// of rivet-nbt's output against Paper's own serializer — that byte-for-byte
/// parity is rivet-parity's job. The re-encode is identical on both sides, so a
/// serializer divergence could not produce a false green here; it would only
/// make the gate's bytes differ from a hypothetical Paper-native extraction.
fn normalize_last_update(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut compound =
        nbt_io::read_unlimited(&mut DataInputStream::new(Cursor::new(bytes))).ok()?;
    if !compound.tags.contains_key("LastUpdate") {
        return None;
    }
    compound.put_long("LastUpdate", 0);
    let mut out = Vec::new();
    nbt_io::write(&compound, &mut DataOutputStream::new(Cursor::new(&mut out))).ok()?;
    Some(out)
}

/// Normalize every chunk payload in an extraction tree (`normalize_last_update`
/// over the `*.nbt` payloads). Applied to both twin-boot extractions before the
/// byte-identity check and to the committed baseline, so the FULL gate compares
/// worldgen content, not the save-clock artifact. Returns how many payloads
/// changed.
fn normalize_last_update_tree(dir: &Path) -> Result<usize, Error> {
    let mut count = 0usize;
    let mut walk = vec![dir.to_path_buf()];
    while let Some(d) = walk.pop() {
        for entry in fs::read_dir(&d)? {
            let p = entry?.path();
            if p.is_dir() {
                walk.push(p);
            } else if p.extension().map(|x| x == "nbt").unwrap_or(false) {
                let bytes = fs::read(&p)?;
                if let Some(normalized) = normalize_last_update(&bytes)
                    && normalized != bytes
                {
                    fs::write(&p, normalized)?;
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

/// Recompute the `sha256`/`bytes` of every `captured[]` entry from the
/// on-disk payloads. `extract_fixtures.py` hashes the raw payloads, so after
/// `normalize_last_update_tree` rewrites them the manifest would otherwise
/// describe bytes that no longer exist — breaking `verify_fixtures`,
/// `diff_chunk_hashes`, and the tamper control's internal-consistency check.
/// The manifest is rewritten with the same `serde_json::to_string_pretty` +
/// trailing-newline formatting as `inject_manifest_metadata`, so regeneration
/// stays byte-stable.
fn rehash_captured(dir: &Path) -> Result<(), Error> {
    let manifest_path = dir.join("manifest.json");
    let mut root: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).map_err(|e| {
            Error::Gate(format!(
                "cannot read {} to rehash captured payloads: {e}",
                manifest_path.display()
            ))
        })?)
        .map_err(|e| {
            Error::Gate(format!(
                "manifest {} unparsable: {e}",
                manifest_path.display()
            ))
        })?;
    let captured = root
        .get_mut("captured")
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| Error::Gate("manifest has no `captured` list to rehash".into()))?;
    for cap in captured.iter_mut() {
        let Some(path) = cap.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        let bytes = fs::read(dir.join(path))
            .map_err(|e| Error::Gate(format!("cannot read {} to rehash: {e}", path)))?;
        cap["sha256"] = serde_json::Value::String(sha256_hex(&bytes));
        cap["bytes"] = serde_json::Value::Number((bytes.len() as u64).into());
    }
    let mut text = serde_json::to_string_pretty(&root)
        .map_err(|e| Error::Gate(format!("cannot serialize manifest: {e}")))?;
    text.push('\n');
    fs::write(&manifest_path, text)?;
    Ok(())
}

/// Require the capture boot to have loaded the forced persistent chunks in
/// every dimension — the proof that corpus-forced generation actually ran. A
/// boot that loaded 0 (the ticket injection silently failed) is refused instead
/// of re-committing a plain spawn boot.
///
/// Paper logs one line per dimension:
/// `Loading N persistent chunks for level 'minecraft:<dim>'...`. Instead of
/// matching one exact message (which would go silently stale if Paper ever
/// reported a different count or wording), this parses the per-dimension count
/// out of each `Loading N persistent chunks` line and requires every ticket
/// dimension to have loaded at least `COORDINATES.len()` chunks. The check is
/// dimension-aware, so a partially-succeeded injection (some dimensions loaded,
/// others 0) is refused too.
fn verify_forced_load(log_path: &Path) -> Result<(), Error> {
    let log_text = fs::read(log_path)?;
    let log_text = String::from_utf8_lossy(&log_text);
    let expected = crate::corpus::COORDINATES.len();
    let mut per_dim: Vec<(String, usize)> = Vec::new();
    for line in log_text.lines() {
        let marker = "persistent chunks for level 'minecraft:";
        let Some(marker_at) = line.find(marker) else {
            continue;
        };
        // Count: the "Loading N" immediately before the marker.
        let count = line[..marker_at]
            .rsplit_once("Loading ")
            .and_then(|(_, n)| n.trim().parse::<usize>().ok());
        // Dim: the "<dim>" between the marker and the closing quote.
        let dim = line[marker_at + marker.len()..]
            .split('\'')
            .next()
            .unwrap_or("")
            .to_string();
        if let Some(count) = count
            && !dim.is_empty()
        {
            per_dim.push((dim, count));
        }
    }
    let mut short: Vec<String> = Vec::new();
    for (dim, _) in TICKET_DIMS {
        let loaded = per_dim
            .iter()
            .find(|(d, _)| d == dim)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if loaded < expected {
            short.push(format!("{dim} loaded {loaded}"));
        }
    }
    if !short.is_empty() {
        return Err(Error::Gate(format!(
            "forced-capture boot did not load the {expected} forced persistent chunks in every \
             dimension ({}) — the ticket injection silently failed; refusing to commit a spawn \
             boot (log {})",
            short.join("; "),
            log_path.display()
        )));
    }
    Ok(())
}

/// The corpus-forced FULL capture (issue #51): a TWO-boot sequence that forces
/// every corpus coordinate to `minecraft:full` in all three dimensions.
///
///   boot1 (create): `prepare_run_dir` + a clean Done/SIGTERM boot creates the
///     world (booting over a missing world crashes with "Overworld settings
///     missing" before any chunk can load).
///   inject: level-33 `minecraft:forced` tickets for all 8 corpus coordinates
///     are written into each dimension's `chunk_tickets.dat`.
///   boot2 (capture): a second Done/SIGTERM boot loads the 8 persistent chunks
///     per dimension, finishes each to `minecraft:full`, and saves them.
///
/// The extraction is then normalized (root `LastUpdate` zeroed) so the twin
/// boots are byte-identical, and the capture log is verified to have loaded the
/// forced chunks (see `verify_forced_load`). The returned log path is the
/// capture boot's — the concurrency-pin provenance the FULL baseline must agree
/// with.
fn full_forced_extraction(
    run_dir: &Path,
    jar: &Path,
    cfg: &BootConfig,
    tag: &str,
) -> Result<(PathBuf, PathBuf), Error> {
    debug_assert_eq!(cfg.kind, KIND_FULL);
    let create_log = run_dir.with_file_name(format!("boot-{tag}-create.log"));
    let capture_log = run_dir.with_file_name(format!("boot-{tag}.log"));
    prepare_run_dir(run_dir, &cfg.props_src)?;
    println!("      [boot1] creating the superflat world...");
    boot_and_shutdown(run_dir, &create_log, jar)?;
    inject_forced_tickets(&run_dir.join("world"))?;
    println!("      [boot2] capturing the forced FULL chunks...");
    boot_and_shutdown(run_dir, &capture_log, jar)?;
    verify_forced_load(&capture_log)?;

    let tmp = env::temp_dir().join(format!("rivet-oracle-verify-{}-{tag}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    let observed = {
        let log_text = fs::read(&capture_log)?;
        parse_boot_thread_counts(&String::from_utf8_lossy(&log_text)).map(|(w, i)| {
            ChunkConcurrency {
                worker_threads: w,
                io_threads: i,
            }
        })
    };
    extract_fresh_fixtures(&run_dir.join("world"), &tmp, true, KIND_FULL, observed)?;
    let normalized = normalize_last_update_tree(&tmp)?;
    // The extracted manifest hashed the pre-normalization payloads; rehash it so
    // the manifest describes the bytes actually on disk (verify_fixtures and the
    // tamper control both rely on captured[] matching the tree).
    rehash_captured(&tmp)?;
    println!("      normalized LastUpdate to 0 in {normalized} chunk payloads");
    Ok((tmp, capture_log))
}

/// Boot a fresh Paper run in `run_dir` and extract its deterministic chunk-NBT
/// slice into a temp dir. Returns the temp extraction dir (caller owns
/// cleanup). Shared by the `verify` gates and negative controls. `tag`
/// distinguishes concurrent/sequential extractions from the same process (the
/// M2 twin-boot regeneration extracts two independent boots and needs both kept
/// simultaneously). The boot always runs under the pinned `fixtures/paper-global.yml`
/// and `boot_and_shutdown` refuses a boot that does not log the 1/1 pin (issue #266).
///
/// The boot log is kept at `run_dir/<parent>/boot-<tag>.log` and its path is
/// returned with the extraction dir so callers can run the provenance drift
/// check against the exact log that produced the extraction.
fn fresh_extraction(
    run_dir: &Path,
    jar: &Path,
    cfg: &BootConfig,
    tag: &str,
) -> Result<(PathBuf, PathBuf), Error> {
    if cfg.kind == KIND_FULL {
        // The FULL gate needs corpus-forced generation (two boots + ticket
        // injection + LastUpdate normalization), not a plain spawn boot.
        return full_forced_extraction(run_dir, jar, cfg, tag);
    }
    let log_path = run_dir.with_file_name(format!("boot-{tag}.log"));
    prepare_run_dir(run_dir, &cfg.props_src)?;
    boot_and_shutdown(run_dir, &log_path, jar)?;
    let tmp = env::temp_dir().join(format!("rivet-oracle-verify-{}-{tag}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    // M2 region extractions record the observed concurrency as provenance.
    let observed = if cfg.chunks_only {
        let log_text = fs::read(&log_path)?;
        parse_boot_thread_counts(&String::from_utf8_lossy(&log_text)).map(|(w, i)| {
            ChunkConcurrency {
                worker_threads: w,
                io_threads: i,
            }
        })
    } else {
        None
    };
    extract_fresh_fixtures(
        &run_dir.join("world"),
        &tmp,
        cfg.chunks_only,
        cfg.kind,
        observed,
    )?;
    Ok((tmp, log_path))
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
    let (tmp, boot_log) = fresh_extraction(&run_dir, &jar, cfg, "negcontrol")?;
    println!("[2/4] world saved cleanly; extracted deterministic chunk slice.");

    // The control is meaningless against a stale/unverifiable Paper (the pin
    // check would already fail `verify`, so a nonzero here proves nothing).
    // Checked after the boot so the pin is read from the jar that actually ran.
    check_pin(baseline_dir, &run_dir)?;
    check_concurrency_provenance(baseline_dir, &boot_log)?;

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
    println!(
        "baseline. Do not fudge fixtures — investigate (see work/verify/boot-gate.log and the"
    );
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
    let (tmp, boot_log) = fresh_extraction(&run_dir, &jar, cfg, "gate")?;
    println!("[2/4] world saved cleanly; extracted deterministic chunk slice.");

    check_pin(baseline_dir, &run_dir)?;
    check_concurrency_provenance(baseline_dir, &boot_log)?;

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
        captured.push(CapturedFile {
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

/// `fixtures/text/` manifest, serialized in the exact committed field order so
/// regeneration is byte-identical (git-clean). The text corpus (issue #98)
/// hashes the committed `corpus.json` (input component JSON, exact wire bytes)
/// and `golden.json` (Paper's accept/reject verdict + canonical decode→re-encode
/// JSON under non-compressed `JsonOps`). The `paper` provenance is read back out
/// of `golden.json` — the file the extractor actually wrote against the live
/// oracle's ping — so the manifest always describes the Paper the golden was
/// captured against, never a stale hand-maintained field.
#[derive(serde::Serialize)]
struct TextManifest<'a> {
    format: u64,
    paper: &'a str,
    kind: &'a str,
    note: &'a str,
    captured: Vec<CapturedFile>,
}

/// Rewrite `fixtures/text/manifest.json` from the committed corpus + golden.
///
/// The `paper` pin comes from `golden.json`'s provenance field, which
/// `extract_text_fixtures.py` records from the live oracle's `ping` — the
/// pinned Paper revision the golden was actually captured against. Mirroring
/// how the worldgen manifest reads its seed + pin back out of the generated
/// samples so it always describes what was actually produced.
fn regenerate_text_manifest(text_dir: &Path) -> Result<(), Error> {
    let golden_path = text_dir.join("golden.json");
    let golden: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&golden_path)
            .map_err(|e| Error::Manifest(format!("{} missing: {e}", golden_path.display())))?,
    )
    .map_err(|e| Error::Manifest(format!("{} unparsable: {e}", golden_path.display())))?;
    let paper = golden
        .get("paper")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut captured = Vec::new();
    for name in ["corpus.json", "golden.json"] {
        let data = fs::read(text_dir.join(name))
            .map_err(|e| Error::Manifest(format!("{name} missing: {e}")))?;
        captured.push(CapturedFile {
            path: name.to_string(),
            sha256: sha256_hex(&data),
            bytes: data.len(),
        });
    }

    let manifest = TextManifest {
        format: 1,
        paper: &paper,
        kind: "text",
        note: "Paper-grounded component JSON corpus + golden (issue #98): `corpus.json` is the \
               committed input corpus (each `input` is the exact wire JSON a \
               chat/title/player-info/scoreboard packet carries, stored as a JSON string so the \
               bytes fed to Paper equal the bytes the Rust side parses); `golden.json` records \
               Paper's accept/reject verdict and the canonical decode->re-encode JSON under \
               non-compressed JsonOps, stored verbatim as a JSON string so no serialization \
               layer re-normalizes it. Regenerate with `regenerate --text` (needs the \
               materialized Paper runtime + reference oracle launcher).",
        captured,
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialize text manifest: {e}")))?;
    text.push('\n');
    fs::write(text_dir.join("manifest.json"), text)?;
    println!("rewrote {}", text_dir.join("manifest.json").display());
    Ok(())
}

/// Regenerate the `fixtures/text/` fixtures: drive the Paper reference oracle
/// over `corpus.json` (`scripts/extract_text_fixtures.py` writes `golden.json`),
/// then rewrite the manifest. No full server boot — only the materialized Paper
/// runtime + the reference oracle launcher are needed.
fn regenerate_text() -> Result<(), Error> {
    let text_dir = crate_dir().join("fixtures/text");
    fs::create_dir_all(&text_dir)?;

    let extractor = crate_dir().join("scripts/extract_text_fixtures.py");
    let corpus = text_dir.join("corpus.json");
    let golden = text_dir.join("golden.json");
    let status = Command::new("python3")
        .arg(&extractor)
        .arg(&corpus)
        .arg(&golden)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| {
            Error::Gate(format!(
                "failed to run python3 {}: {e}",
                extractor.display()
            ))
        })?;
    if !status.success() {
        return Err(Error::Gate(format!(
            "extract_text_fixtures.py exited {status} — see its stderr (Paper oracle runtime \
             missing? set RIVET_PAPER_JAR / RIVET_PAPER_LIBRARIES / RIVET_PAPER_RUNTIME_JAR)"
        )));
    }

    regenerate_text_manifest(&text_dir)?;
    println!("regenerated text fixture corpus (corpus.json, golden.json, manifest.json)");
    Ok(())
}

/// Regenerate the M0 golden chunk slice: boot a fresh superflat run and extract
/// the deterministic chunk-NBT payloads (+ level.dat / server.properties /
/// manifest.json) straight into `fixtures/`. The gate's hash verification is
/// the safety net against a bad regeneration. The boot runs under the pinned
/// `fixtures/paper-global.yml` (issue #266); `boot_and_shutdown` refuses a boot
/// that does not log the 1/1 worker/I-O thread pin.
fn regenerate_m0(dest: &Path) -> Result<(), Error> {
    let jar = ensure_jar()?;
    let run_dir = crate_dir().join("work/verify/run");
    let props = crate_dir().join("fixtures/server.properties");
    let log_path = run_dir.with_file_name("boot-m0.log");
    prepare_run_dir(&run_dir, &props)?;
    boot_and_shutdown(&run_dir, &log_path, &jar)?;
    extract_fresh_fixtures(&run_dir.join("world"), dest, false, KIND_M0, None)?;
    println!("regenerated M0 golden chunk slice under {}", dest.display());
    Ok(())
}

/// Byte-for-byte compare two extraction trees (a twin-boot pair).
///
/// Compares every file present in either tree (relative path -> bytes). The
/// M2 chunks-only extracts contain only the deterministic `.nbt` payloads and
/// the manifest, so this is a strict byte-identity check — any file difference
/// between the two independent boots means the generation is nondeterministic
/// and the fixtures must NOT be committed.
fn trees_byte_identical(a: &Path, b: &Path) -> Result<bool, Error> {
    let mut a_files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in fs::read_dir(a)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let mut walk = vec![path];
            while let Some(dir) = walk.pop() {
                for e in fs::read_dir(&dir)? {
                    let e = e?;
                    if e.path().is_dir() {
                        walk.push(e.path());
                    } else {
                        let rel = e
                            .path()
                            .strip_prefix(a)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned();
                        a_files.insert(rel, e.path());
                    }
                }
            }
        } else {
            let rel = path.strip_prefix(a).unwrap().to_string_lossy().into_owned();
            a_files.insert(rel, path);
        }
    }

    let mut b_files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in fs::read_dir(b)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let mut walk = vec![path];
            while let Some(dir) = walk.pop() {
                for e in fs::read_dir(&dir)? {
                    let e = e?;
                    if e.path().is_dir() {
                        walk.push(e.path());
                    } else {
                        let rel = e
                            .path()
                            .strip_prefix(b)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned();
                        b_files.insert(rel, e.path());
                    }
                }
            }
        } else {
            let rel = path.strip_prefix(b).unwrap().to_string_lossy().into_owned();
            b_files.insert(rel, path);
        }
    }

    if a_files.len() != b_files.len() {
        return Ok(false);
    }
    for (rel, pa) in &a_files {
        let Some(pb) = b_files.get(rel) else {
            return Ok(false);
        };
        if fs::read(pa)? != fs::read(pb)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Regenerate the M2 normal-overworld none-compression region payloads with a
/// twin-boot determinism proof (issue #266).
///
/// Performs TWO independent fresh Paper boots under the pinned concurrency and
/// requires their extracted chunk-NBT payloads (and manifests) to be
/// byte-identical before committing anything into `dest`. If the pair diverges,
/// `dest` is left untouched and the two extraction trees are kept for
/// inspection — a nondeterministic pair is never committed.
fn regenerate_m2(dest: &Path) -> Result<(), Error> {
    let jar = ensure_jar()?;
    let run_dir = crate_dir().join("work/verify/run");
    let cfg = m2_config();

    println!(
        "[1/3] twin-boot 1: fresh normal-overworld Paper boot under the 1/1 concurrency pin..."
    );
    let (boot_a, _) = fresh_extraction(&run_dir, &jar, &cfg, "m2a")?;
    println!(
        "[2/3] twin-boot 2: fresh normal-overworld Paper boot under the 1/1 concurrency pin..."
    );
    let (boot_b, _) = fresh_extraction(&run_dir, &jar, &cfg, "m2b")?;

    println!("[3/3] byte-comparing the two independent extractions...");
    if !trees_byte_identical(&boot_a, &boot_b)? {
        eprintln!(
            "regeneration ABORTED: the two independent Paper boots produced DIFFERENT \
             chunk payloads — the M2 generation is not byte-deterministic, so nothing is \
             committed (issue #266).\n\
             boot A kept at: {}\n\
             boot B kept at: {}\n\
             destination {} was NOT touched.",
            boot_a.display(),
            boot_b.display(),
            dest.display()
        );
        return Err(Error::Gate(
            "M2 twin-boot byte-identity check failed — refusing to commit a nondeterministic pair"
                .into(),
        ));
    }

    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    copy_dir_recursive(&boot_a, dest)?;
    let _ = fs::remove_dir_all(&boot_a);
    let _ = fs::remove_dir_all(&boot_b);
    println!(
        "regenerated M2 normal-overworld region payloads under {} (twin-boot byte-identical; \
         chunk-concurrency provenance recorded)",
        dest.display()
    );
    Ok(())
}

/// Regenerate the superflat status-FULL region payloads with a corpus-forced
/// twin-boot determinism proof (issue #51), mirroring `regenerate_m2`.
///
/// Performs TWO independent corpus-forced captures (`full_forced_extraction`:
/// create -> inject level-33 forced tickets for every corpus coordinate ->
/// capture, under the pinned 1/1 concurrency), normalizes each extraction's
/// save-clock `LastUpdate` to 0, and requires the two extracted chunk-NBT
/// payloads (and manifests) to be byte-identical before committing anything
/// into `dest`. If the pair diverges, `dest` is left untouched and the two
/// extraction trees are kept for inspection — a nondeterministic pair is never
/// committed.
fn regenerate_full(dest: &Path) -> Result<(), Error> {
    let jar = ensure_jar()?;
    let run_dir = crate_dir().join("work/verify/run");
    let cfg = full_config();

    println!(
        "[1/3] corpus-forced capture 1: superflat FULL (create + forced tickets + capture, 1/1 pin)..."
    );
    let (boot_a, _) = fresh_extraction(&run_dir, &jar, &cfg, "fulla")?;
    println!(
        "[2/3] corpus-forced capture 2: superflat FULL (create + forced tickets + capture, 1/1 pin)..."
    );
    let (boot_b, _) = fresh_extraction(&run_dir, &jar, &cfg, "fullb")?;

    println!("[3/3] byte-comparing the two independent extractions...");
    if !trees_byte_identical(&boot_a, &boot_b)? {
        eprintln!(
            "regeneration ABORTED: the two independent corpus-forced Paper captures produced \
             DIFFERENT chunk payloads — the superflat FULL generation is not byte-deterministic \
             (after LastUpdate normalization), so nothing is committed (issue #51).\n\
             capture A kept at: {}\n\
             capture B kept at: {}\n\
             destination {} was NOT touched.",
            boot_a.display(),
            boot_b.display(),
            dest.display()
        );
        return Err(Error::Gate(
            "superflat-FULL twin-boot byte-identity check failed — refusing to commit a \
             nondeterministic pair"
                .into(),
        ));
    }

    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    copy_dir_recursive(&boot_a, dest)?;
    let _ = fs::remove_dir_all(&boot_a);
    let _ = fs::remove_dir_all(&boot_b);
    println!(
        "regenerated superflat status-FULL region payloads under {} (twin-boot byte-identical; \
         chunk-concurrency provenance recorded)",
        dest.display()
    );
    Ok(())
}

/// `regenerate`: full regeneration of every fixture kind (or a sub-selection
/// via `--m0` / `--m2` / `--full` / `--samples` / `--text`). A bare invocation
/// regenerates all five kinds — M0, M2, the superflat status-FULL capture
/// (issue #51), the derived worldgen samples, and the derived text corpus
/// (issue #98).
///
/// Each kind defaults to its committed location (M0 -> `fixtures/`, M2 ->
/// `fixtures/regions/overworld-normal/`, FULL -> `fixtures/regions/superflat-full/`;
/// the derived kinds always regenerate their committed `fixtures/worldgen` /
/// `fixtures/text` trees), so the official path refreshes the golden fixtures in
/// place. An explicit `--to <dir>` overrides the destination for a single
/// *booting* kind (`--m0`/`--m2`/`--full` only; refused for bare/combined
/// selections and for the derived kinds), regenerating into a scratch dir for
/// gate validation before anything is committed — the
/// M0-verify-in-a-temporary-destination path.
fn run_regenerate(only: &[&str], to: Option<&Path>) -> Result<(), Error> {
    for flag in only {
        if !matches!(
            *flag,
            "--m0" | "--m2" | "--full" | "--samples" | "--text" | "--composed-noise"
        ) {
            return Err(Error::Gate(format!("unknown regenerate flag: {flag}")));
        }
    }
    // `--to <dir>` is only meaningful for a single booting kind (see
    // `to_targets_single_booting_kind`): the derived kinds always regenerate
    // their committed fixture trees and ignore a destination, and a shared
    // destination across the booting kinds would let M2/FULL's twin-boot copy
    // replace the whole directory, silently discarding the other kind's output.
    if to.is_some() && !to_targets_single_booting_kind(only) {
        let what = if only.is_empty() {
            String::from("all kinds (bare regenerate)")
        } else {
            only.join(" ")
        };
        return Err(Error::Gate(format!(
            "regenerate --to <dir> requires exactly one of --m0/--m2/--full — bare \
             and combined selections, and the derived kinds --samples/--text/\
             --composed-noise (which regenerate their committed fixture trees and \
             ignore --to), are refused; got {what}"
        )));
    }
    let m0 = regenerates_kind("--m0", only);
    let m2 = regenerates_kind("--m2", only);
    let full = regenerates_kind("--full", only);
    let samples = regenerates_kind("--samples", only);
    let text = regenerates_kind("--text", only);
    let composed_noise = regenerates_kind("--composed-noise", only);
    let m0_default = crate_dir().join("fixtures");
    let m2_default = crate_dir().join("fixtures/regions/overworld-normal");
    let full_default = crate_dir().join("fixtures/regions/superflat-full");
    let m0_dest = to.unwrap_or(&m0_default);
    let m2_dest = to.unwrap_or(&m2_default);
    let full_dest = to.unwrap_or(&full_default);
    if m0 {
        println!("==> regenerating M0 golden chunk slice");
        regenerate_m0(m0_dest)?;
    }
    if m2 {
        println!("==> regenerating M2 normal-overworld region payloads");
        regenerate_m2(m2_dest)?;
    }
    if full {
        println!("==> regenerating superflat status-FULL region payloads (corpus-forced, seed 0)");
        regenerate_full(full_dest)?;
    }
    if samples {
        println!("==> regenerating worldgen semantic samples");
        regenerate_samples()?;
    }
    if text {
        println!("==> regenerating text fixture corpus (corpus.json + golden.json)");
        regenerate_text()?;
    }
    if composed_noise {
        println!("==> regenerating composed-noise seed-42 goldens from pinned Paper");
        composed_noise::run_probe(&crate_dir().join("fixtures/composed-noise"))?;
    }
    Ok(())
}

/// Whether `--flag` is part of a `regenerate` selection: a bare invocation
/// selects every kind; otherwise only the explicitly named kinds run.
fn regenerates_kind(flag: &str, only: &[&str]) -> bool {
    only.is_empty() || only.contains(&flag)
}

/// Whether a `regenerate` selection may legally be combined with `--to <dir>`.
/// Only exactly one of the *booting* kinds (`--m0`/`--m2`/`--full`) writes to
/// the destination; bare/combined selections and the derived kinds
/// (`--samples`/`--text`, which regenerate their committed fixture trees and
/// ignore a destination) are refused.
fn to_targets_single_booting_kind(only: &[&str]) -> bool {
    matches!(only, ["--m0"] | ["--m2"] | ["--full"])
}

// ---- #54 chunk-hash engine commands -----------------------------------------
//
// The xxh3_64 seed-hash gate. Exit-code contract (matches the oracle's
// PASS=0 / FAIL=1 / UNVERIFIED=3 / usage=64):
//   hash-self-check              -> 0 (known-answer vectors pass) or 1
//   hash-paper [dir]             -> 0, builds/refreshes the committed Paper
//                                   manifest under fixtures/chunk-hash/paper/;
//                                   3 UNVERIFIED when its payload source is
//                                   unavailable (never a fabricated
//                                   zero-chunk manifest)
//   hash-rivet <dir>             -> 3 UNVERIFIED until a Rivet chunk tree with
//                                   FULL chunks exists (no Rivet serialization
//                                   today); reads a Rivet region tree layout
//                                   when present.
//   hash-diff <paper> <rivet>    -> 0 match / 1 mismatch naming each chunk /
//                                   3 UNVERIFIED (missing Rivet manifest, a
//                                   Paper-vs-Paper self-diff — the same tree on
//                                   both sides proves nothing about Rivet, or a
//                                   required corpus coordinate with no FULL
//                                   data) / 64 usage.
//   hash-diff --expect-fail ...  -> negative control: corrupt a copy of the
//                                   Rivet baseline, require the tampered chunk
//                                   named — and only it. Kinds: block, light,
//                                   heightmap, nbt-order, nbt-key, or all.
//
// Live FULL-chunk generation is blocked (#51 must capture status-FULL region
// fixtures and Rivet worldgen must reach FULL, #231/#15); pre-worldgen the
// gate skips the Paper-vs-Rivet diff with an explicit NOTICE and never runs a
// self-diff — it never claims parity it does not have. The gate's hash stage is
// milestone-gated (not an oracle prereq), so an absent comparison does not fail
// the gate. Setting RIVET_HASH_DIR opts into the strict check: the comparison
// is then required, and any UNVERIFIED (incomplete corpus coverage, or a
// self-diff if it aliases the paper tree) or FAILED divergence aborts the gate.

/// The world seed recorded in a fixture dir's own manifest.json, when present.
/// `load_manifest` reads the region/worldgen manifest (seed is a string field);
/// a chunk-hash manifest.json parses under the same struct (unknown fields are
/// ignored), so a Rivet tree that already carries provenance yields its seed
/// here too. This keeps the recorded hash-manifest seed honest — it is the
/// actual seed the payloads were generated under, never a magic literal.
fn source_region_seed(dir: &Path) -> Option<String> {
    load_manifest(dir).ok()?.seed
}

/// Read the source region capture's manifest provenance that a hash manifest
/// must inherit (seed, level-type, region-file-compression). `load_manifest`
/// parses the extract script's manifest (unknown fields ignored), so a Rivet
/// tree that already carries provenance yields it here too. Every value is the
/// actual one the payloads were generated under, never a magic literal.
fn source_region_provenance(dir: &Path) -> Option<hash_manifest::CaptureProvenance> {
    let m = load_manifest(dir).ok()?;
    Some(hash_manifest::CaptureProvenance::from_region_manifest(
        m.level_type.as_deref(),
        m.region_file_compression.as_deref(),
    ))
}

/// Parse the `extract-world` subcommand's `--to <path>` and world-dir
/// positional args (shared by `run()` and the CLI exit-code tests). A missing
/// world dir or malformed `--to` is a CLI usage error — `Error::Gate`, never
/// `Error::Unverified`, so the runner classifies it as FAIL, not as a missing
/// world prerequisite.
fn parse_extract_world_args(rest: &[&str]) -> Result<(PathBuf, Option<PathBuf>), Error> {
    let mut to: Option<PathBuf> = None;
    let mut world_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--to" => {
                let Some(path) = rest.get(i + 1) else {
                    return Err(Error::Gate(
                        "extract-world --to requires a destination path".into(),
                    ));
                };
                to = Some(PathBuf::from(path));
                i += 2;
            }
            other if !other.starts_with('-') => {
                world_dir = Some(PathBuf::from(other));
                i += 1;
            }
            other => {
                return Err(Error::Gate(format!(
                    "extract-world: unknown option {other}"
                )));
            }
        }
    }
    let world_dir = world_dir.ok_or_else(|| {
        Error::Gate("extract-world requires a disposable world root directory".into())
    })?;
    Ok((world_dir, to))
}

/// Extract the loaded-world ground-truth manifest from a disposable world copy
/// (issue #374). The extraction is strictly read-only — every region opens
/// through a read descriptor, and an allocated corrupt chunk is a hard
/// `InvalidData` error rather than an absent chunk. The manifest is printed as
/// compact JSON (or written to `to` when given) for the `rivet-loaded-world`
/// runner's PASS comparison and for the tamper negative controls.
fn run_extract_world(world_dir: &Path, to: Option<&Path>) -> Result<(), Error> {
    let manifest = loaded_world::extract_world(world_dir).map_err(|e| match e {
        loaded_world::ExtractError::Unverified(m) => Error::Unverified(m),
        loaded_world::ExtractError::Gate(m) => Error::Gate(m),
        loaded_world::ExtractError::Io(io) => Error::Io(io),
    })?;
    let json = serde_json::to_string(&manifest)
        .map_err(|e| Error::Gate(format!("serializing loaded-world manifest: {e}")))?;
    match to {
        Some(path) => fs::write(path, json.as_bytes())
            .map_err(Error::Io)
            .map(|_| ())?,
        None => println!("{json}"),
    }
    Ok(())
}

/// `hash-paper`: rebuild the committed Paper manifest from the decompressed
/// `.nbt` fixtures. The seed, level-type, region-file-compression, and corpus
/// version recorded are all read back out of the source region capture's
/// manifest — the default source is the M2 capture (working seed 42, distinct
/// from the pinned corpus seeds, which are the #175 sweep targets), whose only
/// FULL chunks are the_nether/0.0 and the_end/0.0. The single `dir` argument
/// overrides both the payload source and the manifest destination (one tree):
/// run it against a scratch copy of a different tree to hash that tree without
/// touching committed fixtures — e.g. a copy of the corpus-forced superflat-full
/// capture reports its 8 FULL chunks per dimension (corpus seed 0,
/// 5207638315753790570, `minecraft\:flat`, issue #51). Nothing is hardcoded;
/// the paper pin is a constant.
fn run_hash_paper(dir: Option<&Path>) -> Result<(), Error> {
    hash::self_check().map_err(Error::Gate)?;
    let dest = dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| crate_dir().join("fixtures/chunk-hash/paper"));
    let payload_dir = dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| crate_dir().join("fixtures/regions/overworld-normal"));
    // A missing payload tree, or a tree with no chunk payloads at all, cannot
    // produce an honest digest table — writing a zero-chunk manifest would let
    // a later Paper-vs-Rivet diff compare "Paper (empty) vs Rivet" and possibly
    // go vacuously green. This is UNVERIFIED (exit 3), never a green. (A tree
    // with payloads but zero FULL chunks is a different, legitimate pre-worldgen
    // state that hash-rivet already reports as UNVERIFIED; hash-paper's source
    // is the committed Paper capture, which must actually exist.)
    if !payload_dir.is_dir() {
        return Err(Error::Unverified(format!(
            "hash-paper: payload source {} is not a directory — cannot build an honest \
             Paper digest table; UNVERIFIED, never a zero-chunk manifest",
            payload_dir.display()
        )));
    }
    if !payload_dir.join("chunk").is_dir() {
        return Err(Error::Unverified(format!(
            "hash-paper: {} has no chunk/ payload tree — nothing to hash; UNVERIFIED, \
             never a zero-chunk manifest",
            payload_dir.display()
        )));
    }
    let prov = source_region_provenance(&payload_dir).unwrap_or_default();
    let seed =
        source_region_seed(&payload_dir).unwrap_or_else(|| hash_manifest::CAPTURE_SEED.to_string());
    let manifest =
        hash_manifest::build_from_payloads_with(&payload_dir, &seed, &prov.level_type, &prov)
            .map_err(Error::Gate)?;
    if manifest.entries.is_empty() {
        return Err(Error::Unverified(format!(
            "hash-paper: no .nbt chunk payloads under {} — writing a zero-chunk manifest \
             would fabricate green; UNVERIFIED",
            payload_dir.display()
        )));
    }
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::Gate(format!("cannot serialize hash manifest: {e}")))?;
    fs::create_dir_all(&dest)
        .map_err(|e| Error::Gate(format!("cannot create {}: {e}", dest.display())))?;
    let path = dest.join("manifest.json");
    fs::write(&path, json + "\n")
        .map_err(|e| Error::Gate(format!("cannot write {}: {e}", path.display())))?;
    let mut dim_full: std::collections::BTreeMap<&str, usize> = Default::default();
    for e in &manifest.entries {
        if e.is_full() {
            *dim_full.entry(e.dim.as_str()).or_default() += 1;
        }
    }
    let dim_narration = dim_full
        .iter()
        .map(|(d, n)| format!("{d}: {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "hash-paper: wrote {} ({} chunks, {} FULL: {})",
        path.display(),
        manifest.chunk_count,
        manifest.full_count,
        if dim_narration.is_empty() {
            "none".to_string()
        } else {
            dim_narration
        }
    );
    let cov = hash_manifest::coverage(&manifest, &corpus::Corpus::from_committed());
    let complete = if cov.is_complete() { " (complete)" } else { "" };
    println!(
        "  coverage vs corpus: {}/{} present{complete}; missing: {}; extra: {}",
        cov.present,
        cov.expected,
        cov.missing.join(", "),
        cov.extra.join(", ")
    );
    Ok(())
}

/// Load a `HashManifest` from a `manifest.json`.
fn load_hash_manifest(dir: &Path) -> Result<hash_manifest::HashManifest, Error> {
    let path = dir.join("manifest.json");
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::Gate(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::Gate(format!("invalid hash manifest {}: {e}", path.display())))
}

/// `hash-rivet`: read a Rivet chunk tree. There is no Rivet chunk
/// serialization yet (RivetTodo #231/#15), so this reports UNVERIFIED (3)
/// rather than fabricating green.
fn run_hash_rivet(dir: &Path) -> Result<(), Error> {
    hash::self_check().map_err(Error::Gate)?;
    if !dir.is_dir() {
        return Err(Error::Gate(format!(
            "Rivet chunk dir {} does not exist — no Rivet chunk serialization yet (#231/#15)",
            dir.display()
        )));
    }
    let seed = source_region_seed(dir).unwrap_or_else(|| hash_manifest::CAPTURE_SEED.to_string());
    let prov = source_region_provenance(dir).unwrap_or_default();
    let manifest = hash_manifest::build_from_payloads_with(dir, &seed, &prov.level_type, &prov)
        .map_err(Error::Gate)?;
    if manifest.full_count == 0 {
        return Err(Error::Gate(format!(
            "Rivet chunk tree {} has 0 FULL chunks — Rivet worldgen has not reached FULL \
             (blocked on #51 capturing status-FULL regions + #231/#15 serialization); \
             the Paper-vs-Rivet hash-diff is UNVERIFIED, never green",
            dir.display()
        )));
    }
    println!(
        "hash-rivet: {} chunks, {} FULL — a Rivet manifest exists to compare",
        manifest.chunk_count, manifest.full_count
    );
    Ok(())
}

/// The per-chunk result of a `hash-diff`.
struct ChunkHashMismatch {
    dim: String,
    cx: i32,
    cz: i32,
    expected: String,
    actual: String,
    /// True when the two chunks are canonical-identical (order-only diff).
    order_only: bool,
}

/// `hash-diff`: compare a Paper `HashManifest` against a Rivet one. Refuses
/// differing provenance; only FULL entries are compared (non-FULL is recorded
/// and reported, never silently included); a missing Rivet manifest, a
/// Paper-vs-Paper self-diff, or a required corpus coordinate with no FULL data
/// on either side, is UNVERIFIED.
///
/// Returns: Ok(true) = PASS, Ok(false) = FAIL, Err = UNVERIFIED (3).
fn run_hash_diff(paper_dir: &Path, rivet_dir: &Path) -> Result<bool, Error> {
    hash::self_check().map_err(Error::Gate)?;
    // A self-comparison (both args the same committed Paper manifest) compares
    // Paper against itself and proves nothing about Rivet — it must never
    // produce a Paper-vs-Rivet PASS. Canonicalize so an aliased path to the
    // same tree is still refused; a nonexistent Rivet dir is caught by the
    // manifest check below with a clearer message.
    if rivet_dir.is_dir() {
        let paper_canon = paper_dir
            .canonicalize()
            .map_err(|e| Error::Gate(format!("cannot resolve {}: {e}", paper_dir.display())))?;
        let rivet_canon = rivet_dir
            .canonicalize()
            .map_err(|e| Error::Gate(format!("cannot resolve {}: {e}", rivet_dir.display())))?;
        if paper_canon == rivet_canon {
            return Err(Error::Gate(format!(
                "paper and rivet dirs are the same tree ({}): a Paper-vs-Paper self-diff \
                 proves nothing about Rivet — pass a distinct Rivet chunk dir; UNVERIFIED, \
                 never green",
                paper_canon.display()
            )));
        }
    }
    if !rivet_dir.join("manifest.json").is_file() {
        return Err(Error::Gate(format!(
            "no Rivet hash manifest at {} — pre-worldgen the Paper-vs-Rivet diff is \
             UNVERIFIED (3), never green (Rivet chunk serialization is #231/#15)",
            rivet_dir.display()
        )));
    }
    let paper = load_hash_manifest(paper_dir)?;
    let rivet = load_hash_manifest(rivet_dir)?;

    if paper.provenance() != rivet.provenance() {
        return Err(Error::Gate(format!(
            "provenance mismatch — refusing to compare manifests of different seed/algorithm/\
             paper/concurrency:\n  paper: {}\n  rivet: {}",
            paper.provenance().describe(),
            rivet.provenance().describe()
        )));
    }

    // Required corpus coordinates: a required coordinate with no FULL entry on
    // either side means the sweep cannot claim coverage — UNVERIFIED.
    let mut required_missing = Vec::new();
    for (x, z) in corpus::COORDINATES {
        let paper_has = paper.full_entry("the_nether", *x, *z).is_some()
            || paper.full_entry("the_end", *x, *z).is_some()
            || paper.full_entry("overworld", *x, *z).is_some();
        let rivet_has = rivet.full_entry("the_nether", *x, *z).is_some()
            || rivet.full_entry("the_end", *x, *z).is_some()
            || rivet.full_entry("overworld", *x, *z).is_some();
        if !paper_has || !rivet_has {
            required_missing.push(format!("({x},{z})"));
        }
    }
    if !required_missing.is_empty() {
        return Err(Error::Gate(format!(
            "required corpus coordinates with no FULL data on both sides: {} — a green \
             sweep over the #175 matrix is not yet achievable (needs #51 to capture \
             status-FULL regions and Rivet worldgen to reach FULL); UNVERIFIED, never green",
            required_missing.join(", ")
        )));
    }

    let (mismatches, paper_only, rivet_only, compared) = compute_hash_diffs(&paper, &rivet);

    if mismatches.is_empty() && paper_only.is_empty() && rivet_only.is_empty() {
        println!(
            "hash-diff PASS: {compared} FULL chunks match Paper == Rivet ({} entries, {} FULL)",
            paper.chunk_count, paper.full_count
        );
        return Ok(true);
    }
    for m in &mismatches {
        let triage = if m.order_only {
            " (canonical-identical — serialization order only, triage)"
        } else {
            ""
        };
        println!(
            "MISMATCH {}/{}.{}: expected {} got {}{}",
            m.dim, m.cx, m.cz, m.expected, m.actual, triage
        );
    }
    for e in &paper_only {
        println!("PAPER-ONLY FULL (Paper has, Rivet is missing): {e}");
    }
    for e in &rivet_only {
        println!("RIVET-ONLY FULL (Rivet over-generated): {e}");
    }
    println!(
        "hash-diff FAIL: {} mismatched, {} paper-only, {} rivet-only, {} compared",
        mismatches.len(),
        paper_only.len(),
        rivet_only.len(),
        compared
    );
    Ok(false)
}

/// Per-chunk FULL-entry comparison between two manifests: `mismatches` are the
/// shared (dim, cx, cz) FULL entries whose raw digest differs (with an
/// order-only triage flag when the canonical digests agree), `paper_only` are
/// the FULL entries present in Paper but absent from Rivet (a chunk Rivet
/// failed to produce — a divergent omission that must not slide through as
/// green), `rivet_only` are the FULL entries present in Rivet but absent from
/// Paper (Rivet over-generation), and `compared` is how many shared FULL pairs
/// were checked. The two one-sided directions are kept separate because they
/// diagnose opposite failures and must be reported with the correct direction
/// (a Paper-only chunk is NOT Rivet over-generation). Pure — the caller prints
/// and decides PASS/FAIL, and tests assert the exact set of named chunks.
fn compute_hash_diffs(
    paper: &hash_manifest::HashManifest,
    rivet: &hash_manifest::HashManifest,
) -> (Vec<ChunkHashMismatch>, Vec<String>, Vec<String>, usize) {
    let mut mismatches: Vec<ChunkHashMismatch> = Vec::new();
    let mut paper_only: Vec<String> = Vec::new();
    let mut rivet_only: Vec<String> = Vec::new();
    let mut compared = 0usize;
    for pe in &paper.entries {
        if !pe.is_full() {
            continue;
        }
        let Some(re) = rivet.full_entry(&pe.dim, pe.cx, pe.cz) else {
            paper_only.push(format!("{}/{}.{}.{}", pe.dim, pe.region, pe.cx, pe.cz));
            continue; // Paper-only FULL chunk — pushed here; the one-sided pass below only computes Rivet-only.
        };
        compared += 1;
        if pe.xxh3_64 != re.xxh3_64 {
            let order_only = pe.xxh3_64_canonical == re.xxh3_64_canonical;
            mismatches.push(ChunkHashMismatch {
                dim: pe.dim.clone(),
                cx: pe.cx,
                cz: pe.cz,
                expected: pe.xxh3_64.clone(),
                actual: re.xxh3_64.clone(),
                order_only,
            });
        }
    }
    // One-sided FULL entries, either direction: Rivet-only (over-generation)
    // and Paper-only (a chunk Rivet failed to produce). Both are divergence and
    // must fail the diff, never pass vacuously. Directions are reported
    // distinctly — a Paper-only chunk diagnoses "Rivet is missing a chunk Paper
    // has", never the reverse.
    for re in &rivet.entries {
        if re.is_full() && paper.full_entry(&re.dim, re.cx, re.cz).is_none() {
            rivet_only.push(format!("{}/{}.{}.{}", re.dim, re.region, re.cx, re.cz));
        }
    }
    (mismatches, paper_only, rivet_only, compared)
}

/// `hash-diff --expect-fail`: negative control. Corrupt a copy of the baseline
/// and require the tampered chunk to be named. Passes only when the diff names
/// exactly the tampered chunk — a FAIL for any other reason (a different chunk,
/// a provenance mismatch, an unrelated divergence) is rejected as a
/// wrong-reason pass.
fn run_hash_diff_negative(
    paper_dir: &Path,
    rivet_dir: &Path,
    kind: mutate::TamperKind,
) -> Result<(), Error> {
    hash::self_check().map_err(Error::Gate)?;
    if !rivet_dir.join("manifest.json").is_file() {
        return Err(Error::Gate(format!(
            "negative control needs a Rivet manifest at {} to corrupt — pre-worldgen this \
             is UNVERIFIED",
            rivet_dir.display()
        )));
    }
    // Corrupt one committed payload in a scratch copy and rebuild its manifest
    // with the tampered bytes, so the copy is internally consistent (a
    // plausible but wrong Rivet baseline).
    let scratch = env::temp_dir().join(format!("rivet-oracle-hash-neg-{}", std::process::id()));
    if scratch.exists() {
        fs::remove_dir_all(&scratch)?;
    }
    copy_dir_recursive(rivet_dir, &scratch)?;
    // Find a FULL payload to tamper. The corrupted copy must keep the original
    // manifest's provenance (seed included) so the diff against Paper sees only
    // the tamper, never a spurious provenance mismatch.
    let manifest = load_hash_manifest(rivet_dir)?;
    let seed = manifest.seed.clone();
    let prov = hash_manifest::CaptureProvenance::from_region_manifest(
        Some(&manifest.level_type),
        Some(&manifest.region_file_compression),
    );
    let Some(full) = manifest.entries.iter().find(|e| e.is_full()) else {
        let _ = fs::remove_dir_all(&scratch);
        return Err(Error::Gate(
            "negative control needs a FULL chunk in the Rivet baseline to tamper".into(),
        ));
    };
    let target = scratch
        .join("chunk")
        .join(&full.dim)
        .join(&full.region)
        .join(format!("{}.{}.nbt", full.cx, full.cz));
    let payload = fs::read(&target)
        .map_err(|e| Error::Gate(format!("cannot read {}: {e}", target.display())))?;
    let tampered = mutate::tamper(&payload, kind).map_err(Error::Gate)?;
    fs::write(&target, tampered)
        .map_err(|e| Error::Gate(format!("cannot write tampered payload: {e}")))?;
    // Rebuild the corrupted copy's manifest from its payloads, keeping the
    // original seed + level-type + compression so the tamper is the only
    // divergence (the corrupted copy inherits the original provenance).
    let rebuilt =
        hash_manifest::build_from_payloads_with(&scratch, &seed, &manifest.level_type, &prov)
            .map_err(Error::Gate)?;
    let json = serde_json::to_string_pretty(&rebuilt)
        .map_err(|e| Error::Gate(format!("cannot serialize: {e}")))?;
    fs::write(scratch.join("manifest.json"), json + "\n")
        .map_err(|e| Error::Gate(format!("cannot write manifest: {e}")))?;

    // Now the Paper-vs-Rivet diff against the corrupted copy must FAIL and name
    // exactly the tampered chunk. Accepting "any diff failure" is not enough: a
    // comparator that broke in an unrelated way (e.g. refusing to compare at
    // all, or failing on the wrong chunk) must not pass the control. The tampered
    // dim + coordinate are asserted by re-reading the rebuilt manifest, and the
    // run_hash_diff return value is examined structurally (FAIL + exactly the
    // tampered coordinate in the mismatch set), never just "exit nonzero".
    let (mismatches, paper_only, rivet_only, _) =
        compute_hash_diffs(&load_hash_manifest(paper_dir)?, &rebuilt);
    let tamper_label = format!(
        "{} tamper on {}/{}",
        kind.cli_name(),
        full.dim,
        fmt_hash_coord(full.cx, full.cz)
    );
    // The tamper must be the ONLY divergence: the digest-mismatch set is
    // non-empty and names exactly the tampered chunk, AND there is no one-sided
    // FULL divergence in either direction (a one-sided divergence alongside the
    // tamper means the comparator is also failing for a second, unrelated
    // reason). A vacuous pass (empty) or a wrong-chunk failure must also be
    // caught — "any diff failure" never satisfies the control.
    if !tamper_divergence_is_exactly(
        &mismatches,
        &paper_only,
        &rivet_only,
        &full.dim,
        full.cx,
        full.cz,
    ) {
        let _ = fs::remove_dir_all(&scratch);
        return Err(Error::Gate(format!(
            "negative control FAILED: {tamper_label} was not reported as the ONLY divergence \
             (digest mismatches: {}; paper-only: {}; rivet-only: {}) — the comparator must \
             name exactly the tampered dimension/coordinate and nothing else, not accept any \
             diff failure",
            mismatches
                .iter()
                .map(|m| format!("{}/{}", m.dim, fmt_hash_coord(m.cx, m.cz)))
                .collect::<Vec<_>>()
                .join(", "),
            paper_only.join(", "),
            rivet_only.join(", ")
        )));
    }
    match run_hash_diff(paper_dir, &scratch) {
        Ok(true) => {
            let _ = fs::remove_dir_all(&scratch);
            Err(Error::Gate(format!(
                "negative control FAILED: {tamper_label} was NOT detected by the full diff — \
                 the comparator is vacuously green"
            )))
        }
        Ok(false) => {
            let _ = fs::remove_dir_all(&scratch);
            println!("negative control PASS: {tamper_label} detected and named exactly");
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&scratch);
            Err(Error::Gate(format!(
                "negative control could not run the diff (exit 3): {e}"
            )))
        }
    }
}

/// Whether a negative-control mismatch set proves the tampered chunk was named
/// **exactly**: the set is non-empty, and every reported divergence is the
/// tampered (dim, cx, cz). An empty set means the comparator went vacuously
/// green; a mismatch at a different coordinate means it failed for the wrong
/// reason — neither satisfies the negative control, which must prove the
/// reported mismatch names exactly the tampered dimension/coordinate.
fn mismatch_set_names_exactly(
    mismatches: &[ChunkHashMismatch],
    dim: &str,
    cx: i32,
    cz: i32,
) -> bool {
    !mismatches.is_empty()
        && mismatches
            .iter()
            .all(|m| m.dim == dim && (m.cx, m.cz) == (cx, cz))
}

/// Whether the negative-control divergence proves the tamper was the **only**
/// divergence: the digest-mismatch set names exactly the tampered chunk AND
/// there is no one-sided FULL divergence in either direction. A one-sided FULL
/// divergence alongside the tamper means the comparator is also failing for a
/// second, unrelated reason, so the reported failure is not strictly "exactly
/// the tampered chunk" — the control must not pass on it.
fn tamper_divergence_is_exactly(
    mismatches: &[ChunkHashMismatch],
    paper_only: &[String],
    rivet_only: &[String],
    dim: &str,
    cx: i32,
    cz: i32,
) -> bool {
    mismatch_set_names_exactly(mismatches, dim, cx, cz)
        && paper_only.is_empty()
        && rivet_only.is_empty()
}

/// `<dim>/<cx>.<cz>` for a coordinate in hash-diff output.
fn fmt_hash_coord(cx: i32, cz: i32) -> String {
    format!("{cx}.{cz}")
}

/// The oracle's shared exit-code contract (the #54 chunk-hash engine and the
/// issue #374 `extract-world` command, whose runner classifies on the code):
/// PASS=0 / FAIL=1 / UNVERIFIED=3 / usage=64 (EX_USAGE). `main()` routes the
/// `hash-*` subcommands here before `run()` so they own their exit codes
/// precisely; `run()` errors map through `exit_code_for_run_error` so an
/// `Error::Unverified` (a missing prerequisite, e.g. `extract-world` against a
/// world with no region layout) exits 3, never a bare FAIL.
const EXIT_FAIL: i32 = 1;
const EXIT_UNVERIFIED: i32 = 3;
const EXIT_USAGE: i32 = 64;

/// Map a [`run()`] error onto the shared exit-code contract. An
/// `Error::Unverified` is a missing prerequisite (3); every other error —
/// malformed CLI, gate/internal failure, io — is a hard FAIL (1). The runner
/// must never see a malformed manifest or an internal failure downgraded to
/// UNVERIFIED.
fn exit_code_for_run_error(e: &Error) -> i32 {
    match e {
        Error::Unverified(_) => EXIT_UNVERIFIED,
        _ => EXIT_FAIL,
    }
}

/// Dispatch a `hash-*` subcommand to the right runner, mapping outcomes to the
/// #54 exit-code contract. Returns the process exit code; the caller exits with
/// it directly. All non-`hash-*` commands return `None`.
fn hash_cli_exit(args: &[String]) -> Option<i32> {
    let cmd = args.first()?;
    if !cmd.starts_with("hash-") {
        return None;
    }
    Some(match cmd.as_str() {
        "hash-self-check" => match hash::self_check() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("rivet-oracle: {e}");
                EXIT_FAIL
            }
        },
        "hash-paper" => {
            let dir = args.get(1).map(PathBuf::from);
            match run_hash_paper(dir.as_deref()) {
                Ok(()) => 0,
                Err(Error::Unverified(m)) => {
                    eprintln!("rivet-oracle: {m}");
                    EXIT_UNVERIFIED
                }
                Err(e) => {
                    eprintln!("rivet-oracle: {e}");
                    EXIT_FAIL
                }
            }
        }
        "hash-rivet" => {
            let Some(dir) = args.get(1) else {
                hash_usage("hash-rivet");
                return Some(EXIT_USAGE);
            };
            match run_hash_rivet(Path::new(dir)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("rivet-oracle: {e}");
                    EXIT_UNVERIFIED
                }
            }
        }
        "hash-diff" => hash_diff_exit(args),
        other => {
            eprintln!("rivet-oracle: unknown hash-* command `{other}`");
            hash_usage(other);
            EXIT_USAGE
        }
    })
}

/// `hash-diff` / `hash-diff --expect-fail`: compare Paper vs Rivet manifests,
/// or run the tamper negative control. Maps to the #54 exit-code contract.
fn hash_diff_exit(args: &[String]) -> i32 {
    let rest = &args[1..];
    if rest.first().map(String::as_str) == Some("--expect-fail") {
        return hash_diff_negative_exit(&rest[1..]);
    }
    let [paper, rivet] = rest else {
        hash_usage("hash-diff");
        return EXIT_USAGE;
    };
    match run_hash_diff(Path::new(paper), Path::new(rivet)) {
        Ok(true) => 0,
        Ok(false) => EXIT_FAIL,
        Err(e) => {
            eprintln!("rivet-oracle: {e}");
            EXIT_UNVERIFIED
        }
    }
}

/// `hash-diff --expect-fail <paper> <rivet> [kind]`: corrupt a copy of the
/// baseline and require the tampered chunk to be named. Passes (0) only when
/// the negative control detects and names the tamper. `kind` defaults to
/// `block`; `all` runs every mutation class so a future kind that the
/// comparator silently ignores is caught.
fn hash_diff_negative_exit(args: &[String]) -> i32 {
    let kinds: Vec<TamperKind> = match args.len() {
        2 => vec![TamperKind::Block],
        3 => match args[2].as_str() {
            "all" => TamperKind::ALL.to_vec(),
            name => match TamperKind::from_cli(name) {
                Some(kind) => vec![kind],
                None => {
                    hash_usage("hash-diff --expect-fail");
                    return EXIT_USAGE;
                }
            },
        },
        _ => {
            hash_usage("hash-diff --expect-fail");
            return EXIT_USAGE;
        }
    };
    let mut failed = false;
    for kind in kinds {
        match run_hash_diff_negative(Path::new(&args[0]), Path::new(&args[1]), kind) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("rivet-oracle: {e}");
                failed = true;
            }
        }
    }
    if failed { EXIT_FAIL } else { 0 }
}

/// Print the #54 command usage line for the given `hash-*` subcommand.
fn hash_usage(cmd: &str) {
    eprintln!("usage: cargo run -p rivet-oracle -- {cmd} ... (see README 'Chunk-hash engine')");
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
    println!("  cargo run -p rivet-oracle -- verify --full  FULL region gate (superflat");
    println!(
        "                                                     status-FULL region capture, issue"
    );
    println!("                                                     #51)");
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
    println!("  cargo run -p rivet-oracle -- composed-noise [--tamper | --sample]");
    println!(
        "                                             composed-noise golden comparison: verify the"
    );
    println!(
        "                                             seed-42 NOISE-checkpoint goldens + print the"
    );
    println!(
        "                                             status/provenance scoreboard; --tamper is the"
    );
    println!(
        "                                             negative control; --sample regenerates from"
    );
    println!("                                             the pinned Paper runtime");
    println!("  cargo run -p rivet-oracle -- regenerate     regenerate ALL fixture kinds");
    println!(
        "                                             (sub-select: --m0 / --m2 / --full / --samples / --text /"
    );
    println!("                                              --composed-noise;");
    println!(
        "                                              --to <dir> — exactly one of --m0/--m2/--full"
    );
    println!("                                              — writes into a scratch dir for");
    println!(
        "                                              gate validation; --samples/--text always"
    );
    println!(
        "                                              regenerate their committed fixture trees,"
    );
    println!(
        "                                              needing the materialized Paper runtime)"
    );
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
            let mut full = false;
            let mut expect_fail = false;
            let mut rest: Vec<String> = Vec::new();
            for a in args.iter().skip(1) {
                match a.as_str() {
                    "--m2" => m2 = true,
                    "--full" => full = true,
                    "--expect-fail" => expect_fail = true,
                    other => rest.push(other.to_string()),
                }
            }
            if m2 && full {
                return Err(Error::Gate(
                    "verify --m2 and verify --full are mutually exclusive".into(),
                ));
            }
            let cfg = if m2 {
                m2_config()
            } else if full {
                full_config()
            } else {
                m0_config()
            };
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
        Some("composed-noise") => {
            // The composed-noise golden comparison slice (NOISE checkpoint).
            //   cargo run -p rivet-oracle -- composed-noise            verify + scoreboard
            //   cargo run -p rivet-oracle -- composed-noise --tamper   negative control
            //   cargo run -p rivet-oracle -- composed-noise --sample   regenerate from pinned Paper
            let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
            if rest.contains(&"--help") || rest.contains(&"-h") {
                print_usage();
                return Ok(());
            }
            let dir = crate_dir().join("fixtures/composed-noise");
            match composed_noise::parse_mode(&rest)? {
                // The absent-golden exit-3 contract lives in require_fixture_tree
                // (shared by verify and the tamper control, matching the gate).
                composed_noise::ComposedNoiseMode::Tamper => {
                    composed_noise::tamper_negative_control(&dir)
                }
                composed_noise::ComposedNoiseMode::Sample => composed_noise::run_probe(&dir),
                composed_noise::ComposedNoiseMode::Verify => {
                    crate::verify_composed_noise_step(&dir)
                }
            }
        }
        Some("sample") => regenerate_samples(),
        Some("extract-world") => {
            // Issue #374 ground-truth extraction: read a disposable world copy
            // read-only and print the deterministic loaded-world manifest.
            // `--to <path>` writes the manifest JSON to a file instead of
            // stdout (the runner captures it for the PASS comparison).
            let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
            let (world_dir, to) = parse_extract_world_args(&rest)?;
            run_extract_world(&world_dir, to.as_deref())
        }
        Some("regenerate") => {
            // `--to <dir>` overrides the destination for a single booting kind
            // (regenerate into a scratch dir for gate validation before
            // committing); run_regenerate refuses bare/combined selections and
            // the derived kinds.
            let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
            let mut to: Option<PathBuf> = None;
            let mut flags: Vec<&str> = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--to" {
                    let Some(dir) = rest.get(i + 1) else {
                        return Err(Error::Gate(
                            "regenerate --to requires a destination dir".into(),
                        ));
                    };
                    to = Some(PathBuf::from(dir));
                    i += 2;
                } else {
                    flags.push(rest[i]);
                    i += 1;
                }
            }
            run_regenerate(&flags, to.as_deref())
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
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(exit) = hash_cli_exit(&args) {
        std::process::exit(exit);
    }
    if let Err(e) = run() {
        eprintln!("rivet-oracle: {e}");
        std::process::exit(exit_code_for_run_error(&e));
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
        // Issue #266: the regenerated region manifest carries the explicit m2
        // capture-kind, and MUST record the pinned 1/1 worker/I-O concurrency
        // it was generated under. Missing or drifted provenance is a hard
        // failure, never a skip.
        assert_eq!(
            manifest.kind.as_deref(),
            Some(KIND_M2),
            "regenerated M2 manifest must carry kind: m2"
        );
        assert_eq!(
            manifest.chunk_concurrency,
            Some(ChunkConcurrency::PINNED),
            "region capture must record pinned 1/1 chunk concurrency (issue #266)"
        );
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

    /// The committed `fixtures/loaded-world/` corpus (issue #371) must verify
    /// clean against its manifest, and each chunk payload must actually carry
    /// the aux-data shape it is named for: a clean FULL spawn chunk, a chunk
    /// with mineshaft `structures.References`, a chunk with saved `fluid_ticks`,
    /// a chunk with saved `block_ticks`, and a chunk with a chest
    /// `block_entities` entry. The payloads are read back through the proven
    /// rivet-nbt codec, so the test is grounded in the real fixture bytes, not
    /// in the extraction script's claims. The committed source fingerprint must
    /// also be present and well-formed — it is the enforcement backing the
    /// never-mutated provenance declaration.
    #[test]
    fn loaded_world_fixtures_verify() {
        let dir = fixtures_dir().join("loaded-world");
        // The corpus is the deliverable of #371, so its absence is a hard
        // failure, never a silent skip — a normal checkout always has it.
        assert!(
            dir.join("chunk").is_dir(),
            "loaded-world corpus is missing entirely (chunk/ absent)"
        );
        assert!(
            dir.join("manifest.json").is_file(),
            "loaded-world manifest.json missing while chunk payloads are present"
        );
        let manifest = verify_fixtures(&dir).expect("loaded-world fixtures should match manifest");
        assert_eq!(manifest.format, 1);
        assert_eq!(manifest.kind.as_deref(), Some("loaded-world"));
        assert_eq!(manifest.chunk_count, Some(5));
        assert_eq!(manifest.captured.len(), 5);
        assert_eq!(
            parse_paper_pin(manifest.paper.as_deref()),
            Some("0a99345".into())
        );
        assert!(
            !is_region_capture(&manifest),
            "loaded-world is a curated per-chunk corpus, never a region capture"
        );

        // Metadata beyond the Manifest subset is read from the raw JSON.
        let raw: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.join("manifest.json")).expect("committed manifest readable"),
        )
        .expect("committed manifest is valid JSON");
        assert_eq!(raw["data-version"], 4903);
        assert_eq!(raw["minecraft"], "26.2");
        // Provenance declaration: the launcher save was never mutated. The
        // committed source fingerprint is the enforcement backing this claim —
        // --verify recomputes it and refuses any drift.
        assert_eq!(
            raw["source"]["launcher-world-mutated"], false,
            "loaded-world provenance declares the launcher save was not mutated"
        );
        // The committed source fingerprint is the enforcement backing the
        // never-mutated provenance claim — --verify recomputes it and refuses
        // any drift. Its committed file must exist, be non-empty, well-formed,
        // and be the file the manifest references, so a pruned/missing
        // fingerprint can never pass silently.
        let fp_name = raw["source"]["fingerprint-file"]
            .as_str()
            .expect("fingerprint-file present");
        assert_eq!(fp_name, "source-fingerprint.txt");
        let fp_text =
            fs::read_to_string(dir.join(fp_name)).expect("committed source fingerprint readable");
        assert!(
            !fp_text.trim().is_empty(),
            "source fingerprint is not empty"
        );
        let mut fp_entries = 0;
        for line in fp_text.lines() {
            let mut fields = line.split('\t');
            let rel = fields.next().expect("fingerprint line has a path");
            let hash = fields.next().expect("fingerprint line has a SHA-256");
            assert!(!rel.is_empty(), "fingerprint path is non-empty");
            assert_eq!(hash.len(), 64, "fingerprint SHA-256 is 64 hex chars");
            assert!(
                hash.chars().all(|c| c.is_ascii_hexdigit()),
                "fingerprint SHA-256 is hex"
            );
            fp_entries += 1;
        }
        assert!(fp_entries > 0, "source fingerprint lists at least one file");

        let mut by_role: std::collections::BTreeMap<&str, &serde_json::Value> =
            std::collections::BTreeMap::new();
        for cap in raw["captured"].as_array().expect("captured is an array") {
            let role = cap["role"].as_str().expect("role present");
            assert!(
                by_role.insert(role, cap).is_none(),
                "role {role} appears once"
            );
            let chunk_path = dir.join(cap["path"].as_str().expect("path present"));
            let bytes = fs::read(&chunk_path).expect("fixture file readable");
            let mut input = DataInputStream::new(std::io::Cursor::new(bytes));
            let root = nbt_io::read(
                &mut input,
                &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
            )
            .expect("fixture parses as NBT");

            assert_eq!(root.get_int("DataVersion"), Some(4903));
            assert_eq!(
                root.get_string("Status").map(String::as_str),
                Some("minecraft:full")
            );
            assert_eq!(
                root.get_int("xPos"),
                Some(
                    cap["chunk"]
                        .as_str()
                        .expect("chunk present")
                        .split('.')
                        .next()
                        .unwrap()
                        .parse::<i32>()
                        .expect("xPos parses")
                )
            );
            assert_eq!(
                root.get_int("zPos"),
                Some(
                    cap["chunk"]
                        .as_str()
                        .expect("chunk present")
                        .split('.')
                        .nth(1)
                        .unwrap()
                        .parse::<i32>()
                        .expect("zPos parses")
                )
            );

            let ticks_len = |key: &str| -> usize {
                match root.get(key) {
                    Some(Tag::List(l)) => l.size(),
                    Some(_) => panic!("{key} is not a list in {}", cap["path"]),
                    None => 0,
                }
            };
            match role {
                "clean-spawn" => {
                    assert_eq!(
                        ticks_len("fluid_ticks"),
                        0,
                        "clean spawn has no fluid_ticks"
                    );
                    assert_eq!(
                        ticks_len("block_ticks"),
                        0,
                        "clean spawn has no block_ticks"
                    );
                    assert!(
                        ticks_len("block_entities") == 0,
                        "clean spawn has no block_entities"
                    );
                    // A FULL chunk always carries the structures compound; the
                    // clean-spawn property is that both References and starts are
                    // empty, so no structure reference blocks the FULL construction.
                    let structures = root
                        .get_compound("structures")
                        .expect("clean spawn carries the structures compound");
                    let refs = structures
                        .get_compound("References")
                        .expect("structures.References present");
                    assert!(
                        refs.tags.is_empty(),
                        "clean spawn has no structure references"
                    );
                    let starts = structures
                        .get_compound("starts")
                        .expect("structures.starts present");
                    assert!(
                        starts.tags.is_empty(),
                        "clean spawn has no structure starts"
                    );
                }
                "mineshaft-structure-refs" => {
                    let structures = root.get_compound("structures").expect("structures present");
                    let refs = structures
                        .get_compound("References")
                        .expect("References present");
                    assert!(
                        refs.contains("minecraft:mineshaft"),
                        "chunk 0.-4 carries a mineshaft structure reference"
                    );
                }
                "fluid-ticks" => {
                    assert_eq!(
                        ticks_len("fluid_ticks"),
                        1,
                        "fluid-ticks chunk has one saved fluid tick"
                    );
                }
                "block-ticks" => {
                    assert_eq!(
                        ticks_len("block_ticks"),
                        1,
                        "block-ticks chunk has one saved block tick"
                    );
                }
                "chest-block-entity" => {
                    let bes = root.get("block_entities").expect("block_entities present");
                    let Tag::List(list) = bes else {
                        panic!("block_entities is a list in {}", cap["path"])
                    };
                    let mut chest = false;
                    for i in 0..list.size() {
                        if let Tag::Compound(be) = list.get(i)
                            && be.get_string("id").map(String::as_str) == Some("minecraft:chest")
                        {
                            chest = true;
                        }
                    }
                    assert!(
                        chest,
                        "chest-block-entity chunk contains a chest block entity"
                    );
                }
                other => panic!("unexpected role {other}"),
            }
        }
        assert_eq!(by_role.len(), 5, "all five corpus roles present");
    }

    /// The committed `fixtures/paper-global.yml` (the pinned Paper global config
    /// every oracle boot runs under) must set chunk-system to exactly 1 worker
    /// and 1 I/O thread (issue #266).
    #[test]
    fn pinned_global_config_is_serialized() {
        let f = fixtures_dir().join("paper-global.yml");
        if !f.is_file() {
            return;
        }
        let text = fs::read_to_string(&f).unwrap();
        assert!(
            text.contains("io-threads: 1"),
            "pinned paper-global.yml must set chunk-system.io-threads=1"
        );
        assert!(
            text.contains("worker-threads: 1"),
            "pinned paper-global.yml must set chunk-system.worker-threads=1"
        );
    }

    /// `is_region_capture` is authoritative by explicit `kind` (issue #266):
    /// `kind: "m2"` is the concurrency-sensitive normal-overworld region
    /// capture; `kind: "m0"` (and every other kind) is not. A kind-less
    /// manifest falls back to the strict string inference ONLY for the two
    /// committed manifests captured before the `kind` field existed.
    #[test]
    fn region_capture_discriminates_normal_overworld() {
        let normal = Manifest {
            format: 1,
            seed: Some("42".into()),
            level_type: Some("minecraft\\:normal".into()),
            paper: Some("26.2-DEV-main@0a99345".into()),
            chunk_count: Some(408),
            region_file_compression: Some("none".into()),
            kind: None,
            chunk_concurrency: None,
            captured: Vec::new(),
        };

        // Kind-less backward-compat fallback: normal + none + chunks is the M2
        // capture (the committed pre-kind region manifest's exact shape).
        assert!(
            is_region_capture(&normal),
            "kind-less normal + none + chunks is the M2 capture"
        );
        // M0 superflat slice (kind-less): same compression + chunk count, but
        // flat level type — never a region capture.
        let m0 = Manifest {
            level_type: Some("minecraft\\:flat".into()),
            chunk_count: Some(432),
            ..normal.clone()
        };
        assert!(
            !is_region_capture(&m0),
            "M0 superflat is not a region capture"
        );
        // Non-none compression or zero chunks are not a region capture either.
        let deflate = Manifest {
            region_file_compression: Some("deflate".into()),
            ..normal.clone()
        };
        assert!(!is_region_capture(&deflate));
        let empty = Manifest {
            chunk_count: Some(0),
            ..normal
        };
        assert!(!is_region_capture(&empty));
    }

    /// The explicit `kind` is authoritative over the inferred strings: a
    /// `kind: "m2"` manifest is a region capture even if its level-type/compression
    /// strings look like the M0 superflat slice, and a `kind: "m0"` manifest is
    /// NOT a region capture even with the normal-overworld strings. Regeneration
    /// stamps these, so classification never depends on the config strings.
    #[test]
    fn region_capture_kind_is_authoritative_over_strings() {
        let base = Manifest {
            format: 1,
            seed: Some("42".into()),
            level_type: Some("minecraft\\:normal".into()),
            paper: Some("26.2-DEV-main@0a99345".into()),
            chunk_count: Some(408),
            region_file_compression: Some("none".into()),
            kind: None,
            chunk_concurrency: None,
            captured: Vec::new(),
        };

        // Mutation: same strings, kind flipped — classification must flip with
        // the kind, never with the strings.
        let m2_kind = Manifest {
            kind: Some(KIND_M2.into()),
            level_type: Some("minecraft\\:flat".into()),
            ..base.clone()
        };
        assert!(
            is_region_capture(&m2_kind),
            "kind: m2 is a region capture regardless of the level-type string"
        );

        let m0_kind = Manifest {
            kind: Some(KIND_M0.into()),
            ..base
        };
        assert!(
            !is_region_capture(&m0_kind),
            "kind: m0 is never a region capture even with normal+none+chunks"
        );
    }

    /// Bare `regenerate` intentionally regenerates every fixture kind —
    /// including the superflat status-FULL capture (issue #51), the derived
    /// worldgen samples, and the text corpus (issue #98). Each `--flag`
    /// sub-selects only its own kind. This pins the selection semantics so a
    /// future change cannot silently drop (or add) a kind from the bare
    /// invocation.
    #[test]
    fn regenerate_selection_includes_text_on_bare() {
        assert!(regenerates_kind("--m0", &[]));
        assert!(regenerates_kind("--m2", &[]));
        assert!(regenerates_kind("--full", &[]));
        assert!(regenerates_kind("--samples", &[]));
        assert!(regenerates_kind("--text", &[]));
        assert!(regenerates_kind("--m0", &["--m0"]));
        assert!(!regenerates_kind("--m0", &["--text"]));
        assert!(regenerates_kind("--full", &["--full"]));
        assert!(!regenerates_kind("--full", &["--m0"]));
        assert!(regenerates_kind("--text", &["--text"]));
        assert!(!regenerates_kind("--text", &["--m0"]));
        assert!(!regenerates_kind("--samples", &["--m0"]));
    }

    /// A freshly regenerated M0 manifest has the exact shape the gate must
    /// accept: `kind: "m0"` (stamped by the regenerate path), `region-file-compression=none`
    /// (emitted by extract_fixtures.py from the M0 props), `chunk-count`, and
    /// NO `chunk-concurrency` field (the M0 path never injects it).
    /// `verify_fixtures` must accept it — this is the documented `regenerate
    /// --m0` refresh path (issue #266).
    #[test]
    fn regenerated_m0_manifest_verifies_without_provenance() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-m0shape-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        fs::write(
            scratch.join("manifest.json"),
            serde_json::json!({
                "format": 1,
                "paper": "26.2-DEV-main@0a99345",
                "seed": "42",
                "kind": KIND_M0,
                "level-type": "minecraft\\:flat",
                "level-name": "world",
                "region-file-compression": "none",
                "spawn-region": "0.0",
                "chunk-count": 432,
                "captured": []
            })
            .to_string(),
        )
        .unwrap();
        // No captured files means only the structural checks run: the format,
        // the hashes (none), and the chunk-concurrency provenance gating. A
        // kind: m0 slice must NOT be treated as a region capture, so it verifies
        // without chunk-concurrency provenance.
        verify_fixtures(&scratch).expect("regenerated-M0-shaped manifest must verify clean");
        let _ = fs::remove_dir_all(&scratch);
    }

    /// A regenerated M2 manifest (`kind: "m2"`) MUST carry the pinned
    /// chunk-concurrency provenance — stripping it (or drifting it) fails
    /// verification even though the kind alone marks it a region capture. M2
    /// always requires provenance; M0 never does.
    #[test]
    fn regenerated_m2_manifest_requires_provenance() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-m2shape-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        let write_manifest = |cc: Option<serde_json::Value>, path: &Path| {
            let mut m = serde_json::json!({
                "format": 1,
                "paper": "26.2-DEV-main@0a99345",
                "seed": "42",
                "kind": KIND_M2,
                "level-type": "minecraft\\:normal",
                "level-name": "world",
                "region-file-compression": "none",
                "spawn-region": "0.0",
                "chunk-count": 408,
                "captured": []
            });
            if let Some(cc) = cc {
                m["chunk-concurrency"] = cc;
            }
            fs::write(
                path.join("manifest.json"),
                serde_json::to_string_pretty(&m).unwrap(),
            )
            .unwrap();
        };

        // Missing provenance on a kind: m2 manifest is a hard failure.
        write_manifest(None, &scratch);
        match verify_fixtures(&scratch) {
            Err(Error::Manifest(m)) => {
                assert!(
                    m.contains("chunk-concurrency"),
                    "message names the missing provenance: {m}"
                );
            }
            other => panic!("expected Manifest error, got {other:?}"),
        }

        // Drifted (non-pinned) provenance is a hard failure too.
        write_manifest(
            Some(serde_json::json!({"worker-threads": 3, "io-threads": 1})),
            &scratch,
        );
        match verify_fixtures(&scratch) {
            Err(Error::Manifest(m)) => {
                assert!(m.contains("3/1"), "message names the drifted counts: {m}");
            }
            other => panic!("expected Manifest error, got {other:?}"),
        }

        // Pinned provenance verifies clean.
        write_manifest(
            Some(serde_json::json!({"worker-threads": 1, "io-threads": 1})),
            &scratch,
        );
        verify_fixtures(&scratch).expect("kind: m2 with pinned provenance must verify clean");
        let _ = fs::remove_dir_all(&scratch);
    }

    /// `regenerate --to <dir>` must refuse every selection that is not exactly
    /// one of the *booting* kinds: bare, multi-kind, a derived kind on its own,
    /// and — critically — a booting kind mixed with a derived kind (e.g. `--m0
    /// --text --to` would silently rewrite the committed `fixtures/text` golden
    /// while the M0 slice went to the scratch dir). A shared destination across
    /// the booting kinds would also misbehave (M2's twin-boot replaces the whole
    /// directory, discarding M0's output). Each refusal happens before any boot,
    /// so no Paper jar is needed.
    #[test]
    fn regenerate_to_requires_single_booting_kind() {
        let to = Some(Path::new("/tmp/rivet-oracle-refused"));
        for (flags, needle) in [
            (&[][..], "all kinds (bare regenerate)"),
            (&["--m0", "--m2"][..], "--m0 --m2"),
            (&["--samples"][..], "ignore --to"),
            (&["--text"][..], "ignore --to"),
            (&["--m0", "--samples"][..], "--m0 --samples"),
            (&["--m0", "--text"][..], "--m0 --text"),
            (&["--m2", "--samples"][..], "--m2 --samples"),
            (&["--m2", "--text"][..], "--m2 --text"),
            (&["--full", "--samples"][..], "--full --samples"),
            (&["--full", "--text"][..], "--full --text"),
            (&["--m0", "--m2", "--samples"][..], "--m0 --m2 --samples"),
            (&["--m0", "--m2", "--text"][..], "--m0 --m2 --text"),
            (&["--m0", "--full", "--text"][..], "--m0 --full --text"),
        ] {
            assert!(
                !to_targets_single_booting_kind(flags),
                "--to must be refused for {flags:?}"
            );
            match run_regenerate(flags, to) {
                Err(Error::Gate(m)) => assert!(
                    m.contains(needle),
                    "refusal must name the offending selection; got: {m}"
                ),
                other => panic!("expected Gate refusal for {flags:?}, got {other:?}"),
            }
        }
        // Exactly one booting kind is the only legal `--to` target.
        assert!(to_targets_single_booting_kind(&["--m0"]));
        assert!(to_targets_single_booting_kind(&["--m2"]));
        assert!(to_targets_single_booting_kind(&["--full"]));
    }

    /// The committed `fixtures/paper-world-defaults.yml` (the pinned Paper
    /// world-defaults every oracle boot runs under) must cap every
    /// `entities.spawning.spawn-limits.*` category at 0 (issue #266). MC 26.2
    /// removed the vanilla spawn-monsters/animals/npcs server.properties keys
    /// (DedicatedServerProperties reads none of them), so these spawn-limits
    /// are the effective no-entity-spawn switch.
    #[test]
    fn pinned_world_defaults_caps_all_spawn_limits() {
        let f = fixtures_dir().join("paper-world-defaults.yml");
        if !f.is_file() {
            return;
        }
        let text = fs::read_to_string(&f).unwrap();
        for category in [
            "ambient",
            "axolotls",
            "creature",
            "monster",
            "underground_water_creature",
            "water_ambient",
            "water_creature",
        ] {
            let needle = format!("{category}: 0");
            assert!(
                text.contains(&needle),
                "pinned paper-world-defaults.yml must cap spawn-limit {category} at 0"
            );
        }
    }

    /// The committed boot-config properties fixtures must NOT carry the
    /// ineffective vanilla spawn-animals/monsters/npcs keys (issue #266): MC
    /// 26.2 removed them from DedicatedServerProperties, so writing them is a
    /// no-op and an overclaim. The effective no-entity-spawn suppression is the
    /// pinned paper-world-defaults.yml spawn-limits (asserted above).
    #[test]
    fn boot_config_fixtures_have_no_vanilla_spawn_keys() {
        for props in ["server.properties", "server-normal.properties"] {
            let f = fixtures_dir().join(props);
            if !f.is_file() {
                continue;
            }
            let text = fs::read_to_string(&f).unwrap();
            for key in ["spawn-animals", "spawn-monsters", "spawn-npcs"] {
                assert!(
                    !text.lines().any(|l| {
                        let l = l.trim();
                        l.starts_with(key) && l.contains('=')
                    }),
                    "{props} must not set the removed no-op key {key} (issue #266)"
                );
            }
        }
    }

    /// Runtime evidence that entity spawning is suppressed: the captured chunk
    /// NBT payloads (M0 superflat and M2 normal-overworld) contain no entity
    /// data at all — no `Entities` list tag anywhere in any captured chunk.
    /// Combined with the spawn-limits-0 world-defaults assertion, this is the
    /// "assert from effective generated config or runtime evidence" guarantee
    /// (issue #266). Twin-boot byte identity and the strict verify diff remain
    /// authoritative even if suppression ever fails.
    #[test]
    fn captured_chunk_payloads_have_no_entities() {
        for root in [
            fixtures_dir().join("chunk"),
            fixtures_dir().join("regions/overworld-normal/chunk"),
        ] {
            if !root.is_dir() {
                continue;
            }
            let mut n = 0;
            let mut walk = vec![root.clone()];
            while let Some(dir) = walk.pop() {
                for entry in fs::read_dir(&dir).unwrap().flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        walk.push(p);
                    } else if p.extension().is_some_and(|e| e == "nbt") {
                        let data = fs::read(&p).unwrap();
                        // NBT is a binary tag tree; the 'Entities' key name is
                        // present verbatim if any entity list tag was written.
                        assert!(
                            !data.windows(b"Entities".len()).any(|w| w == b"Entities"),
                            "captured chunk {} contains an Entities tag — entity \
                             spawning was NOT suppressed (issue #266)",
                            p.display()
                        );
                        n += 1;
                    }
                }
            }
            assert!(n > 0, "no chunk payloads found under {}", root.display());
        }
    }

    /// `prepare_run_dir` must copy BOTH pinned config files into the run dir's
    /// `config/` before every boot: paper-global.yml (chunk-system 1/1, #266)
    /// and paper-world-defaults.yml (spawn-limits 0, #266). A boot that misses
    /// either is not byte-deterministic.
    #[test]
    fn prepare_run_dir_installs_pinned_configs() {
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-prepare-{}", std::process::id()));
        let run_dir = scratch.join("run");
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        let props = fixtures_dir().join("server.properties");
        prepare_run_dir(&run_dir, &props).expect("prepare_run_dir must succeed");

        let config = run_dir.join("config");
        assert!(
            config.join("paper-global.yml").is_file(),
            "paper-global.yml must be copied into the run config"
        );
        assert!(
            config.join("paper-world-defaults.yml").is_file(),
            "paper-world-defaults.yml must be copied into the run config"
        );
        // The copied files are byte-identical to the committed fixtures.
        assert_eq!(
            fs::read(config.join("paper-global.yml")).unwrap(),
            fs::read(fixtures_dir().join("paper-global.yml")).unwrap()
        );
        assert_eq!(
            fs::read(config.join("paper-world-defaults.yml")).unwrap(),
            fs::read(fixtures_dir().join("paper-world-defaults.yml")).unwrap()
        );
        let _ = fs::remove_dir_all(&scratch);
    }

    /// Parsing the exact Moonrise log line yields the pinned (1, 1) counts.
    #[test]
    fn parse_boot_thread_counts_accepts_pinned() {
        let log =
            "[02:08:08 INFO]: [MoonriseCommon] Paper is using 1 worker threads, 1 I/O threads\n";
        assert_eq!(parse_boot_thread_counts(log), Some((1, 1)));
    }

    /// A boot log reporting more than one worker (the pre-pin default) is the
    /// "wrong config" case — parsed as-is so the caller can reject it.
    #[test]
    fn parse_boot_thread_counts_reports_off_pin() {
        let log =
            "[01:05:38 INFO]: [MoonriseCommon] Paper is using 3 worker threads, 1 I/O threads\n";
        assert_eq!(parse_boot_thread_counts(log), Some((3, 1)));
    }

    /// A log with no Moonrise thread line is the "config missing/ineffective"
    /// case — parsed as None so the caller rejects it as unconfirmed.
    #[test]
    fn parse_boot_thread_counts_missing_is_none() {
        assert_eq!(parse_boot_thread_counts("no thread line here\n"), None);
        assert_eq!(parse_boot_thread_counts(""), None);
    }

    /// Two pin lines in one log is ambiguous — refuse to guess.
    #[test]
    fn parse_boot_thread_counts_ambiguous_is_none() {
        let log = "\
[1] Paper is using 1 worker threads, 1 I/O threads
[2] Paper is using 1 worker threads, 1 I/O threads
";
        assert_eq!(parse_boot_thread_counts(log), None);
    }

    /// `check_boot_thread_pin` accepts exactly 1 worker / 1 I/O thread.
    #[test]
    fn boot_thread_pin_accepts_pinned() {
        let log =
            "[02:08:08 INFO]: [MoonriseCommon] Paper is using 1 worker threads, 1 I/O threads\n";
        check_boot_thread_pin(log).expect("pinned log must pass the pin check");
    }

    /// `check_boot_thread_pin` rejects a boot that ran with more threads
    /// (ineffective or overridden pin) — a hard failure, never a skip.
    #[test]
    fn boot_thread_pin_rejects_off_pin() {
        let log =
            "[01:05:38 INFO]: [MoonriseCommon] Paper is using 4 worker threads, 2 I/O threads\n";
        match check_boot_thread_pin(log) {
            Err(Error::Gate(m)) => {
                assert!(
                    m.contains("4 worker threads"),
                    "message names the observed counts: {m}"
                );
            }
            other => panic!("expected Gate error, got {other:?}"),
        }
    }

    /// `check_boot_thread_pin` rejects a boot whose log has no thread line at
    /// all (missing/ineffective config) — the loud-failure guarantee.
    #[test]
    fn boot_thread_pin_rejects_missing_line() {
        match check_boot_thread_pin("Done (...)!") {
            Err(Error::Gate(m)) => {
                assert!(
                    m.contains("no Moonrise"),
                    "message explains the missing pin: {m}"
                );
            }
            other => panic!("expected Gate error, got {other:?}"),
        }
    }

    /// A region-capture manifest with MISSING concurrency provenance fails
    /// static verification — never a silent pass.
    #[test]
    fn region_manifest_requires_concurrency_provenance() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        // Copy the committed region fixtures to scratch and strip the
        // chunk-concurrency field, then verify must reject the drift.
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-provenance-{}", std::process::id()));
        copy_dir_recursive(&dir, &scratch).unwrap();
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(scratch.join("manifest.json")).unwrap())
                .unwrap();
        v.as_object_mut().unwrap().remove("chunk-concurrency");
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string_pretty(&v).unwrap(),
        )
        .unwrap();
        match verify_fixtures(&scratch) {
            Err(Error::Manifest(m)) => {
                assert!(
                    m.contains("chunk-concurrency"),
                    "message names the missing provenance: {m}"
                );
            }
            other => panic!("expected Manifest error, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&scratch);
    }

    /// A region-capture manifest with WRONG (non-1/1) concurrency provenance
    /// fails static verification — drift is detected, never accepted.
    #[test]
    fn region_manifest_rejects_wrong_provenance() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-provenance2-{}", std::process::id()));
        copy_dir_recursive(&dir, &scratch).unwrap();
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(scratch.join("manifest.json")).unwrap())
                .unwrap();
        v["chunk-concurrency"] = serde_json::json!({
            "worker-threads": 3,
            "io-threads": 1,
        });
        fs::write(
            scratch.join("manifest.json"),
            serde_json::to_string_pretty(&v).unwrap(),
        )
        .unwrap();
        match verify_fixtures(&scratch) {
            Err(Error::Manifest(m)) => {
                assert!(m.contains("3/1"), "message names the drifted counts: {m}");
            }
            other => panic!("expected Manifest error, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&scratch);
    }

    /// Provenance drift: a baseline recorded under 1/1 but a boot that ran 3/1
    /// is caught and named by `check_concurrency_provenance`.
    #[test]
    fn concurrency_provenance_detects_run_drift() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let log =
            std::env::temp_dir().join(format!("rivet-oracle-drift-{}.log", std::process::id()));
        fs::write(
            &log,
            "[01:05:38 INFO]: Paper is using 3 worker threads, 1 I/O threads\n",
        )
        .unwrap();
        match check_concurrency_provenance(&dir, &log) {
            Err(Error::Gate(m)) => {
                assert!(
                    m.contains("provenance drift"),
                    "message names the drift: {m}"
                );
            }
            other => panic!("expected Gate error, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&log);
    }

    /// Provenance match: a baseline and boot both under 1/1 is accepted.
    #[test]
    fn concurrency_provenance_accepts_match() {
        let dir = fixtures_dir().join("regions/overworld-normal");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let log =
            std::env::temp_dir().join(format!("rivet-oracle-match-{}.log", std::process::id()));
        fs::write(
            &log,
            "[02:08:08 INFO]: Paper is using 1 worker threads, 1 I/O threads\n",
        )
        .unwrap();
        check_concurrency_provenance(&dir, &log).expect("matching provenance must pass");
        let _ = fs::remove_dir_all(&log);
    }

    /// Twin-boot byte-identity: identical trees are identical, and a single
    /// flipped chunk byte is detected (the regeneration never commits a
    /// nondeterministic pair).
    #[test]
    fn trees_byte_identical_detects_twin_boot_mismatch() {
        let a = std::env::temp_dir().join(format!("rivet-oracle-tree-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("rivet-oracle-tree-b-{}", std::process::id()));
        for d in [&a, &b] {
            fs::create_dir_all(d.join("chunk/overworld/0.0")).unwrap();
            fs::write(d.join("manifest.json"), b"{\"format\":1}").unwrap();
            fs::write(d.join("chunk/overworld/0.0/0.0.nbt"), b"payload").unwrap();
        }
        assert!(
            trees_byte_identical(&a, &b).expect("identical trees compare clean"),
            "identical trees must be byte-identical"
        );

        // Flip a byte in B's chunk — the pair is now different.
        let flipped = b.join("chunk/overworld/0.0/0.0.nbt");
        fs::write(&flipped, b"payloadX").unwrap();
        assert!(
            !trees_byte_identical(&a, &b).expect("comparison runs"),
            "a differing chunk must be detected"
        );

        // Missing file in B is also a mismatch.
        fs::write(&flipped, b"payload").unwrap();
        fs::remove_file(b.join("chunk/overworld/0.0/0.0.nbt")).unwrap();
        assert!(
            !trees_byte_identical(&a, &b).expect("comparison runs"),
            "a missing chunk must be detected"
        );

        for d in [&a, &b] {
            let _ = fs::remove_dir_all(d);
        }
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

    /// The committed `fixtures/text/` corpus (issue #98) must verify clean, carry
    /// the pinned Paper provenance, and capture exactly the two committed files.
    #[test]
    fn text_manifest_verify() {
        let dir = fixtures_dir().join("text");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let manifest = verify_fixtures(&dir).expect("text fixtures should match manifest");
        assert_eq!(manifest.kind.as_deref(), Some("text"));
        assert_eq!(
            parse_paper_pin(manifest.paper.as_deref()),
            Some("0a99345".into())
        );
        let mut names: Vec<&str> = manifest.captured.iter().map(|c| c.path.as_str()).collect();
        names.sort();
        assert_eq!(names, ["corpus.json", "golden.json"]);
    }

    /// The corpus must be non-vacuous and representative: enough entries to
    /// exercise the chat/title/player-info/scoreboard paths, with both accepted
    /// components and strict malformed (reject) fixtures present. A corpus that
    /// silently shrinks to nothing is a failed fixture.
    #[test]
    fn text_corpus_is_non_vacuous() {
        let corpus = fixtures_dir().join("text").join("corpus.json");
        if !corpus.is_file() {
            return;
        }
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&corpus).unwrap()).unwrap();
        let entries = v["entries"].as_array().expect("entries is a list");
        assert!(
            entries.len() >= 50,
            "corpus must be substantial, got {}",
            entries.len()
        );
        let accepts = entries
            .iter()
            .filter(|e| e["accept"] == serde_json::Value::Bool(true))
            .count();
        let rejects = entries
            .iter()
            .filter(|e| e["accept"] == serde_json::Value::Bool(false))
            .count();
        assert!(
            accepts >= 30,
            "corpus must exercise accepted components, got {accepts}"
        );
        assert!(
            rejects >= 5,
            "corpus must include strict malformed fixtures, got {rejects}"
        );
        // Every entry has the required shape.
        for e in entries {
            assert!(e.get("id").is_some(), "entry lacks an id");
            let input = e.get("input").and_then(|i| i.as_str());
            assert!(
                input.is_some() && !input.unwrap().is_empty(),
                "entry {} input must be a non-empty JSON string",
                e["id"]
            );
        }
    }

    /// The corpus `accept` verdicts and the golden (Paper's real verdicts) must
    /// agree entry-for-entry — the committed corpus never lies about what Paper
    /// accepted or rejected (issue #98 byte/JSON identity without normalization).
    #[test]
    fn text_corpus_accept_matches_golden() {
        let dir = fixtures_dir().join("text");
        let corpus = dir.join("corpus.json");
        let golden = dir.join("golden.json");
        if !corpus.is_file() || !golden.is_file() {
            return;
        }
        let cv: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&corpus).unwrap()).unwrap();
        let gv: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&golden).unwrap()).unwrap();
        let mut corpus_by_id: std::collections::BTreeMap<&str, bool> = cv["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["id"].as_str().unwrap(), e["accept"].as_bool().unwrap()))
            .collect();
        assert_eq!(
            corpus_by_id.len(),
            cv["entries"].as_array().unwrap().len(),
            "duplicate corpus ids"
        );
        let golden_entries = gv["entries"].as_array().unwrap();
        assert_eq!(
            corpus_by_id.len(),
            golden_entries.len(),
            "golden must cover every corpus entry"
        );
        for ge in golden_entries {
            let id = ge["id"].as_str().unwrap();
            let accept = ge["accept"].as_bool().unwrap();
            assert_eq!(
                corpus_by_id.remove(id),
                Some(accept),
                "corpus accept disagrees with Paper golden for {id}"
            );
            if accept {
                assert!(
                    ge.get("canonical").is_some(),
                    "accepted entry {id} must record Paper's canonical JSON"
                );
            }
        }
        assert!(corpus_by_id.is_empty(), "golden missed corpus entries");
    }

    /// The committed `manifest.json` is exactly what the manifest writer
    /// produces over the committed `corpus.json` + `golden.json`: re-running
    /// the hashing reproduces it byte-for-byte (git-clean) and the regenerated
    /// manifest verifies its own files. This proves the *writer* is
    /// deterministic over fixed inputs — it does not boot Paper a second time,
    /// so it is not a twin-boot proof of `golden.json` (see `regenerate --m2`).
    #[test]
    fn text_manifest_regeneration_is_byte_identical() {
        let dir = fixtures_dir().join("text");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("rivet-oracle-text-regen-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        fs::create_dir_all(&scratch).unwrap();
        for name in ["corpus.json", "golden.json"] {
            fs::copy(dir.join(name), scratch.join(name)).unwrap();
        }
        regenerate_text_manifest(&scratch).unwrap();
        let committed = fs::read(dir.join("manifest.json")).unwrap();
        let regenerated = fs::read(scratch.join("manifest.json")).unwrap();
        assert_eq!(
            committed, regenerated,
            "regenerating the text manifest must be byte-identical (git-clean)"
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

    // ---- #54 chunk-hash engine ----------------------------------------------
    //
    // These build synthetic Paper/Rivet fixture trees from
    // `mutate::fixture_full_payload` (a deterministic FULL Level payload) so the
    // hash-diff scenarios can be exercised without committing thousands of NBT
    // blobs. The synthetic trees cover the full corpus coordinate matrix, so the
    // required-corpus-coordinate UNVERIFIED guard is satisfied — which the
    // committed live capture (only (0,0) FULL) cannot do, pre-worldgen.

    /// Write a chunk-hash fixture tree: FULL payloads for the given coordinates
    /// under `chunk/the_nether/0.0/`, plus the serialized `HashManifest`. The
    /// tree is exactly what `run_hash_paper`/`hash-rivet` produce from a real
    /// region capture, so `run_hash_diff` treats it identically.
    fn write_hash_fixture_tree(root: &Path, coords: &[(i32, i32)]) -> PathBuf {
        write_hash_fixture_tree_seeded(root, coords, 42)
    }

    /// Like `write_hash_fixture_tree`, but the payloads carry the given world
    /// seed into their block content (`fixture_full_payload_with_seed`), and the
    /// manifest records that seed — so a tree under a different seed is a
    /// genuinely different world, which is the #175 7(e) bogus-seed mechanism.
    fn write_hash_fixture_tree_seeded(root: &Path, coords: &[(i32, i32)], seed: i64) -> PathBuf {
        let chunk_dir = root.join("chunk").join("the_nether").join("0.0");
        fs::create_dir_all(&chunk_dir).unwrap();
        for (cx, cz) in coords {
            let bytes = crate::mutate::fixture_full_payload_with_seed(*cx, *cz, seed);
            fs::write(chunk_dir.join(format!("{cx}.{cz}.nbt")), bytes).unwrap();
        }
        let manifest =
            hash_manifest::build_from_payloads(root, &seed.to_string(), "minecraft\\:normal")
                .unwrap();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(root.join("manifest.json"), json + "\n").unwrap();
        root.to_path_buf()
    }

    /// The full corpus coordinate matrix (every seed × coordinate pair a green
    /// sweep must cover), as a flat coordinate list.
    fn all_corpus_coordinates() -> Vec<(i32, i32)> {
        corpus::COORDINATES.to_vec()
    }

    fn hash_tmp(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rivet-oracle-{prefix}-{}", std::process::id()))
    }

    fn paper_rivet_dirs(
        tmp: &Path,
        paper_extra: &[(i32, i32)],
        rivet_extra: &[(i32, i32)],
    ) -> (PathBuf, PathBuf) {
        let mut paper_coords = all_corpus_coordinates();
        paper_coords.extend_from_slice(paper_extra);
        let mut rivet_coords = all_corpus_coordinates();
        rivet_coords.extend_from_slice(rivet_extra);
        let paper = write_hash_fixture_tree(&tmp.join("paper"), &paper_coords);
        let rivet = write_hash_fixture_tree(&tmp.join("rivet"), &rivet_coords);
        (paper, rivet)
    }

    fn hash_diff_args(paper: &Path, rivet: &Path) -> Vec<String> {
        vec![
            "hash-diff".to_string(),
            paper.to_string_lossy().into_owned(),
            rivet.to_string_lossy().into_owned(),
        ]
    }

    /// Two identical synthetic trees over the full corpus matrix: the diff is a
    /// genuine PASS (all 8 corpus coordinates FULL on both sides), and the CLI
    /// maps it to exit 0.
    #[test]
    fn hash_diff_green_pair_is_pass() {
        let tmp = hash_tmp("hash-green");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        assert!(run_hash_diff(&paper, &rivet).unwrap());
        assert_eq!(hash_diff_exit(&hash_diff_args(&paper, &rivet)), 0);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Flip one chunk's digest in the Rivet tree: the diff must name exactly
    /// that chunk, be a real worldgen difference (not order-only), and the CLI
    /// must map it to FAIL (1).
    #[test]
    fn hash_diff_names_tampered_chunk() {
        let tmp = hash_tmp("hash-flip");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        // Tamper the (15,15) block palette in the Rivet tree and rebuild its
        // manifest so it is a plausible-but-wrong baseline.
        let target = rivet.join("chunk/the_nether/0.0/15.15.nbt");
        let payload = fs::read(&target).unwrap();
        let tampered = crate::mutate::tamper(&payload, TamperKind::Block).unwrap();
        fs::write(&target, tampered).unwrap();
        let manifest =
            hash_manifest::build_from_payloads(&rivet, "42", "minecraft\\:normal").unwrap();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(rivet.join("manifest.json"), json + "\n").unwrap();

        let paper_m = load_hash_manifest(&paper).unwrap();
        let rivet_m = load_hash_manifest(&rivet).unwrap();
        let (mismatches, paper_only, rivet_only, compared) = compute_hash_diffs(&paper_m, &rivet_m);
        assert_eq!(
            compared,
            all_corpus_coordinates().len(),
            "every corpus chunk compared"
        );
        assert_eq!(mismatches.len(), 1, "exactly one chunk diverged");
        assert_eq!(mismatches[0].dim, "the_nether");
        assert_eq!((mismatches[0].cx, mismatches[0].cz), (15, 15));
        assert!(
            !mismatches[0].order_only,
            "block tamper is a real worldgen difference"
        );
        assert!(
            paper_only.is_empty() && rivet_only.is_empty(),
            "no one-sided chunks"
        );
        assert!(!run_hash_diff(&paper, &rivet).unwrap());
        assert_eq!(hash_diff_exit(&hash_diff_args(&paper, &rivet)), EXIT_FAIL);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// An NBT-order swap is serialization-order-only: the raw digest differs,
    /// the canonical digest does not. The diff reports it as order-only (triage)
    /// but still FAILs — order divergence is divergence.
    #[test]
    fn hash_diff_reports_order_only_triage_but_fails() {
        let tmp = hash_tmp("hash-order");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        let target = rivet.join("chunk/the_nether/0.0/31.31.nbt");
        let payload = fs::read(&target).unwrap();
        let tampered = crate::mutate::tamper(&payload, TamperKind::NbtOrder).unwrap();
        fs::write(&target, tampered).unwrap();
        let manifest =
            hash_manifest::build_from_payloads(&rivet, "42", "minecraft\\:normal").unwrap();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(rivet.join("manifest.json"), json + "\n").unwrap();

        let (mismatches, _, _, compared) = compute_hash_diffs(
            &load_hash_manifest(&paper).unwrap(),
            &load_hash_manifest(&rivet).unwrap(),
        );
        assert_eq!(compared, all_corpus_coordinates().len());
        assert_eq!(mismatches.len(), 1);
        assert_eq!((mismatches[0].cx, mismatches[0].cz), (31, 31));
        assert!(
            mismatches[0].order_only,
            "NBT-order swap must be flagged order-only (canonical-identical)"
        );
        assert!(!run_hash_diff(&paper, &rivet).unwrap());
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A Paper FULL chunk outside the corpus that Rivet does not produce is a
    /// one-sided divergence: the diff names it and FAILs (never vacuous green).
    /// The corpus-coordinate guard still passes because both trees cover the
    /// matrix.
    #[test]
    fn hash_diff_reports_paper_only_full_chunk() {
        let tmp = hash_tmp("hash-missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[(7, 7)], &[]);
        let (mismatches, paper_only, rivet_only, compared) = compute_hash_diffs(
            &load_hash_manifest(&paper).unwrap(),
            &load_hash_manifest(&rivet).unwrap(),
        );
        assert!(mismatches.is_empty(), "no digest mismatches");
        assert_eq!(compared, all_corpus_coordinates().len());
        assert_eq!(
            paper_only,
            vec!["the_nether/0.0.7.7".to_string()],
            "the Paper FULL chunk missing from Rivet is reported as PAPER-ONLY, not the reverse"
        );
        assert!(
            rivet_only.is_empty(),
            "Rivet produced no extra FULL chunk in this case"
        );
        assert!(!run_hash_diff(&paper, &rivet).unwrap());
        assert_eq!(hash_diff_exit(&hash_diff_args(&paper, &rivet)), EXIT_FAIL);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A Rivet-only FULL chunk (Rivet produced something Paper did not) is the
    /// reverse one-sided divergence and also FAILs.
    #[test]
    fn hash_diff_reports_rivet_only_full_chunk() {
        let tmp = hash_tmp("hash-extra");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[(7, 7)]);
        let (mismatches, paper_only, rivet_only, compared) = compute_hash_diffs(
            &load_hash_manifest(&paper).unwrap(),
            &load_hash_manifest(&rivet).unwrap(),
        );
        assert!(mismatches.is_empty());
        assert_eq!(compared, all_corpus_coordinates().len());
        assert!(
            paper_only.is_empty(),
            "Paper produced no extra FULL chunk in this case"
        );
        assert_eq!(
            rivet_only,
            vec!["the_nether/0.0.7.7".to_string()],
            "the Rivet FULL chunk Paper lacks is reported as RIVET-ONLY over-generation"
        );
        assert!(!run_hash_diff(&paper, &rivet).unwrap());
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The Paper-vs-Paper self-diff guard: passing the SAME tree as both paper
    /// and rivet compares Paper against itself and proves nothing about Rivet —
    /// it is UNVERIFIED (3), never a false PASS. This is the exact invocation the
    /// gate used to run (`hash-diff "$paper_dir" "$paper_dir"`).
    #[test]
    fn hash_diff_refuses_paper_self_diff() {
        let tmp = hash_tmp("hash-selfdiff");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, _) = paper_rivet_dirs(&tmp, &[], &[]);
        assert!(
            run_hash_diff(&paper, &paper).is_err(),
            "paper-vs-paper self-diff must be refused (UNVERIFIED)"
        );
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &paper)),
            EXIT_UNVERIFIED
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A symlink alias to the same tree is still a self-diff — the guard
    /// canonicalizes so an aliased path cannot sneak a Paper-vs-Paper comparison
    /// past it.
    #[test]
    fn hash_diff_refuses_symlinked_self_diff() {
        let tmp = hash_tmp("hash-selfdiff-link");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, _) = paper_rivet_dirs(&tmp, &[], &[]);
        #[cfg(unix)]
        {
            let alias = tmp.join("alias");
            std::os::unix::fs::symlink(&paper, &alias).unwrap();
            assert!(
                run_hash_diff(&paper, &alias).is_err(),
                "symlinked alias of the paper tree must be refused"
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A different seed in the Rivet manifest is provenance drift: the diff
    /// refuses to compare (UNVERIFIED, exit 3) rather than comparing digests
    /// that mean different worlds.
    #[test]
    fn hash_diff_refuses_provenance_mismatch() {
        let tmp = hash_tmp("hash-seed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        let mut m = load_hash_manifest(&rivet).unwrap();
        m.seed = "999999".to_string();
        let json = serde_json::to_string_pretty(&m).unwrap();
        fs::write(rivet.join("manifest.json"), json + "\n").unwrap();
        assert!(
            run_hash_diff(&paper, &rivet).is_err(),
            "provenance mismatch is UNVERIFIED"
        );
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &rivet)),
            EXIT_UNVERIFIED
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A Rivet dir with no manifest at all (the pre-worldgen reality: no Rivet
    /// chunk serialization yet) is UNVERIFIED, exit 3 — never a vacuous green.
    #[test]
    fn hash_diff_without_rivet_manifest_is_unverified() {
        let tmp = hash_tmp("hash-no-rivet");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, _) = paper_rivet_dirs(&tmp, &[], &[]);
        let empty = tmp.join("rivet-empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(run_hash_diff(&paper, &empty).is_err());
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &empty)),
            EXIT_UNVERIFIED
        );
        assert_eq!(
            hash_cli_exit(&[
                "hash-rivet".to_string(),
                empty.to_string_lossy().into_owned()
            ]),
            Some(EXIT_UNVERIFIED)
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The required-corpus-coordinate guard: if a corpus coordinate has no FULL
    /// data on both sides (as with the committed M2 capture, which is only
    /// (0,0)), the diff is UNVERIFIED (3), never green.
    #[test]
    fn hash_diff_unverified_when_corpus_coordinates_uncovered() {
        let tmp = hash_tmp("hash-partial");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        // Both sides have FULL data only at coordinate (0,0), like the committed
        // M2 capture — so 7 of the 8 required corpus coordinates lack FULL data
        // on both sides, reproduced synthetically.
        let paper = write_hash_fixture_tree(&tmp.join("paper"), &[(0, 0)]);
        let rivet = write_hash_fixture_tree(&tmp.join("rivet"), &[(0, 0)]);
        assert!(run_hash_diff(&paper, &rivet).is_err());
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &rivet)),
            EXIT_UNVERIFIED
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Status and structures mutations through the real verdict (issue #51
    /// component 7): a chunk whose `Status` is demoted, or whose FULL-time
    /// `structures` compound is removed, is no longer a *genuine* FULL payload.
    /// These cannot be a "plausible wrong digest" — the FULL gate refuses them
    /// before any xxh3_64 is compared: a demoted-status chunk stops being FULL
    /// (its corpus coordinate lacks FULL data on that side → UNVERIFIED, exit 3),
    /// and a structures-missing chunk fails `validate_full_payload` at manifest
    /// build (→ UNVERIFIED, exit 3). Neither can ever be a false green.
    #[test]
    fn hash_diff_refuses_status_and_structures_tamper() {
        let tmp = hash_tmp("hash-status-struct");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        let (cx, cz) = all_corpus_coordinates()[0];
        let target = paper
            .join("chunk/the_nether/0.0")
            .join(format!("{cx}.{cz}.nbt"));

        // Status demotion: rebuild the paper manifest with the (0,0) chunk no
        // longer FULL — the required-corpus guard sees no FULL data at (0,0) on
        // the paper side and the diff is UNVERIFIED (3), never green.
        let mut root = crate::mutate::parse_payload(&fs::read(&target).unwrap()).unwrap();
        root.put_string("Status", "minecraft:structure_starts");
        fs::write(&target, crate::mutate::encode_payload(&root).unwrap()).unwrap();
        let m = hash_manifest::build_from_payloads(&paper, "42", "minecraft\\:normal").unwrap();
        fs::write(
            paper.join("manifest.json"),
            serde_json::to_string_pretty(&m).unwrap() + "\n",
        )
        .unwrap();
        assert!(run_hash_diff(&paper, &rivet).is_err());
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &rivet)),
            EXIT_UNVERIFIED,
            "a demoted-status chunk must be refused, never compared as FULL"
        );

        // Structures removal: restore Status to full but drop the `structures`
        // compound — the FULL validator refuses the payload at manifest build, so
        // the comparator can never compare a chunk Paper did not actually finish.
        let mut root =
            crate::mutate::parse_payload(&crate::mutate::fixture_full_payload(cx, cz)).unwrap();
        root.tags.swap_remove("structures");
        fs::write(&target, crate::mutate::encode_payload(&root).unwrap()).unwrap();
        assert!(
            hash_manifest::build_from_payloads(&paper, "42", "minecraft\\:normal").is_err(),
            "FULL validator refuses a FULL chunk whose structures compound is missing"
        );

        // Section-local light tamper: restore the payload's shape but shrink a
        // section's `SkyLight` array below its 2048-byte packed size. The FULL
        // validator refuses malformed light data at manifest build (spec §5), so
        // the comparator never compares a chunk whose light was not finalized.
        let mut root =
            crate::mutate::parse_payload(&crate::mutate::fixture_full_payload(cx, cz)).unwrap();
        let sections = root.get_list_or_empty_mut("sections");
        if let rivet_nbt::tag::Tag::Compound(sec) = &mut sections.list[0] {
            sec.tags.insert(
                "SkyLight".to_string(),
                rivet_nbt::tag::Tag::ByteArray(rivet_nbt::byte_array_tag::ByteArrayTag::new(
                    vec![0i8; 10],
                )),
            );
        }
        fs::write(&target, crate::mutate::encode_payload(&root).unwrap()).unwrap();
        assert!(
            hash_manifest::build_from_payloads(&paper, "42", "minecraft\\:normal").is_err(),
            "FULL validator refuses a FULL chunk whose section light data is malformed"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// `verify_forced_load` is dimension-aware: it parses the per-dimension
    /// `Loading N persistent chunks for level 'minecraft:<dim>'` count and
    /// requires every ticket dimension to have loaded the forced corpus chunks.
    /// A create boot (0 loaded in every dimension), a partial injection (one
    /// dimension short), and a boot whose log line the parser cannot match are
    /// all refused loudly; a genuine capture boot passes.
    #[test]
    fn verify_forced_load_parses_per_dimension_counts() {
        let tmp = hash_tmp("forced-load");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let log = tmp.join("boot.log");

        // A genuine capture: all three dimensions loaded the 8 forced chunks.
        fs::write(
            &log,
            concat!(
                "[00:00:01 INFO]: Loading 8 persistent chunks for level 'minecraft:overworld'...\n",
                "[00:00:01 INFO]: Loading 8 persistent chunks for level 'minecraft:the_nether'...\n",
                "[00:00:01 INFO]: Loading 8 persistent chunks for level 'minecraft:the_end'...\n",
            ),
        )
        .unwrap();
        verify_forced_load(&log).expect("all dimensions loaded the forced chunks");

        // A create boot: zero persistent chunks everywhere — the injection never ran.
        fs::write(
            &log,
            concat!(
                "[00:00:01 INFO]: Loading 0 persistent chunks for level 'minecraft:overworld'...\n",
                "[00:00:01 INFO]: Loading 0 persistent chunks for level 'minecraft:the_nether'...\n",
                "[00:00:01 INFO]: Loading 0 persistent chunks for level 'minecraft:the_end'...\n",
            ),
        )
        .unwrap();
        assert!(
            verify_forced_load(&log).is_err(),
            "a spawn boot must be refused"
        );

        // A partial injection: the_nether never loaded its forced chunks.
        fs::write(
            &log,
            concat!(
                "[00:00:01 INFO]: Loading 8 persistent chunks for level 'minecraft:overworld'...\n",
                "[00:00:01 INFO]: Loading 0 persistent chunks for level 'minecraft:the_nether'...\n",
                "[00:00:01 INFO]: Loading 8 persistent chunks for level 'minecraft:the_end'...\n",
            ),
        )
        .unwrap();
        assert!(
            verify_forced_load(&log).is_err(),
            "a partial injection must be refused"
        );

        // A boot whose message the parser cannot match is refused (never silently
        // accepted as a forced capture).
        fs::write(&log, "[00:00:01 INFO]: Done (7.2s)!\n").unwrap();
        assert!(
            verify_forced_load(&log).is_err(),
            "an unmatchable log must be refused"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Bad argument counts map to usage (64) for every hash-* subcommand.
    #[test]
    fn hash_cli_usage_returns_64() {
        assert_eq!(hash_cli_exit(&["hash-diff".to_string()]), Some(EXIT_USAGE));
        assert_eq!(
            hash_cli_exit(&["hash-diff".to_string(), "only-one".to_string()]),
            Some(EXIT_USAGE)
        );
        assert_eq!(hash_cli_exit(&["hash-rivet".to_string()]), Some(EXIT_USAGE));
        assert_eq!(
            hash_cli_exit(&["hash-unknown".to_string()]),
            Some(EXIT_USAGE)
        );
        assert_eq!(
            hash_diff_exit(&[
                "hash-diff".to_string(),
                "--expect-fail".to_string(),
                "a".to_string(),
                "b".to_string(),
                "bogus".to_string()
            ]),
            EXIT_USAGE
        );
    }

    /// `hash-self-check` passes (0) — the pinned xxh3_64 known-answer vectors.
    #[test]
    fn hash_self_check_exits_zero() {
        assert_eq!(hash_cli_exit(&["hash-self-check".to_string()]), Some(0));
    }

    /// `extract-world` CLI usage errors (missing world dir, malformed `--to`)
    /// are Gate — never the missing-prerequisite UNVERIFIED the runner treats
    /// as "build it / point it at a real world first".
    #[test]
    fn extract_world_cli_usage_errors_are_gate_not_unverified() {
        assert!(matches!(parse_extract_world_args(&[]), Err(Error::Gate(_))));
        assert!(matches!(
            parse_extract_world_args(&["--to"]),
            Err(Error::Gate(_))
        ));
        assert!(matches!(
            parse_extract_world_args(&["--bogus", "world"]),
            Err(Error::Gate(_))
        ));
        assert!(matches!(
            parse_extract_world_args(&["world", "--bogus"]),
            Err(Error::Gate(_))
        ));
        let (dir, to) = parse_extract_world_args(&["world", "--to", "out.json"]).unwrap();
        assert_eq!(dir, PathBuf::from("world"));
        assert_eq!(to, Some(PathBuf::from("out.json")));
    }

    /// The `run()` exit-code mapping: only `Error::Unverified` (a missing
    /// prerequisite) exits 3; every internal/gate/io error is a hard FAIL (1).
    #[test]
    fn run_error_exit_codes_keep_unverified_distinct_from_fail() {
        assert_eq!(
            exit_code_for_run_error(&Error::Unverified("missing region layout".into())),
            EXIT_UNVERIFIED
        );
        assert_eq!(
            exit_code_for_run_error(&Error::Gate("malformed CLI".into())),
            EXIT_FAIL
        );
        assert_eq!(
            exit_code_for_run_error(&Error::Io(io::Error::other("io"))),
            EXIT_FAIL
        );
    }

    /// `extract-world` against a world with no overworld region layout reports
    /// `Error::Unverified` — which `main()` maps to exit 3 — so the runner
    /// classifies a missing world prerequisite as UNVERIFIED, never a bare FAIL.
    #[test]
    fn extract_world_missing_region_layout_is_unverified() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("level.dat"), b"not a world").unwrap();
        assert!(matches!(
            run_extract_world(dir.path(), None),
            Err(Error::Unverified(_))
        ));
    }

    /// The negative-control "names exactly the tampered chunk" predicate: the
    /// mismatch set must be non-empty AND contain only the tampered coordinate.
    /// A vacuous pass (empty) or a wrong-coordinate failure must both be caught,
    /// so "any diff failure" never satisfies the control.
    #[test]
    fn mismatch_set_names_exactly_requires_exact_tampered_chunk() {
        let t = |cx: i32, cz: i32| ChunkHashMismatch {
            dim: "overworld".to_string(),
            cx,
            cz,
            expected: "a".to_string(),
            actual: "b".to_string(),
            order_only: false,
        };
        // Exactly the tampered chunk: PASS.
        assert!(mismatch_set_names_exactly(&[t(7, 7)], "overworld", 7, 7));
        // Empty set (vacuously green): FAIL.
        assert!(!mismatch_set_names_exactly(&[], "overworld", 7, 7));
        // A different coordinate: FAIL.
        assert!(!mismatch_set_names_exactly(&[t(8, 8)], "overworld", 7, 7));
        // A different dimension: FAIL.
        let other_dim = ChunkHashMismatch {
            dim: "the_nether".to_string(),
            ..t(7, 7)
        };
        assert!(!mismatch_set_names_exactly(&[other_dim], "overworld", 7, 7));
        // The tampered chunk plus an unrelated one: FAIL (not *exactly*).
        assert!(!mismatch_set_names_exactly(
            &[t(7, 7), t(8, 8)],
            "overworld",
            7,
            7
        ));
    }

    /// The negative-control "tamper is the ONLY divergence" predicate: besides
    /// naming exactly the tampered chunk, a one-sided FULL divergence in either
    /// direction must also fail the control (the comparator is failing for a
    /// second, unrelated reason).
    #[test]
    fn tamper_divergence_is_exactly_rejects_one_sided_divergence() {
        let t = |cx: i32, cz: i32| ChunkHashMismatch {
            dim: "overworld".to_string(),
            cx,
            cz,
            expected: "a".to_string(),
            actual: "b".to_string(),
            order_only: false,
        };
        // Tamper only, no one-sided divergence: PASS.
        assert!(tamper_divergence_is_exactly(
            &[t(7, 7)],
            &[],
            &[],
            "overworld",
            7,
            7
        ));
        // A paper-only FULL divergence alongside the tamper: FAIL.
        assert!(!tamper_divergence_is_exactly(
            &[t(7, 7)],
            &["overworld/0.0.9.9".to_string()],
            &[],
            "overworld",
            7,
            7
        ));
        // A rivet-only FULL divergence alongside the tamper: FAIL.
        assert!(!tamper_divergence_is_exactly(
            &[t(7, 7)],
            &[],
            &["overworld/0.0.9.9".to_string()],
            "overworld",
            7,
            7
        ));
        // Empty digest mismatches with no one-sided divergence: FAIL (vacuous).
        assert!(!tamper_divergence_is_exactly(
            &[],
            &[],
            &[],
            "overworld",
            7,
            7
        ));
    }

    /// `hash-paper` against a missing payload dir is UNVERIFIED (3) — it must
    /// never write a zero-chunk manifest that a later diff could compare
    /// vacuously green. Both the direct runner and the CLI contract agree.
    #[test]
    fn hash_paper_missing_payload_dir_is_unverified() {
        let missing = hash_tmp("hash-paper-missing");
        let _ = fs::remove_dir_all(&missing);
        assert!(
            matches!(run_hash_paper(Some(&missing)), Err(Error::Unverified(_))),
            "a missing payload tree must be UNVERIFIED, never a zero-chunk manifest"
        );
        assert_eq!(
            hash_cli_exit(&[
                "hash-paper".to_string(),
                missing.to_string_lossy().into_owned()
            ]),
            Some(EXIT_UNVERIFIED)
        );
    }

    /// `hash-paper` against a dir with no `chunk/` payload tree is UNVERIFIED (3)
    /// — the same zero-chunk-manifest guard as the missing-dir case.
    #[test]
    fn hash_paper_empty_payload_dir_is_unverified() {
        let empty = hash_tmp("hash-paper-empty");
        let _ = fs::remove_dir_all(&empty);
        fs::create_dir_all(&empty).unwrap();
        assert!(
            matches!(run_hash_paper(Some(&empty)), Err(Error::Unverified(_))),
            "a payload tree with no chunk/ dir must be UNVERIFIED, never a zero-chunk manifest"
        );
        assert_eq!(
            hash_cli_exit(&[
                "hash-paper".to_string(),
                empty.to_string_lossy().into_owned()
            ]),
            Some(EXIT_UNVERIFIED)
        );
        let _ = fs::remove_dir_all(&empty);
    }

    /// The committed Paper hash manifest records the capture's working seed (42,
    /// read from `fixtures/regions/overworld-normal/manifest.json`), NOT one of
    /// the pinned corpus seeds. Its honest coverage is therefore 0 sweep cells
    /// (0/N present, all FULL entries outside the corpus) — a capture not
    /// generated under a corpus seed cannot claim any of the #175 sweep. The
    /// manifest is committed; if it is ever pruned this load-bearing honesty
    /// claim must FAIL, never silently skip.
    #[test]
    fn committed_paper_manifest_coverage_is_honest() {
        let dir = crate_dir().join("fixtures/chunk-hash/paper");
        if !dir.join("manifest.json").is_file() {
            panic!(
                "committed Paper hash manifest {} is ABSENT — the 0/N-coverage honesty \
                 claim is unverified; this test must FAIL, never silently skip",
                dir.join("manifest.json").display()
            );
        }
        let m = load_hash_manifest(&dir).unwrap();
        let cov = hash_manifest::coverage(&m, &corpus::Corpus::from_committed());
        assert_eq!(
            m.seed,
            hash_manifest::CAPTURE_SEED,
            "committed capture seed"
        );
        assert!(
            !corpus::corpus_seeds()
                .iter()
                .any(|s| s.to_string() == m.seed),
            "working seed 42 must not be a pinned corpus seed"
        );
        assert_eq!(
            cov.present, 0,
            "off-corpus-seed capture covers zero sweep cells"
        );
        assert_eq!(cov.expected, corpus::SEED_COUNT * corpus::COORDINATES.len());
        assert_eq!(
            cov.extra.len(),
            m.full_count,
            "every FULL chunk of the working-seed capture is outside the corpus"
        );
        assert!(
            !cov.is_complete(),
            "capture under working seed 42 is not complete"
        );
    }

    /// The `--expect-fail` negative control: tamper a copy of the baseline and
    /// require the diff to name it. On identical synthetic trees the control
    /// passes (exit 0) for a single kind and for `all`.
    #[test]
    fn hash_diff_expect_fail_detects_tamper() {
        // Distinct from `run_hash_diff_negative`'s scratch name
        // (`rivet-oracle-hash-neg-<pid>`) so the control's copy never deletes
        // the fixture trees under this tmp dir.
        let tmp = hash_tmp("hash-negtree");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let (paper, rivet) = paper_rivet_dirs(&tmp, &[], &[]);
        let mut args = vec!["hash-diff".to_string(), "--expect-fail".to_string()];
        args.push(paper.to_string_lossy().into_owned());
        args.push(rivet.to_string_lossy().into_owned());
        assert_eq!(hash_diff_exit(&args), 0, "block tamper detected and named");
        args.push("all".to_string());
        assert_eq!(
            hash_diff_exit(&args),
            0,
            "every TamperKind detected and named (no vacuous green)"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The #175 7(e) bogus-seed negative: a capture generated under a *different*
    /// seed hashes differently at every chunk. Because the two trees carry
    /// different seeds, the diff refuses to compare at all — provenance drift
    /// is UNVERIFIED (3), never a vacuous green or a misleading exit-1 digest
    /// comparison of two different worlds. Unlike the tamper negatives (which
    /// flip one field in a copy of the baseline), a bogus seed changes the
    /// *whole* tree, so this is a genuine different-world comparison; the
    /// every-chunk-differs claim is asserted on the payload level below.
    #[test]
    fn hash_diff_detects_bogus_seed() {
        let tmp = hash_tmp("hash-bogus-seed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        // Paper tree under the working seed 42; Rivet tree under a bogus seed.
        let paper = write_hash_fixture_tree(&tmp.join("paper"), &all_corpus_coordinates());
        let rivet =
            write_hash_fixture_tree_seeded(&tmp.join("rivet"), &all_corpus_coordinates(), 999);
        // The two trees carry different seeds — provenance drift, so the diff
        // refuses to compare (UNVERIFIED, 3): a different-seed capture is a
        // different world, and comparing its digests would be meaningless. The
        // error is asserted to be the provenance refusal itself, not just any
        // failure — an unrelated Err would not satisfy the stated intent.
        let err = run_hash_diff(&paper, &rivet).unwrap_err();
        assert!(
            err.to_string().contains("provenance"),
            "the diff must refuse on provenance drift, got: {err}"
        );
        assert_eq!(
            hash_diff_exit(&hash_diff_args(&paper, &rivet)),
            EXIT_UNVERIFIED
        );

        // The genuine #175 7(e) assertion is on the *payload* level: the two
        // worlds' serialized bytes differ at every chunk, so every digest
        // differs. That is what the seeded fixture builder reproduces
        // deterministically (asserted in mutate.rs too).
        let paper_m = load_hash_manifest(&paper).unwrap();
        let rivet_m = load_hash_manifest(&rivet).unwrap();
        assert!(
            paper_m.full_count > 0,
            "the baseline tree must carry FULL chunks for the every-chunk-differs claim"
        );
        let mut different = 0usize;
        for pe in paper_m.entries.iter().filter(|e| e.is_full()) {
            // Every paper FULL chunk must exist on the bogus-seed side too — a
            // missing counterpart is a hard failure, never a skipped comparison
            // that could silently shrink the every-chunk-differs claim.
            let re = rivet_m
                .full_entry(&pe.dim, pe.cx, pe.cz)
                .expect("bogus-seed tree has a FULL counterpart for every paper FULL chunk");
            if pe.xxh3_64 != re.xxh3_64 {
                different += 1;
            }
        }
        assert_eq!(
            different, paper_m.full_count,
            "every FULL chunk of the bogus-seed world differs from the baseline world"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The harder #175 7(e) case: a Rivet capture generated under a *bogus* seed
    /// whose manifest **lies** and claims the baseline seed — so the provenance
    /// guard passes (both sides claim seed 42) and only the digest comparison
    /// can catch the different world. This is the real threat an honest
    /// provenance check cannot stop (a capture mislabeling its own seed), and it
    /// is the path the honest-seed test above cannot exercise: there the diff
    /// bails on provenance before any digest is compared, here it must proceed
    /// to the digest level and FAIL (exit 1) naming every diverged chunk.
    #[test]
    fn hash_diff_detects_bogus_seed_with_lying_manifest() {
        let tmp = hash_tmp("hash-bogus-seed-lying");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let paper = write_hash_fixture_tree(&tmp.join("paper"), &all_corpus_coordinates());
        // Rivet payloads genuinely generated under a bogus seed 999...
        let rivet =
            write_hash_fixture_tree_seeded(&tmp.join("rivet"), &all_corpus_coordinates(), 999);
        // ...but the manifest lies and claims the baseline seed 42.
        let mut m = load_hash_manifest(&rivet).unwrap();
        m.seed = "42".to_string();
        fs::write(
            rivet.join("manifest.json"),
            serde_json::to_string_pretty(&m).unwrap() + "\n",
        )
        .unwrap();

        // Provenance now matches, so the diff must proceed to digest comparison
        // and FAIL — never a vacuous green.
        assert!(!run_hash_diff(&paper, &rivet).unwrap());
        assert_eq!(hash_diff_exit(&hash_diff_args(&paper, &rivet)), EXIT_FAIL);
        // Every corpus chunk diverged (the bogus-seed world differs at every
        // chunk) and no chunk is one-sided: the FAIL names all of them.
        let (mismatches, paper_only, rivet_only, compared) = compute_hash_diffs(
            &load_hash_manifest(&paper).unwrap(),
            &load_hash_manifest(&rivet).unwrap(),
        );
        assert_eq!(compared, all_corpus_coordinates().len());
        assert_eq!(
            mismatches.len(),
            all_corpus_coordinates().len(),
            "every FULL chunk of the lying-manifest bogus-seed world diverges"
        );
        assert!(
            paper_only.is_empty() && rivet_only.is_empty(),
            "no one-sided FULL divergence"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The committed Paper manifest's digest table is grounded in the exact
    /// payload bytes under `fixtures/regions/overworld-normal`: rebuilding the
    /// manifest from those payloads reproduces the committed digest table
    /// byte-identically. This pins the #175 §4 digest scope (the serialized
    /// `Level` compound written by `SerializableChunkData.write()`, region
    /// framing excluded) to the actual committed bytes, and guards against a
    /// future hashing change silently retargeting every digest.
    ///
    /// This test is *load-bearing*: the committed Paper digest table and the
    /// region payloads it is grounded in are committed deliverables, so their
    /// absence is a hard failure (panic), never a silent skip — matching the
    /// merged `committed_region_payloads_stamp_true_full_counts` convention
    /// (D8: a missing load-bearing fixture is a red test, never a skip).
    #[test]
    fn committed_paper_manifest_digests_ground_in_payload_bytes() {
        let dir = crate_dir().join("fixtures/chunk-hash/paper");
        assert!(
            dir.join("manifest.json").is_file(),
            "committed Paper digest table {} is ABSENT — the #54 digest-scope grounding \
             guard cannot verify; restore it (git checkout) or this test is red, never \
             silently skipped",
            dir.display()
        );
        let committed = load_hash_manifest(&dir).unwrap();
        let payload_dir = crate_dir().join("fixtures/regions/overworld-normal");
        assert!(
            payload_dir.join("chunk").is_dir(),
            "committed region payloads {} are ABSENT — the #54 digest-scope grounding guard \
             cannot verify; restore them (git checkout) or this test is red, never silently \
             skipped",
            payload_dir.display()
        );
        let seed = source_region_seed(&payload_dir)
            .unwrap_or_else(|| hash_manifest::CAPTURE_SEED.to_string());
        let rebuilt = hash_manifest::build_from_payloads(&payload_dir, &seed, "minecraft\\:normal")
            .expect("rebuild from committed payloads");
        assert_eq!(rebuilt.entries.len(), committed.entries.len());
        for (re, ce) in rebuilt.entries.iter().zip(committed.entries.iter()) {
            assert_eq!(re.dim, ce.dim, "dim for {}.{}", re.cx, re.cz);
            assert_eq!((re.cx, re.cz), (ce.cx, ce.cz));
            assert_eq!(re.bytes, ce.bytes, "payload byte length grounded");
            assert_eq!(re.xxh3_64, ce.xxh3_64, "digest grounded in payload bytes");
        }
    }

    /// `hash-paper [dir]` dir override: run against a scratch copy of a tree
    /// without touching committed fixtures. The single dir is both the payload
    /// source and the manifest destination, and provenance (level-type,
    /// region-file-compression, seed) is inherited from the source region
    /// manifest — nothing hardcoded except the paper pin constant.
    #[test]
    fn hash_paper_dir_override_inherits_provenance_and_covers_corpus() {
        let tmp = hash_tmp("hash-paper-dir");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        // Generate the payloads under the *same* seed the region manifest below
        // claims (corpus seed 0), so the tree is internally consistent: a
        // manifest recording a seed the payloads were not generated under would
        // be exactly the lying-manifest scenario this gate exists to catch.
        // The fixture builder embeds the seed as i64, so corpus seed 0 must fit
        // in i64 for its payload bit pattern to match the u64 decimal string the
        // region manifest records (a high-bit seed would sign-flip through
        // `as i64` and break coverage's u64 parse).
        let seed = corpus::corpus_seed(0);
        assert!(
            seed <= i64::MAX as u64,
            "corpus seed 0 ({seed}) must fit in i64 for the seeded fixture builder"
        );
        let dir = write_hash_fixture_tree_seeded(
            &tmp.join("tree"),
            &all_corpus_coordinates(),
            seed as i64,
        );
        // A region manifest with provenance the source region capture would
        // carry: flat level type, uncompressed regions, corpus seed 0.
        let region = serde_json::json!({
            "format": 1,
            "seed": corpus::corpus_seed(0).to_string(),
            "level-type": "minecraft\\:flat",
            "region-file-compression": "none",
            "kind": "full",
            "chunk-count": all_corpus_coordinates().len(),
        });
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&region).unwrap() + "\n",
        )
        .unwrap();

        run_hash_paper(Some(&dir)).expect("hash-paper dir override succeeds");
        let m = load_hash_manifest(&dir).expect("manifest written into the overridden dir");
        assert_eq!(m.seed, corpus::corpus_seed(0).to_string());
        assert_eq!(m.level_type, "minecraft\\:flat");
        assert_eq!(m.region_file_compression, "none");
        let cov = hash_manifest::coverage(&m, &corpus::Corpus::from_committed());
        assert!(
            cov.is_complete(),
            "the 8 corpus coordinates × full matrix is a complete sweep: {} present",
            cov.present
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}
