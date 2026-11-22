//! Consumable, food, status-effect, cooldown, and use-behavior item components:
//! - `FoodProperties` (`net.minecraft.world.food.FoodProperties`)
//! - `ItemUseAnimation` (`net.minecraft.world.item.ItemUseAnimation`)
//! - `ConsumeEffect` hierarchy (`net.minecraft.world.item.consume_effects.*`)
//! - `Consumable` (`net.minecraft.world.item.component.Consumable`)
//! - `DeathProtection` (`net.minecraft.world.item.component.DeathProtection`)
//! - `SuspiciousStewEffects` & `SuspiciousStewEntry` (`net.minecraft.world.item.component.SuspiciousStewEffects`)
//! - `UseCooldown` (`net.minecraft.world.item.component.UseCooldown`)
//! - `UseEffects` (`net.minecraft.world.item.component.UseEffects`)
//! - `UseRemainder` (`net.minecraft.world.item.component.UseRemainder`)
//! - `OminousBottleAmplifier` (`net.minecraft.world.item.component.OminousBottleAmplifier`)

use crate::effect::MobEffectInstance;
use crate::item::codec_from_fns;
use crate::item::components::core::{HolderSetRef, holder_set_ref_codec};
use crate::item::components::{
    decode_compound_tag, decode_identifier, encode_compound_tag, encode_identifier, get_map_field,
    validate_float_range, validate_int_range, validate_non_negative_float, validate_positive_float,
};
use crate::item::recursive_components::{ItemStackTemplate, item_stack_template_codec};
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// FoodProperties
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.food.FoodProperties`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoodProperties {
    pub nutrition: i32,
    pub saturation: f32,
    pub can_always_eat: bool,
}

impl FoodProperties {
    pub fn try_new(nutrition: i32, saturation: f32, can_always_eat: bool) -> Result<Self, String> {
        if nutrition < 0 {
            return Err(format!(
                "FoodProperties.nutrition must be non-negative: {nutrition}"
            ));
        }
        Ok(Self {
            nutrition,
            saturation,
            can_always_eat,
        })
    }
}

/// `FoodProperties.DIRECT_CODEC`.
pub fn food_properties_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FoodProperties, Ops>> {
    codec_from_fns(
        "food_properties",
        |v: &FoodProperties, ops: &Ops| {
            let mut entries = vec![
                rivet_serialization::Pair::of(
                    ops.create_string("nutrition".to_string()),
                    ops.create_int(v.nutrition),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("saturation".to_string()),
                    ops.create_float(v.saturation),
                ),
            ];
            if v.can_always_eat {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("can_always_eat".to_string()),
                    ops.create_boolean(true),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("FoodProperties must be a map");
            };
            let Some(n_val) = get_map_field(ops, map.as_ref(), "nutrition") else {
                return DataResult::error("FoodProperties missing required field 'nutrition'");
            };
            let Some(nutrition) = ops.get_number_value(&n_val).result().map(|n| n.int_value())
            else {
                return DataResult::error("FoodProperties.nutrition must be a number");
            };
            if nutrition < 0 {
                return DataResult::error(format!("Value must be non-negative: {nutrition}"));
            }
            let Some(s_val) = get_map_field(ops, map.as_ref(), "saturation") else {
                return DataResult::error("FoodProperties missing required field 'saturation'");
            };
            let Some(saturation) = ops
                .get_number_value(&s_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("FoodProperties.saturation must be a number");
            };
            let can_always_eat =
                if let Some(val) = get_map_field(ops, map.as_ref(), "can_always_eat") {
                    match ops.get_boolean_value(&val).result().copied() {
                        Some(b) => b,
                        None => {
                            return DataResult::error(
                                "FoodProperties.can_always_eat must be a boolean",
                            );
                        }
                    }
                } else {
                    false
                };
            DataResult::success(FoodProperties {
                nutrition,
                saturation,
                can_always_eat,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// ItemUseAnimation
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.ItemUseAnimation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ItemUseAnimation {
    None = 0,
    Eat = 1,
    Drink = 2,
    Block = 3,
    Bow = 4,
    Trident = 5,
    Crossbow = 6,
    Spyglass = 7,
    TootHorn = 8,
    Brush = 9,
    Bundle = 10,
    Spear = 11,
}

impl ItemUseAnimation {
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Eat => "eat",
            Self::Drink => "drink",
            Self::Block => "block",
            Self::Bow => "bow",
            Self::Trident => "trident",
            Self::Crossbow => "crossbow",
            Self::Spyglass => "spyglass",
            Self::TootHorn => "toot_horn",
            Self::Brush => "brush",
            Self::Bundle => "bundle",
            Self::Spear => "spear",
        }
    }

    pub fn from_serialized_name(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "eat" => Some(Self::Eat),
            "drink" => Some(Self::Drink),
            "block" => Some(Self::Block),
            "bow" => Some(Self::Bow),
            "trident" => Some(Self::Trident),
            "crossbow" => Some(Self::Crossbow),
            "spyglass" => Some(Self::Spyglass),
            "toot_horn" => Some(Self::TootHorn),
            "brush" => Some(Self::Brush),
            "bundle" => Some(Self::Bundle),
            "spear" => Some(Self::Spear),
            _ => None,
        }
    }

    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Eat,
            2 => Self::Drink,
            3 => Self::Block,
            4 => Self::Bow,
            5 => Self::Trident,
            6 => Self::Crossbow,
            7 => Self::Spyglass,
            8 => Self::TootHorn,
            9 => Self::Brush,
            10 => Self::Bundle,
            11 => Self::Spear,
            _ => Self::None,
        }
    }

    /// `ItemUseAnimation.hasCustomArmTransform()`.
    pub const fn has_custom_arm_transform(self) -> bool {
        matches!(self, Self::Spear)
    }
}

/// `ItemUseAnimation.CODEC`.
pub fn item_use_animation_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemUseAnimation, Ops>>
{
    codec_from_fns(
        "item_use_animation",
        |v: &ItemUseAnimation, ops: &Ops| {
            DataResult::success(ops.create_string(v.serialized_name().to_string()))
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                ItemUseAnimation::from_serialized_name(&s)
                    .map(DataResult::success)
                    .unwrap_or_else(|| DataResult::error(format!("Unknown ItemUseAnimation: {s}")))
            })
        },
    )
}

// ---------------------------------------------------------------------------
// ConsumeEffect hierarchy
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.consume_effects.ApplyStatusEffectsConsumeEffect`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApplyStatusEffectsConsumeEffect {
    pub effects: Vec<MobEffectInstance>,
    pub probability: f32,
}

impl ApplyStatusEffectsConsumeEffect {
    pub fn try_new(effects: Vec<MobEffectInstance>, probability: f32) -> Result<Self, String> {
        if let Some(err) = validate_float_range(probability, 0.0, 1.0, "probability").error_ref() {
            return Err(err.message().to_string());
        }
        Ok(Self {
            effects,
            probability,
        })
    }

    pub fn always(effects: Vec<MobEffectInstance>) -> Self {
        Self {
            effects,
            probability: 1.0,
        }
    }
}

/// Port of `net.minecraft.world.item.consume_effects.RemoveStatusEffectsConsumeEffect`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoveStatusEffectsConsumeEffect {
    pub effects: HolderSetRef,
}

/// Port of `net.minecraft.world.item.consume_effects.ClearAllStatusEffectsConsumeEffect`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ClearAllStatusEffectsConsumeEffect;

/// Port of `net.minecraft.world.item.consume_effects.TeleportRandomlyConsumeEffect`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeleportRandomlyConsumeEffect {
    pub diameter: f32,
}

impl TeleportRandomlyConsumeEffect {
    pub const DEFAULT_DIAMETER: f32 = 16.0;

    pub fn try_new(diameter: f32) -> Result<Self, String> {
        if let Some(err) = validate_positive_float(diameter, "diameter").error_ref() {
            return Err(err.message().to_string());
        }
        Ok(Self { diameter })
    }
}

impl Default for TeleportRandomlyConsumeEffect {
    fn default() -> Self {
        Self {
            diameter: Self::DEFAULT_DIAMETER,
        }
    }
}

/// Port of `net.minecraft.world.item.consume_effects.PlaySoundConsumeEffect`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaySoundConsumeEffect {
    pub sound: Identifier,
}

/// Port of `net.minecraft.world.item.consume_effects.ConsumeEffect` dispatched
/// on `"type"` (`apply_effects`, `remove_effects`, `clear_all_effects`,
/// `teleport_randomly`, `play_sound`).
#[derive(Clone, Debug, PartialEq)]
pub enum ConsumeEffect {
    ApplyEffects(ApplyStatusEffectsConsumeEffect),
    RemoveEffects(RemoveStatusEffectsConsumeEffect),
    ClearAllEffects(ClearAllStatusEffectsConsumeEffect),
    TeleportRandomly(TeleportRandomlyConsumeEffect),
    PlaySound(PlaySoundConsumeEffect),
}

impl ConsumeEffect {
    pub fn type_identifier(&self) -> Identifier {
        let path = match self {
            Self::ApplyEffects(_) => "apply_effects",
            Self::RemoveEffects(_) => "remove_effects",
            Self::ClearAllEffects(_) => "clear_all_effects",
            Self::TeleportRandomly(_) => "teleport_randomly",
            Self::PlaySound(_) => "play_sound",
        };
        Identifier::with_default_namespace(path)
    }
}

/// `ConsumeEffect.CODEC`.
pub fn consume_effect_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ConsumeEffect, Ops>> {
    let hs = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs);
    let dec_hs = hs;
    codec_from_fns(
        "consume_effect",
        move |v: &ConsumeEffect, ops: &Ops| {
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("type".to_string()),
                encode_identifier(&v.type_identifier(), ops),
            )];
            match v {
                ConsumeEffect::ApplyEffects(e) => {
                    let eff_list: Vec<Ops::Output> = e
                        .effects
                        .iter()
                        .map(|inst| encode_compound_tag(&inst.save_nbt(), ops))
                        .collect();
                    entries.push(rivet_serialization::Pair::of(
                        ops.create_string("effects".to_string()),
                        ops.create_list(eff_list),
                    ));
                    if e.probability != 1.0 {
                        entries.push(rivet_serialization::Pair::of(
                            ops.create_string("probability".to_string()),
                            ops.create_float(e.probability),
                        ));
                    }
                }
                ConsumeEffect::RemoveEffects(e) => {
                    let Some(ev) = enc_hs.encode_start(ops, &e.effects).result().cloned() else {
                        return DataResult::error("Failed to encode RemoveEffects.effects");
                    };
                    entries.push(rivet_serialization::Pair::of(
                        ops.create_string("effects".to_string()),
                        ev,
                    ));
                }
                ConsumeEffect::ClearAllEffects(_) => {}
                ConsumeEffect::TeleportRandomly(e) => {
                    if e.diameter != TeleportRandomlyConsumeEffect::DEFAULT_DIAMETER {
                        entries.push(rivet_serialization::Pair::of(
                            ops.create_string("diameter".to_string()),
                            ops.create_float(e.diameter),
                        ));
                    }
                }
                ConsumeEffect::PlaySound(e) => {
                    entries.push(rivet_serialization::Pair::of(
                        ops.create_string("sound".to_string()),
                        encode_identifier(&e.sound, ops),
                    ));
                }
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("ConsumeEffect must be a map");
            };
            let Some(ty_in) = get_map_field(ops, map.as_ref(), "type") else {
                return DataResult::error("ConsumeEffect missing required field 'type'");
            };
            let Some(ty) = decode_identifier(ops, &ty_in).result().cloned() else {
                return DataResult::error("Invalid ConsumeEffect 'type' identifier");
            };
            if ty.namespace() != "minecraft" {
                return DataResult::error(format!("Unknown ConsumeEffect type: {ty}"));
            }
            match ty.path() {
                "apply_effects" => {
                    let Some(eff_in) = get_map_field(ops, map.as_ref(), "effects") else {
                        return DataResult::error("apply_effects missing required field 'effects'");
                    };
                    let Some(items) = ops.get_stream(&eff_in).result().cloned() else {
                        return DataResult::error("apply_effects.effects must be a list");
                    };
                    let mut effects = Vec::with_capacity(items.len());
                    for item in &items {
                        let Some(tag) = decode_compound_tag(ops, item).result().cloned() else {
                            return DataResult::error(
                                "apply_effects.effects elements must be compounds",
                            );
                        };
                        let Some(inst) = MobEffectInstance::load_nbt(&tag) else {
                            return DataResult::error("Invalid MobEffectInstance in apply_effects");
                        };
                        effects.push(inst);
                    }
                    let probability = if let Some(p_in) =
                        get_map_field(ops, map.as_ref(), "probability")
                    {
                        let Some(f) = ops
                            .get_number_value(&p_in)
                            .result()
                            .map(|n| n.float_value())
                        else {
                            return DataResult::error("apply_effects.probability must be a number");
                        };
                        if let Some(err) =
                            validate_float_range(f, 0.0, 1.0, "probability").error_ref()
                        {
                            return DataResult::error(err.message().to_string());
                        }
                        f
                    } else {
                        1.0
                    };
                    DataResult::success(ConsumeEffect::ApplyEffects(
                        ApplyStatusEffectsConsumeEffect {
                            effects,
                            probability,
                        },
                    ))
                }
                "remove_effects" => {
                    let Some(eff_in) = get_map_field(ops, map.as_ref(), "effects") else {
                        return DataResult::error(
                            "remove_effects missing required field 'effects'",
                        );
                    };
                    dec_hs.parse(ops, &eff_in).map_owned(|effects| {
                        ConsumeEffect::RemoveEffects(RemoveStatusEffectsConsumeEffect { effects })
                    })
                }
                "clear_all_effects" => DataResult::success(ConsumeEffect::ClearAllEffects(
                    ClearAllStatusEffectsConsumeEffect,
                )),
                "teleport_randomly" => {
                    let diameter = if let Some(d_in) = get_map_field(ops, map.as_ref(), "diameter")
                    {
                        let Some(f) = ops
                            .get_number_value(&d_in)
                            .result()
                            .map(|n| n.float_value())
                        else {
                            return DataResult::error(
                                "teleport_randomly.diameter must be a number",
                            );
                        };
                        if let Some(err) = validate_positive_float(f, "diameter").error_ref() {
                            return DataResult::error(err.message().to_string());
                        }
                        f
                    } else {
                        TeleportRandomlyConsumeEffect::DEFAULT_DIAMETER
                    };
                    DataResult::success(ConsumeEffect::TeleportRandomly(
                        TeleportRandomlyConsumeEffect { diameter },
                    ))
                }
                "play_sound" => {
                    let Some(s_in) = get_map_field(ops, map.as_ref(), "sound") else {
                        return DataResult::error("play_sound missing required field 'sound'");
                    };
                    decode_identifier(ops, &s_in).map_owned(|sound| {
                        ConsumeEffect::PlaySound(PlaySoundConsumeEffect { sound })
                    })
                }
                _ => DataResult::error(format!("Unknown ConsumeEffect type: {ty}")),
            }
        },
    )
}

// ---------------------------------------------------------------------------
// Consumable
// ---------------------------------------------------------------------------

/// `Consumable.DEFAULT_CONSUME_SECONDS = 1.6F`.
pub const DEFAULT_CONSUME_SECONDS: f32 = 1.6;
const CONSUME_EFFECTS_INTERVAL: i32 = 4;
const CONSUME_EFFECTS_START_FRACTION: f32 = 0.21875;

/// Port of `net.minecraft.world.item.component.Consumable`.
#[derive(Clone, Debug, PartialEq)]
pub struct Consumable {
    pub consume_seconds: f32,
    pub animation: ItemUseAnimation,
    pub sound: Identifier,
    pub has_consume_particles: bool,
    pub on_consume_effects: Vec<ConsumeEffect>,
}

impl Consumable {
    pub fn default_sound() -> Identifier {
        Identifier::with_default_namespace("entity.generic.eat")
    }

    pub fn try_new(
        consume_seconds: f32,
        animation: ItemUseAnimation,
        sound: Identifier,
        has_consume_particles: bool,
        on_consume_effects: Vec<ConsumeEffect>,
    ) -> Result<Self, String> {
        if let Some(err) =
            validate_non_negative_float(consume_seconds, "consume_seconds").error_ref()
        {
            return Err(err.message().to_string());
        }
        Ok(Self {
            consume_seconds,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        })
    }

    /// `Consumable.consumeTicks() = (int)(this.consumeSeconds * 20.0F)`.
    pub fn consume_ticks(&self) -> i32 {
        (self.consume_seconds * 20.0_f32) as i32
    }

    /// Exact port of `Consumable.shouldEmitParticlesAndSounds(int useItemRemainingTicks)`.
    pub fn should_emit_particles_and_sounds(&self, use_item_remaining_ticks: i32) -> bool {
        let item_used_for_ticks = self.consume_ticks().wrapping_sub(use_item_remaining_ticks);
        let wait_ticks = (self.consume_ticks() as f32 * CONSUME_EFFECTS_START_FRACTION) as i32;
        let is_valid_time = item_used_for_ticks > wait_ticks;
        is_valid_time && use_item_remaining_ticks % CONSUME_EFFECTS_INTERVAL == 0
    }
}

impl Default for Consumable {
    fn default() -> Self {
        Self {
            consume_seconds: DEFAULT_CONSUME_SECONDS,
            animation: ItemUseAnimation::Eat,
            sound: Self::default_sound(),
            has_consume_particles: true,
            on_consume_effects: Vec::new(),
        }
    }
}

/// `Consumable.CODEC`.
pub fn consumable_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Consumable, Ops>> {
    let anim_codec = item_use_animation_codec::<Ops>();
    let eff_codec = consume_effect_codec::<Ops>();
    let enc_anim = Arc::clone(&anim_codec);
    let dec_anim = anim_codec;
    let enc_eff = Arc::clone(&eff_codec);
    let dec_eff = eff_codec;
    codec_from_fns(
        "consumable",
        move |v: &Consumable, ops: &Ops| {
            let mut entries = Vec::new();
            if v.consume_seconds != DEFAULT_CONSUME_SECONDS {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("consume_seconds".to_string()),
                    ops.create_float(v.consume_seconds),
                ));
            }
            if v.animation != ItemUseAnimation::Eat {
                match enc_anim.encode_start(ops, &v.animation).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("animation".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode Consumable.animation"),
                }
            }
            if v.sound != Consumable::default_sound() {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("sound".to_string()),
                    encode_identifier(&v.sound, ops),
                ));
            }
            if !v.has_consume_particles {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("has_consume_particles".to_string()),
                    ops.create_boolean(false),
                ));
            }
            if !v.on_consume_effects.is_empty() {
                let mut list = Vec::with_capacity(v.on_consume_effects.len());
                for eff in &v.on_consume_effects {
                    match enc_eff.encode_start(ops, eff).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => {
                            return DataResult::error(
                                "Failed to encode Consumable.on_consume_effects",
                            );
                        }
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("on_consume_effects".to_string()),
                    ops.create_list(list),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Consumable must be a map");
            };
            let consume_seconds = if let Some(val) =
                get_map_field(ops, map.as_ref(), "consume_seconds")
            {
                let Some(f) = ops.get_number_value(&val).result().map(|n| n.float_value()) else {
                    return DataResult::error("Consumable.consume_seconds must be a number");
                };
                if let Some(err) = validate_non_negative_float(f, "consume_seconds").error_ref() {
                    return DataResult::error(err.message().to_string());
                }
                f
            } else {
                DEFAULT_CONSUME_SECONDS
            };
            let animation = if let Some(val) = get_map_field(ops, map.as_ref(), "animation") {
                match dec_anim.parse(ops, &val).result().copied() {
                    Some(a) => a,
                    None => return DataResult::error("Invalid Consumable.animation"),
                }
            } else {
                ItemUseAnimation::Eat
            };
            let sound = if let Some(val) = get_map_field(ops, map.as_ref(), "sound") {
                match decode_identifier(ops, &val).result().cloned() {
                    Some(id) => id,
                    None => return DataResult::error("Invalid Consumable.sound"),
                }
            } else {
                Consumable::default_sound()
            };
            let has_consume_particles =
                if let Some(val) = get_map_field(ops, map.as_ref(), "has_consume_particles") {
                    match ops.get_boolean_value(&val).result().copied() {
                        Some(b) => b,
                        None => {
                            return DataResult::error(
                                "Consumable.has_consume_particles must be a boolean",
                            );
                        }
                    }
                } else {
                    true
                };
            let mut on_consume_effects = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "on_consume_effects") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("Consumable.on_consume_effects must be a list");
                };
                for item in &items {
                    match dec_eff.parse(ops, item).result().cloned() {
                        Some(e) => on_consume_effects.push(e),
                        None => {
                            return DataResult::error(
                                "Invalid entry in Consumable.on_consume_effects",
                            );
                        }
                    }
                }
            }
            DataResult::success(Consumable {
                consume_seconds,
                animation,
                sound,
                has_consume_particles,
                on_consume_effects,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// DeathProtection
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.DeathProtection`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeathProtection {
    pub death_effects: Vec<ConsumeEffect>,
}

impl DeathProtection {
    pub fn new(death_effects: Vec<ConsumeEffect>) -> Self {
        Self { death_effects }
    }
}

/// `DeathProtection.CODEC`.
pub fn death_protection_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DeathProtection, Ops>> {
    let eff = consume_effect_codec::<Ops>();
    let enc = Arc::clone(&eff);
    let dec = eff;
    codec_from_fns(
        "death_protection",
        move |v: &DeathProtection, ops: &Ops| {
            let mut entries = Vec::new();
            if !v.death_effects.is_empty() {
                let mut list = Vec::with_capacity(v.death_effects.len());
                for e in &v.death_effects {
                    match enc.encode_start(ops, e).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => {
                            return DataResult::error(
                                "Failed to encode DeathProtection.death_effects",
                            );
                        }
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("death_effects".to_string()),
                    ops.create_list(list),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("DeathProtection must be a map");
            };
            let mut death_effects = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "death_effects") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("DeathProtection.death_effects must be a list");
                };
                for item in &items {
                    match dec.parse(ops, item).result().cloned() {
                        Some(e) => death_effects.push(e),
                        None => {
                            return DataResult::error(
                                "Invalid entry in DeathProtection.death_effects",
                            );
                        }
                    }
                }
            }
            DataResult::success(DeathProtection { death_effects })
        },
    )
}

// ---------------------------------------------------------------------------
// SuspiciousStewEffects
// ---------------------------------------------------------------------------

/// `SuspiciousStewEffects.DEFAULT_DURATION = 160`.
pub const DEFAULT_STEW_DURATION: i32 = 160;

/// Port of `net.minecraft.world.item.component.SuspiciousStewEffects.Entry`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SuspiciousStewEntry {
    pub effect: Identifier,
    pub duration: i32,
}

impl SuspiciousStewEntry {
    pub fn new(effect: Identifier, duration: i32) -> Self {
        Self { effect, duration }
    }
}

/// Port of `net.minecraft.world.item.component.SuspiciousStewEffects`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SuspiciousStewEffects {
    pub effects: Vec<SuspiciousStewEntry>,
}

impl SuspiciousStewEffects {
    pub fn empty() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    pub fn new(effects: Vec<SuspiciousStewEntry>) -> Self {
        Self { effects }
    }

    /// `SuspiciousStewEffects.withEffectAdded(Entry)` — copies and appends.
    pub fn with_effect_added(&self, entry: SuspiciousStewEntry) -> Self {
        let mut next = self.effects.clone();
        next.push(entry);
        Self { effects: next }
    }
}

/// `SuspiciousStewEffects.CODEC`.
pub fn suspicious_stew_effects_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SuspiciousStewEffects, Ops>> {
    codec_from_fns(
        "suspicious_stew_effects",
        |v: &SuspiciousStewEffects, ops: &Ops| {
            let list: Vec<Ops::Output> = v
                .effects
                .iter()
                .map(|e| {
                    let mut entries = vec![rivet_serialization::Pair::of(
                        ops.create_string("id".to_string()),
                        encode_identifier(&e.effect, ops),
                    )];
                    if e.duration != DEFAULT_STEW_DURATION {
                        entries.push(rivet_serialization::Pair::of(
                            ops.create_string("duration".to_string()),
                            ops.create_int(e.duration),
                        ));
                    }
                    ops.create_map(entries)
                })
                .collect();
            DataResult::success(ops.create_list(list))
        },
        |ops: &Ops, input: &Ops::Output| {
            let Some(items) = ops.get_stream(input).result().cloned() else {
                return DataResult::error("SuspiciousStewEffects must be a list");
            };
            let mut effects = Vec::with_capacity(items.len());
            for item in &items {
                let map_res = ops.get_map(item);
                let Some(map) = map_res.result() else {
                    return DataResult::error("SuspiciousStewEffects.Entry must be a map");
                };
                let Some(id_in) = get_map_field(ops, map.as_ref(), "id") else {
                    return DataResult::error("SuspiciousStewEffects.Entry missing 'id'");
                };
                let Some(effect) = decode_identifier(ops, &id_in).result().cloned() else {
                    return DataResult::error("Invalid 'id' in SuspiciousStewEffects.Entry");
                };
                // Paper uses lenientOptionalFieldOf("duration", 160)
                let duration = match get_map_field(ops, map.as_ref(), "duration") {
                    Some(val) => ops
                        .get_number_value(&val)
                        .result()
                        .map(|n| n.int_value())
                        .unwrap_or(DEFAULT_STEW_DURATION),
                    None => DEFAULT_STEW_DURATION,
                };
                effects.push(SuspiciousStewEntry { effect, duration });
            }
            DataResult::success(SuspiciousStewEffects { effects })
        },
    )
}

// ---------------------------------------------------------------------------
// UseCooldown
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.UseCooldown`.
#[derive(Clone, Debug, PartialEq)]
pub struct UseCooldown {
    pub seconds: f32,
    pub cooldown_group: Option<Identifier>,
}

impl UseCooldown {
    pub fn try_new(seconds: f32, cooldown_group: Option<Identifier>) -> Result<Self, String> {
        if let Some(err) = validate_positive_float(seconds, "seconds").error_ref() {
            return Err(err.message().to_string());
        }
        Ok(Self {
            seconds,
            cooldown_group,
        })
    }

    /// `UseCooldown.ticks() = (int)(this.seconds * 20.0F)`.
    pub fn ticks(&self) -> i32 {
        (self.seconds * 20.0_f32) as i32
    }
}

/// `UseCooldown.CODEC`.
pub fn use_cooldown_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<UseCooldown, Ops>> {
    codec_from_fns(
        "use_cooldown",
        |v: &UseCooldown, ops: &Ops| {
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("seconds".to_string()),
                ops.create_float(v.seconds),
            )];
            if let Some(group) = &v.cooldown_group {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("cooldown_group".to_string()),
                    encode_identifier(group, ops),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("UseCooldown must be a map");
            };
            let Some(s_in) = get_map_field(ops, map.as_ref(), "seconds") else {
                return DataResult::error("UseCooldown missing required field 'seconds'");
            };
            let Some(seconds) = ops
                .get_number_value(&s_in)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("UseCooldown.seconds must be a number");
            };
            if let Some(err) = validate_positive_float(seconds, "seconds").error_ref() {
                return DataResult::error(err.message().to_string());
            }
            let cooldown_group =
                if let Some(g_in) = get_map_field(ops, map.as_ref(), "cooldown_group") {
                    match decode_identifier(ops, &g_in).result().cloned() {
                        Some(id) => Some(id),
                        None => return DataResult::error("Invalid UseCooldown.cooldown_group"),
                    }
                } else {
                    None
                };
            DataResult::success(UseCooldown {
                seconds,
                cooldown_group,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// UseEffects
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.UseEffects`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UseEffects {
    pub can_sprint: bool,
    pub interact_vibrations: bool,
    pub speed_multiplier: f32,
}

impl UseEffects {
    /// `UseEffects.DEFAULT = new UseEffects(false, true, 0.2F)`.
    pub const DEFAULT: Self = Self {
        can_sprint: false,
        interact_vibrations: true,
        speed_multiplier: 0.2,
    };

    pub fn try_new(
        can_sprint: bool,
        interact_vibrations: bool,
        speed_multiplier: f32,
    ) -> Result<Self, String> {
        if let Some(err) =
            validate_float_range(speed_multiplier, 0.0, 1.0, "speed_multiplier").error_ref()
        {
            return Err(err.message().to_string());
        }
        Ok(Self {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        })
    }
}

impl Default for UseEffects {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `UseEffects.CODEC`.
pub fn use_effects_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<UseEffects, Ops>> {
    codec_from_fns(
        "use_effects",
        |v: &UseEffects, ops: &Ops| {
            let mut entries = Vec::new();
            if v.can_sprint {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("can_sprint".to_string()),
                    ops.create_boolean(true),
                ));
            }
            if !v.interact_vibrations {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("interact_vibrations".to_string()),
                    ops.create_boolean(false),
                ));
            }
            if v.speed_multiplier != 0.2 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("speed_multiplier".to_string()),
                    ops.create_float(v.speed_multiplier),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("UseEffects must be a map");
            };
            let can_sprint = if let Some(val) = get_map_field(ops, map.as_ref(), "can_sprint") {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => return DataResult::error("UseEffects.can_sprint must be a boolean"),
                }
            } else {
                false
            };
            let interact_vibrations =
                if let Some(val) = get_map_field(ops, map.as_ref(), "interact_vibrations") {
                    match ops.get_boolean_value(&val).result().copied() {
                        Some(b) => b,
                        None => {
                            return DataResult::error(
                                "UseEffects.interact_vibrations must be a boolean",
                            );
                        }
                    }
                } else {
                    true
                };
            let speed_multiplier = if let Some(val) =
                get_map_field(ops, map.as_ref(), "speed_multiplier")
            {
                let Some(f) = ops.get_number_value(&val).result().map(|n| n.float_value()) else {
                    return DataResult::error("UseEffects.speed_multiplier must be a number");
                };
                if let Some(err) = validate_float_range(f, 0.0, 1.0, "speed_multiplier").error_ref()
                {
                    return DataResult::error(err.message().to_string());
                }
                f
            } else {
                0.2
            };
            DataResult::success(UseEffects {
                can_sprint,
                interact_vibrations,
                speed_multiplier,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// UseRemainder
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.UseRemainder`.
#[derive(Clone, Debug, PartialEq)]
pub struct UseRemainder {
    pub convert_into: ItemStackTemplate,
}

impl UseRemainder {
    pub fn new(convert_into: ItemStackTemplate) -> Self {
        Self { convert_into }
    }
}

/// `UseRemainder.CODEC = ItemStackTemplate.CODEC.xmap(UseRemainder::new, UseRemainder::convertInto)`.
pub fn use_remainder_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<UseRemainder, Ops>> {
    let tpl = item_stack_template_codec::<Ops>();
    let enc = Arc::clone(&tpl);
    let dec = tpl;
    codec_from_fns(
        "use_remainder",
        move |v: &UseRemainder, ops: &Ops| enc.encode_start(ops, &v.convert_into),
        move |ops: &Ops, input: &Ops::Output| dec.parse(ops, input).map_owned(UseRemainder::new),
    )
}

// ---------------------------------------------------------------------------
// OminousBottleAmplifier
// ---------------------------------------------------------------------------

/// `OminousBottleAmplifier.MIN_AMPLIFIER = 0`.
pub const MIN_OMINOUS_BOTTLE_AMPLIFIER: i32 = 0;
/// `OminousBottleAmplifier.MAX_AMPLIFIER = 4`.
pub const MAX_OMINOUS_BOTTLE_AMPLIFIER: i32 = 4;

/// Port of `net.minecraft.world.item.component.OminousBottleAmplifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OminousBottleAmplifier {
    pub value: i32,
}

impl OminousBottleAmplifier {
    pub fn try_new(value: i32) -> Result<Self, String> {
        if !(MIN_OMINOUS_BOTTLE_AMPLIFIER..=MAX_OMINOUS_BOTTLE_AMPLIFIER).contains(&value) {
            return Err(format!(
                "OminousBottleAmplifier must be in [{MIN_OMINOUS_BOTTLE_AMPLIFIER}; {MAX_OMINOUS_BOTTLE_AMPLIFIER}]: {value}"
            ));
        }
        Ok(Self { value })
    }
}

/// `OminousBottleAmplifier.CODEC = ExtraCodecs.intRange(0, 4).xmap(...)`.
pub fn ominous_bottle_amplifier_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<OminousBottleAmplifier, Ops>> {
    codec_from_fns(
        "ominous_bottle_amplifier",
        |v: &OminousBottleAmplifier, ops: &Ops| {
            validate_int_range(
                v.value,
                MIN_OMINOUS_BOTTLE_AMPLIFIER,
                MAX_OMINOUS_BOTTLE_AMPLIFIER,
                "ominous_bottle_amplifier",
            )
            .map_owned(|x| ops.create_int(x))
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_number_value(input).flat_map(|n| {
                validate_int_range(
                    n.int_value(),
                    MIN_OMINOUS_BOTTLE_AMPLIFIER,
                    MAX_OMINOUS_BOTTLE_AMPLIFIER,
                    "ominous_bottle_amplifier",
                )
                .map_owned(|value| OminousBottleAmplifier { value })
            })
        },
    )
}
