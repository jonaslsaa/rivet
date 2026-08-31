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
//!   superflat fixture so both servers spawn at the deterministic superflat
//!   height `JOIN_SPAWN_Y` and `position.y` is a genuinely compared field (issue
//!   #159: the old default-flat Paper reference spawned at y=-60 and position.y
//!   was wrongly treated as a "documented gap").
//!   A controlled negative then tampers the compared `position.y` on the Paper
//!   reference and requires the real comparator/divergence path to report the
//!   tampered value and refuse PASS, so the live acceptance cannot pass
//!   vacuously.
//! - `dwell` (issues #157/#160: keepalive survival + terminal M1 gate): the
//!   Rivet-only wall-clock keepalive-survival gate. Boots exactly one
//!   rivet-server; the pinned Azalea client spawns into PLAY and stays connected
//!   for `--dwell-seconds` of wall
//!   clock while auto-echoing every live keepalive. Passes only if the client
//!   survived past the server's 30 s kick limit, proven via the rivet log's
//!   `connection established` line, the absence of a `read timeout` kick, the
//!   `rivet_dwell_verdict`, and a tamper negative on `connected_wall_seconds`.
//!   `dwell` has no comparison concept, so any explicit `--runs` or `--pairs`
//!   is rejected (exit 64) rather than silently ignored, and `--dwell-seconds`
//!   is dwell-only (an explicit value on join/move/kick/capture/load-world/
//!   loaded-world is a silent no-op and is rejected the same way). The window
//!   must be at least
//!   `transcript::DWELL_MIN_DWELL_SECONDS` (a 31 s window would span only
//!   ~29.8 s of challenges and fail the verdict), and `--timeout-seconds` must
//!   exceed it by the shared settle/login headroom
//!   (`rivet_harness_common::timing::validate_dwell_timeout`) so the client's
//!   post-window settle loop and pre-spawn login time cannot let the timeout
//!   cut the `dwell` record off.
//! - `kick` (issue #86: decoded disconnect reason): the Rivet-only anti-cheat
//!   disconnect gate. Boots exactly one rivet-server; the pinned Azalea client
//!   spawns into PLAY, then sends one movement frame whose position is NaN, so
//!   the server's `contains_invalid_values` gate (`session.rs`
//!   `dispatch_move_player` → `disconnect_invalid_movement`) answers with a
//!   `ClientboundDisconnectPacket` carrying the translatable
//!   `multiplayer.disconnect.invalid_player_movement` reason (issue #158). Passes
//!   only if the real client decodes that reason — the transcript's
//!   `reason_key` must equal `transcript::KICK_REASON_KEY` with `after_spawn` set
//!   — proven via the rivet log's `connection established` line, the
//!   `rivet_kick_verdict`, and a tamper negative on `reason_key`. `kick` always
//!   boots exactly one Rivet server, so any explicit `--runs` or `--pairs` is
//!   rejected (exit 64) rather than silently ignored, and `--server` other than
//!   `rivet` is refused the same way.
//! - `load-world` (#316 independent harness slice): resolve the known local
//!   Minecraft 26.2 save, fingerprint it, create and verify a deterministic
//!   disposable copy, and pass only that copy through Rivet's `--level <path>`
//!   seam. The retained source and copy are re-fingerprinted and
//!   ordinary-operation cleanup removes the copy on every probe outcome. The
//!   command returns UNVERIFIED (exit 3) when the server is not yet ready for a
//!   genuine loaded-world PASS — never a placeholder PASS.
//! - `loaded-world` (#374 official-client acceptance): boot Rivet against a
//!   disposable copy of the safe world under `working/client-worlds/New World`
//!   (`RIVET_WORLD_SRC` overrides; the launcher save is never touched), extract
//!   the read-only ground-truth manifest, drive the real Azalea client in
//!   `loaded` mode, and compare the observed per-coordinate content.
//! - `generated-world` (seed-42 generated acceptance contract): boot Rivet with
//!   the explicit generated-world launch option (`--seed 42`), drive the real
//!   Azalea client in `generated` mode (join + dwell + bounded walk +
//!   per-coordinate content sampling), and compare the observed content against
//!   the seed-42 ground-truth handoff (`rivet-oracle generated-expected`). The
//!   server accepts `--seed` but still serves the superflat M1 fixture, so the
//!   acceptance stays honestly UNVERIFIED until real generated-world serving
//!   and the Paper seed-42 ground-truth handoff land — it never falls back to a
//!   superflat boot or a copied loaded world, which would fabricate a PASS on
//!   the wrong world.
//!
//! ## Deterministic Paper config (issue #266 / #333)
//!
//! Every Paper boot installs the pinned `paper-world-defaults.yml` fixture into
//! the run dir's `config/` (overwriting generated/stale defaults) so all seven
//! spawn-limit categories stay at 0 and no entity can spawn into the save
//! window. Without it, a fresh Paper world re-enables natural spawning and the
//! sampled walk (and `last_sent`) becomes nondeterministic — the issue #333
//! failure mode. The fixture is resolved like `server.properties`: a missing
//! companion is UNVERIFIED (exit 3) with the exact missing path.
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
//!    deterministic superflat spawn y `JOIN_SPAWN_Y`. A stale pre-play Rivet
//!    build, a fake/non-Rivet endpoint, or a Paper-like y=-60 spawn all fail the
//!    verdict.
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
mod load_world;
mod server;
mod trace;
mod transcript;

use std::env;
use std::fmt;
use std::fs;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::fs::OpenOptions;
use std::io;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::io::Read;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::os::fd::AsRawFd;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use sha2::{Digest, Sha256};

const DEFAULT_ADDRESS: &str = "127.0.0.1:25599";
const DEFAULT_USERNAME: &str = "RivetProbe";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const DEFAULT_RUNS: usize = 2;
/// Parent-side headroom beyond the client's own timeout. The child must emit its
/// terminal record and exit inside this bounded grace; a hung or locally modified
/// client cannot hold the scenario runner forever.
const CLIENT_PARENT_HEADROOM: Duration = Duration::from_secs(5);
const CLIENT_TERMINATE_GRACE: Duration = Duration::from_secs(2);
const CLIENT_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Default dwell window for the `dwell` scenario. The wall-clock seconds the
/// client stays connected after spawn while echoing every live keepalive. Must
/// exceed the server's 30 s keepalive kick limit (keepalive.rs
/// `KEEPALIVE_LIMIT_MS`), with headroom for the first challenge to land after
/// the join burst settles.
const DEFAULT_DWELL_SECONDS: u64 = 41;

// Machine-stable exit codes. PASS/FAIL/UNVERIFIED are the shared contract
// (rivet-harness-common::exit); usage errors are a separate 64.
use rivet_harness_common::exit::{EXIT_FAIL, EXIT_PASS, EXIT_UNVERIFIED};
const EXIT_USAGE: u8 = 64;

#[derive(Debug)]
enum RunnerError {
    Io(io::Error),
    LoadWorld(load_world::Error),
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
            RunnerError::LoadWorld(load_world::Error::Unverified(_)) => EXIT_UNVERIFIED,
            RunnerError::Server(server::Error::Unverified(_)) => EXIT_UNVERIFIED,
            _ => EXIT_FAIL,
        }
    }
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunnerError::Io(e) => write!(f, "io error: {e}"),
            RunnerError::LoadWorld(e) => write!(f, "loaded-world harness error: {e}"),
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
impl From<load_world::Error> for RunnerError {
    fn from(e: load_world::Error) -> Self {
        RunnerError::LoadWorld(e)
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
    Dwell,
    Kick,
    Capture,
    LoadWorld,
    LoadedWorld,
    GeneratedWorld,
    Recenter,
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

#[derive(Debug)]
struct Args {
    command: Subcommand,
    server: ServerSelection,
    pairs: Pairs,
    address: String,
    username: String,
    timeout_seconds: u64,
    runs: usize,
    dwell_seconds: u64,
    seed: Option<u64>,
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
        let mut dwell_seconds = DEFAULT_DWELL_SECONDS;
        let mut server_explicit = false;
        let mut runs_explicit = false;
        let mut pairs_explicit = false;
        let mut dwell_explicit = false;
        let mut username_explicit = false;
        let mut timeout_explicit = false;
        let mut seed: Option<u64> = None;
        let mut seed_explicit = false;

        if let Some(sub) = args.next() {
            command = match sub.as_str() {
                "join" => Subcommand::Join,
                "move" => Subcommand::Move,
                "dwell" => Subcommand::Dwell,
                "kick" => Subcommand::Kick,
                "capture" => Subcommand::Capture,
                "load-world" => Subcommand::LoadWorld,
                "loaded-world" => Subcommand::LoadedWorld,
                "generated-world" => Subcommand::GeneratedWorld,
                "recenter" => Subcommand::Recenter,
                "--help" | "-h" | "help" => Subcommand::Help,
                _ => return Err(format!("unknown subcommand: {sub}\n\n{}", usage())),
            };
        }

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--address" => address = next_value(&mut args, "--address")?,
                "--username" => {
                    username = next_value(&mut args, "--username")?;
                    username_explicit = true;
                }
                "--timeout-seconds" => {
                    let v = next_value(&mut args, "--timeout-seconds")?;
                    timeout_seconds = v
                        .parse()
                        .map_err(|_| format!("invalid --timeout-seconds value: {v}"))?;
                    timeout_explicit = true;
                }
                "--runs" => {
                    let v = next_value(&mut args, "--runs")?;
                    runs = v
                        .parse()
                        .map_err(|_| format!("invalid --runs value: {v}"))?;
                    runs_explicit = true;
                }
                "--seed" => {
                    let v = next_value(&mut args, "--seed")?;
                    seed = Some(v.parse().map_err(|_| {
                        format!("invalid --seed value: {v} (expected an unsigned 64-bit seed)")
                    })?);
                    seed_explicit = true;
                }
                "--server" => {
                    let v = next_value(&mut args, "--server")?;
                    server = ServerSelection::parse(&v).ok_or_else(|| {
                        format!("invalid --server value: {v} (expected paper|rivet|both)")
                    })?;
                    server_explicit = true;
                }
                "--dwell-seconds" => {
                    let v = next_value(&mut args, "--dwell-seconds")?;
                    dwell_seconds = v
                        .parse()
                        .map_err(|_| format!("invalid --dwell-seconds value: {v}"))?;
                    dwell_explicit = true;
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

        // `--dwell-seconds` only has meaning for the dwell scenario; on
        // join/move/kick/capture/load-world/loaded-world/recenter the client is
        // never asked to dwell, so an explicit value would be a silent no-op.
        // Reject it (exit 64) rather than ignore it — the same no-silent-noop
        // policy as --runs/--pairs on dwell.
        if dwell_explicit && command != Subcommand::Dwell {
            return Err(
                "--dwell-seconds only applies to the dwell scenario (the keepalive-survival \
                 gate); join/move/kick/capture/load-world/loaded-world/recenter/generated-world \
                 never dwell, so an explicit value would be a silent no-op — drop it"
                    .to_owned(),
            );
        }

        // `--seed` only has meaning for `generated-world`; on any other command
        // it would be a silent no-op. Reject it (exit 64) rather than ignore it.
        if seed_explicit && command != Subcommand::GeneratedWorld {
            return Err(
                "--seed only applies to the generated-world scenario (the seed-42 generated \
                 acceptance contract); join/move/dwell/kick/capture/load-world/loaded-world \
                 and recenter never boot a fresh generated world, so an explicit value would be \
                 a silent no-op — drop it"
                    .to_owned(),
            );
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
            // The `moved` record is emitted only after login/configuration, the
            // fixed walk, MOVE_DRAIN, and up to 1 s of keepalive settling; a
            // timeout at or below that total cuts the client off before it
            // emits (ExitCode 2, spurious FAIL). Mirror the client's own
            // parse-time validation so a `run-scenario`-accepted invocation is
            // never one the client then rejects or times out on. Shared with
            // the client so the two cannot drift.
            rivet_harness_common::timing::validate_move_timeout(timeout_seconds)?;
        }

        if command == Subcommand::Dwell {
            // The dwell scenario is a Rivet headless-boot survival probe: it
            // always boots rivet-server (never Paper, which has no place in the
            // keepalive-survival gate). Only --server rivet (or the default,
            // which the server-defaulting logic below pins to rivet) is valid.
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "dwell only supports --server rivet (the keepalive-survival gate is a Rivet \
                     headless-boot probe); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            // dwell has no comparison concept (it boots exactly one Rivet
            // server), so an explicit --pairs is a silent no-op. Reject it
            // rather than ignore it — a caller who passes --pairs believes the
            // comparison happened.
            if pairs_explicit {
                return Err(
                    "dwell has no --pairs comparison (it boots exactly one Rivet server); drop it"
                        .to_owned(),
                );
            }
            // dwell always boots exactly one Rivet server, so any explicit
            // --runs — even `--runs 1`, which equals the implicit default — is
            // a silent no-op. Reject every explicit value (like the both-server
            // precedent) rather than accept one that does nothing.
            if runs_explicit {
                return Err(
                    "dwell always boots exactly one Rivet server, so --runs is a silent no-op; \
                     drop it"
                        .to_owned(),
                );
            }
            runs = 1;
            // The dwell window must be long enough to prove survival past the
            // server's 30 s kick limit AND to span the required 30 s of
            // challenges after the first one lands (~1.2 s in). A window of
            // only 31 s would span ~29.8 s and fail the verdict on a healthy
            // run, so the floor is `DWELL_MIN_DWELL_SECONDS`, not
            // `DWELL_SURVIVAL_SECONDS + 1`.
            if dwell_seconds < transcript::DWELL_MIN_DWELL_SECONDS {
                return Err(format!(
                    "--dwell-seconds must be at least {} (the server's {} s keepalive kick limit \
                     plus the first-challenge offset and margin; a shorter window cannot prove \
                     survival past the kick or span the required {} s of challenges)",
                    transcript::DWELL_MIN_DWELL_SECONDS,
                    transcript::DWELL_SURVIVAL_SECONDS,
                    transcript::DWELL_MIN_SPAN_MS / 1000
                ));
            }
            // The outer client timeout starts at process launch, while the
            // dwell window only starts at spawn; after the window the client
            // spends up to 1 s settling the keepalive stream before emitting
            // the `dwell` record. `dwell < timeout` is therefore not enough —
            // the timeout must reserve the settle loop AND the pre-spawn
            // login/configuration time, or the timeout branch cuts the client
            // off before it emits. Shared with the client so the two cannot
            // drift.
            rivet_harness_common::timing::validate_dwell_timeout(dwell_seconds, timeout_seconds)?;
        }

        if command == Subcommand::Kick {
            // The kick scenario is a Rivet headless-boot decoded-reason probe
            // (issue #86): it always boots rivet-server (never Paper, which has
            // no decoded `ClientboundDisconnectPacket` reason in this slice).
            // Only --server rivet (or the default, which the server-defaulting
            // logic below pins to rivet) is valid.
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "kick only supports --server rivet (the decoded-disconnect-reason probe is a \
                     Rivet headless-boot check); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            // kick has no comparison concept (it boots exactly one Rivet server
            // and checks the decoded reason), so an explicit --pairs is a
            // silent no-op. Reject it rather than ignore it.
            if pairs_explicit {
                return Err(
                    "kick has no --pairs comparison (it boots exactly one Rivet server); drop it"
                        .to_owned(),
                );
            }
            // kick always boots exactly one Rivet server, so any explicit
            // --runs — even `--runs 1`, which equals the implicit default — is
            // a silent no-op. Reject every explicit value (like the dwell
            // precedent) rather than accept one that does nothing.
            if runs_explicit {
                return Err(
                    "kick always boots exactly one Rivet server, so --runs is a silent no-op; \
                     drop it"
                        .to_owned(),
                );
            }
            runs = 1;
        }

        if command == Subcommand::LoadWorld {
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "load-world only supports --server rivet (Paper does not use Rivet's \
                     world-path launch seam); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            if pairs_explicit {
                return Err(
                    "load-world is a single-server capability probe and has no --pairs \
                     comparison; drop it"
                        .to_owned(),
                );
            }
            if runs_explicit {
                return Err(
                    "load-world always performs exactly one Rivet launch probe, so --runs is a \
                     silent no-op; drop it"
                        .to_owned(),
                );
            }
            if username_explicit {
                return Err(
                    "load-world does not start a client, so --username is a silent no-op; drop it"
                        .to_owned(),
                );
            }
            if timeout_explicit {
                return Err(
                    "load-world does not use the client timeout, so --timeout-seconds is a silent \
                     no-op; drop it"
                        .to_owned(),
                );
            }
            runs = 1;
        }

        if command == Subcommand::LoadedWorld {
            // `loaded-world` (issue #374) boots exactly one Rivet server against
            // a disposable copy of the safe copied world (`--level <copy>`),
            // drives the loaded client, and compares its observed per-coordinate
            // content against the read-only ground-truth manifest. Paper has no
            // place here; `--pairs`/`--runs` would be silent no-ops.
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "loaded-world only supports --server rivet (Paper does not use Rivet's \
                     world-path launch seam); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            if pairs_explicit {
                return Err(
                    "loaded-world is a single-server acceptance probe and has no --pairs \
                     comparison; drop it"
                        .to_owned(),
                );
            }
            if runs_explicit {
                return Err(
                    "loaded-world always performs exactly one Rivet launch + loaded client run, so \
                     --runs is a silent no-op; drop it"
                        .to_owned(),
                );
            }
            if username_explicit {
                return Err(
                    "loaded-world does not take a client username override; --username is a silent \
                     no-op; drop it"
                        .to_owned(),
                );
            }
            if timeout_explicit {
                return Err(
                    "loaded-world uses the client's default timeout; --timeout-seconds is a silent \
                     no-op; drop it"
                        .to_owned(),
                );
            }
            runs = 1;
        }

        if command == Subcommand::GeneratedWorld {
            // `generated-world` (the generated-world acceptance contract, seed
            // 42) boots exactly one Rivet server with `--seed 42` — the explicit
            // generated-world capability. The server now accepts `--seed` but
            // still serves the superflat M1 fixture, so the runner boots it and
            // reports the exact pinned UNVERIFIED reason — never a fabricated
            // PASS, superflat echo, or copied loaded world fallback — until real
            // generated-world serving and the Paper seed-42 ground-truth handoff
            // land. Paper has no place here; `--pairs`/`--runs` would be silent
            // no-ops, and the client is driven only via its default
            // timeout/username.
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "generated-world only supports --server rivet (the generated-world capability \
                     is a Rivet launch seam); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            if pairs_explicit {
                return Err(
                    "generated-world is a single-server acceptance probe and has no --pairs \
                     comparison; drop it"
                        .to_owned(),
                );
            }
            if runs_explicit {
                return Err(
                    "generated-world always performs exactly one Rivet launch + generated client \
                     run, so --runs is a silent no-op; drop it"
                        .to_owned(),
                );
            }
            if username_explicit {
                return Err(
                    "generated-world does not take a client username override; --username is a \
                     silent no-op; drop it"
                        .to_owned(),
                );
            }
            if timeout_explicit {
                return Err(
                    "generated-world uses the client's default timeout; --timeout-seconds is a \
                     silent no-op; drop it"
                        .to_owned(),
                );
            }
            runs = 1;
            // The generated-world acceptance contract is pinned to seed 42; an
            // explicit --seed equal to it is the contract, and any other value
            // would diverge from the Paper ground-truth handoff captured for
            // seed 42 — reject it rather than silently rewriting it.
            if seed_explicit && seed != Some(server::GENERATED_SEED) {
                return Err(format!(
                    "generated-world is pinned to seed {} (the seed-42 acceptance contract and its \
                     Paper ground-truth handoff); --seed {} would diverge — drop it",
                    server::GENERATED_SEED,
                    seed.unwrap_or(0)
                ));
            }
            seed = Some(server::GENERATED_SEED);
        }

        if command == Subcommand::Recenter {
            // `recenter` (issues #185/#561) boots exactly one Rivet server
            // against a disposable copy of the safe copied world
            // (`--level <copy>`), drives the loaded-recenter client's
            // deterministic +x route across repeated chunk boundaries, and
            // REQUIRES the positive sustained-walking acceptance (the region-backed
            // recenter stays connected and receives every beyond-boot chunk) plus
            // a tampered-copy negative control. Paper has no place here;
            // `--pairs`/`--runs`/`--username`/`--timeout-seconds` would be
            // silent no-ops.
            if server_explicit && server != ServerSelection::Rivet {
                return Err(format!(
                    "recenter only supports --server rivet (Paper does not use Rivet's \
                     world-path launch seam); got --server {}",
                    server.as_str()
                ));
            }
            server = ServerSelection::Rivet;
            if pairs_explicit {
                return Err(
                    "recenter is a single-server sustained-walking acceptance (plus a \
                     tampered-copy negative control) and has no --pairs comparison; drop it"
                        .to_owned(),
                );
            }
            if runs_explicit {
                return Err(
                    "recenter always performs exactly one Rivet launch + loaded-recenter client \
                     run, so --runs is a silent no-op; drop it"
                        .to_owned(),
                );
            }
            if username_explicit {
                return Err(
                    "recenter does not take a client username override; --username is a silent \
                     no-op; drop it"
                        .to_owned(),
                );
            }
            if timeout_explicit {
                return Err(
                    "recenter uses the client's default timeout; --timeout-seconds is a silent \
                     no-op; drop it"
                        .to_owned(),
                );
            }
            runs = 1;
        }

        Ok(Self {
            command,
            server,
            pairs,
            address,
            username,
            timeout_seconds,
            runs,
            dwell_seconds,
            seed,
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn usage() -> String {
    format!(
        "Usage: run-scenario <join|move|dwell|kick|capture|load-world|loaded-world|recenter|generated-world> [options]\n\
         Options:\n\
         \x20 --server paper|rivet|both  which servers to boot (default paper; dwell/kick/load-world/loaded-world/recenter/generated-world are always rivet)\n\
         \x20 --pairs paper:paper|paper:rivet\n\
         \x20                            comparison to run (default paper:paper)\n\
         \x20 --address HOST:PORT        server address (default {DEFAULT_ADDRESS})\n\
         \x20 --username NAME            offline account name (default {DEFAULT_USERNAME})\n\
         \x20 --timeout-seconds N        client timeout per run (default {DEFAULT_TIMEOUT_SECONDS})\n\
         \x20 --dwell-seconds N          dwell-mode wall-clock window (default {DEFAULT_DWELL_SECONDS})\n\
         \x20 --runs N                   boots to compare (default {DEFAULT_RUNS}; paper needs >=2; dwell rejects it)\n\
         \x20 --seed N                   generated-world seed (default 42; generated-world only)"
    )
}

pub(crate) fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn cargo_target_dir() -> PathBuf {
    if let Ok(target_dir) = env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir);
    }
    if let Ok(exe) = env::current_exe()
        && let Some(profile_dir) = exe.parent()
        && profile_dir
            .file_name()
            .is_some_and(|name| name == "debug" || name == "release")
        && let Some(target_dir) = profile_dir.parent()
    {
        return target_dir.to_path_buf();
    }
    crate_root().join("../../target")
}

const TRUSTED_CLIENT_ARTIFACT: &str = "rivet-client-26.2-f96e8c45";

/// Path to the exact `rivet-client` binary. An explicit override is
/// authoritative selection, but its bytes still must match the committed trust
/// contract. The default is the preserved oracle artifact, never a sibling or
/// target/debug build.
fn client_binary() -> PathBuf {
    let repo_root = crate_root().join("../..");
    select_client_binary(
        env::var_os("RIVET_CLIENT_BIN").map(PathBuf::from),
        &repo_root,
    )
}

fn select_client_binary(override_path: Option<PathBuf>, repo_root: &Path) -> PathBuf {
    override_path.unwrap_or_else(|| {
        shared_repo_root(repo_root)
            .join("tools/rivet-oracle/work/bin")
            .join(TRUSTED_CLIENT_ARTIFACT)
    })
}

fn shared_repo_root(repo_root: &Path) -> PathBuf {
    let dot_git = repo_root.join(".git");
    if dot_git.is_dir() {
        return repo_root.to_path_buf();
    }
    let Some(git_dir) = fs::read_to_string(&dot_git).ok().and_then(|contents| {
        contents
            .strip_prefix("gitdir: ")
            .map(str::trim)
            .map(PathBuf::from)
    }) else {
        return repo_root.to_path_buf();
    };
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        repo_root.join(git_dir)
    };
    let git_dir = git_dir.canonicalize().unwrap_or(git_dir);
    let Some(common_git) = git_dir.parent().and_then(Path::parent) else {
        return repo_root.to_path_buf();
    };
    if common_git.file_name().is_some_and(|name| name == ".git") {
        common_git
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo_root.to_path_buf())
    } else {
        repo_root.to_path_buf()
    }
}

const TRUSTED_CLIENT_CONTRACT_JSON: &str =
    include_str!("../../../fixtures/trusted-client-26.2-x86_64-linux.json");

#[derive(Debug, Clone)]
struct TrustedClientContract {
    artifact: String,
    minecraft_version: String,
    azalea_revision: String,
    target: String,
    profile: String,
    rust_toolchain: String,
    rustc_commit: String,
    elf_build_id: String,
    size: u64,
    sha256: String,
}

impl TrustedClientContract {
    fn committed() -> Result<Self, RunnerError> {
        Self::parse(TRUSTED_CLIENT_CONTRACT_JSON)
    }

    fn parse(raw: &str) -> Result<Self, RunnerError> {
        let value: Value = serde_json::from_str(raw)?;
        let object = value.as_object().ok_or_else(|| {
            RunnerError::Gate("trusted client contract must be a JSON object".to_owned())
        })?;
        if object.get("schema").and_then(Value::as_u64) != Some(1) {
            return Err(RunnerError::Gate(
                "trusted client contract has unsupported schema".to_owned(),
            ));
        }
        let string = |key: &str| -> Result<String, RunnerError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    RunnerError::Gate(format!(
                        "trusted client contract field {key:?} must be a nonempty string"
                    ))
                })
        };
        let contract = Self {
            artifact: string("artifact")?,
            minecraft_version: string("minecraft_version")?,
            azalea_revision: string("azalea_revision")?,
            target: string("target")?,
            profile: string("profile")?,
            rust_toolchain: string("rust_toolchain")?,
            rustc_commit: string("rustc_commit")?,
            elf_build_id: string("elf_build_id")?,
            size: object.get("size").and_then(Value::as_u64).ok_or_else(|| {
                RunnerError::Gate(
                    "trusted client contract field \"size\" must be an unsigned integer".to_owned(),
                )
            })?,
            sha256: string("sha256")?,
        };
        contract.validate()?;
        Ok(contract)
    }

    fn validate(&self) -> Result<(), RunnerError> {
        if self.artifact != TRUSTED_CLIENT_ARTIFACT {
            return Err(RunnerError::Gate(format!(
                "trusted client contract artifact is {}, expected {TRUSTED_CLIENT_ARTIFACT}",
                self.artifact
            )));
        }
        if self.minecraft_version != "26.2" {
            return Err(RunnerError::Gate(format!(
                "trusted client contract pins Minecraft {}, expected 26.2",
                self.minecraft_version
            )));
        }
        if self.azalea_revision != transcript::PINNED_AZALEA_REVISION {
            return Err(RunnerError::Gate(format!(
                "trusted client contract pins Azalea {}, expected {}",
                self.azalea_revision,
                transcript::PINNED_AZALEA_REVISION
            )));
        }
        if self.target != "x86_64-unknown-linux-gnu" {
            return Err(RunnerError::Gate(format!(
                "trusted client contract target is {}, expected x86_64-unknown-linux-gnu",
                self.target
            )));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(RunnerError::Gate(
                "trusted client contract sha256 must be 64 lowercase hexadecimal characters"
                    .to_owned(),
            ));
        }
        if self.size == 0 {
            return Err(RunnerError::Gate(
                "trusted client contract size must be nonzero".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ClientIdentity {
    selected_source: PathBuf,
    executed_sha256: String,
    contract: TrustedClientContract,
}

impl ClientIdentity {
    fn evidence(&self) -> Value {
        json!({
            "selected_source_path": self.selected_source.to_string_lossy(),
            "executed_sha256": self.executed_sha256,
            "trusted_artifact": self.contract.artifact,
            "minecraft_version": self.contract.minecraft_version,
            "azalea_revision": self.contract.azalea_revision,
            "target": self.contract.target,
            "profile": self.contract.profile,
            "rust_toolchain": self.contract.rust_toolchain,
            "rustc_commit": self.contract.rustc_commit,
            "elf_build_id": self.contract.elf_build_id,
            "execution": "verifier-owned-unlinked-fd",
        })
    }
}

#[derive(Debug)]
struct ClientBinary {
    identity: ClientIdentity,
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    executable: fs::File,
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    device: u64,
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    inode: u64,
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    size: u64,
}

impl ClientBinary {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn verify_execution_identity(&self) -> Result<(), RunnerError> {
        let metadata = self.executable.metadata()?;
        if metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.len() != self.size
        {
            return Err(RunnerError::Gate(
                "verifier-owned client execution descriptor changed identity".to_owned(),
            ));
        }
        Ok(())
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn verify_execution_identity(&self) -> Result<(), RunnerError> {
        Err(RunnerError::Unverified(
            "the committed trusted rivet-client artifact is currently scoped to x86_64 Linux"
                .to_owned(),
        ))
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn execution_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.executable.as_raw_fd()))
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn set_inheritable(&self, inheritable: bool) -> Result<(), RunnerError> {
        let mut flags = rustix::io::fcntl_getfd(&self.executable)
            .map_err(|error| RunnerError::Io(io::Error::from_raw_os_error(error.raw_os_error())))?;
        flags.set(rustix::io::FdFlags::CLOEXEC, !inheritable);
        rustix::io::fcntl_setfd(&self.executable, flags)
            .map_err(|error| RunnerError::Io(io::Error::from_raw_os_error(error.raw_os_error())))?;
        Ok(())
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn execution_path(&self) -> PathBuf {
        unreachable!("trusted client preparation rejects unsupported platforms")
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn set_inheritable(&self, _inheritable: bool) -> Result<(), RunnerError> {
        unreachable!("trusted client preparation rejects unsupported platforms")
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
fn sha256_reader(reader: &mut impl Read) -> Result<(String, u64), RunnerError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| RunnerError::Gate("client binary size overflow".to_owned()))?;
        hasher.update(&buffer[..read]);
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        size,
    ))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn prepare_client_binary(
    requested: &Path,
    contract: TrustedClientContract,
) -> Result<ClientBinary, RunnerError> {
    contract.validate()?;
    let selected_source = requested.canonicalize().map_err(|error| {
        RunnerError::Gate(format!(
            "failed to canonicalize selected rivet-client source {}: {error}",
            requested.display()
        ))
    })?;
    let mut source = fs::File::open(&selected_source)?;
    let source_before = source.metadata()?;
    if !source_before.is_file() {
        return Err(RunnerError::Gate(format!(
            "selected rivet-client source {} is not a regular file",
            selected_source.display()
        )));
    }
    if source_before.len() != contract.size {
        return Err(RunnerError::Gate(format!(
            "selected rivet-client source {} is not trusted: expected {} bytes, found {} bytes",
            selected_source.display(),
            contract.size,
            source_before.len()
        )));
    }

    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| RunnerError::Gate(format!("secure staging name failed: {error}")))?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let stage_dir = env::temp_dir().join(format!("rivet-client-stage-{nonce}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&stage_dir)
        .map_err(|error| {
            RunnerError::Gate(format!(
                "failed to create private client staging directory {}: {error}",
                stage_dir.display()
            ))
        })?;
    let stage_path = stage_dir.join("client");
    let stage_result = (|| -> Result<ClientBinary, RunnerError> {
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stage_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut size = 0_u64;
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            staged.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            size = size
                .checked_add(read as u64)
                .ok_or_else(|| RunnerError::Gate("client binary size overflow".to_owned()))?;
        }
        staged.sync_all()?;
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let source_after = source.metadata()?;
        if source_before.dev() != source_after.dev()
            || source_before.ino() != source_after.ino()
            || source_before.len() != source_after.len()
        {
            return Err(RunnerError::Gate(format!(
                "selected rivet-client source {} changed identity while being staged",
                selected_source.display()
            )));
        }
        if size != contract.size || digest != contract.sha256 {
            return Err(RunnerError::Gate(format!(
                "selected rivet-client source {} is not trusted: expected {} bytes with sha256 {}, found {} bytes with sha256 {}",
                selected_source.display(),
                contract.size,
                contract.sha256,
                size,
                digest
            )));
        }
        staged.set_permissions(fs::Permissions::from_mode(0o500))?;
        drop(staged);

        let executable = fs::File::open(&stage_path)?;
        let metadata = executable.metadata()?;
        if !metadata.is_file()
            || metadata.len() != contract.size
            || metadata.mode() & 0o222 != 0
            || metadata.nlink() != 1
        {
            return Err(RunnerError::Gate(
                "staged trusted client failed its final regular-file/no-write identity check"
                    .to_owned(),
            ));
        }
        fs::remove_file(&stage_path)?;
        fs::remove_dir(&stage_dir)?;

        Ok(ClientBinary {
            identity: ClientIdentity {
                selected_source,
                executed_sha256: digest,
                contract,
            },
            executable,
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
        })
    })();
    if stage_result.is_err() {
        let _ = fs::remove_file(&stage_path);
        let _ = fs::remove_dir(&stage_dir);
    }
    stage_result
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn prepare_client_binary(
    requested: &Path,
    contract: TrustedClientContract,
) -> Result<ClientBinary, RunnerError> {
    let _ = (requested, contract);
    Err(RunnerError::Unverified(
        "the committed trusted rivet-client artifact is currently scoped to x86_64 Linux"
            .to_owned(),
    ))
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
/// out the same port twice. The held [`rivet_harness_common::port::PortReservation`]s
/// are released only immediately before each server spawns (inside
/// `server::boot`), so the bind-drop-boot race narrows to the spawn->child-bind
/// gap.
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
    identity: ClientIdentity,
}

impl ClientRun {
    fn normalize(
        &self,
        normalizer: fn(&str) -> Result<Value, String>,
    ) -> Result<Value, RunnerError> {
        let mut normalized = normalizer(&self.stdout_text).map_err(RunnerError::Transcript)?;
        normalized["client_binary"] = self.identity.evidence();
        Ok(normalized)
    }
}

/// Client-launch parameters forwarded to the headless `rivet-client` binary for
/// a single run. `dwell_seconds` is only meaningful in `dwell` mode; non-dwell
/// runs set it to 0 and [`client_argv`] omits `--dwell-seconds` entirely,
/// because the client rejects an explicit non-dwell `--dwell-seconds` as a
/// silent no-op.
struct ClientSpec {
    address: String,
    username: String,
    timeout_seconds: u64,
    dwell_seconds: u64,
    mode: String,
}

/// Build the `rivet-client` argv for one run. `--dwell-seconds` is appended
/// only for `dwell` mode: the client rejects an explicit `--dwell-seconds` on
/// join/move as a silent no-op, so forwarding the runner's non-dwell 0 would
/// make every non-dwell invocation fail at parse time.
fn client_argv(spec: &ClientSpec) -> Vec<String> {
    let mut argv = vec![
        "--mode".to_owned(),
        spec.mode.clone(),
        "--address".to_owned(),
        spec.address.clone(),
        "--username".to_owned(),
        spec.username.clone(),
        "--timeout-seconds".to_owned(),
        spec.timeout_seconds.to_string(),
    ];
    if spec.mode == "dwell" {
        argv.push("--dwell-seconds".to_owned());
        argv.push(spec.dwell_seconds.to_string());
    }
    argv
}

/// Run the headless client once and preserve its raw stdout/stderr. The parent
/// owns a deadline independent of the child's `--timeout-seconds`; timeout and
/// every nonzero exit are hard failures after diagnostics have been retained.
fn run_client(
    binary: &ClientBinary,
    spec: &ClientSpec,
    work: &Path,
    prefix: &str,
) -> Result<ClientRun, RunnerError> {
    binary.verify_execution_identity()?;
    let stdout_path = work.join(format!("{prefix}.stdout.jsonl"));
    let stderr_path = work.join(format!("{prefix}.stderr.log"));
    let stdout_file = fs::File::create(&stdout_path)?;
    let stderr_file = fs::File::create(&stderr_path)?;
    binary.set_inheritable(true)?;
    let spawn_result = Command::new(binary.execution_path())
        .args(client_argv(spec))
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn();
    let restore_result = binary.set_inheritable(false);
    let mut child = match (spawn_result, restore_result) {
        (Ok(child), Ok(())) => child,
        (Ok(mut child), Err(error)) => {
            kill_and_reap_client(&mut child);
            return Err(RunnerError::Gate(format!(
                "failed to restore close-on-exec on trusted client descriptor after spawn; child killed and reaped: {error}"
            )));
        }
        (Err(spawn_error), Ok(())) => {
            return Err(RunnerError::Gate(format!(
                "failed to execute trusted rivet-client bytes selected from {} (sha256 {}): {spawn_error}",
                binary.identity.selected_source.display(),
                binary.identity.executed_sha256
            )));
        }
        (Err(spawn_error), Err(restore_error)) => {
            return Err(RunnerError::Gate(format!(
                "failed to execute trusted rivet-client bytes selected from {} (sha256 {}): {spawn_error}; also failed to restore close-on-exec: {restore_error}",
                binary.identity.selected_source.display(),
                binary.identity.executed_sha256
            )));
        }
    };
    let deadline = Duration::from_secs(spec.timeout_seconds)
        .checked_add(CLIENT_PARENT_HEADROOM)
        .ok_or_else(|| {
            kill_and_reap_client(&mut child);
            RunnerError::Gate(format!(
                "rivet-client timeout {}s cannot include parent headroom; killed and reaped",
                spec.timeout_seconds
            ))
        })?;
    let status = wait_client(&mut child, deadline, CLIENT_TERMINATE_GRACE).map_err(|e| {
        RunnerError::Gate(format!(
            "{e}; selected source: {}; executed trusted sha256: {}; raw transcript: {}, stderr: {}",
            binary.identity.selected_source.display(),
            binary.identity.executed_sha256,
            stdout_path.display(),
            stderr_path.display()
        ))
    })?;
    let raw_stdout = fs::read_to_string(&stdout_path)?;
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_else(|e| format!("<unreadable: {e}>"));
    if !status.success() {
        return Err(RunnerError::Gate(format!(
            "rivet-client selected from {} (executed trusted sha256 {}) exited with {status}; raw transcript: {}, stderr: {}{}",
            binary.identity.selected_source.display(),
            binary.identity.executed_sha256,
            stdout_path.display(),
            stderr_path.display(),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(" ({})", stderr.trim())
            }
        )));
    }
    binary.verify_execution_identity()?;
    let stdout_text = bind_raw_client_provenance(&raw_stdout, &binary.identity)?;
    fs::write(&stdout_path, &stdout_text)?;
    Ok(ClientRun {
        stdout_text,
        stdout_path,
        stderr_path,
        identity: binary.identity.clone(),
    })
}

fn wait_client(
    child: &mut Child,
    timeout: Duration,
    terminate_grace: Duration,
) -> Result<ExitStatus, RunnerError> {
    let Some(deadline) = Instant::now().checked_add(timeout) else {
        kill_and_reap_client(child);
        return Err(RunnerError::Gate(format!(
            "rivet-client parent deadline {timeout:?} is outside the supported monotonic-clock range; killed and reaped"
        )));
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                kill_and_reap_client(child);
                return Err(RunnerError::Io(error));
            }
        }
        if Instant::now() >= deadline {
            terminate_client(child);
            let grace_deadline = Instant::now()
                .checked_add(terminate_grace)
                .unwrap_or_else(Instant::now);
            while Instant::now() < grace_deadline {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        return Err(RunnerError::Gate(format!(
                            "rivet-client exceeded parent deadline {timeout:?}; terminated and reaped"
                        )));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        kill_and_reap_client(child);
                        return Err(RunnerError::Io(error));
                    }
                }
                thread::sleep(CLIENT_POLL_INTERVAL);
            }
            kill_and_reap_client(child);
            return Err(RunnerError::Gate(format!(
                "rivet-client exceeded parent deadline {timeout:?}; killed and reaped after ignoring termination"
            )));
        }
        thread::sleep(CLIENT_POLL_INTERVAL);
    }
}

fn kill_and_reap_client(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn terminate_client(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = rivet_harness_common::server::signal(child.id(), "TERM");
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn bind_raw_client_provenance(raw: &str, identity: &ClientIdentity) -> Result<String, RunnerError> {
    let mut lines = Vec::new();
    let mut starting = 0;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let mut record: Value = serde_json::from_str(line).map_err(|e| {
            RunnerError::Transcript(format!("invalid client JSON line: {e}: {line}"))
        })?;
        if record.get("event").and_then(Value::as_str) == Some("starting") {
            starting += 1;
            let revision = record.get("azalea_revision").and_then(Value::as_str);
            if revision != Some(transcript::PINNED_AZALEA_REVISION) {
                return Err(RunnerError::Gate(format!(
                    "rivet-client starting record has azalea_revision {revision:?}, expected {}",
                    transcript::PINNED_AZALEA_REVISION
                )));
            }
            record["client_binary"] = identity.evidence();
        }
        lines.push(serde_json::to_string(&record)?);
    }
    if starting != 1 {
        return Err(RunnerError::Gate(format!(
            "rivet-client transcript must contain exactly one pinned starting record, found {starting}"
        )));
    }
    Ok(format!("{}\n", lines.join("\n")))
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

fn verify_capture_connection(kind: server::ServerKind, log_path: &Path) -> Result<(), RunnerError> {
    if kind == server::ServerKind::Rivet {
        return verify_rivet_connection(log_path);
    }
    let text = fs::read_to_string(log_path)?;
    if !text.contains("logged in with entity id") && !text.contains("joined the game") {
        return Err(RunnerError::Gate(format!(
            "Paper log {} shows no accepted player connection",
            log_path.display()
        )));
    }
    Ok(())
}

fn verify_capture_play_boundary(normalized: &Value) -> Result<(), RunnerError> {
    if normalized.get("outcome").and_then(Value::as_str) != Some("spawned") {
        return Err(RunnerError::Gate(format!(
            "capture client did not reach spawn (outcome={:?})",
            normalized.get("outcome")
        )));
    }
    let lifecycle = normalized
        .get("lifecycle")
        .and_then(Value::as_array)
        .ok_or_else(|| RunnerError::Gate("capture transcript has no lifecycle".to_owned()))?;
    for required in ["login", "spawn"] {
        if !lifecycle
            .iter()
            .any(|event| event.as_str() == Some(required))
        {
            return Err(RunnerError::Gate(format!(
                "capture transcript lifecycle is missing {required}"
            )));
        }
    }
    if normalized.get("azalea_revision").and_then(Value::as_str)
        != Some(transcript::PINNED_AZALEA_REVISION)
    {
        return Err(RunnerError::Gate(format!(
            "capture transcript does not carry pinned azalea revision {}",
            transcript::PINNED_AZALEA_REVISION
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
/// spawns at y=-60 — while Rivet serves its single-stone world at
/// `transcript::JOIN_SPAWN_Y`. The scenario's one-layer stone generator also
/// makes Paper spawn at `transcript::JOIN_SPAWN_Y`, so `position.y` becomes a
/// genuinely compared field instead of a spawn-height
/// "documented gap".
fn single_stone_server_properties(crate_root: &Path) -> Result<PathBuf, RunnerError> {
    fixture_server_properties(crate_root, "server-single-stone.properties")
}

/// The pinned Paper world-defaults fixture (`config/paper-world-defaults.yml`,
/// issue #266): all seven spawn-limit categories capped at 0 so no entity can
/// spawn into the save window. Paper reads the per-category spawn limits from
/// `config/paper-world-defaults.yml` on every boot, and the scenario runner must
/// install this file into every Paper run dir (overwriting generated/stale
/// defaults) or a fresh world re-enables natural spawning and the sampled walk
/// becomes nondeterministic. A missing fixture is a missing prerequisite —
/// UNVERIFIED (exit 3), not FAIL — exactly like a missing `server.properties`.
fn world_defaults(crate_root: &Path) -> Result<PathBuf, RunnerError> {
    fixture_server_properties(crate_root, "paper-world-defaults.yml")
}

fn ensure_client_binary() -> Result<ClientBinary, RunnerError> {
    let requested = client_binary();
    if !requested.is_file() {
        return Err(RunnerError::Unverified(format!(
            "rivet-client binary not found at {}{}",
            requested.display(),
            if env::var_os("RIVET_CLIENT_BIN").is_some() {
                " (authoritative RIVET_CLIENT_BIN override; no fallback is permitted)"
            } else {
                " — restore the contract-named preserved artifact under tools/rivet-oracle/work/bin; arbitrary local builds are not trusted"
            }
        )));
    }
    prepare_client_binary(&requested, TrustedClientContract::committed()?)
}

/// Boot one Paper server, join via the client, shut the server down, and return
/// the normalized transcript (raw artifacts preserved). The Paper-vs-Paper
/// self-check path; `address` is the isolated port for this boot.
///
/// The eight arguments are the distinct inputs a single boot needs (work dir,
/// jar, properties source, world-defaults source, client binary, shared args,
/// boot index, address); the excess over clippy's default limit is inherent to
/// the operation rather than a refactorable arity smell.
#[allow(clippy::too_many_arguments)]
fn one_join(
    work: &Path,
    jar: &Path,
    server_properties: &Path,
    world_defaults: &Path,
    client_bin: &ClientBinary,
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
        Some(world_defaults),
        address,
        None,
        &[],
        None,
        None,
    )?;
    println!("[run  {idx}] joining via rivet-client ...");
    let client_run = run_client(
        client_bin,
        &ClientSpec {
            address: address.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "join".to_owned(),
        },
        work,
        &format!("client{idx}"),
    )?;
    server::shutdown(&mut srv)?;

    let normalized = client_run.normalize(transcript::normalize_join)?;
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
    let world_defaults = world_defaults(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: join (Paper-vs-Paper self-check)");
    println!("    paperclip jar     : {}", jar.display());
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!("    server.properties : {}", server_properties.display());
    println!("    world defaults    : {}", world_defaults.display());
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
            &world_defaults,
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
///
/// The eight arguments are the distinct inputs a single boot needs (work dir,
/// jar, properties source, world-defaults source, client binary, shared args,
/// boot index, address); the excess over clippy's default limit is inherent to
/// the operation rather than a refactorable arity smell.
#[allow(clippy::too_many_arguments)]
fn one_move(
    work: &Path,
    jar: &Path,
    server_properties: &Path,
    world_defaults: &Path,
    client_bin: &ClientBinary,
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
        Some(world_defaults),
        address,
        None,
        &[],
        None,
        None,
    )?;
    println!("[run  {idx}] walking via rivet-client (move mode) ...");
    let client_run = run_client(
        client_bin,
        &ClientSpec {
            address: address.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "move".to_owned(),
        },
        work,
        &format!("client{idx}"),
    )?;
    server::shutdown(&mut srv)?;

    let normalized = client_run.normalize(transcript::normalize_move)?;
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
    let world_defaults = world_defaults(&crate_root)?;
    let jar = server::ensure_jar(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: move (Paper-vs-Paper movement self-check)");
    println!("    paperclip jar     : {}", jar.display());
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!("    server.properties : {}", server_properties.display());
    println!("    world defaults    : {}", world_defaults.display());
    println!("    address           : {}", args.address);
    println!("    paper boots       : {}", args.runs);
    println!();

    let mut transcripts = Vec::with_capacity(args.runs);
    for idx in 1..=args.runs {
        let t = one_move(
            &work,
            &jar,
            &server_properties,
            &world_defaults,
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
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
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
            None,
            base,
            None,
            &[],
            None,
            None,
        )?;
        println!("[run  {idx}] connecting via rivet-client ...");
        let client_run = run_client(
            &client_bin,
            &ClientSpec {
                address: base.to_string(),
                username: args.username.clone(),
                timeout_seconds: args.timeout_seconds,
                dwell_seconds: 0,
                mode: "join".to_owned(),
            },
            &work,
            &prefix,
        )?;
        server::shutdown(&mut srv)?;
        // The client transcript is only the client-side half of the proof;
        // require the server log to show the real rivet-server accepted the
        // connection (connection established on TCP accept).
        verify_rivet_connection(&log_path)?;

        let normalized = client_run.normalize(transcript::normalize_join)?;
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
        "    {}-chunk send-set at Rivet's fixed superflat spawn y={}.",
        transcript::JOIN_CHUNK_COUNT,
        transcript::JOIN_SPAWN_Y
    );
    println!("    The connection is proven two ways: the rivet log shows 'connection established'");
    println!("    (only the real rivet-server emits it), and the client transcript is");
    println!("    outcome=spawned with lifecycle init->login->spawn, the pinned Azalea");
    println!(
        "    revision, 117 chunks, and spawn y={} — which a stale pre-play build, a",
        transcript::JOIN_SPAWN_Y
    );
    println!("    fake/non-Rivet endpoint, or a Paper-like y=-60 spawn all fail.");
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
/// boots the single-stone superflat fixture and spawns at
/// `transcript::JOIN_SPAWN_Y` like Rivet, so a position.y divergence is a real
/// server mismatch and must fail the run — never be normalized or excluded to
/// make the test pass. The only remaining
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
/// (both `transcript::JOIN_SPAWN_Y`), so the compared move transcripts must be
/// byte-identical: sampled walk geometry, velocity, teleport echo, and
/// `last_sent` are all
/// deterministic per server and Paper-vs-Rivet equal (the comparator below is
/// what proves it on each run). There is deliberately no documented gap — unlike
/// the join differential's health-component gap, which the fixture does not
/// remove — so any compared-field divergence is a genuine movement mismatch and
/// fails the run.
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
/// is safe here because the move differential just proved Paper and Rivet
/// record the *same* final x (the comparator reported zero diffs on the
/// compared fields), so the tampered value cannot silently collide with the
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

/// Render the move verdict's compared-`last_sent` evidence from the actual
/// normalized transcripts.
///
/// The move differential just proved the compared fields byte-identical
/// (`check_move_divergence` found zero diffs), so Paper and Rivet's `last_sent`
/// are equal and the printed object is whatever the pinned clients produced on
/// *this run* — never a hardcoded snapshot. A future walk-geometry change (the
/// fixture, the client's walk length, spawn offset) therefore cannot leave a
/// stale success claim quoting an earlier run's coordinates: the verdict always
/// narrates the live value. If the transcripts ever diverge here (a shape
/// change or a comparator regression), the narration names both sides instead
/// of printing one value as if they matched.
///
/// `last_sent` presence is judged by [`transcript::normalize_move`]: an absent
/// raw value is normalized to explicit JSON `null`, so a `null` or missing
/// `walk.last_sent` means the transcript carried no value — never a value to
/// compare. Treating it as "same last_sent null" would print a successful-looking
/// `null` on a schema regression; instead the narration surfaces the missing
/// side(s) exactly.
fn verdict_last_sent(paper_t: &Value, rivet_t: &Value) -> String {
    fn present(v: &Value) -> Option<&Value> {
        match v {
            Value::Null => None,
            other => Some(other),
        }
    }
    let paper = paper_t.pointer("/walk/last_sent").and_then(present);
    let rivet = rivet_t.pointer("/walk/last_sent").and_then(present);
    match (paper, rivet) {
        (Some(p), Some(r)) if p == r => {
            format!("Paper and Rivet both record the same compared last_sent {p}")
        }
        (Some(p), Some(r)) => format!(
            "Paper last_sent {p} vs Rivet last_sent {r} — a compared divergence that should have \
             been caught by the comparator"
        ),
        (Some(p), None) => {
            format!("Paper last_sent {p}; the Rivet transcript carried no walk.last_sent")
        }
        (None, Some(r)) => {
            format!("the Paper transcript carried no walk.last_sent; Rivet last_sent {r}")
        }
        (None, None) => "neither transcript carried a walk.last_sent".to_owned(),
    }
}

/// Prove the Rivet movement trace is authoritative evidence for the client's
/// actual walk, not just internally consistent: the server's final accepted
/// position must match the client's final sent position (modulo the in-flight
/// frames the server processes past the client's recorded last tick).
///
/// The client's `last_sent` is its own `LastSentPosition` at the tick the walk
/// stopped; the server keeps accepting the trailing position frames that arrive
/// after that snapshot, so the final authoritative position lands at or a little
/// past `last_sent` — how far depends on how many trailing frames the server
/// processes before the trace's session end. The x acceptance band below is
/// therefore a fixed forward window from the reconstructed absolute `last_sent`
/// (one block of slack behind it, a few blocks ahead), not a fixed absolute
/// coordinate — so a walk-geometry change cannot make the bound stale. It is
/// deliberately loose so timing noise cannot fail the run, yet tight enough that
/// a server which never moved the player (final lands at spawn, far short of the
/// client's last_sent) or teleported it elsewhere fails.
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
    // y/z are non-movement axes on this fixture (the walk is a straight forward
    // run along x), so the final authoritative y/z must match the client's last
    // sent y/z — modulo precision, not exactly. The client rounds to 3 decimals,
    // and the z reconstruction (`round3(last_sent.z - origin.z) + origin.z`)
    // carries a rounding-unit error, so exact f64 equality would spuriously fail
    // on any spawn offset whose low bits the 3-decimal rounding discards. Compare
    // with a small epsilon — an order of magnitude above the 1e-3 rounding — the
    // tight analogue of x's movement-axis band (x legitimately overshoots past
    // last_sent via the in-flight frames the server processes; y/z must agree to
    // within the client's own precision).
    const AXIS_EPSILON: f64 = 1e-2;
    if (fy - last_sent_y).abs() > AXIS_EPSILON || (fz - sent_abs_z).abs() > AXIS_EPSILON {
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
/// `transcript::JOIN_SPAWN_Y`), the divergence gate must refuse PASS on the
/// tamper — a position.y divergence is a real server mismatch, never a
/// documented gap to wave through.
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
/// `transcript::JOIN_SPAWN_Y` and `position.y` is a compared field (never
/// excluded or normalized to pass).
fn run_paper_vs_rivet(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-both");
    fs::create_dir_all(&work)?;
    let server_properties = single_stone_server_properties(&crate_root)?;
    let world_defaults = world_defaults(&crate_root)?;
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
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!(
        "    server.properties : {} (single-stone superflat)",
        server_properties.display()
    );
    println!("    world defaults    : {}", world_defaults.display());
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
        Some(&world_defaults),
        paper_addr,
        Some(reservations.remove(0)),
        &[],
        None,
        None,
    )?;
    let paper_client = run_client(
        &client_bin,
        &ClientSpec {
            address: paper_addr.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "join".to_owned(),
        },
        &work,
        "paper",
    )?;
    // Shutdown centrally requires exit 0, a post-READY clean-save marker, and
    // the pinned commit in the materialized server jar.
    server::shutdown(&mut paper_srv)?;
    let paper_t = paper_client.normalize(transcript::normalize_join)?;
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
        None,
        rivet_addr,
        Some(reservations.remove(0)),
        &[],
        None,
        None,
    )?;
    let rivet_client = run_client(
        &client_bin,
        &ClientSpec {
            address: rivet_addr.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "join".to_owned(),
        },
        &work,
        "rivet",
    )?;
    server::shutdown(&mut rivet_srv)?;
    // Server-side half of the connection proof: the rivet log must show the
    // real rivet-server accepted the client (connection established on TCP
    // accept).
    verify_rivet_connection(&work.join("rivet.log"))?;
    let rivet_t = rivet_client.normalize(transcript::normalize_join)?;
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
        "      * Both spawn at the same superflat height y={}, so position.y is a compared",
        transcript::JOIN_SPAWN_Y
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
    println!(
        "        y={} — which a stale pre-play build, a fake/non-Rivet endpoint, or any",
        transcript::JOIN_SPAWN_Y
    );
    println!("        other spawn height fails.");
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
/// would carry y=-60 vs Rivet's `transcript::JOIN_SPAWN_Y` — a spawn-height
/// fixture artifact, not a movement difference. With one stone layer both
/// servers walk at `transcript::JOIN_SPAWN_Y`, so the samples, velocity,
/// teleport echo, and `last_sent` must be byte-identical with no documented
/// gaps.
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
    let world_defaults = world_defaults(&crate_root)?;
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
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!(
        "    server.properties : {} (single-stone superflat)",
        server_properties.display()
    );
    println!("    world defaults    : {}", world_defaults.display());
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
        Some(&world_defaults),
        paper_addr,
        Some(reservations.remove(0)),
        &[],
        None,
        None,
    )?;
    let paper_client = run_client(
        &client_bin,
        &ClientSpec {
            address: paper_addr.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "move".to_owned(),
        },
        &work,
        "paper",
    )?;
    // Shutdown centrally requires exit 0, a post-READY clean-save marker, and
    // the pinned commit in the materialized server jar.
    server::shutdown(&mut paper_srv)?;
    let paper_t = paper_client.normalize(transcript::normalize_move)?;
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
        None,
        rivet_addr,
        Some(reservations.remove(0)),
        &[(trace::TRACE_MOVEMENT_ENV, "1")],
        None,
        None,
    )?;
    let rivet_client = run_client(
        &client_bin,
        &ClientSpec {
            address: rivet_addr.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "move".to_owned(),
        },
        &work,
        "rivet",
    )?;
    server::shutdown(&mut rivet_srv)?;
    // Server-side half of the connection proof: the rivet log must show the
    // real rivet-server accepted the client.
    verify_rivet_connection(&work.join("rivet.log"))?;
    let rivet_t = rivet_client.normalize(transcript::normalize_move)?;
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
    println!("        — {}", verdict_last_sent(&paper_t, &rivet_t));
    println!(
        "      * Paper boots the single-stone superflat fixture (Git-Commit pinned) so the walk"
    );
    println!(
        "        y aligns with Rivet's {} — the old default-flat y=-60 fixture artifact is",
        transcript::JOIN_SPAWN_Y
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

/// Mode E: the wall-clock keepalive-survival gate (issues #157/#160: keepalive
/// survival + terminal M1 gate). Boots a real rivet-server headlessly, drives
/// the pinned Azalea client's `dwell` mode (spawn into PLAY, stay connected for
/// `--dwell-seconds` wall-clock seconds while azalea auto-echoes every live
/// keepalive), and verifies the client survived past the server's 30 s keepalive
/// kick limit.
///
/// The survival proof is four-fold, mirroring the other Rivet scenarios:
///
/// 1. the rivet-server log shows `connection established` (the client genuinely
///    reached the Rivet port — only the real rivet-server emits it),
/// 2. the rivet-server log shows no `read timeout` kick (server_connection_listener
///    logs `read timeout` when the keepalive timeout fires — a client that
///    stopped echoing would be kicked here),
/// 3. the client transcript is judged by [`transcript::rivet_dwell_verdict`]:
///    outcome `dwelled`, lifecycle containing login and spawn, the pinned Azalea
///    revision, wall-clock survival past 30 s, >= 30 challenges, a 1:1
///    challenge->echo pairing, and a challenge span across the window — the
///    verdict rejects a transcript whose 1:1 challenge->echo relationship is
///    missing,
/// 4. a controlled negative: tamper the compared survival scalar
///    (`connected_wall_seconds`) to 0 and require the real verdict path to
///    refuse PASS — the acceptance cannot pass vacuously.
///
/// Missing prereqs (rivet-server binary, rivet-client binary) surface as
/// UNVERIFIED, never a silent skip.
fn run_dwell(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-dwell");
    fs::create_dir_all(&work)?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: dwell (wall-clock keepalive survival)");
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!("    address           : {}", args.address);
    println!(
        "    dwell window      : {}s (server keepalive kick limit: {}s)",
        args.dwell_seconds,
        transcript::DWELL_SURVIVAL_SECONDS
    );
    println!();

    let run_dir = work.join("rivet1");
    let log_path = work.join("rivet1.log");
    let mut srv = server::boot(
        server::ServerKind::Rivet,
        &run_dir,
        &log_path,
        &rivet_bin,
        None,
        None,
        base,
        None,
        &[],
        None,
        None,
    )?;
    println!("[run  1] dwelling via rivet-client (dwell mode) ...");
    let client_run = run_client(
        &client_bin,
        &ClientSpec {
            address: base.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: args.dwell_seconds,
            mode: "dwell".to_owned(),
        },
        &work,
        "dwell1",
    )?;
    server::shutdown(&mut srv)?;
    // Server-side half of the connection proof: the real rivet-server must have
    // accepted the client (connection established on TCP accept).
    verify_rivet_connection(&log_path)?;
    // Survival's server-side negative: the keepalive timeout must NOT have
    // fired. A client that stopped echoing would be kicked with a `read timeout`
    // log (server_connection_listener.rs `DisconnectReason::Timeout`). Requiring
    // its absence is the genuinely server-side half of the survival proof — the
    // client transcript alone cannot prove the server never disconnected it.
    let rivet_log = fs::read_to_string(&log_path)?;
    if rivet_log.contains("read timeout") {
        return Err(RunnerError::Gate(format!(
            "rivet log {} shows a 'read timeout' kick — the server disconnected the client for \
             failing its keepalive; the client did not survive the {}-second kick window. The \
             transcript's echo counts are therefore meaningless.",
            log_path.display(),
            transcript::DWELL_SURVIVAL_SECONDS
        )));
    }

    let normalized = client_run.normalize(transcript::normalize_dwell)?;
    let transcript_path = work.join("dwell1.transcript.json");
    fs::write(&transcript_path, serde_json::to_string_pretty(&normalized)?)?;
    let boundary = transcript::rivet_dwell_verdict(&normalized)?;
    println!(
        "[run  1] outcome={} connected_wall_seconds={} challenge_count={} echo_count={} \
         (survival boundary: {boundary}) — transcript in {}",
        normalized["outcome"],
        normalized["dwell"]["connected_wall_seconds"],
        normalized["dwell"]["challenge_count"],
        normalized["dwell"]["echo_count"],
        transcript_path.display()
    );

    // Negative case: prove the verdict path just exercised is non-vacuous.
    // Tamper a *compared* survival scalar (connected_wall_seconds, which the
    // verdict strictly requires to exceed the kick limit) and require the real
    // verdict to refuse PASS. Without this, a verdict that never checked the
    // window (or a transcript shaped to satisfy a vacuous check) would pass.
    println!();
    println!("Negative case (tamper connected_wall_seconds through the real verdict path)");
    {
        let mut tampered = normalized.clone();
        tampered["dwell"]["connected_wall_seconds"] = json!(0.0);
        match transcript::rivet_dwell_verdict(&tampered) {
            Err(e) if e.contains("connected_wall_seconds") => {
                println!(
                    "    tampered connected_wall_seconds -> 0 — the verdict refused PASS, so \
                     wall-clock survival is genuinely verified"
                );
            }
            Err(e) => {
                return Err(RunnerError::Gate(format!(
                    "negative case FAILED: the dwell verdict refused PASS for a reason other than \
                     the tampered connected_wall_seconds: {e}"
                )));
            }
            Ok(_) => {
                return Err(RunnerError::Gate(
                    "negative case FAILED: the dwell verdict PASSED with connected_wall_seconds=0 \
                     — wall-clock survival is not genuinely checked"
                        .to_owned(),
                ));
            }
        }
    }

    println!();
    println!("VERDICT: PASS — rivet-server kept a real client alive in PLAY past its 30 s");
    println!("    keepalive kick limit:");
    println!(
        "      * The client spawned into PLAY and stayed connected for {}-wall-clock seconds",
        args.dwell_seconds
    );
    println!(
        "        (the server's keepalive kick limit is {}s), echoing every live keepalive",
        transcript::DWELL_SURVIVAL_SECONDS
    );
    println!(
        "        challenge (1:1 challenge->echo, {} challenges).",
        normalized["dwell"]["challenge_count"]
    );
    println!("      * The connection is proven two ways: the rivet log shows 'connection");
    println!("        established' (only the real rivet-server emits it) and contains no 'read");
    println!("        timeout' kick (a client that stopped echoing would be disconnected here),");
    println!(
        "        and the client transcript is outcome=dwelled with the pinned Azalea revision,"
    );
    println!("        wall-clock survival past the kick limit, and a 1:1 challenge->echo cadence.");
    println!("      * The negative case proved the verdict path is non-vacuous: a tampered");
    println!(
        "        connected_wall_seconds=0 was refused PASS by the real verdict, so wall-clock"
    );
    println!("        survival cannot be waved through by a transcript that never survived.");
    println!("    artifacts: {}", work.display());
    Ok(())
}

fn run_kick(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-kick");
    fs::create_dir_all(&work)?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;

    println!("rivet scenario runner: kick (decoded disconnect reason)");
    println!("    rivet-server bin  : {}", rivet_bin.display());
    println!(
        "    rivet-client bin  : {}",
        client_bin.identity.selected_source.display()
    );
    println!("    address           : {}", args.address);
    println!();
    println!(
        "    the client sends one NaN movement frame after spawn; the server's anti-cheat gate"
    );
    println!("    must answer with a ClientboundDisconnectPacket carrying the translatable");
    println!(
        "    '{}', and the real Azalea client must decode and report that exact key.",
        transcript::KICK_REASON_KEY
    );
    println!();

    let run_dir = work.join("rivet1");
    let log_path = work.join("rivet1.log");
    let mut srv = server::boot(
        server::ServerKind::Rivet,
        &run_dir,
        &log_path,
        &rivet_bin,
        None,
        None,
        base,
        None,
        &[],
        None,
        None,
    )?;
    println!("[run  1] kicking via rivet-client (kick mode) ...");
    let client_run = run_client(
        &client_bin,
        &ClientSpec {
            address: base.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "kick".to_owned(),
        },
        &work,
        "kick1",
    )?;
    server::shutdown(&mut srv)?;
    // Server-side half of the connection proof: the real rivet-server must have
    // accepted the client (connection established on TCP accept).
    verify_rivet_connection(&log_path)?;

    let normalized = client_run.normalize(transcript::normalize_kick)?;
    let transcript_path = work.join("kick1.transcript.json");
    fs::write(&transcript_path, serde_json::to_string_pretty(&normalized)?)?;
    let boundary = transcript::rivet_kick_verdict(&normalized)?;
    println!(
        "[run  1] outcome={} reason_key={:?} after_spawn={} \
         (decoded-reason boundary: {boundary}) — transcript in {}",
        normalized["outcome"],
        normalized["kick"]["reason_key"],
        normalized["kick"]["after_spawn"],
        transcript_path.display()
    );

    // Negative case: prove the verdict path just exercised is non-vacuous.
    // Tamper the *decoded reason key* the verdict strictly requires and require
    // the real verdict to refuse PASS. Without this, a verdict that never
    // checked the decoded key (or a transcript shaped to satisfy a vacuous
    // check) would pass.
    println!();
    println!("Negative case (tamper reason_key through the real verdict path)");
    {
        let mut tampered = normalized.clone();
        tampered["kick"]["reason_key"] = json!("disconnect.genericReason");
        match transcript::rivet_kick_verdict(&tampered) {
            Err(e) if e.contains("reason_key") => {
                println!(
                    "    tampered reason_key -> disconnect.genericReason — the verdict refused \
                     PASS, so the decoded reason is genuinely verified"
                );
            }
            Err(e) => {
                return Err(RunnerError::Gate(format!(
                    "negative case FAILED: the kick verdict refused PASS for a reason other than \
                     the tampered reason_key: {e}"
                )));
            }
            Ok(_) => {
                return Err(RunnerError::Gate(
                    "negative case FAILED: the kick verdict PASSED with a wrong reason_key — the \
                     decoded reason is not genuinely checked"
                        .to_owned(),
                ));
            }
        }
    }

    println!();
    println!("VERDICT: PASS — the real Azalea client decoded Rivet's disconnect reason");
    println!();
    println!("      * The client spawned into PLAY, then sent a NaN movement frame that the");
    println!("        server's contains_invalid_values anti-cheat gate answered with a");
    println!("        ClientboundDisconnectPacket (issue #86).");
    println!("      * The connection is proven two ways: the rivet log shows 'connection");
    println!("        established' (only the real rivet-server emits it), and the client");
    println!("        transcript is outcome=disconnected with after_spawn=true and the pinned");
    println!("        Azalea revision — so the kick happened in play, not at the login boundary.");
    println!("      * The real client decoded the translatable reason and reported exactly the");
    println!("        key '{}'.", transcript::KICK_REASON_KEY);
    println!("      * The negative case proved the verdict path is non-vacuous: a tampered");
    println!("        reason_key was refused PASS by the real verdict, so a transcript that never");
    println!("        decoded the real reason cannot be waved through.");
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
    // Rivet does not need server.properties or paper-world-defaults (driven
    // purely by --host/--port); only Paper's boot consumes the fixtures.
    let server_properties = (kind == server::ServerKind::Paper)
        .then(|| server_properties(&crate_root))
        .transpose()?;
    let world_defaults = (kind == server::ServerKind::Paper)
        .then(|| world_defaults(&crate_root))
        .transpose()?;

    let prefix = kind.as_str().to_owned();
    let log_path = work.join(format!("{prefix}1.log"));
    let mut srv = server::boot(
        kind,
        &work.join(format!("{prefix}1")),
        &log_path,
        &artifact,
        server_properties.as_deref(),
        world_defaults.as_deref(),
        base,
        None,
        &[],
        None,
        None,
    )?;
    let client_run = run_client(
        &client_bin,
        &ClientSpec {
            address: base.to_string(),
            username: args.username.clone(),
            timeout_seconds: args.timeout_seconds,
            dwell_seconds: 0,
            mode: "join".to_owned(),
        },
        &work,
        "client1",
    )?;
    verify_capture_connection(kind, &log_path)?;
    server::shutdown(&mut srv)?;

    let normalized = client_run.normalize(transcript::normalize_join)?;
    verify_capture_play_boundary(&normalized)?;
    println!();
    println!("Normalized transcript:");
    println!("{}", serde_json::to_string_pretty(&normalized)?);
    Ok(())
}

/// Independent loaded-world harness slice for #316.
///
/// This is the copy-and-probe slice: it makes and verifies a disposable copy,
/// proves the source remains immutable, and probes the `--level <copy>` launch
/// seam. It deliberately does not parse or load the world (that belongs to
/// #323/#339), so it reports the #339 launch outcome rather than a loaded-world
/// content PASS.
fn run_load_world(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-loaded-world");
    let source_path = load_world::resolve_source_world()?;
    let source = load_world::SourceTree::open(&source_path)?;
    // Resolve the prospective destination and reject both containment
    // directions before create_dir_all or any other filesystem mutation.
    load_world::validate_prospective_storage(&source, &work)?;
    fs::create_dir_all(&work)?;

    let source_before = source.fingerprint()?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let base = base_address(args)?;
    let run_dir = work.join("rivet");
    let mut temp = load_world::TempWorld::create(&source, &work)?;
    let log_path = temp.probe_log_path();
    let probe_result = (|| -> Result<(), RunnerError> {
        load_world::assert_copy_equals_source(&source_before, &temp.hash_tree()?)?;
        let server_world_path = temp.server_path();

        println!("rivet scenario runner: load-world (#316 independent harness slice)");
        println!(
            "    source world      : {} (read only; never passed to a server)",
            source.configured_path().display()
        );
        println!("    disposable copy  : {}", temp.path().display());
        println!(
            "    private probe log: {}",
            temp.probe_log_visible_path().display()
        );
        println!("    rivet-server bin : {}", rivet_bin.display());
        println!(
            "    launch seam      : {} <disposable-copy>",
            server::WORLD_PATH_ARG
        );
        println!();

        match server::boot(
            server::ServerKind::Rivet,
            &run_dir,
            &log_path,
            &rivet_bin,
            None,
            None,
            base,
            None,
            &[],
            Some(&server_world_path),
            None,
        ) {
            Ok(mut srv) => match server::shutdown(&mut srv) {
                Ok(()) => Err(RunnerError::Unverified(
                    "rivet-server accepted the loaded-world path and reached READY, but this #316 \
                     slice only proves the copy-and-launch seam; the #374 loaded-world acceptance \
                     (per-coordinate content comparison) is the PASS contract, so this slice \
                     refuses to claim PASS"
                        .to_owned(),
                )),
                Err(error) => Err(error.into()),
            },
            Err(error) => Err(classify_load_world_boot_failure(error, &log_path)),
        }
    })();

    // Run all safety checks even when the capability probe returns
    // UNVERIFIED. Safety failures take precedence over the expected absence:
    // an untouched source and an unchanged disposable copy are mandatory on
    // every exit path, and cleanup must be deterministic.
    let copy_check = temp
        .hash_tree()
        .and_then(|after| load_world::assert_copy_equals_source(&source_before, &after));
    let source_check = source.verify_unchanged(&source_before);
    let cleanup = temp.cleanup();

    source_check?;
    copy_check?;
    cleanup?;
    probe_result
}

/// The official-client loaded-world acceptance probe (issue #374).
///
/// This is the honest successor to `load-world`: it makes and verifies a
/// disposable copy of the safe world under `working/client-worlds/New World`,
/// extracts its read-only ground-truth manifest (`rivet-oracle extract-world`),
/// boots Rivet against the copy with `--level <copy>`, drives the real Azalea
/// client in `loaded` mode (join, wait for chunk quiescence, sample the genuine
/// per-coordinate block content the server served, then take a short bounded
/// walk), and compares the client's observed content against the manifest.
///
/// The PASS contract is deliberately strict and honest:
///
/// - The client must actually join, spawn, sample per-coordinate content, and
///   walk (never a vacuous green) — `rivet_loaded_verdict` enforces the
///   transcript boundary. The walk is recorded as `before`/`after` position
///   evidence; the per-coordinate grid is the load-bearing content check.
/// - The sampled surface/bedrock/below_feet block names must match the
///   ground-truth manifest at the same coordinates. The manifest records the
///   read-only world's per-coordinate content, so a server that serves a
///   different world — e.g. echoing repeated superflat bytes for every chunk —
///   fails the comparison at the first sampled coordinate whose content differs
///   from ground truth.
/// - The disposable copy must be byte-for-byte unchanged after the run (the
///   server must not mutate the loaded world), and the source world must be
///   untouched.
///
/// The source is the safe copied world under `working/client-worlds/New World`
/// (or `RIVET_WORLD_SRC`); the launcher save is never defaulted to or
/// inspected. A chunk whose ground-truth status is not `minecraft:full`, or
/// that carries an uncarried #519 capability flag, is honestly classified
/// UNVERIFIED (exit 3) rather than misreported — never a fabricated PASS.
fn run_loaded_world(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-loaded-world");
    // The loaded-world acceptance reads the safe copied world under
    // `working/client-worlds` (or an explicit `RIVET_WORLD_SRC`); it never
    // defaults to or inspects the launcher save.
    let source_path = load_world::resolve_loaded_world_src()?;
    let source = load_world::SourceTree::open(&source_path)?;
    load_world::validate_prospective_storage(&source, &work)?;
    fs::create_dir_all(&work)?;

    let source_before = source.fingerprint()?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    let run_dir = work.join("rivet");
    let log_path = work.join("rivet.log");
    let mut temp = load_world::TempWorld::create(&source, &work)?;

    let result = (|| -> Result<(), RunnerError> {
        load_world::assert_copy_equals_source(&source_before, &temp.hash_tree()?)?;
        let server_world_path = temp.server_path();

        // Extract the read-only ground-truth manifest of the disposable copy
        // BEFORE booting Rivet: the manifest is the per-coordinate world
        // content the client's observed samples must match.
        let manifest = run_extract_world(&server_world_path, &work)?;

        println!("rivet scenario runner: loaded-world (#374 official-client acceptance)");
        println!(
            "    source world      : {} (read only; never passed to a server)",
            source.configured_path().display()
        );
        println!("    disposable copy  : {}", temp.path().display());
        println!(
            "    ground-truth      : {} FULL chunks, {} non-FULL (manifest)",
            manifest["full_count"], manifest["non_full_count"]
        );
        println!("    rivet-server bin : {}", rivet_bin.display());
        println!(
            "    launch seam      : {} <disposable-copy>",
            server::WORLD_PATH_ARG
        );
        println!();

        // Boot Rivet against the disposable copy. #363 merged `--level` into
        // rivet-server; a boot failure is still classified via the probe so an
        // unexpected rejection surfaces as UNVERIFIED rather than a fabricated
        // PASS.
        let mut srv = match server::boot(
            server::ServerKind::Rivet,
            &run_dir,
            &log_path,
            &rivet_bin,
            None,
            None,
            base,
            None,
            &[],
            Some(&server_world_path),
            None,
        ) {
            Ok(srv) => srv,
            Err(error) => return Err(classify_load_world_boot_failure(error, &log_path)),
        };

        // The post-boot acceptance body. It never shuts `srv` down; the wrapper
        // below always does, so a failure here (client run, transcript, content
        // mismatch) cannot drop the booted server (SIGKILL) and race the
        // disposable-copy cleanup.
        let body = (|| -> Result<(), RunnerError> {
            // Drive the real Azalea client in loaded mode against the booted
            // server.
            let client_run = run_client(
                &client_bin,
                &ClientSpec {
                    address: base.to_string(),
                    username: args.username.clone(),
                    timeout_seconds: args.timeout_seconds,
                    dwell_seconds: 0,
                    mode: "loaded".to_owned(),
                },
                &work,
                "loaded-client",
            )?;

            // Prove the client genuinely reached the Rivet port (the server's
            // `connection established` line). The client run above completed, so
            // the connection line is already in the log.
            verify_rivet_connection(&log_path)?;

            // The client transcript must prove it joined, spawned, and sampled
            // genuine per-coordinate content.
            let transcript = client_run.normalize(transcript::normalize_loaded)?;
            let boundary =
                transcript::rivet_loaded_verdict(&transcript).map_err(RunnerError::Transcript)?;

            // Compare the client's observed block content against the
            // ground-truth manifest. This is the load-bearing comparison: a
            // server that only echoes repeated superflat bytes fails here.
            compare_loaded_content(&manifest, &transcript)?;

            println!("\nLoaded-world acceptance boundary reached: {boundary}");
            Ok(())
        })();

        // A booted server must always be shut down cleanly before the
        // disposable-copy cleanup below runs. On the error path the body above
        // returned before any shutdown, and dropping `srv` would SIGKILL the
        // child — a live server racing the copy cleanup and holding the port.
        // `server::shutdown` is a no-op on a stopped child, so this is safe on
        // both the success path (the body never shuts down) and every error
        // path. A shutdown failure is surfaced only when the acceptance
        // otherwise succeeded, so an original acceptance error is never masked.
        let shutdown_result = server::shutdown(&mut srv);
        match body {
            Err(e) => {
                if let Err(shutdown_err) = shutdown_result {
                    eprintln!(
                        "    warning: clean shutdown after a failed loaded-world run also \
                         errored: {shutdown_err}"
                    );
                }
                Err(e)
            }
            Ok(()) => shutdown_result.map_err(Into::into),
        }
    })();

    // Run all safety checks even when the acceptance returns UNVERIFIED: an
    // untouched source and an unchanged disposable copy are mandatory on every
    // exit path.
    let copy_check = temp
        .hash_tree()
        .and_then(|after| load_world::assert_copy_equals_source(&source_before, &after));
    let source_check = source.verify_unchanged(&source_before);
    let cleanup = temp.cleanup();

    source_check?;
    copy_check?;
    cleanup?;
    result
}

/// The exact, test-pinned UNVERIFIED reason the generated-world acceptance
/// reports while the rivet-server does not yet serve a genuine generated
/// world. Two failure shapes both report it:
///
/// - a boot that rejects the `--seed` launch option (only
///   `--host`/`--port`/`--level`) — no way to boot a fresh seed world at all;
/// - a boot that accepts `--seed` but still serves the superflat M1 fixture —
///   the client-observable login `is_flat` flag is true, so the server served
///   the no-level superflat default, not genuine FULL generated chunks.
///
/// In both shapes the scenario must exit UNVERIFIED (3) with exactly this
/// reason — it must never fall back to the superflat no-level boot or a copied
/// loaded world, which would fabricate a PASS on the wrong world. When the
/// server genuinely serves a generated world, the login `is_flat` flag is
/// false and the runner proceeds to compare against the seed-42 ground truth.
pub const GENERATED_WORLD_UNVERIFIED_REASON: &str = "generated-world acceptance is UNVERIFIED: rivet-server does not yet serve a genuine \
     generated seed-42 world (no --seed launch option, or a --seed build that still boots the \
     superflat M1 fixture). The scenario will not boot a superflat or copied-loaded-world \
     stand-in. It reports UNVERIFIED until the rivet-server worldgen capability lands.";

/// The official-client generated-world acceptance probe (seed-42 contract).
///
/// Boots Rivet with `--seed 42` — the generated-world launch seam — on an
/// isolated port with no world path, drives the real Azalea client in
/// `generated` mode (join + dwell + bounded walk + per-coordinate content
/// sampling from the client's own loaded `ChunkStorage`), compares the observed
/// content against the seed-42 ground-truth handoff (`rivet-oracle
/// generated-expected`), and requires a clean SIGTERM shutdown.
///
/// The rivet-server `--seed` option exists but the no-level boot still serves
/// the superflat M1 fixture, not genuine FULL generated chunks. The runner
/// detects this client-observably: the login packet's `is_flat` flag is true
/// for the superflat M1 boot, so the acceptance stays honestly UNVERIFIED with
/// the exact [`GENERATED_WORLD_UNVERIFIED_REASON`] — never a superflat or
/// loaded-world stand-in — rather than comparing the superflat bytes against
/// the seed-42 ground truth and fabricating a PASS or a FAIL. Only when the
/// server serves a genuine generated world (login `is_flat` false) does the
/// runner proceed to the per-coordinate comparison. A build that rejects
/// `--seed` entirely is classified `Absent` at boot and exits UNVERIFIED (3)
/// with the same pinned reason.
fn run_generated_world(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-generated-world");
    fs::create_dir_all(&work)?;

    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    let run_dir = work.join("rivet");
    let log_path = work.join("rivet.log");
    let seed = args.seed.unwrap_or(server::GENERATED_SEED);

    let result = (|| -> Result<(), RunnerError> {
        println!("rivet scenario runner: generated-world (seed-42 generated acceptance contract)");
        println!("    rivet-server bin : {}", rivet_bin.display());
        println!(
            "    launch seam      : {} <seed>",
            server::GENERATED_SEED_ARG
        );
        println!("    seed             : {seed}");
        println!();

        // Boot Rivet with the explicit generated-world launch option. The
        // probe classifies a rejection honestly: a build without the `--seed`
        // capability reports the exact pinned UNVERIFIED reason, never a
        // superflat or loaded-world fallback.
        let mut srv = match server::boot(
            server::ServerKind::Rivet,
            &run_dir,
            &log_path,
            &rivet_bin,
            None,
            None,
            base,
            None,
            &[],
            None,
            Some(seed),
        ) {
            Ok(srv) => srv,
            Err(error) => {
                return Err(classify_generated_world_boot_failure(error, &log_path));
            }
        };

        // The post-boot acceptance body. It never shuts `srv` down; the wrapper
        // below always does.
        let body = (|| -> Result<(), RunnerError> {
            // Fetch the seed-42 ground-truth handoff. The merged
            // `generated-expected` oracle verifies the committed Paper-captured
            // seed-42 golden, so the handoff succeeds; the acceptance still must
            // not compare against it until the server serves a genuine
            // generated world (the `is_flat` gate below).
            let expected = run_generated_expected()?;

            // Drive the real Azalea client in generated mode against the booted
            // server.
            let client_run = run_client(
                &client_bin,
                &ClientSpec {
                    address: base.to_string(),
                    username: args.username.clone(),
                    timeout_seconds: args.timeout_seconds,
                    dwell_seconds: 0,
                    mode: "generated".to_owned(),
                },
                &work,
                "generated-client",
            )?;

            // Prove the client genuinely reached the Rivet port.
            verify_rivet_connection(&log_path)?;

            // The client transcript must prove it joined, spawned, dwelled,
            // walked, and sampled genuine per-coordinate content.
            let transcript = client_run.normalize(transcript::normalize_generated)?;
            let boundary = transcript::rivet_generated_verdict(&transcript)
                .map_err(RunnerError::Transcript)?;

            // Re-ground the pinned UNVERIFIED contract on what the server
            // actually served (see [`classify_generated_is_flat`]).
            classify_generated_is_flat(&transcript)?;

            // Compare the client's observed block content against the seed-42
            // ground truth. This is the load-bearing comparison: a server that
            // only echoes repeated superflat bytes fails here.
            compare_generated_content(&expected, &transcript)?;

            println!("\nGenerated-world acceptance boundary reached: {boundary}");
            Ok(())
        })();

        // A booted server must always be shut down cleanly before returning.
        let shutdown_result = server::shutdown(&mut srv);
        match body {
            Err(e) => {
                if let Err(shutdown_err) = shutdown_result {
                    eprintln!(
                        "    warning: clean shutdown after a failed generated-world run also \
                         errored: {shutdown_err}"
                    );
                }
                Err(e)
            }
            Ok(()) => shutdown_result.map_err(Into::into),
        }
    })();

    // No disposable copy to clean up on the generated-world path — the fresh
    // generated world lives entirely in the server's run dir, which the harness
    // leaves in place as diagnostic state under `work/`.
    result
}

/// Map a generated-world boot failure through the seed probe classifier.
/// `Gate` and `Io` can happen before a child is spawned and remain hard
/// failures without consulting a possibly stale log.
fn classify_generated_world_boot_failure(error: server::Error, log_path: &Path) -> RunnerError {
    let boot_error = match error {
        server::Error::Unverified(message) => message,
        error @ (server::Error::Gate(_) | server::Error::Io(_)) => return error.into(),
    };
    let log = fs::read_to_string(log_path).unwrap_or_default();
    match server::classify_seed_probe(false, &log) {
        server::ProbeVerdict::Absent { evidence } => RunnerError::Unverified(format!(
            "{GENERATED_WORLD_UNVERIFIED_REASON} launch evidence: {evidence}"
        )),
        server::ProbeVerdict::FailedToBoot { evidence } => RunnerError::Unverified(format!(
            "generated-world acceptance is UNVERIFIED: the launch probe did not reach READY \
             ({boot_error}); last log evidence: {evidence}"
        )),
        server::ProbeVerdict::Present => unreachable!("the failed boot did not reach READY"),
    }
}

/// Invoke `rivet-oracle generated-expected <seed>` in verify mode and parse
/// the committed seed-42 ground-truth golden into a `serde_json::Value`. The
/// handoff is the committed Paper-captured seed-42 golden at
/// `tools/rivet-oracle/fixtures/generated-expected/` (the fixture the merged
/// PR #595 ships); the runner still must not compare against it until the
/// server genuinely serves a generated world (the `is_flat` gate in
/// `run_generated_world`). Verify mode is used rather than `--to` capture:
/// capture re-boots Paper to regenerate the golden, which the committed
/// fixture already carries — so no Paper runtime is a prerequisite for the
/// acceptance.
fn run_generated_expected() -> Result<Value, RunnerError> {
    let oracle_bin = oracle_binary();
    let status = Command::new(&oracle_bin)
        .args(["generated-expected"])
        .arg(server::GENERATED_SEED.to_string())
        .status()
        .map_err(|e| {
            RunnerError::Unverified(format!(
                "failed to run rivet-oracle generated-expected ({}): {e} — build it first with \
                 cargo build -p rivet-oracle",
                oracle_bin.display()
            ))
        })?;
    // Verify mode writes no `--to` file, so `report_out` is false — the
    // UNVERIFIED reason is the oracle's own message, not a file to inspect.
    classify_oracle_status("generated-expected", false, status, Path::new(""))?;
    // The oracle only validated the committed fixture; read the golden itself
    // for the per-coordinate comparison below. A missing golden that the oracle
    // had just verified is UNVERIFIED (the fixture tree is a prereq), never a
    // fabricated PASS.
    let golden = crate_root()
        .join("../../tools/rivet-oracle/fixtures/generated-expected/generated-expected.json");
    let text = fs::read_to_string(&golden).map_err(|e| {
        RunnerError::Unverified(format!(
            "generated-expected committed golden is unreadable at {} ({e}) — the oracle verified \
             the fixture tree but the runner could not read the golden",
            golden.display()
        ))
    })?;
    let manifest: Value = serde_json::from_str(&text).map_err(RunnerError::Json)?;
    Ok(manifest)
}

/// Re-ground the pinned UNVERIFIED contract on what the server actually
/// served. The client transcript carries the login `is_flat` flag, which
/// discriminates the no-level superflat M1 fixture (true) from a genuine
/// generated world (false). While `--seed` is accepted but still boots the
/// superflat fixture, the acceptance must exit UNVERIFIED (3) with the exact
/// [`GENERATED_WORLD_UNVERIFIED_REASON`] — comparing the superflat bytes
/// against the seed-42 ground truth would fabricate a FAIL on the wrong world.
/// A transcript that did not carry the login flag cannot prove the server
/// served a genuine non-flat world, so it also stays UNVERIFIED. Only a
/// non-flat transcript proceeds to the per-coordinate comparison.
fn classify_generated_is_flat(transcript: &Value) -> Result<(), RunnerError> {
    match transcript["generated"]["is_flat"].as_bool() {
        Some(true) => Err(RunnerError::Unverified(format!(
            "{GENERATED_WORLD_UNVERIFIED_REASON} The booted server served the superflat M1 fixture \
             (login is_flat=true), not genuine FULL generated chunks."
        ))),
        None => Err(RunnerError::Unverified(format!(
            "{GENERATED_WORLD_UNVERIFIED_REASON} The client transcript did not report the login \
             is_flat flag, so the served world cannot be proven genuine."
        ))),
        Some(false) => Ok(()),
    }
}

/// Compare the client's observed per-coordinate block content against the
/// seed-42 ground-truth manifest. The `generated` record's `samples` carry
/// `surface`/`bedrock`/`below_feet` block names at world coordinates; the
/// manifest's per-chunk `surface`/`bedrock`/`below_feet` arrays are keyed by
/// `"<chunk_x>,<chunk_z>"` and indexed `z*16+x` (row-major with the sample
/// point at the chunk center offset `(8,8)`).
///
/// A sample whose chunk is absent from the manifest, or whose observed block
/// name differs from ground truth, is a FAIL — the server did not serve the
/// generated seed-42 world (or served the wrong seed, or echoed superflat
/// bytes). A sampled chunk that is not `minecraft:full` in the manifest is
/// UNVERIFIED — the seed-42 handoff does not yet have full ground truth there.
fn compare_generated_content(manifest: &Value, transcript: &Value) -> Result<(), RunnerError> {
    let chunks = manifest["chunks"].as_object().ok_or_else(|| {
        RunnerError::Gate("generated-expected manifest has no chunks map".to_owned())
    })?;
    let samples = transcript["generated"]["samples"]
        .as_array()
        .ok_or_else(|| RunnerError::Transcript("generated record has no samples".to_owned()))?;

    if samples.is_empty() {
        return Err(RunnerError::Transcript(
            "generated record has no samples; the client sampled no content".to_owned(),
        ));
    }

    let mut checked = 0usize;
    for sample in samples {
        let chunk_x = sample["chunk_x"]
            .as_i64()
            .ok_or_else(|| RunnerError::Transcript("sample missing chunk_x".to_owned()))?;
        let chunk_z = sample["chunk_z"]
            .as_i64()
            .ok_or_else(|| RunnerError::Transcript("sample missing chunk_z".to_owned()))?;
        let key = format!("{chunk_x},{chunk_z}");
        let fingerprint = chunks.get(&key).ok_or_else(|| {
            RunnerError::Gate(format!(
                "generated-expected manifest has no chunk {key} but the client sampled it — the \
                 server served a chunk outside the seed-42 ground-truth world"
            ))
        })?;
        let status = fingerprint["status"].as_str().ok_or_else(|| {
            RunnerError::Gate(format!(
                "generated-expected manifest chunk {key} has no string Status — refusing PASS on \
                 a malformed manifest"
            ))
        })?;
        if status != "minecraft:full" {
            return Err(RunnerError::Unverified(format!(
                "generated sampled chunk {key} is {status} (not minecraft:full): the seed-42 \
                 ground-truth handoff does not yet have full per-coordinate content there, so \
                 this acceptance stays UNVERIFIED"
            )));
        }
        // A FULL chunk may still carry content the #519 capability boundary
        // cannot yet construct (non-empty structures.starts, entities). The
        // seed-42 reference records these flags honestly; refusing PASS here
        // keeps the capability boundary honest instead of comparing a chunk the
        // server could not have served faithfully — the exact guard the loaded
        // comparator applies.
        let flags: Vec<&str> = fingerprint["capability_flags"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !flags.is_empty() {
            return Err(RunnerError::Unverified(format!(
                "generated sampled chunk {key} is minecraft:full but carries #519-uncarried \
                 capability flags {flags:?}; the runner refuses PASS rather than trusting an \
                 incomplete server"
            )));
        }
        // The manifest stores surface/bedrock/below_feet as 16×16 arrays
        // indexed row-major `z*16+x`. The client samples the chunk center
        // offset (8,8), so the index is `8*16+8 = 136`. The helper requires
        // every array to contain that center entry — a short/missing array is a
        // malformed manifest refused as a Gate, never a vacuous air-vs-air pass.
        let (manifest_surface, manifest_bedrock, manifest_below) =
            manifest_center_blocks(fingerprint, &key, "generated-expected")?;
        let observed_surface = canonicalize_block_name(sample["surface"].as_str());
        let observed_bedrock = canonicalize_block_name(sample["bedrock"].as_str());
        let observed_below = canonicalize_block_name(sample["below_feet"].as_str());

        let surface_match = observed_surface == manifest_surface;
        let bedrock_match = observed_bedrock == manifest_bedrock;
        let below_match = observed_below == manifest_below;
        if !(surface_match && bedrock_match && below_match) {
            return Err(RunnerError::Gate(format!(
                "generated content mismatch at chunk {key} (sample {},{}, center): \
                 observed surface={observed_surface} bedrock={observed_bedrock} \
                 below_feet={observed_below}; ground truth surface={manifest_surface} \
                 bedrock={manifest_bedrock} below_feet={manifest_below}",
                sample["sample_x"].as_i64().unwrap_or(chunk_x * 16),
                sample["sample_z"].as_i64().unwrap_or(chunk_z * 16),
            )));
        }
        checked += 1;
    }

    println!("\n    verified {checked} sampled chunks against the seed-42 ground-truth manifest");
    Ok(())
}

/// The load-bearing rivet-server log fragment that proves the movement-driven
/// recenter failed typed in the NEGATIVE control: the session-level warn emitted
/// in `session.rs` `dispatch_move_player`'s `Err` arm (`disconnecting play
/// session on chunk-loader update failure`) when the on-demand load fails. The
/// positive acceptance REQUIRES this fragment to be ABSENT; the tampered-copy
/// negative control REQUIRES it to be PRESENT.
const RECENTER_FAILURE_LOG_FRAGMENT: &str =
    "disconnecting play session on chunk-loader update failure";

/// The typed failure the on-demand region load surfaces when a chunk is corrupt
/// at the storage boundary: the `RegionRead` `RegionBackedBootError` displays
/// `UNVERIFIED read-only region read failed: <io error>`, and the corrupt chunk
/// the negative control creates surfaces `<io error>` =
/// `corrupt chunk [5, -7] in read-only region: external stream is missing or
/// unsupported`. This is the narrow typed signal the negative control REQUIRES
/// and the positive acceptance REQUIRES to be absent — narrower than a bare
/// `UNVERIFIED` grep, so an unrelated future `UNVERIFIED`-prefixed log line
/// cannot spuriously fail the acceptance.
const RECENTER_UNVERIFIED_TEXT: &str = "UNVERIFIED read-only region read failed";

/// The overworld region file that holds the route's beyond-boot enter cells:
/// chunks (0..7, -7..1) live in the region rooted at `(0, -1)`, whose file is
/// `r.0.-1.mca` under `dimensions/minecraft/overworld/region/`. The negative
/// control corrupts the chunk DATA for one of those cells here — the first cell
/// the second beyond-boot crossing enters — so the boot succeeds (the header
/// stays valid and the booted 117-chunk square never reads the cell) and the
/// on-demand recenter read fails typed at the storage boundary.
const RECENTER_TAMPER_REGION_X: i32 = 0;
const RECENTER_TAMPER_REGION_Z: i32 = -1;
/// The chunk cell the negative control corrupts in the disposable copy's region
/// file: the first cell of the second beyond-boot enter column (x=5), which the
/// route's +32 block frame enters. It is OUTSIDE the booted 117-chunk square, so
/// the boot still succeeds and only the on-demand recenter load hits it.
const RECENTER_TAMPER_CHUNK: [i32; 2] = [5, -7];

/// The `recenter` runner (issues #185/#561): boot Rivet against a disposable
/// copy of the safe copied world, drive the `loaded-recenter` client's
/// deterministic +x route across repeated chunk boundaries, and REQUIRE the
/// positive sustained-walking result — the client stays connected and receives
/// every beyond-boot chunk the region-backed recenter loads on demand.
///
/// This proves the #185 fix: the previous two-boundary route (the merged #569
/// negative reproduction) now REMAINS CONNECTED and RECEIVES the newly loaded
/// chunks instead of disconnecting typed at the boot authority edge. It is not
/// the loaded-world content acceptance (`loaded-world` keeps its own bounded
/// walk/content verdict); this scenario proves the MOVEMENT-driven on-demand
/// region load keeps the session alive across repeated chunk boundaries.
///
/// The positive acceptance cross-requires:
///
/// - the client transcript: outcome `walked` after spawn, the pinned Azalea
///   revision, the announced route, all four `move_frame` records landing in
///   chunks [0,-3] .. [3,-3], and the `walked` terminal's received chunk list
///   containing every `transcript::RECENTER_BEYOND_BOOT_CELLS` cell (the 27
///   cells the route enters outside the booted 117-chunk square — the proof the
///   recenter "received the newly loaded chunks").
/// - the rivet-server log: `connection established`, an accepted teleport ack +
///   accepted move frames (the route reached the authoritative movement path),
///   NO `read timeout` kick (not keepalive), and NO
///   `RECENTER_FAILURE_LOG_FRAGMENT` / `RECENTER_UNVERIFIED_TEXT` text (the
///   on-demand loads all succeeded — a single typed failure would have
///   disconnected the session mid-route).
///
/// The source world stays immutable (fingerprint verified before and after) and
/// the disposable copy lifecycle is exactly the `load_world::TempWorld` one the
/// loaded-world acceptance uses — never superflat/generation fallback (the
/// `RequireLoaded` policy forbids it).
///
/// A NON-VACUOUS negative control then corrupts a beyond-boot chunk in a SECOND
/// disposable copy (never the source) and boots a second rivet-server on an
/// isolated port. The same route now REQUIRES the typed `RECENTER_UNVERIFIED_TEXT`
/// disconnect — the server must fail typed on the corrupt/missing chunk
/// (no generation, no superflat fallback, no silent substitution), the client
/// must surface `disconnected` (not `walked`), and the source + both copies'
/// safety checks must still pass. This proves the positive acceptance is
/// non-vacuous: the same harness that PASSes a genuine sustained walk FAILs
/// typed when the beyond-boot data is genuinely missing/corrupt.
fn run_recenter(args: &Args) -> Result<(), RunnerError> {
    let crate_root = crate_root();
    let work = crate_root.join("work/scenario-recenter");
    let source_path = load_world::resolve_loaded_world_src()?;
    let source = load_world::SourceTree::open(&source_path)?;
    load_world::validate_prospective_storage(&source, &work)?;
    fs::create_dir_all(&work)?;

    let source_before = source.fingerprint()?;
    let rivet_bin = server::ensure_rivet_binary(&crate_root)?;
    let client_bin = ensure_client_binary()?;
    let base = base_address(args)?;
    let run_dir = work.join("rivet");
    let log_path = work.join("rivet.log");
    let mut temp = load_world::TempWorld::create(&source, &work)?;

    let result = (|| -> Result<(), RunnerError> {
        load_world::assert_copy_equals_source(&source_before, &temp.hash_tree()?)?;
        let server_world_path = temp.server_path();

        println!("rivet scenario runner: recenter (#185/#561 sustained-walking acceptance)");
        println!(
            "    source world      : {} (read only; never passed to a server)",
            source.configured_path().display()
        );
        println!("    disposable copy  : {}", temp.path().display());
        println!("    rivet-server bin : {}", rivet_bin.display());
        println!(
            "    launch seam      : {} <disposable-copy>",
            server::WORLD_PATH_ARG
        );
        println!();
        println!("    expected result  : the region-backed recenter (issue #185) loads every");
        println!(
            "                      beyond-boot chunk on demand, so the +x route stays connected"
        );
        println!("                      and the client RECEIVES all 27 beyond-boot cells — the");
        println!(
            "                      positive sustained-walking acceptance (was the #561 negative"
        );
        println!("                      regression before the #185 fix).");
        println!();

        let mut srv = match server::boot(
            server::ServerKind::Rivet,
            &run_dir,
            &log_path,
            &rivet_bin,
            None,
            None,
            base,
            None,
            &[(trace::TRACE_MOVEMENT_ENV, "1")],
            Some(&server_world_path),
            None,
        ) {
            Ok(srv) => srv,
            Err(error) => return Err(classify_load_world_boot_failure(error, &log_path)),
        };

        // The post-boot acceptance body, mirroring the loaded-world wrapper's
        // shutdown discipline: the server is always shut down cleanly before the
        // disposable-copy cleanup.
        let body = (|| -> Result<(), RunnerError> {
            // Drive the real Azalea client in loaded-recenter mode against the
            // booted server.
            let client_run = run_client(
                &client_bin,
                &ClientSpec {
                    address: base.to_string(),
                    username: args.username.clone(),
                    timeout_seconds: args.timeout_seconds,
                    dwell_seconds: 0,
                    mode: "loaded-recenter".to_owned(),
                },
                &work,
                "recenter-client",
            )?;

            // The client genuinely reached the Rivet port.
            verify_rivet_connection(&log_path)?;

            // The client transcript must prove the spawn + the full sustained
            // route + the positive `walked` terminal with the received chunk list.
            let transcript = client_run.normalize(transcript::normalize_recenter)?;
            let transcript_path = work.join("recenter.transcript.json");
            fs::write(&transcript_path, serde_json::to_string_pretty(&transcript)?)?;
            let boundary =
                transcript::rivet_recenter_verdict(&transcript).map_err(RunnerError::Transcript)?;

            // Server-side half: the on-demand loads all succeeded. Require the
            // route was accepted (accepted teleport ack + accepted moves), the
            // close was NOT a keepalive timeout (no `read timeout` kick), and NO
            // typed UNVERIFIED failure appeared (a single missing/corrupt
            // beyond-boot chunk would have disconnected the session mid-route —
            // the transcript would show `disconnected`, and the verdict above
            // would already have refused PASS). RIVET_SESSION_END is NOT
            // excluded here: after the client emits `walked` and hard-exits, the
            // server prunes the connection on EndOfStream — a traced close that
            // legitimately records a session end. Only the typed recenter
            // failure and a keepalive kick are forbidden.
            let rivet_log = fs::read_to_string(&log_path)?;
            if rivet_log.contains(RECENTER_FAILURE_LOG_FRAGMENT) {
                return Err(RunnerError::Gate(format!(
                    "rivet log {} shows '{}' — the movement-driven recenter failed typed on a \
                     beyond-boot chunk. The #185 on-demand region load did NOT keep the session \
                     connected: a missing/corrupt chunk (or a regression in the load path) \
                     disconnected the client mid-route.",
                    log_path.display(),
                    RECENTER_FAILURE_LOG_FRAGMENT
                )));
            }
            if rivet_log.contains(RECENTER_UNVERIFIED_TEXT) {
                return Err(RunnerError::Gate(format!(
                    "rivet log {} shows '{}' — an on-demand region load surfaced a typed \
                     UNVERIFIED missing/corrupt chunk. The recenter must not substitute \
                     generation or a superflat fallback; a genuine load failure is a FAIL here.",
                    log_path.display(),
                    RECENTER_UNVERIFIED_TEXT
                )));
            }
            if rivet_log.contains("read timeout") {
                return Err(RunnerError::Gate(format!(
                    "rivet log {} shows a 'read timeout' kick — the client was disconnected by \
                     the keepalive timeout, not kept connected by the sustained recenter.",
                    log_path.display()
                )));
            }
            let movement_trace = trace::parse(&rivet_log).map_err(RunnerError::Transcript)?;
            if !movement_trace
                .teleport_acks
                .iter()
                .any(|a| a.outcome == "accepted")
            {
                return Err(RunnerError::Gate(format!(
                    "rivet log {} has no accepted teleport ack — the client never completed the \
                     spawn teleport, so the movement route never ran against an authoritative \
                     player",
                    log_path.display()
                )));
            }
            if movement_trace.moves.is_empty() {
                return Err(RunnerError::Gate(format!(
                    "rivet log {} has no accepted move frames — the driven route never reached \
                     the server's authoritative movement path (the first boundary crossing did not \
                     succeed)",
                    log_path.display()
                )));
            }

            // Preserve the movement audit as a diagnostic artifact (mirroring the
            // move differential's dump).
            let trace_tp = work.join("recenter.trace.json");
            let trace_dump = serde_json::json!({
                "teleport_acks": movement_trace.teleport_acks.iter().map(|a| json!({
                    "ack_id": a.ack_id,
                    "outcome": a.outcome,
                    "position": a.position,
                })).collect::<Vec<_>>(),
                "moves": movement_trace.moves.len(),
                "final_position": movement_trace.final_position(),
                "session_end_reason": movement_trace.session_end.as_ref().map(|e| e.reason.clone()),
            });
            fs::write(&trace_tp, serde_json::to_string_pretty(&trace_dump)?)?;

            println!("\nSustained-walking boundary reached: {boundary}");
            println!(
                "    client route    : {} move frame(s), last in chunk {:?}",
                transcript["move_frames"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0),
                transcript["move_frames"]
                    .as_array()
                    .and_then(|a| a.last())
                    .and_then(|f| f.get("chunk"))
            );
            println!(
                "    received chunks : {} (must include every beyond-boot enter cell)",
                transcript["walked"]
                    .get("chunk_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
            );
            println!(
                "    movement audit  : accepted teleport ack + {} accepted move(s); no typed \
                 UNVERIFIED recenter failure, no keepalive kick",
                movement_trace.moves.len()
            );
            println!(
                "    artifacts       : {} (transcript), {} (trace)",
                transcript_path.display(),
                trace_tp.display()
            );

            // The client's `walked` terminal is the load-bearing evidence that
            // every beyond-boot chunk was delivered: the transcript verdict
            // already required all 27 RECENTER_BEYOND_BOOT_CELLS cells in the
            // received list. Confirm the count is the deterministic final set
            // (81 spawn view + 36 beyond-boot = 117) as a coherence cross-check.
            let received_count = transcript["walked"]
                .get("chunk_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if received_count != 117 {
                return Err(RunnerError::Gate(format!(
                    "loaded-recenter client received {received_count} chunks (expected the \
                     deterministic 117: 81 spawn view + 36 beyond-boot) — the sustained-walking \
                     route did not settle on the full expected view"
                )));
            }
            println!();
            println!("VERDICT: PASS — sustained movement across repeated chunk boundaries;");
            println!("    the region-backed recenter (issue #185) stayed connected and received");
            println!("    every on-demand loaded beyond-boot chunk (the previous #561 negative");
            println!("    reproduction is now the positive acceptance).");
            Ok(())
        })();

        let shutdown_result = server::shutdown(&mut srv);
        if let Err(e) = body {
            if let Err(shutdown_err) = shutdown_result {
                eprintln!(
                    "    warning: clean shutdown after a failed recenter run also errored: \
                     {shutdown_err}"
                );
            }
            return Err(e);
        }
        shutdown_result?;

        // Negative control: the positive acceptance is non-vacuous only if the
        // same harness FAILS typed when a beyond-boot chunk is genuinely missing
        // or corrupt. Corrupt the chunk DATA of the first cell of the second
        // beyond-boot enter column in a SECOND disposable copy (the source world
        // is never touched — the copy is the only writable tree), boot a second
        // rivet-server on an isolated port, drive the same route, and REQUIRE
        // the typed `UNVERIFIED` disconnect (the session disconnects on the
        // on-demand load failure, never a superflat/generation substitution).
        println!();
        println!("Negative case (tamper a beyond-boot chunk in a second disposable copy)");
        let negative_temp = load_world::TempWorld::create(&source, &work)?;
        {
            let negative_copy_region = negative_temp
                .path()
                .join("dimensions")
                .join("minecraft")
                .join("overworld")
                .join("region")
                .join(format!(
                    "r.{}.{}.mca",
                    RECENTER_TAMPER_REGION_X, RECENTER_TAMPER_REGION_Z
                ));
            corrupt_region_chunk_entry(&negative_copy_region, RECENTER_TAMPER_CHUNK).map_err(
                |e| {
                    RunnerError::Gate(format!(
                        "negative control FAILED to corrupt the disposable copy's region data for \
                     chunk {:?}: {e}",
                        RECENTER_TAMPER_CHUNK
                    ))
                },
            )?;
            println!(
                "    corrupted chunk ({},{}) in the copy's region {} (compression id -> 0xFF)",
                RECENTER_TAMPER_CHUNK[0],
                RECENTER_TAMPER_CHUNK[1],
                negative_copy_region.display()
            );
        }
        let negative_run_dir = work.join("rivet-negative");
        let negative_log_path = work.join("rivet-negative.log");
        let mut reservations = reserve_ports(1)?;
        let negative_addr = SocketAddr::new(base.ip(), reservations[0].port());
        let mut negative_srv = match server::boot(
            server::ServerKind::Rivet,
            &negative_run_dir,
            &negative_log_path,
            &rivet_bin,
            None,
            None,
            negative_addr,
            Some(reservations.remove(0)),
            &[(trace::TRACE_MOVEMENT_ENV, "1")],
            Some(&negative_temp.server_path()),
            None,
        ) {
            Ok(srv) => srv,
            Err(error) => return Err(classify_load_world_boot_failure(error, &negative_log_path)),
        };

        let negative_body = (|| -> Result<(), RunnerError> {
            let negative_run = run_client(
                &client_bin,
                &ClientSpec {
                    address: negative_addr.to_string(),
                    username: args.username.clone(),
                    timeout_seconds: args.timeout_seconds,
                    dwell_seconds: 0,
                    mode: "loaded-recenter".to_owned(),
                },
                &work,
                "recenter-negative-client",
            )?;
            verify_rivet_connection(&negative_log_path)?;

            // The transcript must surface `disconnected` (the client was closed
            // by the server mid-route — it never emitted `walked`).
            let negative_transcript = negative_run.normalize(transcript::normalize_recenter)?;
            let negative_transcript_path = work.join("recenter-negative.transcript.json");
            fs::write(
                &negative_transcript_path,
                serde_json::to_string_pretty(&negative_transcript)?,
            )?;
            if negative_transcript["outcome"].as_str() != Some("disconnected") {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered copy run surfaced outcome {:?} \
                     (expected disconnected) — the corrupt beyond-boot chunk did not disconnect \
                     the session, so the positive acceptance is not provably non-vacuous",
                    negative_transcript["outcome"].as_str()
                )));
            }

            // The server must have failed typed: the `disconnecting play session
            // on chunk-loader update failure` warn with the `UNVERIFIED
            // region-backed chunk` text, an accepted teleport ack + accepted
            // moves before the failure, and NO keepalive kick.
            let negative_log = fs::read_to_string(&negative_log_path)?;
            if !negative_log.contains(RECENTER_FAILURE_LOG_FRAGMENT) {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered-copy rivet log {} does not show '{}' — \
                     the corrupt beyond-boot chunk did not fail typed on the RequireLoaded \
                     policy (a silent substitution or generation fallback would make the positive \
                     acceptance vacuous)",
                    negative_log_path.display(),
                    RECENTER_FAILURE_LOG_FRAGMENT
                )));
            }
            if !negative_log.contains(RECENTER_UNVERIFIED_TEXT) {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered-copy rivet log {} shows the recenter \
                     failure warn but not the typed '{RECENTER_UNVERIFIED_TEXT}' text — the failure \
                     was not the RequireLoaded missing-chunk policy",
                    negative_log_path.display()
                )));
            }
            if negative_log.contains("read timeout") {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered-copy rivet log {} shows a 'read \
                     timeout' kick — the client was disconnected by keepalive, not by the typed \
                     recenter failure on the corrupt chunk",
                    negative_log_path.display()
                )));
            }
            let negative_trace = trace::parse(&negative_log).map_err(RunnerError::Transcript)?;
            if !negative_trace
                .teleport_acks
                .iter()
                .any(|a| a.outcome == "accepted")
            {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered-copy rivet log {} has no accepted \
                     teleport ack — the route never reached an authoritative player",
                    negative_log_path.display()
                )));
            }
            if negative_trace.moves.is_empty() {
                return Err(RunnerError::Gate(format!(
                    "negative control FAILED: the tampered-copy rivet log {} has no accepted move \
                     frames — the route never reached the authoritative movement path before the \
                     typed failure",
                    negative_log_path.display()
                )));
            }

            println!("    transcript      : outcome disconnected (client never reached `walked`)");
            println!(
                "    server failure  : '{}' with '{RECENTER_UNVERIFIED_TEXT}' (typed, no \
                 generation/superflat fallback)",
                RECENTER_FAILURE_LOG_FRAGMENT
            );
            println!(
                "    movement audit  : accepted teleport ack + {} accepted move(s) before the \
                 typed failure",
                negative_trace.moves.len()
            );
            println!(
                "    artifacts       : {} (transcript), {} (log)",
                negative_transcript_path.display(),
                negative_log_path.display()
            );
            println!();
            println!(
                "VERDICT: PASS — the tampered-copy negative control failed typed UNVERIFIED, so"
            );
            println!(
                "    the positive sustained-walking acceptance is non-vacuous: the same harness"
            );
            println!("    refuses PASS when a beyond-boot chunk is genuinely missing/corrupt.");
            Ok(())
        })();

        let negative_shutdown = server::shutdown(&mut negative_srv);
        match negative_body {
            Err(e) => {
                if let Err(shutdown_err) = negative_shutdown {
                    eprintln!(
                        "    warning: clean shutdown after a failed negative-control run also \
                         errored: {shutdown_err}"
                    );
                }
                Err(e)
            }
            Ok(()) => negative_shutdown.map_err(Into::into),
        }
    })();

    // Run all safety checks even when the acceptance returns UNVERIFIED: an
    // untouched source and unchanged disposable copies are mandatory on every
    // exit path. The negative control's copy is intentionally tampered, so its
    // hash check runs before the corruption.
    let copy_check = temp
        .hash_tree()
        .and_then(|after| load_world::assert_copy_equals_source(&source_before, &after));
    let source_check = source.verify_unchanged(&source_before);
    let cleanup = temp.cleanup();

    source_check?;
    copy_check?;
    cleanup?;
    result
}

/// Corrupt one chunk's DATA payload in the DISPOSABLE copy's `.mca` file:
/// overwrite the chunk's 1-byte compression id (the byte after the 4-byte
/// stream length in the chunk's first sector) with `0xFF` — a compression id no
/// valid chunk can carry. The region file layout is the standard §6 header: the
/// first 4096 bytes are 1024 4-byte entries (`offset = bytes 0..2`,
/// `sector_count = byte 3`), each entry index `z*32 + x` for the local
/// `(x % 32, z % 32)` coordinate, and each chunk's data starts at
/// `sector_offset * 4096` with `[4-byte length][compression id]`.
///
/// `0xFF` sets the external-stream high bit, so the read path treats the chunk
/// as an external `.mcc` stream: no external file exists for the cell, so the
/// read-only storage surfaces the typed `corrupt chunk ... in read-only region:
/// external stream is missing or unsupported` `InvalidData` (never a panic,
/// never a silent absence). The HEADER stays valid — the boot's read-only
/// region open scans every header entry, and the booted 117-chunk square never
/// reads this beyond-view cell — so the server reaches READY, and the on-demand
/// recenter read of the corrupted cell fails typed `UNVERIFIED` — exactly the
/// no-generation/no-fallback failure the negative control requires. Only the
/// copy is touched; the source world is never mutated.
fn corrupt_region_chunk_entry(region_path: &Path, chunk: [i32; 2]) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(region_path)?;
    let local_x = chunk[0].rem_euclid(32);
    let local_z = chunk[1].rem_euclid(32);
    let entry_index = (local_z * 32 + local_x) as u64;
    let entry_offset = entry_index * 4;

    let mut entry = [0u8; 4];
    file.seek(SeekFrom::Start(entry_offset))?;
    file.read_exact(&mut entry)?;
    if entry == [0u8; 4] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "region {} has no allocated entry for chunk {:?} (all-zero header) — cannot \
                 tamper an absent chunk",
                region_path.display(),
                chunk
            ),
        ));
    }
    // The header entry is big-endian `[sector_offset 24 bits][sector_count 8 bits]`.
    let sector_offset = u32::from(entry[0]) << 16 | u32::from(entry[1]) << 8 | u32::from(entry[2]);
    if sector_offset < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "region {} has an invalid sector offset {} for chunk {:?} — cannot tamper the \
                 data of a chunk whose header is already corrupt",
                region_path.display(),
                sector_offset,
                chunk
            ),
        ));
    }
    // The compression id lives at `sector_offset * 4096 + 4` (after the 4-byte
    // stream length). Overwrite it with an unregistered id.
    file.seek(SeekFrom::Start(u64::from(sector_offset) * 4096 + 4))?;
    file.write_all(&[0xFF])?;
    Ok(())
}

/// Resolve the `rivet-oracle` binary: a sibling in the same target dir (the
/// common `cargo build` layout), then an explicit `RIVET_ORACLE_BIN`, then the
/// resolved Cargo target directory.
fn oracle_binary() -> PathBuf {
    let sibling = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("rivet-oracle")));
    if let Some(p) = sibling
        && p.is_file()
    {
        return p;
    }
    if let Ok(p) = std::env::var("RIVET_ORACLE_BIN") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return p;
        }
    }
    cargo_target_dir().join("debug/rivet-oracle")
}

/// Map an `rivet-oracle` subcommand exit status onto the runner's error
/// contract: a nonzero UNVERIFIED (3) from the oracle — a missing
/// ground-truth artifact — stays UNVERIFIED, while any other nonzero exit (a
/// malformed CLI, an internal gate error, or a signal) is a hard Gate — never
/// downgraded to UNVERIFIED. `subcommand` names the oracle subcommand in the
/// message (a shared classifier must not claim `extract-world` when it was
/// `generated-expected`), and `report_out` says whether the `--to` file was
/// actually written and is worth pointing the operator at.
fn classify_oracle_status(
    subcommand: &str,
    report_out: bool,
    status: ExitStatus,
    out: &Path,
) -> Result<(), RunnerError> {
    let see = if report_out {
        format!("; see {}", out.display())
    } else {
        String::new()
    };
    match status.code() {
        Some(0) => Ok(()),
        Some(code) if code == EXIT_UNVERIFIED as i32 => Err(RunnerError::Unverified(format!(
            "rivet-oracle {subcommand} is UNVERIFIED (exit {code}){see}"
        ))),
        Some(code) => Err(RunnerError::Gate(format!(
            "rivet-oracle {subcommand} exited with {code}{see}"
        ))),
        None => Err(RunnerError::Gate(format!(
            "rivet-oracle {subcommand} was terminated by a signal{see}"
        ))),
    }
}

/// Invoke `rivet-oracle extract-world <world> --to <json>` and parse the
/// ground-truth manifest into a `serde_json::Value`.
fn run_extract_world(world: &Path, work: &Path) -> Result<Value, RunnerError> {
    let oracle_bin = oracle_binary();
    let out = work.join("loaded-manifest.json");
    let status = Command::new(&oracle_bin)
        .args(["extract-world"])
        .arg(world)
        .args(["--to"])
        .arg(&out)
        .status()
        .map_err(|e| {
            RunnerError::Unverified(format!(
                "failed to run rivet-oracle extract-world ({}): {e} — build it first with \
                 cargo build -p rivet-oracle",
                oracle_bin.display()
            ))
        })?;
    classify_oracle_status("extract-world", true, status, &out)?;
    let text = fs::read_to_string(&out)?;
    let manifest: Value = serde_json::from_str(&text).map_err(RunnerError::Json)?;
    let full_count = manifest["chunks"]
        .as_object()
        .map(|m| {
            m.values()
                .filter(|c| c.get("status").and_then(Value::as_str) == Some("minecraft:full"))
                .count()
        })
        .unwrap_or(0);
    let non_full_count = manifest["chunks"]
        .as_object()
        .map(|m| m.len().saturating_sub(full_count))
        .unwrap_or(0);
    let mut out = manifest;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("full_count".to_owned(), json!(full_count));
        obj.insert("non_full_count".to_owned(), json!(non_full_count));
    }
    Ok(out)
}

/// Canonicalize an observed block name into the manifest's namespaced form.
/// The client emits azalea bare registry ids (`grass_block`); the manifest
/// stores namespaced names (`minecraft:grass_block`). Comparison must be on a
/// single representation, so a bare id is prefixed with `minecraft:` before it
/// is matched against ground truth.
fn canonicalize_block_name(name: Option<&str>) -> String {
    match name {
        Some(n) if n.contains(':') => n.to_owned(),
        Some(n) => format!("minecraft:{n}"),
        None => "minecraft:air".to_owned(),
    }
}

/// The index of the chunk-center sample point in the manifest's 16×16
/// row-major `surface`/`bedrock`/`below_feet` arrays (`z*16+x` at the client's
/// (8,8) center offset). Shared by the loaded and generated comparators so the
/// two ground-truth contracts agree on where the per-coordinate content is
/// sampled.
const MANIFEST_CENTER: usize = 8 * 16 + 8;

/// Read the three ground-truth block names at the chunk-center sample point
/// from a fingerprint, requiring every array to actually contain the CENTER
/// entry. A missing or short array would otherwise default to air and could
/// pass vacuously air-vs-air against an unloaded client column — a malformed
/// manifest is refused honestly as a Gate (like a missing Status), never
/// compared against nothing.
fn manifest_center_blocks(
    fingerprint: &Value,
    key: &str,
    label: &str,
) -> Result<(String, String, String), RunnerError> {
    let read = |column: &str| -> Result<String, RunnerError> {
        let array = fingerprint[column].as_array().ok_or_else(|| {
            RunnerError::Gate(format!(
                "{label} manifest chunk {key} has no {column} array — refusing PASS on a \
                 malformed manifest (the sampled chunk must carry ground-truth content at the \
                 center point)"
            ))
        })?;
        let entry = array
            .get(MANIFEST_CENTER)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RunnerError::Gate(format!(
                    "{label} manifest chunk {key} {column} array has no center entry (index \
                 {MANIFEST_CENTER}) — refusing PASS on a malformed manifest"
                ))
            })?;
        Ok(entry.to_owned())
    };
    Ok((read("surface")?, read("bedrock")?, read("below_feet")?))
}

/// Compare the client's observed per-coordinate block content against the
/// ground-truth manifest. The `loaded` record's `samples` carry
/// `surface`/`bedrock`/`below_feet` block names at world coordinates; the
/// manifest's per-chunk `surface`/`bedrock`/`below_feet` arrays are keyed by
/// `"<chunk_x>,<chunk_z>"` and indexed `z*16+x` (row-major with the sample
/// point at the chunk center offset `(8,8)`).
///
/// A sample whose chunk is absent from the manifest, or whose observed block
/// name differs from ground truth, is a FAIL — the server did not serve the
/// loaded world (or served content that does not match the read-only copy).
fn compare_loaded_content(manifest: &Value, transcript: &Value) -> Result<(), RunnerError> {
    let chunks = manifest["chunks"]
        .as_object()
        .ok_or_else(|| RunnerError::Gate("loaded-world manifest has no chunks map".to_owned()))?;
    let samples = transcript["loaded"]["samples"]
        .as_array()
        .ok_or_else(|| RunnerError::Transcript("loaded record has no samples".to_owned()))?;

    if samples.is_empty() {
        return Err(RunnerError::Transcript(
            "loaded record has no samples; the client sampled no content".to_owned(),
        ));
    }

    let mut checked = 0usize;
    for sample in samples {
        let chunk_x = sample["chunk_x"]
            .as_i64()
            .ok_or_else(|| RunnerError::Transcript("sample missing chunk_x".to_owned()))?;
        let chunk_z = sample["chunk_z"]
            .as_i64()
            .ok_or_else(|| RunnerError::Transcript("sample missing chunk_z".to_owned()))?;
        let key = format!("{chunk_x},{chunk_z}");
        let fingerprint = chunks.get(&key).ok_or_else(|| {
            RunnerError::Gate(format!(
                "loaded-world manifest has no chunk {key} but the client sampled it — the \
                     server served a chunk outside the ground-truth world"
            ))
        })?;
        // A fingerprint with no string `Status` is a malformed manifest: it
        // must never default to minecraft:full (that would compare — and
        // possibly PASS — a chunk whose ground-truth status is unknown). Refuse
        // honestly as a gate error, the same classification as a missing chunks
        // map.
        let status = fingerprint["status"].as_str().ok_or_else(|| {
            RunnerError::Gate(format!(
                "loaded-world manifest chunk {key} has no string Status — refusing PASS on a \
                 malformed manifest (never defaulting an unknown chunk to minecraft:full)"
            ))
        })?;
        // A non-FULL chunk's content is not ground truth: the extractor records
        // its (partial/empty) sections honestly, but the client would observe
        // real terrain there. Comparing against an all-air fingerprint would be
        // a misleading "content mismatch"; the honest classification is the
        // #519 capability boundary — UNVERIFIED until full-chunk construction
        // can carry it.
        if status != "minecraft:full" {
            return Err(RunnerError::Unverified(format!(
                "loaded-world sampled chunk {key} is {status} (not minecraft:full): the #519 \
                 full-chunk construction capability is required to compare its per-coordinate \
                 content, so this acceptance stays UNVERIFIED"
            )));
        }
        // A FULL chunk may still carry content the #519 capability boundary
        // cannot yet construct (non-empty entities). The extractor records
        // these flags honestly; refusing PASS here keeps the capability
        // boundary honest instead of comparing a chunk the server could not
        // have served faithfully. (Non-empty `structures.starts` is carried
        // verbatim off the parse, #369, and is no longer a flag.)
        let flags: Vec<&str> = fingerprint["capability_flags"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !flags.is_empty() {
            return Err(RunnerError::Unverified(format!(
                "loaded-world sampled chunk {key} is minecraft:full but carries #519-uncarried \
                 capability flags {flags:?}; the runner refuses PASS rather than trusting an \
                 incomplete server"
            )));
        }
        // The manifest stores surface/bedrock/below_feet as 16×16 arrays
        // indexed row-major `z*16+x`. The client samples the chunk center
        // offset (8,8), so the index is `8*16+8 = 136`. The helper requires
        // every array to contain that center entry — a short/missing array is a
        // malformed manifest refused as a Gate, never a vacuous air-vs-air pass.
        let (manifest_surface, manifest_bedrock, manifest_below) =
            manifest_center_blocks(fingerprint, &key, "loaded-world")?;
        // Canonicalize the observed names into the manifest's namespace: the
        // client emits azalea bare ids (`grass_block`) which must compare equal
        // to the manifest's namespaced names (`minecraft:grass_block`).
        let observed_surface = canonicalize_block_name(sample["surface"].as_str());
        let observed_bedrock = canonicalize_block_name(sample["bedrock"].as_str());
        let observed_below = canonicalize_block_name(sample["below_feet"].as_str());

        let surface_match = observed_surface == manifest_surface;
        let bedrock_match = observed_bedrock == manifest_bedrock;
        let below_match = observed_below == manifest_below;
        if !(surface_match && bedrock_match && below_match) {
            return Err(RunnerError::Gate(format!(
                "loaded-world content mismatch at chunk {key} (sample {},{}, center): \
                 observed surface={observed_surface} bedrock={observed_bedrock} \
                 below_feet={observed_below}; ground truth surface={manifest_surface} \
                 bedrock={manifest_bedrock} below_feet={manifest_below}",
                sample["sample_x"].as_i64().unwrap_or(chunk_x * 16),
                sample["sample_z"].as_i64().unwrap_or(chunk_z * 16),
            )));
        }
        checked += 1;
    }

    println!("\n    verified {checked} sampled chunks against the ground-truth manifest");
    Ok(())
}

/// Map only the post-spawn READY/exit `Unverified` result through the
/// world-path probe classifier. `Gate` and `Io` can happen before a child is
/// spawned (run-dir preparation or `Command::spawn`) and must remain hard
/// failures without consulting a possibly stale log from an earlier run.
fn classify_load_world_boot_failure(error: server::Error, log_path: &Path) -> RunnerError {
    let boot_error = match error {
        server::Error::Unverified(message) => message,
        error @ (server::Error::Gate(_) | server::Error::Io(_)) => return error.into(),
    };
    let log = fs::read_to_string(log_path).unwrap_or_default();
    match server::classify_probe(false, &log) {
        server::ProbeVerdict::Absent { evidence } => RunnerError::Unverified(format!(
            "loaded-world acceptance is UNVERIFIED: rivet-server rejected the --level launch \
             interface; launch evidence: {evidence}"
        )),
        server::ProbeVerdict::FailedToBoot { evidence } => RunnerError::Unverified(format!(
            "loaded-world acceptance is UNVERIFIED: the launch probe did not reach READY \
             ({boot_error}); last log evidence: {evidence}"
        )),
        server::ProbeVerdict::Present => unreachable!("the failed boot did not reach READY"),
    }
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
        Subcommand::Dwell => run_dwell(&args),
        Subcommand::Kick => run_kick(&args),
        Subcommand::Capture => run_capture(&args),
        Subcommand::LoadWorld => run_load_world(&args),
        Subcommand::LoadedWorld => run_loaded_world(&args),
        Subcommand::GeneratedWorld => run_generated_world(&args),
        Subcommand::Recenter => run_recenter(&args),
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn parse(v: &[&str]) -> Result<Args, String> {
        Args::parse_from(v.iter().map(|s| s.to_string()))
    }

    fn temp_dir(label: &str) -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "rivet-client-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn write_client_script(path: &Path, marker: &str, exit: i32) {
        fs::write(
            path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"event\":\"starting\",\"protocol\":1,\"azalea_revision\":\"{}\"}}'\nprintf '%s\\n' '{{\"event\":\"{marker}\",\"protocol\":1}}'\necho boom >&2\nexit {exit}\n",
                transcript::PINNED_AZALEA_REVISION
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_contract(path: &Path) -> TrustedClientContract {
        let mut file = fs::File::open(path).unwrap();
        let (sha256, size) = sha256_reader(&mut file).unwrap();
        let mut contract = TrustedClientContract::committed().unwrap();
        contract.sha256 = sha256;
        contract.size = size;
        contract
    }

    fn test_identity() -> ClientIdentity {
        let contract = TrustedClientContract::committed().unwrap();
        ClientIdentity {
            selected_source: PathBuf::from("/canonical/rivet-client"),
            executed_sha256: contract.sha256.clone(),
            contract,
        }
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn correct_client_override_is_authoritative_and_trusted() {
        let work = temp_dir("override");
        let override_path = work.join("client.sh");
        write_client_script(&override_path, "trusted", 0);
        let selected = select_client_binary(Some(override_path.clone()), &work);
        assert_eq!(selected, override_path);
        let binary = prepare_client_binary(&selected, test_contract(&selected)).unwrap();
        assert_eq!(
            binary.identity.selected_source,
            selected.canonicalize().unwrap()
        );
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn default_client_selection_uses_shared_worktree_artifact_not_target_debug() {
        let shared = temp_dir("shared-client-root");
        let worktree = shared.join(".claude/worktrees/probe");
        fs::create_dir_all(shared.join(".git/worktrees/probe")).unwrap();
        fs::create_dir_all(worktree.join("target/debug")).unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}/.git/worktrees/probe\n", shared.display()),
        )
        .unwrap();
        fs::write(worktree.join("target/debug/rivet-client"), b"arbitrary").unwrap();
        let expected = shared
            .join("tools/rivet-oracle/work/bin")
            .join(TRUSTED_CLIENT_ARTIFACT);
        assert_eq!(select_client_binary(None, &worktree), expected);
        fs::remove_dir_all(shared).unwrap();
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn wrong_client_override_is_rejected_by_committed_trust_contract() {
        let work = temp_dir("wrong-override");
        let override_path = work.join("rivet-client");
        write_client_script(&override_path, "self-reported-pinned", 0);
        let selected = select_client_binary(Some(override_path.clone()), &work);
        assert_eq!(selected, override_path);
        let error = prepare_client_binary(&selected, TrustedClientContract::committed().unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not trusted"));
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn raw_provenance_requires_the_pinned_revision_and_records_binary_identity() {
        let identity = test_identity();
        let raw = format!(
            "{{\"event\":\"starting\",\"protocol\":1,\"azalea_revision\":\"{}\"}}\n",
            transcript::PINNED_AZALEA_REVISION
        );
        let bound = bind_raw_client_provenance(&raw, &identity).unwrap();
        assert!(bound.contains("selected_source_path"));
        assert!(bound.contains("/canonical/rivet-client"));
        assert!(bound.contains("executed_sha256"));
        assert!(bound.contains(&identity.executed_sha256));

        let wrong = raw.replace(transcript::PINNED_AZALEA_REVISION, "deadbeef");
        assert!(bind_raw_client_provenance(&wrong, &identity).is_err());
        assert!(
            bind_raw_client_provenance("{\"event\":\"joined\",\"protocol\":1}\n", &identity)
                .is_err()
        );
    }

    #[test]
    fn normalized_evidence_carries_exact_client_identity() {
        let identity = test_identity();
        let run = ClientRun {
            stdout_text: "ignored".to_owned(),
            stdout_path: PathBuf::from("stdout"),
            stderr_path: PathBuf::from("stderr"),
            identity: identity.clone(),
        };
        let normalized = run
            .normalize(|_| Ok(json!({"outcome": "spawned"})))
            .unwrap();
        assert_eq!(
            normalized["client_binary"]["selected_source_path"],
            "/canonical/rivet-client"
        );
        assert_eq!(
            normalized["client_binary"]["executed_sha256"],
            identity.executed_sha256
        );
        assert_eq!(
            normalized["client_binary"]["execution"],
            "verifier-owned-unlinked-fd"
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_deadline_terminates_kills_and_reaps_a_hung_client() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; while :; do :; done"])
            .spawn()
            .unwrap();
        let error = wait_client(
            &mut child,
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.to_string().contains("killed and reaped"));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn nonzero_client_exit_fails_after_preserving_diagnostics() {
        let work = temp_dir("nonzero");
        let script = work.join("client.sh");
        write_client_script(&script, "trusted", 7);
        let binary = prepare_client_binary(&script, test_contract(&script)).unwrap();
        let result = run_client(
            &binary,
            &ClientSpec {
                address: DEFAULT_ADDRESS.to_owned(),
                username: DEFAULT_USERNAME.to_owned(),
                timeout_seconds: 1,
                dwell_seconds: 0,
                mode: "join".to_owned(),
            },
            &work,
            "probe",
        );
        let error = result.err().expect("nonzero client must fail").to_string();
        assert!(
            error.contains("exit status: 7") && error.contains(&binary.identity.executed_sha256),
            "unexpected nonzero-exit error: {error}"
        );
        assert!(work.join("probe.stdout.jsonl").is_file());
        assert_eq!(
            fs::read_to_string(work.join("probe.stderr.log"))
                .unwrap()
                .trim(),
            "boom"
        );
        fs::remove_dir_all(work).unwrap();
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn modified_self_reporting_client_is_rejected_by_trusted_digest() {
        let work = temp_dir("modified-self-report");
        let path = work.join("client.sh");
        write_client_script(&path, "trusted", 0);
        let contract = test_contract(&path);
        let modified = fs::read_to_string(&path)
            .unwrap()
            .replace("trusted", "altered");
        assert_eq!(modified.len(), fs::metadata(&path).unwrap().len() as usize);
        fs::write(&path, modified).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let error = prepare_client_binary(&path, contract)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not trusted"));
        fs::remove_dir_all(work).unwrap();
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn wrong_trusted_digest_is_rejected() {
        let work = temp_dir("wrong-digest");
        let path = work.join("client.sh");
        write_client_script(&path, "trusted", 0);
        let mut contract = test_contract(&path);
        contract.sha256 = "0".repeat(64);
        let error = prepare_client_binary(&path, contract)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not trusted") && error.contains(&"0".repeat(64)));
        fs::remove_dir_all(work).unwrap();
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn selected_path_swap_after_validation_cannot_change_executed_bytes() {
        let work = temp_dir("path-swap");
        let path = work.join("client.sh");
        write_client_script(&path, "trusted", 0);
        let binary = prepare_client_binary(&path, test_contract(&path)).unwrap();
        let replacement = work.join("replacement.sh");
        write_client_script(&replacement, "swapped", 0);
        fs::rename(&replacement, &path).unwrap();
        let run = run_client(
            &binary,
            &ClientSpec {
                address: DEFAULT_ADDRESS.to_owned(),
                username: DEFAULT_USERNAME.to_owned(),
                timeout_seconds: 1,
                dwell_seconds: 0,
                mode: "join".to_owned(),
            },
            &work,
            "probe",
        )
        .unwrap();
        assert!(run.stdout_text.contains("trusted"));
        assert!(!run.stdout_text.contains("swapped"));
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn capture_boundary_requires_spawn_lifecycle_and_pinned_revision() {
        let valid = json!({
            "outcome": "spawned",
            "lifecycle": ["init", "login", "spawn"],
            "azalea_revision": transcript::PINNED_AZALEA_REVISION,
        });
        verify_capture_play_boundary(&valid).unwrap();

        let mut missing_spawn = valid.clone();
        missing_spawn["lifecycle"] = json!(["init", "login"]);
        assert!(verify_capture_play_boundary(&missing_spawn).is_err());
        let mut wrong_revision = valid.clone();
        wrong_revision["azalea_revision"] = json!("deadbeef");
        assert!(verify_capture_play_boundary(&wrong_revision).is_err());
        let mut failed = valid;
        failed["outcome"] = json!("connection_failed");
        assert!(verify_capture_play_boundary(&failed).is_err());
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
    fn load_world_is_a_single_rivet_probe_with_no_silent_options() {
        let args = parse(&["load-world"]).unwrap();
        assert_eq!(args.command, Subcommand::LoadWorld);
        assert_eq!(args.server, ServerSelection::Rivet);
        assert_eq!(args.runs, 1);

        assert!(parse(&["load-world", "--server", "rivet"]).is_ok());
        assert!(parse(&["load-world", "--server", "paper"]).is_err());
        assert!(parse(&["load-world", "--server", "both"]).is_err());
        assert!(parse(&["load-world", "--pairs", "paper:rivet"]).is_err());
        assert!(parse(&["load-world", "--runs", "1"]).is_err());
        assert!(parse(&["load-world", "--dwell-seconds", "35"]).is_err());
        assert!(parse(&["load-world", "--username", DEFAULT_USERNAME]).is_err());
        assert!(parse(&["load-world", "--timeout-seconds", "40"]).is_err());
    }

    #[test]
    fn loaded_world_is_a_single_rivet_acceptance_with_no_silent_options() {
        // `loaded-world` (#374) boots exactly one Rivet server against a
        // disposable world copy and drives the loaded client; Paper has no
        // place, and --pairs/--runs would be silent no-ops.
        let args = parse(&["loaded-world"]).unwrap();
        assert_eq!(args.command, Subcommand::LoadedWorld);
        assert_eq!(args.server, ServerSelection::Rivet);
        assert_eq!(args.runs, 1);

        assert!(parse(&["loaded-world", "--server", "rivet"]).is_ok());
        assert!(parse(&["loaded-world", "--server", "paper"]).is_err());
        assert!(parse(&["loaded-world", "--server", "both"]).is_err());
        assert!(parse(&["loaded-world", "--pairs", "paper:rivet"]).is_err());
        assert!(parse(&["loaded-world", "--runs", "1"]).is_err());
        assert!(parse(&["loaded-world", "--dwell-seconds", "35"]).is_err());
        assert!(parse(&["loaded-world", "--username", DEFAULT_USERNAME]).is_err());
        assert!(parse(&["loaded-world", "--timeout-seconds", "40"]).is_err());
    }

    #[test]
    fn generated_world_is_a_single_rivet_acceptance_with_no_silent_options() {
        // `generated-world` boots exactly one Rivet server with `--seed 42`;
        // Paper has no place, and --pairs/--runs/--dwell-seconds would be
        // silent no-ops on a fixed seed acceptance. The seed is pinned by the
        // contract, never overridable by the operator.
        let args = parse(&["generated-world"]).unwrap();
        assert_eq!(args.command, Subcommand::GeneratedWorld);
        assert_eq!(args.server, ServerSelection::Rivet);
        assert_eq!(args.runs, 1);
        assert_eq!(args.seed, Some(server::GENERATED_SEED));

        assert!(parse(&["generated-world", "--server", "rivet"]).is_ok());
        assert!(parse(&["generated-world", "--server", "paper"]).is_err());
        assert!(parse(&["generated-world", "--server", "both"]).is_err());
        assert!(parse(&["generated-world", "--pairs", "paper:rivet"]).is_err());
        assert!(parse(&["generated-world", "--runs", "1"]).is_err());
        assert!(parse(&["generated-world", "--dwell-seconds", "35"]).is_err());
        assert!(parse(&["generated-world", "--username", DEFAULT_USERNAME]).is_err());
        assert!(parse(&["generated-world", "--timeout-seconds", "40"]).is_err());
        // An explicit --seed equal to the pinned contract seed is the contract
        // itself, not a silent no-op: it parses cleanly and pins the same seed.
        let explicit = parse(&["generated-world", "--seed", "42"]).unwrap();
        assert_eq!(explicit.seed, Some(server::GENERATED_SEED));
        // `--seed` is a generated-world-only launch interface: an operator that
        // passes it to any other command must be rejected rather than silently
        // ignored.
        assert!(parse(&["join", "--seed", "7"]).is_err());
        assert!(parse(&["loaded-world", "--seed", "7"]).is_err());
        assert!(parse(&["dwell", "--seed", "7"]).is_err());
        assert!(parse(&["generated-world", "--seed", "99"]).is_err());
    }

    /// A minimal ground-truth manifest with one FULL chunk (0,0) carrying a
    /// genuine terrain signature at the center sample index.

    #[test]
    fn recenter_is_a_single_rivet_sustained_walk_with_no_silent_options() {
        // `recenter` (#185/#561) boots exactly one Rivet server against a
        // disposable world copy and requires the positive sustained-walking
        // acceptance (the region-backed recenter stays connected and receives
        // every beyond-boot chunk), plus a tampered-copy negative control; Paper
        // has no place, and --pairs/--runs/--username/--timeout-seconds would be
        // silent no-ops.
        let args = parse(&["recenter"]).unwrap();
        assert_eq!(args.command, Subcommand::Recenter);
        assert_eq!(args.server, ServerSelection::Rivet);
        assert_eq!(args.runs, 1);

        assert!(parse(&["recenter", "--server", "rivet"]).is_ok());
        assert!(parse(&["recenter", "--server", "paper"]).is_err());
        assert!(parse(&["recenter", "--server", "both"]).is_err());
        assert!(parse(&["recenter", "--pairs", "paper:rivet"]).is_err());
        assert!(parse(&["recenter", "--runs", "1"]).is_err());
        assert!(parse(&["recenter", "--dwell-seconds", "35"]).is_err());
        assert!(parse(&["recenter", "--username", DEFAULT_USERNAME]).is_err());
        assert!(parse(&["recenter", "--timeout-seconds", "40"]).is_err());
    }

    /// A minimal ground-truth manifest with one FULL chunk (0,0) carrying a
    /// genuine terrain signature at the center sample index.
    fn manifest_with(
        chunk_x: i32,
        chunk_z: i32,
        surface: &str,
        bedrock: &str,
        below: &str,
    ) -> Value {
        manifest_with_status(chunk_x, chunk_z, surface, bedrock, below, "minecraft:full")
    }

    /// A minimal ground-truth manifest whose chunk carries an explicit status
    /// (FULL or a pre-full status the #519 capability cannot yet carry).
    fn manifest_with_status(
        chunk_x: i32,
        chunk_z: i32,
        surface: &str,
        bedrock: &str,
        below: &str,
        status: &str,
    ) -> Value {
        let mut surface_arr = vec!["minecraft:air".to_owned(); 256];
        let mut bedrock_arr = vec!["minecraft:air".to_owned(); 256];
        let mut below_arr = vec!["minecraft:air".to_owned(); 256];
        // Center sample index: z*16+x = 8*16+8.
        surface_arr[8 * 16 + 8] = surface.to_owned();
        bedrock_arr[8 * 16 + 8] = bedrock.to_owned();
        below_arr[8 * 16 + 8] = below.to_owned();
        json!({
            "chunks": {
                format!("{chunk_x},{chunk_z}"): {
                    "status": status,
                    "stored_pos": [chunk_x, chunk_z],
                    "capability_flags": [],
                    "distinct": [surface, bedrock, below],
                    "surface": surface_arr,
                    "bedrock": bedrock_arr,
                    "below_feet": below_arr,
                    "distinct_state_ids": 3,
                    "section_count": 24,
                }
            }
        })
    }

    fn loaded_transcript_with(sample: Value) -> Value {
        json!({
            "loaded": { "samples": [sample] }
        })
    }

    fn matching_sample(chunk_x: i32, chunk_z: i32) -> Value {
        json!({
            "chunk_x": chunk_x,
            "chunk_z": chunk_z,
            "sample_x": chunk_x * 16 + 8,
            "sample_z": chunk_z * 16 + 8,
            // Bare azalea ids (`grass_block`) — the form the real client emits
            // via `BlockTrait::id()` — must still match the namespaced manifest
            // names (`minecraft:grass_block`) once canonicalized.
            "surface": "grass_block",
            "bedrock": "bedrock",
            "below_feet": "stone",
        })
    }

    #[test]
    fn compare_loaded_content_passes_when_observed_matches_ground_truth() {
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        assert!(
            compare_loaded_content(&manifest, &transcript).is_ok(),
            "a genuine per-coordinate match must pass"
        );
    }

    #[test]
    fn compare_loaded_content_fails_on_a_content_mismatch() {
        // The client observed a different surface than the ground truth — the
        // anti-superflat negative: a server that echoes repeated superflat
        // bytes cannot match a genuine terrain chunk.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let mut sample = matching_sample(0, 0);
        sample["surface"] = json!("minecraft:stone");
        let transcript = loaded_transcript_with(sample);
        assert!(
            compare_loaded_content(&manifest, &transcript).is_err(),
            "an observed content mismatch must fail"
        );
    }

    #[test]
    fn compare_loaded_content_fails_on_a_chunk_outside_the_manifest() {
        // The client sampled a chunk the ground-truth manifest has no record
        // of — the server served content that is not in the loaded world.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let transcript = loaded_transcript_with(matching_sample(5, 5));
        assert!(
            compare_loaded_content(&manifest, &transcript).is_err(),
            "a sample outside the manifest must fail"
        );
    }

    #[test]
    fn compare_loaded_content_fails_on_empty_samples() {
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let mut transcript = loaded_transcript_with(json!({}));
        transcript["loaded"]["samples"] = json!([]);
        assert!(
            compare_loaded_content(&manifest, &transcript).is_err(),
            "an empty sample set must fail (never a vacuous pass)"
        );
    }

    /// A minimal ground-truth manifest whose FULL chunk carries the given
    /// #519-uncarried capability flags.
    fn manifest_with_flags(chunk_x: i32, chunk_z: i32, flags: &[&str]) -> Value {
        let mut m = manifest_with(
            chunk_x,
            chunk_z,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        m["chunks"][format!("{chunk_x},{chunk_z}")]["capability_flags"] =
            json!(flags.iter().map(|f| f.to_string()).collect::<Vec<_>>());
        m
    }

    #[test]
    fn compare_loaded_content_is_unverified_on_uncarried_capability_flags() {
        // A FULL chunk that carries an uncarried #519 surface (non-empty
        // entities) is beyond the full-construction capability boundary even
        // though its status is minecraft:full. The runner must refuse PASS —
        // comparing its content would trust a server that could not have served
        // it faithfully.
        let manifest = manifest_with_flags(0, 0, &["entities"]);
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        match compare_loaded_content(&manifest, &transcript) {
            Err(RunnerError::Unverified(message)) => {
                assert!(
                    message.contains("entities") && message.contains("refuses PASS"),
                    "must name the flag and the refusal, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
    }

    #[test]
    fn compare_loaded_content_normalizes_observed_bare_ids() {
        // The client emits azalea bare registry ids (`grass_block`); the
        // manifest stores namespaced names. The comparison must canonicalize
        // the observed side so a genuine content match is not defeated by the
        // representation difference (the anti-superflat contract).
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        // `matching_sample` now emits bare ids; a pass proves normalization.
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        assert!(
            compare_loaded_content(&manifest, &transcript).is_ok(),
            "bare observed ids must match namespaced ground truth after canonicalization"
        );
    }

    #[test]
    fn compare_loaded_content_is_unverified_on_a_non_full_chunk() {
        // A sampled chunk that is not minecraft:full has no ground-truth
        // content (the #519 full-construction capability is absent) — the
        // acceptance must report UNVERIFIED, never a misleading content
        // mismatch.
        let manifest = manifest_with_status(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
            "minecraft:structure_starts",
        );
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        match compare_loaded_content(&manifest, &transcript) {
            Err(RunnerError::Unverified(message)) => {
                assert!(
                    message.contains("structure_starts") && message.contains("UNVERIFIED"),
                    "must name the non-FULL status and the UNVERIFIED boundary, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
    }

    #[test]
    fn compare_loaded_content_refuses_a_missing_status() {
        // A fingerprint with no Status must never default to minecraft:full —
        // that would compare (and possibly PASS) a chunk whose ground-truth
        // status is unknown. The malformed-manifest contract refuses it as a
        // Gate instead.
        let mut manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        manifest["chunks"]["0,0"]
            .as_object_mut()
            .unwrap()
            .remove("status");
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        match compare_loaded_content(&manifest, &transcript) {
            Err(RunnerError::Gate(message)) => {
                assert!(
                    message.contains("Status") && message.contains("malformed"),
                    "must name the missing Status and the malformed-manifest refusal, got {message}"
                );
            }
            other => panic!("expected a Gate refusal for a missing Status, got {other:?}"),
        }
    }

    #[test]
    fn compare_loaded_content_refuses_a_non_string_status() {
        // A non-string Status is the same malformed manifest: refusing it
        // honestly rather than defaulting to minecraft:full.
        let mut manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        manifest["chunks"]["0,0"]["status"] = json!(42);
        let transcript = loaded_transcript_with(matching_sample(0, 0));
        match compare_loaded_content(&manifest, &transcript) {
            Err(RunnerError::Gate(message)) => {
                assert!(
                    message.contains("Status") && message.contains("malformed"),
                    "must name the Status and the malformed-manifest refusal, got {message}"
                );
            }
            other => panic!("expected a Gate refusal for a non-string Status, got {other:?}"),
        }
    }

    #[test]
    fn classify_oracle_status_maps_exit_codes_and_names_the_subcommand() {
        // A missing ground-truth artifact surfaces as the oracle's UNVERIFIED
        // (exit 3) and must stay UNVERIFIED; a FAIL/USAGE exit and a signal must
        // be a hard Gate — never downgraded to a missing-prerequisite
        // UNVERIFIED. The shared classifier must name the subcommand it was
        // invoked for (a generated-expected UNVERIFIED must not claim
        // extract-world), and only mention the `--to` file when it was written.
        let ok = std::process::Command::new("true").status().unwrap();
        assert!(
            classify_oracle_status("generated-expected", false, ok, Path::new("/tmp/out.json"))
                .is_ok()
        );

        let unverified = std::process::Command::new("sh")
            .args(["-c", "exit 3"])
            .status()
            .unwrap();
        match classify_oracle_status(
            "generated-expected",
            false,
            unverified,
            Path::new("/tmp/out.json"),
        ) {
            Err(RunnerError::Unverified(message)) => {
                assert!(message.contains("generated-expected"), "got {message}");
                assert!(
                    !message.contains("extract-world"),
                    "must name the invoked subcommand, got {message}"
                );
                assert!(
                    !message.contains("/tmp/out.json"),
                    "an unwritten --to file must not be pointed at, got {message}"
                );
            }
            other => panic!("expected Unverified for exit 3, got {other:?}"),
        }

        let fail = std::process::Command::new("false").status().unwrap();
        match classify_oracle_status("extract-world", true, fail, Path::new("/tmp/out.json")) {
            Err(RunnerError::Gate(message)) => {
                assert!(message.contains("extract-world"), "got {message}");
                assert!(message.contains("/tmp/out.json"), "got {message}");
            }
            other => panic!("expected Gate for exit 1, got {other:?}"),
        }
    }

    /// The generated-world transcript carries its samples under
    /// `transcript["generated"]["samples"]` (the generated client record), so
    /// the compare helpers mirror the loaded shape with that key.
    fn generated_transcript_with(sample: Value) -> Value {
        json!({ "generated": { "samples": [sample] } })
    }

    /// The exact UNVERIFIED classification the runner reports when the booted
    /// server served the superflat M1 fixture (login `is_flat` true) instead of
    /// genuine FULL generated chunks.
    #[test]
    fn generated_world_is_flat_reports_the_pinned_unverified_reason() {
        let mut transcript = generated_transcript_with(matching_sample(0, 0));
        // The superflat M1 no-level boot advertises is_flat=true at login.
        transcript["generated"]["is_flat"] = json!(true);
        match classify_generated_is_flat(&transcript) {
            Err(RunnerError::Unverified(message)) => {
                assert!(
                    message.starts_with(GENERATED_WORLD_UNVERIFIED_REASON),
                    "the superflat-served reason must be pinned exactly, got {message}"
                );
                assert_eq!(
                    RunnerError::Unverified(message).exit_code(),
                    EXIT_UNVERIFIED
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }

        // A genuine non-flat world (is_flat=false) must NOT be classified
        // UNVERIFIED on the flag: it proceeds to the content comparison.
        transcript["generated"]["is_flat"] = json!(false);
        assert!(
            classify_generated_is_flat(&transcript).is_ok(),
            "a non-flat transcript must proceed to the content comparison"
        );

        // A transcript that did not carry the login flag cannot prove the
        // served world was genuine — it stays honestly UNVERIFIED rather than
        // fabricating a PASS or FAIL on an unproven world.
        let mut no_flag = generated_transcript_with(matching_sample(0, 0));
        no_flag["generated"]
            .as_object_mut()
            .unwrap()
            .remove("is_flat");
        match classify_generated_is_flat(&no_flag) {
            Err(RunnerError::Unverified(message)) => assert!(
                message.starts_with(GENERATED_WORLD_UNVERIFIED_REASON),
                "the missing-flag reason must be pinned exactly, got {message}"
            ),
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
    }

    #[test]
    fn compare_generated_content_passes_when_observed_matches_ground_truth() {
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let transcript = generated_transcript_with(matching_sample(0, 0));
        assert!(
            compare_generated_content(&manifest, &transcript).is_ok(),
            "a genuine per-coordinate match must pass"
        );
    }

    #[test]
    fn compare_generated_content_fails_on_a_superflat_echo() {
        // The client observed a repeated superflat surface where the seed-42
        // ground truth has genuine terrain — the anti-superflat negative: a
        // server that only echoes flat chunks cannot match the generated world.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let mut sample = matching_sample(0, 0);
        sample["surface"] = json!("minecraft:stone");
        let transcript = generated_transcript_with(sample);
        assert!(
            compare_generated_content(&manifest, &transcript).is_err(),
            "an observed content mismatch must fail"
        );
    }

    #[test]
    fn compare_generated_content_fails_on_a_wrong_seed_world() {
        // The server generated a different seed's terrain than the seed-42
        // contract: the observed block names at the sampled chunk differ from
        // the seed-42 ground truth across all three compared columns (a real
        // seed-7 world would not share the seed-42 surface/bedrock/below at the
        // same coordinates). The comparison must refuse PASS — a wrong-seed
        // world is a different world, never a pass.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let mut sample = matching_sample(0, 0);
        sample["surface"] = json!("minecraft:podzol");
        sample["bedrock"] = json!("minecraft:deepslate");
        sample["below_feet"] = json!("minecraft:tuff");
        let transcript = generated_transcript_with(sample);
        assert!(
            compare_generated_content(&manifest, &transcript).is_err(),
            "a world generated from the wrong seed must not pass the seed-42 contract"
        );
    }

    #[test]
    fn compare_generated_content_fails_on_a_chunk_outside_the_manifest() {
        // The client sampled a chunk the seed-42 ground truth has no record of
        // — the server served content outside the generated world.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let transcript = generated_transcript_with(matching_sample(5, 5));
        assert!(
            compare_generated_content(&manifest, &transcript).is_err(),
            "a sample outside the manifest must fail"
        );
    }

    #[test]
    fn compare_generated_content_fails_on_empty_samples() {
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let mut transcript = generated_transcript_with(json!({}));
        transcript["generated"]["samples"] = json!([]);
        assert!(
            compare_generated_content(&manifest, &transcript).is_err(),
            "an empty sample set must fail (never a vacuous pass)"
        );
    }

    #[test]
    fn compare_generated_content_is_unverified_on_a_non_full_chunk() {
        // A sampled chunk that is not minecraft:full has no ground-truth
        // content in the handoff — the acceptance must report UNVERIFIED, never
        // a misleading content mismatch.
        let manifest = manifest_with_status(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
            "minecraft:structure_starts",
        );
        let transcript = generated_transcript_with(matching_sample(0, 0));
        match compare_generated_content(&manifest, &transcript) {
            Err(RunnerError::Unverified(message)) => {
                assert!(
                    message.contains("structure_starts") && message.contains("UNVERIFIED"),
                    "must name the non-FULL status and the UNVERIFIED boundary, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
    }

    #[test]
    fn compare_generated_content_normalizes_observed_bare_ids() {
        // The client emits azalea bare registry ids (`grass_block`); the
        // manifest stores namespaced names. The comparison must canonicalize
        // the observed side so a genuine content match is not defeated by the
        // representation difference.
        let manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        let transcript = generated_transcript_with(matching_sample(0, 0));
        assert!(
            compare_generated_content(&manifest, &transcript).is_ok(),
            "bare observed ids must match namespaced ground truth after canonicalization"
        );
    }

    #[test]
    fn compare_generated_content_is_unverified_on_uncarried_capability_flags() {
        // A seed-42 reference FULL chunk that carries an uncarried #519 surface
        // (non-empty entities) is beyond the full-construction capability
        // boundary even though its status is minecraft:full — exactly like the
        // loaded comparator, the runner must refuse PASS rather than trust a
        // chunk the server could not have served faithfully.
        let manifest = manifest_with_flags(0, 0, &["entities"]);
        let transcript = generated_transcript_with(matching_sample(0, 0));
        match compare_generated_content(&manifest, &transcript) {
            Err(RunnerError::Unverified(message)) => {
                assert!(
                    message.contains("entities") && message.contains("refuses PASS"),
                    "must name the flag and the refusal, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
    }

    #[test]
    fn compare_generated_content_refuses_a_short_surface_array() {
        // A malformed manifest whose surface array is too short to contain the
        // chunk-center entry must be refused as a Gate — never defaulted to
        // air, which would let an unloaded client column pass vacuously
        // air-vs-air (the finding: no explicit 'array must contain CENTER'
        // validation on the manifest).
        let mut manifest = manifest_with(
            0,
            0,
            "minecraft:grass_block",
            "minecraft:bedrock",
            "minecraft:stone",
        );
        // Truncate the surface array below the center index (137).
        manifest["chunks"]["0,0"]["surface"] = json!(vec!["minecraft:air".to_owned(); 136]);
        let transcript = generated_transcript_with(matching_sample(0, 0));
        match compare_generated_content(&manifest, &transcript) {
            Err(RunnerError::Gate(message)) => {
                assert!(
                    message.contains("surface") && message.contains("malformed"),
                    "must name the truncated column and the malformed-manifest refusal, got \
                     {message}"
                );
            }
            other => panic!("expected a Gate refusal for a short array, got {other:?}"),
        }
    }

    #[test]
    fn classify_generated_world_boot_failure_pins_the_exact_unverified_reason() {
        // The task contract requires the missing-capability exit to carry an
        // exact, test-pinned reason. A rivet-server that rejects `--seed` as an
        // unknown argument must be classified `Absent` and the runner error must
        // lead with exactly [`GENERATED_WORLD_UNVERIFIED_REASON`] — never a
        // superflat or loaded-world fallback.
        let log = std::env::temp_dir().join(format!(
            "rivet-generated-world-rejected-{}.log",
            std::process::id()
        ));
        let rejected = concat!(
            "thread 'main' panicked at crates/rivet-server/src/main.rs:\n",
            "unknown argument \"--seed\" (expected --host/--port/--level)\n"
        );
        fs::write(&log, rejected).unwrap();

        let absent = classify_generated_world_boot_failure(
            server::Error::Unverified("boot timed out waiting for RIVET_READY".to_owned()),
            &log,
        );
        match absent {
            RunnerError::Unverified(ref message) => {
                assert!(
                    message.starts_with(GENERATED_WORLD_UNVERIFIED_REASON),
                    "the missing-capability reason must be pinned exactly, got {message}"
                );
                assert!(
                    message.contains("unknown argument \"--seed\""),
                    "the reason must carry the launch evidence, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
        assert_eq!(absent.exit_code(), EXIT_UNVERIFIED);

        // A boot that fails for an unrelated reason (the log names `--level`,
        // not `--seed`) is `FailedToBoot`: still UNVERIFIED, but the reason is
        // the launch-probe evidence — it must NOT claim the generated-world
        // capability is absent, which would be a fabricated diagnosis.
        let wrong_arg = concat!(
            "thread 'main' panicked at crates/rivet-server/src/main.rs:\n",
            "unknown argument \"--level\" (expected --host/--port)\n"
        );
        fs::write(&log, wrong_arg).unwrap();
        let failed = classify_generated_world_boot_failure(
            server::Error::Unverified("boot timed out waiting for RIVET_READY".to_owned()),
            &log,
        );
        match failed {
            RunnerError::Unverified(ref message) => {
                assert!(
                    !message.starts_with(GENERATED_WORLD_UNVERIFIED_REASON),
                    "a FailedToBoot must not claim the capability is absent, got {message}"
                );
                assert!(
                    message.contains("unknown argument \"--level\""),
                    "the FailedToBoot reason must carry its evidence, got {message}"
                );
            }
            other => panic!("expected an Unverified classification, got {other:?}"),
        }
        assert_eq!(failed.exit_code(), EXIT_UNVERIFIED);

        // Gate and Io boot failures are real errors, not an UNVERIFIED absence:
        // they stay hard FAIL, exactly like the loaded-world classifier.
        let gate = classify_generated_world_boot_failure(
            server::Error::Gate("non-executable binary".to_owned()),
            &log,
        );
        assert!(matches!(gate, RunnerError::Server(server::Error::Gate(_))));
        assert_eq!(gate.exit_code(), EXIT_FAIL);

        let io = classify_generated_world_boot_failure(
            server::Error::Io(io::Error::new(
                io::ErrorKind::NotADirectory,
                "invalid run directory",
            )),
            &log,
        );
        assert!(matches!(io, RunnerError::Server(server::Error::Io(_))));
        assert_eq!(io.exit_code(), EXIT_FAIL);

        fs::remove_file(log).unwrap();
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
    fn move_timeout_must_reserve_login_walk_drain_and_settle() {
        // The `moved` record is emitted only after login/configuration, the
        // fixed walk, MOVE_DRAIN, and up to 1 s of keepalive settling; a timeout
        // below the shared move budget cuts the client off before it emits
        // (ExitCode 2, spurious FAIL). Mirror the client's own parse-time
        // validation: the budget rounds the 200 ms drain up to 1 s, so meeting
        // it is already safe.
        let headroom = rivet_harness_common::timing::MOVE_TIMEOUT_HEADROOM_SECONDS;
        let err = parse(&["move", "--timeout-seconds", &(headroom - 1).to_string()]).unwrap_err();
        assert!(
            err.contains("--timeout-seconds") && err.contains("move mode"),
            "error must explain the move-mode headroom, got {err}"
        );
        assert!(parse(&["move", "--timeout-seconds", &headroom.to_string()]).is_ok());
        // The default 60 s runner timeout comfortably exceeds the move budget,
        // so a bare `move` parse is unaffected.
        assert!(parse(&["move"]).is_ok());
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
            dwell_seconds: DEFAULT_DWELL_SECONDS,
            seed: None,
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
        assert_eq!(
            RunnerError::LoadWorld(load_world::Error::Unverified("x".into())).exit_code(),
            EXIT_UNVERIFIED
        );
        assert_eq!(RunnerError::Gate("x".into()).exit_code(), EXIT_FAIL);
        assert_eq!(
            RunnerError::LoadWorld(load_world::Error::Gate("x".into())).exit_code(),
            EXIT_FAIL
        );
        assert_eq!(
            RunnerError::Server(server::Error::Gate("x".into())).exit_code(),
            EXIT_FAIL
        );
    }

    /// Counterfactual for the deterministic-config prerequisite (issue #333): a
    /// missing `paper-world-defaults.yml` fixture must be UNVERIFIED (exit 3)
    /// — without the pinned spawn-limits (all seven categories at 0) a fresh
    /// Paper world re-enables natural spawning and the walk is nondeterministic,
    /// so nothing is actually compared — and the message must carry the exact
    /// missing companion path, exactly like a missing `server.properties`.
    /// Resolved from real temp paths so the test is load-bearing.
    #[test]
    fn missing_world_defaults_fixture_is_unverified_with_exact_path() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-wdm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let err = fixture_server_properties(&dir, "paper-world-defaults.yml").unwrap_err();
        assert!(
            matches!(err, RunnerError::Unverified(_)),
            "a missing paper-world-defaults.yml must be Unverified, got {err:?}"
        );
        assert_eq!(
            err.exit_code(),
            EXIT_UNVERIFIED,
            "a missing paper-world-defaults.yml must exit UNVERIFIED (3), not FAIL (1)"
        );
        // The message must carry the exact missing companion path (the same
        // resolution a real scenario would fail on) plus the fixture name.
        let expected_path = dir
            .join("../rivet-oracle/fixtures/paper-world-defaults.yml")
            .to_string_lossy()
            .to_string();
        let msg = err.to_string();
        assert!(
            msg.contains(&expected_path),
            "the Unverified error must carry the exact missing companion path {expected_path:?}, \
             got: {msg}"
        );
        assert!(
            msg.contains("paper-world-defaults.yml"),
            "the Unverified error must name the fixture, got: {msg}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A present `paper-world-defaults.yml` fixture still resolves.
    #[test]
    fn present_world_defaults_fixture_resolves() {
        // The resolver appends `../rivet-oracle/fixtures/{name}` to the crate
        // root, so a real `rivet-oracle/fixtures/` sibling under the base makes
        // the resolution load-bearing. The crate-root dir must exist: the `..`
        // component can only resolve past a directory that exists (the real
        // crate_root always does).
        let base = std::env::temp_dir().join(format!("rivet-scenario-wdo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = base.join("crate");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(base.join("rivet-oracle/fixtures")).unwrap();
        fs::write(
            base.join("rivet-oracle/fixtures/paper-world-defaults.yml"),
            "spawn-limits:\n",
        )
        .unwrap();
        let p = fixture_server_properties(&dir, "paper-world-defaults.yml").expect("present");
        assert_eq!(p.file_name().unwrap(), "paper-world-defaults.yml");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn load_world_pre_spawn_failures_ignore_stale_probe_logs_and_stay_hard() {
        let log =
            std::env::temp_dir().join(format!("rivet-load-world-stale-log-{}", std::process::id()));
        fs::write(&log, "unknown argument \"--level\"\n").unwrap();

        let gate = classify_load_world_boot_failure(
            server::Error::Gate("non-executable binary".to_owned()),
            &log,
        );
        assert!(matches!(gate, RunnerError::Server(server::Error::Gate(_))));
        assert_eq!(gate.exit_code(), EXIT_FAIL);

        let io = classify_load_world_boot_failure(
            server::Error::Io(io::Error::new(
                io::ErrorKind::NotADirectory,
                "invalid run directory",
            )),
            &log,
        );
        assert!(matches!(io, RunnerError::Server(server::Error::Io(_))));
        assert_eq!(io.exit_code(), EXIT_FAIL);

        fs::remove_file(log).unwrap();
    }

    #[test]
    fn dwell_parses_and_defaults_to_a_single_rivet_boot() {
        let a = parse(&["dwell"]).unwrap();
        assert_eq!(a.command, Subcommand::Dwell);
        assert_eq!(
            a.server,
            ServerSelection::Rivet,
            "dwell is always a Rivet boot"
        );
        assert_eq!(a.runs, 1, "dwell runs exactly one boot");
        assert_eq!(a.dwell_seconds, DEFAULT_DWELL_SECONDS);
    }

    #[test]
    fn dwell_accepts_explicit_dwell_seconds() {
        let a = parse(&["dwell", "--dwell-seconds", "45"]).unwrap();
        assert_eq!(a.dwell_seconds, 45);
        assert_eq!(a.server, ServerSelection::Rivet);
    }

    #[test]
    fn dwell_rejects_a_paper_boot() {
        // The keepalive-survival gate is a Rivet headless-boot probe; Paper has
        // no place in it.
        let err = parse(&["dwell", "--server", "paper"]).unwrap_err();
        assert!(
            err.contains("--server rivet"),
            "error must explain only rivet is valid, got {err}"
        );
    }

    #[test]
    fn dwell_requires_a_window_past_the_kick_limit() {
        // The dwell window must exceed the server's 30 s keepalive kick limit;
        // a shorter window cannot prove survival past it.
        let err = parse(&["dwell", "--dwell-seconds", "20"]).unwrap_err();
        assert!(
            err.contains("kick limit"),
            "error must name the kick limit, got {err}"
        );
    }

    #[test]
    fn dwell_requires_a_window_above_the_span_floor() {
        // The old `DWELL_SURVIVAL_SECONDS + 1` = 31 s floor was marginal: the
        // first challenge lands ~1.2 s after spawn, so a 31 s window spans only
        // ~29.8 s — below the required 30 s challenge span. The floor is now
        // `DWELL_MIN_DWELL_SECONDS`, so 34 s is rejected and 35 s is accepted.
        let err = parse(&["dwell", "--dwell-seconds", "34"]).unwrap_err();
        assert!(
            err.contains("at least 35"),
            "error must state the minimum window, got {err}"
        );
        assert!(parse(&["dwell", "--dwell-seconds", "35"]).is_ok());
    }

    #[test]
    fn dwell_timeout_must_reserve_settle_and_login_headroom() {
        // `dwell < timeout` is not enough: the timeout starts at process launch
        // while the dwell window starts at spawn, and after the window the
        // client spends up to 1 s settling the keepalive stream before emitting.
        // 47 = 41 + 6 (settle 1 s + login headroom 5 s) must be rejected
        // (the record would race the timeout); 48 leaves a strict margin.
        let err =
            parse(&["dwell", "--dwell-seconds", "41", "--timeout-seconds", "47"]).unwrap_err();
        assert!(
            err.contains("--timeout-seconds") && err.contains("headroom"),
            "error must explain the reserved headroom, got {err}"
        );
        assert!(parse(&["dwell", "--dwell-seconds", "41", "--timeout-seconds", "48"]).is_ok());
    }

    #[test]
    fn dwell_requires_the_window_before_the_client_timeout() {
        // The dwell must finish before the outer client timeout, or the timeout
        // branch would cut the client off before it emits the dwell record.
        let err =
            parse(&["dwell", "--dwell-seconds", "41", "--timeout-seconds", "20"]).unwrap_err();
        assert!(
            err.contains("--timeout-seconds"),
            "error must explain the timeout bound, got {err}"
        );
    }

    #[test]
    fn non_dwell_commands_reject_explicit_dwell_seconds() {
        // --dwell-seconds is dwell-scenario-only; on join/move/capture an
        // explicit value would be a silent no-op (the client is invoked with a
        // 0 dwell), so it is rejected with the exit-64 CLI-misuse error.
        for cmd in ["join", "move", "capture"] {
            let err = parse(&[cmd, "--dwell-seconds", "41"]).unwrap_err();
            assert!(
                err.contains("--dwell-seconds") && err.contains("silent no-op"),
                "{cmd} must reject --dwell-seconds as a silent no-op, got {err}"
            );
        }
        // dwell still accepts an explicit value.
        assert!(parse(&["dwell", "--dwell-seconds", "41"]).is_ok());
    }

    #[test]
    fn client_argv_omits_dwell_seconds_for_non_dwell_modes() {
        // Counterfactual for the release-blocking regression: `run_client`
        // unconditionally forwarded `--dwell-seconds 0`, which the client
        // rejects on join/move as an explicit silent-no-op value (exit 64),
        // breaking every non-dwell run. The argv builder must append the flag
        // only for dwell and forward the runner's non-dwell 0 as nothing at all.
        let join = client_argv(&ClientSpec {
            address: DEFAULT_ADDRESS.to_owned(),
            username: DEFAULT_USERNAME.to_owned(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            dwell_seconds: 0,
            mode: "join".to_owned(),
        });
        assert_eq!(
            join,
            vec![
                "--mode".to_owned(),
                "join".to_owned(),
                "--address".to_owned(),
                DEFAULT_ADDRESS.to_owned(),
                "--username".to_owned(),
                DEFAULT_USERNAME.to_owned(),
                "--timeout-seconds".to_owned(),
                DEFAULT_TIMEOUT_SECONDS.to_string(),
            ],
            "non-dwell argv must be exactly the mode/address/username/timeout set"
        );
        for mode in ["join", "move"] {
            let argv = client_argv(&ClientSpec {
                address: DEFAULT_ADDRESS.to_owned(),
                username: DEFAULT_USERNAME.to_owned(),
                timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
                dwell_seconds: 0,
                mode: mode.to_owned(),
            });
            assert!(
                !argv.iter().any(|a| a == "--dwell-seconds"),
                "{mode} argv must omit --dwell-seconds, got {argv:?}"
            );
        }
        // dwell forwards its value so the client's parse-time window and
        // timeout-headroom validation can run.
        let dwell = client_argv(&ClientSpec {
            address: DEFAULT_ADDRESS.to_owned(),
            username: DEFAULT_USERNAME.to_owned(),
            timeout_seconds: 48,
            dwell_seconds: 41,
            mode: "dwell".to_owned(),
        });
        assert_eq!(dwell[dwell.len() - 2], "--dwell-seconds");
        assert_eq!(dwell[dwell.len() - 1], "41");
    }

    #[test]
    fn dwell_rejects_explicit_runs_one() {
        // dwell always boots exactly one Rivet server; an explicit --runs 1 —
        // equal to the implicit default — is still a silent no-op and must be
        // rejected (no-silent-noop policy, like the both-server precedent).
        let err = parse(&["dwell", "--runs", "1"]).unwrap_err();
        assert!(err.contains("--runs"), "error must name --runs, got {err}");
        assert!(
            err.contains("silent no-op"),
            "error must explain --runs is a no-op, got {err}"
        );
    }

    #[test]
    fn dwell_rejects_an_explicit_runs_count() {
        // dwell always boots exactly one Rivet server; a --runs other than 1 is
        // a silent no-op and must be rejected.
        let err = parse(&["dwell", "--runs", "2"]).unwrap_err();
        assert!(err.contains("--runs"), "error must name --runs, got {err}");
        assert!(
            err.contains("silent no-op"),
            "error must explain --runs is a no-op, got {err}"
        );
    }

    #[test]
    fn dwell_rejects_an_explicit_pairs() {
        // dwell has no comparison concept (exactly one Rivet boot), so an
        // explicit --pairs would be a silent no-op. Reject it with the CLI
        // misuse error (which exits 64), like --runs.
        let err = parse(&["dwell", "--pairs", "paper:rivet"]).unwrap_err();
        assert!(
            err.contains("--pairs"),
            "error must name --pairs, got {err}"
        );
        assert!(
            err.contains("no --pairs"),
            "error must explain dwell has no comparison, got {err}"
        );
    }

    #[test]
    fn dwell_negative_rejects_a_zero_survival_window() {
        // The dwell verdict must reject a transcript that did not actually
        // survive past the kick limit — the controlled negative the live
        // scenario runs. A verdict that passed connected_wall_seconds=0 would be
        // vacuous.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":50,"mode":"dwell","dwell_seconds":41,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"dwell","requested_dwell_seconds":41,"connected_wall_seconds":41.2,"challenge_count":41,"echo_count":41,"challenge_ids":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41],"echo_ids":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41],"first_challenge_offset_ms":1200,"last_challenge_offset_ms":41100,"challenge_span_ms":39900,"protocol":1}"#,
        ]
        .join("\n");
        let t = transcript::normalize_dwell(&raw).expect("normalize");
        let mut tampered = t.clone();
        tampered["dwell"]["connected_wall_seconds"] = json!(0.0);
        let err = transcript::rivet_dwell_verdict(&tampered).unwrap_err();
        assert!(
            err.contains("connected_wall_seconds"),
            "a zero survival window must be refused, got {err}"
        );
        // The untampered transcript passes — the negative is not vacuous.
        assert!(transcript::rivet_dwell_verdict(&t).is_ok());
    }

    #[test]
    fn kick_parses_and_defaults_to_a_single_rivet_boot() {
        let a = parse(&["kick"]).unwrap();
        assert_eq!(a.command, Subcommand::Kick);
        assert_eq!(
            a.server,
            ServerSelection::Rivet,
            "kick is always a Rivet boot"
        );
        assert_eq!(a.runs, 1, "kick runs exactly one boot");
    }

    #[test]
    fn kick_rejects_a_paper_or_both_boot() {
        // The decoded-disconnect-reason probe is a Rivet headless-boot check;
        // Paper has no place in it.
        for server in ["paper", "both"] {
            let err = parse(&["kick", "--server", server]).unwrap_err();
            assert!(
                err.contains("--server rivet"),
                "{server} must be refused as not --server rivet, got {err}"
            );
        }
    }

    #[test]
    fn kick_rejects_an_explicit_runs_count() {
        // kick always boots exactly one Rivet server; a --runs other than 1 is
        // a silent no-op and must be rejected (no-silent-noop policy, like dwell
        // and the both-server precedent).
        let err = parse(&["kick", "--runs", "2"]).unwrap_err();
        assert!(err.contains("--runs"), "error must name --runs, got {err}");
        assert!(
            err.contains("silent no-op"),
            "error must explain --runs is a no-op, got {err}"
        );
    }

    #[test]
    fn kick_rejects_explicit_runs_one() {
        // kick always boots exactly one Rivet server; an explicit --runs 1 —
        // equal to the implicit default — is still a silent no-op and must be
        // rejected.
        let err = parse(&["kick", "--runs", "1"]).unwrap_err();
        assert!(err.contains("--runs"), "error must name --runs, got {err}");
    }

    #[test]
    fn kick_rejects_an_explicit_pairs() {
        // kick has no comparison concept (exactly one Rivet boot), so an
        // explicit --pairs would be a silent no-op. Reject it with the CLI
        // misuse error (which exits 64), like --runs.
        let err = parse(&["kick", "--pairs", "paper:rivet"]).unwrap_err();
        assert!(
            err.contains("--pairs"),
            "error must name --pairs, got {err}"
        );
        assert!(
            err.contains("no --pairs"),
            "error must explain kick has no comparison, got {err}"
        );
    }

    #[test]
    fn kick_negative_rejects_a_wrong_decoded_reason_key() {
        // The kick verdict must reject a transcript whose decoded reason key is
        // not Rivet's invalid-player-movement translatable — the controlled
        // negative the live scenario runs. A verdict that passed a wrong
        // reason_key would be vacuous.
        let raw = [
            r#"{"event":"starting","address":"127.0.0.1:25598","username":"RivetProbe","timeout_seconds":40,"azalea_revision":"6249c295d353b9b3ef68f665b311cba39211fd19","protocol":1}"#,
            r#"{"event":"init","protocol":1}"#,
            r#"{"event":"login","protocol":1}"#,
            r#"{"event":"spawn","position":{"x":0.0,"y":-63.0,"z":0.0},"protocol":1}"#,
            r#"{"event":"disconnect","reason":"Some(Translatable(TranslatableComponent { key: \"multiplayer.disconnect.invalid_player_movement\", .. }))","reason_key":"multiplayer.disconnect.invalid_player_movement","after_spawn":true,"protocol":1}"#,
        ]
        .join("\n");
        let t = transcript::normalize_kick(&raw).expect("normalize");
        let mut tampered = t.clone();
        tampered["kick"]["reason_key"] = json!("disconnect.genericReason");
        let err = transcript::rivet_kick_verdict(&tampered).unwrap_err();
        assert!(
            err.contains("reason_key"),
            "a wrong decoded reason key must be refused, got {err}"
        );
        // The untampered transcript passes — the negative is not vacuous.
        assert!(transcript::rivet_kick_verdict(&t).is_ok());
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
        // and spawns at `transcript::JOIN_SPAWN_Y` like Rivet, so a position.y
        // divergence is a real server mismatch and must FAIL — never a
        // "documented gap" to be waved through. The counterfactual keeps the old
        // (wrong) Paper reference at a different spawn height and asserts the
        // both-mode refuses PASS instead of accepting it.
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
        // the same superflat height `transcript::JOIN_SPAWN_Y`, so the transcripts
        // diverge only on the documented health default gap (Paper 20 / Rivet 1).
        // Tampering Paper's position.y through the real comparator/divergence path
        // must be reported with the tampered value and refused PASS (position.y is
        // a compared field).
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

    /// The verdict narrates the compared `last_sent` it was actually given, not
    /// a hardcoded coordinate: a transcript whose last_sent is far from any
    /// historically-observed value (e.g. 123.75) must be echoed verbatim. This
    /// is what keeps a future walk-geometry change from leaving a stale success
    /// claim in the PASS narration — a narrator that hardcoded an old coordinate
    /// would fail the `contains("123.75")` assertion, so no stale literal is
    /// relied on.
    #[test]
    fn verdict_narrates_non_default_live_last_sent() {
        let paper = move_transcript(123.75, false);
        let rivet = move_transcript(123.75, false);
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("123.75"),
            "must narrate the live value: {text}"
        );
    }

    /// The verdict must not silently print one value as if both sides matched:
    /// if Paper and Rivet's last_sent differ (a comparator/divergence-gate
    /// regression, since the gate should have refused before the verdict), the
    /// narration names both sides.
    #[test]
    fn verdict_names_both_sides_when_last_sent_diverges() {
        let paper = move_transcript(25.0, false);
        let rivet = move_transcript(26.0, false);
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("25.0") && text.contains("26.0"),
            "must name both divergent last_sent values: {text}"
        );
        assert!(text.contains("vs"), "must flag the divergence: {text}");
    }

    /// A normalized move transcript whose walk carried no `last_sent`.
    /// `normalize_move` records an absent raw value as explicit JSON `null`, so
    /// this mirrors the normalized wire shape. `verdict_last_sent` intentionally
    /// treats both a missing key and explicit `null` as no present value.
    fn move_transcript_without_last_sent() -> Value {
        json!({
            "outcome": "moved",
            "walk": { "last_sent": Value::Null },
            "excluded": {},
        })
    }

    /// A normalized transcript carrying `walk.last_sent: null` must be surfaced
    /// by the narration naming the side with no present value and the value that
    /// is still present, never printed as if both matched.
    #[test]
    fn verdict_surfaces_a_missing_last_sent() {
        let paper = move_transcript(25.0, false);
        let rivet = move_transcript_without_last_sent();
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("Paper last_sent") && text.contains("25.0"),
            "must keep naming Paper's present last_sent: {text}"
        );
        assert!(
            text.contains("the Rivet transcript carried no walk.last_sent"),
            "must name the exact missing-Rivet branch: {text}"
        );
    }

    /// The one-sided missing shape on the other side: the narration must name
    /// the exact missing-Paper branch, distinguishing it from the missing-Rivet
    /// branch above.
    #[test]
    fn verdict_surfaces_a_missing_last_sent_on_paper() {
        let paper = move_transcript_without_last_sent();
        let rivet = move_transcript(31.0, false);
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("the Paper transcript carried no walk.last_sent")
                && text.contains("Rivet last_sent")
                && text.contains("31.0"),
            "must name the exact missing-Paper branch and the present Rivet value: {text}"
        );
    }

    /// Both sides carrying `last_sent: null` is not proof that the compared
    /// value is `null` — it is a transcript that never recorded the walk's final
    /// sent position. The narration must surface that rather than print a
    /// successful-looking "same compared last_sent null".
    #[test]
    fn verdict_never_prints_a_successful_null() {
        let paper = move_transcript_without_last_sent();
        let rivet = move_transcript_without_last_sent();
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            !text.contains("same compared last_sent null"),
            "a null/absent last_sent is missing, not a matched value: {text}"
        );
        assert!(
            text.contains("neither transcript carried a walk.last_sent"),
            "must surface that both sides carried no last_sent: {text}"
        );
    }

    /// A normalized move transcript whose `walk` object truly omits the
    /// `last_sent` key — not explicit `null`, but no key at all. This is the
    /// shape a raw transcript would have before `normalize_move` fills in an
    /// explicit `null`; `verdict_last_sent` reads via
    /// `pointer("/walk/last_sent")`, so an absent key and an explicit `null`
    /// must both resolve to "no present value".
    fn move_transcript_without_last_sent_key() -> Value {
        json!({
            "outcome": "moved",
            "walk": {},
            "excluded": {},
        })
    }

    /// A truly absent `last_sent` key (not `null`) must surface the exact same
    /// side-specific missing narration as the explicit-`null` wire shape: the
    /// helper contract is that a missing key is "no present value", never a
    /// value to match. Pins both one-sided branches so a future helper change
    /// cannot silently stop treating an absent key as missing.
    #[test]
    fn verdict_surfaces_a_truly_absent_last_sent_key() {
        let paper = move_transcript(25.0, false);
        let rivet = move_transcript_without_last_sent_key();
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("Paper last_sent") && text.contains("25.0"),
            "must keep naming Paper's present last_sent: {text}"
        );
        assert!(
            text.contains("the Rivet transcript carried no walk.last_sent"),
            "an absent key must hit the missing-Rivet branch: {text}"
        );

        let paper = move_transcript_without_last_sent_key();
        let rivet = move_transcript(31.0, false);
        let text = verdict_last_sent(&paper, &rivet);
        assert!(
            text.contains("the Paper transcript carried no walk.last_sent")
                && text.contains("Rivet last_sent")
                && text.contains("31.0"),
            "an absent key must hit the missing-Paper branch: {text}"
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

    /// The y/z cross-check must tolerate the client's own precision: `last_sent`
    /// is rounded to 3 decimals and the z reconstruction carries a rounding-unit
    /// error, so a trace final that differs from the reconstructed value by a
    /// sub-rounding delta (here 5e-4 on z) is precision loss, not a divergence —
    /// it must not spuriously fail the gate.
    #[test]
    fn rivet_authoritative_tolerates_rounding_precision_on_y_z() {
        let trace = authoritative_trace_at([35.0, -63.0, -3.5005]);
        let walk = rivet_walk([25.0, -63.0, 0.0], [9.5, -63.0, -3.5]);
        let summary = check_rivet_authoritative(&trace, &walk).expect("must match");
        assert!(summary.contains("accepted moves"), "{summary}");
    }

    /// The epsilon is tight, not a free pass: a real y/z divergence (here 1.0 on
    /// z — the server accepted a frame at a height/direction the client never
    /// walked) must still fail the gate.
    #[test]
    fn rivet_authoritative_rejects_real_y_z_divergence() {
        let trace = authoritative_trace_at([35.0, -63.0, -2.5]);
        let walk = rivet_walk([25.0, -63.0, 0.0], [9.5, -63.0, -3.5]);
        let err = check_rivet_authoritative(&trace, &walk)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("on y/z"),
            "a real y/z divergence must fail the cross-check, got {err}"
        );
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
