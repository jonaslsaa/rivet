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
/// `entry!(enum name, path)` for `EnumCodec` ones (a concrete struct).
macro_rules! entry {
    (@build $name:literal, $make:ident) => {
        Entry {
            name: $name,
            json: |input| {
                let codec = $make::<JsonOps>();
                reencode(&*codec, &JsonOps::INSTANCE, input, render_json)
            },
            nbt: |input| {
                let codec = $make::<NbtOps>();
                reencode(&*codec, &NbtOps::instance(), input, StringTagVisitor::to_string)
            },
        }
    };
    (enum $name:literal, $make:ident) => {
        Entry {
            name: $name,
            json: |input| {
                let codec = $make::<JsonOps>();
                reencode(&codec, &JsonOps::INSTANCE, input, render_json)
            },
            nbt: |input| {
                let codec = $make::<NbtOps>();
                reencode(&codec, &NbtOps::instance(), input, StringTagVisitor::to_string)
            },
        }
    };
    ($name:literal, $make:ident) => {
        entry!(@build $name, $make)
    };
}

/// Every ported registry-free codec exposed to the op. Polymorphic roots
/// (`IntProviders.CODEC`, `RuleTest.CODEC`, `ItemStack.CODEC`, ...) reach every
/// variant codec registered under them, so those variants are verified through
/// their root rather than listed one by one.
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
