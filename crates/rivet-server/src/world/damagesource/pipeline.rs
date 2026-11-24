//! No Java counterpart: the `LivingEntity` hurt/death bookkeeping, kept beside `DamageSource` and `CombatTracker`.
//!
//! Authoritative Paper 26.2 damage, invulnerability, armor/absorption,
//! death attribution, death timing, and drop/experience hooks.
//!
//! Designed to be consumed by `LivingEntity` (unit E3) without re-owning `LivingEntity`.

use super::combat_rules::{
    Difficulty, apply_resistance, get_damage_after_absorb_with_modifier,
    get_damage_after_magic_absorb, java_max_f32, java_max_f64, java_min_f32, mth_clamp_f32,
    scale_damage_for_difficulty,
};
use super::combat_tracker::{CombatTracker, FallLocation};
use super::damage_source::{DamageEntityRef, DamageSource, DeathMessage};
use super::damage_type::DamageTypeTag;

/// Default invulnerability duration in ticks (`LivingEntity.INVULNERABLE_DURATION`).
pub const INVULNERABLE_DURATION: i32 = 20;
/// Default hurt animation duration in ticks (`LivingEntity.hurtDuration`).
pub const HURT_DURATION: i32 = 10;
/// Ticks after death before `EntityEvent.POOF` and `RemovalReason.KILLED` (`LivingEntity#tickDeath`).
pub const DEATH_REMOVAL_TICKS: i32 = 20;
/// Ticks a player kill credit remains active (`LivingEntity.PLAYER_HURT_EXPERIENCE_TIME`).
pub const PLAYER_HURT_EXPERIENCE_TIME: i32 = 100;

/// Inputs supplied by the victim `LivingEntity` and world for a single `hurt_server` call.
#[derive(Debug, Clone)]
pub struct HurtContext {
    pub victim_name: String,
    pub tick_count: i32,
    pub difficulty: Difficulty,
    pub is_player_victim: bool,
    pub is_invulnerable_to_source: bool,
    pub has_fire_resistance: bool,
    pub is_baby: bool,
    pub mob_drops_gamerule: bool,
    pub always_drops_experience: bool,
    pub freeze_hurts_extra: bool,
    pub has_helmet: bool,
    pub armor: f32,
    pub armor_toughness: f32,
    /// Optional armor-effectiveness multiplier (1.0 = normal, <1.0 = Breach enchantment).
    pub armor_effectiveness_multiplier: f32,
    /// 0-indexed Resistance potion amplifier (`None` if absent).
    pub resistance_amplifier: Option<i32>,
    /// Total Protection enchantment damage-protection factor (`EnchantmentHelper#getDamageProtection`).
    pub magic_armor: f32,
    /// Damage blocked by a shield (or similar blocking mechanic), already computed by the caller.
    pub shield_blocked_damage: f32,
    pub fall_location: Option<FallLocation>,
    pub fall_distance: f32,
}

impl Default for HurtContext {
    fn default() -> Self {
        Self {
            victim_name: String::new(),
            tick_count: 0,
            difficulty: Difficulty::Normal,
            is_player_victim: false,
            is_invulnerable_to_source: false,
            has_fire_resistance: false,
            is_baby: false,
            mob_drops_gamerule: true,
            always_drops_experience: false,
            freeze_hurts_extra: false,
            has_helmet: false,
            armor: 0.0,
            armor_toughness: 0.0,
            armor_effectiveness_multiplier: 1.0,
            resistance_amplifier: None,
            magic_armor: 0.0,
            shield_blocked_damage: 0.0,
            fall_location: None,
            fall_distance: 0.0,
        }
    }
}

/// Exact Paper/CraftBukkit damage modifier breakdown in `LivingEntity#handleEntityDamage` order.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageModifiers {
    pub base: f32,
    pub invulnerability_reduction: f32,
    pub freezing: f32,
    pub hard_hat: f32,
    pub blocking: f32,
    pub armor: f32,
    pub resistance: f32,
    pub magic: f32,
    pub absorption: f32,
    pub final_damage: f32,
}

/// Reason a `hurt_server` call was rejected before applying damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HurtRejection {
    DeadOrRemoved,
    InvulnerableToSource,
    FireResistance,
    PeacefulDifficulty,
    ZeroDamage,
    InvulnerabilityCooldown,
}

/// Drop & experience eligibility computed when a living entity dies (`LivingEntity#die` / `dropAllDeathLoot`).
#[derive(Debug, Clone, PartialEq)]
pub struct DeathOutcome {
    pub death_message: DeathMessage,
    pub kill_credit: Option<DamageEntityRef>,
    pub player_killed: bool,
    pub should_drop_loot: bool,
    pub should_drop_experience: bool,
}

/// Result of a single `DamagePipelineState::hurt_server` invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum HurtOutcome {
    Rejected(HurtRejection),
    Damaged {
        modifiers: DamageModifiers,
        took_full_damage: bool,
        entered_combat: bool,
        should_damage_helmet: bool,
        should_hurt_armor: bool,
        exhaustion: f32,
        death: Option<Box<DeathOutcome>>,
    },
}

/// Result of advancing `tick_death` by one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathTickOutcome {
    NotDead,
    Dying { death_time: i32 },
    RemoveKilled,
}

/// Pure damage, health, invulnerability, combat-tracker, and death state for a `LivingEntity`.
#[derive(Debug, Clone)]
pub struct DamagePipelineState {
    pub health: f32,
    pub max_health: f32,
    pub absorption_amount: f32,
    pub invulnerable_time: i32,
    pub invulnerable_duration: i32,
    pub hurt_time: i32,
    pub hurt_duration: i32,
    pub last_hurt: f32,
    pub death_time: i32,
    pub dead: bool,
    pub removed: bool,
    pub last_hurt_by_mob: Option<DamageEntityRef>,
    pub last_hurt_by_mob_timestamp: i32,
    pub last_hurt_by_player: Option<DamageEntityRef>,
    pub last_hurt_by_player_memory_time: i32,
    pub experience_consumed: bool,
    pub combat_tracker: CombatTracker,
}

impl DamagePipelineState {
    pub fn new(max_health: f32) -> Self {
        Self {
            health: max_health,
            max_health,
            absorption_amount: 0.0,
            invulnerable_time: 0,
            invulnerable_duration: INVULNERABLE_DURATION,
            hurt_time: 0,
            hurt_duration: HURT_DURATION,
            last_hurt: 0.0,
            death_time: 0,
            dead: false,
            removed: false,
            last_hurt_by_mob: None,
            last_hurt_by_mob_timestamp: 0,
            last_hurt_by_player: None,
            last_hurt_by_player_memory_time: 0,
            experience_consumed: false,
            combat_tracker: CombatTracker::new(),
        }
    }

    pub fn is_alive(&self) -> bool {
        !self.removed && self.health > 0.0
    }

    pub fn is_dead_or_dying(&self) -> bool {
        self.health <= 0.0
    }

    pub fn set_health(&mut self, health: f32) {
        self.health = mth_clamp_f32(health, 0.0, self.max_health);
    }

    /// Mirrors `LivingEntity#setAbsorptionAmount(float)` (Paper 26.2 line 4118):
    /// NaN falls back to `0.0`, otherwise clamped to `>= 0.0`.
    pub fn set_absorption_amount(&mut self, amount: f32) {
        self.absorption_amount = if amount.is_nan() {
            0.0
        } else {
            java_max_f32(amount, 0.0)
        };
    }

    /// Mirrors `LivingEntity#getKillCredit()`: prefers `last_hurt_by_player`, else `last_hurt_by_mob`.
    pub fn kill_credit(&self) -> Option<&DamageEntityRef> {
        self.last_hurt_by_player
            .as_ref()
            .or(self.last_hurt_by_mob.as_ref())
    }

    /// Mirrors `LivingEntity#shouldDropLoot(ServerLevel)`: `!isBaby() && mobDrops`.
    pub fn should_drop_loot(is_baby: bool, mob_drops_gamerule: bool) -> bool {
        !is_baby && mob_drops_gamerule
    }

    /// Mirrors `LivingEntity#getExpReward()` (Paper 26.2 lines 1926-1935):
    /// `!wasExperienceConsumed() && (isAlwaysExperienceDropper() || lastHurtByPlayerMemoryTime > 0 && shouldDropExperience() && MOB_DROPS)`.
    pub fn should_drop_experience(
        &self,
        is_baby: bool,
        mob_drops_gamerule: bool,
        always_drops_experience: bool,
    ) -> bool {
        if self.experience_consumed {
            return false;
        }
        always_drops_experience
            || (self.last_hurt_by_player_memory_time > 0 && !is_baby && mob_drops_gamerule)
    }

    /// Marks experience as dropped (`LivingEntity#dropExperience`), preventing double-drops.
    pub fn consume_experience(&mut self) -> bool {
        if self.experience_consumed {
            false
        } else {
            self.experience_consumed = true;
            true
        }
    }

    /// Advances invulnerability/hurt/player-memory timers by one tick.
    pub fn tick_timers(&mut self, tick_count: i32) -> bool {
        if self.invulnerable_time > 0 {
            self.invulnerable_time = self.invulnerable_time.wrapping_sub(1);
        }
        if self.hurt_time > 0 {
            self.hurt_time = self.hurt_time.wrapping_sub(1);
        }
        if self.last_hurt_by_player_memory_time > 0 {
            self.last_hurt_by_player_memory_time =
                self.last_hurt_by_player_memory_time.wrapping_sub(1);
            if self.last_hurt_by_player_memory_time == 0 {
                self.last_hurt_by_player = None;
            }
        }
        // Paper `LivingEntity#baseTick` (lines 520-527): expire `lastHurtByMob` after 100 ticks.
        if self.last_hurt_by_mob.is_some()
            && tick_count.wrapping_sub(self.last_hurt_by_mob_timestamp) > 100
        {
            self.last_hurt_by_mob = None;
        }
        self.combat_tracker
            .recheck_status(tick_count, self.is_alive())
    }

    /// Mirrors `LivingEntity#tickDeath()` (Paper 26.2 lines 603-609).
    pub fn tick_death(&mut self) -> DeathTickOutcome {
        if !self.dead && !self.is_dead_or_dying() {
            return DeathTickOutcome::NotDead;
        }
        self.death_time = self.death_time.wrapping_add(1);
        if self.death_time >= DEATH_REMOVAL_TICKS && !self.removed {
            self.removed = true;
            DeathTickOutcome::RemoveKilled
        } else {
            DeathTickOutcome::Dying {
                death_time: self.death_time,
            }
        }
    }

    /// Mirrors `LivingEntity#hurtServer` + `Player#hurtServer` difficulty scaling +
    /// `handleEntityDamage` modifier pipeline + `actuallyHurt` + `resolveMobResponsibleForDamage`
    /// + `resolvePlayerResponsibleForDamage` + `die`.
    pub fn hurt_server(
        &mut self,
        source: DamageSource,
        raw_amount: f32,
        ctx: &HurtContext,
    ) -> HurtOutcome {
        if self.removed || self.dead || self.is_dead_or_dying() {
            return HurtOutcome::Rejected(HurtRejection::DeadOrRemoved);
        }

        if ctx.is_invulnerable_to_source && !source.is_tag(DamageTypeTag::BypassesInvulnerability) {
            return HurtOutcome::Rejected(HurtRejection::InvulnerableToSource);
        }

        if source.is_tag(DamageTypeTag::IsFire) && ctx.has_fire_resistance {
            return HurtOutcome::Rejected(HurtRejection::FireResistance);
        }

        // Paper/CraftBukkit: clamp negative damage to 0.0; clamp NaN/Infinite to f32::MAX.
        let mut amount = if raw_amount < 0.0 {
            0.0
        } else if !raw_amount.is_finite() {
            f32::MAX
        } else {
            raw_amount
        };

        // Player difficulty scaling and zero-damage rejection (`Player#hurtServer` lines 733-748).
        if ctx.is_player_victim {
            match scale_damage_for_difficulty(amount, ctx.difficulty, &source) {
                Some(scaled) => amount = scaled,
                None => return HurtOutcome::Rejected(HurtRejection::PeacefulDifficulty),
            }
            if amount == 0.0 {
                return HurtOutcome::Rejected(HurtRejection::ZeroDamage);
            }
        }

        // Invulnerability cooldown check (`LivingEntity#hurtServer` lines 1463-1474).
        let in_cooldown = (self.invulnerable_time as f32)
            > (self.invulnerable_duration as f32) / 2.0
            && !source.is_tag(DamageTypeTag::BypassesCooldown);

        let mut took_full_damage = true;
        let invulnerability_reduction = if in_cooldown {
            if amount <= self.last_hurt {
                return HurtOutcome::Rejected(HurtRejection::InvulnerabilityCooldown);
            }
            took_full_damage = false;
            -self.last_hurt
        } else {
            0.0
        };

        // Modifier pipeline in exact Paper `LivingEntity#handleEntityDamage` order:
        // base -> invulnerability_reduction -> freezing -> hard_hat -> blocking
        // -> armor -> resistance -> magic -> absorption.
        let base = amount;
        let mut remaining = java_max_f32(base + invulnerability_reduction, 0.0);

        // Paper `LivingEntity#handleEntityDamage` (lines 2419-2469):
        // each modifier computes a `double` delta from `-(mod - after)` cast to `float`,
        // then accumulates via `damage += modifier` in `float`.
        let freezing = if source.is_tag(DamageTypeTag::IsFreezing) && ctx.freeze_hurts_extra {
            let mod_d = remaining as f64;
            let extra = -(mod_d - mod_d * 5.0_f64) as f32;
            remaining += extra;
            extra
        } else {
            -0.0
        };

        let should_damage_helmet = source.is_tag(DamageTypeTag::DamagesHelmet) && ctx.has_helmet;
        let hard_hat = if should_damage_helmet {
            let mod_d = remaining as f64;
            let reduction = -(mod_d - mod_d * 0.75_f64) as f32;
            remaining += reduction;
            reduction
        } else {
            -0.0
        };

        let blocking =
            if ctx.shield_blocked_damage > 0.0 && !source.is_tag(DamageTypeTag::BypassesShield) {
                let blocked = java_min_f32(ctx.shield_blocked_damage, remaining);
                let delta = -blocked;
                remaining += delta;
                delta
            } else {
                0.0
            };

        let should_hurt_armor = !source.is_tag(DamageTypeTag::BypassesArmor) && remaining > 0.0;
        let armor = if !source.is_tag(DamageTypeTag::BypassesArmor) {
            let after = get_damage_after_absorb_with_modifier(
                remaining,
                ctx.armor,
                ctx.armor_toughness,
                |fraction| fraction * ctx.armor_effectiveness_multiplier,
            );
            let delta = -(remaining as f64 - after as f64) as f32;
            remaining += delta;
            delta
        } else {
            0.0
        };

        let resistance = if !source.is_tag(DamageTypeTag::BypassesEffects)
            && !source.is_tag(DamageTypeTag::BypassesResistance)
        {
            if let Some(amp) = ctx.resistance_amplifier {
                let after = apply_resistance(remaining, amp);
                let delta = -(remaining as f64 - after as f64) as f32;
                remaining += delta;
                delta
            } else {
                -0.0
            }
        } else {
            -0.0
        };

        let magic = if !source.is_tag(DamageTypeTag::BypassesEffects)
            && !source.is_tag(DamageTypeTag::BypassesEnchantments)
            && ctx.magic_armor > 0.0
        {
            let after = get_damage_after_magic_absorb(remaining, ctx.magic_armor);
            let delta = -(remaining as f64 - after as f64) as f32;
            remaining += delta;
            delta
        } else {
            0.0
        };

        let mod_d = remaining as f64;
        let absorption = -(java_max_f64(
            mod_d - java_max_f64(mod_d - self.absorption_amount as f64, 0.0),
            0.0,
        )) as f32;
        let absorbed = -absorption;
        let final_damage_f64 = base as f64
            + invulnerability_reduction as f64
            + freezing as f64
            + hard_hat as f64
            + blocking as f64
            + armor as f64
            + resistance as f64
            + magic as f64
            + absorption as f64;
        let final_damage = java_max_f32(final_damage_f64 as f32, 0.0);

        let modifiers = DamageModifiers {
            base,
            invulnerability_reduction,
            freezing,
            hard_hat,
            blocking,
            armor,
            resistance,
            magic,
            absorption,
            final_damage,
        };

        // Apply damage (`actuallyHurt`).
        // Paper `LivingEntity#computeAmountFromEntityDamageEvent`: `lastHurt` records the
        // pre-armor/resistance/magic/absorption damage after blocking, freezing, and hard-hat.
        self.set_absorption_amount(self.absorption_amount - absorbed);
        let mut last_hurt_amount = 0.0_f32;
        last_hurt_amount += modifiers.base;
        last_hurt_amount += modifiers.blocking;
        last_hurt_amount += modifiers.freezing;
        last_hurt_amount += modifiers.hard_hat;
        self.last_hurt = last_hurt_amount;
        if took_full_damage {
            self.invulnerable_time = self.invulnerable_duration;
            self.hurt_duration = HURT_DURATION;
            self.hurt_time = self.hurt_duration;
        }

        // Resolve mob and player responsibility (`LivingEntity#resolveMobResponsibleForDamage`
        // and `resolvePlayerResponsibleForDamage`).
        if let Some(causing) = source.causing_entity() {
            if causing.is_living && !source.is_tag(DamageTypeTag::NoAnger) {
                self.last_hurt_by_mob = Some(causing.clone());
                self.last_hurt_by_mob_timestamp = ctx.tick_count;
            }
            if causing.is_player {
                self.last_hurt_by_player = Some(causing.clone());
                self.last_hurt_by_player_memory_time = PLAYER_HURT_EXPERIENCE_TIME;
            } else if let Some(owner) = causing.tame_owner_player.as_deref() {
                self.last_hurt_by_player = Some(owner.clone());
                self.last_hurt_by_player_memory_time = PLAYER_HURT_EXPERIENCE_TIME;
            }
        }

        // Paper `LivingEntity#actuallyHurt` (lines 2535-2552): when a player takes 0 final
        // damage (e.g. fully absorbed by shield/absorption/resistance), exhaustion, combat
        // tracker recording, and health mutation are skipped.
        let (exhaustion, entered_combat) = if final_damage > 0.0 || !ctx.is_player_victim {
            let exhaustion = if ctx.is_player_victim {
                source.food_exhaustion()
            } else {
                0.0
            };
            let entered_combat = self.combat_tracker.record_damage(
                source,
                final_damage,
                ctx.fall_location,
                ctx.fall_distance,
                ctx.tick_count,
                self.is_alive(),
            );
            let new_health = self.health - final_damage;
            self.set_health(new_health);
            (exhaustion, entered_combat)
        } else {
            (0.0, false)
        };

        let death = if self.is_dead_or_dying() && !self.dead {
            self.dead = true;
            let kill_credit = self.kill_credit().cloned();
            let kill_credit_name = kill_credit.as_ref().map(|e| e.display_name.as_str());
            let death_message = self
                .combat_tracker
                .death_message(&ctx.victim_name, kill_credit_name);
            let player_killed = self.last_hurt_by_player_memory_time > 0;
            let should_drop_loot = Self::should_drop_loot(ctx.is_baby, ctx.mob_drops_gamerule);
            let should_drop_experience = self.should_drop_experience(
                ctx.is_baby,
                ctx.mob_drops_gamerule,
                ctx.always_drops_experience,
            );
            Some(Box::new(DeathOutcome {
                death_message,
                kill_credit,
                player_killed,
                should_drop_loot,
                should_drop_experience,
            }))
        } else {
            None
        };

        HurtOutcome::Damaged {
            modifiers,
            took_full_damage,
            entered_combat,
            should_damage_helmet,
            should_hurt_armor,
            exhaustion,
            death,
        }
    }
}
