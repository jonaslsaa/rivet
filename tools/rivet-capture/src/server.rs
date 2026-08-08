//! Paper server boot/shutdown for the capture harness.
//!
//! Reuses the shared child-process lifecycle from `rivet-harness-common`
//! (`ChildServer` + `signal`/`wait_for_ready`/`wait_for_exit`); this module
//! owns what is Paper-specific: which command to spawn (`java -jar ... nogui`),
//! the READY marker (`Done (...)!` + `For help`), the run-dir preparation
//! (libraries reuse, spawn-determinism datapack, eula), and the clean-shutdown
//! assertion (`All dimensions are saved` in the post-Done tail).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use rivet_harness_common::server::ChildServer;

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
    /// UNVERIFIED: the server exited or never reached `Done` within its boot
    /// timeout. Maps to the gate's UNVERIFIED exit code 3.
    Unverified(String),
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

/// A running (or runnable) Paper server. Kill-on-drop lives in `ChildServer`:
/// dropping without a clean `shutdown` SIGKILLs the child so it cannot keep
/// its port hostage.
pub struct Server {
    child: ChildServer,
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

/// Install the `rivet-capture` datapack into a fresh world's datapacks dir. It
/// runs `gamerule respawn_radius 0` on world load so the player spawns exactly
/// at the (deterministic) world spawn instead of a per-boot random candidate
/// (PlayerSpawnFinder's `offset` is a fresh ThreadLocalRandom each boot).
fn install_spawn_datapack(run_dir: &Path) -> Result<(), Error> {
    let root = run_dir.join("world/datapacks/rivet-capture");
    let data = root.join("data");
    fs::create_dir_all(data.join("rivet/function"))?;
    fs::create_dir_all(data.join("minecraft/tags/function"))?;
    fs::write(
        root.join("pack.mcmeta"),
        "{\"pack\":{\"pack_format\":107,\"description\":\"rivet-capture: force deterministic player spawn\"}}\n",
    )?;
    fs::write(
        data.join("rivet/function/load.mcfunction"),
        "gamerule respawn_radius 0\n",
    )?;
    fs::write(
        data.join("minecraft/tags/function/load.json"),
        "{\"values\":[\"rivet:load\"]}\n",
    )?;
    Ok(())
}

/// Prepare a clean scratch run dir, reusing the paperclip-materialized
/// libraries so a re-run boots in ~10s instead of ~30s.
pub fn prepare_run_dir(
    run_dir: &Path,
    server_properties_src: &Path,
    world_defaults_src: &Path,
) -> Result<(), Error> {
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
    // The capture's Paper config (all spawn categories capped at 0) lives in
    // the server's config/ dir so Paper merges it into paper-world-defaults.yml.
    let config_dir = run_dir.join("config");
    fs::create_dir_all(&config_dir)?;
    fs::copy(
        world_defaults_src,
        config_dir.join("paper-world-defaults.yml"),
    )?;
    // Pre-place the spawn-determinism datapack in the (not yet generated) world
    // so Paper discovers it on first worldgen and runs it before any join.
    install_spawn_datapack(run_dir)?;
    fs::write(run_dir.join("eula.txt"), EULA)?;
    Ok(())
}

/// Spawn java, wait for `Done`, and return the running server.
pub fn boot(
    run_dir: &Path,
    log_path: &Path,
    jar: &Path,
    server_properties_src: &Path,
    world_defaults_src: &Path,
) -> Result<Server, Error> {
    prepare_run_dir(run_dir, server_properties_src, world_defaults_src)?;

    let mut command = Command::new("java");
    command
        .args(["-Xms512M", "-Xmx2G", "-jar"])
        .arg(jar)
        .arg("nogui")
        .current_dir(run_dir);
    let mut child = ChildServer::spawn(&mut command, log_path).map_err(|e| {
        Error::Gate(format!(
            "failed to spawn java: {e} (is a Java 25+ JRE on PATH?)"
        ))
    })?;
    child.wait_ready("paper server", BOOT_TIMEOUT, POLL_INTERVAL, |text| {
        text.contains("Done (") && text.contains("For help, type \"help\"")
    })?;
    Ok(Server { child })
}

/// SIGTERM the server and wait for the clean save (`All dimensions are saved`
/// must appear in the post-Done log tail).
pub fn shutdown(server: &mut Server) -> Result<(), Error> {
    // Let trailing delayed-init / chunk I/O settle before stopping.
    thread::sleep(Duration::from_millis(1500));
    server.child.shutdown(SHUTDOWN_TIMEOUT, POLL_INTERVAL)?;

    let bytes = fs::read(server.child.log_path())?;
    let done_offset = server.child.ready_offset();
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
