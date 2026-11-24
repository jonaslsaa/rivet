//! The `codec.reencode` corpus, committed as `corpus/codec.reencode.jsonl`.
//!
//! Three sources, all deterministic ([`CORPUS_SEED`] streams):
//!
//! 1. Hand-written valid and invalid examples per codec ([`HAND`]), typed from
//!    the Java codec definitions (field names, ranges, alternatives).
//! 2. Vanilla data-pack JSON harvested from Paper's jar by
//!    `tools/rivet-reference-oracle/harvest_datapack.py` into the committed
//!    `corpus-src/codec-datapack.jsonl` (so regenerating this corpus needs no jar).
//! 3. Seeded structural mutations of every valid input above (key removed, value
//!    replaced by a poison value, element dropped or appended), and the same
//!    inputs converted to SNBT for the `nbt` flavour.
//!
//! Paper answers every input; nothing here hand-types an expected value.

use std::path::PathBuf;

use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_serialization::json_ops::JsonOps;
use serde_json::{Map, Value, json};

use super::codec;
use super::packet_corpus::Rng;

/// Hand-written inputs: `(codec, json text)`. Both well-formed and ill-formed
/// examples belong here; Paper decides which is which.
const HAND: &[(&str, &str)] = &[
    ("ItemStack.CODEC", r#"{"id":"minecraft:stone","count":3}"#),
    (
        "ItemStack.CODEC",
        r#"{"id":"minecraft:diamond_sword","count":1,"components":{"minecraft:damage":5,"minecraft:max_stack_size":1}}"#,
    ),
    ("ItemStack.CODEC", r#"{"id":"minecraft:apple"}"#),
    ("ItemStack.CODEC", r#"{"id":"minecraft:stone","count":0}"#),
    (
        "ItemStack.CODEC",
        r#"{"id":"minecraft:stone","count":99999}"#,
    ),
    ("ItemStack.CODEC", r#"{"id":"minecraft:air","count":1}"#),
    (
        "ItemStack.CODEC",
        r#"{"id":"minecraft:not_an_item","count":1}"#,
    ),
    (
        "ItemStack.CODEC",
        r#"{"id":"minecraft:stick","count":2,"components":{"!minecraft:damage":{}}}"#,
    ),
    ("ItemStack.OPTIONAL_CODEC", "{}"),
    (
        "ItemStack.OPTIONAL_CODEC",
        r#"{"id":"minecraft:stone","count":2}"#,
    ),
    ("ItemStack.OPTIONAL_CODEC", r#"{"id":"minecraft:air"}"#),
    ("DataComponentPatch.CODEC", "{}"),
    ("DataComponentPatch.CODEC", r#"{"minecraft:damage":3}"#),
    ("DataComponentPatch.CODEC", r#"{"!minecraft:damage":{}}"#),
    (
        "DataComponentPatch.CODEC",
        r#"{"minecraft:custom_name":"hello","minecraft:lore":["a","b"]}"#,
    ),
    (
        "DataComponentPatch.CODEC",
        r#"{"minecraft:unbreakable":{}}"#,
    ),
    (
        "DataComponentPatch.CODEC",
        r#"{"minecraft:rarity":"epic","minecraft:enchantment_glint_override":true,"minecraft:max_damage":100}"#,
    ),
    ("DataComponentPatch.CODEC", r#"{"minecraft:damage":-1}"#),
    (
        "DataComponentPatch.CODEC",
        r#"{"minecraft:not_a_component":1}"#,
    ),
    ("DataComponentMap.CODEC", "{}"),
    (
        "DataComponentMap.CODEC",
        r#"{"minecraft:damage":3,"minecraft:rarity":"rare"}"#,
    ),
    (
        "DataComponentMap.CODEC",
        r#"{"minecraft:max_stack_size":16}"#,
    ),
    (
        "ItemStackTemplate.CODEC",
        r#"{"id":"minecraft:apple","count":2}"#,
    ),
    ("ItemStackTemplate.CODEC", r#""minecraft:apple""#),
    (
        "ItemStackTemplate.CODEC",
        r#"{"id":"minecraft:apple","count":2,"components":{"minecraft:damage":1}}"#,
    ),
    ("ItemContainerContents.CODEC", "[]"),
    (
        "ItemContainerContents.CODEC",
        r#"[{"slot":0,"item":{"id":"minecraft:stone","count":1}},{"slot":5,"item":{"id":"minecraft:apple","count":2}}]"#,
    ),
    (
        "ItemContainerContents.CODEC",
        r#"[{"slot":300,"item":{"id":"minecraft:stone","count":1}}]"#,
    ),
    ("BundleContents.CODEC", "[]"),
    (
        "BundleContents.CODEC",
        r#"[{"id":"minecraft:stone","count":2},"minecraft:apple"]"#,
    ),
    (
        "ChargedProjectiles.CODEC",
        r#"[{"id":"minecraft:arrow","count":1}]"#,
    ),
    ("ChargedProjectiles.CODEC", "[]"),
    (
        "FoodProperties.DIRECT_CODEC",
        r#"{"nutrition":4,"saturation":0.3}"#,
    ),
    (
        "FoodProperties.DIRECT_CODEC",
        r#"{"nutrition":4,"saturation":0.3,"can_always_eat":true}"#,
    ),
    (
        "FoodProperties.DIRECT_CODEC",
        r#"{"nutrition":-1,"saturation":0.3}"#,
    ),
    ("ItemUseAnimation.CODEC", r#""eat""#),
    ("ItemUseAnimation.CODEC", r#""drink""#),
    ("ItemUseAnimation.CODEC", r#""block""#),
    ("ItemUseAnimation.CODEC", r#""bogus""#),
    (
        "ConsumeEffect.CODEC",
        r#"{"type":"minecraft:clear_all_effects"}"#,
    ),
    (
        "ConsumeEffect.CODEC",
        r#"{"type":"minecraft:apply_effects","effects":[{"id":"minecraft:speed","duration":100}],"probability":0.5}"#,
    ),
    (
        "ConsumeEffect.CODEC",
        r#"{"type":"minecraft:remove_effects","effects":"minecraft:poison"}"#,
    ),
    (
        "ConsumeEffect.CODEC",
        r#"{"type":"minecraft:teleport_randomly","diameter":16.0}"#,
    ),
    (
        "ConsumeEffect.CODEC",
        r#"{"type":"minecraft:play_sound","sound":"minecraft:entity.generic.eat"}"#,
    ),
    ("Consumable.CODEC", "{}"),
    (
        "Consumable.CODEC",
        r#"{"consume_seconds":1.6,"animation":"eat","sound":"minecraft:entity.generic.eat","has_consume_particles":true,"on_consume_effects":[]}"#,
    ),
    ("Consumable.CODEC", r#"{"consume_seconds":-1.0}"#),
    ("DeathProtection.CODEC", "{}"),
    (
        "DeathProtection.CODEC",
        r#"{"death_effects":[{"type":"minecraft:clear_all_effects"}]}"#,
    ),
    (
        "SuspiciousStewEffects.CODEC",
        r#"[{"id":"minecraft:night_vision","duration":100}]"#,
    ),
    (
        "SuspiciousStewEffects.CODEC",
        r#"[{"id":"minecraft:night_vision"}]"#,
    ),
    ("SuspiciousStewEffects.CODEC", "[]"),
    (
        "UseCooldown.CODEC",
        r#"{"seconds":5.0,"cooldown_group":"minecraft:thing"}"#,
    ),
    ("UseCooldown.CODEC", r#"{"seconds":1}"#),
    ("UseCooldown.CODEC", r#"{"seconds":0}"#),
    ("UseEffects.CODEC", "{}"),
    (
        "UseEffects.CODEC",
        r#"{"can_sprint":true,"interact_vibrations":false,"speed_multiplier":0.5}"#,
    ),
    ("UseEffects.CODEC", r#"{"speed_multiplier":2.0}"#),
    ("UseRemainder.CODEC", r#"{"id":"minecraft:bowl","count":1}"#),
    ("UseRemainder.CODEC", r#""minecraft:bowl""#),
    ("OminousBottleAmplifier.CODEC", "0"),
    ("OminousBottleAmplifier.CODEC", "4"),
    ("OminousBottleAmplifier.CODEC", "5"),
    ("OminousBottleAmplifier.CODEC", "-1"),
    ("BlockState.CODEC", r#"{"Name":"minecraft:stone"}"#),
    (
        "BlockState.CODEC",
        r#"{"Name":"minecraft:oak_log","Properties":{"axis":"y"}}"#,
    ),
    (
        "BlockState.CODEC",
        r#"{"Name":"minecraft:oak_log","Properties":{"axis":"w"}}"#,
    ),
    ("BlockState.CODEC", r#"{"Name":"minecraft:nope"}"#),
    (
        "BlockState.CODEC",
        r#"{"Name":"minecraft:oak_stairs","Properties":{"facing":"north","half":"top","shape":"straight","waterlogged":"false"}}"#,
    ),
    ("IntProviders.CODEC", "5"),
    ("IntProviders.CODEC", "-3"),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:constant","value":4}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:uniform","min_inclusive":3,"max_inclusive":1}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:biased_to_bottom","min_inclusive":0,"max_inclusive":8}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:clamped","min_inclusive":1,"max_inclusive":4,"source":{"type":"minecraft:uniform","min_inclusive":0,"max_inclusive":10}}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:clamped_normal","mean":3.0,"deviation":1.0,"min_inclusive":0,"max_inclusive":6}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:weighted_list","distribution":[{"data":2,"weight":1},{"data":{"type":"minecraft:constant","value":3},"weight":2}]}"#,
    ),
    (
        "IntProviders.CODEC",
        r#"{"type":"minecraft:trapezoid","min":0,"max":10,"plateau":2}"#,
    ),
    ("FloatProviders.CODEC", "1.5"),
    (
        "FloatProviders.CODEC",
        r#"{"type":"minecraft:constant","value":0.5}"#,
    ),
    (
        "FloatProviders.CODEC",
        r#"{"type":"minecraft:uniform","min_inclusive":0.5,"max_exclusive":2.0}"#,
    ),
    (
        "FloatProviders.CODEC",
        r#"{"type":"minecraft:clamped_normal","mean":0.0,"deviation":1.0,"min":-2.0,"max":2.0}"#,
    ),
    (
        "FloatProviders.CODEC",
        r#"{"type":"minecraft:trapezoid","min":0.0,"max":6.0,"plateau":2.0}"#,
    ),
    ("TextColor.CODEC", r#""red""#),
    ("TextColor.CODEC", r##""#FF00AA""##),
    ("TextColor.CODEC", r##""#12""##),
    ("TextColor.CODEC", "16711680"),
    ("TextColor.CODEC", r#""nope""#),
    ("ComponentSerialization.CODEC", r#""hi""#),
    (
        "ComponentSerialization.CODEC",
        r#"{"text":"a","color":"red","bold":true}"#,
    ),
    ("ComponentSerialization.CODEC", r#"[{"text":"a"},"b"]"#),
    (
        "ComponentSerialization.CODEC",
        r#"{"translate":"chat.type.text","with":["a","b"]}"#,
    ),
    (
        "ComponentSerialization.CODEC",
        r#"{"text":"x","extra":["y",{"text":"z","italic":false}]}"#,
    ),
    ("ComponentSerialization.CODEC", r#"{"keybind":"key.jump"}"#),
    (
        "ComponentSerialization.CODEC",
        r#"{"score":{"name":"a","objective":"b"}}"#,
    ),
    ("ComponentSerialization.CODEC", r#"{"selector":"@a"}"#),
    (
        "ComponentSerialization.CODEC",
        r#"{"nbt":"a","block":"0 0 0"}"#,
    ),
    (
        "ComponentSerialization.CODEC",
        r#"{"text":"x","hover_event":{"action":"show_text","value":"y"},"click_event":{"action":"open_url","url":"https://example.com"}}"#,
    ),
    ("Xoroshiro128PlusPlus.CODEC", "[1,2]"),
    ("Xoroshiro128PlusPlus.CODEC", "[0,0]"),
    ("Xoroshiro128PlusPlus.CODEC", "[1,2,3]"),
    ("UUIDUtil.CODEC", "[1,2,3,4]"),
    ("UUIDUtil.CODEC", "[-1,-2,-3,-4]"),
    ("UUIDUtil.CODEC", "[1,2,3]"),
    ("Identifier.CODEC", r#""minecraft:stone""#),
    ("Identifier.CODEC", r#""stone""#),
    ("Identifier.CODEC", r#""Bad Id""#),
    ("Identifier.CODEC", r#""a:b:c""#),
    ("BlockPos.CODEC", "[1,2,3]"),
    ("BlockPos.CODEC", "[-30000000,0,30000000]"),
    ("BlockPos.CODEC", "[1,2]"),
    (
        "GlobalPos.CODEC",
        r#"{"dimension":"minecraft:overworld","pos":[1,2,3]}"#,
    ),
    (
        "GlobalPos.CODEC",
        r#"{"dimension":"minecraft:the_nether","pos":[0,-64,0]}"#,
    ),
    ("Rotations.CODEC", "[1.0,2.0,3.0]"),
    ("Rotations.CODEC", "[1.0]"),
    ("Rotations.CODEC", "[]"),
    ("Direction.CODEC", r#""north""#),
    ("Direction.CODEC", r#""up""#),
    ("Direction.CODEC", r#""sideways""#),
    ("Difficulty.CODEC", r#""easy""#),
    ("Difficulty.CODEC", r#""hard""#),
    ("Difficulty.CODEC", r#""impossible""#),
    ("GameType.CODEC", r#""survival""#),
    ("GameType.CODEC", r#""spectator""#),
    ("GameType.CODEC", r#""god""#),
    ("Heightmap.Types.CODEC", r#""WORLD_SURFACE_WG""#),
    ("Heightmap.Types.CODEC", r#""MOTION_BLOCKING""#),
    ("Heightmap.Types.CODEC", r#""world_surface""#),
    ("LiquidSettings.CODEC", r#""apply_waterlogging""#),
    ("LiquidSettings.CODEC", r#""ignore_waterlogging""#),
    ("LiquidSettings.CODEC", r#""other""#),
    ("HeightProvider.CODEC", r#"{"absolute":5}"#),
    (
        "HeightProvider.CODEC",
        r#"{"type":"minecraft:uniform","min_inclusive":{"absolute":0},"max_inclusive":{"above_bottom":10}}"#,
    ),
    (
        "HeightProvider.CODEC",
        r#"{"type":"minecraft:biased_to_bottom","min_inclusive":{"above_bottom":0},"max_inclusive":{"below_top":0},"inner":4}"#,
    ),
    (
        "HeightProvider.CODEC",
        r#"{"type":"minecraft:trapezoid","min_inclusive":{"absolute":0},"max_inclusive":{"absolute":64},"plateau":8}"#,
    ),
    (
        "HeightProvider.CODEC",
        r#"{"type":"minecraft:weighted_list","distribution":[{"data":{"absolute":1},"weight":3}]}"#,
    ),
    ("VerticalAnchor.CODEC", r#"{"absolute":5}"#),
    ("VerticalAnchor.CODEC", r#"{"above_bottom":3}"#),
    ("VerticalAnchor.CODEC", r#"{"below_top":2}"#),
    ("VerticalAnchor.CODEC", r#"{"absolute":2000}"#),
    ("VerticalAnchor.CODEC", r#"{"absolute":1,"above_bottom":2}"#),
    ("GeodeCrackSettings.CODEC", "{}"),
    (
        "GeodeCrackSettings.CODEC",
        r#"{"generate_crack_chance":0.5,"base_crack_size":1.0,"crack_point_offset":3}"#,
    ),
    (
        "GeodeCrackSettings.CODEC",
        r#"{"generate_crack_chance":2.0}"#,
    ),
    (
        "GeodeLayerSettings.CODEC",
        r#"{"filling":1.7,"inner_layer":2.2,"middle_layer":3.2,"outer_layer":4.2}"#,
    ),
    (
        "GeodeLayerSettings.CODEC",
        r#"{"filling":0.0,"inner_layer":2.2,"middle_layer":3.2,"outer_layer":4.2}"#,
    ),
    ("CarverDebugSettings.CODEC", "{}"),
    (
        "CarverDebugSettings.CODEC",
        r#"{"debug_mode":true,"air_state":{"Name":"minecraft:air"},"water_state":{"Name":"minecraft:water","Properties":{"level":"0"}}}"#,
    ),
    ("MobEffectInstance.CODEC", r#"{"id":"minecraft:speed"}"#),
    (
        "MobEffectInstance.CODEC",
        r#"{"id":"minecraft:speed","amplifier":1,"duration":100,"ambient":true,"show_particles":false,"show_icon":true}"#,
    ),
    (
        "MobEffectInstance.CODEC",
        r#"{"id":"minecraft:speed","amplifier":300,"duration":-1}"#,
    ),
    (
        "MobEffectInstance.CODEC",
        r#"{"id":"minecraft:speed","hidden_effect":{"id":"minecraft:slowness","duration":5}}"#,
    ),
    (
        "FeatureSize.CODEC",
        r#"{"type":"minecraft:two_layers_feature_size","limit":1,"lower_size":0,"upper_size":1}"#,
    ),
    (
        "FeatureSize.CODEC",
        r#"{"type":"minecraft:three_layers_feature_size","limit":1,"upper_limit":1,"lower_size":0,"middle_size":1,"upper_size":1,"min_clipped_height":5}"#,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:always_true"}"#,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:block_match","block":"minecraft:stone"}"#,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:blockstate_match","block_state":{"Name":"minecraft:oak_log","Properties":{"axis":"x"}}}"#,
    ),
    (
        "RuleTest.CODEC",
        r##"{"predicate_type":"minecraft:tag_match","tag":"minecraft:logs"}"##,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:random_block_match","block":"minecraft:stone","probability":0.5}"#,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:random_blockstate_match","block_state":{"Name":"minecraft:stone"},"probability":0.25}"#,
    ),
    (
        "RuleTest.CODEC",
        r#"{"predicate_type":"minecraft:random_block_match","block":"minecraft:stone","probability":2.0}"#,
    ),
    (
        "PosRuleTest.CODEC",
        r#"{"predicate_type":"minecraft:always_true"}"#,
    ),
    (
        "PosRuleTest.CODEC",
        r#"{"predicate_type":"minecraft:linear_pos","min_chance":0.0,"max_chance":1.0,"min_dist":0,"max_dist":10}"#,
    ),
    (
        "PosRuleTest.CODEC",
        r#"{"predicate_type":"minecraft:axis_aligned_linear_pos","axis":"y","min_chance":0.0,"max_chance":1.0,"min_dist":0,"max_dist":10}"#,
    ),
    (
        "RuleBlockEntityModifier.CODEC",
        r#"{"type":"minecraft:clear"}"#,
    ),
    (
        "RuleBlockEntityModifier.CODEC",
        r#"{"type":"minecraft:passthrough"}"#,
    ),
    (
        "RuleBlockEntityModifier.CODEC",
        r#"{"type":"minecraft:append_static","data":{"a":1}}"#,
    ),
    (
        "RuleBlockEntityModifier.CODEC",
        r#"{"type":"minecraft:append_loot","loot_table":"minecraft:chests/simple_dungeon"}"#,
    ),
    (
        "ProcessorRule.CODEC",
        r#"{"input_predicate":{"predicate_type":"minecraft:always_true"},"location_predicate":{"predicate_type":"minecraft:always_true"},"output_state":{"Name":"minecraft:stone"}}"#,
    ),
    (
        "ProcessorRule.CODEC",
        r#"{"input_predicate":{"predicate_type":"minecraft:block_match","block":"minecraft:stone"},"location_predicate":{"predicate_type":"minecraft:always_true"},"position_predicate":{"predicate_type":"minecraft:always_true"},"output_state":{"Name":"minecraft:air"},"block_entity_modifier":{"type":"minecraft:clear"}}"#,
    ),
    (
        "NormalNoise.NoiseParameters.DIRECT_CODEC",
        r#"{"firstOctave":-7,"amplitudes":[1.0,1.0]}"#,
    ),
    (
        "NormalNoise.NoiseParameters.DIRECT_CODEC",
        r#"{"firstOctave":0,"amplitudes":[]}"#,
    ),
    (
        "NormalNoise.NoiseParameters.DIRECT_CODEC",
        r#"{"firstOctave":-7}"#,
    ),
    ("WorldPreset.DIRECT_CODEC", r#"{"dimensions":{}}"#),
    (
        "DataPackConfig.CODEC",
        r#"{"Enabled":["vanilla"],"Disabled":[]}"#,
    ),
    (
        "DataPackConfig.CODEC",
        r#"{"Enabled":["vanilla","file/x"],"Disabled":["y"]}"#,
    ),
    ("DataPackConfig.CODEC", r#"{"Enabled":[]}"#),
    (
        "WorldDataConfiguration.CODEC",
        r#"{"DataPacks":{"Enabled":["vanilla"],"Disabled":[]},"enabled_features":["minecraft:vanilla"]}"#,
    ),
    (
        "WorldDataConfiguration.CODEC",
        r#"{"DataPacks":{"Enabled":["vanilla"],"Disabled":[]}}"#,
    ),
    ("FeatureFlags.CODEC", r#"["minecraft:vanilla"]"#),
    (
        "FeatureFlags.CODEC",
        r#"["minecraft:vanilla","minecraft:nope"]"#,
    ),
    ("FeatureFlags.CODEC", "[]"),
];

/// Replacement values for mutation: wrong types, boundaries, unknown names.
fn poison() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(1),
        json!(2147483648_i64),
        json!(1.5),
        json!(-0.0),
        json!(1e300),
        json!(""),
        json!("minecraft:nope"),
        json!("x"),
        json!([]),
        json!({}),
        json!([1]),
        json!({"a": 1}),
    ]
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Up to `count` deterministic mutants of `value`.
fn mutants(value: &Value, rng: &mut Rng, count: usize) -> Vec<Value> {
    let poison = poison();
    let pick = |rng: &mut Rng| poison[rng.below(poison.len() as u64) as usize].clone();
    let mut out = Vec::new();
    for round in 0..count {
        let mut mutant = value.clone();
        match &mut mutant {
            Value::Object(map) if !map.is_empty() => {
                let keys = object_keys(value);
                let key = keys[rng.below(keys.len() as u64) as usize].clone();
                if round % 3 == 0 {
                    map.remove(&key);
                } else if round % 3 == 1 {
                    map.insert(key, pick(rng));
                } else if let Some(Value::Object(inner)) = map.get_mut(&key) {
                    if let Some(first) = inner.keys().next().cloned() {
                        inner.insert(first, pick(rng));
                    } else {
                        map.insert(key, pick(rng));
                    }
                } else if let Some(Value::Array(items)) = map.get_mut(&key) {
                    if items.is_empty() {
                        items.push(pick(rng));
                    } else {
                        items.pop();
                    }
                } else {
                    map.insert(key, pick(rng));
                }
            }
            Value::Array(items) if !items.is_empty() => match round % 3 {
                0 => {
                    items.pop();
                }
                1 => items.push(pick(rng)),
                _ => {
                    let at = rng.below(items.len() as u64) as usize;
                    items[at] = pick(rng);
                }
            },
            _ => mutant = pick(rng),
        }
        if mutant != *value && !out.contains(&mutant) {
            out.push(mutant);
        }
    }
    out
}

/// The SNBT spelling of a JSON input (`JsonOps` -> `NbtOps` conversion).
fn to_snbt(value: &Value) -> String {
    use rivet_serialization::dynamic_ops::DynamicOps;
    let tag = JsonOps::INSTANCE.convert_to(&NbtOps::instance(), value);
    StringTagVisitor::to_string(&tag)
}

fn seeds_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rivet-reference-oracle/fixtures/ops/corpus-src/codec-datapack.jsonl")
}

fn line(id: String, codec: &str, ops: &str, input: Value) -> String {
    let mut object = Map::new();
    object.insert("id".into(), json!(id));
    object.insert("codec".into(), json!(codec));
    object.insert("ops".into(), json!(ops));
    object.insert("input".into(), input);
    serde_json::to_string(&Value::Object(object)).expect("JSON values always serialize") + "\n"
}

/// Hand-typed SNBT inputs whose numeric kinds the JSON conversion cannot reach.
const SNBT: &[(&str, &str)] = &[
    ("ItemStack.CODEC", "{id:'minecraft:stone',count:3b}"),
    ("ItemStack.CODEC", "{id:\"minecraft:stone\",count:3L}"),
    ("ItemStack.CODEC", "{id:'minecraft:stone',count:3.0d}"),
    (
        "ItemStack.CODEC",
        "{id:'minecraft:stone',count:2,components:{'minecraft:damage':3s}}",
    ),
    (
        "DataComponentPatch.CODEC",
        "{'minecraft:damage':3b,'minecraft:max_damage':100L}",
    ),
    (
        "DataComponentPatch.CODEC",
        "{'minecraft:custom_data':{a:1b,b:[1,2,3],c:[I;1,2],d:[B;1b]}}",
    ),
    ("BlockState.CODEC", "{Name:'minecraft:stone'}"),
    (
        "BlockState.CODEC",
        "{Name:'minecraft:oak_log',Properties:{axis:'y'}}",
    ),
    (
        "NormalNoise.NoiseParameters.DIRECT_CODEC",
        "{firstOctave:-7,amplitudes:[1.0d,1.0d]}",
    ),
    (
        "NormalNoise.NoiseParameters.DIRECT_CODEC",
        "{firstOctave:-7L,amplitudes:[1.0f,1b]}",
    ),
    ("UUIDUtil.CODEC", "[I;1,2,3,4]"),
    ("UUIDUtil.CODEC", "[1,2,3,4]"),
    ("BlockPos.CODEC", "[I;1,2,3]"),
    ("Xoroshiro128PlusPlus.CODEC", "[L;1L,2L]"),
    ("Rotations.CODEC", "[1.0f,2.0f,3.0f]"),
    ("IntProviders.CODEC", "5b"),
    ("IntProviders.CODEC", "5L"),
    ("IntProviders.CODEC", "5.5d"),
    ("TextColor.CODEC", "'red'"),
    ("OminousBottleAmplifier.CODEC", "3b"),
    ("OminousBottleAmplifier.CODEC", "3.0f"),
];

/// Build the corpus text.
pub fn emit() -> Result<String, String> {
    let known: std::collections::BTreeSet<&str> = codec::entries().iter().map(|e| e.name).collect();

    // Valid-ish inputs per codec, in a stable order: hand first, then harvested.
    let mut inputs: Vec<(String, String, Value)> = Vec::new(); // (codec, origin, json)
    for (n, (name, text)) in HAND.iter().enumerate() {
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("hand input {n} ({name}): {e}"))?;
        inputs.push(((*name).to_owned(), format!("hand{n:03}"), value));
    }
    let harvested = std::fs::read_to_string(seeds_path())
        .map_err(|e| format!("cannot read the data-pack seeds: {e}"))?;
    for (n, raw) in harvested.lines().enumerate() {
        let value: Value = serde_json::from_str(raw).map_err(|e| format!("seed {n}: {e}"))?;
        let name = value
            .get("codec")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("seed {n} lacks a codec"))?
            .to_owned();
        let json = value
            .get("json")
            .cloned()
            .ok_or_else(|| format!("seed {n} lacks json"))?;
        inputs.push((name, format!("pack{n:03}"), json));
    }
    for (name, origin, _) in &inputs {
        if !known.contains(name.as_str()) {
            return Err(format!("{origin} names unknown codec {name}"));
        }
    }

    let mut out = String::new();
    let mut per_codec: std::collections::BTreeMap<String, usize> = Default::default();
    for (name, origin, value) in &inputs {
        let n = per_codec.entry(name.clone()).or_default();
        let ordinal = *n;
        *n += 1;
        out += &line(format!("{name}.{origin}.json"), name, "json", value.clone());
        // The SNBT spelling of the first inputs of each codec.
        if ordinal < 6 {
            out += &line(
                format!("{name}.{origin}.nbt"),
                name,
                "nbt",
                json!(to_snbt(value)),
            );
        }
        let mut rng = Rng::for_stream(&format!("codec.{name}.{origin}"));
        for (m, mutant) in mutants(value, &mut rng, 6).iter().enumerate() {
            out += &line(
                format!("{name}.{origin}.mut{m}.json"),
                name,
                "json",
                mutant.clone(),
            );
            if ordinal < 4 && m < 2 {
                out += &line(
                    format!("{name}.{origin}.mut{m}.nbt"),
                    name,
                    "nbt",
                    json!(to_snbt(mutant)),
                );
            }
        }
    }
    for (n, (name, text)) in SNBT.iter().enumerate() {
        if !known.contains(name) {
            return Err(format!("snbt input {n} names unknown codec {name}"));
        }
        out += &line(format!("{name}.snbt{n:03}.nbt"), name, "nbt", json!(text));
    }
    // Every table entry also sees the bare poison values.
    for name in &known {
        for (n, value) in poison().iter().enumerate() {
            out += &line(
                format!("{name}.poison{n:02}.json"),
                name,
                "json",
                value.clone(),
            );
        }
    }
    Ok(out)
}
