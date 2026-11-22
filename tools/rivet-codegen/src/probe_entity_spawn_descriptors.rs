//! `rivet-codegen probe-entity-spawn-descriptors` — re-run the CREATURE entity
//! spawn descriptor extractor against the real Paper 26.2 jar and require
//! byte-identity with the committed `data/entity_spawn_descriptors.json`,
//! valid provenance, and anchor counts.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::extract;

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

    let committed = crate::extract_entity_spawn_descriptors::default_output(&repo_root);
    let scratch = repo_root.join("tools/rivet-codegen/.cache/probe-entity-spawn-descriptors.json");

    crate::extract_entity_spawn_descriptors::run_extractor(&repo_root, &bundler, &scratch)?;
    let server_jar = extract::server_jar_for_bundler(&repo_root, &bundler)?;
    crate::reports::verify_fixture_provenance(
        &server_jar,
        &committed.with_extension("manifest.json"),
        &repo_root,
    )?;

    let fresh_bytes = fs::read(&scratch).context("read fresh entity spawn descriptors fixture")?;
    let committed_bytes =
        fs::read(&committed).with_context(|| format!("read committed {}", committed.display()))?;
    if fresh_bytes != committed_bytes {
        bail!(
            "probe-entity-spawn-descriptors: fresh Paper load differs from committed \
             data/entity_spawn_descriptors.json ({} bytes fresh vs {} bytes committed) — run \
             `rivet-codegen extract-entity-spawn-descriptors` and commit the result",
            fresh_bytes.len(),
            committed_bytes.len()
        );
    }

    crate::entity_spawn_descriptors::validate(&repo_root, &committed)?;
    let _ = fs::remove_file(&scratch);
    println!(
        "CREATURE entity spawn descriptors verified against live Paper (byte-identical, {} bytes, 19 descriptors)",
        committed_bytes.len()
    );
    Ok(())
}
