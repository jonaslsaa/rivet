//! Rivet scenario runner — Paper-vs-Paper differential harness.
//!
//! Boots a real Java Paper server headlessly (the `verify` pattern from
//! `tools/rivet-oracle`), joins it with the Azalea headless client
//! (`rivet-client`), captures a normalized observable transcript, and compares
//! transcripts across Paper boots with a field-level comparator.
//!
//! Subcommands:
//!   run-scenario join       boot N Paper servers, join each, require identical
//!                           normalized transcripts, then prove the comparator
//!                           detects a tampered position (negative case).
//!   run-scenario capture    boot one server, join, print the normalized
//!                           transcript (debugging).
//!
//! Raw diagnostics (server logs, client stdout/stderr, normalized transcripts)
//! are preserved under `work/scenario-join/`.

mod comparator;
mod server;
mod transcript;

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde_json::{Value, json};

const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const DEFAULT_RUNS: usize = 2;

#[derive(Debug)]
enum RunnerError {
    Io(io::Error),
    Server(server::Error),
    Json(serde_json::Error),
    Transcript(String),
    Gate(String),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunnerError::Io(e) => write!(f, "io error: {e}"),
            RunnerError::Server(e) => write!(f, "server error: {e}"),
            RunnerError::Json(e) => write!(f, "json error: {e}"),
            RunnerError::Transcript(m) => write!(f, "transcript error: {m}"),
            RunnerError::Gate(m) => write!(f, "{m}"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Join,
    Capture,
    Help,
}

struct Args {
    command: Subcommand,
    address: String,
    username: String,
    timeout_seconds: u64,
    runs: usize,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut command = Subcommand::Help;
        let mut address = DEFAULT_ADDRESS.to_owned();
        let mut username = DEFAULT_USERNAME.to_owned();
        let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
        let mut runs = DEFAULT_RUNS;
        let mut args = env::args().skip(1);

        if let Some(sub) = args.next() {
            command = match sub.as_str() {
                "join" => Subcommand::Join,
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
                    runs = v.parse().map_err(|_| format!("invalid --runs value: {v}"))?;
                    if runs < 2 {
                        return Err("--runs must be at least 2 (Paper-vs-Paper needs a pair)".to_owned());
                    }
                }
                _ => return Err(format!("unknown argument: {argument}\n\n{}", usage())),
            }
        }

        Ok(Self {
            command,
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
        "Usage: run-scenario <join|capture> [options]\n\
         Options:\n\
         \x20 --address HOST:PORT   server address (default {DEFAULT_ADDRESS})\n\
         \x20 --username NAME       offline account name (default {DEFAULT_USERNAME})\n\
         \x20 --timeout-seconds N   client timeout per run (default {DEFAULT_TIMEOUT_SECONDS})\n\
         \x20 --runs N              Paper boots to compare (default {DEFAULT_RUNS}, min 2)"
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
    idx: usize,
) -> Result<ClientRun, RunnerError> {
    let stdout_path = work.join(format!("client{idx}.stdout.jsonl"));
    let stderr_path = work.join(format!("client{idx}.stderr.log"));
    let output = Command::new(binary)
        .args([
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

/// Boot one Paper server, join via the client, shut the server down, and
/// return the normalized transcript (with the raw artifacts preserved).
fn one_join(
    work: &Path,
    jar: &Path,
    server_properties: &Path,
    client_bin: &Path,
    args: &Args,
    idx: usize,
) -> Result<Value, RunnerError> {
    let run_dir = work.join(format!("run{idx}"));
    let log_path = work.join(format!("boot{idx}.log"));
    println!("[boot {idx}] fresh Paper world in {}", run_dir.display());
    let mut srv = server::boot(&run_dir, &log_path, jar, server_properties)?;
    println!("[run  {idx}] joining via rivet-client ...");
    let client_run = run_client(
        client_bin,
        &args.address,
        &args.username,
        args.timeout_seconds,
        work,
        idx,
    )?;
    server::shutdown(&mut srv)?;

    let normalized = transcript::normalize_join(&client_run.stdout_text)
        .map_err(RunnerError::Transcript)?;
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

fn run_join(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-join");
    fs::create_dir_all(&work)?;
    let server_properties = crate_root.join("../rivet-oracle/fixtures/server.properties");
    if !server_properties.is_file() {
        return Err(RunnerError::Gate(format!(
            "server.properties not found at {} (rivet-oracle fixtures)",
            server_properties.display()
        )));
    }
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = client_binary();
    if !client_bin.is_file() {
        return Err(RunnerError::Gate(format!(
            "rivet-client binary not found at {} — build it first: cargo build --locked",
            client_bin.display()
        )));
    }

    println!("rivet scenario runner: join");
    println!("    paperclip jar     : {}", jar.display());
    println!("    rivet-client bin  : {}", client_bin.display());
    println!("    server.properties : {}", server_properties.display());
    println!("    address           : {}", args.address);
    println!("    username          : {}", args.username);
    println!("    paper boots       : {}", args.runs);
    println!();

    let mut transcripts = Vec::with_capacity(args.runs);
    for idx in 1..=args.runs {
        let t = one_join(&work, &jar, &server_properties, &client_bin, args, idx)?;
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
    // harness must not pass vacuously. Tampering an excluded field (position.x)
    // would prove nothing, so we tamper one that must be compared.
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
        Err(RunnerError::Gate("Paper-vs-Paper comparison failed".to_owned()))
    }
}

fn run_capture(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-join");
    fs::create_dir_all(&work)?;
    let server_properties = crate_root.join("../rivet-oracle/fixtures/server.properties");
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = client_binary();
    if !client_bin.is_file() {
        return Err(RunnerError::Gate(format!(
            "rivet-client binary not found at {} — build it first: cargo build --locked",
            client_bin.display()
        )));
    }
    let t = one_join(&work, &jar, &server_properties, &client_bin, args, 1)?;
    println!();
    println!("Normalized transcript:");
    println!("{}", serde_json::to_string_pretty(&t)?);
    Ok(())
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(64);
        }
    };
    let result = match args.command {
        Subcommand::Join => run_join(&args),
        Subcommand::Capture => run_capture(&args),
        Subcommand::Help => {
            println!("{}", usage());
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("run-scenario: {e}");
            ExitCode::FAILURE
        }
    }
}
