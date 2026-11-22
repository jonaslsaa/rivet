//! `net.minecraft.world.effect` and `net.minecraft.world.item.alchemy` — mob effects,
//! potions, and status-effect runtime (issue #450).
//!
//! Ported faithfully from Paper 26.2 (`0a993450f129c4942c2a9ed45ba047412b4667cf`):
//! - [`mob_effect`]: `MobEffectId`, `MobEffectDef`, `MobEffects` (all 40 built-in effects),
//!   attribute modifier templates, particle factories, blend durations, and cadence rules.
//! - [`instance`]: `MobEffectInstance`, `BlendState`, stacking/update rules, hidden-effect
//!   chains (including Paper's MC-259832 multi-level 0-duration promotion fix), `compareTo`
//!   ordering with the 32147 cutoff, and NBT persistence matching `MobEffectInstance.Details`.
//! - [`runtime`]: `EffectTarget` integration trait and `ActiveEffects` status-effect runtime
//!   mirroring `LivingEntity` (adding, replacing, ticking with re-entrancy deferral,
//!   instantaneous effects, curing/removal, ambient particle sampling, and attribute modifier
//!   lifecycle).
//! - [`util`]: `MobEffectUtil` duration formatting and gameplay queries (dig speed, water
//!   breathing, air-supply refill).
//! - [`potion`]: `PotionId`, `PotionDef`, `Potions` (all 46 built-in potions aligned 1:1 with
//!   `rivet_registry::generated::registries::POTION_BY_ID`), `PotionContents` (weighted ARGB
//!   color mixing, effect composition, duration scaling, NBT persistence), and `PotionBrewing`
//!   vanilla mix rules.

pub mod instance;
pub mod mob_effect;
pub mod potion;
pub mod runtime;
pub mod util;

#[cfg(test)]
mod tests;

pub use instance::{
    BlendState, INFINITE_DURATION, MAX_AMPLIFIER, MIN_AMPLIFIER, MobEffectInstance,
};
pub use mob_effect::{MobEffectDef, MobEffectId, MobEffects};
pub use potion::{BASE_POTION_COLOR, PotionBrewing, PotionContents, PotionDef, PotionId, Potions};
pub use runtime::{
    ActiveEffects, EffectRemovalReason, EffectTarget, EffectUpdateCause, RemovalKind, SoundTrigger,
};
pub use util::MobEffectUtil;

use rivet_protocol::game::clientbound_update_attributes_packet::{AttributeModifier, Operation};
use rivet_registry::Identifier;
use rivet_text::ChatFormatting;

/// `net.minecraft.world.effect.MobEffectCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobEffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

impl MobEffectCategory {
    /// `MobEffectCategory.getTooltipFormatting()`.
    pub const fn tooltip_formatting(self) -> ChatFormatting {
        match self {
            Self::Beneficial | Self::Neutral => ChatFormatting::Blue,
            Self::Harmful => ChatFormatting::Red,
        }
    }
}

/// Exact Java `net.minecraft.util.ARGB` helpers used by `MobEffect` and `PotionContents`.
pub mod argb {
    /// `ARGB.alpha(color)` — unsigned right shift `color >>> 24`.
    #[inline]
    pub const fn alpha(color: i32) -> i32 {
        ((color as u32) >> 24) as i32
    }

    /// `ARGB.red(color)` — `(color >> 16) & 0xFF`.
    #[inline]
    pub const fn red(color: i32) -> i32 {
        (color >> 16) & 0xFF
    }

    /// `ARGB.green(color)` — `(color >> 8) & 0xFF`.
    #[inline]
    pub const fn green(color: i32) -> i32 {
        (color >> 8) & 0xFF
    }

    /// `ARGB.blue(color)` — `color & 0xFF`.
    #[inline]
    pub const fn blue(color: i32) -> i32 {
        color & 0xFF
    }

    /// `ARGB.color(alpha, red, green, blue)`.
    #[inline]
    pub const fn color_argb(alpha: i32, red: i32, green: i32, blue: i32) -> i32 {
        ((alpha & 0xFF) << 24) | ((red & 0xFF) << 16) | ((green & 0xFF) << 8) | (blue & 0xFF)
    }

    /// `ARGB.color(red, green, blue)` (alpha = 255).
    #[inline]
    pub const fn color_rgb(red: i32, green: i32, blue: i32) -> i32 {
        color_argb(255, red, green, blue)
    }

    /// `ARGB.color(alpha, rgb)` — replaces the alpha channel of `rgb` with `alpha`.
    #[inline]
    pub const fn color_with_alpha(alpha: i32, rgb: i32) -> i32 {
        ((alpha & 0xFF) << 24) | (rgb & 0x00FF_FFFF)
    }

    /// `ARGB.opaque(color)` — sets alpha to 255.
    #[inline]
    pub const fn opaque(color: i32) -> i32 {
        color | (0xFF00_0000_u32 as i32)
    }

    /// `ARGB.transparent(color)` — clears alpha to 0.
    #[inline]
    pub const fn transparent(color: i32) -> i32 {
        color & 0x00FF_FFFF
    }
}

/// Particle option emitted for an active [`MobEffectInstance`].
///
/// Mirrors `MobEffect.createParticleOptions(MobEffectInstance)`:
/// - Default effects emit `ColorParticleOption.create(ParticleTypes.ENTITY_EFFECT, ARGB.color(alpha, color))`
///   where `alpha = 38` (`AMBIENT_ALPHA = floor(0.15 * 255)`) if ambient, else `255`.
/// - Specialized effects (`trial_omen`, `raid_omen`, `wind_charged`, `weaving`, `oozing`, `infested`)
///   emit their fixed particle type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffectParticle {
    /// `ParticleTypes.ENTITY_EFFECT` with ARGB color (alpha 38 for ambient, 255 for normal).
    EntityEffect { argb: i32 },
    /// Fixed particle type by Minecraft identifier (e.g. `minecraft:trial_omen`, `minecraft:infested`).
    Simple(&'static str),
}

/// `MobEffect.AMBIENT_ALPHA = Mth.floor(38.25F) = 38`.
pub const AMBIENT_ALPHA: i32 = 38;

/// Built-in attribute modifier template stored on a [`MobEffectDef`].
///
/// Mirrors `MobEffect.AttributeTemplate(Identifier id, double amount, AttributeModifier.Operation operation)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeTemplate {
    /// Attribute identifier (e.g. `minecraft:movement_speed`).
    pub attribute: &'static str,
    /// Modifier identifier (e.g. `minecraft:effect.speed`).
    pub modifier_id: &'static str,
    /// Base amount before scaling by `(amplifier + 1)`.
    pub amount: f64,
    /// Operation (`AddValue`, `AddMultipliedBase`, `AddMultipliedTotal`).
    pub operation: Operation,
}

impl AttributeTemplate {
    /// `AttributeTemplate.create(int amplifier)`: scales `amount * (amplifier + 1)`.
    pub fn create(&self, amplifier: i32) -> AttributeModifier {
        AttributeModifier {
            id: Identifier::parse(self.modifier_id),
            amount: self.amount * f64::from(amplifier.wrapping_add(1)),
            operation: self.operation,
        }
    }
}

/// Exact `AttributeInstance.calculateValue()` operation ordering from Paper 26.2:
///
/// 1. Start with `base`.
/// 2. Add every `AddValue` modifier to `base`.
/// 3. Set `result = base`.
/// 4. For every `AddMultipliedBase` modifier: `result += base * modifier.amount`.
/// 5. For every `AddMultipliedTotal` modifier: `result *= 1.0 + modifier.amount`.
/// 6. Clamp to `[min_value, max_value]` (matching `RangedAttribute.sanitizeValue`, preserving `NaN` -> `min_value`).
pub fn calculate_attribute_value(
    base_value: f64,
    modifiers: &[AttributeModifier],
    min_value: f64,
    max_value: f64,
) -> f64 {
    let mut base = base_value;
    for modifier in modifiers {
        if modifier.operation == Operation::AddValue {
            base += modifier.amount;
        }
    }

    let mut result = base;
    for modifier in modifiers {
        if modifier.operation == Operation::AddMultipliedBase {
            result += base * modifier.amount;
        }
    }

    for modifier in modifiers {
        if modifier.operation == Operation::AddMultipliedTotal {
            result *= 1.0 + modifier.amount;
        }
    }

    if result.is_nan() {
        min_value
    } else {
        result.clamp(min_value, max_value)
    }
}
