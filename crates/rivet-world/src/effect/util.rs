//! `net.minecraft.world.effect.MobEffectUtil` and `StringUtil.formatTickDuration`.

use rivet_util::mth;

use crate::effect::{ActiveEffects, MobEffectId, MobEffectInstance};

/// `net.minecraft.world.effect.MobEffectUtil`.
pub struct MobEffectUtil;

impl MobEffectUtil {
    /// `StringUtil.formatTickDuration(int ticks, float tickrate)`.
    pub fn format_tick_duration(ticks: i32, tickrate: f32) -> String {
        let seconds = mth::floor((ticks as f32) / tickrate);
        let minutes = seconds / 60;
        let rem_seconds = seconds % 60;
        let hours = minutes / 60;
        let rem_minutes = minutes % 60;
        if hours > 0 {
            format!("{hours:02}:{rem_minutes:02}:{rem_seconds:02}")
        } else {
            format!("{rem_minutes:02}:{rem_seconds:02}")
        }
    }

    /// `MobEffectUtil.formatDuration(MobEffectInstance, float scale, float tickrate)`.
    ///
    /// Returns `"effect.duration.infinite"` (the translation key) for infinite effects,
    /// otherwise the formatted duration string (`"MM:SS"` or `"HH:MM:SS"`).
    pub fn format_duration(instance: &MobEffectInstance, scale: f32, tickrate: f32) -> String {
        if instance.is_infinite_duration() {
            "effect.duration.infinite".to_string()
        } else {
            let duration = mth::floor((instance.duration() as f32) * scale);
            Self::format_tick_duration(duration, tickrate)
        }
    }

    /// `MobEffectUtil.hasDigSpeed(LivingEntity)`.
    #[inline]
    pub fn has_dig_speed(effects: &ActiveEffects) -> bool {
        effects.has_effect(MobEffectId::Haste) || effects.has_effect(MobEffectId::ConduitPower)
    }

    /// `MobEffectUtil.getDigSpeedAmplification(LivingEntity)`.
    #[inline]
    pub fn get_dig_speed_amplification(effects: &ActiveEffects) -> i32 {
        let a = effects
            .get_effect(MobEffectId::Haste)
            .map_or(0, MobEffectInstance::amplifier);
        let b = effects
            .get_effect(MobEffectId::ConduitPower)
            .map_or(0, MobEffectInstance::amplifier);
        a.max(b)
    }

    /// `MobEffectUtil.hasWaterBreathing(LivingEntity)`.
    #[inline]
    pub fn has_water_breathing(effects: &ActiveEffects) -> bool {
        effects.has_effect(MobEffectId::WaterBreathing)
            || effects.has_effect(MobEffectId::ConduitPower)
            || effects.has_effect(MobEffectId::BreathOfTheNautilus)
    }

    /// `MobEffectUtil.shouldEffectsRefillAirsupply(LivingEntity)`.
    #[inline]
    pub fn should_effects_refill_air_supply(effects: &ActiveEffects) -> bool {
        !effects.has_effect(MobEffectId::BreathOfTheNautilus)
            || effects.has_effect(MobEffectId::WaterBreathing)
            || effects.has_effect(MobEffectId::ConduitPower)
    }
}
