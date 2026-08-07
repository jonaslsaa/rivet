//! `rivet-codegen probe-biomes-tags` — re-run the biome+tag extractor against
//! the real Paper 26.2 jar and require byte-identity with the committed
//! `data/biomes_tags.json`, plus the anchor counts (issue #49).
//!
//! This is the *live* half of the biome+tag gate: it boots the real
//! `RegistryDataLoader`/`TagNetworkSerialization` pipeline and asserts, against
//! the running JVM, that the emitted fixture is byte-identical to a fresh load
//! and that the anchor counts (66 biomes, 15 tag-carrying registries, 697 tags)
//! reproduce. It guards against a fixture that was hand-edited or generated from
//! a different jar without failing the drift gate.
//!
//! The extractor writes the probe counts into the fixture JSON itself (a
//! `probe` object) rather than stdout, because `Bootstrap.wrapStreams()`
//! redirects `System.out` into the logger and the counts would not reach the
//! captured pipe. The probe reads them back from the fresh fixture.
//!
//! Requires the same runtime as `extract`: the bundler jar (`--bundler`,
//! default `working/Paper`), java + javac on PATH or JAVA_HOME, and unzip.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::extract;

/// The anchor counts a live Paper 26.2 load must reproduce (also asserted by
/// the codegen against the fixture, as `probe` object keys).
const ANCHORS: &[(&str, usize)] = &[
    ("biome_count", 66),
    ("tag_registry_count", 15),
    ("tag_count", 697),
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

    let committed = crate::extract_biomes_tags::default_output(&repo_root);
    let scratch = repo_root.join("tools/rivet-codegen/.cache/probe-biomes-tags.json");

    // The extractor writes the fresh fixture JSON to the scratch path. Assert
    // the probe counts inside it, then byte-compare against the committed one.
    crate::extract_biomes_tags::run_extractor(&repo_root, &bundler, &scratch)?;
    let fresh_bytes = fs::read(&scratch).context("read fresh fixture")?;
    check_probe_counts(&fresh_bytes)?;

    let committed_bytes =
        fs::read(&committed).with_context(|| format!("read committed {}", committed.display()))?;
    if fresh_bytes != committed_bytes {
        bail!(
            "probe-biomes-tags: a fresh Paper load of data/biomes_tags.json differs from the \
             committed fixture ({} bytes fresh vs {} bytes committed) — run \
             `rivet-codegen extract-biomes-tags` and commit the result",
            fresh_bytes.len(),
            committed_bytes.len()
        );
    }
    let _ = fs::remove_file(&scratch);
    println!(
        "Biome id table + tag network content verified against live Paper (byte-identical, \
         {} bytes, anchors match)",
        committed_bytes.len()
    );
    Ok(())
}

/// The extractor embeds the probe counts in the fixture's `probe` object (the
/// same load the codegen consumes), so a live load that produced a fixture
/// would only pass if the counts match — no separate stdout channel.
fn check_probe_counts(fresh_bytes: &[u8]) -> Result<()> {
    let fresh = serde_json::from_slice::<serde_json::Value>(fresh_bytes)
        .context("fresh fixture is not valid JSON")?;
    let probe = fresh
        .get("probe")
        .and_then(serde_json::Value::as_object)
        .context("fresh fixture is missing the `probe` object")?;
    for (key, expected) in ANCHORS {
        let found = probe
            .get(*key)
            .and_then(serde_json::Value::as_u64)
            .with_context(|| format!("probe is missing `{key}`"))?;
        if found != *expected as u64 {
            bail!(
                "live Paper load reported `{key}={found}`, expected {expected} (a different jar \
                 or a load-order change)"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(counts: &[(u64, &str)]) -> Vec<u8> {
        let mut obj = serde_json::Map::new();
        let mut probe = serde_json::Map::new();
        for (v, k) in counts {
            probe.insert(k.to_string(), serde_json::json!(v));
        }
        obj.insert("probe".to_string(), serde_json::Value::Object(probe));
        serde_json::to_vec(&serde_json::Value::Object(obj)).unwrap()
    }

    #[test]
    fn matching_counts_pass() {
        check_probe_counts(&fixture(&[
            (66, "biome_count"),
            (15, "tag_registry_count"),
            (697, "tag_count"),
        ]))
        .unwrap();
    }

    #[test]
    fn missing_probe_object_fails() {
        let bytes = serde_json::to_vec(&serde_json::json!({"biomes": {}})).unwrap();
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("missing the `probe` object"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_count_key_fails() {
        let bytes = fixture(&[(66, "biome_count"), (15, "tag_registry_count")]);
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("missing `tag_count`"),
            "got: {err}"
        );
    }

    #[test]
    fn wrong_count_fails() {
        let bytes = fixture(&[
            (65, "biome_count"),
            (15, "tag_registry_count"),
            (697, "tag_count"),
        ]);
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(err.to_string().contains("expected 66"), "got: {err}");
    }
}
