//! `net.minecraft.world.effect.MobEffect` and `net.minecraft.world.effect.MobEffects`.
//!
//! All 39 built-in Paper 26.2 mob effects with exact IDs, categories, colors,
//! attribute modifier templates, blend durations, added sounds, particles, and
//! tick-cadence formulas.

use rivet_protocol::game::clientbound_update_attributes_packet::Operation;

use crate::effect::{AMBIENT_ALPHA, AttributeTemplate, EffectParticle, MobEffectCategory, argb};

/// Built-in mob effect identifiers in exact Paper 26.2 registration order (`MobEffects.java`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum MobEffectId {
    Speed = 0,
    Slowness = 1,
    Haste = 2,
    MiningFatigue = 3,
    Strength = 4,
    InstantHealth = 5,
    InstantDamage = 6,
    JumpBoost = 7,
    Nausea = 8,
    Regeneration = 9,
    Resistance = 10,
    FireResistance = 11,
    WaterBreathing = 12,
    Invisibility = 13,
    Blindness = 14,
    NightVision = 15,
    Hunger = 16,
    Weakness = 17,
    Poison = 18,
    Wither = 19,
    HealthBoost = 20,
    Absorption = 21,
    Saturation = 22,
    Glowing = 23,
    Levitation = 24,
    Luck = 25,
    Unluck = 26,
    SlowFalling = 27,
    ConduitPower = 28,
    DolphinsGrace = 29,
    BadOmen = 30,
    HeroOfTheVillage = 31,
    Darkness = 32,
    TrialOmen = 33,
    RaidOmen = 34,
    WindCharged = 35,
    Weaving = 36,
    Oozing = 37,
    Infested = 38,
    BreathOfTheNautilus = 39,
}

impl MobEffectId {
    /// Total number of built-in mob effects in Paper 26.2.
    pub const COUNT: usize = 40;

    /// All built-in effects in registration order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Speed,
        Self::Slowness,
        Self::Haste,
        Self::MiningFatigue,
        Self::Strength,
        Self::InstantHealth,
        Self::InstantDamage,
        Self::JumpBoost,
        Self::Nausea,
        Self::Regeneration,
        Self::Resistance,
        Self::FireResistance,
        Self::WaterBreathing,
        Self::Invisibility,
        Self::Blindness,
        Self::NightVision,
        Self::Hunger,
        Self::Weakness,
        Self::Poison,
        Self::Wither,
        Self::HealthBoost,
        Self::Absorption,
        Self::Saturation,
        Self::Glowing,
        Self::Levitation,
        Self::Luck,
        Self::Unluck,
        Self::SlowFalling,
        Self::ConduitPower,
        Self::DolphinsGrace,
        Self::BadOmen,
        Self::HeroOfTheVillage,
        Self::Darkness,
        Self::TrialOmen,
        Self::RaidOmen,
        Self::WindCharged,
        Self::Weaving,
        Self::Oozing,
        Self::Infested,
        Self::BreathOfTheNautilus,
    ];

    /// Numeric registry ID (0-based, matching `BuiltInRegistries.MOB_EFFECT`).
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Look up by numeric registry ID.
    pub const fn by_id(id: i32) -> Option<Self> {
        if id >= 0 && (id as usize) < Self::COUNT {
            Some(Self::ALL[id as usize])
        } else {
            None
        }
    }

    /// Path component without namespace (e.g. `"speed"`, `"bad_omen"`).
    pub const fn path(self) -> &'static str {
        self.def().path
    }

    /// Full namespaced Minecraft identifier (e.g. `"minecraft:speed"`).
    pub const fn identifier(self) -> &'static str {
        self.def().identifier
    }

    /// Translation description ID (`"effect.minecraft.<path>"`).
    pub const fn description_id(self) -> &'static str {
        self.def().description_id
    }

    /// Look up by namespaced identifier (`"minecraft:speed"`) or bare path (`"speed"`).
    pub fn by_name(name: &str) -> Option<Self> {
        let bare = name.strip_prefix("minecraft:").unwrap_or(name);
        Self::ALL.into_iter().find(|effect| effect.path() == bare)
    }

    /// Return the static [`MobEffectDef`] for this built-in effect.
    #[inline]
    pub const fn def(self) -> &'static MobEffectDef {
        &MobEffects::DEFS[self as usize]
    }
}

/// Particle factory variant for a [`MobEffectDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleFactory {
    /// Default colored `minecraft:entity_effect` particle (alpha 38 if ambient, else 255).
    DefaultEntityEffect,
    /// Fixed particle identifier regardless of ambient flag.
    Fixed(&'static str),
}

/// Tick cadence strategy mirroring `MobEffect.shouldApplyEffectTickThisTick(int tickCount, int amplification)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickCadence {
    /// Default `MobEffect`: never applies per-tick logic (`false`).
    Never,
    /// Always applies every tick (`Hunger`, `Absorption`, `BadOmen`).
    Always,
    /// `InstantaneousMobEffect`: `remainingDuration >= 1`.
    Instantaneous,
    /// `RegenerationMobEffect`: `interval = 50 >> amp; interval <= 0 || tickCount % interval == 0`.
    Regeneration,
    /// `PoisonMobEffect`: `interval = 25 >> amp; interval <= 0 || tickCount % interval == 0`.
    Poison,
    /// `WitherMobEffect`: `interval = 40 >> amp; interval <= 0 || tickCount % interval == 0`.
    Wither,
    /// `RaidOmenMobEffect`: `tickCount == 1`.
    RaidOmen,
}

impl TickCadence {
    /// Exact Java `shouldApplyEffectTickThisTick(int tickCount, int amplification)`.
    ///
    /// Note: Java's signed `>>` shift on `int` masks the shift count to the lower 5 bits
    /// (`amplification & 31`), matching `i32::wrapping_shr(amplification as u32)`.
    #[inline]
    pub const fn should_apply(self, tick_count: i32, amplification: i32) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Instantaneous => tick_count >= 1,
            Self::Regeneration => {
                let interval = 50_i32.wrapping_shr(amplification as u32);
                interval <= 0 || tick_count % interval == 0
            }
            Self::Poison => {
                let interval = 25_i32.wrapping_shr(amplification as u32);
                interval <= 0 || tick_count % interval == 0
            }
            Self::Wither => {
                let interval = 40_i32.wrapping_shr(amplification as u32);
                interval <= 0 || tick_count % interval == 0
            }
            Self::RaidOmen => tick_count == 1,
        }
    }
}

/// Static definition of a `net.minecraft.world.effect.MobEffect`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobEffectDef {
    pub id: MobEffectId,
    pub path: &'static str,
    pub identifier: &'static str,
    pub description_id: &'static str,
    pub category: MobEffectCategory,
    pub color: i32,
    pub particle: ParticleFactory,
    pub blend_in_duration_ticks: i32,
    pub blend_out_duration_ticks: i32,
    pub blend_out_advance_ticks: i32,
    pub sound_on_added: Option<&'static str>,
    pub instantaneous: bool,
    pub cadence: TickCadence,
    pub attribute_modifiers: &'static [AttributeTemplate],
}

impl MobEffectDef {
    /// `MobEffect.isBeneficial()`.
    #[inline]
    pub const fn is_beneficial(&self) -> bool {
        matches!(self.category, MobEffectCategory::Beneficial)
    }

    /// `MobEffect.isInstantenous()`.
    #[inline]
    pub const fn is_instantaneous(&self) -> bool {
        self.instantaneous
    }

    /// `MobEffect.shouldApplyEffectTickThisTick(tickCount, amplification)`.
    #[inline]
    pub const fn should_apply_effect_tick_this_tick(
        &self,
        tick_count: i32,
        amplification: i32,
    ) -> bool {
        self.cadence.should_apply(tick_count, amplification)
    }

    /// `MobEffect.createParticleOptions(MobEffectInstance)`.
    #[inline]
    pub const fn create_particle_options(&self, ambient: bool) -> EffectParticle {
        match self.particle {
            ParticleFactory::DefaultEntityEffect => {
                let alpha = if ambient { AMBIENT_ALPHA } else { 255 };
                EffectParticle::EntityEffect {
                    argb: argb::color_with_alpha(alpha, self.color),
                }
            }
            ParticleFactory::Fixed(particle_id) => EffectParticle::Simple(particle_id),
        }
    }

    /// `MobEffect.getColor()` — opaque RGB int as passed to constructor.
    #[inline]
    pub const fn color(&self) -> i32 {
        self.color
    }
}

const SPEED_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:movement_speed",
    modifier_id: "minecraft:effect.speed",
    amount: 0.2,
    operation: Operation::AddMultipliedTotal,
}];

const SLOWNESS_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:movement_speed",
    modifier_id: "minecraft:effect.slowness",
    amount: -0.15,
    operation: Operation::AddMultipliedTotal,
}];

const HASTE_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:attack_speed",
    modifier_id: "minecraft:effect.haste",
    amount: 0.1,
    operation: Operation::AddMultipliedTotal,
}];

const MINING_FATIGUE_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:attack_speed",
    modifier_id: "minecraft:effect.mining_fatigue",
    amount: -0.1,
    operation: Operation::AddMultipliedTotal,
}];

const STRENGTH_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:attack_damage",
    modifier_id: "minecraft:effect.strength",
    amount: 3.0,
    operation: Operation::AddValue,
}];

const JUMP_BOOST_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:safe_fall_distance",
    modifier_id: "minecraft:effect.jump_boost",
    amount: 1.0,
    operation: Operation::AddValue,
}];

const INVISIBILITY_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:waypoint_transmit_range",
    modifier_id: "minecraft:effect.waypoint_transmit_range_hide",
    amount: -1.0,
    operation: Operation::AddMultipliedTotal,
}];

const WEAKNESS_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:attack_damage",
    modifier_id: "minecraft:effect.weakness",
    amount: -4.0,
    operation: Operation::AddValue,
}];

const HEALTH_BOOST_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:max_health",
    modifier_id: "minecraft:effect.health_boost",
    amount: 4.0,
    operation: Operation::AddValue,
}];

const ABSORPTION_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:max_absorption",
    modifier_id: "minecraft:effect.absorption",
    amount: 4.0,
    operation: Operation::AddValue,
}];

const LUCK_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:luck",
    modifier_id: "minecraft:effect.luck",
    amount: 1.0,
    operation: Operation::AddValue,
}];

const UNLUCK_MODIFIERS: &[AttributeTemplate] = &[AttributeTemplate {
    attribute: "minecraft:luck",
    modifier_id: "minecraft:effect.unluck",
    amount: -1.0,
    operation: Operation::AddValue,
}];

/// Built-in `net.minecraft.world.effect.MobEffects` registry table.
pub struct MobEffects;

impl MobEffects {
    /// All 40 built-in Paper 26.2 mob effect definitions indexed by `MobEffectId as usize`.
    pub const DEFS: [MobEffectDef; MobEffectId::COUNT] = [
        MobEffectDef {
            id: MobEffectId::Speed,
            path: "speed",
            identifier: "minecraft:speed",
            description_id: "effect.minecraft.speed",
            category: MobEffectCategory::Beneficial,
            color: 3_402_751,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: SPEED_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Slowness,
            path: "slowness",
            identifier: "minecraft:slowness",
            description_id: "effect.minecraft.slowness",
            category: MobEffectCategory::Harmful,
            color: 9_154_528,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: SLOWNESS_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Haste,
            path: "haste",
            identifier: "minecraft:haste",
            description_id: "effect.minecraft.haste",
            category: MobEffectCategory::Beneficial,
            color: 14_270_531,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: HASTE_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::MiningFatigue,
            path: "mining_fatigue",
            identifier: "minecraft:mining_fatigue",
            description_id: "effect.minecraft.mining_fatigue",
            category: MobEffectCategory::Harmful,
            color: 4_866_583,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: MINING_FATIGUE_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Strength,
            path: "strength",
            identifier: "minecraft:strength",
            description_id: "effect.minecraft.strength",
            category: MobEffectCategory::Beneficial,
            color: 16_762_624,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: STRENGTH_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::InstantHealth,
            path: "instant_health",
            identifier: "minecraft:instant_health",
            description_id: "effect.minecraft.instant_health",
            category: MobEffectCategory::Beneficial,
            color: 16_262_179,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: true,
            cadence: TickCadence::Instantaneous,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::InstantDamage,
            path: "instant_damage",
            identifier: "minecraft:instant_damage",
            description_id: "effect.minecraft.instant_damage",
            category: MobEffectCategory::Harmful,
            color: 11_101_546,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: true,
            cadence: TickCadence::Instantaneous,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::JumpBoost,
            path: "jump_boost",
            identifier: "minecraft:jump_boost",
            description_id: "effect.minecraft.jump_boost",
            category: MobEffectCategory::Beneficial,
            color: 16_646_020,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: JUMP_BOOST_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Nausea,
            path: "nausea",
            identifier: "minecraft:nausea",
            description_id: "effect.minecraft.nausea",
            category: MobEffectCategory::Harmful,
            color: 5_578_058,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 150,
            blend_out_duration_ticks: 20,
            blend_out_advance_ticks: 60,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Regeneration,
            path: "regeneration",
            identifier: "minecraft:regeneration",
            description_id: "effect.minecraft.regeneration",
            category: MobEffectCategory::Beneficial,
            color: 13_458_603,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Regeneration,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Resistance,
            path: "resistance",
            identifier: "minecraft:resistance",
            description_id: "effect.minecraft.resistance",
            category: MobEffectCategory::Beneficial,
            color: 9_520_880,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::FireResistance,
            path: "fire_resistance",
            identifier: "minecraft:fire_resistance",
            description_id: "effect.minecraft.fire_resistance",
            category: MobEffectCategory::Beneficial,
            color: 16_750_848,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::WaterBreathing,
            path: "water_breathing",
            identifier: "minecraft:water_breathing",
            description_id: "effect.minecraft.water_breathing",
            category: MobEffectCategory::Beneficial,
            color: 10_017_472,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Invisibility,
            path: "invisibility",
            identifier: "minecraft:invisibility",
            description_id: "effect.minecraft.invisibility",
            category: MobEffectCategory::Beneficial,
            color: 16_185_078,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: INVISIBILITY_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Blindness,
            path: "blindness",
            identifier: "minecraft:blindness",
            description_id: "effect.minecraft.blindness",
            category: MobEffectCategory::Harmful,
            color: 2_039_587,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::NightVision,
            path: "night_vision",
            identifier: "minecraft:night_vision",
            description_id: "effect.minecraft.night_vision",
            category: MobEffectCategory::Beneficial,
            color: 12_779_366,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Hunger,
            path: "hunger",
            identifier: "minecraft:hunger",
            description_id: "effect.minecraft.hunger",
            category: MobEffectCategory::Harmful,
            color: 5_797_459,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Always,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Weakness,
            path: "weakness",
            identifier: "minecraft:weakness",
            description_id: "effect.minecraft.weakness",
            category: MobEffectCategory::Harmful,
            color: 4_738_376,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: WEAKNESS_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Poison,
            path: "poison",
            identifier: "minecraft:poison",
            description_id: "effect.minecraft.poison",
            category: MobEffectCategory::Harmful,
            color: 8_889_187,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Poison,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Wither,
            path: "wither",
            identifier: "minecraft:wither",
            description_id: "effect.minecraft.wither",
            category: MobEffectCategory::Harmful,
            color: 7_561_558,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Wither,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::HealthBoost,
            path: "health_boost",
            identifier: "minecraft:health_boost",
            description_id: "effect.minecraft.health_boost",
            category: MobEffectCategory::Beneficial,
            color: 16_284_963,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: HEALTH_BOOST_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Absorption,
            path: "absorption",
            identifier: "minecraft:absorption",
            description_id: "effect.minecraft.absorption",
            category: MobEffectCategory::Beneficial,
            color: 2_445_989,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Always,
            attribute_modifiers: ABSORPTION_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Saturation,
            path: "saturation",
            identifier: "minecraft:saturation",
            description_id: "effect.minecraft.saturation",
            category: MobEffectCategory::Beneficial,
            color: 16_262_179,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: true,
            cadence: TickCadence::Instantaneous,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Glowing,
            path: "glowing",
            identifier: "minecraft:glowing",
            description_id: "effect.minecraft.glowing",
            category: MobEffectCategory::Neutral,
            color: 9_740_385,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Levitation,
            path: "levitation",
            identifier: "minecraft:levitation",
            description_id: "effect.minecraft.levitation",
            category: MobEffectCategory::Harmful,
            color: 13_565_951,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Luck,
            path: "luck",
            identifier: "minecraft:luck",
            description_id: "effect.minecraft.luck",
            category: MobEffectCategory::Beneficial,
            color: 5_882_118,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: LUCK_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::Unluck,
            path: "unluck",
            identifier: "minecraft:unluck",
            description_id: "effect.minecraft.unluck",
            category: MobEffectCategory::Harmful,
            color: 12_624_973,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: UNLUCK_MODIFIERS,
        },
        MobEffectDef {
            id: MobEffectId::SlowFalling,
            path: "slow_falling",
            identifier: "minecraft:slow_falling",
            description_id: "effect.minecraft.slow_falling",
            category: MobEffectCategory::Beneficial,
            color: 15_978_425,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::ConduitPower,
            path: "conduit_power",
            identifier: "minecraft:conduit_power",
            description_id: "effect.minecraft.conduit_power",
            category: MobEffectCategory::Beneficial,
            color: 1_950_417,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::DolphinsGrace,
            path: "dolphins_grace",
            identifier: "minecraft:dolphins_grace",
            description_id: "effect.minecraft.dolphins_grace",
            category: MobEffectCategory::Beneficial,
            color: 8_954_814,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::BadOmen,
            path: "bad_omen",
            identifier: "minecraft:bad_omen",
            description_id: "effect.minecraft.bad_omen",
            category: MobEffectCategory::Neutral,
            color: 745_784,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:event.mob_effect.bad_omen"),
            instantaneous: false,
            cadence: TickCadence::Always,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::HeroOfTheVillage,
            path: "hero_of_the_village",
            identifier: "minecraft:hero_of_the_village",
            description_id: "effect.minecraft.hero_of_the_village",
            category: MobEffectCategory::Beneficial,
            color: 4_521_796,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Darkness,
            path: "darkness",
            identifier: "minecraft:darkness",
            description_id: "effect.minecraft.darkness",
            category: MobEffectCategory::Harmful,
            color: 2_696_993,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 22,
            blend_out_duration_ticks: 22,
            blend_out_advance_ticks: 22,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::TrialOmen,
            path: "trial_omen",
            identifier: "minecraft:trial_omen",
            description_id: "effect.minecraft.trial_omen",
            category: MobEffectCategory::Neutral,
            color: 1_484_454,
            particle: ParticleFactory::Fixed("minecraft:trial_omen"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:event.mob_effect.trial_omen"),
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::RaidOmen,
            path: "raid_omen",
            identifier: "minecraft:raid_omen",
            description_id: "effect.minecraft.raid_omen",
            category: MobEffectCategory::Neutral,
            color: 14_565_464,
            particle: ParticleFactory::Fixed("minecraft:raid_omen"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:event.mob_effect.raid_omen"),
            instantaneous: false,
            cadence: TickCadence::RaidOmen,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::WindCharged,
            path: "wind_charged",
            identifier: "minecraft:wind_charged",
            description_id: "effect.minecraft.wind_charged",
            category: MobEffectCategory::Harmful,
            color: 12_438_015,
            particle: ParticleFactory::Fixed("minecraft:small_gust"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:entity.breeze.wind_burst"),
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Weaving,
            path: "weaving",
            identifier: "minecraft:weaving",
            description_id: "effect.minecraft.weaving",
            category: MobEffectCategory::Harmful,
            color: 7_891_290,
            particle: ParticleFactory::Fixed("minecraft:item_cobweb"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:block.cobweb.place"),
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Oozing,
            path: "oozing",
            identifier: "minecraft:oozing",
            description_id: "effect.minecraft.oozing",
            category: MobEffectCategory::Harmful,
            color: 10_092_451,
            particle: ParticleFactory::Fixed("minecraft:item_slime"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:entity.slime.squish"),
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::Infested,
            path: "infested",
            identifier: "minecraft:infested",
            description_id: "effect.minecraft.infested",
            category: MobEffectCategory::Harmful,
            color: 9_214_860,
            particle: ParticleFactory::Fixed("minecraft:infested"),
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: Some("minecraft:entity.silverfish.hurt"),
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
        MobEffectDef {
            id: MobEffectId::BreathOfTheNautilus,
            path: "breath_of_the_nautilus",
            identifier: "minecraft:breath_of_the_nautilus",
            description_id: "effect.minecraft.breath_of_the_nautilus",
            category: MobEffectCategory::Beneficial,
            color: 65_518,
            particle: ParticleFactory::DefaultEntityEffect,
            blend_in_duration_ticks: 0,
            blend_out_duration_ticks: 0,
            blend_out_advance_ticks: 0,
            sound_on_added: None,
            instantaneous: false,
            cadence: TickCadence::Never,
            attribute_modifiers: &[],
        },
    ];
}
