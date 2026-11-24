//! Paper 26.2 `CombatRules`, difficulty scaling, and resistance reduction.

use super::damage_source::DamageSource;

pub const MAX_ARMOR: f32 = 20.0;
pub const ARMOR_PROTECTION_DIVIDER: f32 = 25.0;
pub const BASE_ARMOR_TOUGHNESS: f32 = 2.0;
pub const MIN_ARMOR_RATIO: f32 = 0.2;

/// Exact Java `Math.min(float, float)`: propagates NaN, returns `-0.0` for mixed-sign zeroes,
/// and returns the first operand for same-sign zeroes (per `PORTING.md`).
#[inline]
pub fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() != b.is_sign_negative() {
            -0.0
        } else {
            a
        };
    }
    if a <= b { a } else { b }
}

/// Exact Java `Math.max(float, float)`: propagates NaN, returns `+0.0` for mixed-sign zeroes,
/// and returns the first operand for same-sign zeroes (per `PORTING.md`).
#[inline]
pub fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() != b.is_sign_negative() {
            0.0
        } else {
            a
        };
    }
    if a >= b { a } else { b }
}

/// Exact Java `Math.min(double, double)`: propagates NaN, returns `-0.0` for mixed-sign zeroes,
/// and returns the first operand for same-sign zeroes (per `PORTING.md`).
#[inline]
pub fn java_min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() != b.is_sign_negative() {
            -0.0
        } else {
            a
        };
    }
    if a <= b { a } else { b }
}

/// Exact Java `Math.max(double, double)`: propagates NaN, returns `+0.0` for mixed-sign zeroes,
/// and returns the first operand for same-sign zeroes (per `PORTING.md`).
#[inline]
pub fn java_max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() != b.is_sign_negative() {
            0.0
        } else {
            a
        };
    }
    if a >= b { a } else { b }
}

/// Java `Mth.clamp(float, float, float)`: `value < min ? min : Math.min(value, max)`.
#[inline]
pub fn mth_clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min_f32(value, max)
    }
}

/// World difficulty levels for `Player#hurtServer` difficulty scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// Mirrors `CombatRules#getDamageAfterAbsorb` without weapon armor-effectiveness enchantment.
#[inline]
pub fn get_damage_after_absorb(damage: f32, total_armor: f32, armor_toughness: f32) -> f32 {
    get_damage_after_absorb_with_modifier(damage, total_armor, armor_toughness, |fraction| fraction)
}

/// Mirrors `CombatRules#getDamageAfterAbsorb` with an optional weapon enchantment hook
/// (`EnchantmentHelper.modifyArmorEffectiveness`, e.g. Breach).
#[inline]
pub fn get_damage_after_absorb_with_modifier(
    damage: f32,
    total_armor: f32,
    armor_toughness: f32,
    modify_armor_effectiveness: impl FnOnce(f32) -> f32,
) -> f32 {
    let toughness = BASE_ARMOR_TOUGHNESS + armor_toughness / 4.0;
    let real_armor = mth_clamp_f32(
        total_armor - damage / toughness,
        total_armor * MIN_ARMOR_RATIO,
        MAX_ARMOR,
    );
    let armor_fraction = real_armor / ARMOR_PROTECTION_DIVIDER;
    let modified_armor_fraction =
        mth_clamp_f32(modify_armor_effectiveness(armor_fraction), 0.0, 1.0);
    let damage_multiplier = 1.0 - modified_armor_fraction;
    damage * damage_multiplier
}

/// Mirrors `CombatRules#getDamageAfterMagicAbsorb`.
#[inline]
pub fn get_damage_after_magic_absorb(damage: f32, total_magic_armor: f32) -> f32 {
    let real_armor = mth_clamp_f32(total_magic_armor, 0.0, MAX_ARMOR);
    damage * (1.0 - real_armor / ARMOR_PROTECTION_DIVIDER)
}

/// Mirrors `Player#hurtServer` difficulty scaling (Paper 26.2 lines 733-745).
///
/// Returns `None` when `Difficulty::Peaceful` cancels a scaling damage source (Paper/CraftBukkit
/// returns `false` rather than propagating 0 damage).
#[inline]
pub fn scale_damage_for_difficulty(
    damage: f32,
    difficulty: Difficulty,
    source: &DamageSource,
) -> Option<f32> {
    if !source.scales_with_difficulty() {
        return Some(damage);
    }
    match difficulty {
        Difficulty::Peaceful => None,
        Difficulty::Easy => Some(java_min_f32(damage / 2.0 + 1.0, damage)),
        Difficulty::Normal => Some(damage),
        Difficulty::Hard => Some(damage * 3.0 / 2.0),
    }
}

/// Mirrors `LivingEntity` Resistance potion effect reduction (`MobEffects.RESISTANCE`).
///
/// `amplifier` is 0-indexed (Resistance I = 0 -> 20% reduction, Resistance V = 4 -> 100%).
#[inline]
pub fn apply_resistance(damage: f32, amplifier: i32) -> f32 {
    let absorb_value = amplifier.wrapping_add(1).wrapping_mul(5);
    let absorb = 25_i32.wrapping_sub(absorb_value);
    let v = damage * (absorb as f32);
    java_max_f32(v / 25.0, 0.0)
}
