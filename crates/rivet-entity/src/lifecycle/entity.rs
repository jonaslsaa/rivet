//! Port of `net.minecraft.world.entity.Entity` lifecycle foundation
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use super::dimensions::{EntityAttachment, EntityDimensions, Pose};
use super::level_ctx::{LevelCtx, LifecycleEvent};
use crate::storage::{Aabb, EntityRemovalReason};
use rivet_registry::core::{BlockPos, ChunkPos, Vec3};
use rivet_util::mth::{self, Uuid};
use rivet_util::random::SingleThreadedRandomSource;

/// Paper `Entity.FLAG_ONFIRE`.
pub const FLAG_ONFIRE: i32 = 0;
/// Paper `Entity.FLAG_SHIFT_KEY_DOWN`.
pub const FLAG_SHIFT_KEY_DOWN: i32 = 1;
/// Paper `Entity.FLAG_SPRINTING`.
pub const FLAG_SPRINTING: i32 = 3;
/// Paper `Entity.FLAG_SWIMMING`.
pub const FLAG_SWIMMING: i32 = 4;
/// Paper `Entity.FLAG_INVISIBLE`.
pub const FLAG_INVISIBLE: i32 = 5;
/// Paper `Entity.FLAG_GLOWING`.
pub const FLAG_GLOWING: i32 = 6;
/// Paper `Entity.FLAG_FALL_FLYING`.
pub const FLAG_FALL_FLYING: i32 = 7;

/// Paper `Entity.BOARDING_COOLDOWN`.
pub const BOARDING_COOLDOWN: i32 = 60;
/// Paper `Entity.TOTAL_AIR_SUPPLY`.
pub const TOTAL_AIR_SUPPLY: i32 = 300;
/// Paper `Entity.BASE_TICKS_REQUIRED_TO_FREEZE`.
pub const BASE_TICKS_REQUIRED_TO_FREEZE: i32 = 140;

/// Static entity-type metadata needed by the `Entity` / `LivingEntity` / `Mob` lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityTypeMeta {
    pub type_id: String,
    pub dimensions: EntityDimensions,
    pub can_serialize: bool,
    pub allowed_in_peaceful: bool,
    pub fire_immune: bool,
    pub dismounts_underwater: bool,
    pub is_player: bool,
    pub is_boat: bool,
    /// Membership in `EntityTypeTags.NON_CONTROLLING_RIDER` (`Entity#canControlVehicle()`).
    pub non_controlling_rider: bool,
    /// Paper configurable entity despawn time (`Entity.despawnTime`; `-1` disables).
    pub despawn_time: i32,
    /// Maximum number of passengers accepted by default (`1` in `Entity#canAddPassenger`).
    pub max_passengers: usize,
}

impl EntityTypeMeta {
    /// Creates a standard mob/entity type metadata with Paper defaults.
    #[must_use]
    pub fn new(type_id: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            type_id: type_id.into(),
            dimensions: EntityDimensions::scalable(width, height),
            can_serialize: true,
            allowed_in_peaceful: true,
            fire_immune: false,
            dismounts_underwater: true,
            is_player: false,
            is_boat: false,
            non_controlling_rider: false,
            despawn_time: -1,
            max_passengers: 1,
        }
    }

    /// Standard player metadata (`0.6 x 1.8`, `eye_height = 1.62`, `can_serialize = false`).
    #[must_use]
    pub fn player() -> Self {
        let mut meta = Self::new("minecraft:player", 0.6, 1.8);
        meta.dimensions = meta.dimensions.with_eye_height(1.62);
        meta.can_serialize = false;
        meta.is_player = true;
        meta
    }

    /// Standard boat metadata (`1.375 x 0.5625`, `max_passengers = 2`, `is_boat = true`).
    #[must_use]
    pub fn boat() -> Self {
        let mut meta = Self::new("minecraft:oak_boat", 1.375, 0.5625);
        meta.is_boat = true;
        meta.max_passengers = 2;
        meta
    }

    /// Standard hostile mob metadata (`allowed_in_peaceful = false`).
    #[must_use]
    pub fn hostile(type_id: impl Into<String>, width: f32, height: f32) -> Self {
        let mut meta = Self::new(type_id, width, height);
        meta.allowed_in_peaceful = false;
        meta
    }
}

/// Port of `net.minecraft.world.entity.Entity` lifecycle state.
pub struct Entity {
    pub id: i32,
    pub uuid: Uuid,
    pub entity_type: EntityTypeMeta,
    pub pos: Vec3,
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub delta_movement: Vec3,
    pub last_known_position: Option<Vec3>,
    pub last_known_speed: Vec3,
    pub block_pos: BlockPos,
    pub chunk_pos: ChunkPos,
    pub bounding_box: Aabb,
    pub dimensions: EntityDimensions,
    pub eye_height: f32,
    pub pose: Pose,
    pub fixed_pose: bool,
    pub shared_flags: u8,
    pub visual_fire: Option<bool>,
    pub tick_count: i32,
    pub total_entity_age: i32,
    pub first_tick: bool,
    pub on_ground: bool,
    pub no_physics: bool,
    pub remaining_fire_ticks: i32,
    pub air_supply: i32,
    pub max_air_supply: i32,
    pub invulnerable: bool,
    pub invulnerable_time: i32,
    pub portal_cooldown: i32,
    pub fall_distance: f64,
    pub was_in_powder_snow: bool,
    pub is_in_powder_snow: bool,
    pub was_eye_in_water: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub eye_in_water: bool,
    pub in_bubble_column: bool,
    pub in_wall: bool,
    pub in_block_state_present: bool,
    pub boarding_cooldown: i32,
    pub vehicle: Option<i32>,
    pub passengers: Vec<i32>,
    pub removal_reason: Option<EntityRemovalReason>,
    pub valid: bool,
    pub persist: bool,
    pub random: SingleThreadedRandomSource,
}

impl std::fmt::Debug for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entity")
            .field("id", &self.id)
            .field("uuid", &self.uuid)
            .field("type_id", &self.entity_type.type_id)
            .field("pos", &self.pos)
            .field("pose", &self.pose)
            .field("tick_count", &self.tick_count)
            .field("removal_reason", &self.removal_reason)
            .finish_non_exhaustive()
    }
}

impl Entity {
    /// Paper `Entity(EntityType<?>, Level)` constructor.
    #[must_use]
    pub fn new(id: i32, entity_type: EntityTypeMeta, random_seed: i64) -> Self {
        let mut random = SingleThreadedRandomSource::new(random_seed);
        let uuid = mth::create_insecure_uuid(&mut random);
        let dimensions = entity_type.dimensions.clone();
        let eye_height = dimensions.eye_height;
        let bounding_box = dimensions.make_bounding_box(Vec3::new(0.0, 0.0, 0.0));
        let mut entity = Self {
            id,
            uuid,
            entity_type,
            pos: Vec3::new(0.0, 0.0, 0.0),
            xo: 0.0,
            yo: 0.0,
            zo: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            delta_movement: Vec3::new(0.0, 0.0, 0.0),
            last_known_position: None,
            last_known_speed: Vec3::new(0.0, 0.0, 0.0),
            block_pos: BlockPos::new(0, 0, 0),
            chunk_pos: ChunkPos::new(0, 0),
            bounding_box,
            dimensions,
            eye_height,
            pose: Pose::Standing,
            fixed_pose: false,
            shared_flags: 0,
            visual_fire: None,
            tick_count: 0,
            total_entity_age: 0,
            first_tick: true,
            on_ground: false,
            no_physics: false,
            remaining_fire_ticks: 0,
            air_supply: TOTAL_AIR_SUPPLY,
            max_air_supply: TOTAL_AIR_SUPPLY,
            invulnerable: false,
            invulnerable_time: 0,
            portal_cooldown: 0,
            fall_distance: 0.0,
            was_in_powder_snow: false,
            is_in_powder_snow: false,
            was_eye_in_water: false,
            in_water: false,
            in_lava: false,
            eye_in_water: false,
            in_bubble_column: false,
            in_wall: false,
            in_block_state_present: false,
            boarding_cooldown: 0,
            vehicle: None,
            passengers: Vec::new(),
            removal_reason: None,
            valid: false,
            persist: true,
            random,
        };
        entity.set_pos(0.0, 0.0, 0.0);
        entity
    }

    // -------------------------------------------------------------------------
    // Shared flags (Entity.DATA_SHARED_FLAGS_ID)
    // -------------------------------------------------------------------------

    /// Paper `Entity#getSharedFlag(int flag)`.
    #[must_use]
    pub const fn get_shared_flag(&self, flag: i32) -> bool {
        if flag < 0 || flag >= 8 {
            return false;
        }
        (self.shared_flags & (1_u8 << (flag as u32))) != 0
    }

    /// Paper `Entity#setSharedFlag(int flag, boolean value)`.
    pub fn set_shared_flag(&mut self, flag: i32, value: bool) {
        if !(0..8).contains(&flag) {
            return;
        }
        let mask = 1_u8 << (flag as u32);
        if value {
            self.shared_flags |= mask;
        } else {
            self.shared_flags &= !mask;
        }
    }

    /// Paper `Entity#setSharedFlagOnFire(boolean)`:
    /// `setSharedFlag(FLAG_ONFIRE, visualFire.toBooleanOrElse(value))`.
    pub fn set_shared_flag_on_fire(&mut self, value: bool) {
        self.set_shared_flag(FLAG_ONFIRE, self.visual_fire.unwrap_or(value));
    }

    /// Paper `Entity#isOnFire()` on the server (`isClientSide == false`, so the
    /// shared `FLAG_ONFIRE` bit is not consulted).
    #[must_use]
    pub const fn is_on_fire(&self) -> bool {
        !self.entity_type.fire_immune && self.remaining_fire_ticks > 0
    }

    /// Paper `Entity#canControlVehicle()`.
    #[must_use]
    pub const fn can_control_vehicle(&self) -> bool {
        !self.entity_type.non_controlling_rider
    }

    #[must_use]
    pub const fn is_shift_key_down(&self) -> bool {
        self.get_shared_flag(FLAG_SHIFT_KEY_DOWN)
    }

    pub fn set_shift_key_down(&mut self, value: bool) {
        self.set_shared_flag(FLAG_SHIFT_KEY_DOWN, value);
    }

    #[must_use]
    pub const fn is_sprinting(&self) -> bool {
        self.get_shared_flag(FLAG_SPRINTING)
    }

    pub fn set_sprinting(&mut self, value: bool) {
        self.set_shared_flag(FLAG_SPRINTING, value);
    }

    #[must_use]
    pub const fn is_swimming(&self) -> bool {
        self.get_shared_flag(FLAG_SWIMMING)
    }

    pub fn set_swimming(&mut self, value: bool) {
        self.set_shared_flag(FLAG_SWIMMING, value);
    }

    #[must_use]
    pub const fn is_invisible(&self) -> bool {
        self.get_shared_flag(FLAG_INVISIBLE)
    }

    pub fn set_invisible(&mut self, value: bool) {
        self.set_shared_flag(FLAG_INVISIBLE, value);
    }

    #[must_use]
    pub const fn is_currently_glowing(&self) -> bool {
        self.get_shared_flag(FLAG_GLOWING)
    }

    pub fn set_glowing_tag(&mut self, value: bool) {
        self.set_shared_flag(FLAG_GLOWING, value);
    }

    #[must_use]
    pub const fn is_fall_flying(&self) -> bool {
        self.get_shared_flag(FLAG_FALL_FLYING)
    }

    // -------------------------------------------------------------------------
    // Position, rotation, pose, dimensions
    // -------------------------------------------------------------------------

    /// Paper `Entity#setOldPosAndRot()`.
    pub fn set_old_pos_and_rot(&mut self) {
        self.xo = self.pos.x;
        self.yo = self.pos.y;
        self.zo = self.pos.z;
        self.y_rot_o = self.y_rot;
        self.x_rot_o = self.x_rot;
    }

    /// Paper `Entity#setPos(double x, double y, double z)`.
    pub fn set_pos(&mut self, x: f64, y: f64, z: f64) {
        self.set_pos_raw(x, y, z);
        self.bounding_box = self.dimensions.make_bounding_box_xyz(x, y, z);
    }

    /// Paper `Entity#setPosRaw(double x, double y, double z)`.
    pub fn set_pos_raw(&mut self, x: f64, y: f64, z: f64) {
        self.pos = Vec3::new(x, y, z);
        let bx = mth::floor_d(x);
        let by = mth::floor_d(y);
        let bz = mth::floor_d(z);
        self.block_pos = BlockPos::new(bx, by, bz);
        self.chunk_pos = ChunkPos::new(bx >> 4, bz >> 4);
    }

    /// Paper `Entity#reapplyPosition()`.
    pub fn reapply_position(&mut self) {
        self.set_pos(self.pos.x, self.pos.y, self.pos.z);
    }

    /// Paper `Entity#getEyeY()`.
    #[must_use]
    pub fn eye_y(&self) -> f64 {
        self.pos.y + f64::from(self.eye_height)
    }

    /// Paper `Entity#setPose(Pose)`.
    ///
    /// Mirrors Paper `SynchedEntityData#set(DATA_POSE)` -> `Entity#onSyncedDataUpdated(DATA_POSE)`
    /// which synchronously calls `refreshDimensions()` whenever the pose changes.
    /// Returns `true` if `refresh_dimensions()` triggered the non-player growth fudge.
    pub fn set_pose(&mut self, pose: Pose) -> bool {
        if self.fixed_pose || self.pose == pose {
            return false;
        }
        self.pose = pose;
        self.refresh_dimensions()
    }

    /// Paper `Entity#getDimensions(Pose)`.
    #[must_use]
    pub fn dimensions_for_pose(&self, pose: Pose) -> EntityDimensions {
        match pose {
            Pose::Sleeping => EntityDimensions::fixed(0.2, 0.2).with_eye_height(0.2),
            _ => self.entity_type.dimensions.clone(),
        }
    }

    /// Paper `Entity#refreshDimensions()`.
    ///
    /// Updates `dimensions` and `eye_height` from `dimensions_for_pose(self.pose)`,
    /// reapplies the bounding box, and returns `true` if the server-side position
    /// fudge condition (`!first_tick && !no_physics && is_small && grew && !is_player`)
    /// fired.
    pub fn refresh_dimensions(&mut self) -> bool {
        let old_dim = self.dimensions.clone();
        let new_dim = self.dimensions_for_pose(self.pose);
        self.dimensions = new_dim.clone();
        self.eye_height = new_dim.eye_height;
        self.reapply_position();
        let is_small = new_dim.width <= 4.0_f32 && new_dim.height <= 4.0_f32;
        !self.first_tick
            && !self.no_physics
            && is_small
            && (new_dim.width > old_dim.width || new_dim.height > old_dim.height)
            && !self.entity_type.is_player
    }

    // -------------------------------------------------------------------------
    // Removal & lifecycle state
    // -------------------------------------------------------------------------

    /// Paper `Entity#isRemoved()`.
    #[must_use]
    pub const fn is_removed(&self) -> bool {
        self.removal_reason.is_some()
    }

    /// Paper `Entity#isAlive()`.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        !self.is_removed()
    }

    /// Paper `Entity#unsetRemoved()`.
    pub fn unset_removed(&mut self) {
        self.removal_reason = None;
    }

    /// Paper `Entity#clearFire()`: `setRemainingFireTicks(Math.min(0, remainingFireTicks))`.
    pub fn clear_fire(&mut self) {
        self.remaining_fire_ticks = self.remaining_fire_ticks.min(0);
    }

    /// Paper `Entity#igniteForTicks(int)`.
    pub fn ignite_for_ticks(&mut self, ticks: i32) {
        if self.remaining_fire_ticks < ticks {
            self.remaining_fire_ticks = ticks;
        }
    }

    // -------------------------------------------------------------------------
    // Passenger & vehicle queries
    // -------------------------------------------------------------------------

    /// Paper `Entity#isPassenger()`.
    #[must_use]
    pub const fn is_passenger(&self) -> bool {
        self.vehicle.is_some()
    }

    /// Paper `Entity#isVehicle()`.
    #[must_use]
    pub fn is_vehicle(&self) -> bool {
        !self.passengers.is_empty()
    }

    /// Paper `Entity#hasPassenger(Entity)`.
    #[must_use]
    pub fn has_passenger(&self, passenger_id: i32) -> bool {
        self.passengers.contains(&passenger_id)
    }

    /// Paper `Entity#getFirstPassenger()`.
    #[must_use]
    pub fn first_passenger(&self) -> Option<i32> {
        self.passengers.first().copied()
    }

    /// Paper `Entity#couldAcceptPassenger()`.
    #[must_use]
    pub const fn could_accept_passenger(&self) -> bool {
        !self.is_removed()
    }

    /// Paper `Entity#canRide(Entity)`.
    #[must_use]
    pub const fn can_ride(&self) -> bool {
        !self.is_shift_key_down() && self.boarding_cooldown <= 0
    }

    /// Paper `Entity#canAddPassenger(Entity)`.
    #[must_use]
    pub fn can_add_passenger(&self) -> bool {
        self.passengers.len() < self.entity_type.max_passengers
    }

    /// Paper `Entity#getPassengerRidingPosition(Entity)` using `EntityAttachment.PASSENGER`.
    #[must_use]
    pub fn passenger_riding_position(&self, passenger_index: i32) -> Vec3 {
        let attachment = self.dimensions.attachments.get_clamped(
            EntityAttachment::Passenger,
            passenger_index,
            self.y_rot,
        );
        Vec3::new(
            self.pos.x + attachment.x,
            self.pos.y + attachment.y,
            self.pos.z + attachment.z,
        )
    }

    /// Paper `Entity#getVehicleAttachmentPoint(Entity)` using `EntityAttachment.VEHICLE`.
    #[must_use]
    pub fn vehicle_attachment_point(&self) -> Vec3 {
        self.dimensions
            .attachments
            .get_clamped(EntityAttachment::Vehicle, 0, self.y_rot)
    }

    /// Paper `Entity#computeSpeed()`.
    pub fn compute_speed(&mut self) {
        if let Some(last) = self.last_known_position {
            self.last_known_speed = Vec3::new(
                self.pos.x - last.x,
                self.pos.y - last.y,
                self.pos.z - last.z,
            );
        } else {
            self.last_known_speed = Vec3::new(0.0, 0.0, 0.0);
        }
        self.last_known_position = Some(self.pos);
    }

    // -------------------------------------------------------------------------
    // Core tick & baseTick (explicit parent methods called by subclasses)
    // -------------------------------------------------------------------------

    /// Requests removal of a currently-taken entity (`Entity#setRemoved` during tick/despawn).
    ///
    /// Mirrors `LevelCtx::remove_entity`: respects `ctx.section_update_lock`, preserves the
    /// first `removal_reason`, marks `valid = false`, detaches from its vehicle when
    /// `reason.should_destroy()`, and queues post-putback cleanup (`eject_passengers` and
    /// `LifecycleEvent::EntityRemoved`) on `ctx`.
    pub(crate) fn request_remove_taken(
        &mut self,
        ctx: &mut LevelCtx,
        reason: EntityRemovalReason,
    ) -> bool {
        if ctx.section_update_lock.contains(&self.id) {
            return false;
        }
        let first_time = self.removal_reason.is_none();
        if first_time {
            self.removal_reason = Some(reason);
        }
        self.valid = false;
        let effective_reason = self.removal_reason.unwrap_or(reason);
        if effective_reason.should_destroy() {
            ctx.stop_riding_taken(self, false);
        }
        if first_time {
            ctx.queue_pending_removal(self.id, effective_reason);
        }
        true
    }

    /// Paper `Entity#tick()`.
    pub fn entity_tick(&mut self, ctx: &mut LevelCtx) {
        if self.entity_type.despawn_time >= 0
            && self.total_entity_age >= self.entity_type.despawn_time
        {
            if self.request_remove_taken(ctx, EntityRemovalReason::Discarded) {
                ctx.record_event(LifecycleEvent::Despawn { id: self.id });
            }
            return;
        }
        self.entity_base_tick(ctx);
    }

    /// Paper `Entity#baseTick()` with the base `Entity#onBelowWorld()` (discard).
    pub fn entity_base_tick(&mut self, ctx: &mut LevelCtx) {
        self.entity_base_tick_with_hook(ctx, Self::on_below_world);
    }

    /// Paper `Entity#baseTick()` with a virtual `onBelowWorld()` override supplied by
    /// subclasses (`LivingEntity#onBelowWorld` deals void damage instead of discarding).
    pub fn entity_base_tick_with_hook(
        &mut self,
        ctx: &mut LevelCtx,
        on_below_world: impl FnOnce(&mut Self, &mut LevelCtx),
    ) {
        ctx.record_event(LifecycleEvent::EntityBaseTick { id: self.id });
        self.compute_speed();
        self.in_block_state_present = false;

        if let Some(vehicle_id) = self.vehicle
            && ctx.is_entity_removed(vehicle_id)
        {
            ctx.stop_riding_taken(self, false);
        }

        if self.boarding_cooldown > 0 {
            self.boarding_cooldown = self.boarding_cooldown.wrapping_sub(1);
        }

        self.was_in_powder_snow = self.is_in_powder_snow;
        self.is_in_powder_snow = false;

        // Fire ticking (server branch of Paper Entity#baseTick)
        if self.remaining_fire_ticks > 0 {
            if self.entity_type.fire_immune {
                self.clear_fire();
            } else {
                if self.remaining_fire_ticks % 20 == 0 && !self.in_lava {
                    ctx.record_event(LifecycleEvent::Hurt {
                        id: self.id,
                        amount: 1.0,
                        source: "on_fire",
                    });
                }
                self.remaining_fire_ticks = self.remaining_fire_ticks.wrapping_sub(1);
            }
        }

        if self.in_lava {
            self.fall_distance *= 0.5;
        }

        self.check_below_world(ctx, on_below_world);
        self.set_shared_flag_on_fire(self.remaining_fire_ticks > 0);

        self.first_tick = false;
    }

    /// Paper `Entity#checkBelowWorld()`, invoking the supplied `onBelowWorld()` override.
    pub fn check_below_world(
        &mut self,
        ctx: &mut LevelCtx,
        on_below_world: impl FnOnce(&mut Self, &mut LevelCtx),
    ) {
        if !ctx.void_damage_enabled {
            return;
        }
        let threshold = f64::from(
            ctx.min_y
                .wrapping_add(ctx.void_damage_min_build_height_offset),
        );
        if self.pos.y < threshold {
            ctx.record_event(LifecycleEvent::BelowWorld { id: self.id });
            on_below_world(self, ctx);
        }
    }

    /// Paper `Entity#onBelowWorld()`: `discard(OUT_OF_WORLD)`.
    pub fn on_below_world(&mut self, ctx: &mut LevelCtx) {
        self.request_remove_taken(ctx, EntityRemovalReason::Discarded);
    }

    /// Paper `Entity#postTick()` (CraftBukkit hook).
    pub fn entity_post_tick(&mut self, ctx: &mut LevelCtx) {
        ctx.record_event(LifecycleEvent::EntityPostTick { id: self.id });
    }

    /// Paper `Entity#inactiveTick()` (Paper EAR 2).
    pub fn entity_inactive_tick(&mut self, ctx: &mut LevelCtx) {
        ctx.record_event(LifecycleEvent::EntityInactiveTick { id: self.id });
    }

    /// Paper `Entity#rideTick()`.
    ///
    /// Zeroes `delta_movement`, calls `entity_tick`, and if still a passenger,
    /// positions the rider via `LevelCtx`.
    pub fn entity_ride_tick(&mut self, ctx: &mut LevelCtx) {
        self.delta_movement = Vec3::new(0.0, 0.0, 0.0);
        self.entity_tick(ctx);
        if let Some(vehicle_id) = self.vehicle {
            ctx.position_taken_rider(vehicle_id, self);
        }
    }
}
