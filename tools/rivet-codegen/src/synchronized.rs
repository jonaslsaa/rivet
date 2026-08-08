//! `rivet-codegen generate` synchronized-registries half — consume the
//! deterministic `data/synchronized_registries.json` fixture and emit
//! `generated/synchronized.rs`: the element tables the configuration
//! registry sync (`SynchronizeRegistriesTask` →
//! `RegistrySynchronization.packRegistries`) advertises.
//!
//! `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` is the fixed 29-registry list
//! the `ClientboundRegistryDataPacket` stream carries (one packet per registry,
//! each element `PackedRegistryEntry(id, Optional<Tag> data)`). Element *ids*
//! are always on the wire; this module emits the element *names* (the metadata
//! half). The full per-element NBT `data` content is the sibling `registry_data`
//! codegen (`generated::registry_data` → `SYNCHRONIZED_NBT`).
//!
//! The runtime element order is ascending registry id (`registry.listElements()`
//! == insertion index == network id; OWNERSHIP.md §Registries), so the fixture
//! records each registry's element names in that order. The fixture is derived
//! from the canonical join capture
//! (`tools/rivet-capture/fixtures/join/capture.jsonl`): the `registry_data`
//! packet bodies decoded via `rivet-protocol`'s `ClientboundRegistryDataPacket`
//! codec, then cross-checked against the extract-driven id tables for the 8
//! synchronized registries that already have one (see the anchored `*_BY_NAME`
//! tables in `biomes.rs`/`tags.rs`). The other 21 are datapack registries the
//! report/extract surfaces cannot cover, so the capture is their only source.
//!
//! Determinism: the fixture is read order-insensitively and re-emitted in the
//! fixed `SYNCHRONIZED_REGISTRIES` order; regeneration is byte-idempotent.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` in order (MC 26.2). The wire
/// order of the 29 `ClientboundRegistryDataPacket`s; a drift in which registry
/// or how many fails generation. Shared with `registry_data` (the pre-baked
/// NBT payloads must be served in the same packet order).
pub(crate) const SYNCHRONIZED_KEYS: &[&str] = &[
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:test_environment",
    "minecraft:test_instance",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
];

/// Ground-truth anchors the fixture must reproduce (from the live Paper 26.2
/// join capture). A fixture from a different capture or a hand-edited one fails
/// generation.
const ELEMENT_COUNTS: &[(&str, usize)] = &[
    ("minecraft:worldgen/biome", 66),
    ("minecraft:chat_type", 8),
    ("minecraft:trim_pattern", 18),
    ("minecraft:trim_material", 11),
    ("minecraft:wolf_variant", 9),
    ("minecraft:wolf_sound_variant", 7),
    ("minecraft:pig_variant", 3),
    ("minecraft:pig_sound_variant", 3),
    ("minecraft:frog_variant", 3),
    ("minecraft:cat_variant", 11),
    ("minecraft:cat_sound_variant", 2),
    ("minecraft:cow_sound_variant", 2),
    ("minecraft:cow_variant", 3),
    ("minecraft:chicken_sound_variant", 2),
    ("minecraft:chicken_variant", 3),
    ("minecraft:zombie_nautilus_variant", 2),
    ("minecraft:painting_variant", 51),
    ("minecraft:sulfur_cube_archetype", 12),
    ("minecraft:dimension_type", 4),
    ("minecraft:damage_type", 51),
    ("minecraft:banner_pattern", 43),
    ("minecraft:enchantment", 43),
    ("minecraft:jukebox_song", 22),
    ("minecraft:instrument", 8),
    ("minecraft:test_environment", 1),
    ("minecraft:test_instance", 1),
    ("minecraft:dialog", 3),
    ("minecraft:world_clock", 2),
    ("minecraft:timeline", 4),
];

/// The synchronized registries whose element table is already emitted by
/// another generated file, so the generated `synchronized.rs` re-emits only the
/// element-name list (not a duplicate id table). These are cross-checked at
/// generate time against the existing tables (drift guard), and the id
/// resolution for the update-tags/registry-data payloads uses the canonical
/// tables.
const SHARED_ELEMENT_SURFACES: &[&str] = &[
    "minecraft:worldgen/biome",   // biomes.rs
    "minecraft:enchantment",      // tags.rs
    "minecraft:dialog",           // tags.rs
    "minecraft:painting_variant", // tags.rs
    "minecraft:timeline",         // tags.rs
    "minecraft:instrument",       // tags.rs
    "minecraft:banner_pattern",   // tags.rs
    "minecraft:damage_type",      // tags.rs
];

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/synchronized_registries.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One synchronized registry surface: the registry key + element names in
/// ascending-id (wire) order.
struct SynchronizedRegistry {
    key: String,
    elements: Vec<String>,
}

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
    let registries = validate(&root)?;
    let provenance = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("synchronized.rs"),
        render(&registries, &provenance),
    )
    .context("write generated/synchronized.rs")?;

    println!(
        "Wrote {} synchronized registries / {} elements -> {}",
        registries.len(),
        registries.iter().map(|r| r.elements.len()).sum::<usize>(),
        output.display()
    );
    Ok(())
}

/// Full validation for the committed fixture: structural validation + the
/// shared-surface cross-check against the existing generated id tables + the
/// live-Paper element-count anchors.
fn validate(root: &Value) -> Result<Vec<SynchronizedRegistry>> {
    let registries = validate_structural(root)?;

    // Cross-check the shared surfaces against the existing generated tables (a
    // runtime element-order change that the id tables would catch fails here).
    for key in SHARED_ELEMENT_SURFACES {
        let reg = registries
            .iter()
            .find(|r| r.key.as_str() == *key)
            .context("shared synchronized surface present")?;
        let table = read_generated_table(key).with_context(|| format!("read {key} table"))?;
        cross_check_shared(key, &reg.elements, &table)?;
    }

    // Anchor check: element counts must match the live-Paper ground truth.
    for (key, expected_count) in ELEMENT_COUNTS {
        let reg = registries
            .iter()
            .find(|r| r.key.as_str() == *key)
            .context("anchor registry present")?;
        if reg.elements.len() != *expected_count {
            bail!(
                "anchor drift: `{key}` has {} elements but a live Paper 26.2 load has {expected_count}",
                reg.elements.len()
            );
        }
    }

    Ok(registries)
}

/// Structural validation (independent of the report/provenance): malformed
/// element names, empty registries, and the authoritative registry set.
fn validate_structural(root: &Value) -> Result<Vec<SynchronizedRegistry>> {
    let object = root
        .as_object()
        .context("synchronized_registries.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "generator" | "minecraft_version" | "protocol_version" | "world_version" | "registries"
        ) {
            bail!("synchronized_registries.json has unexpected top-level field `{field}`");
        }
    }
    let _mc = object
        .get("minecraft_version")
        .and_then(Value::as_str)
        .context("synchronized_registries.json is missing `minecraft_version`")?;
    for (field, min) in [("protocol_version", 0u64), ("world_version", 0u64)] {
        match object.get(field).and_then(Value::as_u64) {
            Some(v) if v >= min => {}
            Some(_) => bail!("synchronized_registries.json `{field}` is out of range"),
            None => bail!("synchronized_registries.json is missing `{field}`"),
        }
    }

    let registries_obj = object
        .get("registries")
        .and_then(Value::as_object)
        .context("synchronized_registries.json is missing `registries`")?;

    // The full authoritative set: every SYNCHRONIZED_REGISTRIES key must be
    // present, in order, with nothing extra.
    for key in SYNCHRONIZED_KEYS {
        let names = registries_obj
            .get(*key)
            .and_then(Value::as_array)
            .with_context(|| format!("`{key}` is missing or not a list"))?;
        if names.is_empty() {
            bail!("`{key}` has an empty element list");
        }
        let mut seen = std::collections::HashSet::new();
        for (j, name) in names.iter().enumerate() {
            let s = name
                .as_str()
                .with_context(|| format!("`{key}` element {j} is not a string"))?;
            if s.is_empty() {
                bail!("`{key}` element {j} is an empty string");
            }
            if !s.contains(':') {
                bail!("`{key}` element `{s}` is not namespaced");
            }
            if !seen.insert(s) {
                bail!("`{key}` has duplicate element `{s}`");
            }
        }
    }

    let mut registries = Vec::with_capacity(SYNCHRONIZED_KEYS.len());
    for key in SYNCHRONIZED_KEYS {
        let names = registries_obj[*key]
            .as_array()
            .expect("validated present")
            .iter()
            .map(|v| v.as_str().expect("validated string").to_string())
            .collect();
        registries.push(SynchronizedRegistry {
            key: (*key).to_string(),
            elements: names,
        });
    }
    // Unknown registries in the fixture are drift.
    for key in registries_obj.keys() {
        if !SYNCHRONIZED_KEYS.contains(&key.as_str()) {
            bail!("synchronized_registries.json has unknown registry `{key}`");
        }
    }

    Ok(registries)
}

/// Read the existing generated element table (`*_BY_NAME` dense name→id map)
/// for a shared synchronized surface, as the ordered name list.
fn read_generated_table(registry_key: &str) -> Result<Vec<String>> {
    let (filename, table_name) = match registry_key {
        "minecraft:worldgen/biome" => ("biomes.rs", "BIOME_BY_NAME"),
        "minecraft:enchantment" => ("tags.rs", "ENCHANTMENT_BY_NAME"),
        "minecraft:dialog" => ("tags.rs", "DIALOG_BY_NAME"),
        "minecraft:painting_variant" => ("tags.rs", "PAINTING_VARIANT_BY_NAME"),
        "minecraft:timeline" => ("tags.rs", "TIMELINE_BY_NAME"),
        "minecraft:instrument" => ("tags.rs", "INSTRUMENT_BY_NAME"),
        "minecraft:banner_pattern" => ("tags.rs", "BANNER_PATTERN_BY_NAME"),
        "minecraft:damage_type" => ("tags.rs", "DAMAGE_TYPE_BY_NAME"),
        other => bail!("no generated table for {other}"),
    };
    let repo_root = crate::extract::find_repo_root()?;
    let path = repo_root
        .join("crates/rivet-registry/src/generated")
        .join(filename);
    let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut in_table = false;
    let mut entries = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with(&format!("pub static {table_name}")) {
            in_table = true;
            continue;
        }
        if in_table {
            if line.starts_with("};") {
                break;
            }
            // `"minecraft:badlands" => 0u16,`
            if let Some(rest) = line.strip_prefix('"')
                && let Some(name) = rest.split('"').next()
            {
                entries.push(name.to_string());
            }
        }
    }
    if entries.is_empty() {
        bail!("generated {filename} has no `{table_name}` entries");
    }
    Ok(entries)
}

/// The shared surface's element names (ascending-id order, from the fixture)
/// must equal the existing generated table's dense name→id order — the same
/// bijection the update-tags/registry-data payloads resolve ids through.
fn cross_check_shared(key: &str, elements: &[String], table: &[String]) -> Result<()> {
    let table_key = match key {
        "minecraft:worldgen/biome" => "BIOME_BY_NAME",
        "minecraft:enchantment" => "ENCHANTMENT_BY_NAME",
        "minecraft:dialog" => "DIALOG_BY_NAME",
        "minecraft:painting_variant" => "PAINTING_VARIANT_BY_NAME",
        "minecraft:timeline" => "TIMELINE_BY_NAME",
        "minecraft:instrument" => "INSTRUMENT_BY_NAME",
        "minecraft:banner_pattern" => "BANNER_PATTERN_BY_NAME",
        "minecraft:damage_type" => "DAMAGE_TYPE_BY_NAME",
        other => bail!("no generated table for {other}"),
    };
    if elements.len() != table.len() {
        bail!(
            "`{key}` fixture has {} elements but the generated {table_key} has {} — drift",
            elements.len(),
            table.len()
        );
    }
    for (i, (a, b)) in elements.iter().zip(table.iter()).enumerate() {
        if a != b {
            bail!(
                "`{key}` element {i} is `{a}` in the fixture but `{b}` in the generated {table_key} — drift"
            );
        }
    }
    Ok(())
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/synchronized_registries.manifest.json`, and the
/// emitted header carries that provenance (capture identity + MC/proto/world
/// versions).
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("synchronized_registries.manifest.json"))
        .with_context(|| format!("{} has no parent dir", input.display()))?;
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
            "synchronized_registries.json does not match its provenance manifest (expected sha256 {}, got {})",
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

/// Render `generated/synchronized.rs`.
fn render(registries: &[SynchronizedRegistry], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/synchronized_registries.json\n\
         // (the canonical join capture's registry_data packet bodies, decoded via rivet-protocol;\n\
         //  MC {}, protocol {}, world {}).\n\
         // Source capture sha256 {}; provenance linked to data/synchronized_registries.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` element tables: the names each\n\
         // `ClientboundRegistryDataPacket` carries, in ascending registry id order\n\
         // (`registry.listElements()` == insertion index == network id; OWNERSHIP.md\n\
         // §Registries). For the 8 surfaces with an existing id table (`biomes.rs`/\n\
         // `tags.rs`) the names are cross-checked at generate time; the other 21 are\n\
         // datapack registries the report cannot cover, so the capture is their source.\n\
         // These are the element metadata; the full per-element NBT content is the\n\
         // sibling `registry_data.rs` table (`SYNCHRONIZED_NBT`), cross-checked against\n\
         // these names at generate time. At runtime the server serves `data` as\n\
         // `Optional.empty()` for accepted vanilla elements and the pre-baked payloads\n\
         // otherwise (`registry_sync::pack_registries`).\n\n",
    );

    out.push_str(
        "/// The 29 `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` entries: each registry key\n\
         /// paired with its element names in ascending registry id order (the wire order).\n",
    );
    out.push_str("pub static SYNCHRONIZED_REGISTRIES: &[(&str, &[&str])] = &[\n");
    for r in registries {
        out.push_str(&render_registry(r));
    }
    out.push_str("];\n");
    out
}

/// One registry's element-name list: `("minecraft:worldgen/biome", &["...", ...])`.
fn render_registry(r: &SynchronizedRegistry) -> String {
    let mut out = String::new();
    out.push_str(&format!("    ({:?}, &[\n", r.key));
    for element in &r.elements {
        out.push_str(&format!("        {:?},\n", element));
    }
    out.push_str("    ]),\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic() {
        let registries = vec![
            SynchronizedRegistry {
                key: "minecraft:worldgen/biome".into(),
                elements: vec![
                    "minecraft:badlands".into(),
                    "minecraft:bamboo_jungle".into(),
                ],
            },
            SynchronizedRegistry {
                key: "minecraft:chat_type".into(),
                elements: vec!["minecraft:chat".into()],
            },
        ];
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"x","jar_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let a = render(&registries, &source);
        let b = render(&registries, &source);
        assert_eq!(a, b);
        // The combined table carries each key and its element list.
        assert!(a.contains("(\"minecraft:worldgen/biome\", &["));
        assert!(a.contains("(\"minecraft:chat_type\", &["));
        assert!(a.contains("\"minecraft:badlands\""));
    }
}
