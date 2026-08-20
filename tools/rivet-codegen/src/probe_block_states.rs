//! `rivet-codegen probe-block-states` — compile and run a small Java probe
//! (`java/GlobalPaletteProbe.java`) against the real Paper 26.2 jar and
//! cross-check the emitted block-state global-id table (issue #154).
//!
//! The pinned `data/reports/blocks.json` was itself produced by Paper's own
//! `net.minecraft.data.Main --reports`. This probe is the *live* half: it boots
//! the real `Block.BLOCK_STATE_REGISTRY` and hashes a canonical record for every
//! one of its 32,366 states (global id, block name, default marker, and all
//! serialized properties), then requires that digest to match the committed
//! report. Structural invariants and representative anchors remain additional
//! diagnostics (contiguous ranges, defaults in range, air=0,
//! acacia_button=10780, redstone_wire 4011..5306, chest=3987). An unanchored
//! state/property/order drift therefore cannot pass merely because the sparse
//! anchors still match.
//!
//! Requires the same runtime as `extract`/`mth-gen`: the bundler jar
//! (`--bundler`, default `working/Paper`), java + javac on PATH or JAVA_HOME,
//! and unzip.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

/// The representative anchors the emitted table bakes in (from the pinned
/// report; asserted by the codegen conformance test and by the rivet-registry
/// golden-probe test). The Java probe must reproduce each one.
const EXPECTED: &[(&str, &str)] = &[
    ("count", "32366"),
    ("air", "0"),
    ("acacia_button_default", "10780"),
    ("chest_single_north_true", "3987"),
    ("redstone_wire_first", "4011"),
    ("redstone_wire_last", "5306"),
    ("reverse_missing_is_air", "1"),
];

pub fn run(bundler_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let bundler = match bundler_flag {
        Some(p) => p.to_path_buf(),
        None => crate::extract::default_bundler(&repo_root),
    };
    anyhow::ensure!(
        bundler.is_file(),
        "bundler jar not found at {} — pass --bundler or build Paper first",
        bundler.display()
    );

    let (classpath, java, javac) = crate::extract::prepare_runtime(&repo_root, &bundler)?;
    let server_jar = crate::extract::server_jar_for_bundler(&repo_root, &bundler)?;
    crate::reports::verify_fixture_provenance(
        &server_jar,
        &repo_root.join("tools/rivet-codegen/data/reports/manifest.json"),
        &repo_root,
    )?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let helper_dir = cache.join("paletteprobe");
    fs::create_dir_all(&helper_dir).context("create probe helper dir")?;
    let helper_src = include_str!("java/GlobalPaletteProbe.java");
    let helper_file = helper_dir.join("GlobalPaletteProbe.java");
    fs::write(&helper_file, helper_src).context("write GlobalPaletteProbe.java")?;
    let helper_dir_arg =
        crate::extract::path_to_utf8(&helper_dir, "GlobalPaletteProbe output directory")?;
    let helper_file_arg = crate::extract::path_to_utf8(&helper_file, "GlobalPaletteProbe source")?;
    crate::extract::run_cmd(
        &javac,
        &["-cp", &classpath, "-d", helper_dir_arg, helper_file_arg],
        "compile GlobalPaletteProbe.java",
    )?;

    // Quiet log4j down so stdout only carries the probe's key=value lines.
    let log4j_off = cache.join("log4j2-off.xml");
    fs::write(
        &log4j_off,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#,
    )
    .context("write log4j2-off.xml")?;

    let helper_dir_arg =
        crate::extract::path_to_utf8(&helper_dir, "GlobalPaletteProbe classpath directory")?;
    let classpath_arg = format!("{classpath}:{helper_dir_arg}");
    let log4j_arg = format!(
        "-Dlog4j.configurationFile={}",
        crate::extract::path_to_utf8(&log4j_off, "log4j configuration")?
    );
    let out = crate::extract::run_cmd_capture(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "GlobalPaletteProbe",
        ],
        "run GlobalPaletteProbe",
    )?;

    let blocks_path = repo_root.join("tools/rivet-codegen/data/reports/blocks.json");
    let blocks = fs::read_to_string(&blocks_path)
        .with_context(|| format!("read committed {}", blocks_path.display()))?;
    let expected_digest = canonical_state_digest(&blocks)?;
    check_probe_output(&out, &expected_digest)
}

fn check_probe_output(out: &str, expected_digest: &str) -> Result<()> {
    if !out.contains("PROBE OK") {
        bail!(
            "GlobalPaletteProbe did not report PROBE OK — the live Paper block-state\n\
             registry disagrees with the emitted table.\n{out}"
        );
    }
    let mut probes: HashMap<&str, &str> = HashMap::new();
    for line in out.lines() {
        if let Some((key, value)) = line.split_once('=') {
            probes.insert(key.trim(), value.trim());
        }
    }
    for (key, expected) in EXPECTED {
        match probes.get(key) {
            Some(actual) if *actual == *expected => {}
            Some(actual) => {
                bail!("probe `{key}` = {actual} but the emitted table expects {expected}")
            }
            None => bail!("probe `{key}` was not emitted by GlobalPaletteProbe"),
        }
    }
    match probes.get("state_digest_sha256") {
        Some(actual) if *actual == expected_digest => {}
        Some(actual) => bail!(
            "live Paper's complete 32,366-state digest {actual} differs from committed blocks.json {expected_digest}"
        ),
        None => bail!("complete state digest was not emitted by GlobalPaletteProbe"),
    }
    println!(
        "Block-state global-id table verified against live Paper (all 32,366 states + {} anchors match)",
        EXPECTED.len()
    );
    Ok(())
}

/// Canonicalize every state in the committed vanilla `blocks.json` report using
/// the same cross-language record that `GlobalPaletteProbe` hashes:
/// global id, block name, default marker, and lexicographically ordered
/// serialized properties. Ordering the final records by global id makes the
/// digest independent of JSON object insertion order while preserving the live
/// registry's complete observable table.
fn canonical_state_digest(raw: &str) -> Result<String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).context("parse committed blocks.json")?;
    let blocks = root
        .as_object()
        .context("committed blocks.json root must be an object")?;
    let mut records: Vec<Option<String>> = Vec::new();

    for (block_name, block) in blocks {
        let states = block
            .get("states")
            .and_then(serde_json::Value::as_array)
            .with_context(|| format!("block {block_name} has no states array"))?;
        for state in states {
            let id = state
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .with_context(|| format!("block {block_name} has a state without numeric id"))?
                as usize;
            if records.len() <= id {
                records.resize(id + 1, None);
            }
            ensure!(records[id].is_none(), "duplicate block-state id {id}");

            let mut properties = Vec::new();
            if let Some(values) = state
                .get("properties")
                .and_then(serde_json::Value::as_object)
            {
                for (name, value) in values {
                    let value = value.as_str().with_context(|| {
                        format!("block {block_name} state {id} property {name} is not a string")
                    })?;
                    properties.push((name.as_str(), value));
                }
                properties.sort_unstable_by(|left, right| left.0.cmp(right.0));
            }
            let properties = properties
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join(",");
            let is_default = state
                .get("default")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            records[id] = Some(format!(
                "id={id}\tblock={block_name}\tdefault={}\tproperties={properties}\n",
                u8::from(is_default)
            ));
        }
    }

    ensure!(!records.is_empty(), "committed blocks.json has no states");
    let mut canonical = String::new();
    for (id, record) in records.into_iter().enumerate() {
        canonical.push_str(&record.with_context(|| format!("missing block-state id {id}"))?);
    }
    Ok(crate::reports::sha256_hex(canonical.as_bytes()))
}

use std::fs;

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn probe_output(digest: &str) -> String {
        format!(
            "{}\nstate_digest_sha256={digest}\nPROBE OK\n",
            EXPECTED
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    #[test]
    fn matching_output_passes() {
        check_probe_output(&probe_output(TEST_DIGEST), TEST_DIGEST).unwrap();
    }

    #[test]
    fn missing_probe_key_fails() {
        let out = probe_output(TEST_DIGEST).replace("acacia_button_default=10780", "");
        let err = check_probe_output(&out, TEST_DIGEST).unwrap_err();
        assert!(err.to_string().contains("was not emitted"), "got: {err}");
    }

    #[test]
    fn wrong_probe_value_fails() {
        let out = probe_output(TEST_DIGEST).replace("air=0", "air=1");
        let err = check_probe_output(&out, TEST_DIGEST).unwrap_err();
        assert!(err.to_string().contains("expects 0"), "got: {err}");
    }

    #[test]
    fn missing_probe_ok_fails() {
        let out = probe_output(TEST_DIGEST).replace("PROBE OK", "SOMETHING ELSE");
        let err = check_probe_output(&out, TEST_DIGEST).unwrap_err();
        assert!(
            err.to_string().contains("did not report PROBE OK"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_complete_digest_fails() {
        let out =
            probe_output(TEST_DIGEST).replace(&format!("state_digest_sha256={TEST_DIGEST}\n"), "");
        let err = check_probe_output(&out, TEST_DIGEST).unwrap_err();
        assert!(
            err.to_string()
                .contains("complete state digest was not emitted"),
            "got: {err}"
        );
    }

    #[test]
    fn unanchored_state_tamper_changes_complete_digest_and_fails() {
        let committed = r#"{
            "minecraft:air": {"states": [{"id": 0, "default": true}]},
            "minecraft:test": {"states": [
                {"id": 1, "properties": {"axis": "x", "powered": "false"}},
                {"id": 2, "default": true, "properties": {"axis": "y", "powered": "true"}}
            ]}
        }"#;
        let expected = canonical_state_digest(committed).unwrap();
        let tampered = committed.replace("\"axis\": \"x\"", "\"axis\": \"z\"");
        let tampered_digest = canonical_state_digest(&tampered).unwrap();
        assert_ne!(expected, tampered_digest);

        let err = check_probe_output(&probe_output(&tampered_digest), &expected).unwrap_err();
        assert!(
            err.to_string().contains("complete 32,366-state digest"),
            "got: {err}"
        );
    }
}
