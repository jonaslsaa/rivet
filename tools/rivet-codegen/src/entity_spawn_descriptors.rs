//! Strict validation and Rust codegen for Paper 26.2 CREATURE entity spawn
//! descriptors (`data/entity_spawn_descriptors.json`, issue #685).
//!
//! Consumes the committed fixture produced by `extract-entity-spawn-descriptors`,
//! enforces strict schema (`#[serde(deny_unknown_fields)]`), verifies provenance
//! against the sibling manifest, cross-checks dense `minecraft:entity_type` IDs
//! against `data/reports/registries.json`, cross-checks exact reachable CREATURE
//! membership against `data/feature_data.json` `mob_settings`, and renders the
//! dependency-safe primitive Rust table into
//! `crates/rivet-registry/src/generated/entity_spawn_descriptors.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::extract_entity_spawn_descriptors::{GENERATOR_NAME, PAPER_PIN};
use crate::reports::{
    PINNED_MINECRAFT_VERSION, PINNED_PROTOCOL_VERSION, PINNED_WORLD_VERSION, SourceProvenance,
};

pub(crate) const EXPECTED_FORMAT: u32 = 1;
pub(crate) const EXPECTED_POSSIBLE_BIOME_COUNT: usize = 55;
pub(crate) const EXPECTED_CREATURE_DESCRIPTOR_COUNT: usize = 19;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntitySpawnDescriptorsFile {
    pub(crate) format: u32,
    pub(crate) generator: String,
    pub(crate) paper: String,
    pub(crate) minecraft_version: String,
    pub(crate) protocol_version: u32,
    pub(crate) world_version: u32,
    pub(crate) possible_biome_count: usize,
    pub(crate) descriptors: Vec<EntitySpawnDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntitySpawnDescriptor {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) factory: String,
    pub(crate) can_summon: bool,
    pub(crate) allowed_in_peaceful: bool,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) spawn_dimensions_scale: f32,
    pub(crate) fire_immune: bool,
    pub(crate) immune_to_tag: String,
    pub(crate) placement: String,
    pub(crate) heightmap: String,
    pub(crate) spawn_predicate: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureManifest {
    generator: String,
    source: SourceProvenance,
    file: FixtureFile,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    bytes: u64,
    sha256: String,
}

/// Read + validate `data/entity_spawn_descriptors.json` and emit
/// `crates/rivet-registry/src/generated/entity_spawn_descriptors.rs`.
pub(crate) fn run(input_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let input = match input_flag {
        Some(p) => p.to_path_buf(),
        None => crate::extract_entity_spawn_descriptors::default_output(&repo_root),
    };
    let output_dir = match output_flag {
        Some(p) => p.to_path_buf(),
        None => crate::generate::default_output(&repo_root),
    };
    let data = validate(&repo_root, &input)?;
    let rendered = render(&data)?;
    fs::create_dir_all(&output_dir).with_context(|| format!("create {}", output_dir.display()))?;
    let out_file = output_dir.join("entity_spawn_descriptors.rs");
    fs::write(&out_file, rendered).with_context(|| format!("write {}", out_file.display()))?;
    println!(
        "Wrote {} CREATURE entity spawn descriptors -> {}",
        data.descriptors.len(),
        out_file.display()
    );
    Ok(())
}

/// Validate `fixture_path` (and its sibling `.manifest.json`) against the
/// repo's committed `reports/registries.json` and `feature_data.json`.
pub(crate) fn validate(
    repo_root: &Path,
    fixture_path: &Path,
) -> Result<EntitySpawnDescriptorsFile> {
    let registries_path = crate::registries::default_input(repo_root);
    let feature_data_path = crate::extract_feature_data::default_output(repo_root);
    validate_with_paths(fixture_path, &registries_path, &feature_data_path)
}

pub(crate) fn validate_with_paths(
    fixture_path: &Path,
    registries_path: &Path,
    feature_data_path: &Path,
) -> Result<EntitySpawnDescriptorsFile> {
    let bytes =
        fs::read(fixture_path).with_context(|| format!("read {}", fixture_path.display()))?;
    let manifest_path = fixture_path.with_extension("manifest.json");
    let manifest_raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: FixtureManifest = serde_json::from_str(&manifest_raw)
        .with_context(|| format!("parse {}", manifest_path.display()))?;

    ensure!(
        manifest.generator == GENERATOR_NAME,
        "manifest generator {:?} != expected {:?}",
        manifest.generator,
        GENERATOR_NAME
    );
    crate::reports::verify_pinned_source(&manifest.source)?;
    ensure!(
        manifest.file.bytes == bytes.len() as u64,
        "fixture byte count drift: manifest records {} bytes, file has {} bytes",
        manifest.file.bytes,
        bytes.len()
    );
    let actual_sha = crate::reports::sha256_hex(&bytes);
    ensure!(
        manifest.file.sha256 == actual_sha,
        "fixture sha256 drift: manifest records {}, file hashes to {actual_sha}",
        manifest.file.sha256
    );

    let registries_raw = fs::read_to_string(registries_path)
        .with_context(|| format!("read {}", registries_path.display()))?;
    let feature_data_raw = fs::read_to_string(feature_data_path)
        .with_context(|| format!("read {}", feature_data_path.display()))?;

    validate_payload(&bytes, &registries_raw, &feature_data_raw)
}

/// Validate the raw fixture JSON bytes against the raw `registries.json` and
/// `feature_data.json` contents (separated from filesystem I/O for hermetic
/// negative testing).
pub(crate) fn validate_payload(
    fixture_bytes: &[u8],
    registries_json: &str,
    feature_data_json: &str,
) -> Result<EntitySpawnDescriptorsFile> {
    let parsed: EntitySpawnDescriptorsFile = serde_json::from_slice(fixture_bytes)
        .context("parse entity_spawn_descriptors.json (strict schema)")?;

    ensure!(
        parsed.format == EXPECTED_FORMAT,
        "unsupported entity spawn descriptors format {}, expected {EXPECTED_FORMAT}",
        parsed.format
    );
    ensure!(
        parsed.generator == GENERATOR_NAME,
        "unexpected generator {:?}, expected {:?}",
        parsed.generator,
        GENERATOR_NAME
    );
    ensure!(
        parsed.paper == PAPER_PIN,
        "paper pin drift: got {:?}, expected {:?}",
        parsed.paper,
        PAPER_PIN
    );
    ensure!(
        parsed.minecraft_version == PINNED_MINECRAFT_VERSION,
        "minecraft_version drift: got {:?}, expected {:?}",
        parsed.minecraft_version,
        PINNED_MINECRAFT_VERSION
    );
    ensure!(
        parsed.protocol_version == PINNED_PROTOCOL_VERSION,
        "protocol_version drift: got {}, expected {}",
        parsed.protocol_version,
        PINNED_PROTOCOL_VERSION
    );
    ensure!(
        parsed.world_version == PINNED_WORLD_VERSION,
        "world_version drift: got {}, expected {}",
        parsed.world_version,
        PINNED_WORLD_VERSION
    );
    ensure!(
        parsed.possible_biome_count == EXPECTED_POSSIBLE_BIOME_COUNT,
        "possible_biome_count drift: got {}, expected {EXPECTED_POSSIBLE_BIOME_COUNT}",
        parsed.possible_biome_count
    );

    // Parse & verify dense `minecraft:entity_type` registry from reports/registries.json.
    let entity_registry = parse_dense_entity_type_registry(registries_json)?;

    // Parse exact reachable CREATURE entity names from feature_data.json `mob_settings`.
    let expected_creatures = parse_reachable_creatures_from_feature_data(feature_data_json)?;
    ensure!(
        expected_creatures.len() == EXPECTED_CREATURE_DESCRIPTOR_COUNT,
        "feature_data.json reachable CREATURE count {} != expected {EXPECTED_CREATURE_DESCRIPTOR_COUNT}",
        expected_creatures.len()
    );

    let mut seen_ids = BTreeSet::new();
    let mut seen_names = BTreeSet::new();
    let mut prev_id: Option<u32> = None;

    for desc in &parsed.descriptors {
        ensure!(
            u16::try_from(desc.id).is_ok(),
            "descriptor id {} overflows u16 for {}",
            desc.id,
            desc.name
        );
        if !seen_ids.insert(desc.id) {
            bail!("duplicate entity spawn descriptor id {}", desc.id);
        }
        if !seen_names.insert(desc.name.clone()) {
            bail!("duplicate entity spawn descriptor name {:?}", desc.name);
        }
        if let Some(prev) = prev_id {
            ensure!(
                desc.id > prev,
                "entity spawn descriptors must be strictly sorted by registry id ({} followed by {})",
                prev,
                desc.id
            );
        }
        prev_id = Some(desc.id);

        ensure!(
            desc.name.starts_with("minecraft:") && desc.name.len() > "minecraft:".len(),
            "invalid namespaced entity name {:?}",
            desc.name
        );
        ensure!(
            desc.category == "creature",
            "entity {:?} has category {:?}, expected \"creature\"",
            desc.name,
            desc.category
        );
        ensure!(
            !desc.factory.is_empty() && desc.factory.contains("::"),
            "entity {:?} has malformed factory symbol {:?}",
            desc.name,
            desc.factory
        );
        ensure!(
            desc.width.is_finite() && desc.width > 0.0,
            "entity {:?} has invalid width {}",
            desc.name,
            desc.width
        );
        ensure!(
            desc.height.is_finite() && desc.height > 0.0,
            "entity {:?} has invalid height {}",
            desc.name,
            desc.height
        );
        ensure!(
            desc.spawn_dimensions_scale.is_finite() && desc.spawn_dimensions_scale > 0.0,
            "entity {:?} has invalid spawn_dimensions_scale {}",
            desc.name,
            desc.spawn_dimensions_scale
        );
        ensure!(
            desc.immune_to_tag.starts_with("minecraft:")
                && desc.immune_to_tag.len() > "minecraft:".len(),
            "entity {:?} has invalid immune_to_tag {:?}",
            desc.name,
            desc.immune_to_tag
        );
        placement_variant(&desc.placement)
            .with_context(|| format!("entity {:?} has unknown placement", desc.name))?;
        heightmap_variant(&desc.heightmap)
            .with_context(|| format!("entity {:?} has unknown heightmap", desc.name))?;
        ensure!(
            !desc.spawn_predicate.is_empty() && desc.spawn_predicate.contains("::"),
            "entity {:?} has malformed spawn_predicate symbol {:?}",
            desc.name,
            desc.spawn_predicate
        );

        let Some(&registry_id) = entity_registry.get(desc.name.as_str()) else {
            bail!(
                "entity {:?} is missing from reports/registries.json minecraft:entity_type",
                desc.name
            );
        };
        ensure!(
            registry_id == desc.id,
            "registry id mismatch for {:?}: descriptor has {}, reports/registries.json has {}",
            desc.name,
            desc.id,
            registry_id
        );
    }

    if seen_names != expected_creatures {
        let missing: Vec<&String> = expected_creatures.difference(&seen_names).collect();
        let extra: Vec<&String> = seen_names.difference(&expected_creatures).collect();
        bail!(
            "descriptor set disagrees with feature_data.json possible-biome CREATURE closure \
             (missing={missing:?}, extra={extra:?})"
        );
    }
    ensure!(
        parsed.descriptors.len() == EXPECTED_CREATURE_DESCRIPTOR_COUNT,
        "descriptor count {} != expected {EXPECTED_CREATURE_DESCRIPTOR_COUNT}",
        parsed.descriptors.len()
    );

    Ok(parsed)
}

fn parse_dense_entity_type_registry(registries_json: &str) -> Result<HashMap<String, u32>> {
    let root: serde_json::Value =
        serde_json::from_str(registries_json).context("parse reports/registries.json")?;
    let entries = root
        .get("minecraft:entity_type")
        .and_then(|v| v.get("entries"))
        .and_then(serde_json::Value::as_object)
        .context("reports/registries.json missing minecraft:entity_type.entries")?;

    let mut by_id: BTreeMap<u32, String> = BTreeMap::new();
    let mut by_name: HashMap<String, u32> = HashMap::with_capacity(entries.len());
    for (name, obj) in entries {
        let id = obj
            .get("protocol_id")
            .and_then(serde_json::Value::as_u64)
            .with_context(|| format!("missing protocol_id for entity_type {name:?}"))?;
        let id = u32::try_from(id)
            .with_context(|| format!("protocol_id {id} overflows u32 for {name:?}"))?;
        if let Some(prev) = by_id.insert(id, name.clone()) {
            bail!("duplicate minecraft:entity_type protocol_id {id} ({prev:?} and {name:?})");
        }
        by_name.insert(name.clone(), id);
    }

    for (expected_id, (&actual_id, name)) in by_id.iter().enumerate() {
        ensure!(
            actual_id as usize == expected_id,
            "minecraft:entity_type registry ids are not dense 0..{} (expected {expected_id}, got {actual_id} for {name:?})",
            by_id.len()
        );
    }
    Ok(by_name)
}

fn parse_reachable_creatures_from_feature_data(
    feature_data_json: &str,
) -> Result<BTreeSet<String>> {
    let root: serde_json::Value =
        serde_json::from_str(feature_data_json).context("parse feature_data.json")?;
    let mob_settings = root
        .get("mob_settings")
        .and_then(serde_json::Value::as_object)
        .context("feature_data.json missing mob_settings object")?;

    let mut names = BTreeSet::new();
    for (biome_name, biome_obj) in mob_settings {
        let creature = biome_obj
            .get("creature")
            .and_then(serde_json::Value::as_array)
            .with_context(|| {
                format!("feature_data.json mob_settings[{biome_name:?}] missing creature array")
            })?;
        for spawner in creature {
            let ty = spawner
                .get("type")
                .and_then(serde_json::Value::as_str)
                .with_context(|| {
                    format!(
                        "feature_data.json mob_settings[{biome_name:?}].creature entry missing type"
                    )
                })?;
            names.insert(ty.to_string());
        }
    }
    Ok(names)
}

fn placement_variant(placement: &str) -> Result<&'static str> {
    match placement {
        "no_restrictions" => Ok("EntitySpawnPlacement::NoRestrictions"),
        "in_water" => Ok("EntitySpawnPlacement::InWater"),
        "in_lava" => Ok("EntitySpawnPlacement::InLava"),
        "on_ground" => Ok("EntitySpawnPlacement::OnGround"),
        other => bail!("unknown placement {other:?}"),
    }
}

fn heightmap_variant(heightmap: &str) -> Result<&'static str> {
    match heightmap {
        "WORLD_SURFACE_WG" => Ok("EntitySpawnHeightmap::WorldSurfaceWg"),
        "WORLD_SURFACE" => Ok("EntitySpawnHeightmap::WorldSurface"),
        "OCEAN_FLOOR_WG" => Ok("EntitySpawnHeightmap::OceanFloorWg"),
        "OCEAN_FLOOR" => Ok("EntitySpawnHeightmap::OceanFloor"),
        "MOTION_BLOCKING" => Ok("EntitySpawnHeightmap::MotionBlocking"),
        "MOTION_BLOCKING_NO_LEAVES" => Ok("EntitySpawnHeightmap::MotionBlockingNoLeaves"),
        other => bail!("unknown heightmap {other:?}"),
    }
}

fn fmt_f32_literal(v: f32) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Render `crates/rivet-registry/src/generated/entity_spawn_descriptors.rs`.
pub(crate) fn render(data: &EntitySpawnDescriptorsFile) -> Result<String> {
    let mut out = String::with_capacity(8192);
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/entity_spawn_descriptors.json\n\
         // (Paper {}; MC {}, protocol {}, world {}).\n\
         // Provenance linked to data/entity_spawn_descriptors.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        data.paper, data.minecraft_version, data.protocol_version, data.world_version
    ));

    out.push_str(
        "/// Placement restriction from Paper `SpawnPlacementTypes`.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n\
         pub enum EntitySpawnPlacement {\n\
             NoRestrictions,\n\
             InWater,\n\
             InLava,\n\
             OnGround,\n\
         }\n\n\
         /// Heightmap surface from Paper `Heightmap.Types`.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n\
         pub enum EntitySpawnHeightmap {\n\
             WorldSurfaceWg,\n\
             WorldSurface,\n\
             OceanFloorWg,\n\
             OceanFloor,\n\
             MotionBlocking,\n\
             MotionBlockingNoLeaves,\n\
         }\n\n\
         /// Primitive spawn descriptor for one Paper `EntityType`.\n\
         #[derive(Debug, Clone, Copy, PartialEq)]\n\
         pub struct EntitySpawnDescriptor {\n\
             pub id: u16,\n\
             pub name: &'static str,\n\
             pub category: &'static str,\n\
             pub factory: &'static str,\n\
             pub can_summon: bool,\n\
             pub allowed_in_peaceful: bool,\n\
             pub width: f32,\n\
             pub height: f32,\n\
             pub spawn_dimensions_scale: f32,\n\
             pub fire_immune: bool,\n\
             pub immune_to_tag: &'static str,\n\
             pub placement: EntitySpawnPlacement,\n\
             pub heightmap: EntitySpawnHeightmap,\n\
             pub spawn_predicate: &'static str,\n\
         }\n\n\
         /// All 19 `MobCategory.CREATURE` entity types reachable from overworld\n\
         /// `possibleBiomes()` `MobSpawnSettings`, sorted by dense `minecraft:entity_type` id.\n\
         pub const CREATURE_ENTITY_SPAWN_DESCRIPTORS: &[EntitySpawnDescriptor] = &[\n",
    );

    for desc in &data.descriptors {
        let placement = placement_variant(&desc.placement)?;
        let heightmap = heightmap_variant(&desc.heightmap)?;
        out.push_str(&format!(
            "    EntitySpawnDescriptor {{\n\
             \x20       id: {},\n\
             \x20       name: {:?},\n\
             \x20       category: {:?},\n\
             \x20       factory: {:?},\n\
             \x20       can_summon: {},\n\
             \x20       allowed_in_peaceful: {},\n\
             \x20       width: {},\n\
             \x20       height: {},\n\
             \x20       spawn_dimensions_scale: {},\n\
             \x20       fire_immune: {},\n\
             \x20       immune_to_tag: {:?},\n\
             \x20       placement: {placement},\n\
             \x20       heightmap: {heightmap},\n\
             \x20       spawn_predicate: {:?},\n\
             \x20   }},\n",
            desc.id,
            desc.name,
            desc.category,
            desc.factory,
            desc.can_summon,
            desc.allowed_in_peaceful,
            fmt_f32_literal(desc.width),
            fmt_f32_literal(desc.height),
            fmt_f32_literal(desc.spawn_dimensions_scale),
            desc.fire_immune,
            desc.immune_to_tag,
            desc.spawn_predicate,
        ));
    }

    out.push_str(
        "];\n\n\
         /// Look up a reachable CREATURE spawn descriptor by dense `minecraft:entity_type` id.\n\
         pub fn creature_spawn_descriptor_by_id(id: u16) -> Option<&'static EntitySpawnDescriptor> {\n\
             let idx = CREATURE_ENTITY_SPAWN_DESCRIPTORS\n\
                 .binary_search_by_key(&id, |d| d.id)\n\
                 .ok()?;\n\
             Some(&CREATURE_ENTITY_SPAWN_DESCRIPTORS[idx])\n\
         }\n\n\
         /// Look up a reachable CREATURE spawn descriptor by namespaced entity id.\n\
         pub fn creature_spawn_descriptor_by_name(name: &str) -> Option<&'static EntitySpawnDescriptor> {\n\
             CREATURE_ENTITY_SPAWN_DESCRIPTORS\n\
                 .iter()\n\
                 .find(|d| d.name == name)\n\
         }\n",
    );

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_real_inputs() -> (Vec<u8>, String, String) {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let fixture = fs::read(crate::extract_entity_spawn_descriptors::default_output(
            &repo_root,
        ))
        .unwrap();
        let registries = fs::read_to_string(crate::registries::default_input(&repo_root)).unwrap();
        let feature_data =
            fs::read_to_string(crate::extract_feature_data::default_output(&repo_root)).unwrap();
        (fixture, registries, feature_data)
    }

    #[test]
    fn committed_fixture_validates_and_matches_cross_checks() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let path = crate::extract_entity_spawn_descriptors::default_output(&repo_root);
        let parsed = validate(&repo_root, &path).unwrap();
        assert_eq!(parsed.descriptors.len(), EXPECTED_CREATURE_DESCRIPTOR_COUNT);
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["unexpected_top_field"] = serde_json::json!(true);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("unknown field"),
            "expected unknown field error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_unknown_descriptor_field() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["descriptors"][0]["extra_key"] = serde_json::json!(123);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("unknown field"),
            "expected unknown field error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_schema_version_drift() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["format"] = serde_json::json!(2);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("format"),
            "expected format error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_paper_pin_drift() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["paper"] = serde_json::json!("26.2-DEV-main@0000000");
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("paper pin drift"),
            "expected paper pin drift error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_duplicate_descriptor_entry() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        let dup = v["descriptors"][0].clone();
        v["descriptors"].as_array_mut().unwrap().push(dup);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("duplicate"),
            "expected duplicate error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_missing_descriptor_entry() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["descriptors"].as_array_mut().unwrap().pop();
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("disagrees with feature_data.json"),
            "expected missing entry error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_out_of_order_descriptor_ids() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        let arr = v["descriptors"].as_array_mut().unwrap();
        arr.swap(0, 1);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("strictly sorted"),
            "expected sort order error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_missing_registry_entry() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        v["descriptors"][0]["name"] = serde_json::json!("minecraft:nonexistent_mob");
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("missing from reports/registries.json"),
            "expected missing registry entry error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_registry_id_disagreement() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut v: serde_json::Value = serde_json::from_slice(&fixture).unwrap();
        // Change armadillo's id from 4 to 5 (still strictly less than camel's 19).
        v["descriptors"][0]["id"] = serde_json::json!(5);
        let err = validate_payload(&serde_json::to_vec(&v).unwrap(), &registries, &feature_data)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("registry id mismatch"),
            "expected registry id mismatch error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_non_dense_entity_registry() {
        let (fixture, registries, feature_data) = load_real_inputs();
        let mut reg: serde_json::Value = serde_json::from_str(&registries).unwrap();
        let entries = reg["minecraft:entity_type"]["entries"]
            .as_object_mut()
            .unwrap();
        // Remove id 0 to break density 0..N.
        let zero_key = entries
            .iter()
            .find(|(_, v)| v["protocol_id"].as_u64() == Some(0))
            .map(|(k, _)| k.clone())
            .unwrap();
        entries.remove(&zero_key);
        let err = validate_payload(
            &fixture,
            &serde_json::to_string(&reg).unwrap(),
            &feature_data,
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("not dense"),
            "expected non-dense registry error, got: {err:#}"
        );
    }

    #[test]
    fn rejects_manifest_sha_drift() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let orig_fixture = crate::extract_entity_spawn_descriptors::default_output(&repo_root);
        let orig_manifest = orig_fixture.with_extension("manifest.json");
        let tmp_fixture = tmp.path().join("entity_spawn_descriptors.json");
        let tmp_manifest = tmp.path().join("entity_spawn_descriptors.manifest.json");
        let mut bytes = fs::read(&orig_fixture).unwrap();
        bytes.push(b'\n');
        fs::write(&tmp_fixture, bytes).unwrap();
        fs::copy(&orig_manifest, &tmp_manifest).unwrap();

        let err = validate(&repo_root, &tmp_fixture).unwrap_err();
        assert!(
            format!("{err:#}").contains("drift"),
            "expected manifest drift error, got: {err:#}"
        );
    }
}
