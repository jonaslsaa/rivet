//! `rivet-codegen probe-block-behaviors` — re-run the behavior-table extractor
//! against the real Paper 26.2 jar and require byte-identity with the committed
//! `data/block_behaviors.json`, plus the anchor counts (issue #228).
//!
//! This is the *live* half of the block-behavior gate: it boots the real
//! `Block.BLOCK_STATE_REGISTRY` (evaluating every one of the 32,366 states
//! through its cached accessors) and asserts, against the running JVM, that a
//! fresh dump is byte-identical to the committed fixture and that every anchor
//! the probe documents (state_count 32366, run_count, the representative
//! behavior words, and the representative support/collision face masks) is present with
//! the exact pinned counts. The live probe therefore cannot silently drift in run
//! partitioning, dynamic-state coverage, or fixture coverage; the registry decode
//! tests independently pin the emitted words and masks. Together these guard
//! against a fixture that was hand-edited, generated from a different jar, or
//! emitted by a mis-packed probe.
//!
//! Requires the same runtime as `extract`: the bundler jar (`--bundler`,
//! default `working/Paper`), java + javac on PATH or JAVA_HOME, and unzip.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::extract;

/// The anchor counts a live Paper 26.2 load must reproduce.
const ANCHORS: &[&str] = &[
    "state_count",
    "run_count",
    "face_sturdy_run_count",
    "center_support_run_count",
    "rigid_support_run_count",
    "collision_face_run_count",
    "occlusion_face_run_count",
    "dynamic_shape_state_count",
    "dynamic_fixture_count",
    "air",
    "stone",
    "water",
    "lava",
    "oak_leaves",
    "glass",
    "torch",
    "stone_face_sturdy_mask",
    "oak_slab_face_sturdy_mask",
    "oak_leaves_collision_face_mask",
    "glass_collision_face_mask",
];

pub fn run(bundler_flag: Option<&Path>) -> Result<()> {
    let repo_root = extract::find_repo_root()?;
    let bundler = match bundler_flag {
        Some(p) => p.to_path_buf(),
        None => extract::default_bundler(&repo_root),
    };
    anyhow::ensure!(
        bundler.is_file(),
        "bundler jar not found at {} — pass --bundler or build Paper first",
        bundler.display()
    );

    let committed = crate::extract_block_behaviors::default_output(&repo_root);
    let scratch = repo_root.join("tools/rivet-codegen/.cache/probe-block-behaviors.json");

    let out = crate::extract_block_behaviors::run_extractor(&repo_root, &bundler, &scratch)?;
    let server_jar = crate::extract::server_jar_for_bundler(&repo_root, &bundler)?;
    crate::reports::verify_fixture_provenance(
        &server_jar,
        &crate::extract_block_behaviors::default_output(&repo_root).with_extension("manifest.json"),
        &repo_root,
    )?;
    check_probe_stdout(&out)?;

    let fresh_bytes = fs::read(&scratch).context("read fresh fixture")?;
    let committed_bytes =
        fs::read(&committed).with_context(|| format!("read committed {}", committed.display()))?;
    if fresh_bytes != committed_bytes {
        bail!(
            "probe-block-behaviors: a fresh Paper dump of block_behaviors.json differs from the \
             committed fixture ({} bytes fresh vs {} bytes committed) — run \
             `rivet-codegen extract-block-behaviors` and commit the result",
            fresh_bytes.len(),
            committed_bytes.len()
        );
    }
    let _ = fs::remove_file(&scratch);
    println!(
        "Block behavior table verified against live Paper (byte-identical, {} bytes, anchors match)",
        committed_bytes.len()
    );
    Ok(())
}

fn check_probe_stdout(out: &str) -> Result<()> {
    if !out.contains("PROBE OK") {
        bail!(
            "BlockBehaviourProbe did not report PROBE OK — the live Paper state \
             behaviors disagree with the probe's invariants.\n{out}"
        );
    }
    let probes: std::collections::HashMap<&str, &str> = out
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    for anchor in ANCHORS {
        if !probes.contains_key(*anchor) {
            bail!("probe anchor `{anchor}` was not emitted by BlockBehaviourProbe");
        }
    }
    if probes.get("state_count") != Some(&"32366") {
        bail!(
            "probe state_count = {:?} but the emitted table expects 32366",
            probes.get("state_count")
        );
    }
    if probes.get("face_sturdy_run_count") != Some(&"3504") {
        bail!(
            "probe face_sturdy_run_count = {:?} but the pinned Paper table expects 3504",
            probes.get("face_sturdy_run_count")
        );
    }
    for (key, expected) in [
        ("face_sturdy_run_count", "3504"),
        ("center_support_run_count", "12277"),
        ("rigid_support_run_count", "3504"),
        ("collision_face_run_count", "3506"),
        ("occlusion_face_run_count", "2509"),
        ("dynamic_shape_state_count", "199"),
        ("dynamic_fixture_count", "4"),
    ] {
        if probes.get(key) != Some(&expected) {
            bail!(
                "probe {key} = {:?} but the pinned Paper table expects {expected}",
                probes.get(key)
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_output() -> String {
        format!(
            "{}\nPROBE OK\n",
            ANCHORS
                .iter()
                .map(|k| {
                    // Count anchors are pinned by the checker; the other
                    // anchors are opaque representative values.
                    match *k {
                        "state_count" => format!("{k}=32366"),
                        "face_sturdy_run_count" => format!("{k}=3504"),
                        "center_support_run_count" => format!("{k}=12277"),
                        "rigid_support_run_count" => format!("{k}=3504"),
                        "collision_face_run_count" => format!("{k}=3506"),
                        "occlusion_face_run_count" => format!("{k}=2509"),
                        "dynamic_shape_state_count" => format!("{k}=199"),
                        "dynamic_fixture_count" => format!("{k}=4"),
                        _ => format!("{k}=1"),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    #[test]
    fn matching_output_passes() {
        check_probe_stdout(&probe_output()).unwrap();
    }

    #[test]
    fn missing_probe_key_fails() {
        let out = probe_output().replace("torch=1", "");
        let err = check_probe_stdout(&out).unwrap_err();
        assert!(err.to_string().contains("was not emitted"), "got: {err}");
    }

    #[test]
    fn wrong_state_count_fails() {
        let out = probe_output().replace("state_count=32366", "state_count=32365");
        let err = check_probe_stdout(&out).unwrap_err();
        assert!(err.to_string().contains("expects 32366"), "got: {err}");
    }

    #[test]
    fn missing_probe_ok_fails() {
        let out = probe_output().replace("PROBE OK", "SOMETHING ELSE");
        let err = check_probe_stdout(&out).unwrap_err();
        assert!(
            err.to_string().contains("did not report PROBE OK"),
            "got: {err}"
        );
    }
}
