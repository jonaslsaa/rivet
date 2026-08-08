//! Server boot/shutdown for the scenario runner (issue #155).
//!
//! `ServerKind::Paper` boots the paperclip bundler jar headlessly (the `verify`
//! pattern from `tools/rivet-oracle`), waits for `Done (...)!`, then SIGTERM and
//! waits for the clean save. `ServerKind::Rivet` boots the `rivet-server`
//! binary headlessly, waits for the machine-readable `RIVET_READY` marker on
//! stdout (rivet-server/src/main.rs), then SIGTERM and waits for a clean exit
//! (code 0).
//!
//! Paper boots reuse the paperclip-downloaded `libraries/` and `cache/` (the
//! slow ~160MB downloads) and wipe everything else, so each run is a fresh world
//! at a fixed seed while staying fast on re-runs. `versions/` is deliberately
//! NOT preserved: the paperclip re-materializes the server jar on every boot, so
//! `verify_paper_provenance` can only ever see the jar the artifact actually
//! being booted produced — a swapped, stale, or non-bundler artifact cannot
//! silently stand in for the pinned reference. Every boot gets its own port
//! (`reserve_ports` in main.rs): the Paper run dir's `server.properties` is
//! patched to the allocated port and `rivet-server` is passed `--host`/`--port`,
//! so no two servers in a scenario can collide.

use std::fmt;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

/// A running (or runnable) server.
pub struct Server {
    kind: ServerKind,
    child: Child,
    log_path: PathBuf,
    /// The run dir the server booted in. For Paper this is where the paperclip
    /// materialized the compiled server jar, so `shutdown` can verify the
    /// booted jar's provenance.
    run_dir: PathBuf,
    /// Byte offset in the boot log at the moment READY was seen; used to
    /// inspect only the post-READY tail for the clean-save/clean-exit marker.
    ready_offset: usize,
    /// Set by `shutdown` after the child has exited; `Drop` then leaves it
    /// alone instead of killing an already-reaped process.
    stopped: bool,
}

/// If a `Server` is dropped without a clean `shutdown` (an error or panic
/// anywhere in the join path), kill the underlying process so it does not keep
/// its port hostage for the next run.
impl Drop for Server {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let _ = signal_process(self.child.id(), "KILL");
        let _ = self.child.wait();
    }
}

/// Send a signal to `pid` via the POSIX `kill` utility (no signal crate).
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
    let actual = read_jar_git_commit(&jar)?.ok_or_else(|| {
        Error::Unverified(format!(
            "materialized server jar {} carries no Git-Commit manifest attribute — \
                 cannot verify the Paper reference is the pinned {PAPER_PIN_COMMIT}",
            jar.display()
        ))
    })?;
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

/// Locate the `rivet-server` binary: `RIVET_SERVER_BIN` env wins, then the
/// workspace this harness was built in (`<workspace>/target/debug/rivet-server`).
///
/// The fallback resolves against the harness's own manifest dir, so a harness
/// built inside a worktree picks up that worktree's own server build — not a
/// stale build from a different checkout. Provenance is load-bearing: the
/// fallback is refused (UNVERIFIED) when the binary is older than the
/// rivet-server source in the same workspace, so a stale root `target` from
/// another commit cannot be silently mistaken for the selected tree's server.
/// `RIVET_SERVER_BIN` remains the explicit override when the server under test
/// lives in a different tree than the harness.
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
    let workspace_bin = crate_root.join("../../target/debug/rivet-server");
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
fn prepare_run_dir(
    run_dir: &Path,
    kind: ServerKind,
    server_properties_src: Option<&Path>,
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
        // Paper refuses to boot without eula=true.
        fs::write(run_dir.join("eula.txt"), EULA)?;
    }
    Ok(())
}

/// Poll the boot log until the server reaches its machine-readable READY
/// marker. Returns the byte offset at READY so the caller can inspect only the
/// post-READY tail for the clean-shutdown marker. Kills the child and returns
/// UNVERIFIED on timeout or premature exit.
fn wait_for_ready(kind: ServerKind, child: &mut Child, log_path: &Path) -> Result<usize, Error> {
    let timeout = match kind {
        ServerKind::Paper => BOOT_TIMEOUT,
        ServerKind::Rivet => RIVET_BOOT_TIMEOUT,
    };
    let deadline = Instant::now() + timeout;
    let pid = child.id();
    loop {
        if Instant::now() >= deadline {
            let _ = signal_process(pid, "KILL");
            let _ = child.wait();
            return Err(Error::Unverified(format!(
                "timed out after {timeout:?} waiting for {kind} to reach READY — see {}",
                log_path.display()
            )));
        }
        if let Some(status) = child.try_wait()? {
            return Err(Error::Unverified(format!(
                "{kind} process exited ({status}) before reaching READY — see {}",
                log_path.display()
            )));
        }
        if let Ok(text) = fs::read_to_string(log_path) {
            let ready = match kind {
                ServerKind::Paper => {
                    text.contains("Done (") && text.contains("For help, type \"help\"")
                }
                ServerKind::Rivet => text.lines().any(|l| l.trim() == RIVET_READY),
            };
            if ready {
                let offset = fs::metadata(log_path)
                    .map(|m| m.len() as usize)
                    .unwrap_or(text.len());
                return Ok(offset);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Wait for `child` to exit, SIGKILLing after `timeout`, and return its exit
/// status. The status is load-bearing for Rivet's clean-shutdown contract (the
/// SIGTERM handler must exit 0).
fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status),
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

/// Spawn the server, wait for READY, and return the running server.
///
/// stdout+stderr are teed to `log_path`. `artifact` is the paperclip jar for
/// Paper or the `rivet-server` binary for Rivet. `server_properties_src` is
/// required for Paper and unused for Rivet (pass `None`): the rivet-server
/// binary is driven purely by `--host`/`--port` and never reads
/// `server.properties`.
pub fn boot(
    kind: ServerKind,
    run_dir: &Path,
    log_path: &Path,
    artifact: &Path,
    server_properties_src: Option<&Path>,
    address: SocketAddr,
) -> Result<Server, Error> {
    prepare_run_dir(run_dir, kind, server_properties_src, address.port())?;

    let log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    let log_err = log_file.try_clone()?;

    let mut child = match kind {
        ServerKind::Paper => Command::new("java")
            .args(["-Xms512M", "-Xmx2G", "-jar"])
            .arg(artifact)
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
            })?,
        ServerKind::Rivet => Command::new(artifact)
            .args([
                "--host",
                &address.ip().to_string(),
                "--port",
                &address.port().to_string(),
            ])
            .current_dir(run_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| {
                Error::Gate(format!(
                    "failed to spawn rivet-server ({}): {e} — build it first with: \
                     cargo build -p rivet-server (repo root)",
                    artifact.display()
                ))
            })?,
    };

    let ready_offset = wait_for_ready(kind, &mut child, log_path)?;
    Ok(Server {
        kind,
        child,
        log_path: log_path.to_path_buf(),
        run_dir: run_dir.to_path_buf(),
        ready_offset,
        stopped: false,
    })
}

/// SIGTERM the server and wait for its clean shutdown.
///
/// Paper's clean shutdown is the `All dimensions are saved` marker in the
/// post-`Done` log tail. Rivet's clean shutdown is the SIGTERM handler draining
/// and exiting with code 0 (rivet-server/src/main.rs); Rivet persists no world
/// state yet, so the exit status is the load-bearing assertion.
pub fn shutdown(server: &mut Server) -> Result<(), Error> {
    let pid = server.child.id();
    println!("    server ready; shutting down cleanly (SIGTERM)...");
    // Let trailing delayed-init / chunk I/O settle before stopping.
    thread::sleep(Duration::from_millis(1500));
    let _ = signal_process(pid, "TERM");
    let status = wait_for_exit(&mut server.child, SHUTDOWN_TIMEOUT)?;
    server.stopped = true;

    match server.kind {
        ServerKind::Paper => {
            let bytes = fs::read(&server.log_path)?;
            let tail = if bytes.len() > server.ready_offset {
                String::from_utf8_lossy(&bytes[server.ready_offset..]).into_owned()
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
            // Provenance: the server jar this boot actually materialized and
            // loaded must be the pinned Paper commit. Read from the run dir (what
            // really booted), not the paperclip jar or a co-located proxy.
            verify_paper_provenance(&server.run_dir)?;
        }
        ServerKind::Rivet => {
            // Clean shutdown is the SIGTERM handler draining and exiting 0
            // (rivet-server/src/main.rs). A nonzero exit after SIGTERM means the
            // server crashed or refused the orderly shutdown.
            if !status.success() {
                return Err(Error::Gate(format!(
                    "rivet-server exited with {status} after SIGTERM (expected a clean exit 0) — \
                     see {}",
                    server.log_path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        prepare_run_dir(&dir, ServerKind::Paper, None, 25599).unwrap();

        assert!(
            !dir.join("versions").exists(),
            "versions/ must be wiped so a stale jar cannot fool provenance"
        );
        assert!(
            dir.join("libraries").is_dir(),
            "libraries/ must be preserved"
        );
        assert!(dir.join("cache").is_dir(), "cache/ must be preserved");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Dropping a `Server` without a clean `shutdown` must kill the underlying
    /// process (a leftover server would hold its port hostage for the next run).
    #[test]
    fn drop_kills_the_child_process() {
        // `sleep 60` is a stand-in for a booted server: a long-lived process we
        // can re-parent into a `Server` and then drop.
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let server = Server {
            kind: ServerKind::Rivet,
            child,
            log_path: PathBuf::from("/dev/null"),
            run_dir: PathBuf::from("/nonexistent"),
            ready_offset: 0,
            stopped: false,
        };
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

    /// After a clean `shutdown` (child reaped, `stopped` set), dropping must not
    /// attempt a kill: the child is already gone, so `kill -0` on its pid fails.
    #[test]
    fn clean_shutdown_then_drop_does_not_kill_a_reaped_child() {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let mut server = Server {
            kind: ServerKind::Rivet,
            child,
            log_path: PathBuf::from("/dev/null"),
            run_dir: PathBuf::from("/nonexistent"),
            ready_offset: 0,
            stopped: false,
        };
        // Reap the child and mark it stopped, exactly what `shutdown` does.
        signal_process(pid, "KILL").unwrap();
        server.child.wait().unwrap();
        server.stopped = true;
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
