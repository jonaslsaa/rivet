//! `net.minecraft.data.worldgen.feature` — the seed-42 FEATURES checkpoint's
//! generated-table read surface.
//!
//! The feature data itself lives in the generated registry tables
//! [`rivet_registry::generated::feature_data`] (emitted by
//! `tools/rivet-codegen generate` from `data/feature_data.json`, provenance
//! linked to the live Paper 26.2 load): the generation settings of EVERY
//! overworld possible biome (the full `biomeSource.possibleBiomes()` list in
//! source order — the exact argument Paper's FeatureSorter is built from,
//! `ChunkGenerator.java` 97-100) and the placed/configured feature closure as
//! `RegistryOps` JSON. This module is the thin runtime read API the FEATURES
//! orchestrator will bootstrap from — a single re-export point for the tables,
//! with the committed-file invariants (non-vacuity, full-list coverage, step
//! order, registry identity, exact transitive closure) pinned as tests.
//!
//! Deliberately out of scope (later FEATURES slices): placement modifier
//! dispatch, feature placement bodies, `WorldGenLevel` writes, and the
//! `FeatureSorter` ordering. No `BiomeGenerationSettings` value type is defined
//! here — the generated struct (id + carver names + per-step feature lists) is
//! the data, and `biome::BiomeGenerationSettings` (holder sets + codec) is the
//! runtime value type; they stay distinct.

pub use rivet_registry::generated::feature_data::{
    BIOME_GENERATION_SETTINGS_BY_NAME, CONFIGURED_FEATURE_BY_NAME, DECORATION_STEP_COUNT,
    MOB_SPAWN_SETTINGS_BY_NAME, PLACED_FEATURE_BY_NAME,
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
    /// Mirrors `rivet-codegen`'s `collect_bare_strings` (same walk and
    /// `minecraft:` prefix filter — a non-`minecraft:` string can never name a
    /// table entry, so the filter is a pure narrowing and membership
    /// disambiguation still drops the block-state `Name`s).
    fn collect_bare_strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) if s.starts_with("minecraft:") => out.push(s),
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

    /// Hostile regression for the configured-only-referenced placed feature:
    /// `minecraft:oak_checked` is a placed feature referenced by configured
    /// features (`minecraft:trees_water`, and in the full closure also
    /// `trees_savanna` / `trees_windswept_hills`) under the holder key
    /// `default`, never directly in any biome step list. The committed tables
    /// keep it, and this test pins that the holder-key resolution rule used by
    /// `committed_tables_keep_the_exact_transitive_closure` rejects a generated
    /// file that dropped it: removing `oak_checked` from the placed table leaves
    /// `trees_water`'s holder reference dangling.
    #[test]
    fn hostile_configured_only_placed_ref_is_caught() {
        let trees_water = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:trees_water")
            .expect("fixture must carry `minecraft:trees_water`");
        let v: Value = serde_json::from_str(trees_water.json).unwrap();
        let mut holder_refs = Vec::new();
        collect_feature_holder_refs(&v, &mut holder_refs);
        let holder_refs: HashSet<&str> = holder_refs.into_iter().collect();
        assert_eq!(
            holder_refs,
            ["minecraft:oak_checked", "minecraft:fancy_oak_checked"]
                .into_iter()
                .collect::<HashSet<_>>()
        );
        assert!(
            !CONFIGURED_FEATURE_BY_NAME.contains_key("minecraft:oak_checked"),
            "`minecraft:oak_checked` must not be a configured feature (that would mask the edge)"
        );

        // Simulate the hand-edit: drop the placed entry the fixture references.
        let mut placed: HashSet<&str> = PLACED_FEATURE_BY_NAME.keys().copied().collect();
        placed.remove("minecraft:oak_checked");
        assert!(
            !holder_refs
                .iter()
                .all(|r| placed.contains(r) || CONFIGURED_FEATURE_BY_NAME.contains_key(r)),
            "dropping `minecraft:oak_checked` from the placed table must leave a dangling holder \
             reference"
        );
    }

    /// All bare-string values under a *feature-holder key* (`feature`/`default`)
    /// inside a `RegistryOps`-encoded JSON — the positions the extractor encodes
    /// as registry-reference holders. Mirrors `rivet-codegen`'s
    /// `collect_feature_holder_refs`; block-state `Name`s, tag strings, and
    /// feature `type` dispatch keys are never collected.
    fn collect_feature_holder_refs<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        fn walk<'a>(elem: &'a Value, key: &str, out: &mut Vec<&'a str>) {
            match elem {
                Value::String(s) if matches!(key, "feature" | "default") => out.push(s),
                Value::Array(items) => {
                    for item in items {
                        walk(item, key, out);
                    }
                }
                Value::Object(map) => {
                    for (k, v) in map {
                        walk(v, k, out);
                    }
                }
                _ => {}
            }
        }
        walk(v, "", out);
    }

    /// The committed tables are non-vacuous and carry the pinned counts: every
    /// overworld possible biome (55 — the full list Paper's FeatureSorter is
    /// built from, not just the five seed-42-reachable biomes) and the complete
    /// transitive placed/configured-feature closure.
    #[test]
    fn tables_are_non_vacuous_with_pinned_counts() {
        assert_eq!(DECORATION_STEP_COUNT, 11);
        assert_eq!(BIOME_GENERATION_SETTINGS_BY_NAME.len(), 55);
        assert_eq!(PLACED_FEATURE_BY_NAME.len(), 203);
        assert_eq!(CONFIGURED_FEATURE_BY_NAME.len(), 170);
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

    /// Full-list coverage: the previously-missing full-list head
    /// (`minecraft:mushroom_fields`, source index 0 — the first biome that used
    /// to fail typed `SettingsNotGenerated`) and the other formerly out-of-scope
    /// biomes now resolve. The five seed-42-reachable biomes remain a subset of
    /// the full possible list.
    #[test]
    fn full_possible_list_is_covered() {
        // The head + the tail of the pinned `POSSIBLE_BIOMES_ORDER` (source
        // order — order is semantics because it fixes FeatureSorter's global
        // feature indices).
        for name in [
            "minecraft:mushroom_fields",
            "minecraft:deep_frozen_ocean",
            "minecraft:sulfur_caves",
            "minecraft:deep_dark",
        ] {
            assert!(
                BIOME_GENERATION_SETTINGS_BY_NAME.contains_key(name),
                "full possible list must carry `{name}`"
            );
        }
        // The five reachable seed-42 biomes are a subset of the 55.
        for name in [
            "minecraft:beach",
            "minecraft:dark_forest",
            "minecraft:lush_caves",
            "minecraft:ocean",
            "minecraft:river",
        ] {
            assert!(
                BIOME_GENERATION_SETTINGS_BY_NAME.contains_key(name),
                "reachable biome `{name}` must be present"
            );
        }
    }

    /// The mob-spawn table mirrors the generation-settings table: same key set
    /// (EVERY possible biome), and the two biomes the SPAWN seam's acceptance/
    /// refusal paths concretely depend on are pinned — `river` (seed-42's center
    /// biome) has an EMPTY CREATURE list (zero entities is correct), `beach`
    /// carries a turtle spawner (the non-empty refusal path).
    #[test]
    fn mob_spawn_settings_cover_possible_biomes() {
        assert_eq!(MOB_SPAWN_SETTINGS_BY_NAME.len(), 55);
        // Same key set as the generation settings.
        for (name, _) in MOB_SPAWN_SETTINGS_BY_NAME.entries() {
            assert!(
                BIOME_GENERATION_SETTINGS_BY_NAME.contains_key(*name),
                "mob settings carry `{name}` not in generation settings"
            );
        }
        for (name, _) in BIOME_GENERATION_SETTINGS_BY_NAME.entries() {
            assert!(
                MOB_SPAWN_SETTINGS_BY_NAME.contains_key(*name),
                "generation settings carry `{name}` not in mob settings"
            );
        }
        // seed-42 center biome `river`: empty CREATURE list, 0.1 probability.
        let river = MOB_SPAWN_SETTINGS_BY_NAME.get("minecraft:river").unwrap();
        let _ = river.creature_probability; // presence sanity
        assert_eq!(river.creature.len(), 0, "river CREATURE list must be empty");
        assert_eq!(river.creature_probability, 0.1);
        // `beach`: one turtle spawner (the non-empty refusal path), ordered.
        let beach = MOB_SPAWN_SETTINGS_BY_NAME.get("minecraft:beach").unwrap();
        assert_eq!(beach.creature.len(), 1);
        assert_eq!(beach.creature[0].ty, "minecraft:turtle");
        assert_eq!(beach.creature[0].min, 2);
        assert_eq!(beach.creature[0].max, 5);
        assert_eq!(beach.creature[0].weight, 5);
        // Every non-empty CREATURE entry has min <= max (the `SpawnerData`
        // codec invariant the extractor's live load preserves).
        for (name, m) in MOB_SPAWN_SETTINGS_BY_NAME.entries() {
            for s in m.creature {
                assert!(
                    s.min <= s.max,
                    "biome {name} creature {} min {} > max {}",
                    s.ty,
                    s.min,
                    s.max
                );
            }
        }
    }

    /// The per-step lists keep the `GenerationStep.Decoration` ordinal order and
    /// the builder's holder-set order. Pinned against the live Paper 26.2 seed-42
    /// load (the fixture), which is itself drift-checked by `probe-feature-data`.
    ///
    /// Only the truly universal invariants are asserted for EVERY biome: step 0
    /// (raw_generation) and step 5 (underground_ores) are universally empty, step
    /// 10 (top_layer_modification) is universally `[freeze_top_layer]`, and the
    /// three carvers are shared. Steps 1/2/3 vary across the full list (e.g.
    /// `deep_dark` step 1 is empty, `sulfur_caves` step 1 has four entries, the
    /// frozen oceans' step 2 adds icebergs, swamp/desert step 3 adds fossils) so
    /// those per-biome contents are pinned as spot-checks below — never a shared
    /// assertion.
    #[test]
    fn decoration_step_order_and_content_are_pinned() {
        for (name, b) in BIOME_GENERATION_SETTINGS_BY_NAME.entries() {
            let name = *name;
            assert!(
                b.features[0].is_empty(),
                "biome {name} step 0 (raw_generation) must be empty"
            );
            assert!(
                b.features[5].is_empty(),
                "biome {name} step 5 (underground_ores) must be empty"
            );
            assert_eq!(
                b.features[10],
                ["minecraft:freeze_top_layer"],
                "biome {name} step 10"
            );
        }

        // Universal step-1/2/3/9 base content is NOT shared — only the direct
        // seed-42 reachable biomes carry the canonical `[lava lakes]` step 1 and
        // `[amethyst_geode]` step 2. The full-list head, tail, and variation
        // biomes are pinned individually below.
        let lava_lakes = [
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ];
        let lush = BIOME_GENERATION_SETTINGS_BY_NAME
            .get("minecraft:lush_caves")
            .unwrap();
        assert_eq!(lush.features[1], lava_lakes, "lush_caves step 1");
        assert_eq!(
            lush.features[2],
            ["minecraft:amethyst_geode"],
            "lush_caves step 2"
        );
        assert_eq!(
            lush.features[3],
            ["minecraft:monster_room", "minecraft:monster_room_deep"],
            "lush_caves step 3"
        );
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

    /// Per-biome step-content spot-checks across the full list's variation
    /// surface — the cases that prove the list is genuinely full and faithful
    /// (not the five-biome subset duplicated):
    ///   * `mushroom_fields` (source index 0, the former blocker) — step 9 is
    ///     the mushroom-island vegetation, not the plains/trees default;
    ///   * `deep_frozen_ocean` — step 2 adds `iceberg_packed`/`iceberg_blue`
    ///     before the geode, step 4 carries `blue_ice`, step 9 is ocean-ish;
    ///   * `deep_dark` — step 1 is EMPTY (no lava lakes) and step 7 carries the
    ///     sculk features;
    ///   * `sulfur_caves` — step 1 has four entries (adds the sulfur springs),
    ///     step 7 carries the sulfur spikes;
    ///   * `swamp` — step 3 adds `fossil_upper`/`fossil_lower` (fossils!) and
    ///     step 6 drops `ore_gold_extra`/`disk_sand` in favor of `disk_clay`;
    ///   * `desert` — step 3 carries the same fossils (count 4) and step 4
    ///     carries `desert_well`.
    #[test]
    fn full_list_step_variation_is_pinned() {
        let get = |name: &str| BIOME_GENERATION_SETTINGS_BY_NAME.get(name).unwrap();
        let mushroom = get("minecraft:mushroom_fields");
        assert_eq!(
            mushroom.features[1],
            [
                "minecraft:lake_lava_underground",
                "minecraft:lake_lava_surface"
            ]
        );
        assert_eq!(
            mushroom.features[9][1..3],
            [
                "minecraft:mushroom_island_vegetation",
                "minecraft:brown_mushroom_taiga"
            ]
        );
        assert!(!mushroom.features[9].contains(&"minecraft:trees_water"));

        let df_ocean = get("minecraft:deep_frozen_ocean");
        assert_eq!(
            df_ocean.features[2],
            [
                "minecraft:iceberg_packed",
                "minecraft:iceberg_blue",
                "minecraft:amethyst_geode"
            ]
        );
        assert_eq!(df_ocean.features[4], ["minecraft:blue_ice"]);

        let deep_dark = get("minecraft:deep_dark");
        assert!(
            deep_dark.features[1].is_empty(),
            "deep_dark has no lava lakes"
        );
        assert_eq!(
            deep_dark.features[7],
            ["minecraft:sculk_vein", "minecraft:sculk_patch_deep_dark"]
        );

        let sulfur = get("minecraft:sulfur_caves");
        assert_eq!(
            sulfur.features[1].len(),
            4,
            "sulfur_caves step 1 has the sulfur springs"
        );
        assert_eq!(
            sulfur.features[7],
            ["minecraft:sulfur_spike_cluster", "minecraft:sulfur_spike"]
        );

        let swamp = get("minecraft:swamp");
        assert_eq!(
            swamp.features[3],
            [
                "minecraft:fossil_upper",
                "minecraft:fossil_lower",
                "minecraft:monster_room",
                "minecraft:monster_room_deep"
            ]
        );
        assert!(
            !swamp.features[6].contains(&"minecraft:disk_sand"),
            "swamp step 6 swaps disk_sand for disk_clay"
        );

        let desert = get("minecraft:desert");
        assert_eq!(
            desert.features[3],
            [
                "minecraft:fossil_upper",
                "minecraft:fossil_lower",
                "minecraft:monster_room",
                "minecraft:monster_room_deep"
            ]
        );
        assert_eq!(desert.features[4], ["minecraft:desert_well"]);
    }

    /// Registry identity survives the full-list render: the dense full-registry
    /// ids (not contiguous within the subset) and the biome ids are exact —
    /// including the five seed-42-reachable biomes and the full-list head/tail
    /// that the five-biome subset never carried.
    #[test]
    fn registry_identity_is_preserved() {
        let id = |name: &str| BIOME_GENERATION_SETTINGS_BY_NAME.get(name).unwrap().id;
        assert_eq!(id("minecraft:beach"), 3);
        assert_eq!(id("minecraft:dark_forest"), 8);
        assert_eq!(id("minecraft:lush_caves"), 30);
        assert_eq!(id("minecraft:ocean"), 35);
        assert_eq!(id("minecraft:river"), 41);
        assert_eq!(id("minecraft:deep_dark"), 10);
        assert_eq!(id("minecraft:deep_frozen_ocean"), 11);
        assert_eq!(id("minecraft:mushroom_fields"), 33);
        assert_eq!(id("minecraft:sulfur_caves"), 53);

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

    /// The committed tables keep the exact transitive closure — the runtime
    /// mirror of the codegen fixture gate's three structurally explicit checks.
    /// Every reference resolves (typed: a placed feature's `json.feature` must
    /// be a *configured* feature specifically; a configured feature's holder-key
    /// ref may be placed or configured; a biome step ref must be placed) and
    /// every entry is reachable from the biome step lists through a FORWARD
    /// fixpoint seeded ONLY from those lists — a dead or dangling entry in a
    /// hand-edited generated file fails here before any FEATURES pass runs, and
    /// a disconnected mutually-referencing component no biome can reach is never
    /// self-justifying.
    #[test]
    fn committed_tables_keep_the_exact_transitive_closure() {
        // Seed ONLY from the biome step lists (a stale orphan must never be
        // self-justifying by seeding from every table entry).
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

        loop {
            let mut grew = false;

            // Reachable placed -> their configured-feature ref (typed: must
            // resolve in the CONFIGURED table — a placed feature can never
            // reference a placed feature).
            for name in placed_reachable.clone() {
                let p = &PLACED_FEATURE_BY_NAME[&name];
                let v: Value = serde_json::from_str(p.json)
                    .unwrap_or_else(|e| panic!("placed `{name}` json is not parseable: {e}"));
                let feature = v["feature"]
                    .as_str()
                    .unwrap_or_else(|| panic!("placed `{name}` json.feature is not a string"));
                assert!(
                    CONFIGURED_FEATURE_BY_NAME.contains_key(feature),
                    "placed `{name}` references configured `{feature}` that is absent from the \
                     tables (a dangling or mis-typed holder reference)"
                );
                grew |= configured_reachable.insert(feature.to_string());
            }

            // Reachable configured -> their bare-string refs (by registry
            // membership; a feature `type` key sharing a feature's name is a
            // real reachability edge at capture time).
            for name in configured_reachable.clone() {
                let c = &CONFIGURED_FEATURE_BY_NAME[&name];
                let v: Value = serde_json::from_str(c.json)
                    .unwrap_or_else(|e| panic!("configured `{name}` json is not parseable: {e}"));
                let mut bare = Vec::new();
                collect_bare_strings(&v, &mut bare);
                for s in bare {
                    if PLACED_FEATURE_BY_NAME.contains_key(s) {
                        grew |= placed_reachable.insert(s.to_string());
                    }
                    if CONFIGURED_FEATURE_BY_NAME.contains_key(s) {
                        grew |= configured_reachable.insert(s.to_string());
                    }
                }
            }

            if !grew {
                break;
            }
        }

        // Dangling holder-key refs (`feature`/`default`) inside configured JSONs
        // — the extractor's encoded registry references — must resolve in either
        // table (configured holders legitimately point at both placed and
        // configured features: `trees_water`→`oak_checked` is placed,
        // `moss_patch`→`moss_vegetation` is configured).
        for (name, c) in &CONFIGURED_FEATURE_BY_NAME {
            let v: Value = serde_json::from_str(c.json)
                .unwrap_or_else(|e| panic!("configured `{name}` json is not parseable: {e}"));
            let mut holder = Vec::new();
            collect_feature_holder_refs(&v, &mut holder);
            for s in holder {
                assert!(
                    PLACED_FEATURE_BY_NAME.contains_key(s)
                        || CONFIGURED_FEATURE_BY_NAME.contains_key(s),
                    "configured `{name}` holder reference `{s}` is absent from the tables"
                );
            }
        }

        // No dead entries: every table entry must be reachable from the biome
        // step lists.
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
