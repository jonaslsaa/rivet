//! CLI integration tests for the `rivet-server` binary (issue #533):
//! `--help`/`-h`, `--version`/`-V`, and non-panicking invalid-argument handling.

use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rivet-server"))
        .args(args)
        .output()
        .expect("run rivet-server")
}

#[test]
fn help_flags_print_usage_and_exit_zero() {
    for flag in ["--help", "-h"] {
        let output = run_cli(&[flag]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "{flag} exited with {:?}; stderr={stderr}",
            output.status
        );
        assert!(stderr.is_empty(), "{flag} wrote to stderr: {stderr}");
        assert!(
            !stdout.contains("RIVET_READY"),
            "{flag} must not emit RIVET_READY: {stdout}"
        );
        assert!(stdout.contains("Usage: rivet-server"), "{stdout}");
        for option in [
            "--host",
            "--port",
            "--level",
            "--seed",
            "--help",
            "--version",
        ] {
            assert!(
                stdout.contains(option),
                "{flag} usage missing {option}: {stdout}"
            );
        }
    }
}

#[test]
fn version_flags_print_rivet_and_minecraft_versions_and_exit_zero() {
    for flag in ["--version", "-V"] {
        let output = run_cli(&[flag]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "{flag} exited with {:?}; stderr={stderr}",
            output.status
        );
        assert!(stderr.is_empty(), "{flag} wrote to stderr: {stderr}");
        assert!(
            !stdout.contains("RIVET_READY"),
            "{flag} must not emit RIVET_READY: {stdout}"
        );
        assert!(
            stdout.contains(env!("CARGO_PKG_VERSION")),
            "{flag} missing Rivet version: {stdout}"
        );
        assert!(
            stdout.contains(rivet_core::VERSION_NAME),
            "{flag} missing Minecraft version: {stdout}"
        );
    }
}

#[test]
fn unknown_argument_exits_nonzero_without_panicking() {
    for arg in ["--unknown-flag", "unexpected-positional"] {
        let output = run_cli(&[arg]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            !output.status.success(),
            "{arg} should fail, got {:?}",
            output.status
        );
        assert!(
            stdout.is_empty(),
            "{arg} must not write to stdout: {stdout}"
        );
        assert!(
            stderr.contains("unknown argument") && stderr.contains(arg),
            "unexpected stderr for {arg}: {stderr}"
        );
        assert!(
            !stderr.contains("panicked"),
            "{arg} panicked instead of returning a CLI error: {stderr}"
        );
    }
}

#[test]
fn invalid_option_values_and_missing_values_exit_nonzero_without_panicking() {
    let cases: &[(&[&str], &str)] = &[
        (&["--host"], "--host requires a value"),
        (&["--host", "not-an-ip"], "invalid --host"),
        (&["--port"], "--port requires a value"),
        (&["--port", "99999"], "invalid --port"),
        (
            &["--level"],
            "--level requires a disposable world directory path",
        ),
        (&["--seed"], "--seed requires a value"),
        (&["--seed", "not-a-seed"], "invalid --seed"),
    ];

    for (args, expected) in cases {
        let output = run_cli(args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            !output.status.success(),
            "{args:?} should fail, got {:?}",
            output.status
        );
        assert!(
            stdout.is_empty(),
            "{args:?} must not write to stdout: {stdout}"
        );
        assert!(
            stderr.contains(expected),
            "{args:?} stderr missing {expected:?}: {stderr}"
        );
        assert!(
            !stderr.contains("panicked"),
            "{args:?} panicked instead of returning a CLI error: {stderr}"
        );
    }
}
