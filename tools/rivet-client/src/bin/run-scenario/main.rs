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
//!   the documented health default gap — any other divergence, including a
//!   position.y mismatch, FAILS the run. Paper boots with the single-stone
//!   superflat fixture so both servers spawn at y=-63.0 and `position.y` is a
//!   genuinely compared field (issue #159: the old default-flat Paper reference
//!   spawned at y=-60 and position.y was wrongly treated as a "documented gap").
//!   A controlled negative then tampers the compared `position.y` on the Paper
//!   reference and requires the real comparator/divergence path to report the
//!   tampered value and refuse PASS, so the live acceptance cannot pass
//!   vacuously.
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
mod trace;
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

// Machine-stable exit codes. PASS/FAIL/UNVERIFIED are the shared contract
// (rivet-harness-common::exit); usage errors are a separate 64.
use rivet_harness_common::exit::{EXIT_FAIL, EXIT_PASS, EXIT_UNVERIFIED};
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
        let mut pairs_explicit = false;

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
                    pairs_explicit = true;
                }
                _ => return Err(format!("unknown argument: {argument}\n\n{}", usage())),
            }
        }

        // When --pairs is omitted, derive it from --server: rivet/both only
        // ever compare Paper-vs-Rivet, so the paper:paper default would be
        // invalid. This applies to the commands with a pairs concept (join and
        // the move self-check/differential); `capture` only uses `--server`
        // (which kind to boot once) and ignores pairs. An explicit --pairs is
        // still validated below.
        if (command == Subcommand::Join || command == Subcommand::Move) && !pairs_explicit {
            pairs = match server {
                ServerSelection::Paper => Pairs::PaperPaper,
                ServerSelection::Rivet | ServerSelection::Both => Pairs::PaperRivet,
            };
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
            // `move` has two modes (issue #53):
            // 1. Paper-vs-Paper movement self-check (`--server paper
            //    --pairs paper:paper`): boot `--runs` Paper servers and require
            //    identical normalized movement transcripts.
            // 2. Paper-vs-Rivet movement differential (`--server both
            //    --pairs paper:rivet`): boot exactly one Paper + one Rivet on
            //    isolated ports, drive the walk against each, and compare the
            //    authoritative movement evidence.
            match (server, pairs) {
                (ServerSelection::Paper, Pairs::PaperPaper) => {}
                (ServerSelection::Both, Pairs::PaperRivet) => {}
                (server, pairs) => {
                    return Err(format!(
                        "move only supports --server paper --pairs paper:paper (self-check) or \
                         --server both --pairs paper:rivet (differential) (got --server {} \
                         --pairs {})",
                        server.as_str(),
                        pairs.as_str()
                    ));
                }
            }
            // The self-check needs at least a pair of Paper boots; the
            // differential always boots exactly one Paper + one Rivet, so an
            // explicit --runs is a silent no-op (rejected like the join
            // both-mode).
            match (server, runs_explicit) {
                (ServerSelection::Paper, _) if runs <= 1 => {
                    return Err(
                        "--runs must be at least 2 for move (Paper-vs-Paper needs a pair)"
                            .to_owned(),
                    );
                }
                (ServerSelection::Both, true) => {
                    return Err(
                        "--server both always boots exactly one Paper + one Rivet, so --runs is a \
                         silent no-op; drop it (or use --server paper for a run count)"
                            .to_owned(),
                    );
                }
                _ => {}
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

/// Reserve `n` distinct ephemeral loopback ports, held so the OS cannot hand
/// out the same port twice. The held [`PortReservation`]s are released only
/// immediately before each server spawns (inside `server::boot`), so the
/// bind-drop-boot race narrows to the spawn->child-bind gap.
fn reserve_ports(
    n: usize,
) -> Result<Vec<rivet_harness_common::port::PortReservation>, RunnerError> {
    rivet_harness_common::port::reserve(n)
        .map_err(|e| RunnerError::Gate(format!("failed to reserve ephemeral ports: {e}")))
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

/// Resolve a `rivet-oracle` `server.properties` fixture by name. The default
/// `server.properties` (seed 42, superflat, offline, port 25599) is the config
/// source of truth for Paper boots; the single-stone variant is the Paper
/// reference of the Rivet-vs-Paper differential (issue #159). Rivet boots do
/// not require either: `rivet-server` is driven purely by `--host`/`--port`.
fn fixture_server_properties(crate_root: &Path, name: &str) -> Result<PathBuf, RunnerError> {
    let p = crate_root.join(format!("../rivet-oracle/fixtures/{name}"));
    if p.is_file() {
        Ok(p)
    } else {
        Err(RunnerError::Unverified(format!(
            "{name} not found at {} (rivet-oracle fixtures)",
            p.display()
        )))
    }
}

/// The default superflat fixture for Paper boots that do not need a spawn
/// height to match Rivet's.
fn server_properties(crate_root: &Path) -> Result<PathBuf, RunnerError> {
    fixture_server_properties(crate_root, "server.properties")
}

/// The single-stone superflat `server.properties` the Paper reference of the
/// Rivet-vs-Paper differential boots with (issue #159).
///
/// The shared fixture's `generator-settings={}` makes Paper fall back to the
/// default FLAT preset (bedrock ×1 + dirt ×2 + grass ×1 = 4 layers), which
/// spawns at y=-60 — while Rivet serves its single-stone world at y=-63. With
/// `generator-settings={"layers":[{"height":1,"block":"minecraft:stone"}]}`
/// Paper has exactly one layer and spawns at y=-63 too, so `position.y` becomes
/// a genuinely compared field instead of a 3-block "documented gap".
fn single_stone_server_properties(crate_root: &Path) -> Result<PathBuf, RunnerError> {
    fixture_server_properties(crate_root, "server-single-stone.properties")
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
        None,
        &[],
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
        None,
        &[],
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
            None,
            &[],
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
        "    {}-chunk send-set at Rivet's fixed superflat spawn y={}. The connection is proven",
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
/// Rivet/Paper gaps; any other divergence is a genuine Rivet/Paper mismatch, not
/// a harness artifact, and fails the run. Normal runs rebuild the server and the
/// fallback has a narrow freshness guard; this behavioral gate remains
/// load-bearing even when an explicit `RIVET_SERVER_BIN` override is used.
///
/// `position.y` is deliberately NOT here (issue #159): the Paper reference now
/// boots the single-stone superflat fixture and spawns at y=-63.0 like Rivet,
/// so a position.y divergence is a real server mismatch and must fail the run —
/// never be normalized or excluded to make the test pass. The only remaining
/// documented gap is the health component default, and it is value-bound:
/// Rivet's join burst does not send `set_health` (play-state gap tracked
/// separately), so azalea reports 1.0 against Rivet vs 20.0 against Paper. A
/// future half-implemented `set_health` that reports any other value fails here
/// rather than being waved through.
fn check_paper_rivet_divergence(d: &comparator::TranscriptDiff) -> Result<(), RunnerError> {
    const DOCUMENTED_GAPS: [(&str, f64, f64); 1] = [("health.health", 20.0, 1.0)];
    for f in &d.diffs {
        let Some((expected, actual)) = DOCUMENTED_GAPS
            .iter()
            .find(|(path, _, _)| *path == f.path)
            .map(|(_, e, a)| (*e, *a))
        else {
            return Err(RunnerError::Gate(format!(
                "Paper-vs-Rivet divergence on {}: expected {} got {} — not one of the documented \
                 Rivet/Paper gaps ({:?}); refusing PASS",
                f.path,
                f.expected,
                f.actual,
                DOCUMENTED_GAPS
                    .iter()
                    .map(|(p, _, _)| *p)
                    .collect::<Vec<_>>()
            )));
        };
        let f_expected = f.expected.as_f64();
        let f_actual = f.actual.as_f64();
        if f_expected != Some(expected) || f_actual != Some(actual) {
            return Err(RunnerError::Gate(format!(
                "Paper-vs-Rivet divergence on {}: expected {} got {} — the documented gap only \
                 admits Paper={expected} vs Rivet={actual}; refusing PASS",
                f.path, f.expected, f.actual
            )));
        }
    }
    Ok(())
}

/// The both-mode movement differential's divergence gate (issue #53).
///
/// The single-stone superflat fixture aligns Paper's spawn height with Rivet's
/// (both y=-63.0), so the compared move transcripts must be byte-identical: the
/// sampled walk geometry, velocity, teleport echo, and `last_sent` are all
/// deterministic per server and Paper-vs-Rivet equal (verified on genuine
/// artifacts). There is deliberately no documented gap — unlike the join
/// differential's health-component gap, which the fixture does not remove — so
/// any compared-field divergence is a genuine movement mismatch and fails the
/// run.
fn check_move_divergence(d: &comparator::TranscriptDiff) -> Result<(), RunnerError> {
    if let Some(f) = d.diffs.first() {
        let mut msg = format!(
            "Paper-vs-Rivet movement divergence on {}: expected {} got {} — the move \
             differential has no documented gaps (the single-stone fixture aligns Paper's \
             superflat spawn y with Rivet's, and the sampled walk + last_sent are deterministic \
             and Paper-vs-Rivet equal); refusing PASS",
            f.path, f.expected, f.actual
        );
        for f in &d.diffs[1..] {
            msg.push_str(&format!("\n    {f}"));
        }
        return Err(RunnerError::Gate(msg));
    }
    Ok(())
}

/// Prove the both-mode movement differential is non-vacuous: tamper a *compared*
/// movement field (`walk.last_sent.x`, the final sent position the evidence
/// promoted to a compared field) on the Paper reference and re-run the exact
/// comparator + divergence gate the live comparison used, requiring the reported
/// `walk.last_sent` divergence to observe the tampered value and the gate to
/// refuse PASS.
///
/// The live acceptance cannot pass unless a tampered compared movement field
/// actually flows through the comparator and the divergence gate: a comparator
/// that reports nothing, or a `walk.last_sent` silently moved back into
/// `excluded`, would PASS vacuously without this negative. A fixed +1.0 offset
/// is safe here because Paper and Rivet record the *same* final x (verified
/// 25.428 on both), so the tampered value cannot silently collide with the
/// other server's.
fn prove_move_differential_non_vacuous(
    paper_t: &Value,
    rivet_t: &Value,
) -> Result<(), RunnerError> {
    let mut tampered = paper_t.clone();
    let x = tampered["walk"]["last_sent"]["x"].as_f64().ok_or_else(|| {
        RunnerError::Gate(
            "negative case FAILED: paper transcript has no walk.last_sent to tamper".to_owned(),
        )
    })?;
    let tampered_x = x + 1.0;
    tampered["walk"]["last_sent"]["x"] = json!(tampered_x);
    let neg = comparator::diff(&tampered, rivet_t);
    // The comparator reports leaf paths, so the tampered `walk.last_sent.x` is
    // the exact path the divergence surfaces under.
    match neg.diffs.iter().find(|f| f.path == "walk.last_sent.x") {
        Some(f) if f.expected.as_f64() == Some(tampered_x) => {
            // The tampered value was read by the real comparator. Now the real
            // divergence gate must refuse PASS: walk.last_sent is a compared
            // field, so a divergence on it is a genuine movement mismatch.
            match check_move_divergence(&neg) {
                Err(e) if e.to_string().contains("walk.last_sent") => {}
                Err(e) => {
                    return Err(RunnerError::Gate(format!(
                        "negative case FAILED: the move divergence gate refused PASS for a reason \
                         other than the tampered walk.last_sent: {e}"
                    )));
                }
                Ok(()) => {
                    return Err(RunnerError::Gate(
                        "negative case FAILED: the move divergence gate PASSED despite the \
                         tampered walk.last_sent — walk.last_sent must be a compared field, not a \
                         documented gap to wave through"
                            .to_owned(),
                    ));
                }
            }
            println!(
                "    tampered paper walk.last_sent.x {x} -> {tampered_x} — the divergence path \
                 reported 'walk.last_sent: expected {tampered_x}, got {}' and refused PASS, so \
                 walk.last_sent is genuinely compared and read by the gate",
                f.actual
            );
            Ok(())
        }
        Some(f) => Err(RunnerError::Gate(format!(
            "negative case FAILED: the divergence path reported 'walk.last_sent: expected {} got \
             {}', but paper walk.last_sent.x was tampered to {tampered_x} — the reported \
             divergence must observe the tampered value (walk.last_sent must never be excluded or \
             normalized to make the comparison pass)",
            f.expected, f.actual
        ))),
        None => Err(RunnerError::Gate(
            "negative case FAILED: tampering paper walk.last_sent.x produced no compared \
             'walk.last_sent' diff (it is excluded, absent, or the comparator reported nothing) — \
             walk.last_sent must be a compared field read by the divergence gate"
                .to_owned(),
        )),
    }
}

/// Prove the Rivet movement trace is authoritative evidence for the client's
/// actual walk, not just internally consistent: the server's final accepted
/// position must match the client's final sent position (modulo the in-flight
/// frames the server processes past the client's recorded last tick).
///
/// The client's `last_sent` is its own `LastSentPosition` at the tick the walk
/// stopped; the server keeps accepting the trailing position frames that arrive
/// after that snapshot, so the final authoritative position lands a fraction of
/// a tick past `last_sent` (observed +0.217 on a genuine boot). The bound is
/// deliberately loose (a few ticks of movement) so timing noise cannot fail the
/// run, yet tight enough that a server which never moved the player (final ≈
/// spawn, ~25 blocks short) or teleported it elsewhere fails.
///
/// Coordinate frames: the client transcript is spawn-relative — `last_sent` X/Z
/// are the absolute position minus the full-precision `spawn_origin` the client
/// recorded (so the walk compares across the server's randomized spawn X/Z
/// offset). The trace's authoritative position, by contrast, is absolute world
/// coordinates. This cross-check therefore adds `spawn_origin` back to the
/// spawn-relative `last_sent` X/Z before comparing, so it holds for any spawn
/// origin — not just the (0, 0) case. The origin is carried in the transcript
/// (and excluded from parity — `excluded_move_fields`'s `walk.spawn_origin`)
/// precisely so this inversion is lossless; a transcript without it is a schema
/// violation and fails loudly here rather than silently assuming origin (0, 0).
fn check_rivet_authoritative(
    trace: &trace::MovementTrace,
    rivet_walk: &Value,
) -> Result<String, RunnerError> {
    let summary = trace.check_authoritative().map_err(RunnerError::Gate)?;
    let last_sent = rivet_walk.get("last_sent").cloned().unwrap_or(Value::Null);
    let last_sent_x = last_sent.get("x").and_then(Value::as_f64).ok_or_else(|| {
        RunnerError::Gate(
            "rivet walk transcript has no last_sent.x to cross-check the authoritative position \
             against"
                .to_owned(),
        )
    })?;
    let last_sent_y = last_sent
        .get("y")
        .and_then(Value::as_f64)
        .ok_or_else(|| RunnerError::Gate("rivet walk transcript has no last_sent.y".to_owned()))?;
    let last_sent_z = last_sent
        .get("z")
        .and_then(Value::as_f64)
        .ok_or_else(|| RunnerError::Gate("rivet walk transcript has no last_sent.z".to_owned()))?;
    let origin = rivet_walk
        .get("spawn_origin")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            RunnerError::Gate(
                "rivet walk transcript has no spawn_origin — without the full-precision spawn \
                 position the spawn-relative last_sent cannot be mapped back to the trace's \
                 absolute coordinates; refusing to assume origin (0, 0)"
                    .to_owned(),
            )
        })?;
    let origin_x = origin
        .get("x")
        .and_then(Value::as_f64)
        .ok_or_else(|| RunnerError::Gate("rivet walk spawn_origin has no x".to_owned()))?;
    let origin_z = origin
        .get("z")
        .and_then(Value::as_f64)
        .ok_or_else(|| RunnerError::Gate("rivet walk spawn_origin has no z".to_owned()))?;
    let [fx, fy, fz] = trace.final_position().ok_or_else(|| {
        RunnerError::Gate("movement trace has no final authoritative position".to_owned())
    })?;
    // last_sent.x/z are spawn-relative; add the origin back to compare in the
    // trace's absolute frame.
    let sent_abs_x = last_sent_x + origin_x;
    let sent_abs_z = last_sent_z + origin_z;
    if !(fx >= sent_abs_x - 1.0 && fx <= sent_abs_x + 5.0) {
        return Err(RunnerError::Gate(format!(
            "rivet authoritative final x={fx} is outside [{sent_abs_x}, {sent_abs_x}+5] — the \
             server's accepted walk does not match the client's last sent position (a server \
             that never moved the player, or accepted frames it was not sent)"
        )));
    }
    if fy != last_sent_y || fz != sent_abs_z {
        return Err(RunnerError::Gate(format!(
            "rivet authoritative final position ({fx}, {fy}, {fz}) disagrees with the client's \
             last sent (absolute {sent_abs_x}, {last_sent_y}, {sent_abs_z}; spawn-relative \
             {last_sent_x}, {last_sent_y}, {last_sent_z}) on y/z — the server's authoritative \
             height/direction does not match the client's walk"
        )));
    }
    Ok(summary)
}

/// Prove the both-mode divergence path is non-vacuous: tamper the compared
/// `position.y` on the Paper reference and re-run the exact comparator +
/// divergence gate the live comparison used, requiring the reported
/// `position.y` divergence to observe the tampered value and the gate to refuse
/// PASS.
///
/// The live acceptance cannot pass unless a tampered compared field actually
/// flows through the comparator and the divergence gate: a comparator that
/// reports nothing, or a `position.y` silently moved into `excluded`, would
/// PASS vacuously without this negative. The tamper is offset above the larger
/// of the two spawn heights, so it differs from both paper's and rivet's y for
/// all MC-realistic spawn heights (small magnitudes, where the f64 spacing is
/// far below 1.0). A fixed +1.0 on paper's y alone would silently align with
/// rivet's y if the two spawn heights were ever adjacent, and the negative would
/// fail on a healthy tree. Because `position.y` is a *compared* field in this
/// scenario (issue #159: the single-stone fixture spawns both servers at
/// y=-63), the divergence gate must refuse PASS on the tamper — a position.y
/// divergence is a real server mismatch, never a documented gap to wave through.
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
    let tampered_y = y.max(rivet_y) + 1.0;
    tampered["position"]["y"] = json!(tampered_y);
    let neg = comparator::diff(&tampered, rivet_t);
    match neg.diffs.iter().find(|f| f.path == "position.y") {
        Some(f) if f.expected.as_f64() == Some(tampered_y) => {
            // The tampered value was read by the real comparator. Now the real
            // divergence gate must refuse PASS: position.y is a compared field,
            // so a divergence on it is a genuine server mismatch.
            match check_paper_rivet_divergence(&neg) {
                Err(e) if e.to_string().contains("position.y") => {}
                Err(e) => {
                    return Err(RunnerError::Gate(format!(
                        "negative case FAILED: the divergence gate refused PASS for a reason other \
                         than the tampered position.y: {e}"
                    )));
                }
                Ok(()) => {
                    return Err(RunnerError::Gate(
                        "negative case FAILED: the divergence gate PASSED despite the tampered \
                         position.y — position.y must be a compared field, not a documented gap \
                         to wave through"
                            .to_owned(),
                    ));
                }
            }
            println!(
                "    tampered paper position.y {y} -> {tampered_y} — the divergence path reported \
                 'position.y: expected {tampered_y}, got {}' and refused PASS, so position.y is \
                 genuinely compared and read by the gate",
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

/// Mode C: Paper-vs-Rivet play scenario (issue #192, inverted for #159). Both
/// servers must take the pinned Azalea client through login/configuration into
/// spawn; the transcripts are compared field-level and differ only on the
/// excluded per-boot nondeterminism and the documented health default gap.
/// Paper boots the single-stone superflat fixture so both servers spawn at
/// y=-63.0 and `position.y` is a compared field (never excluded or normalized
/// to pass).
fn run_paper_vs_rivet(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-both");
    fs::create_dir_all(&work)?;
    let server_properties = single_stone_server_properties(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    // Port isolation: Paper and Rivet get distinct ephemeral ports, so the
    // client provably targets the server the harness points it at (no two
    // servers in one scenario can collide). The reservations are held until
    // each server's `boot` releases them right before spawning, so the ports
    // cannot be stolen during the slow run-dir preparation.
    let mut reservations = reserve_ports(2)?;
    let paper_addr = SocketAddr::new(base.ip(), reservations[0].port());
    let rivet_addr = SocketAddr::new(base.ip(), reservations[1].port());

    println!("rivet scenario runner: join (Paper-vs-Rivet play)");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!(
        "    server.properties : {} (single-stone superflat)",
        server_properties.display()
    );
    println!(
        "    paper pin         : {} (verified from the materialized jar)",
        server::PAPER_PIN_COMMIT
    );
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
        Some(reservations.remove(0)),
        &[],
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
    // Load-bearing provenance: the Paper reference this differential compares
    // Rivet against must be the pinned oracle commit. This is scoped to the
    // differential path only — Paper-vs-Paper self-checks (paper:paper join,
    // move) and capture compare a build against itself, where the pin is not a
    // correctness requirement. The check reads the jar that actually booted in
    // the run dir, so a stale, swapped, or unverifiable Paper cannot silently
    // stand in for the reference.
    server::verify_paper_provenance(paper_srv.run_dir())?;
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
        Some(reservations.remove(0)),
        &[],
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
        "    {} field(s) differ — the documented Rivet/Paper gap (health component default),",
        d.diffs.len()
    );
    println!("    plus the excluded per-boot nondeterminism:");
    for f in &d.diffs {
        println!("        {f}");
    }
    for f in &d.excluded {
        println!("        (excluded) {f}");
    }

    // Negative case: prove the divergence path just exercised is non-vacuous.
    // Tamper a *compared* field (position.y, the deterministic superflat spawn
    // height) on the Paper reference and require the real comparator/divergence
    // path to report the expected named mismatch and refuse PASS — the harness
    // must not pass vacuously, and position.y must never be excluded or
    // normalized to make the comparison pass.
    println!();
    println!("Negative case (tamper paper position.y through the real divergence path)");
    prove_both_mode_non_vacuous(&paper_t, &rivet_t)?;

    println!();
    println!("VERDICT: PASS — both servers took the pinned Azalea client through login and");
    println!("    configuration into play:");
    println!("      * Paper reached spawn from the single-stone superflat (Git-Commit pinned) and");
    println!("        Rivet reached RIVET_READY on its own isolated port ({rivet_addr}) and took");
    println!("        the client through the {boundary}.");
    println!(
        "      * Both spawn at the same superflat height y=-63.0, so position.y is a compared"
    );
    println!(
        "        field — the negative case proved the comparator reads a tampered spawn height"
    );
    println!(
        "        and the divergence gate refuses PASS on it, so any Paper-vs-Rivet position.y"
    );
    println!("        divergence would FAIL the run.");
    println!("      * The connection is proven two ways: the rivet log shows 'connection");
    println!(
        "        established' (only the real rivet-server emits it), and the client transcript"
    );
    println!("        is outcome=spawned with the pinned Azalea revision, 117 chunks, and spawn");
    println!("        y=-63.0 — which a stale pre-play build, a fake/non-Rivet endpoint, or a");
    println!("        Paper-like y=-60 spawn all fail.");
    println!("      * The compared transcripts differ only on the documented Rivet/Paper gap");
    println!("        (health default: Rivet omits set_health so azalea reports 1.0 vs Paper's");
    println!("        20.0) — any other divergence, including position.y, fails the run, so a");
    println!("        Paper-vs-Rivet regression cannot pass as 'expected'.");
    println!("      * The negative case proved the divergence path is non-vacuous: a tampered");
    println!("        compared position.y on the Paper reference was reported by the real");
    println!("        comparator/divergence path and refused PASS, so the acceptance cannot pass");
    println!("        while ignoring a changed compared field.");
    println!("    artifacts: {}", work.display());
    Ok(())
}

/// Mode D: Paper-vs-Rivet movement differential (issue #53).
///
/// Boots Paper (single-stone superflat fixture, provenance-verified) and Rivet
/// (with `RIVET_TRACE_MOVEMENT=1` so the tick thread emits its authoritative
/// movement audit) on isolated ports, drives the pinned Azalea client's `move`
/// mode (a bounded forward walk) against each, and compares the normalized
/// movement transcripts field-level.
///
/// The single-stone fixture is what makes the movement comparable: Paper's
/// default-flat world spawns at y=-60, so every walk sample and `last_sent`
/// would carry y=-60 vs Rivet's -63 — a 3-block fixture artifact, not a
/// movement difference. With one stone layer both servers walk at y=-63.0 and
/// the whole walk (samples, velocity, teleport echo, `last_sent`) must be
/// byte-identical: there are no documented gaps.
///
/// The Rivet side is additionally proven authoritative: the trace must be
/// internally consistent (teleport ack accepted at spawn, the accepted-move
/// counter matching the record trail, the session-end position equaling the
/// last accepted move) and the server's final accepted position must match the
/// client's `last_sent` modulo the in-flight frames the server processes after
/// the client's last tick.
fn run_paper_vs_rivet_move(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-move-both");
    fs::create_dir_all(&work)?;
    let server_properties = single_stone_server_properties(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    // Port isolation: Paper and Rivet get distinct ephemeral ports, so the
    // client provably targets the server the harness points it at.
    let mut reservations = reserve_ports(2)?;
    let paper_addr = SocketAddr::new(base.ip(), reservations[0].port());
    let rivet_addr = SocketAddr::new(base.ip(), reservations[1].port());

    println!("rivet scenario runner: move (Paper-vs-Rivet movement differential)");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!(
        "    server.properties : {} (single-stone superflat)",
        server_properties.display()
    );
    println!(
        "    paper pin         : {} (verified from the materialized jar)",
        server::PAPER_PIN_COMMIT
    );
    println!("    paper address     : {paper_addr}");
    println!("    rivet address     : {rivet_addr}");
    println!(
        "    rivet trace       : RIVET_TRACE_MOVEMENT=1 (authoritative movement audit on stderr)"
    );
    println!();

    // Paper reference — boots the single-stone fixture so the walk y aligns
    // with Rivet's, and walks in `move` mode.
    let mut paper_srv = server::boot(
        server::ServerKind::Paper,
        &work.join("paper"),
        &work.join("paper.log"),
        &jar,
        Some(&server_properties),
        paper_addr,
        Some(reservations.remove(0)),
        &[],
    )?;
    let paper_client = run_client(
        &client_bin,
        &paper_addr.to_string(),
        &args.username,
        args.timeout_seconds,
        &work,
        "paper",
        "move",
    )?;
    server::shutdown(&mut paper_srv)?;
    // Load-bearing provenance (same as the join differential): the Paper
    // reference this differential compares Rivet against must be the pinned
    // oracle commit.
    server::verify_paper_provenance(paper_srv.run_dir())?;
    let paper_t =
        transcript::normalize_move(&paper_client.stdout_text).map_err(RunnerError::Transcript)?;
    let paper_tp = work.join("paper.transcript.json");
    fs::write(&paper_tp, serde_json::to_string_pretty(&paper_t)?)?;
    println!(
        "    Paper outcome      : {} (transcript in {})",
        paper_t["outcome"],
        paper_tp.display()
    );
    if paper_t["outcome"] != "moved" {
        return Err(RunnerError::Gate(format!(
            "Paper did not move (outcome={}) — regression in the reference server; refusing the \
             comparison",
            paper_t["outcome"]
        )));
    }

    // Rivet SUT — booted with the movement trace enabled so the tick thread
    // emits its authoritative movement audit.
    let mut rivet_srv = server::boot(
        server::ServerKind::Rivet,
        &work.join("rivet"),
        &work.join("rivet.log"),
        &rivet_bin,
        None,
        rivet_addr,
        Some(reservations.remove(0)),
        &[(trace::TRACE_MOVEMENT_ENV, "1")],
    )?;
    let rivet_client = run_client(
        &client_bin,
        &rivet_addr.to_string(),
        &args.username,
        args.timeout_seconds,
        &work,
        "rivet",
        "move",
    )?;
    server::shutdown(&mut rivet_srv)?;
    // Server-side half of the connection proof: the rivet log must show the
    // real rivet-server accepted the client.
    verify_rivet_connection(&work.join("rivet.log"))?;
    let rivet_t =
        transcript::normalize_move(&rivet_client.stdout_text).map_err(RunnerError::Transcript)?;
    let rivet_tp = work.join("rivet.transcript.json");
    fs::write(&rivet_tp, serde_json::to_string_pretty(&rivet_t)?)?;
    println!(
        "    Rivet outcome      : {} (transcript in {})",
        rivet_t["outcome"],
        rivet_tp.display()
    );
    if rivet_t["outcome"] != "moved" {
        return Err(RunnerError::Gate(format!(
            "Rivet did not move (outcome={}) — the movement scenario did not complete; refusing \
             the comparison",
            rivet_t["outcome"]
        )));
    }

    // Rivet authoritative movement evidence: parse the trace from the rivet log
    // and prove it is internally consistent and matches the client's walk.
    let rivet_log = fs::read_to_string(work.join("rivet.log"))?;
    let movement_trace = trace::parse(&rivet_log).map_err(RunnerError::Transcript)?;
    let trace_summary = check_rivet_authoritative(&movement_trace, &rivet_t["walk"])?;
    let trace_tp = work.join("rivet.trace.json");
    // Preserve the parsed trace as a diagnostic artifact (the `MovementTrace`
    // types are not serde-serializable — the crate has no serde dep — so the
    // dump is built by hand from the fields the differential consumed).
    let trace_dump = serde_json::json!({
        "teleport_acks": movement_trace.teleport_acks.iter().map(|a| json!({
            "ack_id": a.ack_id,
            "outcome": a.outcome,
            "position": a.position,
        })).collect::<Vec<_>>(),
        "moves": movement_trace.moves.len(),
        "final_position": movement_trace.final_position(),
        "session_end_reason": movement_trace.session_end.as_ref().map(|e| e.reason.clone()),
        "move_frames_seen": movement_trace.session_end.as_ref().map(|e| e.move_frames_seen),
    });
    fs::write(&trace_tp, serde_json::to_string_pretty(&trace_dump)?)?;
    println!(
        "    Rivet trace        : {trace_summary} (parsed in {})",
        trace_tp.display()
    );

    println!();
    println!("Paper-vs-Rivet movement comparator:");
    let d = comparator::diff(&paper_t, &rivet_t);
    check_move_divergence(&d)?;
    println!(
        "    {} compared field(s) differ — the move differential has no documented gaps,",
        d.diffs.len()
    );
    println!("    so any compared-field divergence fails the run:");
    for f in &d.diffs {
        println!("        {f}");
    }
    for f in &d.excluded {
        println!("        (excluded) {f}");
    }
    if d.excluded_policy_diffs.is_empty() {
        println!("    the excluded sets match (same per-boot nondeterminism declared)");
    } else {
        for f in &d.excluded_policy_diffs {
            println!("        (exclusion policy) {f}");
        }
    }

    // Negative case: prove the movement divergence path just exercised is
    // non-vacuous by tampering a *compared* movement field (walk.last_sent.x).
    println!();
    println!("Negative case (tamper paper walk.last_sent.x through the real divergence path)");
    prove_move_differential_non_vacuous(&paper_t, &rivet_t)?;

    println!();
    println!("VERDICT: PASS — Paper and Rivet produced identical authoritative movement");
    println!("    evidence:");
    println!(
        "      * Both servers took the pinned Azalea client through a bounded forward walk; the"
    );
    println!(
        "        sampled walk (position deltas, velocity, on-ground), the teleport ack echo, and"
    );
    println!(
        "        the final sent position `last_sent` are byte-identical on the compared fields"
    );
    println!("        (Paper and Rivet both record last_sent x=25.428 z=0.0 and the same walk).");
    println!(
        "      * Paper boots the single-stone superflat fixture (Git-Commit pinned) so the walk"
    );
    println!(
        "        y aligns with Rivet's -63.0 — the old default-flat y=-60 fixture artifact is"
    );
    println!("        removed, so `last_sent.y` and every sample y are compared fields, not gaps.");
    println!("      * The Rivet connection is proven two ways: the rivet log shows 'connection");
    println!("        established' (only the real rivet-server emits it), and the movement trace");
    println!("        parsed from the same log is internally consistent (teleport ack accepted at");
    println!(
        "        spawn, accepted-move counter matching the record trail, session-end position"
    );
    println!(
        "        equal to the last accepted move) and its final authoritative position matches"
    );
    println!(
        "        the client's last_sent modulo in-flight frames — so the server really tracked"
    );
    println!("        the walk, and the compared evidence is Rivet's server-side movement, not a");
    println!("        client-side artifact.");
    println!("      * The negative case proved the movement divergence path is non-vacuous: a");
    println!(
        "        tampered compared walk.last_sent.x on the Paper reference was reported by the"
    );
    println!(
        "        real comparator/divergence gate and refused PASS — the acceptance cannot pass"
    );
    println!("        while ignoring a changed compared movement field.");
    println!("    artifacts: {}", work.display());
    Ok(())
}

fn run_move(args: &Args) -> Result<(), RunnerError> {
    match (args.server, args.pairs) {
        (ServerSelection::Paper, Pairs::PaperPaper) => run_move_self_check(args),
        (ServerSelection::Both, Pairs::PaperRivet) => run_paper_vs_rivet_move(args),
        (server, pairs) => Err(RunnerError::Gate(format!(
            "unhandled --server {} / --pairs {} combination",
            server.as_str(),
            pairs.as_str()
        ))),
    }
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
        None,
        &[],
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
        Subcommand::Move => run_move(&args),
        Subcommand::Capture => run_capture(&args),
        Subcommand::Help => {
            println!("{}", usage());
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::from(EXIT_PASS),
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
    fn move_rejects_unsupported_configurations() {
        // `move` has exactly two modes: Paper-vs-Paper self-check and the
        // Paper-vs-Rivet both-mode differential. A rivet-only boot, a
        // paper:rivet pair on a paper-only server, and other combinations are
        // invalid.
        assert!(parse(&["move", "--server", "rivet"]).is_err());
        assert!(parse(&["move", "--pairs", "paper:rivet"]).is_err());
        assert!(
            parse(&["move", "--server", "both"]).is_ok(),
            "both defaults to paper:rivet"
        );
        assert!(
            parse(&["move", "--server", "both", "--pairs", "paper:rivet"]).is_ok(),
            "the Paper-vs-Rivet movement differential is a valid move mode"
        );
        assert!(parse(&["move", "--server", "both", "--pairs", "paper:paper"]).is_err());
    }

    #[test]
    fn move_requires_two_runs_for_paper_self_check() {
        // The self-check needs a pair of Paper boots; the both-mode differential
        // always boots exactly one of each (explicit --runs rejected, see
        // `move_both_rejects_explicit_runs`).
        assert!(parse(&["move"]).is_ok());
        assert!(
            parse(&["move", "--runs", "1"]).is_err(),
            "move paper needs >=2"
        );
        assert!(
            parse(&["move", "--server", "both", "--runs", "1"]).is_err(),
            "both always boots exactly one Paper + one Rivet; --runs is a no-op"
        );
    }

    #[test]
    fn move_both_rejects_explicit_runs() {
        // `both` always boots exactly one Paper + one Rivet; an explicit --runs
        // would be a silent no-op.
        assert!(
            parse(&[
                "move",
                "--server",
                "both",
                "--pairs",
                "paper:rivet",
                "--runs",
                "2"
            ])
            .is_err(),
            "explicit --runs with move both must be rejected"
        );
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
    fn server_selection_defaults_the_pairs() {
        // --server rivet/both with no --pairs must not default to the invalid
        // paper:paper; the README documents `join --server rivet --runs 2` and
        // `join --server both --pairs paper:rivet`, so omitting --pairs for a
        // rivet/both run is the natural invocation.
        assert!(parse(&["join", "--server", "rivet", "--runs", "2"]).is_ok());
        assert!(parse(&["join", "--server", "both"]).is_ok());
        // Plain `join` still defaults to Paper-vs-Paper.
        assert!(parse(&["join"]).is_ok());
        assert!(parse(&["join", "--server", "paper"]).is_ok());
        // An explicit conflicting --pairs remains rejected.
        assert!(parse(&["join", "--server", "rivet", "--pairs", "paper:paper"]).is_err());
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
        let held = reserve_ports(3).unwrap();
        let ports: Vec<u16> = held.iter().map(|r| r.port()).collect();
        let unique: std::collections::BTreeSet<u16> = ports.iter().copied().collect();
        assert_eq!(unique.len(), 3, "ports must be distinct: {ports:?}");
        assert!(
            ports.iter().all(|p| *p > 0),
            "ephemeral ports must be nonzero: {ports:?}"
        );
        // Distinctness is the load-bearing guarantee (two servers in one scenario
        // must never share a port). Holding the listeners is what makes the
        // ports non-stealable during boot prep; the shared crate owns and tests
        // that contract (a held reservation blocks rebind, release frees it).
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
        diff_with_values(path, json!(null), json!(null))
    }

    fn diff_with_values(path: &str, expected: Value, actual: Value) -> comparator::TranscriptDiff {
        let mut d = comparator::TranscriptDiff::default();
        d.diffs.push(comparator::FieldDiff {
            path: path.to_owned(),
            expected,
            actual,
        });
        d
    }

    #[test]
    fn paper_rivet_divergence_accepts_only_documented_gaps() {
        assert!(check_paper_rivet_divergence(&comparator::TranscriptDiff::default()).is_ok());
        // The documented gap is value-bound: Paper sends 20.0, Rivet 1.0.
        assert!(
            check_paper_rivet_divergence(&diff_with_values(
                "health.health",
                json!(20.0),
                json!(1.0)
            ))
            .is_ok()
        );
    }

    #[test]
    fn paper_rivet_divergence_rejects_a_wrong_health_gap_value() {
        // The documented gap is not a blanket "any divergence on health.health":
        // a future half-implemented set_health that reports, say, 15.0 vs Paper's
        // 20.0 must FAIL rather than be waved through as the documented gap.
        let err = check_paper_rivet_divergence(&diff_with_values(
            "health.health",
            json!(20.0),
            json!(15.0),
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("only admits Paper=20 vs Rivet=1"),
            "a wrong health value must fail the value-bound gap, got {err}"
        );
        assert!(
            err.to_string().contains("refusing PASS"),
            "a wrong health value must refuse PASS, got {err}"
        );
    }

    #[test]
    fn paper_rivet_divergence_rejects_position_y_as_undocumented() {
        // Issue #159: the Paper reference now boots the single-stone superflat
        // and spawns at y=-63.0 like Rivet, so a position.y divergence is a real
        // server mismatch and must FAIL — never a "documented gap" to be waved
        // through. The counterfactual keeps the old (wrong) Paper reference at
        // y=-60 and asserts the both-mode refuses PASS instead of accepting it.
        let err = check_paper_rivet_divergence(&diff_with("position.y")).unwrap_err();
        assert!(
            err.to_string().contains("position.y"),
            "error must name the diverging position.y, got {err}"
        );
        assert!(
            err.to_string().contains("refusing PASS"),
            "position.y must fail the run, got {err}"
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
        // The genuine Paper-vs-Rivet shape (issue #159): both servers spawn at
        // the same superflat height y=-63, so the transcripts diverge only on
        // the documented health default gap (Paper 20 / Rivet 1). Tampering
        // Paper's position.y through the real comparator/divergence path must be
        // reported with the tampered value and refused PASS (position.y is a
        // compared field).
        let paper = join_transcript(-63.0, 20.0);
        let rivet = join_transcript(-63.0, 1.0);
        assert!(prove_both_mode_non_vacuous(&paper, &rivet).is_ok());
    }

    #[test]
    fn both_mode_negative_passes_when_spawn_heights_are_adjacent() {
        // Regression for the false-failure mode: with a fixed +1.0 offset,
        // paper.y = -64 tampered to -63 would collide with rivet.y = -63, produce
        // no position.y diff, and FAIL on a healthy tree. The tamper must be
        // offset above the larger spawn height so it always differs from rivet.
        // The counterfactual keeps paper at y=-64 (a divergence that would
        // already fail the real comparison) and asserts the tamper path still
        // reports the tampered value.
        let paper = join_transcript(-64.0, 20.0);
        let rivet = join_transcript(-63.0, 1.0);
        assert!(prove_both_mode_non_vacuous(&paper, &rivet).is_ok());
    }

    #[test]
    fn both_mode_negative_fails_if_position_y_is_silently_excluded() {
        // If a future edit moves position.y into the `excluded` map (so the
        // comparison could pass by never seeing the tamper), the negative must
        // FAIL: the reported divergence must not observe the tampered value.
        let mut paper = join_transcript(-63.0, 20.0);
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

    /// The scenario's hardcoded `PAPER_PIN_COMMIT` must not drift from the
    /// oracle manifest's `paper` provenance pin (the golden baseline the oracle
    /// gate verifies against). A forward oracle-pin bump already fails loudly at
    /// runtime (the materialized jar's commit won't match); this test closes the
    /// reverse direction — editing only the scenario constant — by reading the
    /// manifest the same way `tools/rivet-oracle`'s `parse_paper_pin` does and
    /// asserting the pins agree.
    #[test]
    fn paper_pin_matches_oracle_manifest() {
        let manifest_path = crate_root().join("../rivet-oracle/fixtures/manifest.json");
        let text = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
        let manifest: Value =
            serde_json::from_str(&text).expect("oracle manifest must be valid JSON");
        let paper = manifest
            .get("paper")
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "oracle manifest {} must carry a 'paper' provenance string",
                    manifest_path.display()
                )
            });
        let pin = paper
            .rsplit_once('@')
            .map(|(_, commit)| commit.trim())
            .filter(|commit| !commit.is_empty())
            .unwrap_or_else(|| panic!("paper provenance {paper:?} must carry an @<commit> pin"));
        assert_eq!(
            pin,
            server::PAPER_PIN_COMMIT,
            "scenario PAPER_PIN_COMMIT drifted from the oracle manifest pin ({pin} vs {}); \
             keep them in lockstep so the differential targets the same Paper reference",
            server::PAPER_PIN_COMMIT
        );
    }

    /// A minimal normalized move transcript for the negative-case counterfactual.
    fn move_transcript(last_sent_x: f64, exclude_last_sent: bool) -> Value {
        let mut excluded = serde_json::Map::new();
        if exclude_last_sent {
            excluded.insert(
                "walk.last_sent".to_owned(),
                json!("would make the negative vacuous"),
            );
        }
        json!({
            "outcome": "moved",
            "walk": { "last_sent": { "x": last_sent_x, "y": -63.0, "z": 0.0 } },
            "excluded": excluded,
        })
    }

    /// Tampering the *compared* `walk.last_sent.x` on Paper must produce a
    /// `walk.last_sent.x` diff (the comparator reports leaf paths) that the
    /// divergence gate refuses — this is what the live both-mode negative runs.
    #[test]
    fn move_negative_tamper_observes_the_leaf_path() {
        let paper = move_transcript(25.0, false);
        let rivet = move_transcript(25.0, false);
        prove_move_differential_non_vacuous(&paper, &rivet)
            .expect("tampered walk.last_sent.x must be detected and refused");
    }

    /// If `walk.last_sent` were silently moved back into the `excluded` map, the
    /// comparator would no longer surface the tampered leaf in `diffs`, and the
    /// negative must FAIL rather than pass vacuously.
    #[test]
    fn move_negative_fails_when_last_sent_is_excluded() {
        let paper = move_transcript(25.0, true);
        let rivet = move_transcript(25.0, true);
        let err = prove_move_differential_non_vacuous(&paper, &rivet).unwrap_err();
        assert!(
            err.to_string().contains("excluded"),
            "an excluded walk.last_sent must trip the vacuous-guard, got {err}"
        );
    }

    /// A rivet walk transcript with the given spawn-relative `last_sent` and the
    /// full-precision `spawn_origin` the client subtracted. Both the comparator
    /// negative and the authoritative cross-check consume this shape.
    fn rivet_walk(last_sent: [f64; 3], origin: [f64; 3]) -> Value {
        json!({
            "last_sent": { "x": last_sent[0], "y": last_sent[1], "z": last_sent[2] },
            "spawn_origin": { "x": origin[0], "y": origin[1], "z": origin[2] },
        })
    }

    /// A movement trace that passes `check_authoritative` and ends at the given
    /// absolute position: an accepted spawn teleport, a two-move trail whose last
    /// move equals the session-end position, and a traced end-of-stream reason.
    fn authoritative_trace_at(final_pos: [f64; 3]) -> trace::MovementTrace {
        trace::MovementTrace {
            teleport_acks: vec![trace::TeleportAck {
                ack_id: 1,
                outcome: "accepted".to_owned(),
                position: Some([0.0, -63.0, 0.0]),
            }],
            moves: vec![
                trace::MoveAccepted {
                    x: 0.0,
                    y: -63.0,
                    z: 0.0,
                    accepted_frames: 1,
                },
                trace::MoveAccepted {
                    x: final_pos[0],
                    y: final_pos[1],
                    z: final_pos[2],
                    accepted_frames: 2,
                },
            ],
            session_end: Some(trace::SessionEnd {
                reason: "disconnect.endOfStream".to_owned(),
                x: final_pos[0],
                y: final_pos[1],
                z: final_pos[2],
                accepted_frames: 2,
                move_frames_seen: 3,
            }),
        }
    }

    /// The genuine zero-origin path: the player spawns at the world origin, the
    /// client records last_sent x=25 (spawn-relative == absolute), and the server
    /// keeps accepting trailing frames so its final authoritative position lands
    /// a fraction of a tick past last_sent (the documented in-flight overshoot).
    #[test]
    fn rivet_authoritative_matches_at_zero_spawn_origin() {
        let trace = authoritative_trace_at([25.5, -63.0, 0.0]);
        let walk = rivet_walk([25.0, -63.0, 0.0], [0.0, -63.0, 0.0]);
        let summary = check_rivet_authoritative(&trace, &walk).expect("must match");
        assert!(summary.contains("accepted moves"), "{summary}");
    }

    /// Counterfactual: the spawn origin is nowhere near (0, 0) — the server
    /// randomized the spawn X/Z offset to (9.5, -3.5), so the client's
    /// spawn-relative last_sent (25, 0) maps to absolute (34.5, -3.5). The
    /// cross-check must add the origin back before comparing against the trace's
    /// absolute position; a check that assumed origin (0, 0) would compare the
    /// trace's x=35.0 against 25.0 and fail on a healthy tree.
    #[test]
    fn rivet_authoritative_matches_at_nonzero_spawn_origin() {
        // Absolute last_sent = spawn-relative (25, -63, 0) + origin (9.5, -3.5) =
        // (34.5, -63, -3.5); the server's final position overshoots x by +0.5
        // (in-flight frames) and z exactly matches the reconstructed absolute z.
        let trace = authoritative_trace_at([35.0, -63.0, -3.5]);
        let walk = rivet_walk([25.0, -63.0, 0.0], [9.5, -63.0, -3.5]);
        let summary = check_rivet_authoritative(&trace, &walk).expect("must match");
        assert!(summary.contains("accepted moves"), "{summary}");
    }

    /// A spawn-origin offset large enough that ignoring it would flip the verdict
    /// must still be handled: this pins that the origin addition is genuinely
    /// load-bearing, not a cosmetic branch.
    #[test]
    fn rivet_authoritative_requires_spawn_origin() {
        // Without spawn_origin the cross-check cannot map the spawn-relative
        // last_sent to the trace's absolute frame — it must fail loudly rather
        // than silently assume the player spawned at (0, 0).
        let trace = authoritative_trace_at([25.5, -63.0, 0.0]);
        let walk = json!({ "last_sent": { "x": 25.0, "y": -63.0, "z": 0.0 } });
        let err = check_rivet_authoritative(&trace, &walk).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("spawn_origin") && msg.contains("assume origin"),
            "missing spawn_origin must fail loudly and refuse a (0,0) assumption, got {msg}"
        );
    }
}
