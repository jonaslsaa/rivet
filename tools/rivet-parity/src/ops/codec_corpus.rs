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
    (
        "WorldBorder.Settings.CODEC",
        r#"{"center_x":0.0,"center_z":0.0,"damage_per_block":0.2,"safe_zone":5.0,"warning_blocks":5,"warning_time":15,"size":59999968.0,"lerp_time":0,"lerp_target":59999968.0}"#,
    ),
    (
        "WorldBorder.Settings.CODEC",
        r#"{"center_x":30000000.0,"center_z":0.0,"damage_per_block":0.2,"safe_zone":5.0,"warning_blocks":5,"warning_time":15,"size":100.0,"lerp_time":0,"lerp_target":100.0}"#,
    ),
    (
        "WorldBorder.Settings.CODEC",
        r#"{"center_x":1.5,"center_z":-2.5,"damage_per_block":1,"safe_zone":2,"warning_blocks":0,"warning_time":0,"size":10,"lerp_time":1000,"lerp_target":20}"#,
    ),
    ("WorldBorder.Settings.CODEC", r#"{"center_x":0.0}"#),
    (
        "WorldBorder.CODEC",
        r#"{"center_x":0.0,"center_z":0.0,"damage_per_block":0.2,"safe_zone":5.0,"warning_blocks":5,"warning_time":15,"size":59999968.0,"lerp_time":0,"lerp_target":59999968.0}"#,
    ),
    (
        "WeatherData.CODEC",
        r#"{"clear_weather_time":0,"rain_time":100,"thunder_time":200,"raining":true,"thundering":false}"#,
    ),
    (
        "WeatherData.CODEC",
        r#"{"clear_weather_time":-5,"rain_time":0,"thunder_time":0,"raining":false,"thundering":true}"#,
    ),
    (
        "WeatherData.CODEC",
        r#"{"clear_weather_time":0,"rain_time":100}"#,
    ),
    ("WanderingTraderData.CODEC", r#"{}"#),
    (
        "WanderingTraderData.CODEC",
        r#"{"spawn_delay":100,"spawn_chance":50}"#,
    ),
    (
        "WanderingTraderData.CODEC",
        r#"{"spawn_delay":24000,"spawn_chance":25}"#,
    ),
    ("WanderingTraderData.CODEC", r#"{"spawn_delay":"x"}"#),
    (
        "LevelSettings.DifficultySettings.CODEC",
        r#"{"difficulty":"hard","hardcore":false,"locked":true}"#,
    ),
    (
        "LevelSettings.DifficultySettings.CODEC",
        r#"{"difficulty":"peaceful","hardcore":true,"locked":false}"#,
    ),
    (
        "LevelSettings.DifficultySettings.CODEC",
        r#"{"difficulty":"easy"}"#,
    ),
    (
        "LevelData.RespawnData.CODEC",
        r#"{"dimension":"minecraft:overworld","pos":[1,64,-3],"yaw":90.0,"pitch":10.0}"#,
    ),
    (
        "LevelData.RespawnData.CODEC",
        r#"{"dimension":"minecraft:the_nether","pos":[0,0,0],"yaw":-180.0,"pitch":-90.0}"#,
    ),
    (
        "LevelData.RespawnData.CODEC",
        r#"{"dimension":"minecraft:overworld","pos":[1,64,-3],"yaw":200.0,"pitch":10.0}"#,
    ),
    (
        "BlendingData.Packed.CODEC",
        r#"{"min_section":-4,"max_section":20}"#,
    ),
    (
        "BlendingData.Packed.CODEC",
        r#"{"min_section":-4,"max_section":20,"heights":[1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0]}"#,
    ),
    (
        "BlendingData.Packed.CODEC",
        r#"{"min_section":0,"max_section":4,"heights":[1.0,2.0,3.0]}"#,
    ),
    (
        "NoiseSettings.CODEC",
        r#"{"min_y":-64,"height":384,"size_horizontal":1,"size_vertical":2}"#,
    ),
    (
        "NoiseSettings.CODEC",
        r#"{"min_y":0,"height":128,"size_horizontal":2,"size_vertical":1}"#,
    ),
    (
        "NoiseSettings.CODEC",
        r#"{"min_y":-60,"height":384,"size_horizontal":1,"size_vertical":2}"#,
    ),
    (
        "NoiseSettings.CODEC",
        r#"{"min_y":0,"height":100,"size_horizontal":1,"size_vertical":2}"#,
    ),
    (
        "NoiseSettings.CODEC",
        r#"{"min_y":-2032,"height":4064,"size_horizontal":5,"size_vertical":2}"#,
    ),
    ("ObjectiveCriteria.RenderType.CODEC", r#""integer""#),
    ("ObjectiveCriteria.RenderType.CODEC", r#""hearts""#),
    ("ObjectiveCriteria.RenderType.CODEC", r#""stars""#),
    ("ObjectiveCriteria.CODEC", r#""dummy""#),
    ("ObjectiveCriteria.CODEC", r#""deathCount""#),
    (
        "ObjectiveCriteria.CODEC",
        r#""minecraft.killed:minecraft.zombie""#,
    ),
    ("ObjectiveCriteria.CODEC", r#""teamkill.red""#),
    ("ObjectiveCriteria.CODEC", r#""bogus""#),
    ("DisplaySlot.CODEC", r#""list""#),
    ("DisplaySlot.CODEC", r#""sidebar.team.red""#),
    ("DisplaySlot.CODEC", r#""below_name""#),
    ("DisplaySlot.CODEC", r#""nope""#),
    ("NumberFormatTypes.CODEC", r#"{"type":"minecraft:blank"}"#),
    (
        "NumberFormatTypes.CODEC",
        r#"{"type":"minecraft:fixed","value":"x"}"#,
    ),
    (
        "NumberFormatTypes.CODEC",
        r#"{"type":"minecraft:styled","bold":true,"color":"red"}"#,
    ),
    ("NumberFormatTypes.CODEC", r#"{"type":"minecraft:nope"}"#),
    (
        "Objective.Packed.CODEC",
        r#"{"Name":"o","DisplayName":"Obj"}"#,
    ),
    (
        "Objective.Packed.CODEC",
        r#"{"Name":"o","CriteriaName":"deathCount","DisplayName":{"text":"Obj","color":"red"},"RenderType":"hearts","display_auto_update":true,"format":{"type":"minecraft:blank"}}"#,
    ),
    ("Objective.Packed.CODEC", r#"{"Name":"o"}"#),
    (
        "Scoreboard.PackedScore.CODEC",
        r#"{"Name":"Notch","Objective":"kills","Score":5,"Locked":true}"#,
    ),
    (
        "Scoreboard.PackedScore.CODEC",
        r#"{"Name":"Notch","Objective":"kills","display":"alias","format":{"type":"minecraft:blank"}}"#,
    ),
    ("Scoreboard.PackedScore.CODEC", r#"{"Name":"Notch"}"#),
    ("ScoreboardSaveData.Packed.CODEC", r#"{}"#),
    (
        "ScoreboardSaveData.Packed.CODEC",
        r#"{"Objectives":[{"Name":"o","DisplayName":"Obj"}],"PlayerScores":[{"Name":"a","Objective":"o","Score":1}],"DisplaySlots":{"sidebar":"o"},"Teams":[{"Name":"red","TeamColor":"red","Players":["a"]}]}"#,
    ),
    (
        "ScoreboardSaveData.Packed.CODEC",
        r#"{"DisplaySlots":{"nope":"o"}}"#,
    ),
    ("Team.Visibility.CODEC", r#""always""#),
    ("Team.Visibility.CODEC", r#""hideForOtherTeams""#),
    ("Team.Visibility.CODEC", r#""nope""#),
    ("Team.CollisionRule.CODEC", r#""pushOwnTeam""#),
    ("Team.CollisionRule.CODEC", r#""never""#),
    ("Team.CollisionRule.CODEC", r#""nope""#),
    (
        "PlayerTeam.Packed.CODEC",
        r#"{"Name":"red","TeamColor":"red","DisplayName":"Red","Players":["Notch"]}"#,
    ),
    (
        "PlayerTeam.Packed.CODEC",
        r#"{"Name":"t","TeamColor":"white","AllowFriendlyFire":false,"SeeFriendlyInvisibles":false,"MemberNamePrefix":"[","MemberNameSuffix":"]","NameTagVisibility":"never","DeathMessageVisibility":"hideForOwnTeam","CollisionRule":"pushOtherTeams"}"#,
    ),
    ("PlayerTeam.Packed.CODEC", r#"{"Name":"t"}"#),
    (
        "PlayerTeam.Packed.CODEC",
        r#"{"Name":"t","TeamColor":"nope"}"#,
    ),
    ("TeamColor.CODEC", r#""red""#),
    ("TeamColor.CODEC", r#""dark_blue""#),
    ("TeamColor.CODEC", r#""nope""#),
    ("Biome.Precipitation.CODEC", r#""none""#),
    ("Biome.Precipitation.CODEC", r#""snow""#),
    ("Biome.Precipitation.CODEC", r#""hail""#),
    ("Biome.TemperatureModifier.CODEC", r#""frozen""#),
    ("Biome.TemperatureModifier.CODEC", r#""none""#),
    ("Biome.TemperatureModifier.CODEC", r#""hot""#),
    (
        "BiomeSpecialEffects.CODEC",
        r##"{"water_color":"#3f76e4"}"##,
    ),
    (
        "BiomeSpecialEffects.CODEC",
        r##"{"water_color":"#3f76e4","foliage_color":"#123456","dry_foliage_color":"#654321","grass_color":"#aabbcc","grass_color_modifier":"swamp"}"##,
    ),
    ("BiomeSpecialEffects.CODEC", r#"{"water_color":4159204}"#),
    (
        "BiomeSpecialEffects.CODEC",
        r##"{"water_color":"#3f76e4","grass_color_modifier":"nope"}"##,
    ),
    ("BiomeSpecialEffects.CODEC", r#"{}"#),
    (
        "BiomeSpecialEffects.GrassColorModifier.CODEC",
        r#""dark_forest""#,
    ),
    ("BiomeSpecialEffects.GrassColorModifier.CODEC", r#""nope""#),
    ("Climate.Parameter.CODEC", r#"{"min":-0.5,"max":0.5}"#),
    ("Climate.Parameter.CODEC", r#"[-0.5,0.5]"#),
    ("Climate.Parameter.CODEC", r#"0.25"#),
    ("Climate.Parameter.CODEC", r#"{"min":0.5,"max":-0.5}"#),
    ("Climate.Parameter.CODEC", r#"[-3.0,0.0]"#),
    (
        "Climate.ParameterPoint.CODEC",
        r#"{"temperature":[-1.0,1.0],"humidity":[-1.0,1.0],"continentalness":[0.0,0.3],"erosion":[-1.0,1.0],"depth":0.0,"weirdness":[-1.0,1.0],"offset":0.5}"#,
    ),
    (
        "Climate.ParameterPoint.CODEC",
        r#"{"temperature":0.1,"humidity":0.2,"continentalness":0.3,"erosion":0.4,"depth":[0.0,1.0],"weirdness":0.6,"offset":1.0}"#,
    ),
    ("Climate.ParameterPoint.CODEC", r#"{"temperature":0.1}"#),
    (
        "MobSpawnSettings.MobSpawnCost.CODEC",
        r#"{"energy_budget":0.12,"charge":1.0}"#,
    ),
    (
        "MobSpawnSettings.MobSpawnCost.CODEC",
        r#"{"energy_budget":1,"charge":-2}"#,
    ),
    ("MobSpawnSettings.MobSpawnCost.CODEC", r#"{"charge":1.0}"#),
    (
        "MultiNoiseBiomeSourceParameterList.Preset.CODEC",
        r#""minecraft:nether""#,
    ),
    (
        "MultiNoiseBiomeSourceParameterList.Preset.CODEC",
        r#""minecraft:overworld""#,
    ),
    (
        "MultiNoiseBiomeSourceParameterList.Preset.CODEC",
        r#""minecraft:nope""#,
    ),
    (
        "MultiNoiseBiomeSourceParameterList.Preset.CODEC",
        r#""overworld""#,
    ),
    ("TickPriority.CODEC", r#"-3"#),
    ("TickPriority.CODEC", r#"0"#),
    ("TickPriority.CODEC", r#"3"#),
    ("TickPriority.CODEC", r#"100"#),
    ("TickPriority.CODEC", r#""x""#),
    ("MobCategory.CODEC", r#""monster""#),
    ("MobCategory.CODEC", r#""underground_water_creature""#),
    ("MobCategory.CODEC", r#""nope""#),
    ("GenerationStep.Decoration.CODEC", r#""raw_generation""#),
    (
        "GenerationStep.Decoration.CODEC",
        r#""top_layer_modification""#,
    ),
    ("GenerationStep.Decoration.CODEC", r#""nope""#),
    ("RarityFilter.CODEC", r#"{"chance":10}"#),
    ("RarityFilter.CODEC", r#"{"chance":0}"#),
    ("RarityFilter.CODEC", r#"{}"#),
    (
        "HeightRangePlacement.CODEC",
        r#"{"height":{"type":"minecraft:uniform","min_inclusive":{"absolute":0},"max_inclusive":{"absolute":64}}}"#,
    ),
    (
        "HeightRangePlacement.CODEC",
        r#"{"height":{"above_bottom":8}}"#,
    ),
    (
        "HeightRangePlacement.CODEC",
        r#"{"height":{"type":"minecraft:trapezoid","min_inclusive":{"absolute":-16},"max_inclusive":{"absolute":48},"plateau":10}}"#,
    ),
    ("HeightRangePlacement.CODEC", r#"{}"#),
    ("CountOnEveryLayerPlacement.CODEC", r#"{"count":4}"#),
    (
        "CountOnEveryLayerPlacement.CODEC",
        r#"{"count":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3}}"#,
    ),
    ("CountOnEveryLayerPlacement.CODEC", r#"{"count":300}"#),
    ("CountOnEveryLayerPlacement.CODEC", r#"{"count":-1}"#),
    ("FixedPlacement.CODEC", r#"{"positions":[[1,2,3],[4,5,6]]}"#),
    ("FixedPlacement.CODEC", r#"{"positions":[]}"#),
    ("FixedPlacement.CODEC", r#"{"positions":[[1,2]]}"#),
    ("SurfaceWaterDepthFilter.CODEC", r#"{"max_water_depth":2}"#),
    ("SurfaceWaterDepthFilter.CODEC", r#"{"max_water_depth":-2}"#),
    (
        "SurfaceWaterDepthFilter.CODEC",
        r#"{"max_water_depth":2.5}"#,
    ),
    ("CaveSurface.CODEC", r#""ceiling""#),
    ("CaveSurface.CODEC", r#""floor""#),
    ("CaveSurface.CODEC", r#""nope""#),
    ("InSquarePlacement.CODEC", r#"{}"#),
    ("InSquarePlacement.CODEC", r#"{"x":1}"#),
    (
        "HeightmapPlacement.CODEC",
        r#"{"heightmap":"MOTION_BLOCKING"}"#,
    ),
    (
        "HeightmapPlacement.CODEC",
        r#"{"heightmap":"OCEAN_FLOOR_WG"}"#,
    ),
    ("HeightmapPlacement.CODEC", r#"{"heightmap":"nope"}"#),
    (
        "RandomOffsetPlacement.CODEC",
        r#"{"xz_spread":{"type":"minecraft:uniform","min_inclusive":-3,"max_inclusive":3},"y_spread":0}"#,
    ),
    (
        "RandomOffsetPlacement.CODEC",
        r#"{"xz_spread":20,"y_spread":0}"#,
    ),
    (
        "RandomOffsetPlacement.CODEC",
        r#"{"xz_spread":-16,"y_spread":16}"#,
    ),
    (
        "SurfaceRelativeThresholdFilter.CODEC",
        r#"{"heightmap":"OCEAN_FLOOR_WG","min_inclusive":-2,"max_inclusive":5}"#,
    ),
    (
        "SurfaceRelativeThresholdFilter.CODEC",
        r#"{"heightmap":"WORLD_SURFACE_WG"}"#,
    ),
    (
        "SurfaceRelativeThresholdFilter.CODEC",
        r#"{"heightmap":"nope","min_inclusive":1}"#,
    ),
    (
        "CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC",
        r#"{"distance_factor":{"type":"minecraft:uniform","min_inclusive":0.75,"max_inclusive":1.0},"thickness":{"type":"minecraft:uniform","min_inclusive":0.75,"max_inclusive":1.0},"width_smoothness":3,"horizontal_radius_factor":{"type":"minecraft:uniform","min_inclusive":0.75,"max_inclusive":1.0},"vertical_radius_default_factor":1.0,"vertical_radius_center_factor":0.0}"#,
    ),
    (
        "CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC",
        r#"{"distance_factor":1.0,"thickness":0.5,"width_smoothness":0,"horizontal_radius_factor":1.0,"vertical_radius_default_factor":1.0,"vertical_radius_center_factor":0.0}"#,
    ),
    (
        "FoliagePlacer.CODEC",
        r#"{"type":"minecraft:blob_foliage_placer","radius":2,"offset":0,"height":3}"#,
    ),
    (
        "FoliagePlacer.CODEC",
        r#"{"type":"minecraft:random_spread_foliage_placer","radius":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3},"offset":0,"foliage_height":2,"leaf_placement_attempts":50}"#,
    ),
    (
        "FoliagePlacer.CODEC",
        r#"{"type":"minecraft:pine_foliage_placer","radius":2,"offset":1,"height":{"type":"minecraft:uniform","min_inclusive":3,"max_inclusive":5}}"#,
    ),
    ("FoliagePlacer.CODEC", r#"{"type":"minecraft:nope"}"#),
    (
        "FoliagePlacer.CODEC",
        r#"{"type":"minecraft:blob_foliage_placer","radius":99,"offset":0,"height":3}"#,
    ),
    ("CountConfiguration.CODEC", r#"{"count":5}"#),
    (
        "CountConfiguration.CODEC",
        r#"{"count":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":4}}"#,
    ),
    ("CountConfiguration.CODEC", r#"{"count":999}"#),
    (
        "BlockStateConfiguration.CODEC",
        r#"{"state":{"Name":"minecraft:stone"}}"#,
    ),
    (
        "BlockStateConfiguration.CODEC",
        r#"{"state":{"Name":"minecraft:oak_log","Properties":{"axis":"x"}}}"#,
    ),
    (
        "BlockStateConfiguration.CODEC",
        r#"{"state":{"Name":"minecraft:nope"}}"#,
    ),
    (
        "LayerConfiguration.CODEC",
        r#"{"height":4,"state":{"Name":"minecraft:snow","Properties":{"layers":"2"}}}"#,
    ),
    (
        "LayerConfiguration.CODEC",
        r#"{"height":5000,"state":{"Name":"minecraft:stone"}}"#,
    ),
    (
        "DeltaFeatureConfiguration.CODEC",
        r#"{"contents":{"Name":"minecraft:lava"},"rim":{"Name":"minecraft:basalt"},"size":{"type":"minecraft:uniform","min_inclusive":3,"max_inclusive":7},"rim_size":{"type":"minecraft:uniform","min_inclusive":0,"max_inclusive":2}}"#,
    ),
    (
        "DeltaFeatureConfiguration.CODEC",
        r#"{"contents":{"Name":"minecraft:lava"},"rim":{"Name":"minecraft:basalt"},"size":20,"rim_size":1}"#,
    ),
    (
        "ReplaceSphereConfiguration.CODEC",
        r#"{"target":{"Name":"minecraft:stone"},"state":{"Name":"minecraft:blackstone"},"radius":{"type":"minecraft:uniform","min_inclusive":3,"max_inclusive":7}}"#,
    ),
    (
        "ReplaceSphereConfiguration.CODEC",
        r#"{"target":{"Name":"minecraft:stone"},"state":{"Name":"minecraft:blackstone"},"radius":13}"#,
    ),
    ("NoneFeatureConfiguration.CODEC", r#"{}"#),
    ("NoneFeatureConfiguration.CODEC", r#"{"x":1}"#),
    (
        "ColumnFeatureConfiguration.CODEC",
        r#"{"reach":{"type":"minecraft:uniform","min_inclusive":0,"max_inclusive":2},"height":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":5}}"#,
    ),
    (
        "ColumnFeatureConfiguration.CODEC",
        r#"{"reach":5,"height":0}"#,
    ),
    (
        "UnderwaterMagmaConfiguration.CODEC",
        r#"{"floor_search_range":5,"placement_radius_around_floor":1,"placement_probability_per_valid_position":0.5}"#,
    ),
    (
        "UnderwaterMagmaConfiguration.CODEC",
        r#"{"floor_search_range":600,"placement_radius_around_floor":1,"placement_probability_per_valid_position":1.5}"#,
    ),
    ("ItemLore.CODEC", r#"[]"#),
    ("ItemLore.CODEC", r#"["a",{"text":"b","italic":false}]"#),
    ("ItemLore.CODEC", r#""x""#),
    ("CustomModelData.CODEC", r#"{}"#),
    (
        "CustomModelData.CODEC",
        r#"{"floats":[1.0,2.5],"flags":[true,false],"strings":["a","b"],"colors":[16711680,255]}"#,
    ),
    ("CustomModelData.CODEC", r#"{"colors":[[1.0,0.5,0.0]]}"#),
    ("DyedItemColor.CODEC", r#"16711680"#),
    ("DyedItemColor.CODEC", r#"[1.0,0.0,0.5]"#),
    ("DyedItemColor.CODEC", r#""red""#),
    ("MapItemColor.CODEC", r#"4603950"#),
    ("MapItemColor.CODEC", r#"-1"#),
    ("MapItemColor.CODEC", r#"1.5"#),
    ("FireworkExplosion.CODEC", r#"{"shape":"small_ball"}"#),
    (
        "FireworkExplosion.CODEC",
        r#"{"shape":"star","colors":[16711680,255],"fade_colors":[65280],"has_trail":true,"has_twinkle":true}"#,
    ),
    ("FireworkExplosion.CODEC", r#"{"shape":"nope"}"#),
    ("Fireworks.CODEC", r#"{}"#),
    (
        "Fireworks.CODEC",
        r#"{"flight_duration":2,"explosions":[{"shape":"burst"}]}"#,
    ),
    ("Fireworks.CODEC", r#"{"flight_duration":300}"#),
    ("WritableBookContent.CODEC", r#"{}"#),
    (
        "WritableBookContent.CODEC",
        r#"{"pages":["page one",{"text":"page two","raw":"x"}]}"#,
    ),
    ("WritableBookContent.CODEC", r#"{"pages":[1]}"#),
    ("BlockItemStateProperties.CODEC", r#"{}"#),
    (
        "BlockItemStateProperties.CODEC",
        r#"{"facing":"north","lit":"true"}"#,
    ),
    ("BlockItemStateProperties.CODEC", r#"{"lit":true}"#),
    ("CustomData.CODEC", r#"{}"#),
    ("CustomData.CODEC", r#"{"a":1,"b":[1,2,3],"c":{"d":"e"}}"#),
    ("CustomData.CODEC", r#"[1]"#),
    ("AttackRange.CODEC", r#"{}"#),
    (
        "AttackRange.CODEC",
        r#"{"min_reach":1.0,"max_reach":4.5,"min_creative_reach":0.5,"max_creative_reach":6.0,"hitbox_margin":0.5,"mob_factor":1.5}"#,
    ),
    ("AttackRange.CODEC", r#"{"min_reach":100.0}"#),
    ("SwingAnimation.CODEC", r#"{}"#),
    ("SwingAnimation.CODEC", r#"{"type":"stab","duration":12}"#),
    ("SwingAnimation.CODEC", r#"{"type":"nope","duration":0}"#),
    ("Weapon.CODEC", r#"{}"#),
    (
        "Weapon.CODEC",
        r#"{"item_damage_per_attack":2,"disable_blocking_for_seconds":5.0}"#,
    ),
    ("Weapon.CODEC", r#"{"item_damage_per_attack":-1}"#),
    (
        "OreConfiguration.CODEC",
        r#"{"size":9,"discard_chance_on_air_exposure":0.5,"targets":[{"target":{"predicate_type":"minecraft:tag_match","tag":"minecraft:stone_ore_replaceables"},"state":{"Name":"minecraft:coal_ore"}}]}"#,
    ),
    (
        "OreConfiguration.CODEC",
        r#"{"size":9,"discard_chance_on_air_exposure":0.0,"targets":[{"target":{"predicate_type":"minecraft:block_match","block":"minecraft:stone"},"state":{"Name":"minecraft:iron_ore"}}]}"#,
    ),
    (
        "OreConfiguration.CODEC",
        r#"{"size":65,"discard_chance_on_air_exposure":0.0,"targets":[]}"#,
    ),
    ("OreConfiguration.CODEC", r#"{"size":3,"targets":[]}"#),
    (
        "ReplaceBlockConfiguration.CODEC",
        r#"{"targets":[{"target":{"predicate_type":"minecraft:always_true"},"state":{"Name":"minecraft:stone"}}]}"#,
    ),
    ("ReplaceBlockConfiguration.CODEC", r#"{"targets":[]}"#),
    ("ReplaceBlockConfiguration.CODEC", r#"{}"#),
    (
        "EndSpikeConfiguration.CODEC",
        r#"{"crystal_invulnerable":true,"spikes":[{"centerX":1,"centerZ":-2,"radius":3,"height":76,"guarded":true}],"crystal_beam_target":[0,100,0]}"#,
    ),
    ("EndSpikeConfiguration.CODEC", r#"{"spikes":[]}"#),
    ("EndSpikeConfiguration.CODEC", r#"{"spikes":[{}]}"#),
    (
        "EndSpikeConfiguration.CODEC",
        r#"{"crystal_invulnerable":false}"#,
    ),
    (
        "EndSpikeFeature.EndSpike.CODEC",
        r#"{"centerX":5,"centerZ":6,"radius":2,"height":90,"guarded":false}"#,
    ),
    ("EndSpikeFeature.EndSpike.CODEC", r#"{}"#),
    ("EndSpikeFeature.EndSpike.CODEC", r#"{"radius":"big"}"#),
    (
        "TwistingVinesConfig.CODEC",
        r#"{"spread_width":8,"spread_height":4,"max_height":8}"#,
    ),
    (
        "TwistingVinesConfig.CODEC",
        r#"{"spread_width":0,"spread_height":4,"max_height":8}"#,
    ),
    (
        "SculkPatchConfiguration.CODEC",
        r#"{"charge_count":10,"amount_per_charge":1,"spread_attempts":64,"growth_rounds":0,"spread_rounds":1,"extra_rare_growths":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3},"catalyst_chance":0.5}"#,
    ),
    ("SculkPatchConfiguration.CODEC", r#"{"charge_count":0}"#),
    (
        "ProbabilityFeatureConfiguration.CODEC",
        r#"{"probability":0.5}"#,
    ),
    (
        "ProbabilityFeatureConfiguration.CODEC",
        r#"{"probability":1.5}"#,
    ),
    (
        "ProbabilityFeatureConfiguration.CODEC",
        r#"{"probability":-0.1}"#,
    ),
    (
        "EndGatewayConfiguration.CODEC",
        r#"{"exact":true,"exit":[1,2,3]}"#,
    ),
    ("EndGatewayConfiguration.CODEC", r#"{"exact":false}"#),
    ("EndGatewayConfiguration.CODEC", r#"{}"#),
    ("WorldOptions.CODEC", r#"{"seed":42}"#),
    (
        "WorldOptions.CODEC",
        r#"{"seed":-9223372036854775808,"generate_structures":false,"bonus_chest":true,"legacy_custom_options":"x"}"#,
    ),
    ("WorldOptions.CODEC", r#"{"generate_structures":true}"#),
    ("WorldOptions.CODEC", r#"{"seed":"abc"}"#),
    (
        "BelowZeroRetrogen.CODEC",
        r#"{"target_status":"minecraft:full"}"#,
    ),
    (
        "BelowZeroRetrogen.CODEC",
        r#"{"target_status":"minecraft:surface","missing_bedrock":[1,2]}"#,
    ),
    (
        "BelowZeroRetrogen.CODEC",
        r#"{"target_status":"minecraft:empty"}"#,
    ),
    (
        "BelowZeroRetrogen.CODEC",
        r#"{"target_status":"minecraft:bogus"}"#,
    ),
    (
        "WrittenBookContent.CODEC",
        r#"{"title":"T","author":"A","generation":1,"pages":["one",{"text":"two","color":"red"}],"resolved":true}"#,
    ),
    (
        "WrittenBookContent.CODEC",
        r#"{"title":{"raw":"T","filtered":"F"},"author":"A"}"#,
    ),
    (
        "WrittenBookContent.CODEC",
        r#"{"title":"T","author":"A","generation":4}"#,
    ),
    (
        "WrittenBookContent.CODEC",
        r#"{"title":"this title is much too long for a book","author":"A"}"#,
    ),
    ("WrittenBookContent.CODEC", r#"{"author":"A"}"#),
    (
        "LodestoneTracker.CODEC",
        r#"{"target":{"dimension":"minecraft:overworld","pos":[1,2,3]},"tracked":false}"#,
    ),
    ("LodestoneTracker.CODEC", r#"{}"#),
    (
        "LodestoneTracker.CODEC",
        r#"{"target":{"dimension":"x","pos":[1]}}"#,
    ),
    (
        "SeededContainerLoot.CODEC",
        r#"{"loot_table":"minecraft:chests/simple_dungeon","seed":123}"#,
    ),
    (
        "SeededContainerLoot.CODEC",
        r#"{"loot_table":"minecraft:chests/simple_dungeon"}"#,
    ),
    ("SeededContainerLoot.CODEC", r#"{"seed":1}"#),
    ("TooltipDisplay.CODEC", r#"{}"#),
    (
        "TooltipDisplay.CODEC",
        r#"{"hide_tooltip":true,"hidden_components":["minecraft:damage","minecraft:rarity"]}"#,
    ),
    (
        "TooltipDisplay.CODEC",
        r#"{"hidden_components":["minecraft:nope"]}"#,
    ),
    ("Enchantable.CODEC", r#"{"value":5}"#),
    ("Enchantable.CODEC", r#"{"value":0}"#),
    ("Enchantable.CODEC", r#"{}"#),
    (
        "BlocksAttacks.ItemDamageFunction.CODEC",
        r#"{"threshold":3.0,"base":1.0,"factor":0.5}"#,
    ),
    (
        "BlocksAttacks.ItemDamageFunction.CODEC",
        r#"{"threshold":-1.0,"base":1.0,"factor":0.5}"#,
    ),
    (
        "KineticWeapon.Condition.CODEC",
        r#"{"max_duration_ticks":20,"min_speed":1.5,"min_relative_speed":0.5}"#,
    ),
    (
        "KineticWeapon.Condition.CODEC",
        r#"{"max_duration_ticks":20}"#,
    ),
    (
        "KineticWeapon.Condition.CODEC",
        r#"{"max_duration_ticks":-1}"#,
    ),
    ("CountPlacement.CODEC", r#"{"count":4}"#),
    (
        "CountPlacement.CODEC",
        r#"{"count":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3}}"#,
    ),
    ("CountPlacement.CODEC", r#"{"count":5000}"#),
    ("CountPlacement.CODEC", r#"{"count":-1}"#),
    (
        "NoiseBasedCountPlacement.CODEC",
        r#"{"noise_to_count_ratio":10,"noise_factor":80.0,"noise_offset":0.3}"#,
    ),
    (
        "NoiseBasedCountPlacement.CODEC",
        r#"{"noise_to_count_ratio":10,"noise_factor":80.0}"#,
    ),
    (
        "NoiseThresholdCountPlacement.CODEC",
        r#"{"noise_level":-0.8,"below_noise":15,"above_noise":4}"#,
    ),
    (
        "NoiseThresholdCountPlacement.CODEC",
        r#"{"noise_level":-0.8}"#,
    ),
    ("BiomeFilter.CODEC", r#"{}"#),
    ("BiomeFilter.CODEC", r#"{"type":"minecraft:biome"}"#),
    ("FontDescription.CODEC", r#""minecraft:default""#),
    ("FontDescription.CODEC", r#""minecraft:uniform""#),
    ("FontDescription.CODEC", r#""Not A Valid Id""#),
    (
        "FontDescription.CODEC",
        r#"{"type":"atlas","atlas":"minecraft:blocks","sprite":"minecraft:block/stone"}"#,
    ),
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

/// Harvested data-pack documents per codec that also get structural mutants.
const MUTATED_HARVESTED: usize = 14;

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
    let mut harvested_per_codec: std::collections::BTreeMap<String, usize> = Default::default();
    for (name, origin, value) in &inputs {
        let n = per_codec.entry(name.clone()).or_default();
        let ordinal = *n;
        *n += 1;
        // Every harvested document is a corpus input; the structural mutants
        // (which multiply the corpus) cover every hand-typed input and the
        // first [`MUTATED_HARVESTED`] harvested documents of each codec.
        let mutate = if origin.starts_with("hand") {
            true
        } else {
            let h = harvested_per_codec.entry(name.clone()).or_default();
            *h += 1;
            *h <= MUTATED_HARVESTED
        };
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
        if !mutate {
            continue;
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
