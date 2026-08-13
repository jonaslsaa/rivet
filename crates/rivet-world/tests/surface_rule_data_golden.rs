//! Paper-grounded golden tests for the merged `SurfaceRules` codec surface,
//! driven by the committed SurfaceRuleData fixture.
//!
//! Fixture provenance: `tools/rivet-oracle/src/java/SurfaceRuleDataProbe.java`
//! captures every static `SurfaceRuleData.*` surface tree through
//! `RuleSource.CODEC`/`ConditionSource.CODEC` under `RegistryOps` on the pinned
//! Paper 26.2 runtime (`0a99345`), plus structural occurrence-count stats.
//! Regenerate with `scripts/run_surface_rule_data_probe.sh`.
//!
//! The nether tree is the only one verified here: it exercises every node type
//! the merged codec surface ports and needs only the five nether biomes in the
//! test registry. The overworld / overworldLike trees stay present in the
//! fixture but UNVERIFIED (see `overworld_trees_are_unverified`) because their
//! `biome_is` holder sets reference the 28 overworld biomes, which the nether-
//! only test registry cannot resolve.
//!
//! Byte-exactness notes:
//! - Both the canonical fixture and the Rust re-encode are serde_json Values
//!   under `arbitrary_precision` + `preserve_order`, so the Java `1.7976931...
//!   E308` exponent casing normalizes identically on both sides and the
//!   comparison is a true byte compare.
//! - Bare-string `biome_is` holder sets (Java's compact form) decode through
//!   the `HolderSetCodec` list arm to a single `Holder::reference` and re-encode
//!   to the same bare identifier.

use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::builder::RegistryBuilder;
use rivet_registry::registration_info::RegistrationInfo;
use rivet_registry::registry_ops::RegistryOps;
use rivet_registry::root::AnyBox;
use rivet_registry::{Identifier, ResourceKey};
use rivet_serialization::json_ops::JsonOps;
use rivet_world::biome::biomes;
use rivet_world::levelgen::surface_rules::SequenceRuleSource;
use rivet_world::levelgen::surface_rules::rule_source_codec;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

type TestOps = RegistryOps<serde_json::Value, JsonOps>;

const FIXTURE: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/surface-rule-data/surface-rule-data.json");
const MANIFEST: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/surface-rule-data/manifest.json");

/// The pinned Paper commit the fixture was captured from.
const PAPER_PIN: &str = "26.2-DEV-main@0a99345";
/// sha256 over the fixture bytes as recorded in `manifest.json` at capture
/// time. Hard-coded here so a silent edit to the golden (without a deliberate
/// re-capture + manifest bump) fails the digest test.
const FIXTURE_SHA256: &str = "8cd8795e78cb583654d2d6fbe0252a0937b204918c5b56911cf8d60813b2abfb";
const FIXTURE_BYTES: usize = 363441;

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture JSON parses")
}

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest JSON parses")
}

fn preset<'a>(fixture: &'a Value, name: &str) -> &'a Value {
    fixture["presets"]
        .as_array()
        .expect("presets array")
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("preset {name} present in fixture"))
        .get("json")
        .expect("preset json")
}

/// A biome registry with the five nether biomes (ids 0..5) under
/// `Registries.BIOME` — enough to resolve every `biome_is` holder the nether
/// tree references.
fn nether_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BIOME);
    for (id, key) in [
        (0, &*biomes::NETHER_WASTES),
        (1, &*biomes::SOUL_SAND_VALLEY),
        (2, &*biomes::CRIMSON_FOREST),
        (3, &*biomes::WARPED_FOREST),
        (4, &*biomes::BASALT_DELTAS),
    ] {
        builder.register(
            key,
            Arc::new(BiomeId::from_id(id)),
            RegistrationInfo::BUILT_IN,
        );
    }
    let registry = builder.freeze();
    RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
        Box::new(registry) as AnyBox,
    )])
}

fn nether_ops() -> TestOps {
    RegistryOps::create_from_access(&JsonOps::INSTANCE, nether_access())
}

// -- structural walkers (mirror `SurfaceRuleDataProbe.countStruct`) ---------

/// Occurrence count of every dispatch `"type"` discriminator, conditions and
/// rules unclassified (the fixture's `node-types` stat).
fn count_node_types(value: &Value, acc: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            if let Some(t) = map.get("type").and_then(Value::as_str) {
                *acc.entry(t.to_string()).or_insert(0) += 1;
            }
            for (k, v) in map {
                if k != "type" {
                    count_node_types(v, acc);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                count_node_types(item, acc);
            }
        }
        _ => {}
    }
}

/// Occurrence count of the `result_state.Name` a `block` rule carries.
fn count_block_names(value: &Value, acc: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("minecraft:block")
                && let Some(name) = map
                    .get("result_state")
                    .and_then(|rs| rs.get("Name"))
                    .and_then(Value::as_str)
            {
                *acc.entry(name.to_string()).or_insert(0) += 1;
            }
            for v in map.values() {
                count_block_names(v, acc);
            }
        }
        Value::Array(items) => {
            for item in items {
                count_block_names(item, acc);
            }
        }
        _ => {}
    }
}

/// Occurrence count of the biome holder identifiers a `biome` condition
/// carries (compact bare id, or id list).
fn count_biome_names(value: &Value, acc: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            if let Some(biomes) = map.get("biome_is") {
                match biomes {
                    Value::String(id) => *acc.entry(id.clone()).or_insert(0) += 1,
                    Value::Array(ids) => {
                        for id in ids {
                            if let Some(id) = id.as_str() {
                                *acc.entry(id.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                    _ => panic!("biome_is is neither a string nor a list"),
                }
            }
            for v in map.values() {
                count_biome_names(v, acc);
            }
        }
        Value::Array(items) => {
            for item in items {
                count_biome_names(item, acc);
            }
        }
        _ => {}
    }
}

// -- deep mutation helpers (hostile tests) -----------------------------------

/// Apply `mutate` to every object whose `"type"` is `kind`, then stop
/// descending through that subtree.
fn mutate_type_nodes(
    value: &mut Value,
    kind: &str,
    mutate: &dyn Fn(&mut serde_json::Map<String, Value>),
) {
    match value {
        Value::Object(map) => {
            let is_target = map.get("type").and_then(Value::as_str) == Some(kind);
            if is_target {
                mutate(map);
                return;
            }
            for v in map.values_mut() {
                mutate_type_nodes(v, kind, mutate);
            }
        }
        Value::Array(items) => {
            for item in items {
                mutate_type_nodes(item, kind, mutate);
            }
        }
        _ => {}
    }
}

/// Swap the first two elements of the first `sequence` with at least two.
fn reorder_first_sequence(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("minecraft:sequence")
                && let Some(seq) = map.get_mut("sequence").and_then(Value::as_array_mut)
                && seq.len() >= 2
            {
                seq.swap(0, 1);
                return;
            }
            for v in map.values_mut() {
                reorder_first_sequence(v);
            }
        }
        Value::Array(items) => {
            for item in items {
                reorder_first_sequence(item);
            }
        }
        _ => {}
    }
}

// -- tests -------------------------------------------------------------------

/// The manifest pins the fixture provenance (format 1, Paper 0a99345, kind
/// `surface-rule-data`) and the digest test catches any silent edit to the
/// golden file.
#[test]
fn fixture_sha256_matches_manifest_capture() {
    assert_eq!(manifest()["paper"].as_str().unwrap(), PAPER_PIN);
    assert_eq!(manifest()["format"].as_u64().unwrap(), 1);
    assert_eq!(manifest()["kind"].as_str().unwrap(), "surface-rule-data");
    let capture = &manifest()["captured"][0];
    assert_eq!(capture["path"].as_str().unwrap(), "surface-rule-data.json");
    assert_eq!(capture["bytes"].as_u64().unwrap(), FIXTURE_BYTES as u64);
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE.as_bytes())),
        FIXTURE_SHA256
    );
}

/// The nether tree is present in the fixture with the pinned provenance.
#[test]
fn nether_preset_present_with_provenance() {
    let fixture = fixture();
    assert_eq!(fixture["paper"].as_str().unwrap(), PAPER_PIN);
    assert_eq!(fixture["format"].as_u64().unwrap(), 1);
    assert!(preset(&fixture, "nether").is_object());
}

/// The canonical nether tree parses through `rule_source_codec` under a
/// nether-only biome registry and re-encodes byte-identically.
#[test]
fn nether_tree_parses_and_reencodes_byte_exactly() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let canonical = preset(&fixture(), "nether").clone();
    let decoded = codec
        .parse(&ops, &canonical)
        .get_or_throw("decode nether tree")
        .clone();
    assert!(decoded.as_any().is::<SequenceRuleSource>());
    let reencoded = codec
        .encode_start(&ops, &decoded)
        .get_or_throw("re-encode nether tree")
        .clone();
    assert_eq!(
        serde_json::to_vec(&reencoded).expect("re-encoded JSON serializes"),
        serde_json::to_vec(&canonical).expect("canonical JSON serializes"),
        "nether tree must re-encode byte-identically to the Paper capture"
    );
}

/// The fixture's structural stats for the nether tree pin the node-type,
/// block, and biome occurrence counts captured from Paper.
#[test]
fn nether_structural_stats_match_capture() {
    let fixture = fixture();
    let json = preset(&fixture, "nether");
    let mut node_types = BTreeMap::new();
    count_node_types(json, &mut node_types);
    assert_eq!(
        node_types,
        BTreeMap::from([
            ("minecraft:biome".to_string(), 5),
            ("minecraft:block".to_string(), 22),
            ("minecraft:condition".to_string(), 41),
            ("minecraft:hole".to_string(), 3),
            ("minecraft:noise_threshold".to_string(), 11),
            ("minecraft:not".to_string(), 10),
            ("minecraft:sequence".to_string(), 12),
            ("minecraft:stone_depth".to_string(), 7),
            ("minecraft:vertical_gradient".to_string(), 2),
            ("minecraft:y_above".to_string(), 13),
        ])
    );

    let mut blocks = BTreeMap::new();
    count_block_names(json, &mut blocks);
    assert_eq!(
        blocks,
        BTreeMap::from([
            ("minecraft:basalt".to_string(), 2),
            ("minecraft:bedrock".to_string(), 2),
            ("minecraft:blackstone".to_string(), 1),
            ("minecraft:crimson_nylium".to_string(), 1),
            ("minecraft:gravel".to_string(), 4),
            ("minecraft:lava".to_string(), 1),
            ("minecraft:nether_wart_block".to_string(), 1),
            ("minecraft:netherrack".to_string(), 3),
            ("minecraft:soul_sand".to_string(), 3),
            ("minecraft:soul_soil".to_string(), 2),
            ("minecraft:warped_nylium".to_string(), 1),
            ("minecraft:warped_wart_block".to_string(), 1),
        ])
    );

    let mut biomes = BTreeMap::new();
    count_biome_names(json, &mut biomes);
    assert_eq!(
        biomes,
        BTreeMap::from([
            ("minecraft:basalt_deltas".to_string(), 1),
            ("minecraft:crimson_forest".to_string(), 1),
            ("minecraft:nether_wastes".to_string(), 1),
            ("minecraft:soul_sand_valley".to_string(), 1),
            ("minecraft:warped_forest".to_string(), 1),
        ])
    );
}

/// The overworld / overworldLike trees stay present in the fixture but are
/// UNVERIFIED: their `biome_is` holder sets reference the 28 overworld biomes,
/// which the nether-only test registry cannot resolve. A decode attempt fails
/// at the first missing biome holder, so this test pins the boundary — wiring
/// the overworld biomes into the test registry flips the expectation and must
/// be a deliberate extension, not a silent one.
#[test]
fn overworld_trees_are_unverified() {
    let fixture = fixture();
    for name in [
        "overworld",
        "overworld_like_true_false_true",
        "overworld_like_false_false_true",
    ] {
        let json = preset(&fixture, name);
        // The trees reference the 28 overworld biomes — none registered in the
        // nether-only test registry.
        let mut biomes = BTreeMap::new();
        count_biome_names(json, &mut biomes);
        assert_eq!(biomes.len(), 28, "{name} references 28 biomes");

        let ops = nether_ops();
        let codec = rule_source_codec::<TestOps>();
        assert!(
            codec.parse(&ops, json).result().is_none(),
            "{name} is UNVERIFIED: decode must fail on the missing overworld biome holders"
        );
    }
}

/// Reordering a sequence's elements changes the canonical bytes: byte-exactness
/// is order-sensitive, not a vacuous pass.
#[test]
fn reordered_sequence_does_not_reencode_byte_exactly() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let canonical = preset(&fixture(), "nether").clone();
    let mut tampered = canonical.clone();
    reorder_first_sequence(&mut tampered);
    // The reordered tree still parses (order is preserved through decode).
    let decoded = codec
        .parse(&ops, &tampered)
        .get_or_throw("decode reordered nether tree")
        .clone();
    let reencoded = codec
        .encode_start(&ops, &decoded)
        .get_or_throw("re-encode reordered nether tree")
        .clone();
    assert_ne!(
        serde_json::to_vec(&reencoded).expect("serializes"),
        serde_json::to_vec(&canonical).expect("serializes"),
        "a reordered sequence must not re-encode to the canonical bytes"
    );
}

/// A bogus dispatch type on the root rule must be rejected.
#[test]
fn wrong_rule_type_is_rejected() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let mut tampered = preset(&fixture(), "nether").clone();
    tampered["type"] = Value::String("minecraft:no_such_rule".into());
    assert!(
        codec.parse(&ops, &tampered).result().is_none(),
        "an unknown rule type must be rejected"
    );
}

/// Dropping the required `biome_is` field from a biome condition is rejected.
#[test]
fn missing_biome_holder_is_rejected() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let mut tampered = preset(&fixture(), "nether").clone();
    mutate_type_nodes(&mut tampered, "minecraft:biome", &|map| {
        map.remove("biome_is");
    });
    assert!(
        codec.parse(&ops, &tampered).result().is_none(),
        "a biome condition without biome_is must be rejected"
    );
}

/// A biome identifier that is not in the registry is rejected by the
/// `HolderSetCodec` list arm (`RegistryFixedCodec` cannot resolve it).
#[test]
fn unregistered_biome_holder_is_rejected() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let mut tampered = preset(&fixture(), "nether").clone();
    mutate_type_nodes(&mut tampered, "minecraft:biome", &|map| {
        map.insert(
            "biome_is".into(),
            Value::String("minecraft:deep_dark".into()),
        );
    });
    assert!(
        codec.parse(&ops, &tampered).result().is_none(),
        "an unregistered biome holder must be rejected"
    );
}

/// A block rule carrying a name outside the generated block table is rejected.
#[test]
fn unknown_block_name_is_rejected() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let mut tampered = preset(&fixture(), "nether").clone();
    mutate_type_nodes(&mut tampered, "minecraft:block", &|map| {
        if let Some(rs) = map.get_mut("result_state").and_then(Value::as_object_mut) {
            rs.insert("Name".into(), Value::String("minecraft:not_a_block".into()));
        }
    });
    assert!(
        codec.parse(&ops, &tampered).result().is_none(),
        "an unknown block name must be rejected"
    );
}

/// A malformed vertical anchor is rejected.
#[test]
fn malformed_anchor_is_rejected() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let mut tampered = preset(&fixture(), "nether").clone();
    mutate_type_nodes(&mut tampered, "minecraft:y_above", &|map| {
        map.insert("anchor".into(), serde_json::json!({"below_top": "garbage"}));
    });
    assert!(
        codec.parse(&ops, &tampered).result().is_none(),
        "a malformed vertical anchor must be rejected"
    );
}

/// Semantic mutations that stay well-formed (a different noise key, a shifted
/// threshold, a different block) still decode — the codecs are faithful, not
/// strict — but must NOT re-encode to the canonical bytes.
#[test]
fn mutated_values_do_not_reencode_byte_exactly() {
    let ops = nether_ops();
    let codec = rule_source_codec::<TestOps>();
    let canonical = preset(&fixture(), "nether").clone();

    let mut noise_key = canonical.clone();
    mutate_type_nodes(&mut noise_key, "minecraft:noise_threshold", &|map| {
        map.insert(
            "noise".into(),
            Value::String("minecraft:gravel_layer".into()),
        );
    });
    let reencoded = codec
        .parse(&ops, &noise_key)
        .get_or_throw("decode noise-key mutation")
        .clone();
    let bytes = codec
        .encode_start(&ops, &reencoded)
        .get_or_throw("encode")
        .clone();
    assert_ne!(
        serde_json::to_vec(&bytes).expect("serializes"),
        serde_json::to_vec(&canonical).expect("serializes"),
        "a mutated noise key must not re-encode to the canonical bytes"
    );

    let mut threshold = canonical.clone();
    mutate_type_nodes(&mut threshold, "minecraft:noise_threshold", &|map| {
        map.insert("min_threshold".into(), Value::from(-0.013f64));
    });
    let reencoded = codec
        .parse(&ops, &threshold)
        .get_or_throw("decode threshold mutation")
        .clone();
    let bytes = codec
        .encode_start(&ops, &reencoded)
        .get_or_throw("encode")
        .clone();
    assert_ne!(
        serde_json::to_vec(&bytes).expect("serializes"),
        serde_json::to_vec(&canonical).expect("serializes"),
        "a mutated threshold must not re-encode to the canonical bytes"
    );

    let mut block = canonical.clone();
    mutate_type_nodes(&mut block, "minecraft:block", &|map| {
        if let Some(rs) = map.get_mut("result_state").and_then(Value::as_object_mut) {
            rs.insert("Name".into(), Value::String("minecraft:end_stone".into()));
        }
    });
    let reencoded = codec
        .parse(&ops, &block)
        .get_or_throw("decode block mutation")
        .clone();
    let bytes = codec
        .encode_start(&ops, &reencoded)
        .get_or_throw("encode")
        .clone();
    assert_ne!(
        serde_json::to_vec(&bytes).expect("serializes"),
        serde_json::to_vec(&canonical).expect("serializes"),
        "a mutated block must not re-encode to the canonical bytes"
    );
}
