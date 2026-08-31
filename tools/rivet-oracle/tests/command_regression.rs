use std::process::Command;

#[test]
fn cargo_run_without_bin_selects_oracle_binary() {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "rivet-oracle", "--", "--help"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo run should start");

    assert!(
        output.status.success(),
        "cargo run -p rivet-oracle -- --help failed: status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("rivet-oracle"),
        "default binary did not print oracle help: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn generated_full_tamper_without_retained_replay_is_unverified() {
    let output = Command::new("cargo")
        .args([
            "run",
            "-q",
            "-p",
            "rivet-oracle",
            "--",
            "verify-generated-full",
            "--tamper",
            "all",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("generated-full tamper command should start");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing retained replay must be UNVERIFIED/3, status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("latest replay pointer")
            && String::from_utf8_lossy(&output.stderr).contains("absent"),
        "missing retained replay should name the absent pointer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
