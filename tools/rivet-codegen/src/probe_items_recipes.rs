//! `rivet-codegen probe-items-recipes` — re-run the item+recipe extractor
//! against the real Paper 26.2 jar and require byte-identity with the committed
//! `data/items_recipes.json`, plus the anchor counts (issue #186).
//!
//! This is the *live* half of the item+recipe gate: it boots the real
//! `BuiltInRegistries.ITEM` (with holder components bound via
//! `DATA_COMPONENT_INITIALIZERS`) and the `Recipe.CODEC` load of the embedded
//! recipe datapack, and asserts the emitted fixture is byte-identical to a
//! fresh load and that the anchor counts (1537 items, 1585 recipes) reproduce.
//! It guards against a fixture that was hand-edited or generated from a
//! different jar without failing the drift gate.
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
    ("item_count", 1537),
    ("recipe_count", 1585),
    ("recipe_file_count", 1585),
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

    let committed = crate::items_recipes::default_output(&repo_root);
    let scratch = repo_root.join("tools/rivet-codegen/.cache/probe-items-recipes.json");

    // The extractor writes the fresh fixture JSON to the scratch path. Run the
    // full validator on it (structural + report + anchors), assert the probe
    // counts inside it, then byte-compare against the committed one.
    crate::items_recipes::run_extractor(&repo_root, &bundler, &scratch)?;
    crate::items_recipes::validate_items_recipes(&scratch)?;
    let fresh_bytes = fs::read(&scratch).context("read fresh fixture")?;
    check_probe_counts(&fresh_bytes)?;

    let committed_bytes =
        fs::read(&committed).with_context(|| format!("read committed {}", committed.display()))?;
    if fresh_bytes != committed_bytes {
        bail!(
            "probe-items-recipes: a fresh Paper load of data/items_recipes.json differs from the \
             committed fixture ({} bytes fresh vs {} bytes committed) — run \
             `rivet-codegen extract-items-recipes` and commit the result",
            fresh_bytes.len(),
            committed_bytes.len()
        );
    }
    // Validate the committed fixture too: the fresh byte-compare proves
    // determinism, and this proves the committed fixture still matches its
    // provenance manifest (sha256 pin read-verified, not just written).
    crate::items_recipes::validate_items_recipes(&committed)?;
    let _ = fs::remove_file(&scratch);
    println!(
        "Item metadata + canonical recipes verified against live Paper (byte-identical, \
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
            (1537, "item_count"),
            (1585, "recipe_count"),
            (1585, "recipe_file_count"),
        ]))
        .unwrap();
    }

    #[test]
    fn missing_probe_object_fails() {
        let bytes = serde_json::to_vec(&serde_json::json!({"items": {}})).unwrap();
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("missing the `probe` object"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_count_key_fails() {
        let bytes = fixture(&[(1537, "item_count"), (1585, "recipe_count")]);
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("missing `recipe_file_count`"),
            "got: {err}"
        );
    }

    #[test]
    fn wrong_count_fails() {
        let bytes = fixture(&[
            (1536, "item_count"),
            (1585, "recipe_count"),
            (1585, "recipe_file_count"),
        ]);
        let err = check_probe_counts(&bytes).unwrap_err();
        assert!(err.to_string().contains("expected 1537"), "got: {err}");
    }
}
