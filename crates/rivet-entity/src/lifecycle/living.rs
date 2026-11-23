//! Port of `net.minecraft.world.entity.LivingEntity` lifecycle foundation
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).
//!
//! Uses struct embedding (`pub entity: Entity`) and explicit parent calls
//! (`self.entity.entity_base_tick(ctx)`, `self.entity.entity_tick(ctx)`) per `PORTING.md`.

use super::dimensions::Pose;
use super::entity::{Entity, EntityTypeMeta};
use super::equipment::{EntityEquipment, EquipmentItem, EquipmentSlot, EquipmentSlotChange};
use super::level_ctx::{LevelCtx, LifecycleEvent};
use crate::damage::DEATH_REMOVAL_TICKS;
use crate::storage::EntityRemovalReason;
use rivet_registry::core::{BlockPos, Vec3};
use rivet_util::mth;

/// Paper `LivingEntity.LIVING_ENTITY_FLAG_IS_USING`.
pub const LIVING_ENTITY_FLAG_IS_USING: u8 = 1;
/// Paper `LivingEntity.LIVING_ENTITY_FLAG_OFF_HAND`.
pub const LIVING_ENTITY_FLAG_OFF_HAND: u8 = 2;
/// Paper `LivingEntity.LIVING_ENTITY_FLAG_SPIN_ATTACK`.
pub const LIVING_ENTITY_FLAG_SPIN_ATTACK: u8 = 4;

/// Paper `LivingEntity#getMaxHeadRotationRelativeToBody()` default.
pub const MAX_HEAD_ROTATION_RELATIVE_TO_BODY: f32 = 50.0;

/// Paper `LivingEntity.PLAYER_HURT_EXPERIENCE_TIME` (100 ticks).
pub const LAST_HURT_BY_MOB_EXPIRY_TICKS: i32 = 100;

/// Port of `net.minecraft.world.entity.LivingEntity` lifecycle state.
#[derive(Debug)]
pub struct LivingEntity {
    pub entity: Entity,
    pub living_flags: u8,
    pub health: f32,
    pub max_health: f32,
    pub hurt_time: i32,
    pub hurt_duration: i32,
    pub death_time: i32,
    /// Paper `LivingEntity.noActionTime`.
    pub no_action_time: i32,
    pub attack_anim: f32,
    pub o_attack_anim: f32,
    pub y_body_rot: f32,
    pub y_body_rot_o: f32,
    pub y_head_rot: f32,
    pub y_head_rot_o: f32,
    pub no_jump_delay: i32,
    pub fall_fly_ticks: i32,
    pub arrow_count: i32,
    pub remove_arrow_time: i32,
    pub stinger_count: i32,
    pub remove_stinger_time: i32,
    pub last_hurt_by_player_memory_time: i32,
    pub last_hurt_by_player: Option<i32>,
    pub last_hurt_by_mob: Option<i32>,
    pub last_hurt_by_mob_timestamp: i32,
    pub last_hurt_mob: Option<i32>,
    pub sleeping_pos: Option<BlockPos>,
    pub bed_exists: bool,
    pub last_pos: Option<BlockPos>,
    pub equipment: EntityEquipment,
    pub last_equipment_items: EntityEquipment,
    pub jumping: bool,
    pub xxa: f32,
    pub zza: f32,
    pub can_breathe_underwater: bool,
    pub has_water_breathing: bool,
    pub should_effects_refill_air: bool,
    pub effective_ai: bool,
    pub current_impulse_context_reset_grace_time: i32,
}

impl LivingEntity {
    /// Paper `LivingEntity(EntityType<? extends LivingEntity>, Level)` constructor.
    #[must_use]
    pub fn new(id: i32, entity_type: EntityTypeMeta, max_health: f32, random_seed: i64) -> Self {
        let mut entity = Entity::new(id, entity_type, random_seed);
        entity.is_living = true;
        entity.reapply_position();
        Self {
            entity,
            living_flags: 0,
            health: max_health,
            max_health,
            hurt_time: 0,
            hurt_duration: 0,
            death_time: 0,
            no_action_time: 0,
            attack_anim: 0.0,
            o_attack_anim: 0.0,
            y_body_rot: 0.0,
            y_body_rot_o: 0.0,
            y_head_rot: 0.0,
            y_head_rot_o: 0.0,
            no_jump_delay: 0,
            fall_fly_ticks: 0,
            arrow_count: 0,
            remove_arrow_time: 0,
            stinger_count: 0,
            remove_stinger_time: 0,
            last_hurt_by_player_memory_time: 0,
            last_hurt_by_player: None,
            last_hurt_by_mob: None,
            last_hurt_by_mob_timestamp: 0,
            last_hurt_mob: None,
            sleeping_pos: None,
            bed_exists: true,
            last_pos: None,
            equipment: EntityEquipment::new(),
            last_equipment_items: EntityEquipment::new(),
            jumping: false,
            xxa: 0.0,
            zza: 0.0,
            can_breathe_underwater: false,
            has_water_breathing: false,
            should_effects_refill_air: false,
            effective_ai: true,
            current_impulse_context_reset_grace_time: 0,
        }
    }

    // -------------------------------------------------------------------------
    // Living flags
    // -------------------------------------------------------------------------

    #[must_use]
    pub const fn is_using_item(&self) -> bool {
        (self.living_flags & LIVING_ENTITY_FLAG_IS_USING) != 0
    }

    pub fn set_living_entity_flag(&mut self, mask: u8, value: bool) {
        if value {
            self.living_flags |= mask;
        } else {
            self.living_flags &= !mask;
        }
    }

    // -------------------------------------------------------------------------
    // Alive / death / sleeping / immobility
    // -------------------------------------------------------------------------

    /// Paper `LivingEntity#isDeadOrDying()`.
    #[must_use]
    pub fn is_dead_or_dying(&self) -> bool {
        self.health <= 0.0_f32
    }

    /// Paper `LivingEntity#isAlive()`.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.entity.is_removed() && self.health > 0.0_f32
    }

    /// Paper `LivingEntity#isSleeping()`.
    #[must_use]
    pub const fn is_sleeping(&self) -> bool {
        self.sleeping_pos.is_some()
    }

    /// Paper `LivingEntity#stopSleeping()`.
    pub fn stop_sleeping(&mut self) {
        self.sleeping_pos = None;
        self.entity.set_pose(Pose::Standing);
        self.entity.refresh_dimensions();
    }

    /// Paper `LivingEntity#isImmobile()`.
    #[must_use]
    pub fn is_immobile(&self) -> bool {
        self.is_dead_or_dying()
    }

    /// Paper `LivingEntity#isEffectiveAi()`.
    #[must_use]
    pub const fn is_effective_ai(&self) -> bool {
        self.effective_ai
    }

    /// Paper `LivingEntity#shouldTakeDrowningDamage()`.
    #[must_use]
    pub const fn should_take_drowning_damage(&self) -> bool {
        self.entity.air_supply <= -20
    }

    /// Paper `LivingEntity#decreaseAirSupply(int)` (base without respiration = `current - 1`).
    #[must_use]
    pub const fn decrease_air_supply(&self, current: i32) -> i32 {
        current.wrapping_sub(1)
    }

    /// Paper `LivingEntity#increaseAirSupply(int)` = `Math.min(current + 4, maxAirSupply)`.
    #[must_use]
    pub fn increase_air_supply(&self, current: i32) -> i32 {
        current.wrapping_add(4).min(self.entity.max_air_supply)
    }

    /// Paper `LivingEntity#setLastHurtByMob(LivingEntity)`.
    pub fn set_last_hurt_by_mob(&mut self, attacker_id: Option<i32>) {
        self.last_hurt_by_mob = attacker_id;
        self.last_hurt_by_mob_timestamp = self.entity.tick_count;
    }

    // -------------------------------------------------------------------------
    // Equipment hooks (Paper LivingEntity lines 720, 2902, 3562-3665)
    // -------------------------------------------------------------------------

    /// Paper `LivingEntity#getItemBySlot(EquipmentSlot)`.
    #[must_use]
    pub fn get_item_by_slot(&self, slot: EquipmentSlot) -> EquipmentItem {
        self.equipment.get(slot)
    }

    /// Paper `LivingEntity#hasItemInSlot(EquipmentSlot)`.
    #[must_use]
    pub fn has_item_in_slot(&self, slot: EquipmentSlot) -> bool {
        !self.get_item_by_slot(slot).is_empty()
    }

    /// Paper `LivingEntity#setItemSlot(EquipmentSlot, ItemStack, boolean silent)`.
    ///
    /// Calls `on_equip_item` with the old and new stacks in exact Paper order.
    pub fn set_item_slot(
        &mut self,
        slot: EquipmentSlot,
        item: EquipmentItem,
        silent: bool,
        ctx: &mut LevelCtx,
    ) {
        let old_item = self.equipment.set(slot, item.clone());
        self.on_equip_item(slot, &old_item, &item, silent, ctx);
    }

    /// Paper `LivingEntity#onEquipItem(EquipmentSlot, ItemStack oldItem, ItemStack newItem, boolean silent)`.
    ///
    /// Compares with `ItemStack.isSameItemSameComponents` (count-insensitive).
    pub fn on_equip_item(
        &mut self,
        slot: EquipmentSlot,
        old_item: &EquipmentItem,
        new_item: &EquipmentItem,
        silent: bool,
        ctx: &mut LevelCtx,
    ) {
        if !old_item.is_same_item_same_components(new_item) && !self.entity.first_tick {
            ctx.record_event(LifecycleEvent::EquipmentEquipped {
                id: self.entity.id,
                slot,
                silent,
            });
        }
    }

    /// Paper `LivingEntity#detectEquipmentUpdates()`.
    ///
    /// Exact Paper ordering:
    /// 1. `collect_equipment_changes` (iterating `EquipmentSlot.VALUES`)
    /// 2. `handle_hand_swap` (suppresses MAINHAND/OFFHAND updates and emits `HandSwap` if swapped)
    /// 3. `handle_equipment_changes` (records remaining slot deltas and updates `last_equipment_items`)
    pub fn detect_equipment_updates(&mut self, ctx: &mut LevelCtx) {
        let mut changed = self.collect_equipment_changes();
        if changed.is_empty() {
            return;
        }
        self.handle_hand_swap(&mut changed, ctx);
        if !changed.is_empty() {
            self.handle_equipment_changes(changed, ctx);
        }
    }

    /// Paper `LivingEntity#collectEquipmentChanges(Map<EquipmentSlot, ItemStack>)`.
    fn collect_equipment_changes(&self) -> Vec<EquipmentSlotChange> {
        let mut changed = Vec::new();
        for slot in EquipmentSlot::VALUES {
            let previous = self.last_equipment_items.get(slot);
            let current = self.get_item_by_slot(slot);
            if !current.matches(&previous) {
                changed.push(EquipmentSlotChange {
                    slot,
                    old_item: previous,
                    new_item: current,
                });
            }
        }
        changed
    }

    /// Paper `LivingEntity#handleHandSwap(Map<EquipmentSlot, ItemStack>)`.
    fn handle_hand_swap(&mut self, changed: &mut Vec<EquipmentSlotChange>, ctx: &mut LevelCtx) {
        let main_idx = changed
            .iter()
            .position(|c| c.slot == EquipmentSlot::MainHand);
        let off_idx = changed
            .iter()
            .position(|c| c.slot == EquipmentSlot::OffHand);

        if let (Some(mi), Some(oi)) = (main_idx, off_idx) {
            let cur_main = changed[mi].new_item.clone();
            let cur_off = changed[oi].new_item.clone();
            let prev_off = self.last_equipment_items.get(EquipmentSlot::OffHand);
            let prev_main = self.last_equipment_items.get(EquipmentSlot::MainHand);
            if cur_main.matches(&prev_off) && cur_off.matches(&prev_main) {
                ctx.record_event(LifecycleEvent::HandSwap { id: self.entity.id });
                changed.retain(|c| {
                    c.slot != EquipmentSlot::MainHand && c.slot != EquipmentSlot::OffHand
                });
                self.last_equipment_items
                    .set(EquipmentSlot::MainHand, cur_main);
                self.last_equipment_items
                    .set(EquipmentSlot::OffHand, cur_off);
            }
        }
    }

    /// Paper `LivingEntity#handleEquipmentChanges(Map<EquipmentSlot, ItemStack>)`.
    fn handle_equipment_changes(&mut self, changed: Vec<EquipmentSlotChange>, ctx: &mut LevelCtx) {
        for change in &changed {
            self.last_equipment_items
                .set(change.slot, change.new_item.clone());
        }
        ctx.record_event(LifecycleEvent::EquipmentChanged {
            id: self.entity.id,
            changes: changed,
        });
    }

    // -------------------------------------------------------------------------
    // Core lifecycle: baseTick, tick, aiStep, tickDeath, rideTick
    // -------------------------------------------------------------------------

    /// Paper `LivingEntity#baseTick()`.
    ///
    /// Exact parent-call order:
    /// 1. `o_attack_anim = attack_anim`, first-tick sleeping pos snap
    /// 2. `self.entity.entity_base_tick(ctx)` (`super.baseTick()`)
    /// 3. Alive checks: suffocation (`in_wall`), water/drowning/underwater dismount,
    ///    `on_changed_block`
    /// 4. `hurt_time--`, `invulnerable_time--` (non-player), `tick_death` if dead/dying
    /// 5. `last_hurt_by_player_memory_time--`, `last_hurt_mob` / `last_hurt_by_mob` expiry
    /// 6. Rotation `O` snapshots (`y_head_rot_o`, `y_body_rot_o`, `y_rot_o`, `x_rot_o`)
    pub fn living_base_tick(&mut self, ctx: &mut LevelCtx) {
        self.o_attack_anim = self.attack_anim;
        if self.entity.first_tick
            && let Some(bed_pos) = self.sleeping_pos
        {
            self.entity.set_pos(
                f64::from(bed_pos.get_x()) + 0.5,
                f64::from(bed_pos.get_y()) + 0.6875,
                f64::from(bed_pos.get_z()) + 0.5,
            );
        }

        // super.baseTick() with the LivingEntity#onBelowWorld override
        self.entity
            .entity_base_tick_with_hook(ctx, Self::on_below_world);

        ctx.record_event(LifecycleEvent::LivingBaseTick { id: self.entity.id });

        if self.is_alive() {
            if self.entity.in_wall {
                ctx.record_event(LifecycleEvent::Hurt {
                    id: self.entity.id,
                    amount: 1.0,
                    source: "in_wall",
                });
            }

            if self.entity.eye_in_water && !self.entity.in_bubble_column {
                let can_drown = !self.can_breathe_underwater
                    && !self.has_water_breathing
                    && !(self.entity.entity_type.is_player && self.entity.invulnerable);
                if can_drown {
                    self.entity.air_supply = self.decrease_air_supply(self.entity.air_supply);
                    if self.should_take_drowning_damage() {
                        self.entity.air_supply = 0;
                        ctx.record_event(LifecycleEvent::DrownParticles { id: self.entity.id });
                        ctx.record_event(LifecycleEvent::Hurt {
                            id: self.entity.id,
                            amount: 2.0,
                            source: "drown",
                        });
                    }
                } else if self.entity.air_supply < self.entity.max_air_supply
                    && self.should_effects_refill_air
                {
                    self.entity.air_supply = self.increase_air_supply(self.entity.air_supply);
                }

                if let Some(vehicle_id) = self.entity.vehicle
                    && ctx.vehicle_dismounts_underwater(vehicle_id)
                {
                    ctx.stop_riding_taken(&mut self.entity, false);
                }
            } else if self.entity.air_supply < self.entity.max_air_supply {
                self.entity.air_supply = self.increase_air_supply(self.entity.air_supply);
            }

            let pos = self.entity.block_pos;
            if self.last_pos != Some(pos) {
                self.last_pos = Some(pos);
                ctx.record_event(LifecycleEvent::ChangedBlock {
                    id: self.entity.id,
                    pos,
                });
            }
        }

        if self.hurt_time > 0 {
            self.hurt_time = self.hurt_time.wrapping_sub(1);
        }

        if self.entity.invulnerable_time > 0 && !self.entity.entity_type.is_player {
            self.entity.invulnerable_time = self.entity.invulnerable_time.wrapping_sub(1);
        }

        if self.is_dead_or_dying() {
            self.tick_death(ctx);
        }

        if self.last_hurt_by_player_memory_time > 0 {
            self.last_hurt_by_player_memory_time =
                self.last_hurt_by_player_memory_time.wrapping_sub(1);
        } else {
            self.last_hurt_by_player = None;
        }

        if let Some(last_mob) = self.last_hurt_mob {
            let alive = if last_mob == self.entity.id {
                self.is_alive()
            } else {
                ctx.is_entity_alive(last_mob)
            };
            if !alive {
                self.last_hurt_mob = None;
            }
        }

        if let Some(hurt_by) = self.last_hurt_by_mob {
            let alive = if hurt_by == self.entity.id {
                self.is_alive()
            } else {
                ctx.is_entity_alive(hurt_by)
            };
            if !alive
                || self
                    .entity
                    .tick_count
                    .wrapping_sub(self.last_hurt_by_mob_timestamp)
                    > LAST_HURT_BY_MOB_EXPIRY_TICKS
            {
                self.set_last_hurt_by_mob(None);
            }
        }

        self.y_head_rot_o = self.y_head_rot;
        self.y_body_rot_o = self.y_body_rot;
        self.entity.y_rot_o = self.entity.y_rot;
        self.entity.x_rot_o = self.entity.x_rot;
    }

    /// Paper `LivingEntity#onBelowWorld()`:
    /// `hurt(damageSources().fellOutOfWorld(), level().getWorld().getVoidDamageAmount())`.
    pub fn on_below_world(entity: &mut Entity, ctx: &mut LevelCtx) {
        ctx.record_event(LifecycleEvent::Hurt {
            id: entity.id,
            amount: ctx.void_damage_amount,
            source: "fell_out_of_world",
        });
    }

    /// Paper `LivingEntity#inactiveTick()`: `super.inactiveTick(); ++noActionTime;`.
    pub fn living_inactive_tick(&mut self, ctx: &mut LevelCtx) {
        self.entity.entity_inactive_tick(ctx);
        self.no_action_time = self.no_action_time.wrapping_add(1);
    }

    /// Paper `LivingEntity#tickDeath()`.
    pub fn tick_death(&mut self, ctx: &mut LevelCtx) {
        self.death_time = self.death_time.wrapping_add(1);
        ctx.record_event(LifecycleEvent::DeathTick {
            id: self.entity.id,
            death_time: self.death_time,
        });
        if self.death_time >= DEATH_REMOVAL_TICKS && !self.entity.is_removed() {
            self.entity
                .request_remove_taken(ctx, EntityRemovalReason::Killed);
        }
    }

    /// Paper `LivingEntity#tick()` with the virtual `baseTick()`, `aiStep()`, and
    /// `tickHeadTurn(float)` overrides supplied as hooks (used by `Mob`).
    pub fn living_tick_with_hooks(
        &mut self,
        ctx: &mut LevelCtx,
        mut run_base_tick: impl FnMut(&mut Self, &mut LevelCtx),
        mut run_ai_step: impl FnMut(&mut Self, &mut LevelCtx),
        mut run_tick_head_turn: impl FnMut(&mut Self, f32, &mut LevelCtx),
    ) {
        // super.tick(): Entity.tick() checks despawnTime, then calls this.baseTick()
        if self.entity.entity_type.despawn_time >= 0
            && self.entity.total_entity_age >= self.entity.entity_type.despawn_time
        {
            if self
                .entity
                .request_remove_taken(ctx, EntityRemovalReason::Discarded)
            {
                ctx.record_event(LifecycleEvent::Despawn { id: self.entity.id });
            }
        } else {
            run_base_tick(self, ctx);
        }

        ctx.record_event(LifecycleEvent::LivingTick { id: self.entity.id });

        if self.arrow_count > 0 {
            if self.remove_arrow_time <= 0 {
                self.remove_arrow_time = 20_i32.wrapping_mul(30_i32.wrapping_sub(self.arrow_count));
            }
            self.remove_arrow_time = self.remove_arrow_time.wrapping_sub(1);
            if self.remove_arrow_time <= 0 {
                self.arrow_count = self.arrow_count.wrapping_sub(1);
            }
        }

        if self.stinger_count > 0 {
            if self.remove_stinger_time <= 0 {
                self.remove_stinger_time =
                    20_i32.wrapping_mul(30_i32.wrapping_sub(self.stinger_count));
            }
            self.remove_stinger_time = self.remove_stinger_time.wrapping_sub(1);
            if self.remove_stinger_time <= 0 {
                self.stinger_count = self.stinger_count.wrapping_sub(1);
            }
        }

        self.detect_equipment_updates(ctx);

        if self.entity.tick_count % 20 == 0 {
            ctx.record_event(LifecycleEvent::CombatTrackerRecheck { id: self.entity.id });
        }

        if self.is_sleeping() && !self.bed_exists {
            self.stop_sleeping();
        }

        if !self.entity.is_removed() {
            run_ai_step(self, ctx);
        }

        // Body rotation & Paper 360-degree rangeChecks
        let xd = self.entity.pos.x - self.entity.xo;
        let zd = self.entity.pos.z - self.entity.zo;
        let side_dist = (xd * xd + zd * zd) as f32;
        let mut y_body_rot_t = self.y_body_rot;
        if side_dist > 0.0025000002_f32 {
            let walk_dir = (mth::atan2(zd, xd) as f32) * mth::RAD_TO_DEG - 90.0_f32;
            let diff = (mth::wrap_degrees_f32(self.entity.y_rot) - walk_dir).abs();
            if 95.0_f32 < diff && diff < 265.0_f32 {
                y_body_rot_t = walk_dir - 180.0_f32;
            } else {
                y_body_rot_t = walk_dir;
            }
        }
        if self.attack_anim > 0.0_f32 {
            y_body_rot_t = self.entity.y_rot;
        }
        run_tick_head_turn(self, y_body_rot_t, ctx);

        // Paper start - stop large pitch and yaw changes from crashing the server
        self.entity.y_rot_o +=
            java_round_f32((self.entity.y_rot - self.entity.y_rot_o) / 360.0_f32) * 360.0_f32;
        self.y_body_rot_o +=
            java_round_f32((self.y_body_rot - self.y_body_rot_o) / 360.0_f32) * 360.0_f32;
        self.entity.x_rot_o +=
            java_round_f32((self.entity.x_rot - self.entity.x_rot_o) / 360.0_f32) * 360.0_f32;
        self.y_head_rot_o +=
            java_round_f32((self.y_head_rot - self.y_head_rot_o) / 360.0_f32) * 360.0_f32;
        // Paper end

        if self.entity.is_fall_flying() {
            self.fall_fly_ticks = self.fall_fly_ticks.wrapping_add(1);
        } else {
            self.fall_fly_ticks = 0;
        }

        if self.is_sleeping() {
            self.entity.x_rot = 0.0_f32;
        }

        if self.current_impulse_context_reset_grace_time > 0 {
            self.current_impulse_context_reset_grace_time = self
                .current_impulse_context_reset_grace_time
                .wrapping_sub(1);
        }
    }

    /// Paper `LivingEntity#tick()`.
    pub fn living_tick(&mut self, ctx: &mut LevelCtx) {
        self.living_tick_with_hooks(
            ctx,
            Self::living_base_tick,
            |this, c| this.living_ai_step(c, |_, _| {}),
            |this, y_body_rot_t, _| this.tick_head_turn(y_body_rot_t),
        );
    }

    /// Paper `LivingEntity#aiStep()`.
    pub fn living_ai_step(
        &mut self,
        ctx: &mut LevelCtx,
        mut run_server_ai_step: impl FnMut(&mut Self, &mut LevelCtx),
    ) {
        ctx.record_event(LifecycleEvent::LivingAiStep { id: self.entity.id });

        if self.no_jump_delay > 0 {
            self.no_jump_delay = self.no_jump_delay.wrapping_sub(1);
        }

        // Clamp tiny movement components (exact Paper thresholds: 9.0E-6 for players, 0.003 otherwise)
        let mut dx = self.entity.delta_movement.x;
        let mut dy = self.entity.delta_movement.y;
        let mut dz = self.entity.delta_movement.z;
        if self.entity.entity_type.is_player {
            if dx * dx + dz * dz < 9.0e-6 {
                dx = 0.0;
                dz = 0.0;
            }
        } else {
            if dx.abs() < 0.003 {
                dx = 0.0;
            }
            if dz.abs() < 0.003 {
                dz = 0.0;
            }
        }
        if dy.abs() < 0.003 {
            dy = 0.0;
        }
        self.entity.delta_movement = Vec3::new(dx, dy, dz);

        if self.is_immobile() {
            self.jumping = false;
            self.xxa = 0.0_f32;
            self.zza = 0.0_f32;
        } else if self.is_effective_ai() {
            run_server_ai_step(self, ctx);
        }

        if !self.jumping {
            self.no_jump_delay = 0;
        }
    }

    /// Paper `LivingEntity#tickHeadTurn(float)`.
    pub fn tick_head_turn(&mut self, y_body_rot_t: f32) {
        let y_body_rot_d = mth::wrap_degrees_f32(y_body_rot_t - self.y_body_rot);
        self.y_body_rot += y_body_rot_d * 0.3_f32;
        let head_diff = mth::wrap_degrees_f32(self.entity.y_rot - self.y_body_rot);
        let max_head_rotation = MAX_HEAD_ROTATION_RELATIVE_TO_BODY;
        if head_diff.abs() > max_head_rotation {
            self.y_body_rot +=
                head_diff - mth::sign(f64::from(head_diff)) as f32 * max_head_rotation;
        }
    }

    /// Paper `LivingEntity#rideTick()`: `super.rideTick(); this.resetFallDistance();`.
    pub fn living_ride_tick(&mut self, ctx: &mut LevelCtx) {
        self.entity.delta_movement = Vec3::new(0.0, 0.0, 0.0);
        self.living_tick(ctx);
        if let Some(vehicle_id) = self.entity.vehicle {
            ctx.position_taken_rider(vehicle_id, &mut self.entity);
        }
        self.entity.fall_distance = 0.0;
    }
}

/// Exact port of Java `Math.round(float)` (JDK-6430675) cast back to `f32`.
#[inline]
fn java_round_f32(value: f32) -> f32 {
    crate::survival::player_survival::java_round_f32(value) as f32
}
