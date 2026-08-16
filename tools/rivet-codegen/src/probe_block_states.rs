//! `rivet-codegen probe-block-states` — compile and run a small Java probe
//! (`java/GlobalPaletteProbe.java`) against the real Paper 26.2 jar and
//! cross-check the emitted block-state global-id table (issue #154).
//!
//! The pinned `data/reports/blocks.json` was itself produced by Paper's own
//! `net.minecraft.data.Main --reports`, so the codegen conformance test already
//! re-derives every id from it. This probe is the *live* half: it boots the
//! real `Block.BLOCK_STATE_REGISTRY` and asserts, against the running JVM, the
//! invariants + representative anchors the Rust table bakes in (size 32366,
//! per-block contiguous ranges partitioning 0..32366, defaults in range,
//! air=0 / acacia_button=10780 / redstone_wire 4011..5306 / chest=3987). It
//! guards against a fixture that was hand-edited or generated from a different
//! jar without failing the drift gate.
//!
//! Requires the same runtime as `extract`/`mth-gen`: the bundler jar
//! (`--bundler`, default `working/Paper`), java + javac on PATH or JAVA_HOME,
//! and unzip.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

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

    check_probe_output(&out)
}

fn check_probe_output(out: &str) -> Result<()> {
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
    println!(
        "Block-state global-id table verified against live Paper ({} probes match)",
        EXPECTED.len()
    );
    Ok(())
}

use std::fs;

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_output() -> String {
        format!(
            "{}\nPROBE OK\n",
            EXPECTED
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    #[test]
    fn matching_output_passes() {
        check_probe_output(&probe_output()).unwrap();
    }

    #[test]
    fn missing_probe_key_fails() {
        let out = probe_output().replace("acacia_button_default=10780", "");
        let err = check_probe_output(&out).unwrap_err();
        assert!(err.to_string().contains("was not emitted"), "got: {err}");
    }

    #[test]
    fn wrong_probe_value_fails() {
        let out = probe_output().replace("air=0", "air=1");
        let err = check_probe_output(&out).unwrap_err();
        assert!(err.to_string().contains("expects 0"), "got: {err}");
    }

    #[test]
    fn missing_probe_ok_fails() {
        let out = probe_output().replace("PROBE OK", "SOMETHING ELSE");
        let err = check_probe_output(&out).unwrap_err();
        assert!(
            err.to_string().contains("did not report PROBE OK"),
            "got: {err}"
        );
    }
}
