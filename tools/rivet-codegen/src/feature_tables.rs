//! `rivet-codegen generate` feature-data half — consume the deterministic
//! seed-42 `data/feature_data.json` fixture (produced by `extract-feature-data`,
//! see [`crate::extract_feature_data`]) and emit `generated/feature_data.rs`:
//! the Rust generated-table consumer for the FEATURES checkpoint.
//!
//! The fixture contract (structure, order, closure, provenance) is validated by
//! [`crate::feature_data`]; this module is the codegen half that renders the
//! validated data as deterministic Rust tables:
//!
//! - `BIOME_GENERATION_SETTINGS_BY_NAME` — the five reachable seed-42 biome
//!   generation settings: the registry id, the carver identity names, and the 11
//!   `GenerationStep.Decoration` per-step placed-feature lists (step order +
//!   holder-set order preserved).
//! - `PLACED_FEATURE_BY_NAME` / `CONFIGURED_FEATURE_BY_NAME` — the reachable
//!   placed/configured feature closure's name -> `(full-registry id,
//!   `RegistryOps` JSON)` entries. The ids are the dense *full-registry* ids
//!   (not contiguous within this subset — registry identity is preserved by the
//!   id, never by array position). The JSON is the datapack shape (holder refs
//!   are strings), re-serialized compactly and embedded verbatim: placement
//!   modifier chains and config semantics survive.
//!
//! Determinism: element tables are re-ordered by id (the fixture is read
//! order-insensitively, never trusted by key order) and the `RegistryOps` JSON
//! re-serializes through `serde_json`'s canonical map ordering. Regeneration is
//! byte-idempotent (enforced by the `generate.rs` drift test).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// Ground-truth anchors a live Paper 26.2 load must reproduce (kept in sync
/// with `ANCHORS` in `probe_feature_data.rs`).
pub const REACHABLE_BIOME_COUNT: usize = 5;
pub const PLACED_FEATURE_COUNT: usize = 72;
pub const CONFIGURED_FEATURE_COUNT: usize = 70;

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/feature_data.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One validated biome generation setting (the `biomes` table of the fixture).
#[derive(Debug)]
struct BiomeSettings {
    name: String,
    id: u16,
    carvers: Vec<String>,
    /// The 11 per-step placed-feature lists, in `GenerationStep.Decoration`
    /// ordinal order; holder-set order within a step preserved.
    features: Vec<Vec<String>>,
}

/// One validated placed/configured feature entry.
#[derive(Debug)]
struct FeatureEntry {
    name: String,
    id: u16,
    /// The `RegistryOps`-encoded JSON (datapack shape), re-serialized compactly.
    json: String,
}

/// The fully-validated fixture tables, each ordered by id.
type Validated = (Vec<BiomeSettings>, Vec<FeatureEntry>, Vec<FeatureEntry>);

pub fn run(input_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let input = match input_flag {
        Some(p) => p.to_path_buf(),
        None => default_input(&repo_root),
    };
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let json = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", input.display()))?;
    // The full fixture contract (structure, order, closure) + the pinned
    // provenance; a hand-edited or foreign fixture fails generation.
    crate::feature_data::validate_structural(&root)?;
    let (biomes, placed, configured) = extract(&root)?;
    // Anchor the extract against the pinned live-Paper counts (in addition to
    // `validate_structural`'s internal count check, so generation and the
    // `probe-feature-data` anchors cannot silently diverge).
    if biomes.len() != REACHABLE_BIOME_COUNT {
        anyhow::bail!(
            "extracted {} reachable biomes but a live Paper 26.2 load has {REACHABLE_BIOME_COUNT}",
            biomes.len()
        );
    }
    if placed.len() != PLACED_FEATURE_COUNT {
        anyhow::bail!(
            "extracted {} placed features but a live Paper 26.2 load has {PLACED_FEATURE_COUNT}",
            placed.len()
        );
    }
    if configured.len() != CONFIGURED_FEATURE_COUNT {
        anyhow::bail!(
            "extracted {} configured features but a live Paper 26.2 load has {CONFIGURED_FEATURE_COUNT}",
            configured.len()
        );
    }
    let source = crate::feature_data::load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("feature_data.rs"),
        render(&biomes, &placed, &configured, &source),
    )
    .context("write generated/feature_data.rs")?;

    println!(
        "Wrote {} biomes + {} placed features + {} configured features -> {}",
        biomes.len(),
        placed.len(),
        configured.len(),
        output.display()
    );
    Ok(())
}

/// Extract the render structs from the validated fixture, re-ordered by id (the
/// fixture is read order-insensitively; ids are the stable ordering key).
fn extract(root: &Value) -> Result<Validated> {
    let biomes = extract_biomes(&root["biomes"])?;
    let placed = extract_feature_table("placed_features", root)?;
    let configured = extract_feature_table("configured_features", root)?;
    Ok((biomes, placed, configured))
}

fn extract_biomes(value: &Value) -> Result<Vec<BiomeSettings>> {
    let obj = value
        .as_object()
        .context("`biomes` element table must be a JSON object")?;
    let mut out = Vec::with_capacity(obj.len());
    for (name, entry) in obj {
        crate::registries::validate_name("minecraft:worldgen/biome", name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("biome `{name}` entry must be a JSON object"))?;
        let id = parse_u16_id(name, &entry["id"])?;
        let carvers = entry
            .get("carvers")
            .and_then(Value::as_array)
            .with_context(|| format!("biome `{name}` is missing `carvers`"))?
            .iter()
            .map(|c| {
                c.as_str()
                    .map(str::to_string)
                    .with_context(|| format!("biome `{name}` has a non-string carver"))
            })
            .collect::<Result<Vec<_>>>()?;
        let steps = entry
            .get("features")
            .and_then(Value::as_array)
            .with_context(|| format!("biome `{name}` is missing `features`"))?;
        let mut features = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            let step = step
                .as_array()
                .with_context(|| format!("biome `{name}` step {i} is not an array"))?;
            let names = step
                .iter()
                .map(|p| {
                    p.as_str().map(str::to_string).with_context(|| {
                        format!("biome `{name}` step {i} has a non-string element")
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            features.push(names);
        }
        out.push(BiomeSettings {
            name: name.clone(),
            id,
            carvers,
            features,
        });
    }
    out.sort_unstable_by_key(|b| b.id);
    Ok(out)
}

fn extract_feature_table(field: &str, root: &Value) -> Result<Vec<FeatureEntry>> {
    let table = root
        .get(field)
        .and_then(Value::as_object)
        .with_context(|| format!("feature_data.json is missing `{field}`"))?;
    let registry = if field == "placed_features" {
        "minecraft:placed_feature"
    } else {
        "minecraft:configured_feature"
    };
    let mut out = Vec::with_capacity(table.len());
    for (name, entry) in table {
        crate::registries::validate_name(registry, name)?;
        let entry = entry
            .as_object()
            .with_context(|| format!("`{field}` entry `{name}` must be a JSON object"))?;
        let id = parse_u16_id(name, &entry["id"])?;
        let json = entry
            .get("json")
            .with_context(|| format!("`{field}` entry `{name}` is missing `json`"))?;
        let json = serde_json::to_string(json)
            .with_context(|| format!("`{field}` entry `{name}` JSON is not re-serializable"))?;
        out.push(FeatureEntry {
            name: name.clone(),
            id,
            json,
        });
    }
    // Ids are the dense full-registry ids, not contiguous within the subset;
    // id order is still the stable render order (unique, per the fixture's
    // name->id bijection check).
    out.sort_unstable_by_key(|e| e.id);
    Ok(out)
}

fn parse_u16_id(name: &str, id: &Value) -> Result<u16> {
    let id = id
        .as_u64()
        .with_context(|| format!("entry `{name}` has a non-integer id"))?;
    u16::try_from(id).with_context(|| format!("entry `{name}` id out of u16 range"))
}

/// Render `generated/feature_data.rs`.
fn render(
    biomes: &[BiomeSettings],
    placed: &[FeatureEntry],
    configured: &[FeatureEntry],
    source: &SourceProvenance,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/feature_data.json\n\
         // (live Paper seed-42 FEATURES load via WorldgenFeatureDataExtractor; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/feature_data.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// Seed-42 FEATURES data (issue #549): the reachable biome generation settings\n\
         // and the placed/configured feature closure a FEATURES pass must decode. The\n\
         // `RegistryOps` JSON is the datapack shape (holder refs are bare strings);\n\
         // the closure is exact — every placed `feature` ref and every bare string in a\n\
         // configured JSON resolves within these tables. All values are extracted from a\n\
         // live Paper 26.2 load, never hand-typed.\n\n",
    );

    out.push_str(
        "/// The number of `GenerationStep.Decoration` steps (raw_generation ..\n\
         /// top_layer_modification). A biome's `features` slice always holds exactly\n\
         /// this many step lists, in ordinal order.\n\
         pub const DECORATION_STEP_COUNT: usize = 11;\n\n",
    );

    out.push_str(
        "/// One reachable seed-42 biome generation setting, read directly by the\n\
         /// FEATURES orchestrator. `features` holds the 11 `GenerationStep.Decoration`\n\
         /// step lists in ordinal order; holder-set order within a step is the builder's\n\
         /// fixed order (part of the decoration semantics — never sorted).\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct BiomeGenerationSettings {\n\
         \x20   pub id: u16,\n\
         \x20   pub carvers: &'static [&'static str],\n\
         \x20   pub features: &'static [&'static [&'static str]],\n\
         }\n\n",
    );

    out.push_str(
        "/// A reachable `minecraft:placed_feature` entry: the full-registry dense id\n\
         /// (registry identity, never an array position) plus the `RegistryOps`-encoded\n\
         /// JSON (the datapack shape) preserving the `feature` holder reference and the\n\
         /// ordered placement-modifier chain.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct PlacedFeatureEntry {\n\
         \x20   pub id: u16,\n\
         \x20   pub json: &'static str,\n\
         }\n\n\
         /// A reachable `minecraft:configured_feature` entry: the full-registry dense id\n\
         /// plus the `RegistryOps`-encoded JSON preserving the feature `type` dispatch key\n\
         /// and the configuration verbatim.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct ConfiguredFeatureEntry {\n\
         \x20   pub id: u16,\n\
         \x20   pub json: &'static str,\n\
         }\n\n",
    );

    out.push_str(&render_biome_settings(biomes));
    out.push_str(&render_feature_entries(
        "PLACED_FEATURE_BY_NAME",
        placed,
        "PlacedFeatureEntry",
    ));
    out.push_str(&render_feature_entries(
        "CONFIGURED_FEATURE_BY_NAME",
        configured,
        "ConfiguredFeatureEntry",
    ));
    out
}

fn render_biome_settings(biomes: &[BiomeSettings]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "/// `minecraft:worldgen/biome` -> the reachable seed-42 generation settings\n\
         /// ({} biomes, keyed by full biome name; id order).\n",
        biomes.len()
    ));
    out.push_str(
        "pub static BIOME_GENERATION_SETTINGS_BY_NAME: phf::Map<&'static str, BiomeGenerationSettings> = phf::phf_map! {\n",
    );
    for b in biomes {
        let carvers = b
            .carvers
            .iter()
            .map(|c| format!("{c:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let steps = b
            .features
            .iter()
            .map(|names| {
                if names.is_empty() {
                    "&[]".to_string()
                } else {
                    let inner = names
                        .iter()
                        .map(|n| format!("{n:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("&[{inner}]")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "    {:?} => BiomeGenerationSettings {{ id: {}u16, carvers: &[{carvers}], features: &[{steps}] }},\n",
            b.name, b.id
        ));
    }
    out.push_str("};\n\n");
    out
}

fn render_feature_entries(const_name: &str, entries: &[FeatureEntry], entry_type: &str) -> String {
    let mut out = String::new();
    // Each physical line must already carry its `/// ` prefix — a string-literal
    // line continuation would strip it.
    let doc = if const_name.starts_with("PLACED") {
        "`minecraft:placed_feature` — placed feature name -> the full-registry entry\n\
         /// (id + `RegistryOps` JSON)."
    } else {
        "`minecraft:configured_feature` — configured feature name -> the full-registry\n\
         /// entry (id + `RegistryOps` JSON)."
    };
    out.push_str(&format!("/// {doc}\n"));
    out.push_str(&format!(
        "pub static {const_name}: phf::Map<&'static str, {entry_type}> = phf::phf_map! {{\n"
    ));
    for e in entries {
        out.push_str(&format!(
            "    {:?} => {entry_type} {{ id: {}u16, json: {:?} }},\n",
            e.name, e.id, e.json
        ));
    }
    out.push_str("};\n\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../data/feature_data.json")).unwrap()
    }

    /// The committed fixture extracts to the pinned counts, in id order.
    #[test]
    fn committed_fixture_extracts_to_pinned_counts() {
        let (biomes, placed, configured) = extract(&fixture()).unwrap();
        assert_eq!(biomes.len(), REACHABLE_BIOME_COUNT);
        assert_eq!(placed.len(), PLACED_FEATURE_COUNT);
        assert_eq!(configured.len(), CONFIGURED_FEATURE_COUNT);
        // Id order is the stable render order.
        assert!(biomes.windows(2).all(|w| w[0].id < w[1].id));
        assert!(placed.windows(2).all(|w| w[0].id < w[1].id));
        assert!(configured.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn biome_step_orders_are_preserved() {
        let (biomes, _, _) = extract(&fixture()).unwrap();
        // Every biome has the full 11-step surface; lush_caves' steps (the deep
        // cave biome, including its dense 9-entry step 9 decoration set) must
        // match the fixture's holder-set order element-for-element.
        for b in &biomes {
            assert_eq!(b.features.len(), 11, "biome {} must have 11 steps", b.name);
        }
        let lush = biomes
            .iter()
            .find(|b| b.name == "minecraft:lush_caves")
            .unwrap();
        let root = fixture();
        let fixture_steps = root["biomes"]["minecraft:lush_caves"]["features"]
            .as_array()
            .unwrap();
        for (i, names) in lush.features.iter().enumerate() {
            let fixture_names = fixture_steps[i].as_array().unwrap();
            let fixture_names: Vec<&str> =
                fixture_names.iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(
                names.as_slice(),
                fixture_names,
                "lush_caves step {i} order drifted"
            );
        }
    }

    /// A step-list reorder must fail the fixture contract before rendering.
    #[test]
    fn step_reorder_fails_validation() {
        let mut root = fixture();
        let steps = root["biomes"]["minecraft:dark_forest"]["features"]
            .as_array_mut()
            .unwrap();
        steps.swap(1, 2);
        let err = crate::feature_data::validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("step 1 has"), "got: {err}");
    }

    #[test]
    fn placed_configured_identity_is_preserved() {
        let (_, placed, configured) = extract(&fixture()).unwrap();
        // The dense full-registry ids are not contiguous within the subset, but
        // they are unique (registry identity). The reference amethyst_geode
        // entry pins the concrete full-registry ids.
        let p = placed
            .iter()
            .find(|e| e.name == "minecraft:amethyst_geode")
            .unwrap();
        assert_eq!(p.id, 2);
        let c = configured
            .iter()
            .find(|e| e.name == "minecraft:amethyst_geode")
            .unwrap();
        assert_eq!(c.id, 1);
    }

    #[test]
    fn json_round_trips_through_render() {
        // The re-serialized RegistryOps JSON is parseable and preserves the
        // datapack shape (placed: `feature` + `placement`; configured: `type` +
        // `config`).
        let (_, placed, configured) = extract(&fixture()).unwrap();
        for e in &placed {
            let v: Value = serde_json::from_str(&e.json).unwrap();
            assert!(v["feature"].is_string(), "{} missing feature ref", e.name);
            assert!(v["placement"].is_array(), "{} missing placement", e.name);
        }
        for e in &configured {
            let v: Value = serde_json::from_str(&e.json).unwrap();
            assert!(v["type"].is_string(), "{} missing type", e.name);
            assert!(v["config"].is_object(), "{} missing config", e.name);
        }
    }

    /// The rendered table keeps the exact transitive closure: every biome step
    /// reference and every placed `feature` ref resolves within the emitted
    /// tables.
    #[test]
    fn render_preserves_closure() {
        let (biomes, placed, configured) = extract(&fixture()).unwrap();
        let placed_names: std::collections::HashSet<&str> =
            placed.iter().map(|e| e.name.as_str()).collect();
        let configured_names: std::collections::HashSet<&str> =
            configured.iter().map(|e| e.name.as_str()).collect();
        for b in &biomes {
            for names in &b.features {
                for n in names {
                    assert!(
                        placed_names.contains(n.as_str()),
                        "biome {} references placed `{n}` outside the closure",
                        b.name
                    );
                }
            }
        }
        for e in &placed {
            let v: Value = serde_json::from_str(&e.json).unwrap();
            let feature = v["feature"].as_str().unwrap();
            assert!(
                configured_names.contains(feature),
                "placed {} references configured `{feature}` outside the closure",
                e.name
            );
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let (biomes, placed, configured) = extract(&fixture()).unwrap();
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let first = render(&biomes, &placed, &configured, &source);
        let second = render(&biomes, &placed, &configured, &source);
        assert_eq!(first, second);
        assert!(first.contains("MC 26.2, protocol 776, world 4903"));
        assert!(first.contains("DECORATION_STEP_COUNT"));
        assert!(first.contains("BIOME_GENERATION_SETTINGS_BY_NAME"));
        assert!(first.contains("\"minecraft:beach\" => BiomeGenerationSettings"));
        assert!(first.contains("PLACED_FEATURE_BY_NAME"));
        assert!(first.contains("CONFIGURED_FEATURE_BY_NAME"));
        assert!(first.contains("PlacedFeatureEntry { id: 2u16, json:"));
    }
}
