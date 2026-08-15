//! `net.minecraft.data.worldgen.feature` — the seed-42 FEATURES checkpoint's
//! generated-table read surface.
//!
//! The feature data itself lives in the generated registry tables
//! [`rivet_registry::generated::feature_data`] (emitted by
//! `tools/rivet-codegen generate` from `data/feature_data.json`, provenance
//! linked to the live Paper 26.2 load): the five reachable seed-42 biome
//! generation settings and the placed/configured feature closure as
//! `RegistryOps` JSON. This module is the thin runtime read API the FEATURES
//! orchestrator will bootstrap from — a single re-export point for the tables,
//! with the committed-file invariants (non-vacuity, step order, registry
//! identity, exact transitive closure) pinned as tests.
//!
//! Deliberately out of scope (later FEATURES slices): placement modifier
//! dispatch, feature placement bodies, `WorldGenLevel` writes, and the
//! `FeatureSorter` ordering. No `BiomeGenerationSettings` value type is defined
//! here — the generated struct (id + carver names + per-step feature lists) is
//! the data, and `biome::BiomeGenerationSettings` (holder sets + codec) is the
//! runtime value type; they stay distinct.

pub use rivet_registry::generated::feature_data::{
    BIOME_GENERATION_SETTINGS_BY_NAME, CONFIGURED_FEATURE_BY_NAME, DECORATION_STEP_COUNT,
    PLACED_FEATURE_BY_NAME,
};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::Value;

    use super::*;

    /// All bare-string values inside a `RegistryOps`-encoded JSON. A bare string
    /// is a feature holder reference only when it names a feature in the tables
    /// (registry-membership disambiguation, mirroring the fixture extractor);
    /// block-state `Name` values like `minecraft:oak_log` are in neither table.
    fn collect_bare_strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) => out.push(s),
            Value::Array(items) => {
                for item in items {
                    collect_bare_strings(item, out);
                }
            }
            Value::Object(map) => {
                for value in map.values() {
                    collect_bare_strings(value, out);
                }
            }
            _ => {}
        }
    }

    /// The committed tables are non-vacuous and carry the pinned counts.
    #[test]
    fn tables_are_non_vacuous_with_pinned_counts() {
        assert_eq!(DECORATION_STEP_COUNT, 11);
        assert_eq!(BIOME_GENERATION_SETTINGS_BY_NAME.len(), 5);
        assert_eq!(PLACED_FEATURE_BY_NAME.len(), 72);
        assert_eq!(CONFIGURED_FEATURE_BY_NAME.len(), 70);
        for (name, b) in BIOME_GENERATION_SETTINGS_BY_NAME.entries() {
            let name = *name;
            assert_eq!(
                b.features.len(),
                DECORATION_STEP_COUNT,
                "biome {name} must carry all decoration steps"
            );
            assert_eq!(
                b.carvers,
                [
                    "minecraft:cave",
                    "minecraft:cave_extra_underground",
                    "minecraft:canyon"
                ],
                "biome {name} carver identities"
            );
        }
    }

    /// The per-step lists keep the `GenerationStep.Decoration` ordinal order and
    /// the builder's holder-set order. Pinned against the live Paper 26.2 seed-42
    /// load (the fixture), which is itself drift-checked by `probe-feature-data`.
    #[test]
    fn decoration_step_order_and_content_are_pinned() {
        let shared = [
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ];
        let geode = ["minecraft:amethyst_geode"];
        let monster = ["minecraft:monster_room", "minecraft:monster_room_deep"];
        for (name, b) in BIOME_GENERATION_SETTINGS_BY_NAME.entries() {
            let name = *name;
            assert!(
                b.features[0].is_empty(),
                "biome {name} step 0 must be empty"
            );
            assert_eq!(b.features[1], shared, "biome {name} step 1");
            assert_eq!(b.features[2], geode, "biome {name} step 2");
            assert_eq!(b.features[3], monster, "biome {name} step 3");
            assert_eq!(
                b.features[10],
                ["minecraft:freeze_top_layer"],
                "biome {name} step 10"
            );
        }
        let lush = BIOME_GENERATION_SETTINGS_BY_NAME
            .get("minecraft:lush_caves")
            .unwrap();
        assert_eq!(
            lush.features[9],
            [
                "minecraft:glow_lichen",
                "minecraft:patch_tall_grass_2",
                "minecraft:lush_caves_ceiling_vegetation",
                "minecraft:cave_vines",
                "minecraft:lush_caves_clay",
                "minecraft:lush_caves_vegetation",
                "minecraft:rooted_azalea_tree",
                "minecraft:spore_blossom",
                "minecraft:classic_vines_cave_feature",
            ]
        );
    }

    /// Registry identity survives the subset render: the dense full-registry ids
    /// (not contiguous within the subset) and the biome ids are exact.
    #[test]
    fn registry_identity_is_preserved() {
        let id = |name: &str| BIOME_GENERATION_SETTINGS_BY_NAME.get(name).unwrap().id;
        assert_eq!(id("minecraft:beach"), 3);
        assert_eq!(id("minecraft:dark_forest"), 8);
        assert_eq!(id("minecraft:lush_caves"), 30);
        assert_eq!(id("minecraft:ocean"), 35);
        assert_eq!(id("minecraft:river"), 41);

        assert_eq!(
            PLACED_FEATURE_BY_NAME
                .get("minecraft:amethyst_geode")
                .unwrap()
                .id,
            2
        );
        assert_eq!(
            CONFIGURED_FEATURE_BY_NAME
                .get("minecraft:amethyst_geode")
                .unwrap()
                .id,
            1
        );
    }

    /// The committed tables keep the exact transitive closure: every reference
    /// resolves and every entry is reachable — a dead or dangling entry in a
    /// hand-edited generated file fails here before any FEATURES pass runs.
    #[test]
    fn committed_tables_keep_the_exact_transitive_closure() {
        // placed reachable  = biome step refs ∪ placed refs inside configured JSONs
        // configured reach = placed `json.feature` refs ∪ configured refs inside
        //                    configured JSONs (registry-membership disambiguation)
        let mut placed_reachable: HashSet<String> = HashSet::new();
        for b in BIOME_GENERATION_SETTINGS_BY_NAME.values() {
            for step in b.features {
                for name in *step {
                    assert!(
                        PLACED_FEATURE_BY_NAME.contains_key(name),
                        "biome step references placed `{name}` that is absent from the tables"
                    );
                    placed_reachable.insert(name.to_string());
                }
            }
        }

        let mut configured_reachable: HashSet<String> = HashSet::new();
        for (name, p) in &PLACED_FEATURE_BY_NAME {
            let v: Value = serde_json::from_str(p.json)
                .unwrap_or_else(|e| panic!("placed `{name}` json is not parseable: {e}"));
            let feature = v["feature"]
                .as_str()
                .unwrap_or_else(|| panic!("placed `{name}` json.feature is not a string"));
            assert!(
                CONFIGURED_FEATURE_BY_NAME.contains_key(feature),
                "placed `{name}` references configured `{feature}` that is absent from the tables"
            );
            configured_reachable.insert(feature.to_string());
        }

        for c in CONFIGURED_FEATURE_BY_NAME.values() {
            let v: Value = serde_json::from_str(c.json)
                .unwrap_or_else(|e| panic!("configured json is not parseable: {e}"));
            let mut bare = Vec::new();
            collect_bare_strings(&v, &mut bare);
            for s in bare {
                if PLACED_FEATURE_BY_NAME.contains_key(s) {
                    placed_reachable.insert(s.to_string());
                }
                if CONFIGURED_FEATURE_BY_NAME.contains_key(s) {
                    configured_reachable.insert(s.to_string());
                }
            }
        }

        // No dangling refs (checked inline above) and no dead entries.
        for name in PLACED_FEATURE_BY_NAME.keys() {
            let name = *name;
            assert!(
                placed_reachable.contains(name),
                "placed `{name}` is present but unreachable (stale/dead entry)"
            );
        }
        for name in CONFIGURED_FEATURE_BY_NAME.keys() {
            let name = *name;
            assert!(
                configured_reachable.contains(name),
                "configured `{name}` is present but unreachable (stale/dead entry)"
            );
        }
        assert_eq!(placed_reachable.len(), PLACED_FEATURE_BY_NAME.len());
        assert_eq!(configured_reachable.len(), CONFIGURED_FEATURE_BY_NAME.len());
    }
}
