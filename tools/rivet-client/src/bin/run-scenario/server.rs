//! Paper server boot/shutdown for the scenario runner.
//!
//! Ported from `tools/rivet-oracle/src/main.rs` (the M0 `verify` pattern):
//! boot the paperclip bundler jar headlessly, wait for `Done (...)!`, then
//! SIGTERM and wait for the clean save. Reuses the paperclip-materialized
//! `libraries/`/`versions/`/`cache/` across boots and wipes everything else,
//! so each run is a fresh world at a fixed seed while staying fast on re-runs.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Name of the paperclip bundler jar we boot through.
pub const PAPERCLIP_JAR: &str = "paper-paperclip-26.2.local-SNAPSHOT.jar";

/// How long to wait for the server to reach `Done (...)!` (covers the
/// paperclip first-boot materialization of ~160MB libraries + worldgen).
pub const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
/// How long to wait for a clean shutdown after SIGTERM.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(90);
/// Poll interval while watching the boot log / process exit.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// eula.txt content; the server refuses to boot without `eula=true`.
const EULA: &str = "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\neula=true\n";

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Gate(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
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

/// A running (or runnable) Paper server.
pub struct Server {
    child: Child,
    log_path: PathBuf,
    /// Byte offset in the boot log at the moment `Done` was seen; used to
    /// inspect only the post-Done tail for the clean-save marker.
    done_offset: usize,
    /// Set by `shutdown` after the child has exited; `Drop` then leaves it
    /// alone instead of killing an already-reaped process.
    stopped: bool,
}

/// If a `Server` is dropped without a clean `shutdown` (an error or panic
/// anywhere in the join path), kill the underlying java process so it does not
/// keep port 25599 hostage for the next run.
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
        return Err(Error::Gate(format!(
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
    Err(Error::Gate(format!(
        "Paper paperclip jar not found. Looked at {} and {}. \
         Copy it into work/jars/ or set RIVET_ORACLE_JAR.",
        local.display(),
        from_source.display()
    )))
}

/// Prepare a clean scratch run dir, reusing the paperclip-materialized
/// libraries so a re-run boots in ~10s instead of ~30s.
pub fn prepare_run_dir(run_dir: &Path, server_properties_src: &Path) -> Result<(), Error> {
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

/// Spawn java, wait for `Done`, and return the running server.
///
/// stdout+stderr are teed to `log_path`.
pub fn boot(
    run_dir: &Path,
    log_path: &Path,
    jar: &Path,
    server_properties_src: &Path,
) -> Result<Server, Error> {
    prepare_run_dir(run_dir, server_properties_src)?;

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

    let done_offset = wait_for_done(&mut child, log_path)?;
    Ok(Server {
        child,
        log_path: log_path.to_path_buf(),
        done_offset,
        stopped: false,
    })
}

/// SIGTERM the server and wait for the clean save (`All dimensions are saved`
/// must appear in the post-Done log tail).
pub fn shutdown(server: &mut Server) -> Result<(), Error> {
    let pid = server.child.id();
    println!("    server ready ('Done'); shutting down cleanly (SIGTERM)...");
    // Let trailing delayed-init / chunk I/O settle before stopping.
    thread::sleep(Duration::from_millis(1500));
    let _ = signal_process(pid, "TERM");
    wait_for_exit(&mut server.child, SHUTDOWN_TIMEOUT)?;
    server.stopped = true;

    let bytes = fs::read(&server.log_path)?;
    let tail = if bytes.len() > server.done_offset {
        String::from_utf8_lossy(&bytes[server.done_offset..]).into_owned()
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
