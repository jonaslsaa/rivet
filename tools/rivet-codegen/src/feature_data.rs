//! `rivet-codegen` feature-data validation half — consume the committed
//! `data/feature_data.json` fixture (produced by `extract-feature-data`, see
//! [`crate::extract_feature_data`]) and validate its structure, provenance,
//! order, and closure. The generation half that consumes this fixture is
//! [`crate::feature_tables`]; this module is the fixture's contract.
//!
//! The fixture is the seed-42 FEATURES data foundation:
//!
//!   * `possible_biomes` — the FULL overworld `biomeSource.possibleBiomes()`
//!     list in source order: the exact argument Paper's `ChunkGenerator` feeds
//!     `FeatureSorter.buildFeaturesPerStep` (`ChunkGenerator.java` 97-100:
//!     `List.copyOf(biomeSource.possibleBiomes())`). `BiomeSource.
//!     possibleBiomes()` is `collectPossibleBiomes().distinct().collect(
//!     ImmutableSet.toImmutableSet())`, and `MultiNoiseBiomeSource.
//!     collectPossibleBiomes()` is `parameters().values().stream().map(
//!     Pair::getSecond)` — so this is the `OverworldBiomeBuilder.addBiomes`
//!     first-appearance (emission) order. The order is pinned: a reorder
//!     changes FeatureSorter's global feature indices and must fail.
//!   * `reachable_biomes` — the seed-42 biome set that can drive FEATURES
//!     placement into the committed grid {(3,3),(4,3),(3,4),(4,4)} (chunks
//!     1..6, full Y range), sorted by registry id. A subset of
//!     `possible_biomes` (the convergence non-vacuity anchor).
//!   * `biomes` — per-biome `BiomeGenerationSettings` of EVERY possible biome:
//!     `id`, the carver identity names, and the per-step placed-feature name
//!     lists. Step `i` is `GenerationStep.Decoration.values()[i]`
//!     (raw_generation .. top_layer_modification); holder-set order within a
//!     step is the builder's fixed order (part of the decoration semantics).
//!   * `placed_features` / `configured_features` — the transitive closure of
//!     referenced registry entries, keyed by name with the dense registry `id`
//!     and the full `RegistryOps`-encoded JSON (the datapack JSON shape).
//!
//! Validation (all read order-insensitively):
//!   * provenance — the fixture bytes must match the sha256 recorded next to it
//!     in `data/feature_data.manifest.json` (a hand-edited fixture fails);
//!   * structure — every top-level field, the counts, and the per-biome step
//!     arrays;
//!   * order — `possible_biomes` matches the pinned 55-name emission order and
//!     is a superset of `reachable_biomes`; reachable biomes are id-sorted; the
//!     `biomes` key set == the possible-biome set; the per-biome per-step arrays
//!     are pinned (a step-list reorder changes decoration semantics and must
//!     fail);
//!   * closure, three structurally explicit checks (mirrored verbatim by the
//!     runtime committed-table test in `crates/rivet-world/src/data/feature_data.rs`):
//!       - dangling refs: (1) the top-level `feature` of every placed feature
//!         must resolve in `configured_features` — a placed feature can only
//!         reference a configured feature, never a placed one; (2) a bare
//!         string under a *feature-holder key* (`feature`/`default`) inside a
//!         configured feature's JSON must resolve in either table (configured
//!         holders legitimately point at both placed and configured features —
//!         `trees_water`→`oak_checked` is placed, `moss_patch`→`moss_vegetation`
//!         is configured); (3) every biome step-list entry (a direct
//!         placed-feature reference) must resolve in `placed_features`.
//!         Block-state `Name` values, tag strings, and feature `type` dispatch
//!         keys are never holder refs (the holder-key positions are the only
//!         ones the extractor encodes as registry-reference holders). Resolving
//!         by table membership would silently accept a closure that dropped a
//!         referenced entry — e.g. a `random_selector` `default` pointing at a
//!         placed feature the closure omitted (`oak_checked` via `trees_water`).
//!       - dead entries: every table entry must be reachable from the biomes'
//!         step lists through the extractor's fixpoint, which resolves every
//!         bare `minecraft:` string by registry membership (a feature `type`
//!         key that shares a feature's name, like `minecraft:vines`, is a real
//!         reachability edge at capture time — membership, not key position,
//!         disambiguates it). The fixpoint is seeded ONLY from the biome step
//!         lists (not from every table entry) so a stale orphan is never
//!         self-justifying.
//!   * probe — the committed `probe` counts must match the tables, and
//!     `probe.per_biome` must agree element-for-element with the `biomes` step
//!     lists (a stale probe object or a drifted `biomes` table fails). The
//!     probe records `per_step` counts AND the ordered `per_step_names`, so a
//!     within-step reorder also fails (order is semantics).
//!
//! Regeneration is byte-idempotent (the live probe proves a fresh load is
//! byte-identical to the committed fixture).

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// Ground-truth anchors a live Paper 26.2 load must reproduce (kept in sync
/// with `ANCHORS` in `probe_feature_data.rs`).
pub const POSSIBLE_BIOME_COUNT: usize = 55;
pub const REACHABLE_BIOME_COUNT: usize = 5;
pub const PLACED_FEATURE_COUNT: usize = 203;
pub const CONFIGURED_FEATURE_COUNT: usize = 170;

/// The pinned full-overworld possible-biome list in source (emission) order —
/// the exact `List.copyOf(biomeSource.possibleBiomes())` Paper feeds the
/// FeatureSorter (`ChunkGenerator.java` 97-100). The order is part of the
/// decoration semantics (it fixes FeatureSorter's global feature indices).
pub const POSSIBLE_BIOMES_ORDER: &[&str] = &[
    "minecraft:mushroom_fields",
    "minecraft:deep_frozen_ocean",
    "minecraft:frozen_ocean",
    "minecraft:deep_cold_ocean",
    "minecraft:cold_ocean",
    "minecraft:deep_ocean",
    "minecraft:ocean",
    "minecraft:deep_lukewarm_ocean",
    "minecraft:lukewarm_ocean",
    "minecraft:warm_ocean",
    "minecraft:stony_shore",
    "minecraft:swamp",
    "minecraft:mangrove_swamp",
    "minecraft:snowy_slopes",
    "minecraft:snowy_plains",
    "minecraft:snowy_beach",
    "minecraft:windswept_gravelly_hills",
    "minecraft:grove",
    "minecraft:windswept_hills",
    "minecraft:snowy_taiga",
    "minecraft:windswept_forest",
    "minecraft:taiga",
    "minecraft:plains",
    "minecraft:meadow",
    "minecraft:beach",
    "minecraft:forest",
    "minecraft:old_growth_spruce_taiga",
    "minecraft:flower_forest",
    "minecraft:birch_forest",
    "minecraft:dark_forest",
    "minecraft:pale_garden",
    "minecraft:savanna_plateau",
    "minecraft:savanna",
    "minecraft:jungle",
    "minecraft:badlands",
    "minecraft:desert",
    "minecraft:wooded_badlands",
    "minecraft:jagged_peaks",
    "minecraft:stony_peaks",
    "minecraft:frozen_river",
    "minecraft:river",
    "minecraft:ice_spikes",
    "minecraft:old_growth_pine_taiga",
    "minecraft:sunflower_plains",
    "minecraft:old_growth_birch_forest",
    "minecraft:sparse_jungle",
    "minecraft:bamboo_jungle",
    "minecraft:eroded_badlands",
    "minecraft:windswept_savanna",
    "minecraft:cherry_grove",
    "minecraft:frozen_peaks",
    "minecraft:dripstone_caves",
    "minecraft:lush_caves",
    "minecraft:sulfur_caves",
    "minecraft:deep_dark",
];

/// Non-vacuity: the reachable biome set must include the deep `lush_caves`
/// biome AND at least one surface biome (decoration evidence).
pub const REQUIRED_BIOMES: &[&str] = &["minecraft:lush_caves", "minecraft:beach"];

/// The pinned per-biome feature-step counts (step order + holder-set order is
/// part of the decoration semantics; a reorder must fail). Keyed by biome name.
/// One entry per possible biome (the `biomes` table must carry every biome).
pub const PER_BIOME_STEP_COUNTS: &[(&str, &[usize])] = &[
    (
        "minecraft:mushroom_fields",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 6, 1],
    ),
    (
        "minecraft:deep_frozen_ocean",
        &[0, 2, 3, 2, 1, 0, 29, 0, 2, 9, 1],
    ),
    (
        "minecraft:frozen_ocean",
        &[0, 2, 3, 2, 1, 0, 29, 0, 2, 9, 1],
    ),
    (
        "minecraft:deep_cold_ocean",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    ("minecraft:cold_ocean", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:deep_ocean", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:ocean", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    (
        "minecraft:deep_lukewarm_ocean",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    (
        "minecraft:lukewarm_ocean",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    ("minecraft:warm_ocean", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 12, 1]),
    ("minecraft:stony_shore", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 8, 1]),
    ("minecraft:swamp", &[0, 2, 1, 4, 0, 0, 27, 0, 2, 15, 1]),
    (
        "minecraft:mangrove_swamp",
        &[0, 2, 1, 4, 0, 0, 28, 0, 2, 7, 1],
    ),
    (
        "minecraft:snowy_slopes",
        &[0, 2, 1, 2, 0, 0, 30, 1, 3, 2, 1],
    ),
    (
        "minecraft:snowy_plains",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 9, 1],
    ),
    ("minecraft:snowy_beach", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 8, 1]),
    (
        "minecraft:windswept_gravelly_hills",
        &[0, 2, 1, 2, 0, 0, 30, 1, 2, 10, 1],
    ),
    ("minecraft:grove", &[0, 2, 1, 2, 0, 0, 30, 1, 3, 3, 1]),
    (
        "minecraft:windswept_hills",
        &[0, 2, 1, 2, 0, 0, 30, 1, 2, 10, 1],
    ),
    (
        "minecraft:snowy_taiga",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    (
        "minecraft:windswept_forest",
        &[0, 2, 1, 2, 0, 0, 30, 1, 2, 10, 1],
    ),
    ("minecraft:taiga", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:plains", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:meadow", &[0, 2, 1, 2, 0, 0, 30, 1, 2, 6, 1]),
    ("minecraft:beach", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 8, 1]),
    ("minecraft:forest", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    (
        "minecraft:old_growth_spruce_taiga",
        &[0, 2, 2, 2, 0, 0, 29, 0, 2, 14, 1],
    ),
    (
        "minecraft:flower_forest",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 10, 1],
    ),
    (
        "minecraft:birch_forest",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 12, 1],
    ),
    (
        "minecraft:dark_forest",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    ("minecraft:pale_garden", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 9, 1]),
    (
        "minecraft:savanna_plateau",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 10, 1],
    ),
    ("minecraft:savanna", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 10, 1]),
    ("minecraft:jungle", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 12, 1]),
    ("minecraft:badlands", &[0, 2, 1, 2, 0, 0, 30, 0, 2, 10, 1]),
    ("minecraft:desert", &[0, 2, 1, 4, 1, 0, 29, 0, 2, 10, 1]),
    (
        "minecraft:wooded_badlands",
        &[0, 2, 1, 2, 0, 0, 30, 0, 2, 11, 1],
    ),
    (
        "minecraft:jagged_peaks",
        &[0, 2, 1, 2, 0, 0, 30, 1, 3, 1, 1],
    ),
    ("minecraft:stony_peaks", &[0, 2, 1, 2, 0, 0, 30, 1, 2, 1, 1]),
    (
        "minecraft:frozen_river",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 10, 1],
    ),
    ("minecraft:river", &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1]),
    ("minecraft:ice_spikes", &[0, 2, 1, 2, 2, 0, 29, 0, 2, 9, 1]),
    (
        "minecraft:old_growth_pine_taiga",
        &[0, 2, 2, 2, 0, 0, 29, 0, 2, 14, 1],
    ),
    (
        "minecraft:sunflower_plains",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    (
        "minecraft:old_growth_birch_forest",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 12, 1],
    ),
    (
        "minecraft:sparse_jungle",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 11, 1],
    ),
    (
        "minecraft:bamboo_jungle",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 12, 1],
    ),
    (
        "minecraft:eroded_badlands",
        &[0, 2, 1, 2, 0, 0, 30, 0, 2, 10, 1],
    ),
    (
        "minecraft:windswept_savanna",
        &[0, 2, 1, 2, 0, 0, 29, 0, 2, 9, 1],
    ),
    (
        "minecraft:cherry_grove",
        &[0, 2, 1, 2, 0, 0, 30, 1, 2, 5, 1],
    ),
    (
        "minecraft:frozen_peaks",
        &[0, 2, 1, 2, 0, 0, 30, 1, 3, 1, 1],
    ),
    (
        "minecraft:dripstone_caves",
        &[0, 2, 2, 2, 0, 0, 29, 2, 2, 8, 1],
    ),
    ("minecraft:lush_caves", &[0, 2, 1, 2, 0, 0, 30, 0, 2, 9, 1]),
    (
        "minecraft:sulfur_caves",
        &[0, 4, 1, 2, 0, 0, 29, 2, 2, 2, 1],
    ),
    ("minecraft:deep_dark", &[0, 0, 1, 2, 0, 0, 29, 2, 0, 8, 1]),
];

/// Validate the fixture at `input` and return the source provenance from its
/// sibling manifest. Called by `extract-feature-data` (self-validation of a
/// fresh capture) and by the generation slice ([`crate::feature_tables`]); the
/// tests here exercise it against the committed fixture.
pub fn validate(input: &Path) -> Result<SourceProvenance> {
    let json = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", input.display()))?;
    validate_structural(&root)?;
    load_provenance(input)
}

/// Structural + order + closure validation, run purely on the fixture JSON
/// (no live Paper needed). Enforces the pinned live-Paper anchors too — the
/// reachable-biome count, the per-biome step counts, and the placed/configured
/// table counts — so a hand-edited fixture fails here. `pub(crate)` so the
/// generation half ([`crate::feature_tables`]) enforces the full contract on
/// the fixture it renders.
pub(crate) fn validate_structural(root: &Value) -> Result<()> {
    let object = root
        .as_object()
        .context("feature_data.json root must be a JSON object")?;
    ensure!(
        object.get("paper").and_then(Value::as_str) == Some(crate::extract_feature_data::PAPER_PIN),
        "feature_data.json `paper` pin does not match the exact Paper 26.2 source"
    );
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
                | "possible_biomes"
                | "reachable_biomes"
                | "biomes"
                | "mob_settings"
                | "placed_features"
                | "configured_features"
                | "probe"
        ) {
            bail!("feature_data.json has unexpected top-level field `{field}`");
        }
    }

    // Full possible-biome list: the pinned emission (source) order — the exact
    // `List.copyOf(biomeSource.possibleBiomes())` Paper feeds the FeatureSorter.
    // A reorder or a missing biome changes FeatureSorter's global feature
    // indices and must fail.
    let possible = object
        .get("possible_biomes")
        .and_then(Value::as_array)
        .context("feature_data.json is missing `possible_biomes`")?;
    if possible.len() != POSSIBLE_BIOME_COUNT {
        bail!(
            "possible_biomes has {} entries but a live Paper 26.2 load has {POSSIBLE_BIOME_COUNT}",
            possible.len()
        );
    }
    let mut possible_names: Vec<&str> = Vec::with_capacity(possible.len());
    for (i, v) in possible.iter().enumerate() {
        let name = v
            .as_str()
            .with_context(|| format!("possible_biomes[{i}] is not a string"))?;
        crate::registries::validate_name("minecraft:worldgen/biome", name)?;
        possible_names.push(name);
    }
    if possible_names != POSSIBLE_BIOMES_ORDER {
        bail!("possible_biomes order diverges from the pinned Paper 26.2 emission order");
    }

    // Reachable biome list: id-sorted names, non-empty, required biomes present,
    // and a subset of the possible-biome set (the convergence non-vacuity
    // anchor: what seed-42 actually drives through the committed grid).
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
        if !possible_names.contains(&name) {
            bail!(
                "reachable biome `{name}` is absent from `possible_biomes` (reachable must be a subset of possible)"
            );
        }
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

    // Biomes: key set == the possible-biome set (the FULL generation settings of
    // every biome the FeatureSorter can select), each with id, carvers, features.
    let biomes = object
        .get("biomes")
        .and_then(Value::as_object)
        .context("feature_data.json is missing `biomes`")?;
    if biomes.len() != possible_names.len() {
        bail!(
            "`biomes` has {} entries but possible_biomes has {}",
            biomes.len(),
            possible_names.len()
        );
    }
    for name in &possible_names {
        if !biomes.contains_key(*name) {
            bail!("possible biome `{name}` is absent from `biomes`");
        }
    }
    for name in biomes.keys() {
        if !possible_names.contains(&name.as_str()) {
            bail!("biome `{name}` is in `biomes` but absent from `possible_biomes`");
        }
    }
    let mut biome_ids: Vec<u16> = Vec::with_capacity(biomes.len());
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
        let id =
            u16::try_from(id).with_context(|| format!("biome `{name}` id out of u16 range"))?;
        biome_ids.push(id);
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

    // Mob settings: key set == the possible-biome set, each with
    // creature_spawn_probability and the ordered CREATURE spawner list (the
    // `Weighted<SpawnerData>` list `NaturalSpawner.spawnMobsForChunkGeneration`
    // reads — `mobSettings.getMobs(MobCategory.CREATURE)`). The spawners are
    // emitted in builder (holder-set) order; a reorder changes the weighted
    // selection and must fail.
    let mob = object
        .get("mob_settings")
        .and_then(Value::as_object)
        .context("feature_data.json is missing `mob_settings`")?;
    if mob.len() != possible_names.len() {
        bail!(
            "`mob_settings` has {} entries but possible_biomes has {}",
            mob.len(),
            possible_names.len()
        );
    }
    for name in &possible_names {
        if !mob.contains_key(*name) {
            bail!("possible biome `{name}` is absent from `mob_settings`");
        }
    }
    for name in mob.keys() {
        if !possible_names.contains(&name.as_str()) {
            bail!("biome `{name}` is in `mob_settings` but absent from `possible_biomes`");
        }
    }
    for (name, entry) in mob {
        let entry = entry
            .as_object()
            .with_context(|| format!("mob_settings `{name}` entry must be an object"))?;
        for field in entry.keys() {
            if !matches!(field.as_str(), "creature_spawn_probability" | "creature") {
                bail!("mob_settings `{name}` has unexpected field `{field}`");
            }
        }
        entry
            .get("creature_spawn_probability")
            .and_then(Value::as_f64)
            .with_context(|| format!("mob_settings `{name}` missing creature_spawn_probability"))?;
        let creature = entry
            .get("creature")
            .and_then(Value::as_array)
            .with_context(|| format!("mob_settings `{name}` is missing `creature`"))?;
        for (i, e) in creature.iter().enumerate() {
            let e = e
                .as_object()
                .with_context(|| format!("mob_settings `{name}` creature[{i}] must be an object"))?;
            for field in e.keys() {
                if !matches!(field.as_str(), "type" | "min" | "max" | "weight") {
                    bail!("mob_settings `{name}` creature[{i}] has unexpected field `{field}`");
                }
            }
            let ty = e
                .get("type")
                .and_then(Value::as_str)
                .with_context(|| format!("mob_settings `{name}` creature[{i}] missing `type`"))?;
            crate::registries::validate_name("minecraft:entity_type", ty)?;
            e.get("min").and_then(Value::as_u64).with_context(|| {
                format!("mob_settings `{name}` creature[{i}] missing `min`")
            })?;
            e.get("max").and_then(Value::as_u64).with_context(|| {
                format!("mob_settings `{name}` creature[{i}] missing `max`")
            })?;
            e.get("weight").and_then(Value::as_u64).with_context(|| {
                format!("mob_settings `{name}` creature[{i}] missing `weight`")
            })?;
        }
    }

    // Biome registry ids are the full-registry dense ids and must be unique
    // across the possible set (a hand-edited id reassignment or duplicate
    // would corrupt the deterministic id -> biome identity the runtime and the
    // generated tables rely on). This mirrors the feature tables' uniqueness
    // check.
    biome_ids.sort_unstable();
    for pair in biome_ids.windows(2) {
        if pair[0] == pair[1] {
            bail!("`biomes` has a duplicate registry id `{}`", pair[0]);
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

    // Closure, three structurally explicit checks (see the module doc; the
    // runtime committed-table test in `crates/rivet-world/src/data/feature_data.rs`
    // mirrors them). A fixture with a dead (unreachable) entry or a dangling
    // reference is stale or hand-edited.
    //
    // Check 1 — typed dangling refs. RegistryOps encodes a feature holder
    // reference as a bare string under a *feature-holder key*: `feature` (a
    // `random_selector`/`random_boolean_selector`/`vegetation_patch`/`root_system`
    // sub-feature) or `default` (a `random_selector` default). The top-level
    // `feature` of a placed feature is its configured-feature ref and must
    // resolve in `configured_features` SPECIFICALLY — a placed-only key is
    // type-invalid (a placed feature's `feature` is always a configured
    // feature). A configured feature's holder ref may legitimately name either
    // a placed or a configured feature (e.g. `trees_water`'s `feature` is
    // placed `fancy_oak_checked`, while `moss_patch`'s `feature` is configured
    // `moss_vegetation`), so it resolves in either table. Every such string
    // must resolve in the fixture's own tables — checking by membership of the
    // *referencing* feature's own table would silently accept a closure that
    // dropped a referenced entry (e.g. a `random_selector` `default` pointing
    // at a placed feature the closure omitted). Block-state `Name` values, tag
    // strings, and feature `type` dispatch keys are never holder refs and are
    // not collected here.
    let mut direct_placed: HashSet<&str> = HashSet::new();
    for entry in biomes.values() {
        for step in entry["features"].as_array().unwrap() {
            for v in step.as_array().unwrap() {
                direct_placed.insert(v.as_str().unwrap());
            }
        }
    }

    // Placed -> configured (typed): a placed feature's top-level `feature` is
    // its configured-feature ref.
    for (pname, pentry) in placed {
        let json = pentry
            .get("json")
            .and_then(Value::as_object)
            .with_context(|| format!("placed feature `{pname}` is missing `json`"))?;
        let feature_ref = json
            .get("feature")
            .and_then(Value::as_str)
            .with_context(|| format!("placed feature `{pname}` `json.feature` is not a string"))?;
        if !configured.contains_key(feature_ref) {
            bail!(
                "placed feature `{pname}` references configured feature `{feature_ref}` that is \
                 absent from `configured_features` (a dangling or mis-typed holder reference)"
            );
        }
    }

    // Configured -> either table: a configured feature's holder ref names a
    // placed or a configured feature (both occur in the fixture).
    for (cname, centry) in configured {
        let json = centry
            .get("json")
            .with_context(|| format!("configured feature `{cname}` is missing `json`"))?;
        for r in collect_feature_holder_refs(json) {
            if !placed.contains_key(r) && !configured.contains_key(r) {
                bail!(
                    "configured feature `{cname}` references feature `{r}` that is absent from \
                     `placed_features` and `configured_features` (a dangling holder reference)"
                );
            }
        }
    }

    // A biome step list is a direct placed-feature reference: every entry must
    // resolve in `placed_features` (a dangling biome ref would emit a table whose
    // step list names an absent feature).
    for p in &direct_placed {
        if !placed.contains_key(*p) {
            bail!(
                "biome step list references placed feature `{p}` that is absent from \
                 `placed_features`"
            );
        }
    }

    // Check 2 — dead entries. Every table entry must be reachable from the
    // biomes' step lists through the extractor's fixpoint, which resolves every
    // bare `minecraft:` string by registry membership (a feature `type` key that
    // shares a feature's name, like `minecraft:vines`, is a real reachability
    // edge at capture time — membership, not key position, disambiguates it).
    // The walk is a genuine FORWARD reachability seeded ONLY from the biomes'
    // step lists and iterated to a fixpoint: a disconnected, mutually-
    // referencing component that no biome can reach must fail as a stale entry
    // (a walk that seeds from every table entry would let such a component
    // appear reachable).
    let mut placed_reachable: HashSet<&str> = direct_placed.clone();
    let mut configured_reachable: HashSet<&str> = HashSet::new();
    loop {
        let mut grew = false;

        // Reachable placed -> their configured-feature ref.
        for p in placed_reachable.clone() {
            let pentry = placed
                .get(p)
                .expect("a reachable placed feature must be a table member");
            let json = pentry
                .get("json")
                .and_then(Value::as_object)
                .with_context(|| format!("placed feature `{p}` is missing `json`"))?;
            let feature_ref = json
                .get("feature")
                .and_then(Value::as_str)
                .with_context(|| format!("placed feature `{p}` `json.feature` is not a string"))?;
            if configured.contains_key(feature_ref) {
                grew |= configured_reachable.insert(feature_ref);
            }
        }

        // Reachable configured -> their bare-string refs (by membership).
        for c in configured_reachable.clone() {
            let centry = configured
                .get(c)
                .expect("a reachable configured feature must be a table member");
            let json = centry
                .get("json")
                .with_context(|| format!("configured feature `{c}` is missing `json`"))?;
            for r in collect_bare_strings(json) {
                if placed.contains_key(r) {
                    grew |= placed_reachable.insert(r);
                }
                if configured.contains_key(r) {
                    grew |= configured_reachable.insert(r);
                }
            }
        }

        if !grew {
            break;
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
    check("possible_biome_count", possible_names.len())?;
    check("reachable_biome_count", names.len())?;
    check("placed_feature_count", placed.len())?;
    check("configured_feature_count", configured.len())?;

    // The probe's per-biome observations must agree with the recorded `biomes`
    // step lists (and with each other): the probe is the extractor's live-Paper
    // snapshot, so a stale per_biome block means a hand-edited fixture or a
    // capture whose `biomes` and `probe` drifted.
    let per_biome = probe
        .get("per_biome")
        .and_then(Value::as_object)
        .context("probe is missing `per_biome`")?;
    if per_biome.len() != biomes.len() {
        bail!(
            "probe.per_biome has {} entries but the fixture has {} biomes",
            per_biome.len(),
            biomes.len()
        );
    }
    for (bname, bentry) in per_biome {
        let b = biomes
            .get(bname)
            .with_context(|| format!("probe.per_biome has unknown biome `{bname}`"))?;
        let per_step = bentry
            .get("per_step")
            .and_then(Value::as_array)
            .with_context(|| format!("probe.per_biome.{bname} is missing `per_step`"))?;
        let steps = b["features"]
            .as_array()
            .with_context(|| format!("biome `{bname}` is missing `features`"))?;
        if per_step.len() != steps.len() {
            bail!(
                "probe.per_biome.{bname}.per_step has {} entries but the biome has {} steps",
                per_step.len(),
                steps.len()
            );
        }
        for (i, (recorded, step)) in per_step.iter().zip(steps).enumerate() {
            let recorded = recorded.as_u64().with_context(|| {
                format!("probe.per_biome.{bname}.per_step[{i}] is not an integer")
            })?;
            let step = step
                .as_array()
                .with_context(|| format!("biome `{bname}` step {i} is not an array"))?;
            if recorded as usize != step.len() {
                bail!(
                    "probe.per_biome.{bname}.per_step[{i}] is {recorded} but the biome step has {} features",
                    step.len()
                );
            }
        }
        // The probe records the ordered placed-feature names per step; the biome
        // step lists must match element-for-element (a within-step reorder
        // changes decoration semantics and must fail — the same invariant the
        // runtime committed-table test pins).
        let per_step_names = bentry
            .get("per_step_names")
            .and_then(Value::as_array)
            .with_context(|| format!("probe.per_biome.{bname} is missing `per_step_names`"))?;
        if per_step_names.len() != steps.len() {
            bail!(
                "probe.per_biome.{bname}.per_step_names has {} entries but the biome has {} steps",
                per_step_names.len(),
                steps.len()
            );
        }
        for (i, (recorded_names, step)) in per_step_names.iter().zip(steps).enumerate() {
            let recorded_names = recorded_names.as_array().with_context(|| {
                format!("probe.per_biome.{bname}.per_step_names[{i}] is not an array")
            })?;
            let step = step
                .as_array()
                .with_context(|| format!("biome `{bname}` step {i} is not an array"))?;
            let step_names: Vec<&str> = step
                .iter()
                .map(|v| {
                    v.as_str()
                        .with_context(|| format!("biome `{bname}` step {i} entry is not a string"))
                })
                .collect::<Result<_>>()?;
            let probe_names: Vec<&str> = recorded_names
                .iter()
                .map(|v| {
                    v.as_str().with_context(|| {
                        format!("probe.per_biome.{bname}.per_step_names[{i}] entry is not a string")
                    })
                })
                .collect::<Result<_>>()?;
            if probe_names != step_names {
                bail!(
                    "probe.per_biome.{bname}.per_step_names[{i}] does not match the biome step \
                     (order is semantics: FeatureSorter's per-step holder order) — recorded {probe_names:?} vs {step_names:?}"
                );
            }
        }
        let total = bentry
            .get("total")
            .and_then(Value::as_u64)
            .with_context(|| format!("probe.per_biome.{bname} is missing `total`"))?;
        let sum: u64 = per_step.iter().filter_map(Value::as_u64).sum();
        if total != sum {
            bail!("probe.per_biome.{bname}.total is {total} but the per_step counts sum to {sum}");
        }
    }

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

/// Bare strings under a *feature-holder key* (`feature`/`default`) inside a
/// configured feature's RegistryOps-encoded JSON — the only positions the
/// extractor encodes as registry-reference holders (a `random_selector`/`
/// random_boolean_selector`/`vegetation_patch`/`root_system` sub-feature, or a
/// `random_selector` default). Block-state `Name` values, tag strings, and
/// feature `type` dispatch keys are never collected here. The caller resolves
/// each collected string against the tables (a string can name either a placed
/// or a configured feature).
fn collect_feature_holder_refs(elem: &Value) -> Vec<&str> {
    fn walk<'a>(elem: &'a Value, key: &str, out: &mut Vec<&'a str>) {
        match elem {
            Value::String(s) => {
                if matches!(key, "feature" | "default") {
                    out.push(s.as_str());
                }
            }
            Value::Array(a) => a.iter().for_each(|e| walk(e, key, out)),
            Value::Object(o) => o.iter().for_each(|(k, v)| walk(v, k.as_str(), out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(elem, "", &mut out);
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
    crate::reports::verify_pinned_source(&manifest.source)?;
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
    fn paper_pin_is_pinned() {
        let mut root = fixture();
        root["paper"] = serde_json::json!("26.2-DEV-main@foreign");
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("`paper` pin"), "got: {err}");
    }

    #[test]
    fn possible_biome_count_is_pinned() {
        let mut root = fixture();
        // Drop one possible biome from the list: must fail (the FeatureSorter
        // source list is 55).
        let mut arr = root["possible_biomes"].as_array().unwrap().clone();
        arr.pop();
        root["possible_biomes"] = Value::Array(arr);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("possible_biomes has 54 entries"),
            "got: {err}"
        );
    }

    #[test]
    fn possible_biome_order_is_pinned() {
        let mut root = fixture();
        // Swap two entries of the possible list: must fail (the emission order
        // fixes FeatureSorter's global feature indices).
        let mut arr = root["possible_biomes"].as_array().unwrap().clone();
        arr.swap(0, 1);
        root["possible_biomes"] = Value::Array(arr);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("order diverges from the pinned"),
            "got: {err}"
        );
    }

    #[test]
    fn reachable_biomes_must_be_a_subset_of_possible() {
        let mut root = fixture();
        // Replace a reachable name with one that is not in the possible list
        // (keep the count at 5 so the subset check — not the count check —
        // fires): must fail.
        let mut arr = root["reachable_biomes"].as_array().unwrap().clone();
        arr[0] = serde_json::json!("minecraft:the_end");
        root["reachable_biomes"] = Value::Array(arr);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("absent from `possible_biomes`"),
            "got: {err}"
        );
    }

    #[test]
    fn biomes_key_set_must_equal_possible_set() {
        let mut root = fixture();
        // Remove one biome from `biomes`: must fail (every possible biome needs
        // its full generation settings). The cardinality check fires first
        // (55 -> 54).
        let removed = root["possible_biomes"][0].as_str().unwrap().to_string();
        root["biomes"].as_object_mut().unwrap().remove(&removed);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("`biomes` has 54 entries"),
            "got: {err}"
        );

        // And a `biomes` entry that is not in the possible list must fail too.
        let mut root = fixture();
        root["biomes"].as_object_mut().unwrap().insert(
            "minecraft:the_end".to_string(),
            serde_json::json!({ "id": 200, "carvers": [], "features": [[],[],[],[],[],[],[],[],[],[],[]] }),
        );
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("`biomes` has 56 entries"),
            "got: {err}"
        );
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
        // 66 is above every biome id in the full possible set (max is 65), so
        // the break is a genuine id-order break and not an id collision.
        let beach_id = root["biomes"]["minecraft:beach"]["id"].as_u64().unwrap();
        let dark_forest_id = root["biomes"]["minecraft:dark_forest"]["id"]
            .as_u64()
            .unwrap();
        root["biomes"]["minecraft:beach"]["id"] = serde_json::json!(66);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("not sorted by registry id"),
            "got: {err}"
        );
        // Sanity: the edit is a real change (beach id differs from its original
        // and from dark_forest's, so the id-order check — not a duplicate-id
        // check — is what fires).
        assert_ne!(beach_id, 66);
        assert_ne!(beach_id, dark_forest_id);
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
    fn stale_probe_per_biome_step_counts_fail() {
        let mut root = fixture();
        // A probe.per_biome block whose per_step counts no longer match the
        // recorded per-biome step lists must fail: the probe is the extractor's
        // live-Paper observation and must agree with `biomes`.
        root["probe"]["per_biome"]["minecraft:beach"]["per_step"][1] = serde_json::json!(99);
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("per_step"), "got: {err}");
    }

    #[test]
    fn stale_probe_per_biome_step_names_fail() {
        let mut root = fixture();
        // A probe.per_biome block whose ordered per-step names no longer match
        // the biome step lists must fail even when the counts agree: order is
        // semantics (FeatureSorter's per-step holder order). Swap two names in
        // beach's step 1 probe.
        let names = root["probe"]["per_biome"]["minecraft:beach"]["per_step_names"][1]
            .as_array_mut()
            .unwrap();
        names.swap(0, 1);
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("per_step_names"), "got: {err}");
    }

    #[test]
    fn duplicate_biome_registry_id_fails() {
        let mut root = fixture();
        // Duplicate the beach biome's dense registry id onto another biome's id:
        // the `biomes` table must be a name->id bijection (registry identity).
        let dup = root["biomes"]["minecraft:beach"]["id"].as_u64().unwrap();
        root["biomes"]["minecraft:dark_forest"]["id"] = serde_json::json!(dup);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("duplicate registry id"),
            "got: {err}"
        );
    }

    #[test]
    fn placed_to_configured_mis_type_fails() {
        let mut root = fixture();
        // Point a placed feature's `json.feature` at a *placed* feature instead
        // of a configured one: the placed->configured edge is typed and must
        // fail (a placed feature can only reference a configured feature). Pick a
        // placed-only name (in placed_features, not in configured_features) so
        // the mis-type is real — every well-formed placed json.feature resolves
        // in configured_features.
        let configured: Vec<String> = root["configured_features"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let placed_only = root["placed_features"]
            .as_object()
            .unwrap()
            .keys()
            .find(|k| !configured.contains(k))
            .expect("the full closure has a placed-only feature")
            .clone();
        root["placed_features"]["minecraft:lake_lava_underground"]["json"]["feature"] =
            serde_json::json!(placed_only);
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("references configured feature")
                && err.to_string().contains(&placed_only),
            "got: {err}"
        );
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
    fn removed_configured_only_placed_feature_fails_dangling() {
        let mut root = fixture();
        // `minecraft:acacia_checked` is a placed feature referenced ONLY by the
        // configured `minecraft:trees_savanna` JSON under the holder key
        // `feature` (never in any biome step list). Dropping it from the tables
        // must fail: the reference is still present and would dangle. A
        // membership-only check (resolve refs against the tables being
        // validated) would have accepted this, because the dropped entry is
        // itself a table member the reference "resolves" against — the
        // holder-key check makes the edge structurally explicit.
        assert!(
            root["placed_features"]["minecraft:acacia_checked"].is_object(),
            "fixture must carry `minecraft:acacia_checked` as a placed feature"
        );
        assert!(
            !root["configured_features"]
                .as_object()
                .unwrap()
                .contains_key("minecraft:acacia_checked"),
            "`minecraft:acacia_checked` must not be a configured feature (that would mask the edge)"
        );
        assert!(
            !root["biomes"]
                .as_object()
                .unwrap()
                .values()
                .any(|b| b["features"].as_array().unwrap().iter().any(|s| s
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| *p == serde_json::json!("minecraft:acacia_checked")))),
            "`minecraft:acacia_checked` must not appear in any biome step list (that would mask the edge)"
        );
        root["placed_features"]
            .as_object_mut()
            .unwrap()
            .remove("minecraft:acacia_checked");
        let err = validate_structural(&root).unwrap_err();
        assert!(
            err.to_string().contains("dangling holder reference"),
            "got: {err}"
        );
        assert!(
            err.to_string().contains("minecraft:acacia_checked"),
            "got: {err}"
        );
    }

    #[test]
    fn dereferenced_configured_only_placed_feature_fails_dead_entry() {
        let mut root = fixture();
        // Point `minecraft:trees_savanna`'s `feature` holder ref (the first
        // weighted placed feature) away from `minecraft:acacia_checked` while
        // keeping the entry in the table: the reference now resolves
        // (`minecraft:oak_checked` is a placed member), so no dangling ref is
        // reported, but `minecraft:acacia_checked` becomes unreachable and must
        // fail the dead-entry check.
        assert_eq!(
            root["configured_features"]["minecraft:trees_savanna"]["json"]["config"]["features"][0]
                ["feature"],
            serde_json::json!("minecraft:acacia_checked")
        );
        root["configured_features"]["minecraft:trees_savanna"]["json"]["config"]["features"][0]["feature"] =
            serde_json::json!("minecraft:oak_checked");
        let err = validate_structural(&root).unwrap_err();
        assert!(err.to_string().contains("unreachable"), "got: {err}");
        assert!(
            err.to_string().contains("minecraft:acacia_checked"),
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
            crate::reports::PINNED_SERVER_JAR_SHA256_FEATURE_DATA
        );
    }
}
