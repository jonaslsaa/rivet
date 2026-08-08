//! Shared Paper/Rivet child-process boot lifecycle with kill-on-drop cleanup.
//!
//! The harness tools boot a real server (the Paper paperclip jar via `java`,
//! or the `rivet-server` binary), tee stdout+stderr to a log file, poll the
//! log for a machine-readable READY marker, then SIGTERM and wait for a clean
//! exit. [`ChildServer`] owns that lifecycle: it spawns the child with the log
//! tee installed, waits for READY, shuts it down, and — if dropped without a
//! clean shutdown (an error or panic anywhere in the join path) — SIGKILLs the
//! child so it cannot keep its port hostage for the next run.
//!
//! What stays in the calling tool: *which* command to spawn, the READY marker
//! test (Paper: `Done (...)!` + `For help`; rivet-server: the `RIVET_READY`
//! line), and the post-shutdown clean-save check (Paper: `All dimensions are
//! saved`; rivet-server: exit status 0).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// UNVERIFIED: the child exited or never reached READY within its boot
    /// timeout. Maps to the gate's UNVERIFIED exit code 3.
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

/// Send a signal to `pid` via the POSIX `kill` utility (keeps the crate
/// std-only — no signal crate).
pub fn signal(pid: u32, signal: &str) -> io::Result<()> {
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

/// Spawn `command` with stdout+stderr teed to a fresh (truncated) `log_path`.
/// The caller sets the child's args/current_dir/stdin; this installs the log
/// file as both stdout and stderr.
pub fn spawn_logged(command: &mut Command, log_path: &Path) -> io::Result<Child> {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    let log_err = log_file.try_clone()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err))
        .spawn()
}

/// Poll `child`'s log until `ready` tests true. Returns the byte offset in the
/// log at the moment READY was seen, so the caller can inspect only the
/// post-READY tail for the clean-shutdown marker. Kills the child and returns
/// `Error::Unverified` on timeout or premature exit.
pub fn wait_for_ready(
    child: &mut Child,
    log_path: &Path,
    what: &str,
    timeout: Duration,
    poll_interval: Duration,
    ready: impl Fn(&str) -> bool,
) -> Result<usize, Error> {
    let deadline = Instant::now() + timeout;
    let pid = child.id();
    loop {
        if Instant::now() >= deadline {
            let _ = signal(pid, "KILL");
            let _ = child.wait();
            return Err(Error::Unverified(format!(
                "timed out after {timeout:?} waiting for {what} to reach READY — see {}",
                log_path.display()
            )));
        }
        if let Some(status) = child.try_wait()? {
            return Err(Error::Unverified(format!(
                "{what} process exited ({status}) before reaching READY — see {}",
                log_path.display()
            )));
        }
        if let Ok(text) = fs::read_to_string(log_path)
            && ready(&text)
        {
            return Ok(fs::metadata(log_path)
                .map(|m| m.len() as usize)
                .unwrap_or(text.len()));
        }
        thread::sleep(poll_interval);
    }
}

/// Wait for `child` to exit, SIGKILLing after `timeout`, and return its exit
/// status. The status is load-bearing for rivet-server's clean-shutdown
/// contract (the SIGTERM handler must exit 0).
pub fn wait_for_exit(
    child: &mut Child,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<ExitStatus, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status),
            None => {
                if Instant::now() >= deadline {
                    let _ = signal(child.id(), "KILL");
                    child.wait()?;
                    return Err(Error::Gate(format!(
                        "server did not exit after SIGTERM within {timeout:?}; killed with SIGKILL"
                    )));
                }
                thread::sleep(poll_interval);
            }
        }
    }
}

/// A running server child with the boot lifecycle and kill-on-drop cleanup.
pub struct ChildServer {
    child: Child,
    log_path: PathBuf,
    ready_offset: usize,
    stopped: bool,
}

impl ChildServer {
    /// Spawn the command (which already has its args/current_dir/stdin set)
    /// with the log tee installed.
    pub fn spawn(command: &mut Command, log_path: &Path) -> Result<Self, Error> {
        let child = spawn_logged(command, log_path)?;
        Ok(Self {
            child,
            log_path: log_path.to_path_buf(),
            ready_offset: 0,
            stopped: false,
        })
    }

    /// Poll the log until `ready` tests true (see [`wait_for_ready`]).
    pub fn wait_ready(
        &mut self,
        what: &str,
        timeout: Duration,
        poll_interval: Duration,
        ready: impl Fn(&str) -> bool,
    ) -> Result<(), Error> {
        self.ready_offset = wait_for_ready(
            &mut self.child,
            &self.log_path,
            what,
            timeout,
            poll_interval,
            ready,
        )?;
        Ok(())
    }

    /// SIGTERM the child, wait for exit, and mark it stopped. Returns the exit
    /// status so the caller can run its clean-shutdown assertion (rivet-server
    /// must exit 0). On timeout the child is SIGKILLed and `Error::Gate` is
    /// returned. Either way the child is reaped by [`wait_for_exit`] before
    /// this returns, so `Drop` must not try to kill it again.
    pub fn shutdown(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<ExitStatus, Error> {
        let _ = signal(self.child.id(), "TERM");
        let result = wait_for_exit(&mut self.child, timeout, poll_interval);
        // wait_for_exit reaps the child on every path (clean exit or SIGKILL
        // timeout), so the child is gone regardless of whether it exited 0.
        self.stopped = true;
        result
    }

    /// Byte offset in the log at the moment READY was seen.
    pub fn ready_offset(&self) -> usize {
        self.ready_offset
    }

    /// Path the child's stdout+stderr were teed to.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// The child's pid.
    pub fn id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for ChildServer {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let _ = signal(self.child.id(), "KILL");
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test `ChildServer` wrapping a long-lived stand-in process.
    fn sleep_server() -> ChildServer {
        let mut command = Command::new("sleep");
        command.arg("60");
        let child = spawn_logged(&mut command, Path::new("/dev/null")).expect("spawn sleep");
        ChildServer {
            child,
            log_path: PathBuf::from("/dev/null"),
            ready_offset: 0,
            stopped: false,
        }
    }

    /// Dropping a `ChildServer` without a clean `shutdown` must kill the
    /// underlying process (a leftover server would hold its port hostage for
    /// the next run).
    #[test]
    fn drop_kills_the_child_process() {
        let server = sleep_server();
        let pid = server.id();
        drop(server);

        // The process must be gone: kill -0 fails for a dead pid. Stderr is
        // silenced so a "No such process" message does not pollute test output.
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0");
        assert!(!status.success(), "dropped ChildServer must kill its child");
    }

    /// After a clean `shutdown` (child reaped, `stopped` set), dropping must
    /// not attempt a kill: the child is already gone.
    #[test]
    fn clean_shutdown_then_drop_does_not_kill_a_reaped_child() {
        let mut server = sleep_server();
        let pid = server.id();
        // Reap the child and mark it stopped, exactly what `shutdown` does.
        signal(pid, "KILL").unwrap();
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

    /// A `shutdown` of a process that refuses to exit cleanly must SIGKILL it
    /// after the timeout and report a gate error — the child cannot be left
    /// running.
    #[test]
    fn shutdown_sigkills_a_stuck_child() {
        // A child that ignores SIGTERM (an ignored disposition survives exec):
        // `sh -c 'trap "" TERM; echo READY; exec sleep 60'`. Shutdown's TERM is
        // ignored, so wait_for_exit must SIGKILL after the timeout. The child
        // prints READY only after the trap is installed, so the test cannot
        // race the shutdown against shell startup (a TERM delivered before the
        // trap would kill the shell and yield a clean exit instead).
        let dir = std::env::temp_dir().join(format!("rivet-hc-stuck-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("boot.log");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("trap '' TERM; echo READY; exec sleep 60");
        let child = spawn_logged(&mut command, &log).expect("spawn");
        let pid = child.id();
        let mut server = ChildServer {
            child,
            log_path: log.clone(),
            ready_offset: 0,
            stopped: false,
        };
        server
            .wait_ready(
                "test",
                Duration::from_secs(5),
                Duration::from_millis(20),
                |text| text.contains("READY"),
            )
            .expect("trap installed");
        let err = server.shutdown(Duration::from_millis(1), Duration::from_millis(1));
        assert!(
            matches!(err, Err(Error::Gate(_))),
            "a stuck child must SIGKILL after the timeout, got {err:?}"
        );
        // The SIGKILL in the timeout path reaps the child, so it must be gone.
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0");
        assert!(!status.success(), "a SIGKILLed child must be gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `wait_for_ready` reports UNVERIFIED (not FAIL) when the child exits
    /// before READY — the gate's "oracle could not boot" signal.
    #[test]
    fn wait_ready_premature_exit_is_unverified() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("exit 3");
        let mut child = spawn_logged(&mut command, Path::new("/dev/null")).expect("spawn");
        let result = wait_for_ready(
            &mut child,
            Path::new("/dev/null"),
            "test",
            Duration::from_secs(5),
            Duration::from_millis(10),
            |_| false,
        );
        assert!(
            matches!(result, Err(Error::Unverified(_))),
            "premature exit is UNVERIFIED: {result:?}"
        );
    }

    /// `wait_for_ready` returns the log offset at the moment READY is seen.
    #[test]
    fn wait_ready_returns_offset_when_marker_appears() {
        let dir = std::env::temp_dir().join(format!("rivet-hc-ready-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("boot.log");
        fs::write(&log, "started\n").unwrap();

        // A child that appends the READY marker then keeps running.
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 1; echo READY; sleep 60");
        let mut child = spawn_logged(&mut command, &log).expect("spawn");
        let offset = wait_for_ready(
            &mut child,
            &log,
            "test",
            Duration::from_secs(5),
            Duration::from_millis(20),
            |text| text.contains("READY"),
        )
        .expect("ready seen");
        assert!(offset > 0, "offset at READY must be nonzero");

        let _ = signal(child.id(), "KILL");
        let _ = child.wait();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_logged_tees_stdout_and_stderr() {
        let dir = std::env::temp_dir().join(format!("rivet-hc-spawn-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("out.log");
        let mut command = Command::new("sh");
        command.arg("-c").arg("echo to-stdout; echo to-stderr >&2");
        let mut child = spawn_logged(&mut command, &log).expect("spawn");
        let status = child.wait().expect("wait");
        assert!(status.success());
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.contains("to-stdout"), "stdout must be teed");
        assert!(text.contains("to-stderr"), "stderr must be teed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wait_for_exit_returns_the_exit_status() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("exit 7");
        let mut child = spawn_logged(&mut command, Path::new("/dev/null")).expect("spawn");
        let status = wait_for_exit(
            &mut child,
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .expect("exit");
        assert_eq!(status.code(), Some(7));
    }
}
