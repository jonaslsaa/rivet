//! Server boot/shutdown for the scenario runner (issue #155).
//!
//! `ServerKind::Paper` boots the paperclip bundler jar headlessly (the `verify`
//! pattern from `tools/rivet-oracle`), waits for `Done (...)!`, then SIGTERM and
//! waits for the clean save. `ServerKind::Rivet` boots the `rivet-server`
//! binary headlessly, waits for the machine-readable `RIVET_READY` marker on
//! stdout (rivet-server/src/main.rs), then SIGTERM and waits for a clean exit
//! (code 0).
//!
//! The child-process lifecycle (spawn with log tee, READY poll, kill-on-drop,
//! SIGTERM + reap) is the shared `rivet-harness-common::server` module; this
//! module owns what is scenario-specific: which command to spawn per kind, the
//! READY marker test, the per-kind boot timeout, the run-dir preparation, the
//! Paper clean-save / Rivet clean-exit assertions, and the load-bearing Paper
//! provenance check.
//!
//! Paper boots reuse the paperclip-downloaded `libraries/` and `cache/` (the
//! slow ~160MB downloads) and wipe everything else, so each run is a fresh world
//! at a fixed seed while staying fast on re-runs. `versions/` is deliberately
//! NOT preserved: the paperclip re-materializes the server jar on every boot, so
//! `verify_paper_provenance` (called only by the Rivet-vs-Paper differential
//! path in main.rs, where the Paper reference must be the pinned oracle commit)
//! can only ever see the jar the artifact actually being booted produced — a
//! swapped, stale, or non-bundler artifact cannot silently stand in for the
//! pinned reference. Paper-vs-Paper self-checks and capture do not require the
//! pin: they compare a build against itself. Concurrent servers get distinct
//! ports (held `rivet-harness-common::port` reservations from main.rs): the
//! Paper run dir's `server.properties` is patched to the allocated port and
//! `rivet-server` is passed `--host`/`--port`, so no two servers in a scenario
//! can collide. Sequential single-server modes reuse the base port one boot at a
//! time, so they never contend.

use std::fmt;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use rivet_harness_common::server::ChildServer;

use crate::cargo_target_dir;

/// Name of the paperclip bundler jar we boot through.
pub const PAPERCLIP_JAR: &str = "paper-paperclip-26.2.local-SNAPSHOT.jar";

/// The Paper commit the scenario's Paper reference must be built from — the
/// same pin `tools/rivet-oracle/fixtures/manifest.json` records
/// (`paper: 26.2-DEV-main@0a99345`). The paperclip materializes the compiled
/// server into `versions/26.2/paper-26.2.jar`; its `Git-Commit` manifest
/// attribute must equal this pin for a Paper boot to be provenance-verified.
pub const PAPER_PIN_COMMIT: &str = "0a99345";

/// Machine-readable readiness marker printed by `rivet-server` on stdout once
/// the TCP listener is bound (crates/rivet-server/src/main.rs).
pub const RIVET_READY: &str = "RIVET_READY";

/// The world-path launch option the loaded-world acceptance probe passes to
/// rivet-server (`--level <path>`, issue #316). Kept in the seam module so the
/// argv construction and the probe classification share one token.
pub const WORLD_PATH_ARG: &str = "--level";

/// The generated-world launch option the generated-world acceptance probe
/// passes to rivet-server (`--seed <n>`). This is the explicit generated-world
/// server seam: a rivet-server build that rejects it (only
/// `--host`/`--port`/`--level`) is classified `Absent` (no way to boot a fresh
/// seed world at all). A build that accepts `--seed` still boots the superflat
/// M1 fixture (login `is_flat` true), not genuine FULL generated chunks; the
/// runner keys the pinned UNVERIFIED reason on the client-observable `is_flat`
/// flag until the server genuinely serves generated chunks. Kept in the seam
/// module so the argv construction and the probe classification share one token.
pub const GENERATED_SEED_ARG: &str = "--seed";

/// The seed the generated-world acceptance contract boots and compares: a
/// fresh disposable seed-42 world. The Paper ground-truth reference
/// (`rivet-oracle generated-expected`) is captured for exactly this seed.
///
/// This runner is the single source of truth: it always passes this seed
/// explicitly to both the server boot and `rivet-oracle generated-expected
/// <seed>`. The oracle has no seed default of its own, so there is no second
/// copy of the constant for the runner and oracle to drift apart on — a bare
/// oracle CLI call without a seed is a usage error, never a silent comparison
/// of the wrong world.
pub const GENERATED_SEED: u64 = 42;

/// How long to wait for Paper to reach `Done (...)!` (covers the paperclip
/// first-boot materialization of ~160MB libraries + worldgen).
pub const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
/// How long to wait for rivet-server to reach `RIVET_READY`.
pub const RIVET_BOOT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long to wait for a clean shutdown after SIGTERM.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(90);
/// Poll interval while watching the boot log / process exit.
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// eula.txt content; the Paper server refuses to boot without `eula=true`.
const EULA: &str = "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\neula=true\n";

/// Which server implementation the scenario drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    Paper,
    Rivet,
}

impl ServerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerKind::Paper => "paper",
            ServerKind::Rivet => "rivet",
        }
    }
}

impl fmt::Display for ServerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// UNVERIFIED: a prerequisite is missing (paperclip jar / rivet-server
    /// binary) or the server did not reach READY within its boot timeout. Maps
    /// to the gate's UNVERIFIED exit code 3.
    Unverified(String),
    /// A hard orchestration failure (spawn, shutdown, clean-save check).
    Gate(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Unverified(m) => write!(f, "{m}"),
            Error::Gate(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rivet_harness_common::server::Error> for Error {
    fn from(e: rivet_harness_common::server::Error) -> Self {
        match e {
            rivet_harness_common::server::Error::Io(e) => Error::Io(e),
            rivet_harness_common::server::Error::Unverified(m) => Error::Unverified(m),
            rivet_harness_common::server::Error::Gate(m) => Error::Gate(m),
        }
    }
}

/// A running (or runnable) server. Kill-on-drop lives in the shared
/// `ChildServer`: dropping without a clean `shutdown` SIGKILLs the child so it
/// cannot keep its port hostage.
pub struct Server {
    kind: ServerKind,
    child: ChildServer,
    /// The run dir the server booted in. For Paper this is where the paperclip
    /// materialized the compiled server jar, so `shutdown` can verify the
    /// booted jar's provenance.
    run_dir: PathBuf,
}

impl Server {
    /// The run dir the server booted in. For Paper this is where the paperclip
    /// materialized the compiled server jar, so a caller that needs the
    /// load-bearing provenance check (the Rivet-vs-Paper differential) can
    /// verify the booted jar after shutdown.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }
}

/// Locate the paperclip jar: `RIVET_ORACLE_JAR` env wins, then a copy in
/// `<crate>/work/jars/`, then copy it from `working/Paper` (main checkout).
pub fn ensure_jar(crate_root: &Path) -> Result<PathBuf, Error> {
    if let Ok(p) = std::env::var("RIVET_ORACLE_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(Error::Unverified(format!(
            "RIVET_ORACLE_JAR is set to {} but it is not a file",
            p.display()
        )));
    }
    let local = crate_root.join("work/jars").join(PAPERCLIP_JAR);
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
        println!(
            "    copied {} -> {}",
            from_source.display(),
            local.display()
        );
        return Ok(local);
    }
    Err(Error::Unverified(format!(
        "Paper paperclip jar not found. Looked at {} and {}. \
         Copy it into work/jars/ or set RIVET_ORACLE_JAR.",
        local.display(),
        from_source.display()
    )))
}

/// Read the `Git-Commit: <sha>` attribute from a Paper server jar's
/// `META-INF/MANIFEST.MF` by shelling out to `unzip -p` (the same dependency-
/// minimal approach as `tools/rivet-oracle`'s `read_jar_git_commit`). Returns
/// `None` when the jar has no such attribute (a paperclip wrapper, not a
/// compiled server).
///
/// This mirrors the oracle's helper rather than sharing it: the two binaries
/// classify failures differently (the oracle maps a missing `unzip` to `Gate`;
/// here `classify_commit_lookup_error` maps it to `Unverified`, the scenario
/// harness's UNVERIFIED contract for a missing prerequisite). Extracting a
/// shared helper would couple two standalone binaries' error models for two
/// small functions.
pub fn read_jar_git_commit(jar: &Path) -> io::Result<Option<String>> {
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

/// Verify the server jar a Paper boot actually materialized (the paperclip
/// writes `versions/26.2/paper-26.2.jar` into the run dir) carries the pinned
/// `Git-Commit` attribute. This is the same provenance the oracle gate enforces
/// (`tools/rivet-oracle` `check_pin`): the Paper reference is only meaningful
/// for a Rivet-vs-Paper differential if it is known to be the pinned commit.
/// The check is intentionally read from what actually booted — not from the
/// paperclip jar or any co-located proxy — so a stale, swapped, or
/// unverifiable Paper cannot silently stand in for the reference.
pub fn verify_paper_provenance(run_dir: &Path) -> Result<(), Error> {
    let jar = run_dir.join("versions/26.2/paper-26.2.jar");
    if !jar.is_file() {
        return Err(Error::Unverified(format!(
            "materialized server jar {} missing — the paperclip did not materialize a server",
            jar.display()
        )));
    }
    let actual = match read_jar_git_commit(&jar) {
        Ok(Some(commit)) => commit,
        Ok(None) => {
            return Err(Error::Unverified(format!(
                "materialized server jar {} carries no Git-Commit manifest attribute — \
                 cannot verify the Paper reference is the pinned {PAPER_PIN_COMMIT}",
                jar.display()
            )));
        }
        Err(e) => return Err(classify_commit_lookup_error(e)),
    };
    if actual != PAPER_PIN_COMMIT {
        return Err(Error::Unverified(format!(
            "materialized server jar {} is Git-Commit {actual}, but the scenario's Paper \
             reference is pinned to {PAPER_PIN_COMMIT}. Rebuild the paperclip from the pinned \
             Paper (build working/Paper at {PAPER_PIN_COMMIT}) before running the differential.",
            jar.display()
        )));
    }
    Ok(())
}

/// Classify a `read_jar_git_commit` failure. When the `unzip` binary itself is
/// missing, `Command::spawn` reports `ErrorKind::NotFound`; that is a missing
/// prerequisite — UNVERIFIED, not a hard FAIL — since the differential cannot
/// establish the Paper reference's pin without reading the materialized jar.
/// Any other failure is a genuine IO error.
fn classify_commit_lookup_error(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::NotFound {
        Error::Unverified(
            "cannot verify the Paper reference: `unzip` is not installed (needed to read the \
             materialized server jar's Git-Commit attribute)"
                .to_string(),
        )
    } else {
        Error::Io(e)
    }
}

/// Locate the `rivet-server` binary: `RIVET_SERVER_BIN` env wins, then the
/// sibling binary in Cargo's resolved target directory, then `CARGO_TARGET_DIR`.
/// The wrapper builds both workspaces into one target, so the sibling path is
/// the same for a nested tools workspace and the root workspace.
pub fn ensure_rivet_binary(crate_root: &Path) -> Result<PathBuf, Error> {
    if let Ok(p) = std::env::var("RIVET_SERVER_BIN") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(Error::Unverified(format!(
            "RIVET_SERVER_BIN is set to {} but it is not a file",
            p.display()
        )));
    }
    let workspace_bin = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("rivet-server")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| cargo_target_dir().join("debug/rivet-server"));
    if !workspace_bin.is_file() {
        return Err(Error::Unverified(format!(
            "rivet-server binary not found at {}. Build it (cargo build -p rivet-server from \
             the selected workspace root) or set RIVET_SERVER_BIN.",
            workspace_bin.display()
        )));
    }
    // Provenance: the fallback must be fresh relative to the rivet-server
    // source in the same workspace. Git checkouts stamp changed files with the
    // checkout time, so a binary built for an older commit predates the newer
    // source; refuse it rather than booting the wrong server.
    let src_marker = crate_root.join("../../crates/rivet-server/src/main.rs");
    let src_modified = fs::metadata(&src_marker)
        .ok()
        .and_then(|m| m.modified().ok());
    let bin_modified = fs::metadata(&workspace_bin)?.modified()?;
    if let Some(src_modified) = src_modified
        && bin_modified < src_modified
    {
        return Err(Error::Unverified(format!(
            "rivet-server binary {} is older than its source {} — it is a stale build \
             from a different commit. Rebuild it in this workspace (cargo build -p \
             rivet-server) or point RIVET_SERVER_BIN at the intended binary.",
            workspace_bin.display(),
            src_marker.display()
        )));
    }
    Ok(workspace_bin)
}

/// Rewrite `server-port=` in a run dir's `server.properties` so every boot
/// listens on its own isolated port. The committed fixtures file is never
/// touched — this operates on the copied run-dir file.
fn patch_server_port(properties: &Path, port: u16) -> Result<(), Error> {
    let text = fs::read_to_string(properties)?;
    let mut found = false;
    let mut out = String::with_capacity(text.len() + 8);
    for line in text.lines() {
        if line.trim_start().starts_with("server-port=") {
            found = true;
            out.push_str(&format!("server-port={port}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        return Err(Error::Gate(format!(
            "{} has no server-port= line to patch for port isolation",
            properties.display()
        )));
    }
    fs::write(properties, out)?;
    Ok(())
}

/// Prepare a clean scratch run dir. For Paper, reuses the paperclip-downloaded
/// libraries and vanilla cache so a re-run boots in ~10s instead of ~30s.
/// `versions/` is deliberately NOT preserved: the paperclip re-materializes the
/// server jar from the artifact actually booted on every boot, so
/// `verify_paper_provenance` can never be fooled by a stale jar left by a prior
/// (possibly swapped) artifact. A regular Paper jar booted in place of the
/// bundler never writes `versions/`, so the missing jar fails UNVERIFIED. When
/// a `server.properties` source is provided, copies it (seed 42, superflat,
/// offline) and patches its port to the allocated one, guaranteeing config
/// parity by construction plus port isolation. Rivet boots pass `None`: the
/// rivet-server binary is driven purely by `--host`/`--port` and never reads
/// `server.properties`.
///
/// `world_defaults_src` is the pinned `config/paper-world-defaults.yml` source
/// (issue #266), required for every Paper boot and unused for Rivet. Paper reads
/// the per-category spawn limits from `config/paper-world-defaults.yml` on every
/// boot, and a fresh Paper world *generates* its defaults with the vanilla
/// `spawn-limits` untouched — if the scenario run dir never installs the pinned
/// file, natural spawning re-enables and the sampled walk (and `last_sent`)
/// becomes nondeterministic. The pinned file is therefore installed into the
/// (possibly stale) `config/` dir on every boot, overwriting whatever a prior
/// boot generated or left behind, so the deterministic config is guaranteed by
/// construction (same mechanism as `tools/rivet-capture`).
fn prepare_run_dir(
    run_dir: &Path,
    kind: ServerKind,
    server_properties_src: Option<&Path>,
    world_defaults_src: Option<&Path>,
    port: u16,
) -> Result<(), Error> {
    let libs = run_dir.join("libraries");
    let reuse_libs = kind == ServerKind::Paper
        && libs.is_dir()
        && fs::read_dir(&libs)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);

    if run_dir.exists() {
        if reuse_libs {
            for entry in fs::read_dir(run_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if matches!(name.as_str(), "libraries" | "cache") {
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

    if let Some(src) = server_properties_src {
        let properties = run_dir.join("server.properties");
        fs::copy(src, &properties)?;
        patch_server_port(&properties, port)?;
    }
    if kind == ServerKind::Paper {
        // The deterministic world-defaults are load-bearing for every Paper
        // boot: without the pinned spawn-limits (all seven categories at 0), a
        // fresh world re-enables natural spawning and the sampled walk (and
        // `last_sent`) becomes nondeterministic — the issue #333 failure mode.
        // Requiring the source here (not silently skipping on `None`) makes the
        // invariant enforceable: a future Paper boot site that forgets to
        // resolve the fixture fails UNVERIFIED instead of booting nondeterministic.
        let world_defaults_src = world_defaults_src.ok_or_else(|| {
            Error::Unverified(
                "a Paper boot requires the pinned config/paper-world-defaults.yml source \
                 (issue #266/#333): without it the spawn-limits stay at the vanilla defaults and \
                 the sampled walk is nondeterministic"
                    .to_owned(),
            )
        })?;
        // Install into the server's `config/` dir so Paper merges them into
        // `paper-world-defaults.yml` on boot. Overwrite unconditionally: the
        // run dir may already hold a stale copy (from a prior boot, or
        // Paper-generated defaults with the vanilla spawn limits intact), and
        // only the pinned fixture is deterministic.
        let config_dir = run_dir.join("config");
        fs::create_dir_all(&config_dir)?;
        fs::copy(
            world_defaults_src,
            config_dir.join("paper-world-defaults.yml"),
        )?;
        // Paper refuses to boot without eula=true.
        fs::write(run_dir.join("eula.txt"), EULA)?;
    }
    Ok(())
}

/// The READY marker test for a server kind (used by the shared
/// [`ChildServer::wait_ready`]).
fn ready_test(kind: ServerKind, text: &str) -> bool {
    match kind {
        ServerKind::Paper => text.contains("Done (") && text.contains("For help, type \"help\""),
        ServerKind::Rivet => text.lines().any(|l| l.trim() == RIVET_READY),
    }
}

/// The boot timeout for a server kind.
fn boot_timeout(kind: ServerKind) -> Duration {
    match kind {
        ServerKind::Paper => BOOT_TIMEOUT,
        ServerKind::Rivet => RIVET_BOOT_TIMEOUT,
    }
}

/// The outcome of the world-path launch probe (issue #316).
#[derive(Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// The server accepted `--level <path>` and reached `RIVET_READY`: the
    /// world-path/loading capability is present.
    Present,
    /// The server exited before READY and its log shows it rejected the
    /// world-path argument: the capability is absent.
    Absent { evidence: String },
    /// The server exited before READY for an unrelated reason: the capability
    /// could not be confirmed present or absent. Still UNVERIFIED per the exit
    /// contract (the acceptance did not run), but the reason differs.
    FailedToBoot { evidence: String },
}

/// Classify a world-path launch probe from whether the server reached READY and
/// its log. Pure, so the classification is unit-tested on fixture logs instead
/// of requiring a real boot.
pub fn classify_probe(reached_ready: bool, log: &str) -> ProbeVerdict {
    classify_arg_probe(reached_ready, log, WORLD_PATH_ARG)
}

/// Classify a generated-world launch probe (`--seed <n>`) from whether the
/// server reached READY and its log. Pure and unit-tested like
/// [`classify_probe`]. `Absent` means the server build has no generated-world
/// capability — it exited before READY and its log names `--seed` as unknown.
pub fn classify_seed_probe(reached_ready: bool, log: &str) -> ProbeVerdict {
    classify_arg_probe(reached_ready, log, GENERATED_SEED_ARG)
}

/// The shared launch-capability classifier: a server that reached READY has the
/// capability (`Present`); a server that exited before READY rejecting `arg` as
/// an unknown argument has it `Absent` (the capability is not in this build);
/// any other premature exit is `FailedToBoot` (the capability could not be
/// confirmed present or absent, still UNVERIFIED per the exit contract).
fn classify_arg_probe(reached_ready: bool, log: &str, arg: &str) -> ProbeVerdict {
    if reached_ready {
        return ProbeVerdict::Present;
    }
    let evidence = log
        .lines()
        .find(|l| l.contains("unknown argument") && l.contains(arg))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            log.lines()
                .last()
                .map(str::to_owned)
                .unwrap_or_else(|| "<empty log>".to_owned())
        });
    if log.contains(arg) && log.contains("unknown argument") {
        ProbeVerdict::Absent { evidence }
    } else {
        ProbeVerdict::FailedToBoot { evidence }
    }
}

/// Resolve an artifact path against the harness's current directory when it is
/// relative. The child is spawned with its working directory set to `run_dir`
/// (where the world and `server.properties` live), so a relative artifact path
/// — which `RIVET_ORACLE_JAR` may legitimately carry — would otherwise be
/// resolved against `run_dir` and fail to boot (Paper: `Unable to access
/// jarfile`).
fn absolutize_artifact(artifact: &Path) -> PathBuf {
    if artifact.is_absolute() {
        artifact.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(artifact))
            .unwrap_or_else(|_| artifact.to_path_buf())
    }
}

/// Spawn the server, wait for READY, and return the running server.
///
/// stdout+stderr are teed to `log_path`. `artifact` is the paperclip jar for
/// Paper or the `rivet-server` binary for Rivet. `server_properties_src` is
/// required for Paper and unused for Rivet (pass `None`): the rivet-server
/// binary is driven purely by `--host`/`--port` and never reads
/// `server.properties`. `world_defaults_src` is the pinned
/// `config/paper-world-defaults.yml` source (issue #266), also required for
/// Paper and unused for Rivet (pass `None`). `envs` are extra environment
/// variables for the child (currently only the Rivet-vs-Paper movement
/// differential passes any: it boots the rivet-server with
/// `RIVET_TRACE_MOVEMENT=1` so the tick thread emits its authoritative movement
/// audit on stderr).
///
/// The ten arguments are the distinct inputs a boot needs (server kind, run
/// dir, log tee, artifact, optional properties source, optional world-defaults
/// source, bind address, optional held port reservation, child envs, optional
/// world-path launch option, `seed` the generated-world `--seed` launch option);
/// the excess over clippy's default limit is inherent to the operation rather
/// than a refactorable arity smell.
#[allow(clippy::too_many_arguments)]
pub fn boot(
    kind: ServerKind,
    run_dir: &Path,
    log_path: &Path,
    artifact: &Path,
    server_properties_src: Option<&Path>,
    world_defaults_src: Option<&Path>,
    address: SocketAddr,
    port_reservation: Option<rivet_harness_common::port::PortReservation>,
    envs: &[(&str, &str)],
    world_path: Option<&Path>,
    seed: Option<u64>,
) -> Result<Server, Error> {
    prepare_run_dir(
        run_dir,
        kind,
        server_properties_src,
        world_defaults_src,
        address.port(),
    )?;
    // The child runs with its cwd set to `run_dir`; a relative artifact (e.g.
    // `RIVET_ORACLE_JAR=work/jars/...`) must be resolved against the harness's
    // own cwd, not the run dir, or Java cannot find the jar.
    let artifact = absolutize_artifact(artifact);

    if kind == ServerKind::Paper && world_path.is_some() {
        return Err(Error::Gate(
            "a world-path launch option is a Rivet-only interface; Paper boots its own world \
             from the run dir"
                .into(),
        ));
    }
    let mut command = match kind {
        ServerKind::Paper => {
            let mut c = Command::new("java");
            c.args(["-Xms512M", "-Xmx2G", "-jar"])
                .arg(&artifact)
                .arg("nogui")
                .current_dir(run_dir);
            c
        }
        ServerKind::Rivet => {
            let mut c = Command::new(&artifact);
            c.args([
                "--host",
                &address.ip().to_string(),
                "--port",
                &address.port().to_string(),
            ])
            .current_dir(run_dir);
            // The world-path launch interface (`--level <path>`, issue #316):
            // the narrow seam the loaded-world acceptance probe drives. Since
            // #363, rivet-server accepts the arg and boots against the copied
            // world; a rejection is still surfaced honestly as UNVERIFIED by
            // the probe classifier.
            if let Some(world) = world_path {
                c.arg(WORLD_PATH_ARG).arg(world);
            }
            // The generated-world launch interface (`--seed <n>`): the explicit
            // generated-world capability the generated-world acceptance probe
            // drives. A rivet-server build that accepts it boots a fresh
            // generated world of the requested seed; one that rejects it (the
            // current `--host`/`--port`/`--level`-only build) is classified
            // `Absent` by `classify_seed_probe` and the runner reports the
            // exact pinned UNVERIFIED reason — never a superflat or loaded-world
            // fallback.
            if let Some(seed) = seed {
                c.arg(GENERATED_SEED_ARG).arg(seed.to_string());
            }
            c
        }
    };
    command.envs(envs.iter().copied());
    // A held port reservation (used by the multi-server Paper-vs-Rivet mode) is
    // released only now, immediately before the child process binds the port,
    // so the port cannot be stolen during the (possibly slow) run-dir prep.
    // Single-server modes pass `None` and boot on their base port.
    if let Some(reservation) = port_reservation {
        reservation.release();
    }
    let mut child = ChildServer::spawn(&mut command, log_path).map_err(|e| match kind {
        ServerKind::Paper => Error::Gate(format!(
            "failed to spawn java: {e} (is a Java 25+ JRE on PATH?)"
        )),
        ServerKind::Rivet => Error::Gate(format!(
            "failed to spawn rivet-server ({}): {e} — build it first with: \
             cargo build -p rivet-server (repo root)",
            artifact.display()
        )),
    })?;
    child.wait_ready(
        &kind.to_string(),
        boot_timeout(kind),
        POLL_INTERVAL,
        |text| ready_test(kind, text),
    )?;
    Ok(Server {
        kind,
        child,
        run_dir: run_dir.to_path_buf(),
    })
}

/// SIGTERM the server and wait for its clean shutdown.
///
/// Paper's clean shutdown is the `All dimensions are saved` marker in the
/// post-`Done` log tail. Rivet's clean shutdown is the SIGTERM handler draining
/// and exiting with code 0 (rivet-server/src/main.rs); Rivet persists no world
/// state yet, so the exit status is the load-bearing assertion.
pub fn shutdown(server: &mut Server) -> Result<(), Error> {
    println!("    server ready; shutting down cleanly (SIGTERM)...");
    // Let trailing delayed-init / chunk I/O settle before stopping.
    thread::sleep(Duration::from_millis(1500));
    let status = server.child.shutdown(SHUTDOWN_TIMEOUT, POLL_INTERVAL)?;

    match server.kind {
        ServerKind::Paper => {
            let bytes = fs::read(server.child.log_path())?;
            let done_offset = server.child.ready_offset();
            let tail = if bytes.len() > done_offset {
                String::from_utf8_lossy(&bytes[done_offset..]).into_owned()
            } else {
                String::new()
            };
            if !tail.contains("All dimensions are saved") {
                return Err(Error::Gate(
                    "server shut down without a clean save ('All dimensions are saved' missing \
                     from post-Done log tail)"
                        .into(),
                ));
            }
            // Provenance is verified only where it is load-bearing: the
            // Rivet-vs-Paper differential (run_paper_vs_rivet) requires the
            // Paper reference to be the pinned oracle commit. Paper-vs-Paper
            // self-checks (paper:paper join, move) and capture compare a build
            // against itself, so the pin is not a correctness requirement
            // there. The differential path calls verify_paper_provenance
            // explicitly after this shutdown.
        }
        ServerKind::Rivet => {
            // Clean shutdown is the SIGTERM handler draining and exiting 0
            // (rivet-server/src/main.rs). A nonzero exit after SIGTERM means the
            // server crashed or refused the orderly shutdown.
            if !status.success() {
                return Err(Error::Gate(format!(
                    "rivet-server exited with {status} after SIGTERM (expected a clean exit 0) — \
                     see {}",
                    server.child.log_path().display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    fn test_socket() -> SocketAddr {
        "127.0.0.1:25599".parse().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn non_executable_rivet_binary_is_a_hard_spawn_failure() {
        let base =
            std::env::temp_dir().join(format!("rivet-scenario-nonexec-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let artifact = base.join("rivet-server");
        let run_dir = base.join("run");
        let log = base.join("rivet.log");
        fs::write(&artifact, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&artifact, fs::Permissions::from_mode(0o644)).unwrap();
        // If pre-spawn failures were incorrectly classified from old evidence,
        // this would look like a --level rejection (Absent) instead of a hard
        // spawn failure.
        fs::write(&log, "unknown argument \"--level\"\n").unwrap();

        let error = boot(
            ServerKind::Rivet,
            &run_dir,
            &log,
            &artifact,
            None,
            None,
            test_socket(),
            None,
            &[],
            Some(Path::new("world")),
            None,
        )
        .err()
        .expect("a non-executable binary must not spawn");
        assert!(
            matches!(error, Error::Gate(_)),
            "spawn failure must remain a hard Gate error: {error}"
        );

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn invalid_run_dir_is_a_hard_io_failure_before_spawn() {
        let base =
            std::env::temp_dir().join(format!("rivet-scenario-bad-run-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let run_dir = base.join("run");
        let artifact = base.join("unused-rivet-server");
        let log = base.join("rivet.log");
        fs::write(&run_dir, b"not a directory").unwrap();
        fs::write(&artifact, b"unused").unwrap();
        fs::write(&log, "unknown argument \"--level\"\n").unwrap();

        let error = boot(
            ServerKind::Rivet,
            &run_dir,
            &log,
            &artifact,
            None,
            None,
            test_socket(),
            None,
            &[],
            Some(Path::new("world")),
            None,
        )
        .err()
        .expect("a file cannot be prepared as a run directory");
        assert!(
            matches!(error, Error::Io(_)),
            "run-dir preparation failure must remain a hard Io error: {error}"
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "unknown argument \"--level\"\n",
            "pre-spawn run-dir failure must not inspect or rewrite a stale log"
        );

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn world_path_probe_classifies_ready_absent_and_unrelated_failures() {
        assert_eq!(classify_probe(true, "anything"), ProbeVerdict::Present);

        let rejected = concat!(
            "thread 'main' panicked at crates/rivet-server/src/main.rs:\n",
            "unknown argument \"--level\" (expected --host/--port)\n"
        );
        assert!(matches!(
            classify_probe(false, rejected),
            ProbeVerdict::Absent { evidence }
                if evidence.contains("unknown argument \"--level\"")
        ));

        assert_eq!(
            classify_probe(false, "server error: address already in use\n"),
            ProbeVerdict::FailedToBoot {
                evidence: "server error: address already in use".to_owned()
            }
        );
        assert_eq!(
            classify_probe(false, ""),
            ProbeVerdict::FailedToBoot {
                evidence: "<empty log>".to_owned()
            }
        );
    }

    /// The generated-world launch probe (`--seed`) classifies ready, absent,
    /// and unrelated failures exactly like the world-path probe — the
    /// generated-world acceptance boundary depends on `Absent` meaning "this
    /// rivet-server build has no generated-world capability", never a fallback.
    #[test]
    fn seed_probe_classifies_ready_absent_and_unrelated_failures() {
        assert_eq!(classify_seed_probe(true, "anything"), ProbeVerdict::Present);

        let rejected = concat!(
            "thread 'main' panicked at crates/rivet-server/src/main.rs:\n",
            "unknown argument \"--seed\" (expected --host/--port/--level)\n"
        );
        assert!(matches!(
            classify_seed_probe(false, rejected),
            ProbeVerdict::Absent { evidence }
                if evidence.contains("unknown argument \"--seed\"")
        ));

        // A log that names the world-path argument but not `--seed` must NOT
        // classify as `Absent` for the seed probe: the capability is unconfirmed.
        let wrong_arg = concat!(
            "thread 'main' panicked at crates/rivet-server/src/main.rs:\n",
            "unknown argument \"--level\" (expected --host/--port)\n"
        );
        assert_eq!(
            classify_seed_probe(false, wrong_arg),
            ProbeVerdict::FailedToBoot {
                evidence: "unknown argument \"--level\" (expected --host/--port)".to_owned()
            }
        );

        assert_eq!(
            classify_seed_probe(false, "server error: address already in use\n"),
            ProbeVerdict::FailedToBoot {
                evidence: "server error: address already in use".to_owned()
            }
        );
    }

    /// A relative artifact must resolve against the harness's cwd, not the run
    /// dir the child is spawned with — `RIVET_ORACLE_JAR=work/jars/...` is a
    /// relative path, and without this resolution Java's `-jar` would look in
    /// the run dir and fail with `Unable to access jarfile`.
    #[test]
    fn relative_artifact_resolves_against_harness_cwd() {
        let cwd = std::env::current_dir().expect("harness cwd");
        let resolved = absolutize_artifact(Path::new("work/jars/paper.jar"));
        assert!(resolved.is_absolute(), "must absolutize, got {resolved:?}");
        assert_eq!(resolved, cwd.join("work/jars/paper.jar"));
        // An already-absolute artifact passes through untouched.
        let absolute = PathBuf::from("/tmp/paper.jar");
        assert_eq!(absolutize_artifact(&absolute), absolute);
    }

    /// CRC-32 (IEEE) over `data`, matching what zip tools verify. The test
    /// builds real jar files so `unzip` reads the MANIFEST without a bad-CRC
    /// warning.
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
    /// `dir/paper.jar` and return its path. `commit` is the `Git-Commit`
    /// attribute value, or omitted to emulate a jar with no provenance.
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

    /// A run dir laid out like a Paper boot: the materialized server jar under
    /// `versions/26.2/`. `tag` keeps parallel tests from sharing a directory.
    fn run_dir_with_materialized_jar(tag: &str, commit: Option<&str>) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("rivet-scenario-jar-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let versions = dir.join("versions/26.2");
        fs::create_dir_all(&versions).unwrap();
        let jar = make_jar(&versions, commit);
        fs::rename(&jar, versions.join("paper-26.2.jar")).unwrap();
        (dir, versions.join("paper-26.2.jar"))
    }

    #[test]
    fn read_jar_git_commit_extracts_the_attribute() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-mf-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let jar = make_jar(&dir, Some("0a99345"));
        assert_eq!(
            read_jar_git_commit(&jar).unwrap(),
            Some("0a99345".to_owned())
        );
        // A jar with no Git-Commit attribute reads back None.
        let bare = make_jar(&dir, None);
        assert_eq!(read_jar_git_commit(&bare).unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paper_provenance_accepts_the_pinned_commit() {
        let (dir, _jar) = run_dir_with_materialized_jar("pinned", Some("0a99345"));
        verify_paper_provenance(&dir).expect("pinned jar must verify");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paper_provenance_rejects_a_different_commit() {
        let (dir, _jar) = run_dir_with_materialized_jar("mismatch", Some("deadbeef"));
        let err = verify_paper_provenance(&dir).unwrap_err();
        assert!(
            err.to_string().contains("deadbeef") && err.to_string().contains("0a99345"),
            "must name both the booted and the pinned commit, got {err}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paper_provenance_unverified_when_no_commit_attribute() {
        let (dir, _jar) = run_dir_with_materialized_jar("noattr", None);
        let err = verify_paper_provenance(&dir).unwrap_err();
        assert!(
            matches!(err, Error::Unverified(_)),
            "a jar without provenance must be UNVERIFIED, got {err}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paper_provenance_unverified_when_jar_missing() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-nomf-{}", std::process::id()));
        fs::create_dir_all(dir.join("versions/26.2")).unwrap();
        let err = verify_paper_provenance(&dir).unwrap_err();
        assert!(
            matches!(err, Error::Unverified(_)),
            "a missing materialized jar must be UNVERIFIED, got {err}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paper_provenance_unverified_when_unzip_missing() {
        // `Command::new("unzip")` fails with ErrorKind::NotFound when the binary
        // is absent from PATH; `verify_paper_provenance` routes that through
        // `classify_commit_lookup_error`, which must classify it as UNVERIFIED (a
        // missing prerequisite), not a hard FAIL. Fabricate the io::Error
        // directly rather than mutating PATH (unsafe on this toolchain and racy
        // in multi-threaded test runs).
        let err = classify_commit_lookup_error(io::Error::new(
            io::ErrorKind::NotFound,
            "program not found",
        ));
        assert!(
            matches!(err, Error::Unverified(_)),
            "a missing unzip binary must be UNVERIFIED, got {err}"
        );
        assert!(
            err.to_string().contains("unzip"),
            "must name unzip as the missing prereq, got {err}"
        );
    }

    #[test]
    fn paper_provenance_io_failure_is_a_hard_error() {
        // Any read failure that is not a missing unzip binary stays a genuine
        // IO error, so a corrupt or unreadable jar cannot masquerade as
        // "no provenance" UNVERIFIED.
        let err = classify_commit_lookup_error(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        assert!(
            matches!(err, Error::Io(_)),
            "a non-NotFound read failure must stay an IO error, got {err}"
        );
    }

    #[test]
    fn patch_server_port_replaces_only_the_port_line() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-patch-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let props = dir.join("server.properties");
        fs::write(
            &props,
            "#Minecraft server properties\nserver-port=25599\nlevel-seed=42\n",
        )
        .unwrap();

        patch_server_port(&props, 25598).unwrap();
        let patched = fs::read_to_string(&props).unwrap();
        assert!(patched.contains("server-port=25598"));
        assert!(!patched.contains("server-port=25599"));
        assert!(patched.contains("level-seed=42"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn patch_server_port_errors_when_line_missing() {
        let dir =
            std::env::temp_dir().join(format!("rivet-scenario-patch2-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let props = dir.join("server.properties");
        fs::write(&props, "level-seed=42\n").unwrap();
        let result = patch_server_port(&props, 25598);
        assert!(matches!(result, Err(Error::Gate(_))));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The stale-jar provenance fix: `prepare_run_dir` must wipe `versions/` on
    /// every Paper boot so `verify_paper_provenance` can only see the jar the
    /// artifact actually being booted materialized. A stale jar left by a prior
    /// (possibly swapped) artifact must not survive — otherwise a regular Paper
    /// jar booted in place of the bundler would pass on the old pinned jar.
    /// `libraries/` and `cache/` (the slow downloads) are still preserved.
    #[test]
    fn prepare_run_dir_wipes_stale_versions_but_keeps_download_cache() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-rd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("libraries/org")).unwrap();
        fs::create_dir_all(dir.join("cache")).unwrap();
        fs::create_dir_all(dir.join("versions/26.2")).unwrap();
        fs::write(dir.join("libraries/org/dummy.jar"), b"lib").unwrap();
        fs::write(dir.join("cache/mojang.jar"), b"vanilla").unwrap();
        // A stale materialized jar from a *prior* boot (e.g. the pinned commit).
        let stale = make_jar(&dir.join("versions/26.2"), Some("0a99345"));
        fs::rename(&stale, dir.join("versions/26.2/paper-26.2.jar")).unwrap();
        let src = temp_world_defaults_src("wipe");
        fs::write(&src, b"spawn-limits:\n").unwrap();

        prepare_run_dir(&dir, ServerKind::Paper, None, Some(&src), 25599).unwrap();

        assert!(
            !dir.join("versions").exists(),
            "versions/ must be wiped so a stale jar cannot fool provenance"
        );
        assert!(
            dir.join("libraries").is_dir(),
            "libraries/ must be preserved"
        );
        assert!(dir.join("cache").is_dir(), "cache/ must be preserved");
        assert!(
            dir.join("config/paper-world-defaults.yml").is_file(),
            "the pinned world-defaults must be installed on every Paper boot"
        );
        fs::remove_dir_all(&dir).unwrap();
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    /// A Paper boot with no world-defaults source must fail UNVERIFIED, never
    /// silently boot with the vanilla spawn-limits: without the pinned fixture
    /// the sampled walk is nondeterministic (issue #333), so skipping the
    /// install is the same defect the guard exists to prevent. The message must
    /// name the required fixture.
    #[test]
    fn prepare_run_dir_paper_requires_world_defaults_source() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-wdreq-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let err = prepare_run_dir(&dir, ServerKind::Paper, None, None, 25599).unwrap_err();
        assert!(
            matches!(err, Error::Unverified(_)),
            "a Paper boot without the world-defaults source must be UNVERIFIED, got {err:?}"
        );
        assert!(
            err.to_string().contains("paper-world-defaults.yml"),
            "the Unverified error must name the required fixture, got {err}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The world-defaults source lives OUTSIDE the run dir (the wipe clears the
    /// run dir's non-cache entries before anything is copied in), and its bytes
    /// must land in the server's `config/paper-world-defaults.yml` byte-for-byte
    /// — the deterministic spawn limits (all seven categories at 0, issue #266)
    /// are the load-bearing config and must never be altered in transit.
    fn temp_world_defaults_src(tag: &str) -> PathBuf {
        let src = std::env::temp_dir().join(format!(
            "rivet-scenario-wd-src-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&src);
        fs::create_dir_all(&src).unwrap();
        src.join("paper-world-defaults.yml")
    }

    #[test]
    fn prepare_run_dir_installs_world_defaults_byte_identical() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-wd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = temp_world_defaults_src("install");
        let pinned = b"entities:\n  spawning:\n    spawn-limits:\n      monster: 0\n";
        fs::write(&src, pinned).unwrap();

        prepare_run_dir(&dir, ServerKind::Paper, None, Some(&src), 25599).unwrap();

        let installed = fs::read(dir.join("config/paper-world-defaults.yml")).unwrap();
        assert_eq!(
            installed, pinned,
            "the installed paper-world-defaults.yml must be byte-identical to the pinned \
             fixture — a modified spawn-limits would silently re-enable natural spawning"
        );
        fs::remove_dir_all(&dir).unwrap();
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    /// A stale `config/paper-world-defaults.yml` left by a prior boot (or
    /// generated by Paper's first boot with the vanilla spawn-limits intact)
    /// must be overwritten on every Paper boot — the deterministic spawn limits
    /// are what keep the sampled walk and `last_sent` nondeterministic-free, so
    /// a leftover permissive config must never survive into the next boot.
    #[test]
    fn prepare_run_dir_overwrites_stale_world_defaults() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-wd2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("config")).unwrap();
        // A stale generated/defaults config with natural spawning re-enabled.
        fs::write(
            dir.join("config/paper-world-defaults.yml"),
            "entities:\n  spawning:\n    spawn-limits:\n      monster: -1\n",
        )
        .unwrap();
        let src = temp_world_defaults_src("stale");
        let pinned = b"entities:\n  spawning:\n    spawn-limits:\n      monster: 0\n";
        fs::write(&src, pinned).unwrap();

        prepare_run_dir(&dir, ServerKind::Paper, None, Some(&src), 25599).unwrap();

        let installed = fs::read(dir.join("config/paper-world-defaults.yml")).unwrap();
        assert_eq!(
            installed, pinned,
            "a stale permissive paper-world-defaults.yml must be overwritten by the pinned \
             fixture (spawn-limits all 0) on every boot"
        );
        fs::remove_dir_all(&dir).unwrap();
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    /// Rivet boots pass `None` for the world-defaults source: the rivet-server
    /// binary never reads `config/paper-world-defaults.yml`, so its run dir must
    /// not be populated with Paper's config at all.
    #[test]
    fn prepare_run_dir_rivet_does_not_install_paper_config() {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-wd3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = temp_world_defaults_src("rivet");

        prepare_run_dir(&dir, ServerKind::Rivet, None, Some(&src), 25599).unwrap();

        assert!(
            !dir.join("config/paper-world-defaults.yml").exists(),
            "Rivet boots must not install Paper's config/paper-world-defaults.yml"
        );
        fs::remove_dir_all(&dir).unwrap();
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    /// A `Server` wrapping a long-lived stand-in process (`sleep 60` — a booted
    /// server that would hold its port hostage if leaked). The kill-on-drop and
    /// clean-shutdown behaviors now live in the shared `ChildServer` and are
    /// exercised there with dedicated tests; here we only pin that `Server`
    /// forwards them (dropping without a clean shutdown kills, a clean shutdown
    /// leaves nothing to kill).
    fn sleep_server() -> Server {
        let dir = std::env::temp_dir().join(format!("rivet-scenario-srv-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("boot.log");
        let mut command = Command::new("sleep");
        command.arg("60");
        let child = ChildServer::spawn(&mut command, &log).expect("spawn sleep");
        Server {
            kind: ServerKind::Rivet,
            child,
            run_dir: dir.clone(),
        }
    }

    /// Dropping a `Server` without a clean `shutdown` must kill the underlying
    /// process (a leftover server would hold its port hostage for the next run).
    #[test]
    fn drop_kills_the_child_process() {
        let server = sleep_server();
        let pid = server.child.id();
        drop(server);

        // The process must be gone: kill -0 fails for a dead pid. Stderr is
        // silenced so a "No such process" message does not pollute test output.
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0");
        assert!(!status.success(), "dropped Server must kill its child");
    }

    /// After a clean `shutdown` (child reaped), dropping must not attempt a
    /// kill: the child is already gone, so `kill -0` on its pid fails. `sleep`
    /// dies on SIGTERM, so the shared `shutdown` reaps it cleanly and marks it
    /// stopped — exactly the code path `Server::shutdown` uses.
    #[test]
    fn clean_shutdown_then_drop_does_not_kill_a_reaped_child() {
        let mut server = sleep_server();
        let pid = server.child.id();
        server
            .child
            .shutdown(Duration::from_secs(5), Duration::from_millis(20))
            .expect("clean shutdown");
        drop(server);

        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0");
        assert!(!status.success(), "reaped child must be gone");
    }
}
