//! `codec.reencode`: the Rust twin of the reference oracle's op of that name.
//!
//! A request names a codec (`Class.CONSTANT`, the key of `CodecTable` in
//! `RivetReferenceOracle.java`), an ops flavour and an input. Paper decodes the
//! input with the named vanilla codec and re-encodes the result, answering
//! `{accept:true, canonical}` with the exact JSON text (or compact SNBT) the
//! encoder produced, or `{accept:false, phase:"decode"|"encode"}` naming the
//! phase that declined. The Rust handler does the same with the ported codec.
//!
//! `ops:"json"` carries the input as a JSON value (so there is no JSON text
//! layer to disagree about; `component.json` owns that) and `ops:"nbt"` as
//! SNBT text. A panic inside a ported codec is the Rust spelling of the Java
//! exception the Java op maps to the same decline.

use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_nbt::tag::Tag;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::map_codec;
use serde_json::{Map, Value, json};

use super::compare::Outcome;
use super::packet;

/// What one re-encode answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reencoded {
    Accept(String),
    Decline(&'static str),
}

/// One ported codec, instantiated under both ops flavours.
pub struct Entry {
    /// `Class.CONSTANT`, as the Java `CodecTable` names it.
    pub name: &'static str,
    json: fn(&Value) -> Reencoded,
    nbt: fn(&Tag) -> Reencoded,
}

/// Decode then re-encode with `codec`; a panic is a decline at its phase.
fn reencode<A, Ops, C>(
    codec: &C,
    ops: &Ops,
    input: &Ops::Output,
    render: impl Fn(&Ops::Output) -> String,
) -> Reencoded
where
    Ops: DynamicOps + 'static,
    C: Decoder<A, Ops> + Encoder<A, Ops> + ?Sized,
{
    let decoded = match packet::guarded(|| codec.parse(ops, input)) {
        Ok(decoded) => decoded,
        Err(_) => return Reencoded::Decline("decode"),
    };
    let Some(value) = decoded.result() else {
        return Reencoded::Decline("decode");
    };
    let encoded = match packet::guarded(|| codec.encode_start(ops, value)) {
        Ok(encoded) => encoded,
        Err(_) => return Reencoded::Decline("encode"),
    };
    match encoded.result() {
        Some(output) => Reencoded::Accept(render(output)),
        None => Reencoded::Decline("encode"),
    }
}

fn render_json(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON values always serialize")
}

/// One table row. `entry!(name, path)` is for `Arc<dyn Codec>` constructors;
/// `entry!(enum name, path)` for `EnumCodec` ones (a concrete struct);
/// `entry!(map name, path)` for `MapCodec` constructors (Java's `CODEC.codec()`).
macro_rules! entry {
    (@build $name:literal, $($make:ident)::+) => {
        Entry {
            name: $name,
            json: |input| {
                let codec = $($make)::+::<JsonOps>();
                reencode(&*codec, &JsonOps::INSTANCE, input, render_json)
            },
            nbt: |input| {
                let codec = $($make)::+::<NbtOps>();
                reencode(&*codec, &NbtOps::instance(), input, StringTagVisitor::to_string)
            },
        }
    };
    (map $name:literal, $($make:ident)::+) => {
        Entry {
            name: $name,
            json: |input| {
                let codec = map_codec::codec_of($($make)::+::<JsonOps>());
                reencode(&*codec, &JsonOps::INSTANCE, input, render_json)
            },
            nbt: |input| {
                let codec = map_codec::codec_of($($make)::+::<NbtOps>());
                reencode(&*codec, &NbtOps::instance(), input, StringTagVisitor::to_string)
            },
        }
    };
    (enum $name:literal, $($make:ident)::+) => {
        Entry {
            name: $name,
            json: |input| {
                let codec = $($make)::+::<JsonOps>();
                reencode(&codec, &JsonOps::INSTANCE, input, render_json)
            },
            nbt: |input| {
                let codec = $($make)::+::<NbtOps>();
                reencode(&codec, &NbtOps::instance(), input, StringTagVisitor::to_string)
            },
        }
    };
    ($name:literal, $($make:ident)::+) => {
        entry!(@build $name, $($make)::+)
    };
}

/// Every ported registry-free codec exposed to the op.
///
/// RivetTodo(#812): codecs deliberately not in this table: (1) the ported codecs that need a
/// `RegistryOpsLookup` (holders, tags, `RegistryFileCodec`: configured and placed features,
/// trunk and root placers, block predicates, state providers, processors, density functions,
/// `GeodeBlockSettings`, the flat-world settings), tracked by #803; (2) the MapCodec-only
/// constructors that no Java `Codec` constant wraps; (3) registry-free codecs not in this
/// table yet: `template_feature_configuration`, `level_stem`, `world_dimensions`,
/// `world_gen_settings`, `attribute_modifier_display`, `attribute_operation`, `ClickEvent`,
/// the `ServerStatus` forms, `NameAndId`, `WorldClocks`, and every function not yet triaged.
/// Polymorphic roots (`IntProviders.CODEC`, `RuleTest.CODEC`, `ItemStack.CODEC`, ...) reach the
/// variant codecs registered under them, so those variants are verified through their root
/// rather than listed one by one. The item components that carry registry ids (`Tool`,
/// `Repairable`, `MapDecorations`, ...) are here, but Rivet does not validate those ids (#811).
pub fn entries() -> Vec<Entry> {
    use rivet_registry::block_state_codec::block_state_codec;
    use rivet_registry::core::block_pos_codec;
    use rivet_registry::core::difficulty_codec;
    use rivet_registry::core::direction_codec;
    use rivet_registry::core::game_type_codec;
    use rivet_registry::core::global_pos_codec;
    use rivet_registry::core::rotations_codec;
    use rivet_registry::core::uuid_codec;
    use rivet_registry::identifier::identifier_codec;
    use rivet_text::component_serialization::codec as component_codec;
    use rivet_text::text_color::text_color_codec;
    use rivet_util::random::xoroshiro128plusplus_codec;
    use rivet_util::valueproviders::float_provider::float_provider_codec;
    use rivet_util::valueproviders::int_provider::int_provider_codec;
    use rivet_world::effect::instance::mob_effect_instance_codec;
    use rivet_world::flag::feature_flags::codec as feature_flags_codec;
    use rivet_world::item::components::consumable::*;
    use rivet_world::item::{
        bundle_contents_codec, charged_projectiles_codec, data_component_map_codec,
        data_component_patch_codec, item_container_contents_codec, item_stack_codec,
        item_stack_optional_codec, item_stack_template_codec,
    };
    use rivet_world::level::data_pack_config::codec as data_pack_config_codec;
    use rivet_world::level::world_data_configuration::codec as world_data_configuration_codec;
    use rivet_world::levelgen::carver::carver_debug_settings::carver_debug_settings_codec;
    use rivet_world::levelgen::feature::featuresize::feature_size::feature_size_codec;
    use rivet_world::levelgen::heightmap::types_codec as heightmap_types_codec;
    use rivet_world::levelgen::heightproviders::height_provider::height_provider_codec;
    use rivet_world::levelgen::presets::world_preset::direct_codec as world_preset_direct_codec;
    use rivet_world::levelgen::settings::geode_crack_settings::geode_crack_settings_codec;
    use rivet_world::levelgen::settings::geode_layer_settings::geode_layer_settings_codec;
    use rivet_world::levelgen::structure::templatesystem::liquid_settings::liquid_settings_codec;
    use rivet_world::levelgen::structure::templatesystem::pos_rule_test::pos_rule_test_codec;
    use rivet_world::levelgen::structure::templatesystem::processor_rule::processor_rule_codec;
    use rivet_world::levelgen::structure::templatesystem::rule_block_entity_modifier::rule_block_entity_modifier_codec;
    use rivet_world::levelgen::structure::templatesystem::rule_test::rule_test_codec;
    use rivet_world::levelgen::synth::normal_noise::noise_parameters_direct_codec;
    use rivet_world::levelgen::vertical_anchor::vertical_anchor_codec;
    vec![
        entry!("ItemStack.CODEC", item_stack_codec),
        entry!("ItemStack.OPTIONAL_CODEC", item_stack_optional_codec),
        entry!("DataComponentPatch.CODEC", data_component_patch_codec),
        entry!("DataComponentMap.CODEC", data_component_map_codec),
        entry!("ItemStackTemplate.CODEC", item_stack_template_codec),
        entry!("ItemContainerContents.CODEC", item_container_contents_codec),
        entry!("BundleContents.CODEC", bundle_contents_codec),
        entry!("ChargedProjectiles.CODEC", charged_projectiles_codec),
        entry!("FoodProperties.DIRECT_CODEC", food_properties_codec),
        entry!("ItemUseAnimation.CODEC", item_use_animation_codec),
        entry!("ConsumeEffect.CODEC", consume_effect_codec),
        entry!("Consumable.CODEC", consumable_codec),
        entry!("DeathProtection.CODEC", death_protection_codec),
        entry!("SuspiciousStewEffects.CODEC", suspicious_stew_effects_codec),
        entry!("UseCooldown.CODEC", use_cooldown_codec),
        entry!("UseEffects.CODEC", use_effects_codec),
        entry!("UseRemainder.CODEC", use_remainder_codec),
        entry!(
            "OminousBottleAmplifier.CODEC",
            ominous_bottle_amplifier_codec
        ),
        entry!("BlockState.CODEC", block_state_codec),
        entry!("IntProviders.CODEC", int_provider_codec),
        entry!("FloatProviders.CODEC", float_provider_codec),
        entry!("TextColor.CODEC", text_color_codec),
        entry!("ComponentSerialization.CODEC", component_codec),
        entry!("Xoroshiro128PlusPlus.CODEC", xoroshiro128plusplus_codec),
        entry!("UUIDUtil.CODEC", uuid_codec),
        entry!("Identifier.CODEC", identifier_codec),
        entry!("BlockPos.CODEC", block_pos_codec),
        entry!("GlobalPos.CODEC", global_pos_codec),
        entry!("Rotations.CODEC", rotations_codec),
        entry!("Direction.CODEC", direction_codec),
        entry!(enum "Difficulty.CODEC", difficulty_codec),
        entry!(enum "GameType.CODEC", game_type_codec),
        entry!(enum "Heightmap.Types.CODEC", heightmap_types_codec),
        entry!(enum "LiquidSettings.CODEC", liquid_settings_codec),
        entry!("HeightProvider.CODEC", height_provider_codec),
        entry!("VerticalAnchor.CODEC", vertical_anchor_codec),
        entry!("GeodeCrackSettings.CODEC", geode_crack_settings_codec),
        entry!("GeodeLayerSettings.CODEC", geode_layer_settings_codec),
        entry!("CarverDebugSettings.CODEC", carver_debug_settings_codec),
        entry!("MobEffectInstance.CODEC", mob_effect_instance_codec),
        entry!("FeatureSize.CODEC", feature_size_codec),
        entry!("RuleTest.CODEC", rule_test_codec),
        entry!("PosRuleTest.CODEC", pos_rule_test_codec),
        entry!(
            "RuleBlockEntityModifier.CODEC",
            rule_block_entity_modifier_codec
        ),
        entry!("ProcessorRule.CODEC", processor_rule_codec),
        entry!(
            "NormalNoise.NoiseParameters.DIRECT_CODEC",
            noise_parameters_direct_codec
        ),
        entry!("WorldPreset.DIRECT_CODEC", world_preset_direct_codec),
        entry!("DataPackConfig.CODEC", data_pack_config_codec),
        entry!(
            "WorldDataConfiguration.CODEC",
            world_data_configuration_codec
        ),
        entry!("FeatureFlags.CODEC", feature_flags_codec),
        entry!("WorldBorder.CODEC", rivet_world::level::border::world_border::WorldBorder::codec),
        entry!("WorldBorder.Settings.CODEC", rivet_world::level::border::world_border::Settings::codec),
        entry!("WeatherData.CODEC", rivet_world::level::saveddata::weather_data::WeatherData::codec),
        entry!("WanderingTraderData.CODEC", rivet_world::level::saveddata::wandering_trader_data::WanderingTraderData::codec),
        entry!("LevelSettings.DifficultySettings.CODEC", rivet_world::level::level_settings::DifficultySettings::codec),
        entry!("LevelData.RespawnData.CODEC", rivet_world::level::storage::level_data::respawn_data_codec),
        entry!("BlendingData.Packed.CODEC", rivet_world::levelgen::blending::blending_data::Packed::packed_codec),
        entry!("NoiseSettings.CODEC", rivet_world::levelgen::noise::noise_settings::noise_settings_codec),
        entry!("ObjectiveCriteria.RenderType.CODEC", rivet_world::scores::RenderType::codec),
        entry!("ObjectiveCriteria.CODEC", rivet_world::scores::ObjectiveCriteria::codec),
        entry!("DisplaySlot.CODEC", rivet_world::scores::DisplaySlot::codec),
        entry!("NumberFormatTypes.CODEC", rivet_world::scores::NumberFormat::codec),
        entry!("Objective.Packed.CODEC", rivet_world::scores::objective::PackedObjective::codec),
        entry!("ScoreboardSaveData.Packed.CODEC", rivet_world::scores::save_data::PackedScoreboardData::codec),
        entry!("Scoreboard.PackedScore.CODEC", rivet_world::scores::scoreboard::PackedScore::codec),
        entry!("Team.Visibility.CODEC", rivet_world::scores::team::Visibility::codec),
        entry!("Team.CollisionRule.CODEC", rivet_world::scores::team::CollisionRule::codec),
        entry!("PlayerTeam.Packed.CODEC", rivet_world::scores::team::PackedPlayerTeam::codec),
        entry!("TeamColor.CODEC", rivet_world::scores::team_color::TeamColor::codec),
        entry!("Biome.Precipitation.CODEC", rivet_world::biome::biome::Precipitation::codec),
        entry!("Biome.TemperatureModifier.CODEC", rivet_world::biome::biome::TemperatureModifier::codec),
        entry!("BiomeSpecialEffects.CODEC", rivet_world::biome::biome_special_effects::BiomeSpecialEffects::codec),
        entry!("BiomeSpecialEffects.GrassColorModifier.CODEC", rivet_world::biome::biome_special_effects::GrassColorModifier::codec),
        entry!("Climate.Parameter.CODEC", rivet_world::biome::climate::Parameter::codec),
        entry!("Climate.ParameterPoint.CODEC", rivet_world::biome::climate::ParameterPoint::codec),
        entry!("MobSpawnSettings.MobSpawnCost.CODEC", rivet_world::biome::mob_spawn_settings::MobSpawnCost::codec),
        entry!("MultiNoiseBiomeSourceParameterList.Preset.CODEC", rivet_world::biome::multi_noise_biome_source_parameter_list::Preset::codec),
        entry!("TickPriority.CODEC", rivet_world::ticks::TickPriority::codec),
        entry!("MobCategory.CODEC", rivet_world::entity::MobCategory::codec),
        entry!(enum "GenerationStep.Decoration.CODEC", rivet_world::levelgen::generation_step::decoration_codec),
        entry!("RarityFilter.CODEC", rivet_world::levelgen::placement::rarity_filter_codec),
        entry!("HeightRangePlacement.CODEC", rivet_world::levelgen::placement::height_range_placement_codec),
        entry!("CountOnEveryLayerPlacement.CODEC", rivet_world::levelgen::placement::count_on_every_layer_placement_codec),
        entry!("FixedPlacement.CODEC", rivet_world::levelgen::placement::fixed_placement_codec),
        entry!("SurfaceWaterDepthFilter.CODEC", rivet_world::levelgen::placement::surface_water_depth_filter_codec),
        entry!(enum "CaveSurface.CODEC", rivet_world::levelgen::placement::cave_surface_codec),
        entry!("InSquarePlacement.CODEC", rivet_world::levelgen::placement::in_square_placement_codec),
        entry!("HeightmapPlacement.CODEC", rivet_world::levelgen::placement::heightmap_placement_codec),
        entry!("RandomOffsetPlacement.CODEC", rivet_world::levelgen::placement::random_offset_placement_codec),
        entry!("SurfaceRelativeThresholdFilter.CODEC", rivet_world::levelgen::placement::surface_relative_threshold_filter_codec),
        entry!("CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC", rivet_world::levelgen::carver::canyon_carver_configuration::canyon_shape_configuration_codec),
        entry!("FoliagePlacer.CODEC", rivet_world::levelgen::feature::foliageplacers::foliage_placer::foliage_placer_codec),
        entry!("CountConfiguration.CODEC", rivet_world::levelgen::feature::configurations::count_configuration::count_configuration_codec),
        entry!("BlockStateConfiguration.CODEC", rivet_world::levelgen::feature::configurations::block_state_configuration::block_state_configuration_codec),
        entry!("LayerConfiguration.CODEC", rivet_world::levelgen::feature::configurations::layer_configuration::layer_configuration_codec),
        entry!("DeltaFeatureConfiguration.CODEC", rivet_world::levelgen::feature::configurations::delta_feature_configuration::delta_feature_configuration_codec),
        entry!("ReplaceSphereConfiguration.CODEC", rivet_world::levelgen::feature::configurations::replace_sphere_configuration::replace_sphere_configuration_codec),
        entry!("NoneFeatureConfiguration.CODEC", rivet_world::levelgen::feature::configurations::none_feature_configuration::none_feature_configuration_codec),
        entry!("ColumnFeatureConfiguration.CODEC", rivet_world::levelgen::feature::configurations::column_feature_configuration::column_feature_configuration_codec),
        entry!("UnderwaterMagmaConfiguration.CODEC", rivet_world::levelgen::feature::configurations::underwater_magma_configuration::underwater_magma_configuration_codec),
        entry!("ItemLore.CODEC", rivet_world::item::components::display::item_lore_codec),
        entry!("CustomModelData.CODEC", rivet_world::item::components::display::custom_model_data_codec),
        entry!("DyedItemColor.CODEC", rivet_world::item::components::display::dyed_item_color_codec),
        entry!("MapItemColor.CODEC", rivet_world::item::components::display::map_item_color_codec),
        entry!("FireworkExplosion.CODEC", rivet_world::item::components::world_and_books::firework_explosion_codec),
        entry!("Fireworks.CODEC", rivet_world::item::components::world_and_books::fireworks_codec),
        entry!("WritableBookContent.CODEC", rivet_world::item::components::world_and_books::writable_book_content_codec),
        entry!("BlockItemStateProperties.CODEC", rivet_world::item::components::world_and_books::block_item_state_properties_codec),
        entry!("CustomData.CODEC", rivet_world::item::components::core::custom_data_codec),
        entry!("AttackRange.CODEC", rivet_world::item::components::combat::attack_range_codec),
        entry!("SwingAnimation.CODEC", rivet_world::item::components::combat::swing_animation_codec),
        entry!("Weapon.CODEC", rivet_world::item::components::combat::weapon_codec),
        entry!("OreConfiguration.CODEC", rivet_world::levelgen::feature::configurations::ore_configuration::ore_configuration_codec),
        entry!("ReplaceBlockConfiguration.CODEC", rivet_world::levelgen::feature::configurations::replace_block_configuration::replace_block_configuration_codec),
        entry!("EndSpikeConfiguration.CODEC", rivet_world::levelgen::feature::configurations::end_spike_configuration::end_spike_configuration_codec),
        entry!("EndSpikeFeature.EndSpike.CODEC", rivet_world::levelgen::feature::configurations::end_spike_configuration::end_spike_codec),
        entry!("TwistingVinesConfig.CODEC", rivet_world::levelgen::feature::configurations::twisting_vines_config::twisting_vines_config_codec),
        entry!("SculkPatchConfiguration.CODEC", rivet_world::levelgen::feature::configurations::sculk_patch_configuration::sculk_patch_configuration_codec),
        entry!("ProbabilityFeatureConfiguration.CODEC", rivet_world::levelgen::feature::configurations::probability_feature_configuration::probability_feature_configuration_codec),
        entry!("EndGatewayConfiguration.CODEC", rivet_world::levelgen::feature::configurations::end_gateway_configuration::end_gateway_configuration_codec),
        entry!("WorldOptions.CODEC", rivet_world::levelgen::settings::world_options::world_options_codec),
        entry!("BelowZeroRetrogen.CODEC", rivet_world::levelgen::settings::below_zero_retrogen::below_zero_retrogen_codec),
        entry!("WrittenBookContent.CODEC", rivet_world::item::components::world_and_books::written_book_content_codec),
        entry!("LodestoneTracker.CODEC", rivet_world::item::components::world_and_books::lodestone_tracker_codec),
        entry!("SeededContainerLoot.CODEC", rivet_world::item::components::world_and_books::seeded_container_loot_codec),
        entry!("TooltipDisplay.CODEC", rivet_world::item::components::display::tooltip_display_codec),
        entry!("Enchantable.CODEC", rivet_world::item::components::combat::enchantable_codec),
        entry!("BlocksAttacks.ItemDamageFunction.CODEC", rivet_world::item::components::combat::item_damage_function_codec),
        entry!("KineticWeapon.Condition.CODEC", rivet_world::item::components::combat::kinetic_weapon_condition_codec),
        entry!(map "CountPlacement.CODEC", rivet_world::levelgen::placement::count_placement_codec),
        entry!(map "NoiseBasedCountPlacement.CODEC", rivet_world::levelgen::placement::noise_based_count_placement_codec),
        entry!(map "NoiseThresholdCountPlacement.CODEC", rivet_world::levelgen::placement::noise_threshold_count_placement_codec),
        entry!("BiomeFilter.CODEC", rivet_world::levelgen::placement::biome_filter_codec),
        entry!("FontDescription.CODEC", rivet_text::font_description::font_description_codec),
        entry!("Tool.CODEC", rivet_world::item::components::combat::tool_codec),
        entry!("Tool.Rule.CODEC", rivet_world::item::components::combat::tool_rule_codec),
        entry!("Repairable.CODEC", rivet_world::item::components::combat::repairable_codec),
        entry!("BlocksAttacks.CODEC", rivet_world::item::components::combat::blocks_attacks_codec),
        entry!("BlocksAttacks.DamageReduction.CODEC", rivet_world::item::components::combat::damage_reduction_codec),
        entry!("DamageResistant.CODEC", rivet_world::item::components::combat::damage_resistant_codec),
        entry!("KineticWeapon.CODEC", rivet_world::item::components::combat::kinetic_weapon_codec),
        entry!("PiercingWeapon.CODEC", rivet_world::item::components::combat::piercing_weapon_codec),
        entry!("SwingAnimationType.CODEC", rivet_world::item::components::combat::swing_animation_type_codec),
        entry!("MapDecorations.CODEC", rivet_world::item::components::display::map_decorations_codec),
        entry!("MapDecorations.Entry.CODEC", rivet_world::item::components::display::map_decoration_entry_codec),
        entry!("ItemAttributeModifiers.CODEC", rivet_world::item::components::world_and_books::item_attribute_modifiers_codec),
        entry!("ItemAttributeModifiers.Entry.CODEC", rivet_world::item::components::world_and_books::attribute_modifier_entry_codec),
        entry!("FireworkExplosion.Shape.CODEC", rivet_world::item::components::world_and_books::firework_shape_codec),
        entry!("InstrumentComponent.CODEC", rivet_world::item::components::world_and_books::instrument_component_codec),
        entry!("Instrument.DIRECT_CODEC", rivet_world::item::components::world_and_books::instrument_direct_codec),
        entry!("TrimMaterial.DIRECT_CODEC", rivet_world::item::components::world_and_books::trim_material_direct_codec),
        entry!("ProvidesTrimMaterial.CODEC", rivet_world::item::components::world_and_books::provides_trim_material_codec),
        entry!("SulfurCubeContent.CODEC", rivet_world::item::components::world_and_books::sulfur_cube_content_codec),
        entry!("IntProviders.NON_NEGATIVE_CODEC", rivet_util::valueproviders::int_provider::non_negative_int_provider_codec),
        entry!("IntProviders.POSITIVE_CODEC", rivet_util::valueproviders::int_provider::positive_int_provider_codec),
        entry!("Direction.Axis.CODEC", rivet_world::levelgen::structure::templatesystem::axis_codec::axis_codec),
        entry!("Biome.NETWORK_CODEC", rivet_world::biome::biome::Biome::network_codec),
    ]
}

thread_local! {
    static ENTRIES: Vec<Entry> = entries();
}

/// `codec.reencode`: `{codec, ops, input}` -> `{accept, phase?, canonical?}`.
pub fn codec_reencode(request: &Map<String, Value>) -> Outcome {
    let string = |key: &str| {
        request.get(key).and_then(Value::as_str).ok_or_else(|| {
            Outcome::Unavailable(format!("request property '{key}' must be a string"))
        })
    };
    let (name, ops) = match (string("codec"), string("ops")) {
        (Ok(n), Ok(o)) => (n, o),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let Some(input) = request.get("input") else {
        return Outcome::Unavailable("request property 'input' is required".into());
    };
    ENTRIES.with(|entries| {
        let Some(entry) = entries.iter().find(|e| e.name == name) else {
            return Outcome::Unavailable(format!("codec {name} is not in the Rust table"));
        };
        let answered = match ops {
            "json" => (entry.json)(input),
            "nbt" => {
                let Some(text) = input.as_str() else {
                    return Outcome::Unavailable("nbt input must be an SNBT string".into());
                };
                match crate::rust_parse_snbt(text) {
                    Ok(tag) => (entry.nbt)(&tag),
                    // `TagParser.parseFully` failing is the op's rejection verdict.
                    Err(_) => return Outcome::Rejected,
                }
            }
            other => return Outcome::Unavailable(format!("unknown ops: {other}")),
        };
        let mut result = Map::new();
        match answered {
            Reencoded::Accept(canonical) => {
                result.insert("accept".into(), json!(true));
                result.insert("canonical".into(), json!(canonical));
            }
            Reencoded::Decline(phase) => {
                result.insert("accept".into(), json!(false));
                result.insert("phase".into(), json!(phase));
            }
        }
        Outcome::Accepted(result)
    })
}

/// The scoreboard subject of a request: the codec it exercises.
pub fn subject_of(request: &Map<String, Value>) -> Option<String> {
    request.get("codec")?.as_str().map(str::to_owned)
}
