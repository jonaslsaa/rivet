//! Rivet scenario runner — differential harness over Paper and Rivet (issue
//! #155).
//!
//! Boots a real Java Paper server and/or the Rust `rivet-server` headlessly,
//! joins each with the Azalea headless client (`rivet-client`), captures a
//! normalized observable transcript, and compares transcripts with a field-level
//! comparator.
//!
//! Modes (`--server` selects which servers boot, `--pairs` selects the
//! comparison):
//!
//! - `join --server paper --pairs paper:paper` (default): the Paper-vs-Paper
//!   self-check. Boot `--runs` Paper servers, require identical normalized
//!   transcripts, then prove the comparator detects a tampered position
//!   (negative case). Behavior is unchanged from before issue #155.
//! - `move` (Paper-vs-Paper movement self-check, issue #53): boot `--runs`
//!   Paper servers, drive a bounded forward walk through the client's `move`
//!   mode, require identical normalized movement transcripts (per-tick
//!   spawn-relative position deltas, velocity, on-ground, teleport/keepalive
//!   echo relationships), then prove the comparator detects a tampered sampled
//!   position (negative case).
//! - `join --server rivet --pairs paper:rivet` (issue #192): the Rivet
//!   headless-boot play check. Boot `--runs` rivet-servers, wait for the
//!   machine-readable `RIVET_READY` marker, join each with the client, shut
//!   down cleanly on SIGTERM. Requires the pinned Azalea client to complete
//!   offline login, configuration (registry sync), the play handoff, spawn,
//!   and receive exactly the deterministic 117-chunk send-set.
//! - `join --server both --pairs paper:rivet` (issue #192): the Paper-vs-Rivet
//!   play scenario. Boot Paper and Rivet on isolated ports, join each, and
//!   compare the play-state observables. Both reach spawn; the compared
//!   transcripts must diverge only on the excluded per-boot nondeterminism and
//!   the documented Rivet/Paper gaps (spawn height y and the health component
//!   default) — any other divergence FAILS the run. A controlled negative then
//!   tampers the compared `position.y` on the Paper reference and requires the
//!   real comparator/divergence path to report the tampered value, so the live
//!   acceptance cannot pass vacuously.
//!
//! ## Connection proof (Rivet modes)
//!
//! The Rivet modes prove the client actually completed a genuine play session
//! against the Rivet port — they cannot pass against a dead endpoint, a fake
//! `RIVET_READY` binary, or a stale pre-play Rivet build. Two independent
//! observables are required:
//!
//! 1. the rivet-server log contains `connection established` (the per-connection
//!    task logs this on TCP accept) — a line only the real `rivet-server`
//!    binary emits, and
//! 2. the client transcript is judged by [`transcript::rivet_play_verdict`]:
//!    outcome `spawned`, lifecycle containing `login` and `spawn`, the pinned
//!    Azalea build revision, exactly `JOIN_CHUNK_COUNT` (117) chunks, and the
//!    deterministic superflat spawn y `JOIN_SPAWN_Y` (-63.0). A stale pre-play
//!    Rivet build, a fake/non-Rivet endpoint, or a Paper-like y=-60 spawn all
//!    fail the verdict.
//!
//! Raw diagnostics (server logs, client stdout/stderr, normalized transcripts)
//! are preserved under `work/`.
//!
//! Exit codes are machine-stable (consumed by gate.sh):
//!   0  PASS
//!   1  FAIL (scenario comparison failed, negative case failed, harness error)
//!   3  UNVERIFIED (missing prereq — paperclip jar / rivet-server binary — or a
//!      server did not reach READY within its boot timeout)
//!   64 invalid CLI arguments

mod comparator;
mod server;
mod transcript;

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde_json::{Value, json};

const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const DEFAULT_RUNS: usize = 2;

// Machine-stable exit codes (see module doc).
const EXIT_OK: u8 = 0;
const EXIT_FAIL: u8 = 1;
const EXIT_UNVERIFIED: u8 = 3;
const EXIT_USAGE: u8 = 64;

#[derive(Debug)]
enum RunnerError {
    Io(io::Error),
    Server(server::Error),
    Json(serde_json::Error),
    Transcript(String),
    Gate(String),
    Unverified(String),
}

impl RunnerError {
    fn exit_code(&self) -> u8 {
        match self {
            RunnerError::Unverified(_) => EXIT_UNVERIFIED,
            RunnerError::Server(server::Error::Unverified(_)) => EXIT_UNVERIFIED,
            _ => EXIT_FAIL,
        }
    }
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunnerError::Io(e) => write!(f, "io error: {e}"),
            RunnerError::Server(e) => write!(f, "server error: {e}"),
            RunnerError::Json(e) => write!(f, "json error: {e}"),
            RunnerError::Transcript(m) => write!(f, "transcript error: {m}"),
            RunnerError::Gate(m) => write!(f, "{m}"),
            RunnerError::Unverified(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for RunnerError {}

impl From<io::Error> for RunnerError {
    fn from(e: io::Error) -> Self {
        RunnerError::Io(e)
    }
}
impl From<server::Error> for RunnerError {
    fn from(e: server::Error) -> Self {
        RunnerError::Server(e)
    }
}
impl From<serde_json::Error> for RunnerError {
    fn from(e: serde_json::Error) -> Self {
        RunnerError::Json(e)
    }
}
impl From<String> for RunnerError {
    fn from(e: String) -> Self {
        RunnerError::Transcript(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Join,
    Move,
    Capture,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerSelection {
    Paper,
    Rivet,
    Both,
}

impl ServerSelection {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "paper" => Some(ServerSelection::Paper),
            "rivet" => Some(ServerSelection::Rivet),
            "both" => Some(ServerSelection::Both),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            ServerSelection::Paper => "paper",
            ServerSelection::Rivet => "rivet",
            ServerSelection::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pairs {
    PaperPaper,
    PaperRivet,
}

impl Pairs {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "paper:paper" => Some(Pairs::PaperPaper),
            "paper:rivet" => Some(Pairs::PaperRivet),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Pairs::PaperPaper => "paper:paper",
            Pairs::PaperRivet => "paper:rivet",
        }
    }
}

struct Args {
    command: Subcommand,
    server: ServerSelection,
    pairs: Pairs,
    address: String,
    username: String,
    timeout_seconds: u64,
    runs: usize,
}

impl Args {
    fn parse() -> Result<Self, String> {
        Self::parse_from(env::args().skip(1))
    }

    fn parse_from(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = args.peekable();
        let mut command = Subcommand::Help;
        let mut server = ServerSelection::Paper;
        let mut pairs = Pairs::PaperPaper;
        let mut address = DEFAULT_ADDRESS.to_owned();
        let mut username = DEFAULT_USERNAME.to_owned();
        let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
        let mut runs = DEFAULT_RUNS;
        let mut runs_explicit = false;

        if let Some(sub) = args.next() {
            command = match sub.as_str() {
                "join" => Subcommand::Join,
                "move" => Subcommand::Move,
                "capture" => Subcommand::Capture,
                "--help" | "-h" | "help" => Subcommand::Help,
                _ => return Err(format!("unknown subcommand: {sub}\n\n{}", usage())),
            };
        }

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--address" => address = next_value(&mut args, "--address")?,
                "--username" => username = next_value(&mut args, "--username")?,
                "--timeout-seconds" => {
                    let v = next_value(&mut args, "--timeout-seconds")?;
                    timeout_seconds = v
                        .parse()
                        .map_err(|_| format!("invalid --timeout-seconds value: {v}"))?;
                }
                "--runs" => {
                    let v = next_value(&mut args, "--runs")?;
                    runs = v
                        .parse()
                        .map_err(|_| format!("invalid --runs value: {v}"))?;
                    runs_explicit = true;
                }
                "--server" => {
                    let v = next_value(&mut args, "--server")?;
                    server = ServerSelection::parse(&v).ok_or_else(|| {
                        format!("invalid --server value: {v} (expected paper|rivet|both)")
                    })?;
                }
                "--pairs" => {
                    let v = next_value(&mut args, "--pairs")?;
                    pairs = Pairs::parse(&v).ok_or_else(|| {
                        format!("invalid --pairs value: {v} (expected paper:paper|paper:rivet)")
                    })?;
                }
                _ => return Err(format!("unknown argument: {argument}\n\n{}", usage())),
            }
        }

        if command == Subcommand::Join {
            // Valid --server/--pairs combinations: paper:paper needs a Paper
            // boot; paper:rivet needs a Rivet boot (and a Paper reference when
            // `both`). `capture` only uses `--server` (which kind to boot once),
            // so these checks are join-only.
            match (server, pairs) {
                (ServerSelection::Paper, Pairs::PaperPaper) => {}
                (ServerSelection::Rivet, Pairs::PaperRivet)
                | (ServerSelection::Both, Pairs::PaperRivet) => {}
                (server, pairs) => {
                    return Err(format!(
                        "invalid --server/--pairs combination: --server {} cannot run --pairs {}",
                        server.as_str(),
                        pairs.as_str()
                    ));
                }
            }

            if server == ServerSelection::Both && runs_explicit {
                return Err(
                    "--server both always boots exactly one Paper + one Rivet, so --runs is a \
                     silent no-op; drop it (or use --server paper/--server rivet for a run count)"
                        .to_owned(),
                );
            }

            match (server, runs) {
                (ServerSelection::Paper, 0..=1) => {
                    return Err(
                        "--runs must be at least 2 (Paper-vs-Paper needs a pair)".to_owned()
                    );
                }
                (ServerSelection::Rivet, 0) => {
                    return Err("--runs must be at least 1 for --server rivet".to_owned());
                }
                _ => {}
            }
        }

        if command == Subcommand::Move {
            // `move` is a Paper-vs-Paper movement self-check (issue #53): it
            // always boots Paper and compares Paper movement transcripts.
            match (server, pairs) {
                (ServerSelection::Paper, Pairs::PaperPaper) => {}
                (server, pairs) => {
                    return Err(format!(
                        "move only supports --server paper with --pairs paper:paper (got \
                         --server {} --pairs {})",
                        server.as_str(),
                        pairs.as_str()
                    ));
                }
            }
            if runs <= 1 {
                return Err(
                    "--runs must be at least 2 for move (Paper-vs-Paper needs a pair)".to_owned(),
                );
            }
        }

        Ok(Self {
            command,
            server,
            pairs,
            address,
            username,
            timeout_seconds,
            runs,
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn usage() -> String {
    format!(
        "Usage: run-scenario <join|move|capture> [options]\n\
         Options:\n\
         \x20 --server paper|rivet|both  which servers to boot (default paper)\n\
         \x20 --pairs paper:paper|paper:rivet\n\
         \x20                            comparison to run (default paper:paper)\n\
         \x20 --address HOST:PORT        server address (default {DEFAULT_ADDRESS})\n\
         \x20 --username NAME            offline account name (default {DEFAULT_USERNAME})\n\
         \x20 --timeout-seconds N        client timeout per run (default {DEFAULT_TIMEOUT_SECONDS})\n\
         \x20 --runs N                   boots to compare (default {DEFAULT_RUNS}; paper needs >=2)"
    )
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Path to the `rivet-client` binary, assumed to sit next to this binary in
/// the same Cargo target dir.
fn client_binary() -> PathBuf {
    let sibling = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("rivet-client")));
    if let Some(p) = sibling
        && p.is_file()
    {
        return p;
    }
    // Fall back to the package's own target dir (cargo run from this crate).
    crate_root().join("target/debug/rivet-client")
}

/// Resolve `--address` into the first socket address. The port is the
/// port-isolation base. Accepts hostnames (e.g. `localhost:25599`), not just
/// numeric IPs — the pre-#155 client accepted any `HOST:PORT` and resolved it;
/// only the isolated-port modes replace the host with `127.0.0.1`.
fn base_address(args: &Args) -> Result<SocketAddr, RunnerError> {
    args.address
        .to_socket_addrs()
        .map_err(|e| {
            RunnerError::Gate(format!(
                "invalid --address: {} (expected HOST:PORT): {e}",
                args.address
            ))
        })?
        .next()
        .ok_or_else(|| {
            RunnerError::Gate(format!(
                "invalid --address: {} resolved to no addresses",
                args.address
            ))
        })
}

/// Reserve `n` distinct ephemeral loopback ports by binding `n` listeners
/// simultaneously (so the OS cannot hand out the same port twice), reading
/// their addresses, and dropping the listeners. The small bind-drop-boot race
/// is standard practice for test harnesses; a collision surfaces as a loud
/// UNVERIFIED bind failure, never as silent cross-talk between servers.
fn reserve_ports(n: usize) -> Result<Vec<u16>, RunnerError> {
    let mut listeners = Vec::with_capacity(n);
    let mut ports = Vec::with_capacity(n);
    for _ in 0..n {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| RunnerError::Gate(format!("failed to reserve an ephemeral port: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| RunnerError::Gate(format!("failed to read an ephemeral port: {e}")))?
            .port();
        ports.push(port);
        listeners.push(listener);
    }
    drop(listeners);
    Ok(ports)
}

struct ClientRun {
    stdout_text: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

/// Run the headless client once and preserve its raw stdout/stderr.
fn run_client(
    binary: &Path,
    address: &str,
    username: &str,
    timeout_seconds: u64,
    work: &Path,
    prefix: &str,
    mode: &str,
) -> Result<ClientRun, RunnerError> {
    let stdout_path = work.join(format!("{prefix}.stdout.jsonl"));
    let stderr_path = work.join(format!("{prefix}.stderr.log"));
    let output = Command::new(binary)
        .args([
            "--mode",
            mode,
            "--address",
            address,
            "--username",
            username,
            "--timeout-seconds",
            &timeout_seconds.to_string(),
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            RunnerError::Gate(format!(
                "failed to run rivet-client ({}): {e} — build it first with: cargo build --locked",
                binary.display()
            ))
        })?;
    fs::write(&stdout_path, &output.stdout)?;
    fs::write(&stderr_path, &output.stderr)?;
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        println!(
            "    rivet-client exited with code {code} (raw transcript in {}, stderr in {})",
            stdout_path.display(),
            stderr_path.display()
        );
    }
    Ok(ClientRun {
        stdout_text: String::from_utf8_lossy(&output.stdout).into_owned(),
        stdout_path,
        stderr_path,
    })
}

/// Verify the rivet-server log shows the client was actually accepted: the
/// per-connection task logs `connection established` on TCP accept.
///
/// This is the genuinely Rivet-specific half of the connection proof. The
/// client transcript alone cannot prove the client reached the Rivet port:
/// azalea fires `Event::Init` before any TCP connect, and a live-but-hung peer
/// could still appear to make progress. Only the real rivet-server binary emits
/// this line for a genuine exchange, so requiring it kills the false-green (a
/// dead endpoint, a hung port, or a fake `RIVET_READY` binary). The play-side
/// half of the proof — that the client completed login/configuration into
/// spawn and received the deterministic 117-chunk send-set — is judged by
/// [`transcript::rivet_play_verdict`] on the client transcript.
fn verify_rivet_connection(log_path: &Path) -> Result<(), RunnerError> {
    let text = fs::read_to_string(log_path)?;
    if !text.contains("connection established") {
        return Err(RunnerError::Gate(format!(
            "rivet log {} shows no accepted connection — the client did not reach the Rivet port",
            log_path.display()
        )));
    }
    Ok(())
}

/// Fixtures `server.properties` (seed 42, superflat, offline, port 25599) —
/// the config source of truth for Paper boots. Rivet boots do not require it:
/// `rivet-server` is driven purely by `--host`/`--port`.
fn server_properties(crate_root: &Path) -> Result<PathBuf, RunnerError> {
    let p = crate_root.join("../rivet-oracle/fixtures/server.properties");
    if p.is_file() {
        Ok(p)
    } else {
        Err(RunnerError::Unverified(format!(
            "server.properties not found at {} (rivet-oracle fixtures)",
            p.display()
        )))
    }
}

fn ensure_client_binary() -> Result<PathBuf, RunnerError> {
    let bin = client_binary();
    if bin.is_file() {
        Ok(bin)
    } else {
        Err(RunnerError::Unverified(format!(
            "rivet-client binary not found at {} — build it first: cargo build --locked",
            bin.display()
        )))
    }
}

/// Boot one Paper server, join via the client, shut the server down, and return
/// the normalized transcript (raw artifacts preserved). The Paper-vs-Paper
/// self-check path; `address` is the isolated port for this boot.
fn one_join(
    work: &Path,
    jar: &Path,
    server_properties: &Path,
    client_bin: &Path,
    args: &Args,
    idx: usize,
    address: SocketAddr,
) -> Result<Value, RunnerError> {
    let run_dir = work.join(format!("run{idx}"));
    let log_path = work.join(format!("boot{idx}.log"));
    println!("[boot {idx}] fresh Paper world in {}", run_dir.display());
    let mut srv = server::boot(
        server::ServerKind::Paper,
        &run_dir,
        &log_path,
        jar,
        Some(server_properties),
        address,
    )?;
    println!("[run  {idx}] joining via rivet-client ...");
    let client_run = run_client(
        client_bin,
        &address.to_string(),
        &args.username,
        args.timeout_seconds,
        work,
        &format!("client{idx}"),
        "join",
    )?;
    server::shutdown(&mut srv)?;

    let normalized =
        transcript::normalize_join(&client_run.stdout_text).map_err(RunnerError::Transcript)?;
    let transcript_path = work.join(format!("transcript{idx}.json"));
    fs::write(&transcript_path, serde_json::to_string_pretty(&normalized)?)?;
    println!(
        "[run  {idx}] outcome={} (transcript in {})",
        normalized["outcome"],
        transcript_path.display()
    );
    if normalized["outcome"] != "spawned" {
        return Err(RunnerError::Gate(format!(
            "run {idx} did not spawn (outcome={}) — refusing to compare. Raw transcript: {}, stderr: {}",
            normalized["outcome"],
            client_run.stdout_path.display(),
            client_run.stderr_path.display()
        )));
    }
    Ok(normalized)
}

/// Mode A: the Paper-vs-Paper self-check (current behavior, unchanged).
fn run_paper_self_check(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-join");
    fs::create_dir_all(&work)?;
    let server_properties = server_properties(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: join (Paper-vs-Paper self-check)");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!("    server.properties : {}", server_properties.display());
    println!("    address           : {}", args.address);
    println!("    paper boots       : {}", args.runs);
    println!();

    let mut transcripts = Vec::with_capacity(args.runs);
    for idx in 1..=args.runs {
        // Sequential boots, so every Paper boot reuses the base port — exactly
        // the pre-#155 behavior.
        let t = one_join(
            &work,
            &jar,
            &server_properties,
            &client_bin,
            args,
            idx,
            base,
        )?;
        transcripts.push(t);
    }

    // Paper-vs-Paper: every consecutive pair must be byte-for-byte identical.
    println!();
    println!("Paper-vs-Paper comparison ({} boots)", args.runs);
    let mut identical = true;
    for (i, pair) in transcripts.windows(2).enumerate() {
        let boot_a = i + 1;
        let boot_b = i + 2;
        let d = comparator::diff(&pair[0], &pair[1]);
        if d.is_identical() {
            println!("    boot {boot_a} vs boot {boot_b}: IDENTICAL");
            if !d.excluded.is_empty() {
                println!("      excluded from parity: {}", d.excluded.len());
                for f in &d.excluded {
                    println!("        {f}");
                }
            }
            if !d.excluded_policy_diffs.is_empty() {
                identical = false;
                println!("      WARNING: exclusion policy differs between runs:");
                for f in &d.excluded_policy_diffs {
                    println!("        {f}");
                }
            }
        } else {
            identical = false;
            println!(
                "    boot {boot_a} vs boot {boot_b}: DIFFERS ({} field(s))",
                d.diffs.len()
            );
            for f in &d.diffs {
                println!("      {f}");
            }
        }
    }

    // Negative case: tamper a *compared* field (position.y, the deterministic
    // superflat spawn height) and require the comparator to detect it — the
    // harness must not pass vacuously.
    println!();
    println!("Negative case (tamper boot 1 position.y)");
    let reference = &transcripts[0];
    let mut tampered = reference.clone();
    match tampered["position"]["y"].as_f64() {
        Some(y) => {
            tampered["position"]["y"] = json!(y + 1.0);
            let neg = comparator::diff(reference, &tampered);
            if neg.is_identical() {
                return Err(RunnerError::Gate(
                    "negative case FAILED: comparator did not detect a tampered position"
                        .to_owned(),
                ));
            }
            println!(
                "    tampered position.y += 1 -> detected {} field diff(s):",
                neg.diffs.len()
            );
            for f in &neg.diffs {
                println!("      {f}");
            }
        }
        None => {
            return Err(RunnerError::Gate(
                "negative case FAILED: reference transcript has no position to tamper".to_owned(),
            ));
        }
    }

    println!();
    println!("Reference transcript (boot 1):");
    println!(
        "{}",
        serde_json::to_string_pretty(reference).expect("transcript serializes")
    );
    println!();

    if identical {
        println!(
            "VERDICT: PASS — all {} Paper boots produced identical normalized transcripts; the \
             negative case confirmed the comparator detects a known difference.",
            args.runs
        );
        println!("    artifacts (raw logs, transcripts): {}", work.display());
        Ok(())
    } else {
        println!("VERDICT: FAIL — Paper-vs-Paper transcripts differ.");
        println!("    artifacts (for diagnosis): {}", work.display());
        Err(RunnerError::Gate(
            "Paper-vs-Paper comparison failed".to_owned(),
        ))
    }
}

/// Boot one Paper server, drive the client's `move` mode (a bounded forward
/// walk sampling per-tick position/velocity/on-ground plus the teleport and
/// keepalive echoes), shut the server down, and return the normalized movement
/// transcript (raw artifacts preserved).
fn one_move(
    work: &Path,
    jar: &Path,
    server_properties: &Path,
    client_bin: &Path,
    args: &Args,
    idx: usize,
    address: SocketAddr,
) -> Result<Value, RunnerError> {
    let run_dir = work.join(format!("run{idx}"));
    let log_path = work.join(format!("boot{idx}.log"));
    println!("[boot {idx}] fresh Paper world in {}", run_dir.display());
    let mut srv = server::boot(
        server::ServerKind::Paper,
        &run_dir,
        &log_path,
        jar,
        Some(server_properties),
        address,
    )?;
    println!("[run  {idx}] walking via rivet-client (move mode) ...");
    let client_run = run_client(
        client_bin,
        &address.to_string(),
        &args.username,
        args.timeout_seconds,
        work,
        &format!("client{idx}"),
        "move",
    )?;
    server::shutdown(&mut srv)?;

    let normalized =
        transcript::normalize_move(&client_run.stdout_text).map_err(RunnerError::Transcript)?;
    let transcript_path = work.join(format!("transcript{idx}.json"));
    fs::write(&transcript_path, serde_json::to_string_pretty(&normalized)?)?;
    println!(
        "[run  {idx}] outcome={} (transcript in {})",
        normalized["outcome"],
        transcript_path.display()
    );
    if normalized["outcome"] != "moved" {
        return Err(RunnerError::Gate(format!(
            "run {idx} did not move (outcome={}) — refusing to compare. Raw transcript: {}, stderr: {}",
            normalized["outcome"],
            client_run.stdout_path.display(),
            client_run.stderr_path.display()
        )));
    }
    Ok(normalized)
}

/// The `move` scenario: Paper-vs-Paper movement self-check (issue #53).
///
/// Boots `--runs` Paper servers, drives the client's bounded forward walk
/// against each, requires identical normalized movement transcripts (per-tick
/// spawn-relative position deltas, velocity, on-ground, the teleport/keepalive
/// echo relationships), then proves the comparator detects a tampered sampled
/// position (negative case).
fn run_move_self_check(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-move");
    fs::create_dir_all(&work)?;
    let server_properties = server_properties(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: move (Paper-vs-Paper movement self-check)");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!("    server.properties : {}", server_properties.display());
    println!("    address           : {}", args.address);
    println!("    paper boots       : {}", args.runs);
    println!();

    let mut transcripts = Vec::with_capacity(args.runs);
    for idx in 1..=args.runs {
        let t = one_move(
            &work,
            &jar,
            &server_properties,
            &client_bin,
            args,
            idx,
            base,
        )?;
        transcripts.push(t);
    }

    // Paper-vs-Paper: every consecutive pair must be byte-for-byte identical.
    println!();
    println!("Paper-vs-Paper movement comparison ({} boots)", args.runs);
    let mut identical = true;
    for (i, pair) in transcripts.windows(2).enumerate() {
        let boot_a = i + 1;
        let boot_b = i + 2;
        let d = comparator::diff(&pair[0], &pair[1]);
        if d.is_identical() {
            println!("    boot {boot_a} vs boot {boot_b}: IDENTICAL");
            if !d.excluded.is_empty() {
                println!("      excluded from parity: {}", d.excluded.len());
                for f in &d.excluded {
                    println!("        {f}");
                }
            }
            if !d.excluded_policy_diffs.is_empty() {
                identical = false;
                println!("      WARNING: exclusion policy differs between runs:");
                for f in &d.excluded_policy_diffs {
                    println!("        {f}");
                }
            }
        } else {
            identical = false;
            println!(
                "    boot {boot_a} vs boot {boot_b}: DIFFERS ({} field(s))",
                d.diffs.len()
            );
            for f in &d.diffs {
                println!("      {f}");
            }
        }
    }

    // Negative case: tamper a *compared* sampled position (walk.samples[60].dx,
    // the midpoint of the walk) and require the comparator to detect it — the
    // harness must not pass vacuously.
    println!();
    println!("Negative case (tamper boot 1 sampled position walk.samples[60].dx)");
    let reference = &transcripts[0];
    let mut tampered = reference.clone();
    match tampered["walk"]["samples"]
        .get(60)
        .and_then(|sample| sample.get("dx"))
        .and_then(Value::as_f64)
    {
        Some(dx) => {
            tampered["walk"]["samples"][60]["dx"] = json!(dx + 0.5);
            let neg = comparator::diff(reference, &tampered);
            if neg.is_identical() {
                return Err(RunnerError::Gate(
                    "negative case FAILED: comparator did not detect a tampered sampled position"
                        .to_owned(),
                ));
            }
            println!(
                "    tampered walk.samples[60].dx += 0.5 -> detected {} field diff(s):",
                neg.diffs.len()
            );
            for f in &neg.diffs {
                println!("      {f}");
            }
        }
        None => {
            return Err(RunnerError::Gate(
                "negative case FAILED: reference transcript has no midpoint sample to tamper"
                    .to_owned(),
            ));
        }
    }

    println!();
    println!("Reference transcript (boot 1):");
    println!(
        "{}",
        serde_json::to_string_pretty(reference).expect("transcript serializes")
    );
    println!();

    if identical {
        println!(
            "VERDICT: PASS — all {} Paper boots produced identical normalized movement \
             transcripts; the negative case confirmed the comparator detects a known difference.",
            args.runs
        );
        println!("    artifacts (raw logs, transcripts): {}", work.display());
        Ok(())
    } else {
        println!("VERDICT: FAIL — Paper-vs-Paper movement transcripts differ.");
        println!("    artifacts (for diagnosis): {}", work.display());
        Err(RunnerError::Gate(
            "Paper-vs-Paper movement comparison failed".to_owned(),
        ))
    }
}

/// Mode B: Rivet headless boot + play transcript (issue #192).
fn run_rivet_play(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-rivet");
    fs::create_dir_all(&work)?;
    // Rivet does not need server.properties: the binary is driven purely by
    // --host/--port. Fetching it here would let a missing fixture spuriously
    // UNVERIFIED a Rivet-only run.
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: rivet (headless boot + play transcript)");
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!("    address           : {}", args.address);
    println!("    rivet boots       : {}", args.runs);
    println!();

    let mut transcripts = Vec::with_capacity(args.runs);
    for idx in 1..=args.runs {
        let run_dir = work.join(format!("run{idx}"));
        let log_path = work.join(format!("rivet{idx}.log"));
        let prefix = format!("rivet{idx}");
        println!(
            "[boot {idx}] headless rivet-server in {}",
            run_dir.display()
        );
        let mut srv = server::boot(
            server::ServerKind::Rivet,
            &run_dir,
            &log_path,
            &rivet_bin,
            None,
            base,
        )?;
        println!("[run  {idx}] connecting via rivet-client ...");
        let client_run = run_client(
            &client_bin,
            &base.to_string(),
            &args.username,
            args.timeout_seconds,
            &work,
            &prefix,
            "join",
        )?;
        server::shutdown(&mut srv)?;
        // The client transcript is only the client-side half of the proof;
        // require the server log to show the real rivet-server accepted the
        // connection (connection established on TCP accept).
        verify_rivet_connection(&log_path)?;

        let normalized =
            transcript::normalize_join(&client_run.stdout_text).map_err(RunnerError::Transcript)?;
        let transcript_path = work.join(format!("{prefix}.transcript.json"));
        fs::write(&transcript_path, serde_json::to_string_pretty(&normalized)?)?;
        let boundary = transcript::rivet_play_verdict(&normalized)?;
        println!(
            "[run  {idx}] outcome={} lifecycle={:?} chunk_count={} (play boundary: {boundary}) — transcript in {}",
            normalized["outcome"],
            normalized["lifecycle"],
            normalized["chunk_count"],
            transcript_path.display()
        );
        transcripts.push(normalized);
    }

    println!();
    println!("Rivet play summary ({} boots)", args.runs);
    println!(
        "    {}/{}\tRivet boots reached RIVET_READY, accepted a real client connection",
        args.runs, args.runs
    );
    println!(
        "        (server log: 'connection established'), took the pinned Azalea client through"
    );
    println!(
        "        login/configuration into spawn with the deterministic 117-chunk send-set, and"
    );
    println!("        shut down cleanly on SIGTERM");
    if args.runs >= 2 {
        println!("    deterministic Rivet-vs-Rivet self-check:");
        let mut identical = true;
        for (i, pair) in transcripts.windows(2).enumerate() {
            let d = comparator::diff(&pair[0], &pair[1]);
            if d.is_identical() {
                println!("      boot {} vs boot {}: IDENTICAL", i + 1, i + 2);
            } else {
                identical = false;
                println!(
                    "      boot {} vs boot {}: DIFFERS ({} field(s))",
                    i + 1,
                    i + 2,
                    d.diffs.len()
                );
                for f in &d.diffs {
                    println!("        {f}");
                }
            }
        }
        if !identical {
            return Err(RunnerError::Gate(
                "Rivet-vs-Rivet play transcripts differ (expected identical play)".to_owned(),
            ));
        }
    }

    println!();
    println!("VERDICT: PASS — rivet-server boots headlessly, reaches RIVET_READY, and takes the");
    println!("    pinned unmodified Azalea client through offline login, configuration (registry");
    println!("    sync), and the play handoff to spawn, receiving exactly the deterministic");
    println!(
        "    {}​-chunk send-set at Rivet's fixed superflat spawn y={}​. The connection is proven",
        transcript::JOIN_CHUNK_COUNT,
        transcript::JOIN_SPAWN_Y
    );
    println!(
        "    two ways: the rivet log shows 'connection established' (only the real rivet-server"
    );
    println!("    emits it), and the client transcript is outcome=spawned with lifecycle");
    println!("    init->login->spawn, the pinned Azalea revision, 117 chunks, and spawn y=-63.0 —");
    println!("    which a stale pre-play build, a fake/non-Rivet endpoint, or a Paper-like y=-60");
    println!("    spawn all fail.");
    println!("    artifacts: {}", work.display());
    Ok(())
}

/// The compared Paper-vs-Rivet transcripts must diverge only on the documented
/// Rivet/Paper gaps (spawn height y and the health component default); any other
/// divergence is a genuine Rivet/Paper mismatch, not a harness artifact, and
/// fails the run. Normal runs rebuild the server and the fallback has a narrow
/// freshness guard; this behavioral gate remains load-bearing even when an
/// explicit `RIVET_SERVER_BIN` override is used.
fn check_paper_rivet_divergence(d: &comparator::TranscriptDiff) -> Result<(), RunnerError> {
    const DOCUMENTED_GAPS: [&str; 2] = ["position.y", "health.health"];
    for f in &d.diffs {
        if !DOCUMENTED_GAPS.contains(&f.path.as_str()) {
            return Err(RunnerError::Gate(format!(
                "Paper-vs-Rivet divergence on {}: expected {} got {} — not one of the documented \
                 Rivet/Paper gaps ({DOCUMENTED_GAPS:?}); refusing PASS",
                f.path, f.expected, f.actual
            )));
        }
    }
    Ok(())
}

/// Prove the both-mode divergence path is non-vacuous: tamper the compared
/// `position.y` on the Paper reference and re-run the exact comparator +
/// divergence gate the live comparison used, requiring the reported
/// `position.y` divergence to observe the tampered value.
///
/// The live acceptance cannot pass unless a tampered compared field actually
/// flows through the comparator and the divergence gate: a comparator that
/// reports nothing, or a `position.y` silently moved into `excluded`, would
/// PASS vacuously without this negative. (The gate's own rejection of an
/// undocumented divergence is asserted separately by the
/// `paper_rivet_divergence_rejects_an_undocumented_gap` unit test; this
/// negative's job is to prove the comparator path is read, not to re-test the
/// gate's accept/reject decision.)
fn prove_both_mode_non_vacuous(paper_t: &Value, rivet_t: &Value) -> Result<(), RunnerError> {
    let mut tampered = paper_t.clone();
    let y = tampered["position"]["y"].as_f64().ok_or_else(|| {
        RunnerError::Gate(
            "negative case FAILED: paper transcript has no position to tamper".to_owned(),
        )
    })?;
    let rivet_y = rivet_t["position"]["y"].as_f64().ok_or_else(|| {
        RunnerError::Gate(
            "negative case FAILED: rivet transcript has no position to compare against".to_owned(),
        )
    })?;
    // Offset above the larger of the two spawn heights, so the tampered value
    // differs from both paper's and rivet's y for all MC-realistic spawn heights
    // (small magnitudes, where the f64 spacing is far below 1.0). A fixed +1.0
    // on paper's y alone would silently align with rivet's y if the two spawn
    // heights were ever adjacent, and the negative would fail on a healthy tree.
    let tampered_y = y.max(rivet_y) + 1.0;
    tampered["position"]["y"] = json!(tampered_y);
    let neg = comparator::diff(&tampered, rivet_t);
    check_paper_rivet_divergence(&neg)?;
    match neg.diffs.iter().find(|f| f.path == "position.y") {
        Some(f) if f.expected.as_f64() == Some(tampered_y) => {
            println!(
                "    tampered paper position.y {y} -> {tampered_y} — the divergence path reported \
                 'position.y: expected {tampered_y}, got {}', so position.y is genuinely compared \
                 and read by the gate",
                f.actual
            );
            Ok(())
        }
        Some(f) => Err(RunnerError::Gate(format!(
            "negative case FAILED: the divergence path reported 'position.y: expected {} got {}', \
             but paper position.y was tampered to {tampered_y} — the reported divergence must \
             observe the tampered value (position.y must never be excluded or normalized to make \
             the comparison pass)",
            f.expected, f.actual
        ))),
        None => Err(RunnerError::Gate(
            "negative case FAILED: tampering paper position.y produced no compared 'position.y' \
             diff (it is excluded, absent, or the comparator reported nothing) — position.y must \
             be a compared field read by the divergence gate"
                .to_owned(),
        )),
    }
}

/// Mode C: Paper-vs-Rivet play scenario (issue #192). Both servers must take
/// the pinned Azalea client through login/configuration into spawn; the
/// transcripts are compared field-level and differ only on the excluded
/// per-boot nondeterminism and the documented Rivet/Paper gaps.
fn run_paper_vs_rivet(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-both");
    fs::create_dir_all(&work)?;
    let server_properties = server_properties(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    // Port isolation: Paper and Rivet get distinct ephemeral ports, so the
    // client provably targets the server the harness points it at (no two
    // servers in one scenario can collide).
    let ports = reserve_ports(2)?;
    let paper_addr = SocketAddr::new(base.ip(), ports[0]);
    let rivet_addr = SocketAddr::new(base.ip(), ports[1]);

    println!("rivet scenario runner: join (Paper-vs-Rivet play)");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!("    server.properties : {}", server_properties.display());
    println!("    paper address     : {paper_addr}");
    println!("    rivet address     : {rivet_addr}");
    println!();

    // Paper reference — behavior must be unchanged (reaches spawn).
    let mut paper_srv = server::boot(
        server::ServerKind::Paper,
        &work.join("paper"),
        &work.join("paper.log"),
        &jar,
        Some(&server_properties),
        paper_addr,
    )?;
    let paper_client = run_client(
        &client_bin,
        &paper_addr.to_string(),
        &args.username,
        args.timeout_seconds,
        &work,
        "paper",
        "join",
    )?;
    server::shutdown(&mut paper_srv)?;
    let paper_t =
        transcript::normalize_join(&paper_client.stdout_text).map_err(RunnerError::Transcript)?;
    let paper_tp = work.join("paper.transcript.json");
    fs::write(&paper_tp, serde_json::to_string_pretty(&paper_t)?)?;
    println!(
        "    Paper outcome      : {} (transcript in {})",
        paper_t["outcome"],
        paper_tp.display()
    );
    if paper_t["outcome"] != "spawned" {
        return Err(RunnerError::Gate(format!(
            "Paper did not spawn (outcome={}) — regression in the reference server; refusing the comparison",
            paper_t["outcome"]
        )));
    }

    // Rivet SUT — must reach READY and take the client through play.
    let mut rivet_srv = server::boot(
        server::ServerKind::Rivet,
        &work.join("rivet"),
        &work.join("rivet.log"),
        &rivet_bin,
        None,
        rivet_addr,
    )?;
    let rivet_client = run_client(
        &client_bin,
        &rivet_addr.to_string(),
        &args.username,
        args.timeout_seconds,
        &work,
        "rivet",
        "join",
    )?;
    server::shutdown(&mut rivet_srv)?;
    // Server-side half of the connection proof: the rivet log must show the
    // real rivet-server accepted the client (connection established on TCP
    // accept).
    verify_rivet_connection(&work.join("rivet.log"))?;
    let rivet_t =
        transcript::normalize_join(&rivet_client.stdout_text).map_err(RunnerError::Transcript)?;
    let rivet_tp = work.join("rivet.transcript.json");
    fs::write(&rivet_tp, serde_json::to_string_pretty(&rivet_t)?)?;
    let boundary = transcript::rivet_play_verdict(&rivet_t)?;
    println!(
        "    Rivet outcome      : {} chunk_count={} (transcript in {})",
        rivet_t["outcome"],
        rivet_t["chunk_count"],
        rivet_tp.display()
    );

    println!();
    println!("Paper-vs-Rivet comparator (play-state divergence):");
    let d = comparator::diff(&paper_t, &rivet_t);
    check_paper_rivet_divergence(&d)?;
    println!(
        "    {} field(s) differ — the documented Rivet/Paper gaps (spawn height y and the",
        d.diffs.len()
    );
    println!("    health component default), plus the excluded per-boot nondeterminism:");
    for f in &d.diffs {
        println!("        {f}");
    }

    // Negative case: prove the divergence path just exercised is non-vacuous.
    // Tamper a *compared* field (position.y) on the Paper reference and require
    // the real comparator/divergence path to detect the expected named mismatch.
    println!();
    println!("Negative case (tamper paper position.y through the real divergence path)");
    prove_both_mode_non_vacuous(&paper_t, &rivet_t)?;

    println!();
    println!("VERDICT: PASS — both servers took the pinned Azalea client through login and");
    println!("    configuration into play:");
    println!("      * Paper reached spawn (reference behavior unchanged).");
    println!(
        "      * Rivet reached RIVET_READY on its own isolated port ({rivet_addr}) and took the"
    );
    println!("        client through the {boundary}.");
    println!("      * The connection is proven two ways: the rivet log shows 'connection");
    println!(
        "        established' (only the real rivet-server emits it), and the client transcript"
    );
    println!("        is outcome=spawned with the pinned Azalea revision, 117 chunks, and spawn");
    println!("        y=-63.0 — which a stale pre-play build, a fake/non-Rivet endpoint, or a");
    println!("        Paper-like y=-60 spawn all fail.");
    println!("      * The compared transcripts differ only on the documented Rivet/Paper gaps");
    println!("        (position.y: Rivet superflat y=-63 vs Paper y=-60; health default: Rivet");
    println!(
        "        omits set_health so azalea reports 1.0 vs Paper's 20.0) — any other divergence"
    );
    println!("        fails the run, so a Paper-vs-Rivet regression cannot pass as 'expected'.");
    println!("      * The negative case proved the divergence path is non-vacuous: a tampered");
    println!("        compared position.y on the Paper reference was reported by the real");
    println!("        comparator/divergence path, so the acceptance cannot pass while ignoring");
    println!("        a changed compared field.");
    println!("    artifacts: {}", work.display());
    Ok(())
}

fn run_join(args: &Args) -> Result<(), RunnerError> {
    match (args.server, args.pairs) {
        (ServerSelection::Paper, Pairs::PaperPaper) => run_paper_self_check(args),
        (ServerSelection::Rivet, Pairs::PaperRivet) => run_rivet_play(args),
        (ServerSelection::Both, Pairs::PaperRivet) => run_paper_vs_rivet(args),
        (server, pairs) => Err(RunnerError::Gate(format!(
            "unhandled --server {} / --pairs {} combination",
            server.as_str(),
            pairs.as_str()
        ))),
    }
}

fn run_capture(args: &Args) -> Result<(), RunnerError> {
    let kind = match args.server {
        ServerSelection::Paper => server::ServerKind::Paper,
        ServerSelection::Rivet => server::ServerKind::Rivet,
        ServerSelection::Both => {
            return Err(RunnerError::Gate(
                "capture does not support --server both".to_owned(),
            ));
        }
    };
    let crate_root = crate_root();
    let work = crate_root.join(if kind == server::ServerKind::Paper {
        "work/scenario-join"
    } else {
        "work/scenario-rivet"
    });
    fs::create_dir_all(&work)?;
    let base = base_address(args)?;
    let artifact = match kind {
        server::ServerKind::Paper => server::ensure_jar(&crate_root)?,
        server::ServerKind::Rivet => server::ensure_rivet_binary(&crate_root)?,
    };
    let client_bin = ensure_client_binary()?;
    // Rivet does not need server.properties (driven purely by --host/--port);
    // only Paper's boot consumes the fixture.
    let server_properties = (kind == server::ServerKind::Paper)
        .then(|| server_properties(&crate_root))
        .transpose()?;

    let prefix = kind.as_str().to_owned();
    let mut srv = server::boot(
        kind,
        &work.join(format!("{prefix}1")),
        &work.join(format!("{prefix}1.log")),
        &artifact,
        server_properties.as_deref(),
        base,
    )?;
    let client_run = run_client(
        &client_bin,
        &base.to_string(),
        &args.username,
        args.timeout_seconds,
        &work,
        "client1",
        "join",
    )?;
    server::shutdown(&mut srv)?;

    let normalized =
        transcript::normalize_join(&client_run.stdout_text).map_err(RunnerError::Transcript)?;
    println!();
    println!("Normalized transcript:");
    println!("{}", serde_json::to_string_pretty(&normalized)?);
    Ok(())
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let result = match args.command {
        Subcommand::Join => run_join(&args),
        Subcommand::Move => run_move_self_check(&args),
        Subcommand::Capture => run_capture(&args),
        Subcommand::Help => {
            println!("{}", usage());
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(e) => {
            eprintln!("run-scenario: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Result<Args, String> {
        Args::parse_from(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_are_paper_self_check() {
        let a = parse(&["join"]).unwrap();
        assert_eq!(a.command, Subcommand::Join);
        assert_eq!(a.server, ServerSelection::Paper);
        assert_eq!(a.pairs, Pairs::PaperPaper);
        assert_eq!(a.address, DEFAULT_ADDRESS);
        assert_eq!(a.runs, DEFAULT_RUNS);
    }

    #[test]
    fn accepts_rivet_and_both_servers() {
        let a = parse(&["join", "--server", "rivet", "--pairs", "paper:rivet"]).unwrap();
        assert_eq!(a.server, ServerSelection::Rivet);
        assert_eq!(a.pairs, Pairs::PaperRivet);
        let a = parse(&["join", "--server", "both", "--pairs", "paper:rivet"]).unwrap();
        assert_eq!(a.server, ServerSelection::Both);
        assert_eq!(a.pairs, Pairs::PaperRivet);
    }

    #[test]
    fn move_parses_and_defaults_to_paper_self_check() {
        let a = parse(&["move"]).unwrap();
        assert_eq!(a.command, Subcommand::Move);
        assert_eq!(a.server, ServerSelection::Paper);
        assert_eq!(a.pairs, Pairs::PaperPaper);
    }

    #[test]
    fn move_rejects_non_paper_configuration() {
        // `move` is a Paper-vs-Paper movement self-check: it must reject any
        // server/pairs combination that would not compare Paper against Paper.
        assert!(parse(&["move", "--server", "rivet"]).is_err());
        assert!(parse(&["move", "--pairs", "paper:rivet"]).is_err());
    }

    #[test]
    fn move_requires_two_runs() {
        assert!(parse(&["move"]).is_ok());
        assert!(parse(&["move", "--runs", "1"]).is_err(), "move needs >=2");
    }

    #[test]
    fn rejects_invalid_server_pairs_combinations() {
        assert!(parse(&["join", "--server", "rivet", "--pairs", "paper:paper"]).is_err());
        assert!(parse(&["join", "--server", "both", "--pairs", "paper:paper"]).is_err());
        assert!(parse(&["join", "--server", "paper", "--pairs", "paper:rivet"]).is_err());
        assert!(parse(&["join", "--server", "nope"]).is_err());
        assert!(parse(&["join", "--pairs", "rivet:rivet"]).is_err());
    }

    #[test]
    fn runs_validation_per_mode() {
        assert!(parse(&["join"]).is_ok());
        assert!(parse(&["join", "--runs", "1"]).is_err(), "paper needs >=2");
        assert!(
            parse(&[
                "join",
                "--server",
                "rivet",
                "--pairs",
                "paper:rivet",
                "--runs",
                "1"
            ])
            .is_ok(),
            "rivet needs >=1"
        );
        assert!(
            parse(&[
                "join",
                "--server",
                "rivet",
                "--pairs",
                "paper:rivet",
                "--runs",
                "0"
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_explicit_runs_for_server_both() {
        // `both` always boots exactly one Paper + one Rivet; an explicit --runs
        // would be a silent no-op.
        let a = parse(&["join", "--server", "both", "--pairs", "paper:rivet"]);
        assert!(a.is_ok(), "default runs is fine for both");
        assert!(
            parse(&[
                "join",
                "--server",
                "both",
                "--pairs",
                "paper:rivet",
                "--runs",
                "2"
            ])
            .is_err(),
            "explicit --runs with both must be rejected"
        );
    }

    #[test]
    fn base_address_accepts_a_hostname() {
        // Pre-#155 the raw --address string went straight to the client, which
        // resolves hostnames; base_address must keep that working (only the
        // isolated-port modes replace the host with 127.0.0.1).
        let args = Args {
            command: Subcommand::Join,
            server: ServerSelection::Paper,
            pairs: Pairs::PaperPaper,
            address: "localhost:25599".to_owned(),
            username: DEFAULT_USERNAME.to_owned(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            runs: DEFAULT_RUNS,
        };
        let addr = base_address(&args).expect("hostname resolves");
        assert_eq!(addr.port(), 25599);
        assert!(
            addr.ip().is_loopback(),
            "localhost should resolve to loopback"
        );
    }

    #[test]
    fn reserve_ports_gives_distinct_usable_ports() {
        let ports = reserve_ports(3).unwrap();
        let unique: std::collections::BTreeSet<u16> = ports.iter().copied().collect();
        assert_eq!(unique.len(), 3, "ports must be distinct: {ports:?}");
        assert!(
            ports.iter().all(|p| *p > 0),
            "ephemeral ports must be nonzero: {ports:?}"
        );
        // Distinctness is the load-bearing guarantee (two servers in one scenario
        // must never share a port). The bind-drop-boot race is intentionally not
        // asserted here: rebinding after `reserve_ports` drops its listeners races
        // with other tests binding ephemeral ports in parallel.
    }

    #[test]
    fn exit_codes_classify_unverified() {
        assert_eq!(
            RunnerError::Unverified("x".into()).exit_code(),
            EXIT_UNVERIFIED
        );
        assert_eq!(
            RunnerError::Server(server::Error::Unverified("x".into())).exit_code(),
            EXIT_UNVERIFIED
        );
        assert_eq!(RunnerError::Gate("x".into()).exit_code(), EXIT_FAIL);
        assert_eq!(
            RunnerError::Server(server::Error::Gate("x".into())).exit_code(),
            EXIT_FAIL
        );
    }

    fn diff_with(path: &str) -> comparator::TranscriptDiff {
        let mut d = comparator::TranscriptDiff::default();
        d.diffs.push(comparator::FieldDiff {
            path: path.to_owned(),
            expected: json!(null),
            actual: json!(null),
        });
        d
    }

    #[test]
    fn paper_rivet_divergence_accepts_only_documented_gaps() {
        assert!(check_paper_rivet_divergence(&comparator::TranscriptDiff::default()).is_ok());
        assert!(check_paper_rivet_divergence(&diff_with("position.y")).is_ok());
        assert!(check_paper_rivet_divergence(&diff_with("health.health")).is_ok());
    }

    #[test]
    fn paper_rivet_divergence_rejects_an_undocumented_gap() {
        // Counterfactual against a current Rivet whose play state has drifted
        // from Paper on a compared (non-excluded) observable: the divergence is
        // not one of the documented gaps, so the both-mode must FAIL rather
        // than print a PASS while diverging.
        let err = check_paper_rivet_divergence(&diff_with("gamemode")).unwrap_err();
        assert!(
            err.to_string().contains("gamemode"),
            "error must name the diverging field, got {err}"
        );
    }

    /// A join transcript with the deterministic observables set, matching the
    /// shape `normalize_join` produces (position.x/z and chunks excluded).
    fn join_transcript(y: f64, health: f64) -> Value {
        json!({
            "protocol": 1,
            "scenario": "join",
            "outcome": "spawned",
            "lifecycle": ["init", "login", "spawn"],
            "azalea_revision": "6249c295d353b9b3ef68f665b311cba39211fd19",
            "position": { "x": 9.5, "y": y, "z": -3.5 },
            "world": "minecraft:overworld",
            "gamemode": "survival",
            "health": { "health": health, "food": 20, "saturation": 5.0 },
            "chunk_count": 117,
            "chunks": [[-4, -4], [-4, -3], [0, 0]],
            "excluded": {
                "position.x": "randomized per boot",
                "position.z": "randomized per boot",
                "chunks": "centered on the randomized spawn chunk",
            },
        })
    }

    #[test]
    fn both_mode_negative_passes_when_tampered_position_y_is_reported() {
        // The genuine Paper-vs-Rivet shape: the transcripts diverge only on the
        // documented gaps (position.y: Paper -60 / Rivet -63; health.health:
        // Paper 20 / Rivet 1). Tampering Paper's position.y through the real
        // comparator/divergence path must be reported with the tampered value.
        let paper = join_transcript(-60.0, 20.0);
        let rivet = join_transcript(-63.0, 1.0);
        assert!(prove_both_mode_non_vacuous(&paper, &rivet).is_ok());
    }

    #[test]
    fn both_mode_negative_passes_when_spawn_heights_are_adjacent() {
        // Regression for the false-failure mode: with the old fixed +1.0 offset,
        // paper.y = -64 tampered to -63 would collide with rivet.y = -63, produce
        // no position.y diff, and FAIL on a healthy tree. The tamper must be
        // offset above the larger spawn height so it always differs from rivet.
        let paper = join_transcript(-64.0, 20.0);
        let rivet = join_transcript(-63.0, 1.0);
        assert!(prove_both_mode_non_vacuous(&paper, &rivet).is_ok());
    }

    #[test]
    fn both_mode_negative_fails_if_position_y_is_silently_excluded() {
        // If a future edit moves position.y into the `excluded` map (so the
        // comparison could pass by never seeing the tamper), the negative must
        // FAIL: the reported divergence must not observe the tampered value.
        let mut paper = join_transcript(-60.0, 20.0);
        paper["excluded"]["position.y"] = json!("silently dropped from parity");
        let rivet = join_transcript(-63.0, 1.0);
        let err = prove_both_mode_non_vacuous(&paper, &rivet).unwrap_err();
        assert!(
            err.to_string().contains("position.y"),
            "error must name the missing position.y divergence, got {err}"
        );
    }

    #[test]
    fn both_mode_negative_fails_without_a_position_to_tamper() {
        // A transcript with no position (or one stripped of y) cannot prove the
        // comparator reads position.y; the negative must FAIL, not skip.
        let paper = json!({
            "outcome": "spawned",
            "health": { "health": 20.0 },
            "excluded": { "position.y": "normalized away" },
        });
        let rivet = join_transcript(-63.0, 1.0);
        let err = prove_both_mode_non_vacuous(&paper, &rivet).unwrap_err();
        assert!(
            err.to_string().contains("no position to tamper"),
            "error must explain there is no position to tamper, got {err}"
        );
    }
}
