//! Port of `net.minecraft.world.entity.Mob` lifecycle foundation
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).
//!
//! Uses struct embedding (`pub living: LivingEntity`) and explicit parent calls
//! preserving Java virtual-dispatch order:
//! `Mob::mob_tick` -> `LivingEntity::living_tick` -> `Entity::entity_tick` ->
//! `Mob::mob_base_tick` -> `LivingEntity::living_base_tick` -> `Entity::entity_base_tick`,
//! followed by `LivingEntity::living_ai_step` -> `Mob#serverAiStep`, the
//! `Mob#tickHeadTurn` override (`BodyRotationControl#clientTick`), and finally
//! `Mob::update_control_flags` every 5 ticks.

use super::entity::EntityTypeMeta;
use super::equipment::HumanoidArm;
use super::level_ctx::{LevelCtx, LifecycleEvent};
use super::living::LivingEntity;
use crate::ai::control::BodyRotationControl;
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
/// Soft despawn distance squared (`MobCategory#getNoDespawnDistance()` = 32, squared).
pub const SOFT_DESPAWN_DISTANCE_SQR: f64 = 32.0 * 32.0;
/// Paper `Mob#getMaxHeadYRot()` default.
pub const DEFAULT_MAX_HEAD_Y_ROT: i32 = 75;

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
    /// Paper `Mob.aware` (default `true`).
    pub aware: bool,
    pub persistence_required: bool,
    /// Narrow seam for `Leashable#isLeashed()` until leash data is ported.
    pub leashed: bool,
    pub can_pick_up_loot: bool,
    pub xp_reward: i32,
    pub target: Option<i32>,
    pub control_flags: MobControlFlags,
    /// Paper `Mob.bodyRotationControl` (`Mob#createBodyControl()`).
    pub body_rotation_control: BodyRotationControl,
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
            aware: true,
            persistence_required: false,
            leashed: false,
            can_pick_up_loot: false,
            xp_reward: 0,
            target: None,
            control_flags: MobControlFlags::default(),
            body_rotation_control: BodyRotationControl::new(),
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

    /// Paper `Mob#getMaxHeadYRot()`.
    #[must_use]
    pub const fn max_head_y_rot(&self) -> i32 {
        DEFAULT_MAX_HEAD_Y_ROT
    }

    /// Paper `Mob#requiresCustomPersistence()`: `isPassenger() || isLeashed()`.
    #[must_use]
    pub const fn requires_custom_persistence(&self) -> bool {
        self.living.entity.is_passenger() || self.leashed
    }

    /// Paper `Mob#getControllingPassenger()`: the first passenger when
    /// `!isNoAi() && firstPassenger instanceof Mob && firstPassenger.canControlVehicle()`.
    #[must_use]
    pub fn controlling_passenger(&self, ctx: &LevelCtx) -> Option<i32> {
        let first_passenger = self.living.entity.first_passenger()?;
        (!self.is_no_ai() && ctx.is_entity_mob_controlling_vehicle(first_passenger))
            .then_some(first_passenger)
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
        let max_head_y_rot = self.max_head_y_rot();
        let body_rotation_control = &mut self.body_rotation_control;

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
                    liv.no_action_time = liv.no_action_time.wrapping_add(1);
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
            // Mob#tickHeadTurn -> bodyRotationControl.clientTick()
            |living, _y_body_rot_t, c| {
                let carrying_mob_passenger = living
                    .entity
                    .first_passenger()
                    .is_some_and(|pid| c.is_entity_mob(pid));
                body_rotation_control.client_tick(living, max_head_y_rot, carrying_mob_passenger);
            },
        );

        self.ambient_sound_time = ambient_sound_time;

        ctx.record_event(LifecycleEvent::MobTick {
            id: self.living.entity.id,
        });

        if self.living.entity.tick_count % 5 == 0 {
            self.update_control_flags(ctx);
        }
    }

    /// Paper `Mob#updateControlFlags()`.
    pub fn update_control_flags(&mut self, ctx: &mut LevelCtx) {
        let no_controller = self.controlling_passenger(ctx).is_none();
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

    /// Paper `Mob#checkDespawn()` with Paper's default despawn ranges
    /// (`DespawnRange.Shape.ELLIPSOID`, horizontal == vertical, so the vertical ratio is `1.0`).
    ///
    /// Returns `true` if this call discarded the mob.
    pub fn check_despawn(&mut self, ctx: &mut LevelCtx) -> bool {
        if ctx.difficulty == Difficulty::Peaceful
            && !self.living.entity.entity_type.allowed_in_peaceful
        {
            return self.discard_for_despawn(ctx);
        }

        if self.persistence_required || self.requires_custom_persistence() {
            self.living.no_action_time = 0;
            return false;
        }

        let Some(player_pos) = ctx.nearest_player_pos else {
            return false;
        };

        let pos = self.living.entity.pos;
        let dy = (player_pos.y - pos.y).abs();
        let dy_sqr = dy * dy;
        let dx_sqr = (player_pos.x - pos.x) * (player_pos.x - pos.x);
        let dz_sqr = (player_pos.z - pos.z) * (player_pos.z - pos.z);

        // DespawnRange#preComputed: Math.pow(MobCategory#getDespawnDistance(), 2)
        let hard_distance = f64::from(self.living.entity.entity_type.despawn_distance);
        let hard_distance_sqr = hard_distance * hard_distance;
        let mut discarded = false;
        // DespawnRange#shouldDespawn(ELLIPSOID): dxSqr + dzSqr + dySqr * 1.0 > limitSqr
        if dx_sqr + dz_sqr + dy_sqr > hard_distance_sqr {
            discarded |= self.discard_for_despawn(ctx);
        }

        if dx_sqr + dz_sqr + dy_sqr > SOFT_DESPAWN_DISTANCE_SQR {
            if self.living.no_action_time > 600
                && self.living.entity.random.next_int_bound(800) == 0
            {
                discarded |= self.discard_for_despawn(ctx);
            }
        } else {
            self.living.no_action_time = 0;
        }

        discarded
    }

    /// `discard(EntityRemoveEvent.Cause.DESPAWN)` from `Mob#checkDespawn()`.
    ///
    /// Returns `true` only when this call newly removed the mob.
    fn discard_for_despawn(&mut self, ctx: &mut LevelCtx) -> bool {
        let already_removed = self.living.entity.is_removed();
        let removed = self
            .living
            .entity
            .request_remove_taken(ctx, EntityRemovalReason::Discarded);
        if removed && !already_removed {
            ctx.record_event(LifecycleEvent::Despawn {
                id: self.living.entity.id,
            });
            return true;
        }
        false
    }

    /// Paper `Mob#inactiveTick()` (Paper EAR 2): `super.inactiveTick()`, then the goal and
    /// target selector inactive ticks when `aware`.
    pub fn mob_inactive_tick(&mut self, ctx: &mut LevelCtx) {
        self.living.living_inactive_tick(ctx);
        // RivetTodo(#422) run goalSelector/targetSelector inactiveTick() (gated on `aware`) once `Mob` owns its `GoalSelector`s.
    }

    /// Paper `rideTick()` for a `Mob` (inherited `LivingEntity#rideTick()`:
    /// `Entity#rideTick()` dispatching the `Mob#tick()` override, then `resetFallDistance()`).
    pub fn mob_ride_tick(&mut self, ctx: &mut LevelCtx) {
        self.living.entity.delta_movement = Vec3::new(0.0, 0.0, 0.0);
        self.mob_tick(ctx);
        if let Some(vehicle_id) = self.living.entity.vehicle {
            ctx.position_taken_rider(vehicle_id, &mut self.living.entity);
        }
        self.living.entity.fall_distance = 0.0;
    }
}
