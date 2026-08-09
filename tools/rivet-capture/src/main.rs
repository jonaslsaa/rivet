//! rivet-capture — the join-path packet-capture harness (#153).
//!
//! Boots a real Java Paper server headlessly (the `verify` pattern from
//! `tools/rivet-oracle`), joins it with the Azalea headless client through a
//! byte-transparent TCP proxy, and records every packet as
//! `(state, direction, packet id, body bytes)` at the framing boundary. The raw
//! capture is normalized (`normalize::canonicalize`) into a deterministic
//! canonical form — only the fields the server randomizes per boot are
//! rewritten, each with a documented justification — and compared byte-for-byte
//! against the committed fixture (`fixtures/join/`).
//!
//! Subcommands:
//!
//!   rivet-capture capture            boot one Paper, join, print the normalized
//!                                    transcript and packet summary (debugging).
//!   rivet-capture capture --runs N   boot N Papers, require identical normalized
//!                                    captures (Paper-vs-Paper determinism check).
//!   rivet-capture fixture            capture once and (re)write the committed
//!                                    fixture under fixtures/join/.
//!   rivet-capture verify             boot fresh, capture, normalize, and diff
//!                                    against the committed fixture (PASS/FAIL).
//!   rivet-capture verify --expect-fail
//!                                    negative control: boot fresh, diff against a
//!                                    deliberately corrupted copy of the fixture;
//!                                    exits 0 only when the tampered packet is
//!                                    detected AND named.
//!   rivet-capture verify --mutate KIND
//!                                    detector discrimination: apply a controlled
//!                                    mutation and require the named detector to trip.
//!   rivet-capture audit --runs N     raw-field variance evidence across boots.
//!
//! Both verify modes enforce the pinned Paper commit (fixtures/join/manifest.json
//! `paper` provenance) against the Git-Commit attribute of the server jar the
//! paperclip actually materialized and booted, exactly like `rivet-oracle
//! verify` — a stale or unverifiable Paper fails loudly.

mod fixture;
mod frame;
mod invariants;
mod mutate;
mod normalize;
mod ordering;
mod packet;
mod proxy;
mod relationships;
mod semantic;
mod server;
mod structured;
mod variance;

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::fixture::Manifest;
use crate::normalize::{NormalizedPacket, canonicalize};
use crate::packet::{CapturedPacket, Direction, State};

const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const AZALEA_REVISION: &str = "6249c295d353b9b3ef68f665b311cba39211fd19";
/// Distinct from the oracle's 25599 so the capture never collides with a
/// concurrently-running oracle boot (the offline join only needs a local port).
const SERVER_PORT: u16 = 25600;
/// Env var naming the rivet-client binary (built in the nested tools/rivet-client
/// workspace under nightly).
const CLIENT_BIN_ENV: &str = "RIVET_CLIENT_BIN";
/// The paperclip jar env var shared with rivet-oracle.
const ORACLE_JAR_ENV: &str = "RIVET_ORACLE_JAR";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn state_str(s: State) -> &'static str {
    match s {
        State::Handshake => "handshake",
        State::Status => "status",
        State::Login => "login",
        State::Configuration => "configuration",
        State::Play => "play",
    }
}

/// Locate the rivet-client binary (offline Azalea bot) the harness drives.
fn client_binary() -> Result<PathBuf, CaptureError> {
    let default_bin = crate_root().join("../rivet-client/target/debug/rivet-client");
    resolve_client_binary(env::var(CLIENT_BIN_ENV).ok(), default_bin)
}

/// Resolve the client binary from an explicit override or the default sibling
/// build. A missing binary is a missing prerequisite, not a failure: the shared
/// exit contract (and `rivet-client run-scenario`) classifies it as UNVERIFIED
/// — nothing was compared. The resolution core is split from the env read so a
/// counterfactual test can pin that classification without mutating the process
/// environment (a global that would race across parallel tests).
fn resolve_client_binary(
    override_path: Option<String>,
    default_bin: PathBuf,
) -> Result<PathBuf, CaptureError> {
    if let Some(p) = override_path {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(CaptureError::Unverified(format!(
            "{CLIENT_BIN_ENV} is set to {} but it is not a file",
            p.display()
        )));
    }
    if default_bin.is_file() {
        return Ok(default_bin);
    }
    Err(CaptureError::Unverified(format!(
        "rivet-client binary not found at {} — build it first (tools/rivet-client/run.sh or \
         `cd tools/rivet-client && cargo build --locked`) or set {CLIENT_BIN_ENV}",
        default_bin.display()
    )))
}

/// Resolve the capture's own fixture config (`fixtures/server.properties` and
/// `fixtures/paper-world-defaults.yml`). A missing fixture is a missing
/// prerequisite — the deterministic join scenario's config cannot be
/// reproduced, so nothing is actually compared — and is UNVERIFIED (exit 3)
/// under the shared 0/1/3 contract, not FAIL (exit 1), matching how
/// `rivet-client run-scenario` classifies its own missing fixtures. Split from
/// `capture_one` so a counterfactual test can pin that classification against
/// real temp paths without booting Paper.
fn resolve_fixture_config(crate_root: &Path) -> Result<(PathBuf, PathBuf), CaptureError> {
    let server_properties = crate_root.join("fixtures/server.properties");
    if !server_properties.is_file() {
        return Err(CaptureError::Unverified(format!(
            "server.properties not found at {} (rivet-capture fixtures)",
            server_properties.display()
        )));
    }
    let world_defaults = crate_root.join("fixtures/paper-world-defaults.yml");
    if !world_defaults.is_file() {
        return Err(CaptureError::Unverified(format!(
            "paper-world-defaults.yml not found at {} (rivet-capture fixtures)",
            world_defaults.display()
        )));
    }
    Ok((server_properties, world_defaults))
}

/// The capture harness's error type. Carries the machine-stable exit code so
/// `main` can honor the shared 0 PASS / 1 FAIL / 3 UNVERIFIED contract: a
/// missing prerequisite or a server that never booted is UNVERIFIED (nothing
/// was compared), every other failure is FAIL.
#[derive(Debug)]
enum CaptureError {
    Unverified(String),
    Fail(String),
}

impl CaptureError {
    fn exit_code(&self) -> u8 {
        match self {
            CaptureError::Unverified(_) => rivet_harness_common::exit::EXIT_UNVERIFIED,
            CaptureError::Fail(_) => rivet_harness_common::exit::EXIT_FAIL,
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Unverified(m) | CaptureError::Fail(m) => write!(f, "{m}"),
        }
    }
}

impl From<String> for CaptureError {
    fn from(m: String) -> Self {
        CaptureError::Fail(m)
    }
}

impl From<&str> for CaptureError {
    fn from(m: &str) -> Self {
        CaptureError::Fail(m.to_owned())
    }
}

impl From<io::Error> for CaptureError {
    fn from(e: io::Error) -> Self {
        CaptureError::Fail(e.to_string())
    }
}

impl From<server::Error> for CaptureError {
    fn from(e: server::Error) -> Self {
        match e {
            server::Error::Unverified(m) => CaptureError::Unverified(m),
            other => CaptureError::Fail(other.to_string()),
        }
    }
}

struct ClientRun {
    stdout_text: String,
}

/// Run the headless client against the proxy once and preserve its stdout.
fn run_client(
    binary: &Path,
    proxy_port: u16,
    work: &Path,
    idx: usize,
) -> Result<ClientRun, String> {
    let address = format!("127.0.0.1:{proxy_port}");
    let stdout_path = work.join(format!("client{idx}.stdout.jsonl"));
    let stderr_path = work.join(format!("client{idx}.stderr.log"));
    let output = Command::new(binary)
        .args([
            "--address",
            &address,
            "--username",
            DEFAULT_USERNAME,
            "--timeout-seconds",
            &DEFAULT_TIMEOUT_SECONDS.to_string(),
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to run rivet-client ({}): {e}", binary.display()))?;
    fs::write(&stdout_path, &output.stdout).map_err(|e| e.to_string())?;
    fs::write(&stderr_path, &output.stderr).map_err(|e| e.to_string())?;
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "rivet-client exited with code {code} before the join completed (raw transcript in \
             {}, stderr in {}). A successful offline join must exit 0.",
            stdout_path.display(),
            stderr_path.display()
        ));
    }
    Ok(ClientRun {
        stdout_text: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

fn client_joined(stdout: &str) -> bool {
    stdout
        .lines()
        .any(|l| l.contains("\"event\":\"joined\"") || l.contains("\"event\": \"joined\""))
}

/// The raw + canonical packet lists from one full boot→join→capture pipeline.
/// Both are returned: the detectors run on the raw (order/relationships) and on
/// the canonical (content preservation) sides.
async fn capture_one(
    work: &Path,
    idx: usize,
) -> Result<(Vec<CapturedPacket>, Vec<NormalizedPacket>), CaptureError> {
    let jar = server::ensure_jar(&crate_root())?;
    let client_bin = client_binary()?;
    // The capture's own server config (seed 42 / superflat / offline, mob
    // spawning disabled so the world is empty apart from the player — the
    // deterministic join scenario). This is a deliberate, documented deviation
    // from the M0 config: random mob spawns would break the byte identity of
    // the join capture (see fixtures/server.properties,
    // fixtures/paper-world-defaults.yml and the manifest provenance).
    let (server_properties, world_defaults) = resolve_fixture_config(&crate_root())?;

    let run_dir = work.join(format!("run{idx}"));
    let log_path = work.join(format!("boot{idx}.log"));
    println!("[boot {idx}] fresh Paper world in {}", run_dir.display());
    let mut srv = server::boot(
        &run_dir,
        &log_path,
        &jar,
        &server_properties,
        &world_defaults,
    )?;

    // Reserve the proxy port after the (already-completed) server boot and
    // hold the bound listener until the proxy task spawns. The hold is short —
    // it covers only the gap between this bind and the proxy's own bind — but
    // it prevents the OS from handing this exact ephemeral port to a concurrent
    // binder in that window. Release it immediately before the proxy binds, so
    // the bind-drop-boot race narrows to the spawn->bind gap.
    let proxy_reservation = rivet_harness_common::port::PortReservation::bind()?;
    let proxy_port = proxy_reservation.port();
    let proxy_addr: SocketAddr = format!("127.0.0.1:{proxy_port}")
        .parse()
        .expect("proxy addr");
    let server_addr: SocketAddr = format!("127.0.0.1:{SERVER_PORT}")
        .parse()
        .expect("server addr");
    println!("[proxy {idx}] 127.0.0.1:{proxy_port} -> 127.0.0.1:{SERVER_PORT}");
    proxy_reservation.release();

    let proxy_task = tokio::spawn(proxy::run(proxy_addr, server_addr));

    println!("[run   {idx}] joining via rivet-client through the proxy ...");
    let work_owned = work.to_path_buf();
    let client_run =
        tokio::task::spawn_blocking(move || run_client(&client_bin, proxy_port, &work_owned, idx))
            .await
            .map_err(|e| e.to_string())??;
    if !client_joined(&client_run.stdout_text) {
        return Err(
            "client did not emit a `joined` record — the join path did not complete".into(),
        );
    }
    println!("[run   {idx}] client joined; shutting down Paper cleanly (SIGTERM)...");
    server::shutdown(&mut srv)?;

    let shared = proxy_task
        .await
        .map_err(|e| format!("proxy task failed: {e}"))?
        .map_err(|e| format!("proxy io error: {e}"))?;
    let raw = shared.lock().expect("proxy lock poisoned").packets.clone();
    if raw.is_empty() {
        return Err("proxy captured no packets — the connection did not reach the server".into());
    }

    // Preserve the raw (pre-normalization) capture for diagnostics, exactly as
    // the scenario runner preserves raw client stdout.
    {
        let mut lines = String::new();
        for p in &raw {
            let line = serde_json::json!({
                "state": state_str(p.state),
                "direction": p.direction.flow(),
                "id": p.id,
                "name": crate::packet::packet_name(p.state, p.direction, p.id),
                "body_hex": fixture::hex(&p.body),
            });
            lines.push_str(&line.to_string());
            lines.push('\n');
        }
        let raw_path = work.join(format!("raw{idx}.jsonl"));
        fs::write(&raw_path, lines).map_err(|e| format!("cannot write raw capture: {e}"))?;
    }

    let canon = canonicalize(&raw);
    if canon.is_empty() {
        return Err("normalized capture is empty".into());
    }
    Ok((raw, canon))
}

/// Shared paper/provenance for the fixture manifest.
fn manifest_provenance() -> Manifest {
    Manifest {
        format: fixture::FORMAT,
        scenario: "join".into(),
        protocol: fixture::PROTOCOL,
        paper: "26.2-DEV-main@0a99345".into(),
        bot_identity: DEFAULT_USERNAME.into(),
        server_config: "seed=42; level-type=minecraft:flat; online-mode=false; \
                        view-distance=4; simulation-distance=4; \
                        network-compression-threshold=256; \
                        spawn-limits all 0 (paper-world-defaults.yml)"
            .into(),
        azalea_revision: AZALEA_REVISION.into(),
        captured: Vec::new(),
    }
}

/// Write the fixture under fixtures/join/ from a canonical capture.
fn write_fixture(packets: &[NormalizedPacket]) -> io::Result<PathBuf> {
    let dir = crate_root().join("fixtures/join");
    let mut manifest = manifest_provenance();
    manifest.captured = fixture::build_captured(packets);
    fixture::write_fixture(&dir, packets, &manifest)?;
    Ok(dir)
}

/// Verify the committed fixture's capture.jsonl matches its manifest SHA-256s.
fn verify_committed_fixture(dir: &Path) -> Result<usize, String> {
    let manifest_path = dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest: Manifest =
        serde_json::from_str(&raw).map_err(|e| format!("invalid manifest.json: {e}"))?;
    if manifest.format != fixture::FORMAT {
        return Err(format!(
            "unsupported fixture format {} (expected {})",
            manifest.format,
            fixture::FORMAT
        ));
    }
    let packets = fixture::read_capture(dir).map_err(|e| e.to_string())?;
    let mut count = 0;
    for (entry, packet) in manifest.captured.iter().zip(packets.iter()) {
        if entry.sha256 != fixture::sha256_hex(&packet.body) {
            return Err(format!(
                "fixture manifest sha256 mismatch for {}:{}/{} — re-run `rivet-capture fixture`",
                entry.state, entry.direction, entry.id
            ));
        }
        count += 1;
    }
    Ok(count)
}

/// Read the `Git-Commit: <sha>` attribute from a Paper server jar's
/// `META-INF/MANIFEST.MF` by shelling out to `unzip -p` (mirrors
/// `rivet-client`'s `run-scenario/server.rs`). Returns `None` when the jar has
/// no such attribute (a paperclip wrapper, not a compiled server). The helper
/// is duplicated rather than shared because the two harnesses classify failures
/// through their own error types (`CaptureError` here, `RunnerError` there).
fn read_jar_git_commit(jar: &Path) -> io::Result<Option<String>> {
    let out = Command::new("unzip")
        .arg("-p")
        .arg(jar)
        .arg("META-INF/MANIFEST.MF")
        .output()?;
    if !out.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Git-Commit:")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        }))
}

/// Classify a `read_jar_git_commit` failure. A missing `unzip` binary is a
/// missing prerequisite — the pin cannot be established without reading the
/// materialized jar — so it is UNVERIFIED (exit 3), not FAIL. Any other
/// failure is a genuine IO error (FAIL), so an unreadable jar cannot
/// masquerade as "no provenance".
fn classify_commit_lookup_error(e: io::Error) -> CaptureError {
    if e.kind() == io::ErrorKind::NotFound {
        CaptureError::Unverified(
            "cannot verify the Paper pin: `unzip` is not installed (needed to read the \
             materialized server jar's Git-Commit attribute)"
                .to_string(),
        )
    } else {
        CaptureError::Fail(e.to_string())
    }
}

/// Enforce the pinned Paper commit (manifest `paper` provenance) against the
/// Git-Commit attribute of the server jar the paperclip materialized.
///
/// Every failure mode — an unreadable or unpinned fixture manifest, a missing
/// `unzip`, a missing or provenance-less materialized jar, or a commit
/// mismatch — is a missing/unverifiable prerequisite, not executed divergence:
/// without the pin holding, nothing was actually compared against the pinned
/// baseline, so all are UNVERIFIED (exit 3), exactly like `rivet-client`
/// run-scenario's `verify_paper_provenance`.
fn check_pin(fixtures_dir: &Path, run_dir: &Path) -> Result<(), CaptureError> {
    let raw = fs::read_to_string(fixtures_dir.join("manifest.json"))
        .map_err(|e| CaptureError::Unverified(format!("cannot read fixture manifest: {e}")))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|e| CaptureError::Unverified(format!("invalid fixture manifest: {e}")))?;
    let expected = manifest
        .paper
        .rsplit_once('@')
        .map(|(_, c)| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            CaptureError::Unverified(
                "fixture manifest carries no `@<commit>` Paper pin".to_string(),
            )
        })?;

    let jar = run_dir.join("versions/26.2/paper-26.2.jar");
    if !jar.is_file() {
        return Err(CaptureError::Unverified(format!(
            "materialized server jar {} missing — the paperclip did not materialize a server",
            jar.display()
        )));
    }
    let actual = read_jar_git_commit(&jar)
        .map_err(classify_commit_lookup_error)?
        .ok_or_else(|| {
            CaptureError::Unverified(format!(
                "the materialized server jar {} carries no readable Git-Commit",
                jar.display()
            ))
        })?;

    if expected != actual {
        return Err(CaptureError::Unverified(format!(
            "Paper commit mismatch: the server jar that actually booted carries Git-Commit \
             {actual}, but the fixture baseline (fixtures/join/manifest.json) is pinned to \
             {expected}. Regenerate the fixture against the pinned Paper and re-pin the manifest \
             before relying on this gate — never fudge fixtures to pass."
        )));
    }
    println!(
        "   paper pin      : {} (fixtures/join provenance) — enforced (booted jar is Git-Commit {actual})",
        manifest.paper
    );
    Ok(())
}

/// Print a packet-summary transcript for a canonical capture (debugging).
fn print_summary(packets: &[NormalizedPacket]) {
    let mut by_key: Vec<(&NormalizedPacket, usize)> = Vec::new();
    for p in packets {
        match by_key
            .iter_mut()
            .find(|(e, _)| e.state == p.state && e.direction == p.direction && e.id == p.id)
        {
            Some((_, n)) => *n += 1,
            None => by_key.push((p, 1)),
        }
    }
    println!("captured {} canonical packets:", packets.len());
    for (p, count) in by_key {
        let name =
            crate::packet::packet_name(p.state, p.direction, p.id).unwrap_or("minecraft:unknown");
        println!(
            "  {:>3}  {:<13} {:<11} {:>3}  {}  {:>6} bytes",
            count,
            p.state.to_string(),
            p.direction.flow(),
            p.id,
            name,
            p.body.len()
        );
        if !p.note.is_empty() {
            println!("         normalize: {}", p.note);
        }
    }
}

fn usage() -> String {
    format!(
        "rivet-capture — join-path packet-capture harness (#153, #195)\n\
         \n\
         USAGE:\n\
         \x20 rivet-capture capture [--runs N]      boot+join one or more Papers, print the\n\
         \x20                                     normalized packet summary (Paper-vs-Paper\n\
         \x20                                     determinism when --runs >= 2)\n\
         \x20 rivet-capture fixture                boot+join once and (re)write fixtures/join/\n\
         \x20 rivet-capture verify                boot+join, normalize, diff against the\n\
         \x20                                     committed fixture AND run the #195 semantic/\n\
         \x20                                     order detectors (PASS/FAIL)\n\
         \x20 rivet-capture verify --expect-fail  negative control: boot+join, diff against a\n\
         \x20                                     deliberately corrupted fixture copy; exits 0\n\
         \x20                                     only when the tampered packet is detected AND\n\
         \x20                                     named\n\
         \x20 rivet-capture verify --mutate KIND  boot+join, apply a controlled mutation\n\
         \x20                                     (reorder|delete|insert|field|canon|relabel|burst|entity-id|set-time-absent)\n\
         \x20                                     to the capture and REQUIRE the named detector\n\
         \x20                                     failure — a clean run is itself a failure\n\
         \x20 rivet-capture audit --runs N        boot N Papers, report per-packet raw-body\n\
         \x20                                     variance (the evidence behind each normalize\n\
         \x20                                     rewrite)\n\
         \n\
         ENV:\n\
         \x20 {ORACLE_JAR_ENV}          path to the paperclip jar (default: work/jars/ or working/Paper/)\n\
         \x20 {CLIENT_BIN_ENV}   path to the rivet-client binary (default: ../rivet-client/target/debug/rivet-client)"
    )
}

enum Subcommand {
    Capture {
        runs: usize,
    },
    Fixture,
    Verify {
        expect_fail: bool,
        mutate: Option<crate::mutate::MutationKind>,
    },
    Audit {
        runs: usize,
    },
}

fn parse_args() -> Result<Subcommand, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("capture") => {
            let mut runs = 1usize;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--runs" => {
                        i += 1;
                        runs = args
                            .get(i)
                            .ok_or("missing value for --runs")?
                            .parse()
                            .map_err(|_| "invalid --runs value")?;
                    }
                    "--help" | "-h" => return Err(usage()),
                    other => return Err(format!("unknown argument: {other}\n\n{}", usage())),
                }
                i += 1;
            }
            if runs == 0 {
                return Err("--runs must be >= 1".into());
            }
            Ok(Subcommand::Capture { runs })
        }
        Some("fixture") => Ok(Subcommand::Fixture),
        Some("verify") => {
            let mut expect_fail = false;
            let mut mutate = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--expect-fail" => expect_fail = true,
                    "--mutate" => {
                        i += 1;
                        let name = args.get(i).ok_or("missing value for --mutate")?;
                        mutate = Some(
                            crate::mutate::MutationKind::from_name(name)
                                .ok_or_else(|| format!("unknown --mutate kind: {name}"))?,
                        );
                    }
                    other => return Err(format!("unknown argument: {other}\n\n{}", usage())),
                }
                i += 1;
            }
            if expect_fail && mutate.is_some() {
                return Err("--expect-fail and --mutate are mutually exclusive".into());
            }
            Ok(Subcommand::Verify {
                expect_fail,
                mutate,
            })
        }
        Some("audit") => {
            let mut runs = 2usize;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--runs" => {
                        i += 1;
                        runs = args
                            .get(i)
                            .ok_or("missing value for --runs")?
                            .parse()
                            .map_err(|_| "invalid --runs value")?;
                    }
                    "--help" | "-h" => return Err(usage()),
                    other => return Err(format!("unknown argument: {other}\n\n{}", usage())),
                }
                i += 1;
            }
            if runs < 2 {
                return Err("audit --runs must be >= 2".into());
            }
            Ok(Subcommand::Audit { runs })
        }
        Some("--help") | Some("-h") | None => Err(usage()),
        Some(other) => Err(format!("unknown subcommand: {other}\n\n{}", usage())),
    }
}

/// The negative control: copy the committed fixture, corrupt one packet body
/// and its manifest SHA-256 (so the copy is internally consistent), then boot
/// fresh and require the divergence to name the tampered packet.
async fn run_negative_control() -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/verify");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let baseline = crate_root.join("fixtures/join");

    println!("negative control: verify --expect-fail");
    println!("   baseline fixture : {}", baseline.display());

    // Copy the fixture and corrupt one packet's body + manifest sha.
    let scratch = env::temp_dir().join(format!("rivet-capture-negcontrol-{}", std::process::id()));
    if scratch.exists() {
        fs::remove_dir_all(&scratch).map_err(|e| e.to_string())?;
    }
    rivet_harness_common::negative::copy_dir_recursive(&baseline, &scratch)
        .map_err(|e| e.to_string())?;
    let (tampered_index, tampered_identity) = tamper_fixture_copy(&scratch)?;
    println!(
        "   fixture copied to {} and packet [{tampered_index}] ({tampered_identity}) corrupted",
        scratch.display()
    );

    let (_raw, canon) = capture_one(&work, 1).await?;

    // The corrupted copy is the "expected" baseline; the fresh boot is the
    // "actual". If the fresh capture reproduces the original bytes, the only
    // divergence is at the tampered packet.
    let corrupted_packets = fixture::read_capture(&scratch).map_err(|e| e.to_string())?;
    let diff = fixture::diff_packets(&corrupted_packets, &canon);

    // The negative control passes only when the tampered packet is the one
    // named in the mismatch list. A clean diff (false negative) or a divergence
    // naming a different packet must fail.
    let tampered_index = tampered_index.to_string();
    let mismatched: Vec<String> = diff
        .mismatched
        .iter()
        .map(|(i, _, _, _)| i.to_string())
        .collect();
    match rivet_harness_common::negative::verdict(&tampered_index, &mismatched) {
        rivet_harness_common::negative::Verdict::Detected(_) => {
            println!();
            println!(
                "PASS: the fresh capture differs from the corrupted copy at exactly the tampered \
                 packet [{tampered_index}] ({tampered_identity})."
            );
            let _ = fs::remove_dir_all(&scratch);
            Ok(())
        }
        v => {
            let _ = fs::remove_dir_all(&scratch);
            Err(format!(
                "negative control FAILED: {v} — the capture->normalize->diff chain did not name \
                 the tampered packet [{tampered_index}] ({tampered_identity}); {diff}"
            )
            .into())
        }
    }
}

/// Corrupt one packet body in a copy of the fixture and its manifest SHA-256,
/// returning (index, identity) of the tampered packet.
///
/// The tamper deliberately lands in a packet the normalizer does NOT rewrite
/// (a `note`-less manifest entry), so the negative control proves the
/// byte-compare detects drift in a field that is genuinely compared — not in a
/// field that would have been normalized to a fixed value anyway.
fn tamper_fixture_copy(dir: &Path) -> Result<(usize, String), String> {
    let mut manifest: Manifest = serde_json::from_str(
        &fs::read_to_string(dir.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut packets = fixture::read_capture(dir).map_err(|e| e.to_string())?;

    let normalized: Vec<bool> = manifest
        .captured
        .iter()
        .map(|e| !e.note.is_empty())
        .collect();
    let idx = packets
        .iter()
        .enumerate()
        .filter(|(i, p)| p.body.len() > 1 && !normalized.get(*i).copied().unwrap_or(false))
        .map(|(i, _)| i)
        .next()
        .or_else(|| {
            packets
                .iter()
                .enumerate()
                .filter(|(_, p)| p.body.len() > 1)
                .map(|(i, _)| i)
                .next()
        })
        .ok_or("fixture has no tamperable packet (all bodies are empty or length 1)")?;

    let identity = {
        let p = &packets[idx];
        format!("{}/{} id {}", p.state, p.direction.flow(), p.id)
    };
    let i = packets[idx].body.len() / 2;
    packets[idx].body[i] ^= 0xFF;

    // Rewrite the tampered packet's body in capture.jsonl and its sha/bytes in
    // the manifest so the copy is internally consistent (a plausible but wrong
    // baseline).
    fs::write(dir.join("capture.jsonl"), fixture::capture_lines(&packets))
        .map_err(|e| e.to_string())?;
    let entry = manifest
        .captured
        .get_mut(idx)
        .ok_or("tampered packet has no manifest entry")?;
    entry.sha256 = fixture::sha256_hex(&packets[idx].body);
    entry.bytes = packets[idx].body.len();
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok((idx, identity))
}

async fn run_verify() -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/verify");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let baseline = crate_root.join("fixtures/join");

    println!("verify (join packet-capture fixture + semantic/order invariants)");
    println!("   baseline fixture : {}", baseline.display());

    verify_committed_fixture(&baseline)?;
    println!("   committed fixture verifies against its manifest");

    let (raw, canon) = capture_one(&work, 1).await?;
    let baseline_packets = fixture::read_capture(&baseline).map_err(|e| e.to_string())?;

    check_pin(&baseline, &work.join("run1"))?;

    let mut failures = crate::invariants::check(&raw, &canon);
    if !failures.is_empty() {
        failures.sort_by(|a, b| (a.kind, &a.identity).cmp(&(b.kind, &b.identity)));
        println!();
        println!(
            "FAIL: {} invariant violation(s) in the fresh capture:",
            failures.len()
        );
        for f in &failures {
            println!("  - {f}");
        }
        return Err("join-capture semantic/order invariants violated".into());
    }
    println!("   semantic + order invariants pass on the fresh capture");

    let diff = fixture::diff_packets(&baseline_packets, &canon);
    // The identity diff names missing/extra packets by their stable identity
    // (chunk coordinates, registry ordinals) instead of a shifted index.
    let (mismatched, missing, extra) = fixture::identity_diff(&baseline_packets, &canon);
    if diff.is_clean() && mismatched.is_empty() && missing.is_empty() && extra.is_empty() {
        println!();
        println!(
            "PASS: {} normalized packets are byte-identical to the committed join fixture \
             (seed 42 / superflat / offline RivetProbe) — green against vanilla itself.",
            baseline_packets.len()
        );
        Ok(())
    } else {
        println!();
        println!("FAIL: the fresh normalized capture diverges from the committed fixture:");
        print!("{diff}");
        for (id, want, got) in &mismatched {
            println!("    mismatched {id}\n      expected {want}\n      actual   {got}");
        }
        for id in &missing {
            println!("    missing    {id}");
        }
        for id in &extra {
            println!("    extra      {id}");
        }
        Err("join-capture fixture divergence".into())
    }
}

/// Read a raw `rawN.jsonl` capture back into `CapturedPacket`s. The raw file is
/// the diagnostic artifact `capture_one` writes (`{state, direction, id, name,
/// body_hex}` per line).
#[cfg(test)]
fn read_raw_jsonl(path: &Path) -> Result<Vec<CapturedPacket>, String> {
    #[derive(serde::Deserialize)]
    struct RawLine {
        state: String,
        direction: String,
        id: i32,
        #[serde(default)]
        body_hex: String,
    }
    let raw =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let l: RawLine = serde_json::from_str(line)
            .map_err(|e| format!("invalid raw line in {}: {e}", path.display()))?;
        let state = match l.state.as_str() {
            "handshake" => State::Handshake,
            "status" => State::Status,
            "login" => State::Login,
            "configuration" => State::Configuration,
            _ => State::Play,
        };
        let direction = match l.direction.as_str() {
            "serverbound" => Direction::Serverbound,
            _ => Direction::Clientbound,
        };
        out.push(CapturedPacket {
            state,
            direction,
            id: l.id,
            body: fixture::unhex_pub(&l.body_hex).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

/// `verify --mutate <kind>`: apply a controlled mutation to the fresh raw
/// capture (or its canonical form) and require the named detector failure. A
/// clean run is itself a failure (false-negative trap).
async fn run_verify_mutate(kind: crate::mutate::MutationKind) -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/verify-mutate");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let baseline = crate_root.join("fixtures/join");
    println!(
        "verify --mutate {} (negative control for the #195 detectors)",
        kind.name()
    );

    // The committed fixture is the canonical baseline. Boot a fresh Paper, run
    // the detectors on its clean raw+canon, then apply the mutation and require
    // the detectors to trip with the expected kinds.
    let (raw, canon) = capture_one(&work, 1).await?;
    check_pin(&baseline, &work.join("run1"))?;

    // The clean capture must pass every detector.
    let clean_failures = crate::invariants::check(&raw, &canon);
    if !clean_failures.is_empty() {
        return Err(format!(
            "verify --mutate {}: the clean capture already fails {} invariant(s) — fix the harness before testing the mutation:\n  {clean_failures:?}",
            kind.name(),
            clean_failures.len()
        )
        .into());
    }

    let mutated = crate::mutate::mutate_raw(kind, &raw);
    let mut mutated_canon = canonicalize(&mutated);
    if kind == crate::mutate::MutationKind::Canon {
        crate::mutate::mutate_canon(&mut mutated_canon);
    } else if kind == crate::mutate::MutationKind::SetTimeAbsent {
        crate::mutate::mutate_set_time_absent(&mut mutated_canon);
    }

    let failures = crate::invariants::check(&mutated, &mutated_canon);
    let expected = kind.expected_kinds();
    let hit: Vec<&str> = expected
        .iter()
        .filter(|k| failures.iter().any(|f| f.kind == **k))
        .copied()
        .collect();

    if hit.is_empty() {
        return Err(format!(
            "verify --mutate {} FAILED (false negative): the mutation produced NO failure of the expected kinds {expected:?} — the detectors are not discriminating.\n  actual failures: {failures:?}",
            kind.name()
        )
        .into());
    }
    let _ = expected;

    println!();
    println!(
        "PASS: verify --mutate {} detected and named the defect ({}) — the #195 detectors are discriminating.",
        kind.name(),
        hit.join(", ")
    );
    for f in &failures {
        println!("  - {f}");
    }
    Ok(())
}

/// `audit --runs N`: boot N Papers, collect the raw captures, and report per
/// packet identity how many distinct raw bodies were observed (the multi-boot
/// field-variance evidence that justifies each normalization rewrite).
async fn run_audit(runs: usize) -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/audit");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut raws = Vec::with_capacity(runs);
    for idx in 1..=runs {
        let (raw, _canon) = capture_one(&work, idx).await?;
        raws.push(raw);
    }
    let report = crate::variance::analyze(&raws);
    println!("raw-field variance across {runs} Paper boots:");
    println!("  {:>5}  packet identity", "distinct");
    for (identity, distinct, _hexes) in &report.fields {
        let marker = if *distinct == 1 {
            " (deterministic)"
        } else {
            ""
        };
        println!("  {distinct:>5}  {identity}{marker}");
    }
    let varying = report.fields.iter().filter(|(_, d, _)| *d > 1).count();
    println!();
    println!(
        "{} of {} packet identities vary across the {runs} boots (the rest are byte-deterministic).",
        varying,
        report.fields.len()
    );
    Ok(())
}

async fn run_capture(runs: usize) -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/capture");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    let mut captures = Vec::with_capacity(runs);
    for idx in 1..=runs {
        let (_raw, canon) = capture_one(&work, idx).await?;
        print_summary(&canon);
        if canon
            .iter()
            .any(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 49)
        {
            println!("    (login packet present — join path reached play)");
        }
        captures.push(canon);
    }

    if runs >= 2 {
        println!();
        println!("Paper-vs-Paper comparison ({} boots)", runs);
        let mut identical = true;
        for (i, pair) in captures.windows(2).enumerate() {
            let a = fixture::capture_lines(&pair[0]);
            let b = fixture::capture_lines(&pair[1]);
            if a == b {
                println!(
                    "    boot {} vs boot {}: IDENTICAL ({} bytes)",
                    i + 1,
                    i + 2,
                    a.len()
                );
            } else {
                identical = false;
                let d = fixture::diff_packets(&pair[0], &pair[1]);
                println!("    boot {} vs boot {}: DIFFERS", i + 1, i + 2);
                print!("{d}");
            }
        }
        if identical {
            println!(
                "VERDICT: PASS — all {runs} Paper boots produced identical normalized captures."
            );
        } else {
            return Err("Paper-vs-Paper captures differ".into());
        }
    }
    Ok(())
}

async fn run_fixture() -> Result<(), CaptureError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/fixture");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let (_raw, canon) = capture_one(&work, 1).await?;
    let dir = write_fixture(&canon).map_err(|e| e.to_string())?;
    println!("wrote fixture to {}", dir.display());
    print_summary(&canon);
    Ok(())
}

fn main() -> ExitCode {
    let command = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(64);
        }
    };
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let result = runtime.block_on(async move {
        match command {
            Subcommand::Capture { runs } => run_capture(runs).await,
            Subcommand::Fixture => run_fixture().await,
            Subcommand::Verify {
                expect_fail,
                mutate,
            } => {
                if let Some(kind) = mutate {
                    run_verify_mutate(kind).await
                } else if expect_fail {
                    run_negative_control().await
                } else {
                    run_verify().await
                }
            }
            Subcommand::Audit { runs } => run_audit(runs).await,
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rivet-capture: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

/// Validate every #195 detector against a committed raw capture (no Paper boot):
/// run the full invariant set over `work/capture/rawN.jsonl` when present. This
/// is the ground-truth false-positive check — the two real raw captures from the
/// research (`raw1`/`raw2`) must pass every detector cleanly.
#[cfg(test)]
mod real_capture_tests {
    use super::*;

    fn raw_path() -> PathBuf {
        crate_root().join("work/capture/raw1.jsonl")
    }

    /// Offline counterfactual of the negative-control machinery (no Paper boot):
    /// copy the committed fixture, corrupt one packet, and prove the diff names
    /// exactly the tampered index — a clean diff or a wrong-index diff must fail
    /// the control.
    #[test]
    fn tamper_fixture_copy_detects_and_names_the_tampered_packet() {
        // The wrong-path tampering control must be load-bearing: the committed
        // fixture (fixtures/join/capture.jsonl) is the ground truth, so a missing
        // fixture is a hard failure, not a silent skip that would mask the control
        // being untested.
        let baseline = crate_root().join("fixtures/join");
        assert!(
            baseline.join("capture.jsonl").is_file(),
            "no committed fixture at {} — the wrong-path tampering control is untested",
            baseline.display()
        );
        let scratch =
            std::env::temp_dir().join(format!("rivet-capture-tamper-{}", std::process::id()));
        if scratch.exists() {
            fs::remove_dir_all(&scratch).unwrap();
        }
        rivet_harness_common::negative::copy_dir_recursive(&baseline, &scratch).unwrap();
        let (tampered_index, _identity) = tamper_fixture_copy(&scratch).unwrap();

        let original = fixture::read_capture(&baseline).unwrap();
        let corrupted = fixture::read_capture(&scratch).unwrap();
        let mismatched: Vec<String> = fixture::diff_packets(&original, &corrupted)
            .mismatched
            .iter()
            .map(|(i, _, _, _)| i.to_string())
            .collect();

        // The tamper is detected and named by index. A clean diff (the pipeline
        // never saw the injection) or a wrong-index diff must fail the control.
        let v = rivet_harness_common::negative::verdict(&tampered_index.to_string(), &mismatched);
        assert!(
            v.passed(),
            "the tampered packet was not detected and named: {v}"
        );
        assert!(
            !rivet_harness_common::negative::verdict(
                &(tampered_index + 1).to_string(),
                &mismatched
            )
            .passed(),
            "a wrong-path verdict must not satisfy the negative control"
        );
        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn real_raw_capture_passes_all_detectors() {
        let path = raw_path();
        if !path.is_file() {
            eprintln!("skipping: no committed raw capture at {}", path.display());
            return;
        }
        let raw = read_raw_jsonl(&path).expect("parse raw capture");
        let canon = canonicalize(&raw);
        let failures = crate::invariants::check(&raw, &canon);
        assert!(
            failures.is_empty(),
            "real raw capture violated {} invariant(s):\n  {failures:?}",
            failures.len()
        );
    }

    #[test]
    fn real_raw_capture_reproaches_committed_fixture() {
        let path = raw_path();
        if !path.is_file() {
            eprintln!("skipping: no committed raw capture at {}", path.display());
            return;
        }
        let raw = read_raw_jsonl(&path).expect("parse raw capture");
        let canon = canonicalize(&raw);
        let baseline = crate_root().join("fixtures/join");
        let baseline_packets = fixture::read_capture(&baseline).expect("read fixture");
        let (mismatched, missing, extra) = fixture::identity_diff(&baseline_packets, &canon);
        assert!(
            mismatched.is_empty() && missing.is_empty() && extra.is_empty(),
            "committed raw capture does not canonicalize to the committed fixture: \
             mismatched={mismatched:?} missing={missing:?} extra={extra:?}"
        );
    }

    #[test]
    fn real_raw1_raw2_canonicalize_set_time_identically() {
        let p1 = crate_root().join("work/capture/raw1.jsonl");
        let p2 = crate_root().join("work/capture/raw2.jsonl");
        if !p1.is_file() || !p2.is_file() {
            eprintln!(
                "skipping: raw captures not both present ({} and {})",
                p1.display(),
                p2.display()
            );
            return;
        }
        let set_time_bodies = |p: PathBuf| {
            let raw = read_raw_jsonl(&p).expect("parse raw capture");
            let canon = canonicalize(&raw);
            let bodies: Vec<Vec<u8>> = canon
                .iter()
                .filter(|q| {
                    q.state == State::Play && q.direction == Direction::Clientbound && q.id == 113
                })
                .map(|q| q.body.clone())
                .collect();
            assert!(
                !bodies.is_empty(),
                "raw capture {} carries no canonical set_time",
                p.display()
            );
            bodies
        };
        let a = set_time_bodies(p1.clone());
        let b = set_time_bodies(p2.clone());
        assert_eq!(
            a, b,
            "raw1 and raw2 canonicalize set_time to different bodies — the holder order is \
             boot-varying and was not sorted deterministically"
        );
    }

    /// Every controlled mutation must trip its expected detector (the DoD for
    /// #195): a mutation the detectors miss is a false negative — the harness is
    /// not discriminating. Runs offline on the committed raw capture, mirroring
    /// exactly what `verify --mutate <kind>` does against a fresh Paper boot.
    #[test]
    fn each_mutation_trips_its_expected_detector() {
        let path = raw_path();
        if !path.is_file() {
            eprintln!("skipping: no committed raw capture at {}", path.display());
            return;
        }
        let raw = read_raw_jsonl(&path).expect("parse raw capture");
        let clean_canon = canonicalize(&raw);
        let clean_failures = crate::invariants::check(&raw, &clean_canon);
        assert!(
            clean_failures.is_empty(),
            "clean real raw capture already fails detectors (false positive):\n  {clean_failures:?}"
        );

        for kind in crate::mutate::MutationKind::all() {
            let mutated = crate::mutate::mutate_raw(kind, &raw);
            let mut canon = canonicalize(&mutated);
            if kind == crate::mutate::MutationKind::Canon {
                crate::mutate::mutate_canon(&mut canon);
            } else if kind == crate::mutate::MutationKind::SetTimeAbsent {
                crate::mutate::mutate_set_time_absent(&mut canon);
            }
            let failures = crate::invariants::check(&mutated, &canon);
            let expected = kind.expected_kinds();
            let hit: Vec<&str> = expected
                .iter()
                .filter(|k| failures.iter().any(|f| f.kind == **k))
                .copied()
                .collect();
            assert!(
                !hit.is_empty(),
                "mutate {:?} produced NO failure of expected kinds {expected:?} on the real raw \
                 capture — the detectors are not discriminating. Actual failures: {failures:?}",
                kind.name()
            );
        }
    }
}

/// The exit classification of a missing/invalid rivet-client binary, pinned
/// against the shared 0/1/3 contract (see `rivet-harness-common::exit`).
#[cfg(test)]
mod client_binary_tests {
    use super::*;

    /// Path that is guaranteed to exist as a file, so the positive resolution
    /// branch is deterministic regardless of whether a rivet-client has been
    /// built in this checkout.
    fn existing_file() -> PathBuf {
        let p = crate_root().join("Cargo.toml");
        assert!(p.is_file(), "rivet-capture Cargo.toml must exist");
        p
    }

    /// Path that is guaranteed not to be a file, so the missing-prerequisite
    /// branch is deterministic regardless of the build state.
    fn non_file() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rivet-capture-no-such-client-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        assert!(
            !p.is_file(),
            "temp path must not already be a file: {}",
            p.display()
        );
        p
    }

    /// Counterfactual for the exit contract: a missing or invalid rivet-client
    /// binary is UNVERIFIED (exit 3), not FAIL (exit 1) — the shared
    /// 0/1/3 classification, matching what `rivet-client run-scenario` does for
    /// its own missing client. Both the default and the override path are
    /// injected so the test is load-bearing without mutating the process
    /// environment (a global that would race across parallel tests).
    #[test]
    fn missing_or_invalid_client_binary_classifies_unverified() {
        let missing = non_file();

        // Default path: the sibling rivet-client build does not exist.
        let err = resolve_client_binary(None, missing.clone()).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a missing default client binary must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a missing default client binary must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains("rivet-client binary not found"),
            "the Unverified error must name the missing default binary, got: {err}"
        );

        // Override path: RIVET_CLIENT_BIN points at a path that is not a file.
        let err = resolve_client_binary(Some(missing.display().to_string()), missing.clone())
            .unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "an invalid RIVET_CLIENT_BIN must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "an invalid RIVET_CLIENT_BIN must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains(CLIENT_BIN_ENV),
            "the Unverified error must name RIVET_CLIENT_BIN, got: {err}"
        );
    }

    /// A resolvable client binary (default or override) still succeeds.
    #[test]
    fn existing_client_binary_resolves() {
        let real = existing_file();
        assert_eq!(
            resolve_client_binary(None, real.clone()).expect("default existing path resolves"),
            real
        );
        assert_eq!(
            resolve_client_binary(Some(real.display().to_string()), real.clone())
                .expect("override existing path resolves"),
            real
        );
    }
}

/// Exit classification of the capture's missing/unverifiable prerequisites —
/// the fixture config and the Paper pin — pinned against the shared 0/1/3
/// contract. Each is a missing prerequisite, not executed divergence: nothing
/// was actually compared against the pinned baseline, so all classify
/// UNVERIFIED (exit 3), exactly like `rivet-client run-scenario`'s
/// `verify_paper_provenance` and missing-fixture errors.
#[cfg(test)]
mod prerequisite_tests {
    use super::*;

    /// A guaranteed-absent path under a fresh temp dir (never already a file),
    /// so the missing-prerequisite branch is deterministic.
    fn non_file() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rivet-capture-no-such-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        assert!(
            !p.is_file(),
            "temp path must not already be a file: {}",
            p.display()
        );
        p
    }

    /// CRC-32 (IEEE) over `data`, matching what zip tools verify. The test
    /// builds real jar files so `unzip` reads the MANIFEST without a bad-CRC
    /// warning (a bad CRC makes `unzip -p` exit nonzero, which
    /// `read_jar_git_commit` treats as "no attribute").
    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }

    /// Write a minimal valid zip (one stored `META-INF/MANIFEST.MF` entry) to
    /// `dir/paper.jar`. `commit` is the `Git-Commit` attribute value, or omitted
    /// to emulate a jar with no provenance.
    fn make_jar(dir: &Path, commit: Option<&str>) -> PathBuf {
        let manifest = match commit {
            Some(c) => format!(
                "Manifest-Version: 1.0\r\nGit-Commit: {c}\r\nSpecification-Version: 26.2\r\n"
            ),
            None => "Manifest-Version: 1.0\r\nSpecification-Version: 26.2\r\n".to_owned(),
        };
        let name = b"META-INF/MANIFEST.MF";
        let data = manifest.as_bytes();
        let crc = crc32(data);
        let jar = dir.join("paper.jar");

        let mut bytes = Vec::new();
        // Local file header (method 0 = store, so sizes match data).
        bytes.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        let local_offset = 0u32;
        let central_start = bytes.len() as u32;

        // Central directory entry.
        bytes.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&local_offset.to_le_bytes());
        bytes.extend_from_slice(name);
        let central_size = bytes.len() as u32 - central_start;

        // End of central directory.
        bytes.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&central_size.to_le_bytes());
        bytes.extend_from_slice(&central_start.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());

        fs::write(&jar, bytes).expect("write jar");
        jar
    }

    /// A fixtures dir whose manifest.json pins `paper` to `commit`.
    fn fixtures_dir(tag: &str, paper: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rivet-capture-fixtures-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut manifest = manifest_provenance();
        manifest.paper = paper.to_owned();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    /// A run dir laid out like a Paper boot: the materialized server jar under
    /// `versions/26.2/`. `commit` is the jar's Git-Commit attribute.
    fn run_dir_with_materialized_jar(tag: &str, commit: Option<&str>) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rivet-capture-jar-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let versions = dir.join("versions/26.2");
        fs::create_dir_all(&versions).unwrap();
        let jar = make_jar(&versions, commit);
        fs::rename(&jar, versions.join("paper-26.2.jar")).unwrap();
        dir
    }

    /// Whether `unzip` is on PATH; the jar-based pin tests need the real binary
    /// (they shell out exactly like `check_pin` does at runtime).
    fn unzip_available() -> bool {
        Command::new("unzip")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Counterfactual for the exit contract: a missing capture fixture config
    /// (server.properties / paper-world-defaults.yml) is UNVERIFIED (exit 3),
    /// not FAIL (exit 1) — without the deterministic scenario config, nothing
    /// was actually compared. Both fixtures are resolved from real temp paths so
    /// the test is load-bearing without mutating the process environment.
    #[test]
    fn missing_fixture_config_classifies_unverified() {
        // server.properties missing: the first branch fires.
        let base = non_file();
        let err = resolve_fixture_config(&base).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a missing server.properties must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a missing server.properties must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains("server.properties"),
            "the Unverified error must name the missing fixture, got: {err}"
        );

        // server.properties present, paper-world-defaults.yml missing: the
        // second branch fires.
        let dir = std::env::temp_dir().join(format!(
            "rivet-capture-fixtures-partial-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("fixtures")).unwrap();
        fs::write(dir.join("fixtures/server.properties"), "level-seed=42\n").unwrap();
        let err = resolve_fixture_config(&dir).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a missing paper-world-defaults.yml must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a missing paper-world-defaults.yml must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains("paper-world-defaults.yml"),
            "the Unverified error must name the missing fixture, got: {err}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Both capture fixtures present still resolve.
    #[test]
    fn present_fixture_config_resolves() {
        let dir =
            std::env::temp_dir().join(format!("rivet-capture-fixtures-ok-{}", std::process::id()));
        fs::create_dir_all(dir.join("fixtures")).unwrap();
        fs::write(dir.join("fixtures/server.properties"), "level-seed=42\n").unwrap();
        fs::write(
            dir.join("fixtures/paper-world-defaults.yml"),
            "spawn-limits:\n",
        )
        .unwrap();
        let (props, defaults) = resolve_fixture_config(&dir).expect("present fixtures resolve");
        assert_eq!(props.file_name().unwrap(), "server.properties");
        assert_eq!(defaults.file_name().unwrap(), "paper-world-defaults.yml");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A missing `unzip` binary is a missing prerequisite — the pin cannot be
    /// established without reading the materialized jar — so it is UNVERIFIED
    /// (exit 3), not FAIL. Fabricate the io::Error directly rather than mutating
    /// PATH (unsafe on this toolchain and racy across parallel tests).
    #[test]
    fn missing_unzip_classifies_unverified() {
        let err = classify_commit_lookup_error(io::Error::new(
            io::ErrorKind::NotFound,
            "program not found",
        ));
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a missing unzip binary must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a missing unzip binary must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains("unzip"),
            "must name unzip as the missing prereq, got {err}"
        );
    }

    /// A non-NotFound unzip failure stays a genuine FAIL (a corrupt or
    /// unreadable jar must not masquerade as "no provenance" UNVERIFIED).
    #[test]
    fn non_notfound_commit_lookup_error_is_fail() {
        let err = classify_commit_lookup_error(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        assert!(
            matches!(err, CaptureError::Fail(_)),
            "a non-NotFound read failure must stay FAIL, got {err:?}"
        );
    }

    /// The materialized jar's Git-Commit must equal the fixture manifest's pin;
    /// when it does not, the pin cannot be enforced and the run classifies
    /// UNVERIFIED (exit 3) — a missing/unverifiable prerequisite, not a FAIL.
    #[test]
    fn pin_mismatch_classifies_unverified() {
        if !unzip_available() {
            eprintln!("skipping: unzip not on PATH");
            return;
        }
        let fixtures = fixtures_dir("mismatch", "26.2-DEV-main@0a99345");
        let run_dir = run_dir_with_materialized_jar("mismatch", Some("deadbeef"));
        let err = check_pin(&fixtures, &run_dir).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a pin mismatch must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a pin mismatch must exit UNVERIFIED (3), not FAIL (1)"
        );
        assert!(
            err.to_string().contains("Paper commit mismatch"),
            "must name the commit mismatch, got {err}"
        );
        let _ = fs::remove_dir_all(&fixtures);
        let _ = fs::remove_dir_all(&run_dir);
    }

    /// The matching pin still passes (and prints the enforced provenance).
    #[test]
    fn matching_pin_passes() {
        if !unzip_available() {
            eprintln!("skipping: unzip not on PATH");
            return;
        }
        let fixtures = fixtures_dir("match", "26.2-DEV-main@0a99345");
        let run_dir = run_dir_with_materialized_jar("match", Some("0a99345"));
        check_pin(&fixtures, &run_dir).expect("matching pin must pass");
        let _ = fs::remove_dir_all(&fixtures);
        let _ = fs::remove_dir_all(&run_dir);
    }

    /// A materialized jar with no Git-Commit attribute cannot have its pin
    /// enforced: UNVERIFIED (exit 3), not FAIL.
    #[test]
    fn no_git_commit_attribute_classifies_unverified() {
        if !unzip_available() {
            eprintln!("skipping: unzip not on PATH");
            return;
        }
        let fixtures = fixtures_dir("noattr", "26.2-DEV-main@0a99345");
        let run_dir = run_dir_with_materialized_jar("noattr", None);
        let err = check_pin(&fixtures, &run_dir).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a jar without provenance must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a jar without provenance must exit UNVERIFIED (3), not FAIL (1)"
        );
        let _ = fs::remove_dir_all(&fixtures);
        let _ = fs::remove_dir_all(&run_dir);
    }

    /// A missing materialized jar (e.g. a paperclip boot that never wrote
    /// `versions/`) cannot have its pin enforced: UNVERIFIED, not FAIL.
    #[test]
    fn missing_materialized_jar_classifies_unverified() {
        let fixtures = fixtures_dir("nojar", "26.2-DEV-main@0a99345");
        let run_dir =
            std::env::temp_dir().join(format!("rivet-capture-nojar-{}", std::process::id()));
        fs::create_dir_all(run_dir.join("versions/26.2")).unwrap();
        let err = check_pin(&fixtures, &run_dir).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "a missing materialized jar must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "a missing materialized jar must exit UNVERIFIED (3), not FAIL (1)"
        );
        let _ = fs::remove_dir_all(&fixtures);
        let _ = fs::remove_dir_all(&run_dir);
    }

    /// A fixture manifest with no `@<commit>` pin cannot be enforced:
    /// UNVERIFIED, not FAIL.
    #[test]
    fn unpinned_manifest_classifies_unverified() {
        let fixtures = fixtures_dir("unpinned", "26.2-DEV-main");
        let run_dir = run_dir_with_materialized_jar("unpinned", Some("0a99345"));
        let err = check_pin(&fixtures, &run_dir).unwrap_err();
        assert!(
            matches!(err, CaptureError::Unverified(_)),
            "an unpinned manifest must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            rivet_harness_common::exit::EXIT_UNVERIFIED,
            "an unpinned manifest must exit UNVERIFIED (3), not FAIL (1)"
        );
        let _ = fs::remove_dir_all(&fixtures);
        let _ = fs::remove_dir_all(&run_dir);
    }
}
