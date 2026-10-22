use super::super::{
    TerrainAdjustment,
    structure_body::{LiquidSettings, MineshaftKind, PoolAliasBinding},
    structure_settings::SpawnBoundingBox,
};
use super::*;
use rivet_registry::registries::BIOME;

fn access() -> RegistryAccess {
    static ACCESS: std::sync::LazyLock<RegistryAccess> = std::sync::LazyLock::new(|| {
        build_structure_access(
            &crate::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
        )
        .expect("complete actual pinned structure registry")
    });
    ACCESS.clone()
}

#[test]
fn complete_registries_ids_tag_members_and_defaulted_settings_match_live_paper() {
    let access = access();
    let structures = access.lookup(&STRUCTURE).unwrap();
    let sets = access.lookup(&STRUCTURE_SET).unwrap();
    assert_eq!(structures.key_set().len(), 34);
    assert_eq!(sets.key_set().len(), 20);
    let mut tags = 0;
    let mut supported = 0;
    for row in pinned_records().unwrap() {
        let key = record_key(&row).unwrap();
        match row["kind"].as_str().unwrap() {
            "structure" => {
                let actual = structures.get_optional(&key).unwrap();
                assert_eq!(
                    i64::from(structures.get_id(actual)),
                    row["id"].as_i64().unwrap()
                );
                let normalized = Structure::parse(&row["encoded"], &access).unwrap();
                assert_eq!(*actual, normalized, "actual Java codec defaults {key}");
                if key.to_string() == "minecraft:buried_treasure" {
                    assert!(actual.native_definition(&key).is_ok());
                    supported += 1;
                } else {
                    assert!(matches!(
                        actual.native_definition(&key),
                        Err(StructureError::UnsupportedStructure(_))
                    ));
                }
            }
            "structure_set" => {
                let actual = sets.get_optional(&key).unwrap();
                assert_eq!(i64::from(sets.get_id(actual)), row["id"].as_i64().unwrap());
                let normalized = StructureSet::parse(&row["encoded"], &access).unwrap();
                assert_eq!(*actual, normalized, "actual Java set defaults {key}");
                for selected in &actual.structures {
                    let resolved = selected.structure.value(structures);
                    let key = selected.structure.key(structures);
                    assert!(std::ptr::eq(
                        resolved,
                        structures.get_optional(key.identifier()).unwrap()
                    ));
                    assert!(selected.weight > 0);
                }
            }
            "structure_tag" => {
                let tag = structures
                    .get_tag(&TagKey::create(&STRUCTURE, key))
                    .expect("actual bound structure tag");
                let names: Vec<_> = tag
                    .iter()
                    .map(|h| {
                        structures
                            .get_key(structures.by_id(h.0 as i32).unwrap())
                            .unwrap()
                            .to_string()
                    })
                    .collect();
                assert_eq!(serde_json::json!(names), row["members"]);
                tags += 1;
            }
            kind => panic!("unknown captured record {kind}"),
        }
    }
    assert_eq!(tags, 20);
    assert_eq!(supported, 1);
    assert!(
        build_structure_access(&access).is_err(),
        "cannot silently replace holder identity"
    );
}

#[test]
fn settings_and_unsupported_bodies_keep_actual_nontrivial_data() {
    let access = access();
    let registry = access.lookup(&STRUCTURE).unwrap();
    let biomes = access.lookup(&BIOME).unwrap();
    let get = |name| {
        registry
            .get_optional(&Identifier::with_default_namespace(name))
            .unwrap()
    };
    let treasure = get("buried_treasure");
    let names: Vec<_> = treasure
        .settings
        .biomes
        .stream()
        .iter()
        .map(|h| h.key(biomes).identifier().to_string())
        .collect();
    assert_eq!(names, ["minecraft:beach", "minecraft:snowy_beach"]);
    let fortress = get("fortress");
    let monsters = &fortress.settings.spawn_overrides[&crate::entity::MobCategory::Monster];
    assert_eq!(monsters.bounding_box, SpawnBoundingBox::Piece);
    assert_eq!(monsters.spawns.unwrap().len(), 5);
    assert!(matches!(
        get("mineshaft_mesa").body,
        StructureBody::Mineshaft {
            kind: MineshaftKind::Mesa
        }
    ));
    let StructureBody::Jigsaw(trial) = &get("trial_chambers").body else {
        panic!("actual jigsaw");
    };
    assert_eq!(trial.size, 20);
    assert_eq!(trial.dimension_padding.bottom, 10);
    assert_eq!(trial.dimension_padding.top, 10);
    assert_eq!(trial.liquid_settings, LiquidSettings::IgnoreWaterlogging);
    assert_eq!(trial.pool_aliases.len(), 3);
    let PoolAliasBinding::RandomGroup { groups } = &trial.pool_aliases[0] else {
        panic!("source random group");
    };
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].value().len(), 2);
    assert_eq!(
        get("stronghold").settings.terrain_adaptation,
        TerrainAdjustment::Bury
    );
    let sets = access.lookup(&STRUCTURE_SET).unwrap();
    let portals = sets
        .get_optional(&Identifier::with_default_namespace("ruined_portals"))
        .unwrap();
    assert_eq!(portals.structures.len(), 7);
}

#[test]
fn malformed_settings_jigsaw_range_and_unknown_structure_refs_refuse() {
    let access = access();
    let records = pinned_records().unwrap();
    let mut village = records
        .iter()
        .find(|v| v["key"] == "minecraft:village_plains")
        .unwrap()["data"]
        .clone();
    village["max_distance_from_center"] = serde_json::json!(117);
    assert!(
        Structure::parse(&village, &access).is_err(),
        "117+12 exceeds source maximum128"
    );
    village["max_distance_from_center"] = serde_json::json!(116);
    village["biomes"] = serde_json::json!("#minecraft:missing_actual_biome_tag");
    assert!(Structure::parse(&village, &access).is_err());
    let mut set = records
        .iter()
        .find(|v| v["key"] == "minecraft:buried_treasures")
        .unwrap()["data"]
        .clone();
    set["structures"][0]["structure"] = serde_json::json!("minecraft:missing_structure");
    assert!(StructureSet::parse(&set, &access).is_err());
    set["structures"][0]["structure"] = serde_json::json!("minecraft:buried_treasure");
    set["structures"][0]["weight"] = serde_json::json!(0);
    assert!(StructureSet::parse(&set, &access).is_err());
}

#[test]
fn random_pool_aliases_require_nonempty_positive_nonoverflowing_weight_totals() {
    let access = access();
    let records = pinned_records().unwrap();
    let mut trial = records
        .iter()
        .find(|v| v["key"] == "minecraft:trial_chambers")
        .unwrap()["data"]
        .clone();
    for weights in [vec![], vec![0], vec![0, 0], vec![i32::MAX, 1]] {
        trial["pool_aliases"] = serde_json::json!([{
            "type":"minecraft:random", "alias":"minecraft:probe",
            "targets":weights.iter().map(|weight| serde_json::json!({
                "data":"minecraft:target", "weight":weight
            })).collect::<Vec<_>>()
        }]);
        assert!(
            Structure::parse(&trial, &access).is_err(),
            "weights {weights:?}"
        );
    }
    trial["pool_aliases"][0]["targets"] = serde_json::json!([
        {"data":"minecraft:zero", "weight":0},
        {"data":"minecraft:positive", "weight":i32::MAX}
    ]);
    assert!(
        Structure::parse(&trial, &access).is_ok(),
        "zero-weight entries are valid when total is positive"
    );
    trial["type"] = serde_json::json!(":jigsaw");
    trial["pool_aliases"][0]["type"] = serde_json::json!("random");
    assert!(
        Structure::parse(&trial, &access).is_ok(),
        "registry type dispatch normalizes Identifier default namespaces"
    );
}
