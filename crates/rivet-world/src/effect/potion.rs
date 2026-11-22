//! `net.minecraft.world.item.alchemy` — `Potion`, `Potions`, `PotionContents`, and `PotionBrewing`.
//!
//! Aligned 1:1 with Paper 26.2 (`Potions.java`, `Potion.java`, `PotionContents.java`,
//! `PotionBrewing.java`) and `rivet_registry::generated::registries::POTION_BY_ID`.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;

use crate::effect::{
    ActiveEffects, EffectTarget, EffectUpdateCause, MobEffectId, MobEffectInstance, argb,
};

/// `PotionContents.BASE_POTION_COLOR = -13083194` (`0xFF385DC6`).
pub const BASE_POTION_COLOR: i32 = -13_083_194;

/// `PotionBrewing.BREWING_TIME_SECONDS = 20` (400 ticks).
pub const BREWING_TIME_SECONDS: i32 = 20;

/// Built-in potion identifiers in exact Paper 26.2 / `POTION_BY_ID` order (0..46).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum PotionId {
    Water = 0,
    Mundane = 1,
    Thick = 2,
    Awkward = 3,
    NightVision = 4,
    LongNightVision = 5,
    Invisibility = 6,
    LongInvisibility = 7,
    Leaping = 8,
    LongLeaping = 9,
    StrongLeaping = 10,
    FireResistance = 11,
    LongFireResistance = 12,
    Swiftness = 13,
    LongSwiftness = 14,
    StrongSwiftness = 15,
    Slowness = 16,
    LongSlowness = 17,
    StrongSlowness = 18,
    TurtleMaster = 19,
    LongTurtleMaster = 20,
    StrongTurtleMaster = 21,
    WaterBreathing = 22,
    LongWaterBreathing = 23,
    Healing = 24,
    StrongHealing = 25,
    Harming = 26,
    StrongHarming = 27,
    Poison = 28,
    LongPoison = 29,
    StrongPoison = 30,
    Regeneration = 31,
    LongRegeneration = 32,
    StrongRegeneration = 33,
    Strength = 34,
    LongStrength = 35,
    StrongStrength = 36,
    Weakness = 37,
    LongWeakness = 38,
    Luck = 39,
    SlowFalling = 40,
    LongSlowFalling = 41,
    WindCharged = 42,
    Weaving = 43,
    Oozing = 44,
    Infested = 45,
}

impl PotionId {
    /// Total number of built-in potions in Paper 26.2 (matches `POTION_BY_ID.len()`).
    pub const COUNT: usize = 46;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Water,
        Self::Mundane,
        Self::Thick,
        Self::Awkward,
        Self::NightVision,
        Self::LongNightVision,
        Self::Invisibility,
        Self::LongInvisibility,
        Self::Leaping,
        Self::LongLeaping,
        Self::StrongLeaping,
        Self::FireResistance,
        Self::LongFireResistance,
        Self::Swiftness,
        Self::LongSwiftness,
        Self::StrongSwiftness,
        Self::Slowness,
        Self::LongSlowness,
        Self::StrongSlowness,
        Self::TurtleMaster,
        Self::LongTurtleMaster,
        Self::StrongTurtleMaster,
        Self::WaterBreathing,
        Self::LongWaterBreathing,
        Self::Healing,
        Self::StrongHealing,
        Self::Harming,
        Self::StrongHarming,
        Self::Poison,
        Self::LongPoison,
        Self::StrongPoison,
        Self::Regeneration,
        Self::LongRegeneration,
        Self::StrongRegeneration,
        Self::Strength,
        Self::LongStrength,
        Self::StrongStrength,
        Self::Weakness,
        Self::LongWeakness,
        Self::Luck,
        Self::SlowFalling,
        Self::LongSlowFalling,
        Self::WindCharged,
        Self::Weaving,
        Self::Oozing,
        Self::Infested,
    ];

    #[inline]
    pub const fn id(self) -> u16 {
        self as u16
    }

    pub const fn by_id(id: u16) -> Option<Self> {
        if (id as usize) < Self::COUNT {
            Some(Self::ALL[id as usize])
        } else {
            None
        }
    }

    pub const fn identifier(self) -> &'static str {
        self.def().identifier
    }

    pub const fn name(self) -> &'static str {
        self.def().name
    }

    pub fn by_name(name: &str) -> Option<Self> {
        let bare = name.strip_prefix("minecraft:").unwrap_or(name);
        Self::ALL
            .into_iter()
            .find(|p| p.def().identifier.strip_prefix("minecraft:") == Some(bare))
    }

    #[inline]
    pub const fn def(self) -> &'static PotionDef {
        &Potions::DEFS[self as usize]
    }
}

/// Static effect entry on a built-in [`PotionDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionEffectSpec {
    pub effect: MobEffectId,
    pub duration: i32,
    pub amplifier: i32,
}

impl PotionEffectSpec {
    pub fn to_instance(self) -> MobEffectInstance {
        MobEffectInstance::with_duration_and_amplifier(self.effect, self.duration, self.amplifier)
    }
}

/// Static definition of a `net.minecraft.world.item.alchemy.Potion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionDef {
    pub id: PotionId,
    pub identifier: &'static str,
    /// Translation suffix returned by `Potion.name()` (e.g. `"swiftness"` for `long_swiftness`).
    pub name: &'static str,
    pub effects: &'static [PotionEffectSpec],
}

impl PotionDef {
    /// `Potion.getEffects()` — constructs fresh `MobEffectInstance`s in exact order.
    pub fn effects(&self) -> Vec<MobEffectInstance> {
        self.effects.iter().map(|s| s.to_instance()).collect()
    }

    /// `Potion.hasInstantEffects()`.
    pub fn has_instant_effects(&self) -> bool {
        self.effects
            .iter()
            .any(|s| s.effect.def().is_instantaneous())
    }
}

/// Built-in `net.minecraft.world.item.alchemy.Potions` registry table.
pub struct Potions;

impl Potions {
    pub const DEFS: [PotionDef; PotionId::COUNT] = [
        PotionDef {
            id: PotionId::Water,
            identifier: "minecraft:water",
            name: "water",
            effects: &[],
        },
        PotionDef {
            id: PotionId::Mundane,
            identifier: "minecraft:mundane",
            name: "mundane",
            effects: &[],
        },
        PotionDef {
            id: PotionId::Thick,
            identifier: "minecraft:thick",
            name: "thick",
            effects: &[],
        },
        PotionDef {
            id: PotionId::Awkward,
            identifier: "minecraft:awkward",
            name: "awkward",
            effects: &[],
        },
        PotionDef {
            id: PotionId::NightVision,
            identifier: "minecraft:night_vision",
            name: "night_vision",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::NightVision,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongNightVision,
            identifier: "minecraft:long_night_vision",
            name: "night_vision",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::NightVision,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Invisibility,
            identifier: "minecraft:invisibility",
            name: "invisibility",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Invisibility,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongInvisibility,
            identifier: "minecraft:long_invisibility",
            name: "invisibility",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Invisibility,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Leaping,
            identifier: "minecraft:leaping",
            name: "leaping",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::JumpBoost,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongLeaping,
            identifier: "minecraft:long_leaping",
            name: "leaping",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::JumpBoost,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongLeaping,
            identifier: "minecraft:strong_leaping",
            name: "leaping",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::JumpBoost,
                duration: 1800,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::FireResistance,
            identifier: "minecraft:fire_resistance",
            name: "fire_resistance",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::FireResistance,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongFireResistance,
            identifier: "minecraft:long_fire_resistance",
            name: "fire_resistance",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::FireResistance,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Swiftness,
            identifier: "minecraft:swiftness",
            name: "swiftness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Speed,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongSwiftness,
            identifier: "minecraft:long_swiftness",
            name: "swiftness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Speed,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongSwiftness,
            identifier: "minecraft:strong_swiftness",
            name: "swiftness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Speed,
                duration: 1800,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Slowness,
            identifier: "minecraft:slowness",
            name: "slowness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Slowness,
                duration: 1800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongSlowness,
            identifier: "minecraft:long_slowness",
            name: "slowness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Slowness,
                duration: 4800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongSlowness,
            identifier: "minecraft:strong_slowness",
            name: "slowness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Slowness,
                duration: 400,
                amplifier: 3,
            }],
        },
        PotionDef {
            id: PotionId::TurtleMaster,
            identifier: "minecraft:turtle_master",
            name: "turtle_master",
            effects: &[
                PotionEffectSpec {
                    effect: MobEffectId::Slowness,
                    duration: 400,
                    amplifier: 3,
                },
                PotionEffectSpec {
                    effect: MobEffectId::Resistance,
                    duration: 400,
                    amplifier: 2,
                },
            ],
        },
        PotionDef {
            id: PotionId::LongTurtleMaster,
            identifier: "minecraft:long_turtle_master",
            name: "turtle_master",
            effects: &[
                PotionEffectSpec {
                    effect: MobEffectId::Slowness,
                    duration: 800,
                    amplifier: 3,
                },
                PotionEffectSpec {
                    effect: MobEffectId::Resistance,
                    duration: 800,
                    amplifier: 2,
                },
            ],
        },
        PotionDef {
            id: PotionId::StrongTurtleMaster,
            identifier: "minecraft:strong_turtle_master",
            name: "turtle_master",
            effects: &[
                PotionEffectSpec {
                    effect: MobEffectId::Slowness,
                    duration: 400,
                    amplifier: 5,
                },
                PotionEffectSpec {
                    effect: MobEffectId::Resistance,
                    duration: 400,
                    amplifier: 3,
                },
            ],
        },
        PotionDef {
            id: PotionId::WaterBreathing,
            identifier: "minecraft:water_breathing",
            name: "water_breathing",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::WaterBreathing,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongWaterBreathing,
            identifier: "minecraft:long_water_breathing",
            name: "water_breathing",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::WaterBreathing,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Healing,
            identifier: "minecraft:healing",
            name: "healing",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::InstantHealth,
                duration: 1,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongHealing,
            identifier: "minecraft:strong_healing",
            name: "healing",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::InstantHealth,
                duration: 1,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Harming,
            identifier: "minecraft:harming",
            name: "harming",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::InstantDamage,
                duration: 1,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongHarming,
            identifier: "minecraft:strong_harming",
            name: "harming",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::InstantDamage,
                duration: 1,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Poison,
            identifier: "minecraft:poison",
            name: "poison",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Poison,
                duration: 900,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongPoison,
            identifier: "minecraft:long_poison",
            name: "poison",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Poison,
                duration: 1800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongPoison,
            identifier: "minecraft:strong_poison",
            name: "poison",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Poison,
                duration: 432,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Regeneration,
            identifier: "minecraft:regeneration",
            name: "regeneration",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Regeneration,
                duration: 900,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongRegeneration,
            identifier: "minecraft:long_regeneration",
            name: "regeneration",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Regeneration,
                duration: 1800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongRegeneration,
            identifier: "minecraft:strong_regeneration",
            name: "regeneration",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Regeneration,
                duration: 450,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Strength,
            identifier: "minecraft:strength",
            name: "strength",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Strength,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongStrength,
            identifier: "minecraft:long_strength",
            name: "strength",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Strength,
                duration: 9600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::StrongStrength,
            identifier: "minecraft:strong_strength",
            name: "strength",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Strength,
                duration: 1800,
                amplifier: 1,
            }],
        },
        PotionDef {
            id: PotionId::Weakness,
            identifier: "minecraft:weakness",
            name: "weakness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Weakness,
                duration: 1800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongWeakness,
            identifier: "minecraft:long_weakness",
            name: "weakness",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Weakness,
                duration: 4800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Luck,
            identifier: "minecraft:luck",
            name: "luck",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Luck,
                duration: 6000,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::SlowFalling,
            identifier: "minecraft:slow_falling",
            name: "slow_falling",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::SlowFalling,
                duration: 1800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::LongSlowFalling,
            identifier: "minecraft:long_slow_falling",
            name: "slow_falling",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::SlowFalling,
                duration: 4800,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::WindCharged,
            identifier: "minecraft:wind_charged",
            name: "wind_charged",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::WindCharged,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Weaving,
            identifier: "minecraft:weaving",
            name: "weaving",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Weaving,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Oozing,
            identifier: "minecraft:oozing",
            name: "oozing",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Oozing,
                duration: 3600,
                amplifier: 0,
            }],
        },
        PotionDef {
            id: PotionId::Infested,
            identifier: "minecraft:infested",
            name: "infested",
            effects: &[PotionEffectSpec {
                effect: MobEffectId::Infested,
                duration: 3600,
                amplifier: 0,
            }],
        },
    ];
}

/// `net.minecraft.world.item.alchemy.PotionContents`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PotionContents {
    pub potion: Option<PotionId>,
    pub custom_color: Option<i32>,
    pub custom_effects: Vec<MobEffectInstance>,
    pub custom_name: Option<String>,
}

impl PotionContents {
    /// `PotionContents.EMPTY`.
    pub const EMPTY: Self = Self {
        potion: None,
        custom_color: None,
        custom_effects: Vec::new(),
        custom_name: None,
    };

    /// `new PotionContents(Holder<Potion>)`.
    pub fn from_potion(potion: PotionId) -> Self {
        Self {
            potion: Some(potion),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        }
    }

    /// `PotionContents.is(Holder<Potion>)` — true iff potion matches and `custom_effects` is empty.
    #[inline]
    pub fn is(&self, potion: PotionId) -> bool {
        self.potion == Some(potion) && self.custom_effects.is_empty()
    }

    /// `PotionContents.withPotion(Holder<Potion>)`.
    pub fn with_potion(&self, potion: PotionId) -> Self {
        Self {
            potion: Some(potion),
            custom_color: self.custom_color,
            custom_effects: self.custom_effects.clone(),
            custom_name: self.custom_name.clone(),
        }
    }

    /// `PotionContents.withEffectAdded(MobEffectInstance)`.
    pub fn with_effect_added(&self, effect: MobEffectInstance) -> Self {
        let mut custom_effects = self.custom_effects.clone();
        custom_effects.push(effect);
        Self {
            potion: self.potion,
            custom_color: self.custom_color,
            custom_effects,
            custom_name: self.custom_name.clone(),
        }
    }

    /// `PotionContents.getAllEffects()` — potion effects followed by custom effects.
    pub fn all_effects(&self) -> Vec<MobEffectInstance> {
        let mut out = match self.potion {
            Some(p) => p.def().effects(),
            None => Vec::new(),
        };
        out.extend(self.custom_effects.iter().cloned());
        out
    }

    /// `PotionContents.forEachEffect(Consumer<MobEffectInstance>, float durationScale)`.
    pub fn for_each_effect(&self, duration_scale: f32, mut f: impl FnMut(MobEffectInstance)) {
        if let Some(potion) = self.potion {
            for effect in potion.def().effects() {
                f(effect.with_scaled_duration(duration_scale));
            }
        }
        for effect in &self.custom_effects {
            f(effect.with_scaled_duration(duration_scale));
        }
    }

    /// `PotionContents.hasEffects()`.
    pub fn has_effects(&self) -> bool {
        if !self.custom_effects.is_empty() {
            return true;
        }
        self.potion.is_some_and(|p| !p.def().effects.is_empty())
    }

    /// `PotionContents.customEffects()` — returns copies matching Java `Lists.transform(..., MobEffectInstance::new)`.
    pub fn custom_effects_copies(&self) -> Vec<MobEffectInstance> {
        self.custom_effects
            .iter()
            .map(MobEffectInstance::copy_details)
            .collect()
    }

    /// `PotionContents.getColor()` — returns `getColorOr(BASE_POTION_COLOR)`.
    #[inline]
    pub fn color(&self) -> i32 {
        self.color_or(BASE_POTION_COLOR)
    }

    /// `PotionContents.getColorOr(int defaultColor)`.
    pub fn color_or(&self, default_color: i32) -> i32 {
        if let Some(custom) = self.custom_color {
            custom
        } else {
            Self::get_color_optional(&self.all_effects()).unwrap_or(default_color)
        }
    }

    /// `PotionContents.getColorOptional(Iterable<MobEffectInstance>)` — exact Paper 26.2
    /// weighted ARGB averaging by `(amplifier + 1)` over visible effects only.
    pub fn get_color_optional<'a>(
        effects: impl IntoIterator<Item = &'a MobEffectInstance>,
    ) -> Option<i32> {
        let mut red: i32 = 0;
        let mut green: i32 = 0;
        let mut blue: i32 = 0;
        let mut total_weight: i32 = 0;

        for effect in effects {
            if effect.is_visible() {
                let color = effect.effect_def().color();
                let weight = effect.amplifier().wrapping_add(1);
                red = red.wrapping_add(weight.wrapping_mul(argb::red(color)));
                green = green.wrapping_add(weight.wrapping_mul(argb::green(color)));
                blue = blue.wrapping_add(weight.wrapping_mul(argb::blue(color)));
                total_weight = total_weight.wrapping_add(weight);
            }
        }

        if total_weight == 0 {
            None
        } else {
            Some(argb::color_rgb(
                red / total_weight,
                green / total_weight,
                blue / total_weight,
            ))
        }
    }

    /// `PotionContents.getName(String prefix)` — translation key
    /// `<prefix><custom_name | potion.name() | "empty">`.
    pub fn translation_key(&self, prefix: &str) -> String {
        let suffix = self
            .custom_name
            .as_deref()
            .or_else(|| self.potion.map(PotionId::name))
            .unwrap_or("empty");
        format!("{prefix}{suffix}")
    }

    /// `PotionContents.applyToLivingEntity(LivingEntity, float durationScale)` — applies
    /// instantaneous effects immediately and adds non-instantaneous effects to `active`.
    pub fn apply_to_target(
        &self,
        active: &mut ActiveEffects,
        target: &mut dyn EffectTarget,
        duration_scale: f32,
        cause: EffectUpdateCause,
    ) {
        self.for_each_effect(duration_scale, |effect| {
            if effect.effect_def().is_instantaneous() {
                ActiveEffects::apply_instantaneous_effect(
                    effect.effect(),
                    effect.amplifier(),
                    1.0,
                    false,
                    target,
                );
            } else {
                let _ = active.add_effect(effect, target, cause);
            }
        });
    }

    /// Serialize to NBT matching `PotionContents.FULL_CODEC`:
    /// `potion`, `custom_color`, `custom_effects`, `custom_name`.
    pub fn save_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        if let Some(potion) = self.potion {
            tag.put_string("potion", potion.identifier());
        }
        if let Some(color) = self.custom_color {
            tag.put_int("custom_color", color);
        }
        if !self.custom_effects.is_empty() {
            let mut list = ListTag::new();
            for effect in &self.custom_effects {
                list.add(Tag::Compound(effect.save_nbt()));
            }
            tag.put("custom_effects".to_string(), Tag::List(list));
        }
        if let Some(ref name) = self.custom_name {
            tag.put_string("custom_name", name);
        }
        tag
    }

    /// Deserialize from NBT matching `PotionContents.FULL_CODEC`.
    pub fn load_nbt(tag: &CompoundTag) -> Self {
        let potion = tag.get_string("potion").and_then(|s| PotionId::by_name(s));
        let custom_color = tag.get_int("custom_color");
        let mut custom_effects = Vec::new();
        if let Some(list) = tag.get_list("custom_effects") {
            for i in 0..list.size() {
                if let Some(compound) = list.get_compound(i)
                    && let Some(inst) = MobEffectInstance::load_nbt(compound)
                {
                    custom_effects.push(inst);
                }
            }
        }
        let custom_name = tag.get_string("custom_name").map(ToOwned::to_owned);
        Self {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        }
    }
}

/// Container item transformation rule (`PotionBrewing.containerMixes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerMix {
    pub from: &'static str,
    pub ingredient: &'static str,
    pub to: &'static str,
}

/// Potion transformation rule (`PotionBrewing.potionMixes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionMix {
    pub from: PotionId,
    pub ingredient: &'static str,
    pub to: PotionId,
}

/// `net.minecraft.world.item.alchemy.PotionBrewing` — vanilla brewing recipe registry.
#[derive(Debug, Clone)]
pub struct PotionBrewing {
    containers: Vec<&'static str>,
    container_mixes: Vec<ContainerMix>,
    potion_mixes: Vec<PotionMix>,
}

impl PotionBrewing {
    /// `PotionBrewing.addVanillaMixes` — exact Paper 26.2 vanilla brewing graph.
    pub fn vanilla() -> Self {
        let mut b = Self {
            containers: vec![
                "minecraft:potion",
                "minecraft:splash_potion",
                "minecraft:lingering_potion",
            ],
            container_mixes: vec![
                ContainerMix {
                    from: "minecraft:potion",
                    ingredient: "minecraft:gunpowder",
                    to: "minecraft:splash_potion",
                },
                ContainerMix {
                    from: "minecraft:splash_potion",
                    ingredient: "minecraft:dragon_breath",
                    to: "minecraft:lingering_potion",
                },
            ],
            potion_mixes: Vec::new(),
        };

        b.add_mix(PotionId::Water, "minecraft:glowstone_dust", PotionId::Thick);
        b.add_mix(PotionId::Water, "minecraft:redstone", PotionId::Mundane);
        b.add_mix(PotionId::Water, "minecraft:nether_wart", PotionId::Awkward);

        b.add_start_mix("minecraft:breeze_rod", PotionId::WindCharged);
        b.add_start_mix("minecraft:slime_block", PotionId::Oozing);
        b.add_start_mix("minecraft:stone", PotionId::Infested);
        b.add_start_mix("minecraft:cobweb", PotionId::Weaving);

        b.add_mix(
            PotionId::Water,
            "minecraft:fermented_spider_eye",
            PotionId::Weakness,
        );
        b.add_mix(
            PotionId::Weakness,
            "minecraft:redstone",
            PotionId::LongWeakness,
        );

        b.add_mix(
            PotionId::Awkward,
            "minecraft:golden_carrot",
            PotionId::NightVision,
        );
        b.add_mix(
            PotionId::NightVision,
            "minecraft:redstone",
            PotionId::LongNightVision,
        );
        b.add_mix(
            PotionId::NightVision,
            "minecraft:fermented_spider_eye",
            PotionId::Invisibility,
        );
        b.add_mix(
            PotionId::LongNightVision,
            "minecraft:fermented_spider_eye",
            PotionId::LongInvisibility,
        );
        b.add_mix(
            PotionId::Invisibility,
            "minecraft:redstone",
            PotionId::LongInvisibility,
        );

        b.add_start_mix("minecraft:magma_cream", PotionId::FireResistance);
        b.add_mix(
            PotionId::FireResistance,
            "minecraft:redstone",
            PotionId::LongFireResistance,
        );

        b.add_start_mix("minecraft:rabbit_foot", PotionId::Leaping);
        b.add_mix(
            PotionId::Leaping,
            "minecraft:redstone",
            PotionId::LongLeaping,
        );
        b.add_mix(
            PotionId::Leaping,
            "minecraft:glowstone_dust",
            PotionId::StrongLeaping,
        );
        b.add_mix(
            PotionId::Leaping,
            "minecraft:fermented_spider_eye",
            PotionId::Slowness,
        );
        b.add_mix(
            PotionId::LongLeaping,
            "minecraft:fermented_spider_eye",
            PotionId::LongSlowness,
        );

        b.add_mix(
            PotionId::Slowness,
            "minecraft:redstone",
            PotionId::LongSlowness,
        );
        b.add_mix(
            PotionId::Slowness,
            "minecraft:glowstone_dust",
            PotionId::StrongSlowness,
        );

        b.add_mix(
            PotionId::Awkward,
            "minecraft:turtle_helmet",
            PotionId::TurtleMaster,
        );
        b.add_mix(
            PotionId::TurtleMaster,
            "minecraft:redstone",
            PotionId::LongTurtleMaster,
        );
        b.add_mix(
            PotionId::TurtleMaster,
            "minecraft:glowstone_dust",
            PotionId::StrongTurtleMaster,
        );

        b.add_mix(
            PotionId::Swiftness,
            "minecraft:fermented_spider_eye",
            PotionId::Slowness,
        );
        b.add_mix(
            PotionId::LongSwiftness,
            "minecraft:fermented_spider_eye",
            PotionId::LongSlowness,
        );
        b.add_start_mix("minecraft:sugar", PotionId::Swiftness);
        b.add_mix(
            PotionId::Swiftness,
            "minecraft:redstone",
            PotionId::LongSwiftness,
        );
        b.add_mix(
            PotionId::Swiftness,
            "minecraft:glowstone_dust",
            PotionId::StrongSwiftness,
        );

        b.add_mix(
            PotionId::Awkward,
            "minecraft:pufferfish",
            PotionId::WaterBreathing,
        );
        b.add_mix(
            PotionId::WaterBreathing,
            "minecraft:redstone",
            PotionId::LongWaterBreathing,
        );

        b.add_start_mix("minecraft:glistering_melon_slice", PotionId::Healing);
        b.add_mix(
            PotionId::Healing,
            "minecraft:glowstone_dust",
            PotionId::StrongHealing,
        );
        b.add_mix(
            PotionId::Healing,
            "minecraft:fermented_spider_eye",
            PotionId::Harming,
        );
        b.add_mix(
            PotionId::StrongHealing,
            "minecraft:fermented_spider_eye",
            PotionId::StrongHarming,
        );
        b.add_mix(
            PotionId::Harming,
            "minecraft:glowstone_dust",
            PotionId::StrongHarming,
        );
        b.add_mix(
            PotionId::Poison,
            "minecraft:fermented_spider_eye",
            PotionId::Harming,
        );
        b.add_mix(
            PotionId::LongPoison,
            "minecraft:fermented_spider_eye",
            PotionId::Harming,
        );
        b.add_mix(
            PotionId::StrongPoison,
            "minecraft:fermented_spider_eye",
            PotionId::StrongHarming,
        );

        b.add_start_mix("minecraft:spider_eye", PotionId::Poison);
        b.add_mix(PotionId::Poison, "minecraft:redstone", PotionId::LongPoison);
        b.add_mix(
            PotionId::Poison,
            "minecraft:glowstone_dust",
            PotionId::StrongPoison,
        );

        b.add_start_mix("minecraft:ghast_tear", PotionId::Regeneration);
        b.add_mix(
            PotionId::Regeneration,
            "minecraft:redstone",
            PotionId::LongRegeneration,
        );
        b.add_mix(
            PotionId::Regeneration,
            "minecraft:glowstone_dust",
            PotionId::StrongRegeneration,
        );

        b.add_start_mix("minecraft:blaze_powder", PotionId::Strength);
        b.add_mix(
            PotionId::Strength,
            "minecraft:redstone",
            PotionId::LongStrength,
        );
        b.add_mix(
            PotionId::Strength,
            "minecraft:glowstone_dust",
            PotionId::StrongStrength,
        );

        b.add_mix(
            PotionId::Awkward,
            "minecraft:phantom_membrane",
            PotionId::SlowFalling,
        );
        b.add_mix(
            PotionId::SlowFalling,
            "minecraft:redstone",
            PotionId::LongSlowFalling,
        );

        b
    }

    fn add_mix(&mut self, from: PotionId, ingredient: &'static str, to: PotionId) {
        self.potion_mixes.push(PotionMix {
            from,
            ingredient,
            to,
        });
    }

    /// `PotionBrewing.Builder.addStartMix`: registers both `WATER -> MUNDANE` and `AWKWARD -> to`.
    fn add_start_mix(&mut self, ingredient: &'static str, to: PotionId) {
        self.add_mix(PotionId::Water, ingredient, PotionId::Mundane);
        self.add_mix(PotionId::Awkward, ingredient, to);
    }

    /// `PotionBrewing.isInput(ItemStack)`.
    pub fn is_input(&self, container_item: &str) -> bool {
        self.containers.contains(&container_item)
    }

    /// `PotionBrewing.isIngredient(ItemStack)`.
    pub fn is_ingredient(&self, ingredient: &str) -> bool {
        self.is_container_ingredient(ingredient) || self.is_potion_ingredient(ingredient)
    }

    /// `PotionBrewing.isContainerIngredient(ItemStack)`.
    pub fn is_container_ingredient(&self, ingredient: &str) -> bool {
        self.container_mixes
            .iter()
            .any(|m| m.ingredient == ingredient)
    }

    /// `PotionBrewing.isPotionIngredient(ItemStack)`.
    pub fn is_potion_ingredient(&self, ingredient: &str) -> bool {
        self.potion_mixes.iter().any(|m| m.ingredient == ingredient)
    }

    /// `PotionBrewing.mix(ingredient, container_item, contents)` — returns `(new_container, new_contents)`
    /// if a container or potion mix applies, or `None` if no mix matches.
    pub fn mix(
        &self,
        ingredient: &str,
        container_item: &'static str,
        contents: &PotionContents,
    ) -> Option<(&'static str, PotionContents)> {
        if !self.is_input(container_item) {
            return None;
        }
        let potion = contents.potion?;

        for mix in &self.container_mixes {
            if mix.from == container_item && mix.ingredient == ingredient {
                return Some((mix.to, PotionContents::from_potion(potion)));
            }
        }

        for mix in &self.potion_mixes {
            if mix.from == potion && mix.ingredient == ingredient {
                return Some((container_item, PotionContents::from_potion(mix.to)));
            }
        }

        None
    }

    /// `PotionBrewing.hasMix(container_item, contents, ingredient)`.
    pub fn has_mix(
        &self,
        container_item: &'static str,
        contents: &PotionContents,
        ingredient: &str,
    ) -> bool {
        self.mix(ingredient, container_item, contents).is_some()
    }
}
