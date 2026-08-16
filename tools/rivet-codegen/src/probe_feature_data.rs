//! `rivet-codegen probe-feature-data` — re-run the seed-42 feature-data
//! extractor against the real Paper 26.2 jar and require byte-identity with the
//! committed `data/feature_data.json`, plus the anchor counts (issue seed-42
//! FEATURES checkpoint).
//!
//! This is the *live* half of the feature-data gate: it boots the real
//! `RegistryDataLoader` pipeline and asserts, against the running JVM, that the
//! emitted fixture is byte-identical to a fresh load and that the anchor counts
//! (55 possible biomes, 5 reachable biomes, 203 placed features, 170 configured
//! features, per-biome feature-step totals) reproduce. It guards against a
//! fixture that was hand-edited or generated from a different jar without
//! failing the drift gate.
//!
//! The extractor writes the probe counts into the fixture JSON itself (a
//! `probe` object) rather than stdout, because `Bootstrap.wrapStreams()`
//! redirects `System.out` into the logger and the counts would not reach the
//! captured pipe. The probe reads them back from the fresh fixture.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::extract;

/// The anchor counts a live Paper 26.2 load must reproduce (also asserted by
/// the codegen-side validation, as the `probe` object keys).
const ANCHORS: &[(&str, u64)] = &[
    ("possible_biome_count", 55),
    ("reachable_biome_count", 5),
    ("placed_feature_count", 203),
    ("configured_feature_count", 170),
];

/// Non-vacuity: the reachable biome set must include the deep `lush_caves`
/// biome AND at least one surface biome (a fixture that cannot distinguish a
/// decorated seed-42 chunk from an undecorated one is refused loudly). Shared
/// with the codegen validator so the two halves cannot drift apart.
const REQUIRED_BIOMES: &[&str] = crate::feature_data::REQUIRED_BIOMES;

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

    let committed = crate::extract_feature_data::default_output(&repo_root);
    let scratch = repo_root.join("tools/rivet-codegen/.cache/probe-feature-data.json");

    // The extractor writes the fresh fixture JSON to the scratch path. Assert
    // the probe counts inside it, then byte-compare against the committed one.
    crate::extract_feature_data::run_extractor(&repo_root, &bundler, &scratch)?;
    let server_jar = crate::extract::server_jar_for_bundler(&repo_root, &bundler)?;
    crate::reports::verify_fixture_provenance(
        &server_jar,
        &crate::extract_feature_data::default_output(&repo_root).with_extension("manifest.json"),
        &repo_root,
    )?;
    let fresh_bytes = fs::read(&scratch).context("read fresh fixture")?;
    check_anchors(&fresh_bytes)?;

    let committed_bytes =
        fs::read(&committed).with_context(|| format!("read committed {}", committed.display()))?;
    if fresh_bytes != committed_bytes {
        bail!(
            "probe-feature-data: a fresh Paper load of data/feature_data.json differs from the \
             committed fixture ({} bytes fresh vs {} bytes committed) — run \
             `rivet-codegen extract-feature-data` and commit the result",
            fresh_bytes.len(),
            committed_bytes.len()
        );
    }
    let _ = fs::remove_file(&scratch);
    println!(
        "Seed-42 feature data verified against live Paper (byte-identical, {} bytes, anchors match)",
        committed_bytes.len()
    );
    Ok(())
}

/// The extractor embeds the probe counts in the fixture's `probe` object (the
/// same load the codegen consumes), so a live load that produced a fixture
/// would only pass if the counts match — no separate stdout channel.
fn check_anchors(fresh_bytes: &[u8]) -> Result<()> {
    let fresh: Value =
        serde_json::from_slice(fresh_bytes).context("fresh fixture is not valid JSON")?;
    let probe = fresh
        .get("probe")
        .and_then(Value::as_object)
        .context("fresh fixture is missing the `probe` object")?;
    for (key, expected) in ANCHORS {
        let found = probe
            .get(*key)
            .and_then(Value::as_u64)
            .with_context(|| format!("probe is missing `{key}`"))?;
        if found != *expected {
            bail!(
                "live Paper load reported `{key}={found}`, expected {expected} (a different jar \
                 or a load-order change)"
            );
        }
    }

    // Paper provenance + non-vacuity: the fresh fixture must carry the pinned
    // provenance string and a reachable biome set that includes the deep biome
    // plus a surface biome (decoration evidence).
    match fresh.get("paper").and_then(Value::as_str) {
        Some(p) if p == crate::extract_feature_data::PAPER_PIN => {}
        other => bail!(
            "fresh fixture paper pin `{other:?}` != `{}`",
            crate::extract_feature_data::PAPER_PIN
        ),
    }
    let reachable = fresh
        .get("reachable_biomes")
        .and_then(Value::as_array)
        .context("fresh fixture is missing `reachable_biomes`")?;
    let names: Vec<&str> = reachable.iter().filter_map(Value::as_str).collect();
    for required in REQUIRED_BIOMES {
        if !names.contains(required) {
            bail!(
                "reachable biome set is missing required `{required}` (non-vacuity) — got {names:?}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(reachable: &[&str], counts: &[(&str, u64)]) -> Vec<u8> {
        let mut probe = serde_json::Map::new();
        for (k, v) in counts {
            probe.insert(k.to_string(), serde_json::json!(v));
        }
        serde_json::to_vec(&serde_json::json!({
            "paper": crate::extract_feature_data::PAPER_PIN,
            "reachable_biomes": reachable,
            "probe": probe,
        }))
        .unwrap()
    }

    #[test]
    fn matching_anchors_pass() {
        let bytes = fixture(
            &["minecraft:lush_caves", "minecraft:beach"],
            &[
                ("possible_biome_count", 55),
                ("reachable_biome_count", 5),
                ("placed_feature_count", 203),
                ("configured_feature_count", 170),
            ],
        );
        check_anchors(&bytes).unwrap();
    }

    #[test]
    fn wrong_count_fails() {
        let bytes = fixture(
            &["minecraft:lush_caves", "minecraft:beach"],
            &[
                ("possible_biome_count", 55),
                ("reachable_biome_count", 5),
                ("placed_feature_count", 202),
                ("configured_feature_count", 170),
            ],
        );
        let err = check_anchors(&bytes).unwrap_err();
        assert!(err.to_string().contains("expected 203"), "got: {err}");
    }

    #[test]
    fn wrong_paper_pin_fails() {
        let mut v: Value = serde_json::from_slice(&fixture(
            &["minecraft:lush_caves", "minecraft:beach"],
            &[
                ("possible_biome_count", 55),
                ("reachable_biome_count", 5),
                ("placed_feature_count", 203),
                ("configured_feature_count", 170),
            ],
        ))
        .unwrap();
        v["paper"] = serde_json::json!("26.2-DEV-main@0000000");
        let err = check_anchors(&serde_json::to_vec(&v).unwrap()).unwrap_err();
        assert!(err.to_string().contains("paper pin"), "got: {err}");
    }

    #[test]
    fn missing_reachable_biome_fails_non_vacuity() {
        let bytes = fixture(
            &["minecraft:lush_caves"],
            &[
                ("possible_biome_count", 55),
                ("reachable_biome_count", 5),
                ("placed_feature_count", 203),
                ("configured_feature_count", 170),
            ],
        );
        let err = check_anchors(&bytes).unwrap_err();
        assert!(err.to_string().contains("non-vacuity"), "got: {err}");
    }
}
