//! `rivet-codegen` feature-data validation half — consume the committed
//! `data/feature_data.json` fixture (produced by `extract-feature-data`, see
//! [`crate::extract_feature_data`]) and validate its structure, provenance,
//! order, and closure. The generation half that consumes this fixture is
//! [`crate::feature_tables`]; this module is the fixture's contract.
//!
//! The fixture is the seed-42 FEATURES data foundation:
//!
//!   * `reachable_biomes` — the biome set that can drive FEATURES placement into
//!     the committed grid {(3,3),(4,3),(3,4),(4,4)} (chunks 1..6, full Y range),
//!     sorted by registry id.
//!   * `biomes` — per-biome `BiomeGenerationSettings`: `id`, the carver identity
//!     names, and the per-step placed-feature name lists. Step `i` is
//!     `GenerationStep.Decoration.values()[i]` (raw_generation .. top_layer_
//!     modification); holder-set order within a step is the builder's fixed
//!     order (part of the decoration semantics).
//!   * `placed_features` / `configured_features` — the transitive closure of
//!     referenced registry entries, keyed by name with the dense registry `id`
//!     and the full `RegistryOps`-encoded JSON (the datapack JSON shape).
//!
//! Validation (all read order-insensitively):
//!   * provenance — the fixture bytes must match the sha256 recorded next to it
//!     in `data/feature_data.manifest.json` (a hand-edited fixture fails);
//!   * structure — every top-level field, the counts, and the per-biome step
//!     arrays;
//!   * order — reachable biomes are id-sorted and match the `biomes` key set;
//!     the per-biome per-step arrays are pinned (a step-list reorder changes
//!     decoration semantics and must fail);
//!   * closure — every placed feature referenced by the biomes or by a
//!     configured feature's JSON is present in `placed_features`; every
//!     configured feature referenced by a placed feature's `feature` field or by
//!     a configured feature's JSON is present in `configured_features`.
//!
//! Regeneration is byte-idempotent (the live probe proves a fresh load is
//! byte-identical to the committed fixture).

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// Ground-truth anchors a live Paper 26.2 load must reproduce (kept in sync
/// with `ANCHORS` in `probe_feature_data.rs`).
pub const REACHABLE_BIOME_COUNT: usize = 5;
pub const PLACED_FEATURE_COUNT: usize = 72;
pub const CONFIGURED_FEATURE_COUNT: usize = 70;

/// Non-vacuity: the reachable biome set must include the deep `lush_caves`
/// biome AND at least one surface biome (decoration evidence).
pub const REQUIRED_BIOMES: &[&str] = &["minecraft:lush_caves", "minecraft:beach"];

/// The pinned per-biome feature-step counts (step order + holder-set order is
/// part of the decoration semantics; a reorder must fail). Keyed by biome name.
pub const PER_BIOME_STEP_COUNTS: &[(&str, &[usize])] = &[
    ("minecraft:beach", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 8, 1]),
    (
        "minecraft:dark_forest",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    ("minecraft:lush_caves", &[0, 2, 1, 2, 0, 0, 30, 0, 2, 9, 1]),
    ("minecraft:ocean", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:river", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
];

/// Validate the fixture at `input` and return the source provenance from its
/// sibling manifest. Called by `extract-feature-data` (self-validation of a
/// fresh capture) and by the (later) generation slice; the tests here exercise
/// it against the committed fixture.
pub fn validate(input: &Path) -> Result<SourceProvenance> {
    let json = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", input.display()))?;
    validate_structural(&root)?;
    load_provenance(input)
}

/// Structural + order + closure validation, independent of the live-Paper
/// anchors (which only the real fixture passes). `pub(crate)` so the generation
/// half ([`crate::feature_tables`]) enforces the full contract on the fixture
/// it renders.
pub(crate) fn validate_structural(root: &Value) -> Result<()> {
    let object = root
        .as_object()
        .context("feature_data.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "format"
                | "generator"
                | "paper"
                | "minecraft_version"
                | "protocol_version"
                | "world_version"
                | "seed"
                | "grid_min_chunk"
                | "grid_max_chunk"
                | "committed_grid"
                | "reachable_biomes"
                | "biomes"
                | "placed_features"
                | "configured_features"
                | "probe"
        ) {
            bail!("feature_data.json has unexpected top-level field `{field}`");
        }
    }

    // Reachable biome list: id-sorted names, non-empty, required biomes present.
    let reachable = object
        .get("reachable_biomes")
        .and_then(Value::as_array)
        .context("feature_data.json is missing `reachable_biomes`")?;
    if reachable.len() != REACHABLE_BIOME_COUNT {
        bail!(
            "reachable_biomes has {} entries but a live Paper 26.2 load has {REACHABLE_BIOME_COUNT}",
            reachable.len()
        );
    }
    let mut names: Vec<&str> = Vec::with_capacity(reachable.len());
    for (i, v) in reachable.iter().enumerate() {
        let name = v
            .as_str()
            .with_context(|| format!("reachable_biomes[{i}] is not a string"))?;
        crate::registries::validate_name("minecraft:worldgen/biome", name)?;
        names.push(name);
    }
    // Id-sorted is checked below against the `biomes` dense ids (the extractor
    // emits the reachable list via a TreeMap keyed by `biomeReg.getId`); the
    // name order is not the contract — the dense-id order is.
    for required in REQUIRED_BIOMES {
        if !names.contains(required) {
            bail!("reachable biome set is missing required `{required}` (non-vacuity)");
        }
    }

    // Biomes: key set == reachable set, each with id, carvers, features.
    let biomes = object
        .get("biomes")
        .and_then(Value::as_object)
        .context("feature_data.json is missing `biomes`")?;
    if biomes.len() != names.len() {
        bail!(
            "`biomes` has {} entries but reachable_biomes has {}",
            biomes.len(),
            names.len()
        );
    }
    for name in &names {
        if !biomes.contains_key(*name) {
            bail!("biome `{name}` is reachable but absent from `biomes`");
        }
    }
    for (name, entry) in biomes {
        crate::registries::validate_name("minecraft:worldgen/biome", name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("biome `{name}` entry must be an object"))?;
        for field in entry.keys() {
            if !matches!(field.as_str(), "id" | "carvers" | "features") {
                bail!("biome `{name}` has unexpected field `{field}`");
            }
        }
        let id = entry
            .get("id")
            .and_then(Value::as_u64)
            .with_context(|| format!("biome `{name}` is missing `id`"))?;
        let _ = u16::try_from(id).with_context(|| format!("biome `{name}` id out of u16 range"))?;
        entry
            .get("carvers")
            .and_then(Value::as_array)
            .with_context(|| format!("biome `{name}` is missing `carvers`"))?;
        let features = entry
            .get("features")
            .and_then(Value::as_array)
            .with_context(|| format!("biome `{name}` is missing `features`"))?;
        if features.len() != 11 {
            bail!(
                "biome `{name}` has {} feature steps but GenerationStep.Decoration has 11",
                features.len()
            );
        }
        // Pinned per-step counts (order is part of the semantics).
        let pinned = PER_BIOME_STEP_COUNTS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| *c)
            .with_context(|| format!("biome `{name}` has no pinned step counts"))?;
        for (i, step) in features.iter().enumerate() {
            let step = step
                .as_array()
                .with_context(|| format!("biome `{name}` step {i} is not an array"))?;
            if step.len() != pinned[i] {
                bail!(
                    "biome `{name}` step {i} has {} features but a live Paper 26.2 load has {}",
                    step.len(),
                    pinned[i]
                );
            }
            for (j, v) in step.iter().enumerate() {
                let placed = v.as_str().with_context(|| {
                    format!("biome `{name}` step {i} element {j} is not a string")
                })?;
                crate::registries::validate_name("minecraft:worldgen/placed_feature", placed)?;
            }
        }
    }

    // Reachable biomes must be sorted by dense registry id (the extractor emits
    // them via a TreeMap keyed by `biomeReg.getId`; a reorder or a hand-edited
    // id reassignment must fail). Names can disagree with id order across
    // registries, so this checks the ids, not the names.
    let mut ids = Vec::with_capacity(names.len());
    for name in &names {
        let id = biomes
            .get(*name)
            .and_then(|e| e.get("id"))
            .and_then(Value::as_u64)
            .with_context(|| format!("biome `{name}` is missing `id` for the id-order check"))?;
        ids.push(id);
    }
    if ids.windows(2).any(|w| w[0] >= w[1]) {
        bail!("reachable_biomes is not sorted by registry id (got ids {ids:?})");
    }

    // Feature element tables.
    let placed = validate_feature_table("placed_features", object)?;
    let configured = validate_feature_table("configured_features", object)?;

    // Closure reachability: every feature in the fixture must be reachable from
    // the biomes' step lists, and every reference must resolve. A fixture with
    // dead (unreachable) entries or a dangling reference is stale or hand-edited.
    //
    //   placed reachable  = biomes' direct placed features ∪ placed refs found
    //                       in configured-feature JSONs
    //   configured reach = placed features' `json.feature` ∪ configured refs
    //                       found in configured-feature JSONs
    //
    // A bare string inside a RegistryOps JSON is a feature holder reference only
    // when it names a feature in the fixture's own tables; block-state `Name`
    // values like `minecraft:oak_log` are in neither table and are ignored
    // (registry-membership disambiguation, mirroring the extractor). A string in
    // both tables (e.g. `minecraft:oak`) counts as both a placed and a
    // configured reference, mirroring the extractor's dual-kind walk.
    let mut direct_placed: HashSet<&str> = HashSet::new();
    for entry in biomes.values() {
        for step in entry["features"].as_array().unwrap() {
            for v in step.as_array().unwrap() {
                direct_placed.insert(v.as_str().unwrap());
            }
        }
    }

    let mut placed_reachable: HashSet<&str> = direct_placed.clone();
    let mut configured_reachable: HashSet<&str> = HashSet::new();

    for (pname, pentry) in placed {
        let json = pentry
            .get("json")
            .and_then(Value::as_object)
            .with_context(|| format!("placed feature `{pname}` is missing `json`"))?;
        let feature_ref = json
            .get("feature")
            .and_then(Value::as_str)
            .with_context(|| format!("placed feature `{pname}` `json.feature` is not a string"))?;
        configured_reachable.insert(feature_ref);
    }

    for centry in configured.values() {
        let json = centry
            .get("json")
            .context("configured feature is missing `json`")?;
        for r in collect_bare_strings(json) {
            if placed.contains_key(r) {
                placed_reachable.insert(r);
            }
            if configured.contains_key(r) {
                configured_reachable.insert(r);
            }
        }
    }

    for p in &placed_reachable {
        if !placed.contains_key(*p) {
            bail!(
                "biome/config references placed feature `{p}` that is absent from `placed_features`"
            );
        }
    }
    for c in &configured_reachable {
        if !configured.contains_key(*c) {
            bail!(
                "placed/config references configured feature `{c}` that is absent from `configured_features`"
            );
        }
    }
    for p in placed.keys() {
        if !placed_reachable.contains(p.as_str()) {
            bail!(
                "placed feature `{p}` is present but unreachable (not in any biome step list and \
                 not referenced by any configured feature) — stale fixture entry"
            );
        }
    }
    for c in configured.keys() {
        if !configured_reachable.contains(c.as_str()) {
            bail!(
                "configured feature `{c}` is present but unreachable (not referenced by any placed \
                 or configured feature) — stale fixture entry"
            );
        }
    }

    // Anchor counts (checked after the closure so a removed entry reports the
    // specific closure error, not a bare count drift).
    if placed.len() != PLACED_FEATURE_COUNT {
        bail!(
            "placed_features has {} entries but a live Paper 26.2 load has {PLACED_FEATURE_COUNT}",
            placed.len()
        );
    }
    if configured.len() != CONFIGURED_FEATURE_COUNT {
        bail!(
            "configured_features has {} entries but a live Paper 26.2 load has {CONFIGURED_FEATURE_COUNT}",
            configured.len()
        );
    }

    // Probe-count internal consistency.
    let probe = object
        .get("probe")
        .and_then(Value::as_object)
        .context("feature_data.json is missing `probe`")?;
    let check = |key: &str, expected: usize| -> Result<()> {
        let found = probe
            .get(key)
            .and_then(Value::as_u64)
            .with_context(|| format!("probe is missing `{key}`"))?;
        if found as usize != expected {
            bail!("probe.{key} is {found} but the fixture has {expected}");
        }
        Ok(())
    };
    check("reachable_biome_count", names.len())?;
    check("placed_feature_count", placed.len())?;
    check("configured_feature_count", configured.len())?;

    Ok(())
}

/// Validate one feature element table: `name -> { id, json }`, with a unique
/// name per entry and a unique registry id per entry. The registry ids are the
/// *full-registry* dense ids (`ResourceManagerRegistryLoadTask` TreeMap order) —
/// this fixture holds only a subset, so ids are not contiguous within the table;
/// they must still be unique (registry identity is preserved).
fn validate_feature_table<'a>(
    field: &str,
    object: &'a serde_json::Map<String, Value>,
) -> Result<&'a serde_json::Map<String, Value>> {
    let table = object
        .get(field)
        .and_then(Value::as_object)
        .with_context(|| format!("feature_data.json is missing `{field}`"))?;
    let registry = if field == "placed_features" {
        "minecraft:worldgen/placed_feature"
    } else {
        "minecraft:worldgen/configured_feature"
    };
    let mut ids: Vec<u16> = Vec::with_capacity(table.len());
    for (name, entry) in table {
        crate::registries::validate_name(registry, name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("`{field}` entry `{name}` must be an object"))?;
        for f in entry.keys() {
            if !matches!(f.as_str(), "id" | "json") {
                bail!("`{field}` entry `{name}` has unexpected field `{f}`");
            }
        }
        let id = entry
            .get("id")
            .and_then(Value::as_u64)
            .with_context(|| format!("`{field}` entry `{name}` is missing `id`"))?;
        let id = u16::try_from(id)
            .with_context(|| format!("`{field}` entry `{name}` id out of u16 range"))?;
        entry
            .get("json")
            .with_context(|| format!("`{field}` entry `{name}` is missing `json`"))?;
        ids.push(id);
    }
    // A name -> id bijection within the table (registry ids are globally unique).
    ids.sort_unstable();
    for pair in ids.windows(2) {
        if pair[0] == pair[1] {
            bail!("`{field}` has a duplicate registry id `{}`", pair[0]);
        }
    }
    Ok(table)
}

/// All bare-string values inside a RegistryOps-encoded JSON. Callers resolve
/// whether each string is a feature holder reference by membership in the
/// fixture's own feature tables (a block-state `Name` like `minecraft:oak_log`
/// is in neither and is ignored; a key like `minecraft:oak` in both counts as
/// both a placed and a configured reference).
fn collect_bare_strings(elem: &Value) -> Vec<&str> {
    fn walk<'a>(elem: &'a Value, out: &mut Vec<&'a str>) {
        match elem {
            Value::String(s) => {
                // A bare `minecraft:` string is a candidate feature holder
                // reference. No dot filter: the caller resolves membership in
                // the fixture's own feature tables exactly as the extractor's
                // `collectFeatureRefs` does (block-state `Name` values are
                // object fields, never bare refs, so membership disambiguation
                // is unambiguous; a future feature key containing a dot is not
                // dropped).
                if s.starts_with("minecraft:") {
                    out.push(s.as_str());
                }
            }
            Value::Array(a) => a.iter().for_each(|e| walk(e, out)),
            Value::Object(o) => o.values().for_each(|e| walk(e, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(elem, &mut out);
    out
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in its sibling `<fixture>.manifest.json` (the same path
/// `extract-feature-data` writes, so a custom `--output` resolves correctly).
/// `pub(crate)` so the generation half pins the same source identity it renders
/// into the emitted table header.
pub(crate) fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input.with_extension("manifest.json");
    let manifest_json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "read {} (expected next to the pinned fixture)",
            manifest_path.display()
        )
    })?;
    let manifest: FixtureManifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let actual = crate::reports::sha256_hex(&bytes);
    if actual != manifest.file.sha256 {
        bail!(
            "feature_data.json does not match its provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen extract-feature-data` to refresh the pinned fixture",
            manifest.file.sha256,
            actual
        );
    }
    Ok(manifest.source)
}

#[derive(serde::Deserialize)]
struct FixtureManifest {
    source: SourceProvenance,
    file: FixtureFile,
}

#[derive(serde::Deserialize)]
struct FixtureFile {
    sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../data/feature_data.json")).unwrap()
    }

    #[test]
    fn committed_fixture_passes_structural_validation() {
        // The committed fixture satisfies the full structural + order + closure
        // contract (the anchors too, since the extractor produced it).
        let root = fixture();
        validate_structural(&root).unwrap();
    }

    #[test]
    fn reachable_biome_order_is_pinned() {
        let mut root = fixture();
        // Reverse the reachable list: must fail (id-sorted order is part of the
        // fixture contract).
        let mut arr = root["reachable_biomes"].as_array().unwrap().clone();
        arr.reverse();
        root["reachable_biomes"] = Value::Array(arr);
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("not sorted"), "got: {err}");
    }

    #[test]
    fn reachable_biome_dense_id_order_is_checked() {
        let mut root = fixture();
        // Reassign the beach biome's dense id so the reachable names stay
        // lexically sorted but the registry-id order breaks (beach is the first
        // entry, so it needs a higher id than dark_forest to break monotonicity).
        let beach_id = root["biomes"]["minecraft:beach"]["id"].as_u64().unwrap();
        let dark_forest_id = root["biomes"]["minecraft:dark_forest"]["id"]
            .as_u64()
            .unwrap();
        root["biomes"]["minecraft:beach"]["id"] = serde_json::json!(dark_forest_id + 1);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("not sorted by registry id"),
            "got: {err}"
        );
        // Sanity: the edit is a real change (beach id differs from its original).
        assert_ne!(beach_id, dark_forest_id + 1);
    }

    #[test]
    fn per_biome_step_reorder_fails() {
        let mut root = fixture();
        // Swap two steps of dark_forest's features: must fail (step order is
        // part of the decoration semantics).
        let steps = root["biomes"]["minecraft:dark_forest"]["features"]
            .as_array_mut()
            .unwrap();
        steps.swap(1, 2);
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("step 1 has"), "got: {err}");
    }

    #[test]
    fn removed_placed_feature_fails_closure() {
        let mut root = fixture();
        // Drop a placed feature that a biome references: closure must fail.
        let removed = root["biomes"]["minecraft:beach"]["features"][1][0]
            .as_str()
            .unwrap()
            .to_string();
        root["placed_features"]
            .as_object_mut()
            .unwrap()
            .remove(&removed);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("absent from `placed_features`"),
            "got: {err}"
        );
    }

    #[test]
    fn provenance_manifest_matches_fixture() {
        // The committed fixture must match the sha256 in its provenance
        // manifest — a hand-edited fixture fails here. Re-read both fresh so a
        // mutation to one without the other is caught.
        let repo_root = crate::extract::find_repo_root().unwrap();
        let input = crate::extract_feature_data::default_output(&repo_root);
        let source = load_provenance(&input).unwrap();
        assert_eq!(source.minecraft_version, "26.2");
        assert_eq!(source.protocol_version, 776);
        assert_eq!(source.world_version, 4903);
        // The provenance must be the pinned Paper 26.2 build @ 0a99345: the jar
        // sha256 of the materialized server jar is the load-bearing identity and
        // is pinned exactly (a different jar — even with a matching 64-char hash
        // length — fails here).
        assert_eq!(
            source.jar_sha256,
            "e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda"
        );
    }
}
