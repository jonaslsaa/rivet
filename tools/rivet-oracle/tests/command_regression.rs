use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const VALID_COORDINATES: &str = r#"[{"x":0,"z":0},{"x":15,"z":15},{"x":31,"z":31},{"x":-1,"z":-1},{"x":-16,"z":-16},{"x":-31,"z":-31},{"x":-1,"z":0},{"x":0,"z":-1}]"#;
const VALID_NONCE: &str = "abc-1-0";

fn run_generated_full(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rivet-generated-full"))
        .args(args)
        .output()
        .expect("generated-full producer should start")
}

fn valid_producer_args<'a>(config: &'a Path, output: &'a Path) -> Vec<&'a str> {
    vec![
        "--generated-full",
        "--seed",
        "42",
        "--config",
        config.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--coordinates",
        VALID_COORDINATES,
        "--nonce",
        VALID_NONCE,
    ]
}

fn assert_failed(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains(expected), "stderr={stderr}");
    assert!(
        !stderr.contains("RIVET_GENERATED_FULL_BLOCKED:"),
        "ordinary failure must not carry the BLOCKED marker: {stderr}"
    );
}

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

#[test]
fn generated_full_producer_rejects_unknown_and_incomplete_arguments() {
    assert_failed(&run_generated_full(&["--bad"]), "unknown producer argument");
    assert_failed(&run_generated_full(&["--generated-full"]), "usage requires");
    assert_failed(
        &run_generated_full(&["--generated-full", "--seed", "not-u64"]),
        "seed must be an unsigned 64-bit integer",
    );
    assert_failed(
        &run_generated_full(&["--coordinates", "not-json"]),
        "coordinates are invalid JSON",
    );
    assert_failed(
        &run_generated_full(&["--nonce", "../escape"]),
        "does not match",
    );
    assert_failed(
        &run_generated_full(&["--generated-full", "--generated-full"]),
        "duplicate producer argument",
    );
}

#[test]
fn generated_full_producer_requires_nonce_and_exact_coordinate_corpus() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.json");
    fs::write(&config, b"{}\n").unwrap();
    let output = temp.path().join("output");

    let mut missing_both = valid_producer_args(&config, &output);
    missing_both.truncate(7);
    assert_failed(&run_generated_full(&missing_both), "usage requires");

    let mut missing_coordinates = valid_producer_args(&config, &output);
    missing_coordinates.remove(7);
    missing_coordinates.remove(7);
    assert_failed(&run_generated_full(&missing_coordinates), "usage requires");

    let mut missing_nonce = valid_producer_args(&config, &output);
    missing_nonce.truncate(9);
    assert_failed(&run_generated_full(&missing_nonce), "usage requires");

    let mut empty = valid_producer_args(&config, &output);
    empty[8] = "[]";
    assert_failed(&run_generated_full(&empty), "must not be empty");

    let mut malformed_schema = valid_producer_args(&config, &output);
    malformed_schema[8] = r#"[{"x":0}]"#;
    assert_failed(
        &run_generated_full(&malformed_schema),
        "exactly integer x and z",
    );

    let mut duplicate = valid_producer_args(&config, &output);
    duplicate[8] = r#"[{"x":0,"z":0},{"x":0,"z":0}]"#;
    assert_failed(&run_generated_full(&duplicate), "contain duplicate");

    let mut outside_contract = valid_producer_args(&config, &output);
    outside_contract[8] = r#"[{"x":1,"z":1}]"#;
    assert_failed(
        &run_generated_full(&outside_contract),
        "do not match the generated-FULL v1 contract corpus",
    );
}

#[test]
fn generated_full_producer_rejects_missing_and_empty_config() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.json");
    let output = temp.path().join("output");
    assert_failed(
        &run_generated_full(&valid_producer_args(&missing, &output)),
        "cannot be read",
    );

    let empty = temp.path().join("empty.json");
    fs::write(&empty, []).unwrap();
    assert_failed(
        &run_generated_full(&valid_producer_args(&empty, &output)),
        "producer config is empty",
    );

    let malformed = temp.path().join("malformed.json");
    fs::write(&malformed, b"{not-json").unwrap();
    assert_failed(
        &run_generated_full(&valid_producer_args(&malformed, &output)),
        "producer config is invalid JSON",
    );

    let wrong_root = temp.path().join("array.json");
    fs::write(&wrong_root, b"[]").unwrap();
    assert_failed(
        &run_generated_full(&valid_producer_args(&wrong_root, &output)),
        "config root must be a JSON object",
    );
}

#[test]
fn generated_full_producer_rejects_reused_output_root() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.json");
    fs::write(&config, b"{}\n").unwrap();
    let output = temp.path().join("output");
    fs::create_dir(&output).unwrap();
    assert_failed(
        &run_generated_full(&valid_producer_args(&config, &output)),
        "already exists",
    );
}

#[test]
fn generated_full_producer_reserves_exit_four_for_capability_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.json");
    fs::write(&config, b"{}\n").unwrap();
    let output = temp.path().join("output");
    let result = run_generated_full(&valid_producer_args(&config, &output));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(4), "stderr={stderr}");
    assert!(
        stderr.contains("RIVET_GENERATED_FULL_BLOCKED:")
            && stderr.contains("real OverworldGenerator"),
        "stderr={stderr}"
    );
    assert!(
        !output.exists(),
        "blocked producer must not leave partial evidence"
    );
}
