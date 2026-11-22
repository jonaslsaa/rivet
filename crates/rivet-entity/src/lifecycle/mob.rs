//! Port of `net.minecraft.world.entity.Mob` lifecycle foundation
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).
//!
//! Uses struct embedding (`pub living: LivingEntity`) and explicit parent calls
//! preserving Java virtual-dispatch order:
//! `Mob::mob_tick` -> `LivingEntity::living_tick` -> `Entity::entity_tick` ->
//! `Mob::mob_base_tick` -> `LivingEntity::living_base_tick` -> `Entity::entity_base_tick`,
//! followed by `Mob::mob_ai_step` -> `LivingEntity::living_ai_step` -> `Mob::mob_server_ai_step`,
//! and finally `Mob::update_control_flags` every 5 ticks.

use super::entity::EntityTypeMeta;
use super::equipment::HumanoidArm;
use super::level_ctx::{LevelCtx, LifecycleEvent};
use super::living::LivingEntity;
use crate::damage::Difficulty;
use crate::storage::EntityRemovalReason;
use rivet_registry::core::Vec3;
use rivet_util::random::RandomSource;

/// Paper `Mob.MOB_FLAG_NO_AI`.
pub const MOB_FLAG_NO_AI: u8 = 1;
/// Paper `Mob.MOB_FLAG_LEFTHANDED`.
pub const MOB_FLAG_LEFTHANDED: u8 = 2;
/// Paper `Mob.MOB_FLAG_AGGRESSIVE`.
pub const MOB_FLAG_AGGRESSIVE: u8 = 4;

/// Default ambient sound interval (`Mob#getAmbientSoundInterval()` = 80).
pub const DEFAULT_AMBIENT_SOUND_INTERVAL: i32 = 80;
/// Hard despawn distance squared (`128 * 128`).
pub const HARD_DESPAWN_DISTANCE_SQR: f64 = 128.0 * 128.0;
/// Soft despawn distance squared (`32 * 32`).
pub const SOFT_DESPAWN_DISTANCE_SQR: f64 = 32.0 * 32.0;

/// Narrow seam for `GoalSelector` control flags (`Mob#updateControlFlags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobControlFlags {
    pub move_enabled: bool,
    pub jump_enabled: bool,
    pub look_enabled: bool,
}

impl Default for MobControlFlags {
    fn default() -> Self {
        Self {
            move_enabled: true,
            jump_enabled: true,
            look_enabled: true,
        }
    }
}

/// Port of `net.minecraft.world.entity.Mob` lifecycle state.
#[derive(Debug)]
pub struct Mob {
    pub living: LivingEntity,
    pub mob_flags: u8,
    pub ambient_sound_time: i32,
    pub ambient_sound_interval: i32,
    pub no_action_time: i32,
    /// Paper `Mob.aware` (default `true`).
    pub aware: bool,
    pub persistence_required: bool,
    pub requires_custom_persistence: bool,
    pub can_pick_up_loot: bool,
    pub xp_reward: i32,
    pub target: Option<i32>,
    pub control_flags: MobControlFlags,
}

impl Mob {
    /// Paper `Mob(EntityType<? extends Mob>, Level)` constructor.
    #[must_use]
    pub fn new(id: i32, entity_type: EntityTypeMeta, max_health: f32, random_seed: i64) -> Self {
        Self {
            living: LivingEntity::new(id, entity_type, max_health, random_seed),
            mob_flags: 0,
            ambient_sound_time: 0,
            ambient_sound_interval: DEFAULT_AMBIENT_SOUND_INTERVAL,
            no_action_time: 0,
            aware: true,
            persistence_required: false,
            requires_custom_persistence: false,
            can_pick_up_loot: false,
            xp_reward: 0,
            target: None,
            control_flags: MobControlFlags::default(),
        }
    }

    // -------------------------------------------------------------------------
    // Mob flags (Mob.DATA_MOB_FLAGS_ID)
    // -------------------------------------------------------------------------

    /// Paper `Mob#setNoAi(boolean)`.
    pub fn set_no_ai(&mut self, flag: bool) {
        if flag {
            self.mob_flags |= MOB_FLAG_NO_AI;
        } else {
            self.mob_flags &= !MOB_FLAG_NO_AI;
        }
    }

    /// Paper `Mob#isNoAi()`.
    #[must_use]
    pub const fn is_no_ai(&self) -> bool {
        (self.mob_flags & MOB_FLAG_NO_AI) != 0
    }

    /// Paper `Mob#setLeftHanded(boolean)`.
    pub fn set_left_handed(&mut self, flag: bool) {
        if flag {
            self.mob_flags |= MOB_FLAG_LEFTHANDED;
        } else {
            self.mob_flags &= !MOB_FLAG_LEFTHANDED;
        }
    }

    /// Paper `Mob#isLeftHanded()`.
    #[must_use]
    pub const fn is_left_handed(&self) -> bool {
        (self.mob_flags & MOB_FLAG_LEFTHANDED) != 0
    }

    /// Paper `Mob#setAggressive(boolean)`.
    pub fn set_aggressive(&mut self, flag: bool) {
        if flag {
            self.mob_flags |= MOB_FLAG_AGGRESSIVE;
        } else {
            self.mob_flags &= !MOB_FLAG_AGGRESSIVE;
        }
    }

    /// Paper `Mob#isAggressive()`.
    #[must_use]
    pub const fn is_aggressive(&self) -> bool {
        (self.mob_flags & MOB_FLAG_AGGRESSIVE) != 0
    }

    /// Paper `Mob#getMainArm()`.
    #[must_use]
    pub const fn main_arm(&self) -> HumanoidArm {
        if self.is_left_handed() {
            HumanoidArm::Left
        } else {
            HumanoidArm::Right
        }
    }

    /// Paper `Mob#isEffectiveAi()` -> `!this.isNoAi() && super.isEffectiveAi()`.
    #[must_use]
    pub const fn is_effective_ai(&self) -> bool {
        !self.is_no_ai() && self.living.is_effective_ai()
    }

    /// Paper `Mob#resetAmbientSoundTime()`.
    pub fn reset_ambient_sound_time(&mut self) {
        self.ambient_sound_time = -self.ambient_sound_interval;
    }

    // -------------------------------------------------------------------------
    // Core lifecycle: baseTick, tick, aiStep, serverAiStep, updateControlFlags, checkDespawn
    // -------------------------------------------------------------------------

    /// Paper `Mob#baseTick()`.
    ///
    /// Calls `LivingEntity::living_base_tick` first (`super.baseTick()`), then runs
    /// ambient-sound check (`isAlive() && random.nextInt(1000) < ambientSoundTime++`).
    pub fn mob_base_tick(&mut self, ctx: &mut LevelCtx) {
        self.living.living_base_tick(ctx);
        ctx.record_event(LifecycleEvent::MobBaseTick {
            id: self.living.entity.id,
        });
        if self.living.is_alive() {
            let roll = self.living.entity.random.next_int_bound(1000);
            let prev = self.ambient_sound_time;
            self.ambient_sound_time = self.ambient_sound_time.wrapping_add(1);
            if roll < prev {
                self.reset_ambient_sound_time();
                ctx.record_event(LifecycleEvent::MobAmbientSound {
                    id: self.living.entity.id,
                });
            }
        }
    }

    /// Paper `Mob#tick()`.
    ///
    /// Runs `LivingEntity::living_tick` (with virtual dispatch to `mob_base_tick`
    /// and `mob_ai_step`), then `if tickCount % 5 == 0 { updateControlFlags(); }`.
    pub fn mob_tick(&mut self, ctx: &mut LevelCtx) {
        // Mirror Java virtual dispatch cleanly:
        // Entity.tick -> Mob.baseTick -> LivingEntity.baseTick -> Entity.baseTick
        // LivingEntity.tick -> Mob.aiStep -> LivingEntity.aiStep -> Mob.serverAiStep
        let ambient_sound_interval = self.ambient_sound_interval;
        let mut ambient_sound_time = self.ambient_sound_time;
        let aware = self.aware;
        let is_no_ai = self.is_no_ai();
        let mut no_action_time = self.no_action_time;

        self.living.living_tick_with_hooks(
            ctx,
            |living, c| {
                living.living_base_tick(c);
                c.record_event(LifecycleEvent::MobBaseTick {
                    id: living.entity.id,
                });
                if living.is_alive() {
                    let roll = living.entity.random.next_int_bound(1000);
                    let prev = ambient_sound_time;
                    ambient_sound_time = ambient_sound_time.wrapping_add(1);
                    if roll < prev {
                        ambient_sound_time = -ambient_sound_interval;
                        c.record_event(LifecycleEvent::MobAmbientSound {
                            id: living.entity.id,
                        });
                    }
                }
            },
            |living, c| {
                // Temporarily align effective_ai with Mob#isEffectiveAi() (!isNoAi() && super)
                let saved_effective = living.effective_ai;
                living.effective_ai = !is_no_ai && saved_effective;
                living.living_ai_step(c, |liv, ctx_inner| {
                    no_action_time = no_action_time.wrapping_add(1);
                    if !aware {
                        return;
                    }
                    let id = liv.entity.id;
                    let tick_count = liv.entity.tick_count;
                    // Paper Mob#serverAiStep:
                    // int i = this.tickCount + this.getId();
                    // if (i % 2 != 0 && this.tickCount > 1) -> tickRunningGoals(false)
                    // else -> tick()
                    let sum = tick_count.wrapping_add(id);
                    let full_selector_tick = !(sum % 2 != 0 && tick_count > 1);
                    ctx_inner.record_event(LifecycleEvent::MobServerAiStep {
                        id,
                        full_selector_tick,
                    });
                });
                living.effective_ai = saved_effective;
            },
        );

        self.ambient_sound_time = ambient_sound_time;
        self.no_action_time = no_action_time;

        ctx.record_event(LifecycleEvent::MobTick {
            id: self.living.entity.id,
        });

        if self.living.entity.tick_count % 5 == 0 {
            self.update_control_flags(ctx);
        }
    }

    /// Paper `Mob#updateControlFlags()`.
    pub fn update_control_flags(&mut self, ctx: &mut LevelCtx) {
        let no_controller = !ctx.has_mob_controlling_passenger(self.living.entity.id);
        let not_in_boat = self
            .living
            .entity
            .vehicle
            .is_none_or(|vid| !ctx.is_entity_boat(vid));
        self.control_flags = MobControlFlags {
            move_enabled: no_controller,
            jump_enabled: no_controller && not_in_boat,
            look_enabled: no_controller,
        };
        ctx.record_event(LifecycleEvent::MobControlFlagsUpdated {
            id: self.living.entity.id,
            flags: self.control_flags,
        });
    }

    /// Paper `Mob#checkDespawn()`.
    ///
    /// Returns `true` if the mob was discarded by despawn rules.
    pub fn check_despawn(&mut self, ctx: &mut LevelCtx) -> bool {
        if ctx.difficulty == Difficulty::Peaceful
            && !self.living.entity.entity_type.allowed_in_peaceful
        {
            self.living.entity.removal_reason = Some(EntityRemovalReason::Discarded);
            ctx.record_event(LifecycleEvent::Despawn {
                id: self.living.entity.id,
            });
            return true;
        }

        if self.persistence_required || self.requires_custom_persistence {
            self.no_action_time = 0;
            return false;
        }

        let Some(player_pos) = ctx.nearest_player_pos else {
            return false;
        };

        let pos = self.living.entity.pos;
        let dx = pos.x - player_pos.x;
        let dy = pos.y - player_pos.y;
        let dz = pos.z - player_pos.z;
        let dist_sqr = dx * dx + dy * dy + dz * dz;

        if dist_sqr > HARD_DESPAWN_DISTANCE_SQR {
            self.living.entity.removal_reason = Some(EntityRemovalReason::Discarded);
            ctx.record_event(LifecycleEvent::Despawn {
                id: self.living.entity.id,
            });
            return true;
        }

        if self.no_action_time > 600
            && self.living.entity.random.next_int_bound(800) == 0
            && dist_sqr > SOFT_DESPAWN_DISTANCE_SQR
        {
            self.living.entity.removal_reason = Some(EntityRemovalReason::Discarded);
            ctx.record_event(LifecycleEvent::Despawn {
                id: self.living.entity.id,
            });
            return true;
        }

        if dist_sqr < SOFT_DESPAWN_DISTANCE_SQR {
            self.no_action_time = 0;
        }

        false
    }

    /// Paper `Mob#rideTick()`.
    pub fn mob_ride_tick(&mut self, ctx: &mut LevelCtx) {
        self.living.entity.delta_movement = Vec3::new(0.0, 0.0, 0.0);
        self.mob_tick(ctx);
        if let Some(vehicle_id) = self.living.entity.vehicle {
            ctx.position_taken_rider(vehicle_id, &mut self.living.entity);
        }
    }
}
