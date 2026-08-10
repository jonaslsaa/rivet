//! End-to-end headless boot test (issue #155): spawn the `rivet-server` binary
//! on an ephemeral port, wait for the machine-readable `RIVET_READY` marker on
//! stdout, SIGTERM it, and require a clean exit (code 0). This is the contract
//! the scenario runner (`tools/rivet-client`) relies on: headless boot, a hard
//! readiness gate, and deterministic SIGTERM shutdown.

use std::fs;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The exact stdout marker `rivet-server/src/main.rs` prints once bound.
const RIVET_READY: &str = "RIVET_READY";
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn signal(pid: u32, sig: &str) {
    let status = Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .status()
        .expect("run kill");
    assert!(status.success(), "kill -{sig} {pid} failed");
}

/// Reserve an ephemeral loopback port by binding and dropping the listener.
fn ephemeral_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
    listener.local_addr().expect("local addr").port()
}

/// Read lines from the child's stdout until `RIVET_READY` (trimmed, exact) or
/// EOF or timeout.
fn wait_for_ready(child: &mut Child) -> bool {
    let stdout = child.stdout.take().expect("rivet-server stdout is piped");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return false, // EOF before the marker
            Ok(_) => {
                if line.trim() == RIVET_READY {
                    return true;
                }
            }
            Err(_) => return false,
        }
    }
    false
}

/// Wait for the child to exit (bounded); SIGKILL + panic on timeout.
fn wait_for_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("rivet-server did not exit after SIGTERM within {SHUTDOWN_TIMEOUT:?}");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn boots_headless_reaches_ready_and_shuts_down_cleanly_on_sigterm() {
    let port = ephemeral_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rivet-server"))
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rivet-server");

    assert!(
        wait_for_ready(&mut child),
        "rivet-server did not print {RIVET_READY} within {READY_TIMEOUT:?}"
    );

    let pid = child.id();
    signal(pid, "TERM");
    let status = wait_for_exit(&mut child);
    assert!(
        status.success(),
        "expected a clean exit (code 0) after SIGTERM, got {status}"
    );
}

#[test]
fn ready_marker_never_appears_when_the_binary_cannot_bind() {
    // Occupy a port, then point rivet-server at it: bind fails, so no RIVET_READY
    // and a nonzero exit. This proves the marker is load-bearing (printed only
    // after a successful bind), not a startup banner.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy a port");
    let port = listener.local_addr().expect("local addr").port();

    let mut child = Command::new(env!("CARGO_BIN_EXE_rivet-server"))
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rivet-server");

    assert!(
        !wait_for_ready(&mut child),
        "RIVET_READY must not appear when bind fails"
    );
    let status = wait_for_exit(&mut child);
    assert!(
        !status.success(),
        "a failed bind must exit nonzero, got {status}"
    );
}

#[test]
fn disposable_level_fails_visible_before_ready_at_current_codec_boundary() {
    let level = tempfile::tempdir().expect("temp level");
    fs::write(level.path().join("level.dat"), b"copied-level").expect("level.dat");
    fs::create_dir_all(level.path().join("dimensions/minecraft/overworld/region"))
        .expect("overworld region");

    let output = Command::new(env!("CARGO_BIN_EXE_rivet-server"))
        .arg("--level")
        .arg(level.path())
        .output()
        .expect("spawn rivet-server");

    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(RIVET_READY));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RIVET_WORLD_UNVERIFIED"), "{stderr}");
    assert!(stderr.contains("level.dat codecs"), "{stderr}");
    assert!(stderr.contains("#323"), "{stderr}");
}
