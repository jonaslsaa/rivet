//! Single-threaded, tick-thread-owned `LevelCtx` using take-tick-putback without
//! `Arc<RwLock>` game state (per `OWNERSHIP.md` and Paper 26.2 `ServerLevel.java`).

use super::dimensions::Pose;
use super::entity::{BOARDING_COOLDOWN, Entity};
use super::equipment::{EquipmentSlot, EquipmentSlotChange};
use super::living::LivingEntity;
use super::mob::{Mob, MobControlFlags};
use crate::damage::Difficulty;
use crate::storage::{EntityRemovalReason, NetworkIdAllocator};
use indexmap::IndexMap;
use rivet_registry::core::{BlockPos, Vec3};
use std::collections::HashSet;

/// Unified entity node stored in `LevelCtx` preserving Paper's inheritance hierarchy
/// via struct embedding (`Entity` <- `LivingEntity` <- `Mob`).
#[derive(Debug)]
pub enum LifecycleEntity {
    Base(Entity),
    Living(LivingEntity),
    Mob(Mob),
}

impl LifecycleEntity {
    /// Immutable access to the embedded base [`Entity`].
    #[must_use]
    pub const fn entity(&self) -> &Entity {
        match self {
            Self::Base(e) => e,
            Self::Living(l) => &l.entity,
            Self::Mob(m) => &m.living.entity,
        }
    }

    /// Mutable access to the embedded base [`Entity`].
    pub fn entity_mut(&mut self) -> &mut Entity {
        match self {
            Self::Base(e) => e,
            Self::Living(l) => &mut l.entity,
            Self::Mob(m) => &mut m.living.entity,
        }
    }

    /// Immutable access to the embedded [`LivingEntity`], if applicable.
    #[must_use]
    pub const fn living(&self) -> Option<&LivingEntity> {
        match self {
            Self::Base(_) => None,
            Self::Living(l) => Some(l),
            Self::Mob(m) => Some(&m.living),
        }
    }

    /// Mutable access to the embedded [`LivingEntity`], if applicable.
    pub fn living_mut(&mut self) -> Option<&mut LivingEntity> {
        match self {
            Self::Base(_) => None,
            Self::Living(l) => Some(l),
            Self::Mob(m) => Some(&mut m.living),
        }
    }

    /// Immutable access to [`Mob`], if applicable.
    #[must_use]
    pub const fn mob(&self) -> Option<&Mob> {
        match self {
            Self::Mob(m) => Some(m),
            _ => None,
        }
    }

    /// Mutable access to [`Mob`], if applicable.
    pub fn mob_mut(&mut self) -> Option<&mut Mob> {
        match self {
            Self::Mob(m) => Some(m),
            _ => None,
        }
    }

    /// Network entity ID (`Entity#getId()`).
    #[must_use]
    pub const fn id(&self) -> i32 {
        self.entity().id
    }

    /// Paper `Entity#isAlive()` / `LivingEntity#isAlive()`.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        match self {
            Self::Base(e) => e.is_alive(),
            Self::Living(l) => l.is_alive(),
            Self::Mob(m) => m.living.is_alive(),
        }
    }

    /// Dispatches `tick()` with explicit Paper parent-call order.
    pub fn tick(&mut self, ctx: &mut LevelCtx) {
        match self {
            Self::Base(e) => e.entity_tick(ctx),
            Self::Living(l) => l.living_tick(ctx),
            Self::Mob(m) => m.mob_tick(ctx),
        }
    }

    /// Dispatches `rideTick()` with explicit Paper parent-call order.
    pub fn ride_tick(&mut self, ctx: &mut LevelCtx) {
        match self {
            Self::Base(e) => e.entity_ride_tick(ctx),
            Self::Living(l) => l.living_ride_tick(ctx),
            Self::Mob(m) => m.mob_ride_tick(ctx),
        }
    }

    /// Dispatches `postTick()`.
    pub fn post_tick(&mut self, ctx: &mut LevelCtx) {
        self.entity_mut().entity_post_tick(ctx);
    }

    /// Dispatches `inactiveTick()` with explicit Paper parent-call order.
    pub fn inactive_tick(&mut self, ctx: &mut LevelCtx) {
        match self {
            Self::Base(e) => e.entity_inactive_tick(ctx),
            Self::Living(l) => l.living_inactive_tick(ctx),
            Self::Mob(m) => m.mob_inactive_tick(ctx),
        }
    }

    /// Dispatches `Mob#checkDespawn()` (no-op for non-mob entities).
    pub fn check_despawn(&mut self, ctx: &mut LevelCtx) -> bool {
        match self {
            Self::Mob(m) => m.check_despawn(ctx),
            Self::Base(_) | Self::Living(_) => false,
        }
    }
}

/// Observable lifecycle events emitted in exact Paper execution order.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleEvent {
    EntityAdded {
        id: i32,
    },
    EntityRemoved {
        id: i32,
        reason: EntityRemovalReason,
    },
    EntityTickStart {
        id: i32,
        as_passenger: bool,
    },
    EntityBaseTick {
        id: i32,
    },
    EntityPostTick {
        id: i32,
    },
    EntityInactiveTick {
        id: i32,
    },
    LivingBaseTick {
        id: i32,
    },
    LivingTick {
        id: i32,
    },
    LivingAiStep {
        id: i32,
    },
    MobBaseTick {
        id: i32,
    },
    MobTick {
        id: i32,
    },
    MobServerAiStep {
        id: i32,
        full_selector_tick: bool,
    },
    MobControlFlagsUpdated {
        id: i32,
        flags: MobControlFlags,
    },
    MobAmbientSound {
        id: i32,
    },
    Mount {
        passenger_id: i32,
        vehicle_id: i32,
    },
    /// `GameEvent.ENTITY_DISMOUNT` from `Entity#removeVehicle` (emitted even when the
    /// dismount is cancelled, as in Paper).
    Dismount {
        passenger_id: i32,
        vehicle_id: i32,
    },
    PositionRider {
        vehicle_id: i32,
        passenger_id: i32,
        pos: Vec3,
    },
    EquipmentEquipped {
        id: i32,
        slot: EquipmentSlot,
        silent: bool,
    },
    EquipmentChanged {
        id: i32,
        changes: Vec<EquipmentSlotChange>,
    },
    HandSwap {
        id: i32,
    },
    Hurt {
        id: i32,
        amount: f32,
        source: &'static str,
    },
    DeathTick {
        id: i32,
        death_time: i32,
    },
    DrownParticles {
        id: i32,
    },
    ChangedBlock {
        id: i32,
        pos: BlockPos,
    },
    BelowWorld {
        id: i32,
    },
    CombatTrackerRecheck {
        id: i32,
    },
    Despawn {
        id: i32,
    },
}

/// Single-threaded, tick-thread-owned level context implementing take-tick-putback.
///
/// Entities are stored in an `IndexMap<i32, Option<LifecycleEntity>>`:
/// - `take_entity(id)` moves the entity out of its slot (`Option::take()`), leaving the
///   key in place so iteration order is preserved across putback.
/// - While an entity is taken, it can be ticked with `&mut LevelCtx`, allowing safe
///   reads/mutations of *other* entities by ID without `Arc<RwLock>`.
/// - `put_back_entity(entity)` restores the entity into its exact slot.
#[derive(Debug)]
pub struct LevelCtx {
    entities: IndexMap<i32, Option<LifecycleEntity>>,
    taken_ids: HashSet<i32>,
    id_allocator: NetworkIdAllocator,
    pub game_time: i64,
    pub difficulty: Difficulty,
    pub min_y: i32,
    pub void_damage_enabled: bool,
    pub void_damage_min_build_height_offset: i32,
    /// `CraftWorld#getVoidDamageAmount()` (Paper world config default `4.0`).
    pub void_damage_amount: f32,
    pub nearest_player_pos: Option<Vec3>,
    /// Paper Moonrise `canRemoveEntity` guard: IDs locked during section status updates.
    pub section_update_lock: HashSet<i32>,
    /// Test/plugin hook: cancels mount attempts by valid passengers
    /// (`VehicleEnterEvent` / `EntityMountEvent`).
    pub cancel_mount: bool,
    /// Test/plugin hook: cancels non-suppressed dismount attempts (`EntityDismountEvent`).
    pub cancel_dismount: bool,
    pub events: Vec<LifecycleEvent>,
    /// Removed entities whose storage slot is dropped at the next purge point. Purging is
    /// deferred so an entity discarded by `checkDespawn` still receives its tick, as in Paper.
    purge_queue: Vec<i32>,
}

impl Default for LevelCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl LevelCtx {
    /// Creates a new `LevelCtx` with standard Overworld defaults (`min_y = -64`,
    /// `void_damage_min_build_height_offset = -64`, `Difficulty::Normal`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: IndexMap::new(),
            taken_ids: HashSet::new(),
            id_allocator: NetworkIdAllocator::new(),
            game_time: 0,
            difficulty: Difficulty::Normal,
            min_y: -64,
            void_damage_enabled: true,
            void_damage_min_build_height_offset: -64,
            void_damage_amount: 4.0,
            nearest_player_pos: None,
            section_update_lock: HashSet::new(),
            cancel_mount: false,
            cancel_dismount: false,
            events: Vec::new(),
            purge_queue: Vec::new(),
        }
    }

    /// Allocates the next Paper `Entity.ENTITY_COUNTER` network ID (starting at 1).
    pub fn next_entity_id(&mut self) -> i32 {
        self.id_allocator.next_id()
    }

    /// Records an observable lifecycle event.
    pub fn record_event(&mut self, event: LifecycleEvent) {
        self.events.push(event);
    }

    /// Clears recorded events and returns them.
    pub fn take_events(&mut self) -> Vec<LifecycleEvent> {
        std::mem::take(&mut self.events)
    }

    /// Inserts an entity into the level in exact insertion order and marks it `valid = true`.
    ///
    /// Returns `false` if an entity with the same ID already exists.
    pub fn add_entity(&mut self, mut entity: LifecycleEntity) -> bool {
        let id = entity.id();
        if self.entities.contains_key(&id) {
            return false;
        }
        entity.entity_mut().valid = true;
        self.entities.insert(id, Some(entity));
        self.record_event(LifecycleEvent::EntityAdded { id });
        true
    }

    /// Returns an immutable reference to a resident (non-taken) entity.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&LifecycleEntity> {
        self.entities.get(&id).and_then(Option::as_ref)
    }

    /// Returns a mutable reference to a resident (non-taken) entity.
    pub fn get_mut(&mut self, id: i32) -> Option<&mut LifecycleEntity> {
        self.entities.get_mut(&id).and_then(Option::as_mut)
    }

    /// Returns whether an entity ID is currently taken out for ticking.
    #[must_use]
    pub fn is_taken(&self, id: i32) -> bool {
        self.taken_ids.contains(&id)
    }

    /// Returns all registered entity IDs in exact `IndexMap` tick order.
    #[must_use]
    pub fn entity_ids(&self) -> Vec<i32> {
        self.entities.keys().copied().collect()
    }

    /// Returns the number of resident/taken entity slots currently stored in the level.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    // -------------------------------------------------------------------------
    // Take-tick-putback core
    // -------------------------------------------------------------------------

    /// Takes an entity out of storage for mutation/ticking while keeping its
    /// `IndexMap` slot reserved. Returns `None` if missing or already taken.
    pub fn take_entity(&mut self, id: i32) -> Option<LifecycleEntity> {
        if self.taken_ids.contains(&id) {
            return None;
        }
        let slot = self.entities.get_mut(&id)?;
        let entity = slot.take()?;
        self.taken_ids.insert(id);
        Some(entity)
    }

    /// Puts a previously taken entity back into its reserved `IndexMap` slot.
    pub fn put_back_entity(&mut self, entity: LifecycleEntity) {
        let id = entity.id();
        self.taken_ids.remove(&id);
        if let Some(slot) = self.entities.get_mut(&id) {
            *slot = Some(entity);
        } else {
            self.entities.insert(id, Some(entity));
        }
    }

    /// Runs `ServerLevel#tickNonPassenger(Entity)` using take-tick-putback,
    /// then recursively runs `tick_passenger` for every passenger in order.
    pub fn tick_non_passenger(&mut self, id: i32, is_active: bool) {
        let Some(mut entity) = self.take_entity(id) else {
            return;
        };

        self.record_event(LifecycleEvent::EntityTickStart {
            id,
            as_passenger: false,
        });
        entity.entity_mut().set_old_pos_and_rot();
        entity.entity_mut().tick_count = entity.entity().tick_count.wrapping_add(1);
        entity.entity_mut().total_entity_age = entity.entity().total_entity_age.wrapping_add(1);

        if is_active {
            entity.tick(self);
            entity.post_tick(self);
        } else {
            entity.inactive_tick(self);
        }

        let passengers = entity.entity().passengers.clone();
        self.put_back_entity(entity);

        for passenger_id in passengers {
            self.tick_passenger(id, passenger_id, is_active);
        }
        self.purge_removed();
    }

    /// Runs `ServerLevel#tickPassenger(Entity vehicle, Entity passenger, boolean isActive)`
    /// using take-tick-putback, recursing into nested passengers.
    ///
    /// A removed passenger, or one whose vehicle is no longer `vehicle_id`, gets a plain
    /// `entity.stopRiding()` (cancellable, acting on its actual vehicle) instead of a tick.
    pub fn tick_passenger(&mut self, vehicle_id: i32, passenger_id: i32, is_active: bool) {
        let Some(p) = self.get(passenger_id) else {
            // Purged passenger still listed by its vehicle.
            self.stop_riding_purged_passenger(vehicle_id, passenger_id);
            return;
        };
        if p.entity().is_removed() || p.entity().vehicle != Some(vehicle_id) {
            self.stop_riding(passenger_id, false);
            return;
        }

        let Some(mut passenger) = self.take_entity(passenger_id) else {
            return;
        };

        self.record_event(LifecycleEvent::EntityTickStart {
            id: passenger_id,
            as_passenger: true,
        });
        passenger.entity_mut().set_old_pos_and_rot();
        passenger.entity_mut().tick_count = passenger.entity().tick_count.wrapping_add(1);
        passenger.entity_mut().total_entity_age =
            passenger.entity().total_entity_age.wrapping_add(1);

        if is_active {
            passenger.ride_tick(self);
            passenger.post_tick(self);
        } else {
            passenger.entity_mut().delta_movement = Vec3::new(0.0, 0.0, 0.0);
            passenger.inactive_tick(self);
            if let Some(vid) = passenger.entity().vehicle {
                self.position_taken_rider(vid, passenger.entity_mut());
            }
        }

        let sub_passengers = passenger.entity().passengers.clone();
        self.put_back_entity(passenger);

        for sub_id in sub_passengers {
            self.tick_passenger(passenger_id, sub_id, is_active);
        }
    }

    /// Runs the Paper `ServerLevel#tick` entity loop over all entities in `IndexMap` order.
    ///
    /// For each non-removed entity: `checkDespawn()`; then, without re-checking removal,
    /// skip it if its vehicle is live and still lists it, otherwise `stopRiding()` and
    /// `tickNonPassenger`. A mob discarded by `checkDespawn()` therefore still ticks once.
    pub fn tick_all(&mut self, is_active: bool) {
        self.game_time = self.game_time.wrapping_add(1);
        let ids: Vec<i32> = self.entities.keys().copied().collect();
        for id in ids {
            if self.get(id).is_none_or(|e| e.entity().is_removed()) {
                continue;
            }

            if matches!(self.get(id), Some(LifecycleEntity::Mob(_)))
                && let Some(mut entity) = self.take_entity(id)
            {
                entity.check_despawn(self);
                self.put_back_entity(entity);
            }

            let vehicle_id = self.get(id).and_then(|e| e.entity().vehicle);
            if let Some(vehicle_id) = vehicle_id {
                let still_riding = self
                    .get(vehicle_id)
                    .is_some_and(|v| !v.entity().is_removed() && v.entity().has_passenger(id));
                if still_riding {
                    self.purge_removed();
                    continue;
                }
                self.stop_riding(id, false);
            }

            self.tick_non_passenger(id, is_active);
            self.purge_removed();
        }
    }

    // -------------------------------------------------------------------------
    // Queries used by ticking entities via &LevelCtx
    // -------------------------------------------------------------------------

    /// Returns whether entity `id` is removed (or absent).
    #[must_use]
    pub fn is_entity_removed(&self, id: i32) -> bool {
        self.get(id).is_none_or(|e| e.entity().is_removed())
    }

    /// Returns whether entity `id` is alive.
    #[must_use]
    pub fn is_entity_alive(&self, id: i32) -> bool {
        self.get(id).is_some_and(LifecycleEntity::is_alive)
    }

    /// Returns whether entity `id` is a boat (`EntityTypeMeta::is_boat`).
    #[must_use]
    pub fn is_entity_boat(&self, id: i32) -> bool {
        self.get(id).is_some_and(|e| e.entity().entity_type.is_boat)
    }

    /// Returns whether entity `id` dismounts passengers underwater.
    #[must_use]
    pub fn vehicle_dismounts_underwater(&self, id: i32) -> bool {
        self.get(id)
            .is_some_and(|e| e.entity().entity_type.dismounts_underwater)
    }

    /// Returns whether entity `id` is a `Mob` (`entity instanceof Mob`).
    #[must_use]
    pub fn is_entity_mob(&self, id: i32) -> bool {
        self.get(id).is_some_and(|e| e.mob().is_some())
    }

    /// `entity instanceof Mob && entity.canControlVehicle()`, used by
    /// `Mob#getControllingPassenger()`.
    #[must_use]
    pub fn is_entity_mob_controlling_vehicle(&self, id: i32) -> bool {
        self.get(id)
            .is_some_and(|e| e.mob().is_some() && e.entity().can_control_vehicle())
    }

    /// Paper `Entity#getRootVehicle()`.
    #[must_use]
    pub fn root_vehicle(&self, mut id: i32) -> Option<i32> {
        let mut visited = 0usize;
        let limit = self.entities.len().saturating_add(1);
        while let Some(e) = self.get(id) {
            let Some(parent) = e.entity().vehicle else {
                return Some(id);
            };
            id = parent;
            visited += 1;
            if visited > limit {
                return None;
            }
        }
        None
    }

    /// Paper `Entity#isPassengerOfSameVehicle(Entity)`.
    #[must_use]
    pub fn is_passenger_of_same_vehicle(&self, a: i32, b: i32) -> bool {
        match (self.root_vehicle(a), self.root_vehicle(b)) {
            (Some(ra), Some(rb)) => ra == rb,
            _ => false,
        }
    }

    /// Paper `Entity#hasIndirectPassenger(Entity)`.
    #[must_use]
    pub fn has_indirect_passenger(&self, vehicle_id: i32, candidate_id: i32) -> bool {
        let Some(e) = self.get(vehicle_id) else {
            return false;
        };
        for &pid in &e.entity().passengers {
            if pid == candidate_id || self.has_indirect_passenger(pid, candidate_id) {
                return true;
            }
        }
        false
    }

    /// Paper `Entity#getSelfAndPassengers()` (depth-first preorder).
    #[must_use]
    pub fn self_and_passengers(&self, id: i32) -> Vec<i32> {
        let mut out = Vec::new();
        self.collect_self_and_passengers(id, &mut out);
        out
    }

    fn collect_self_and_passengers(&self, id: i32, out: &mut Vec<i32>) {
        let Some(e) = self.get(id) else {
            return;
        };
        out.push(id);
        for &pid in &e.entity().passengers {
            self.collect_self_and_passengers(pid, out);
        }
    }

    // -------------------------------------------------------------------------
    // Passenger & vehicle mutations (exact Paper Entity.java semantics)
    // -------------------------------------------------------------------------

    /// Paper `Entity#startRiding(Entity entityToRide, boolean force, boolean emitEvent)`.
    ///
    /// Enforces all Paper preconditions:
    /// - Rejects self-ride and already-riding-same-vehicle.
    /// - Rejects missing or non-accepting vehicle (`couldAcceptPassenger`).
    /// - Rejects non-serializable vehicle when `!force` (unless `can_serialize`).
    /// - Rejects vehicle cycles (walking `entityToRide.vehicle` up to root).
    /// - Rejects when `!force && !(passenger.canRide() && vehicle.canAddPassenger())`.
    /// - Rejects if `cancel_mount` is set and the passenger is `valid` (CraftBukkit
    ///   `VehicleEnterEvent` / `EntityMountEvent`, fired regardless of `emit_event`).
    /// - Stops existing riding (result ignored) before attaching to the new vehicle.
    /// - `emit_event` (`sendEventAndTriggers`) only controls the `ENTITY_MOUNT` game event.
    /// - Resets passenger pose to `Pose::Standing` and attaches with player-priority order.
    /// - Applies the `Mob#startRiding` override: a mounted leashed mob loses its leash.
    pub fn start_riding(
        &mut self,
        passenger_id: i32,
        vehicle_id: i32,
        force: bool,
        emit_event: bool,
    ) -> bool {
        if passenger_id == vehicle_id {
            return false;
        }
        let (already_same, can_ride, has_old_vehicle, passenger_is_player, passenger_valid) = {
            let Some(p) = self.get(passenger_id) else {
                return false;
            };
            let pe = p.entity();
            (
                pe.vehicle == Some(vehicle_id),
                pe.can_ride(),
                pe.is_passenger(),
                pe.entity_type.is_player,
                pe.valid,
            )
        };
        if already_same {
            return false;
        }

        let (could_accept, can_serialize, can_add) = {
            let Some(v) = self.get(vehicle_id) else {
                return false;
            };
            let ve = v.entity();
            (
                ve.could_accept_passenger(),
                ve.entity_type.can_serialize,
                ve.can_add_passenger(),
            )
        };
        if !could_accept {
            return false;
        }
        if !force && !can_serialize {
            return false;
        }

        // Cycle check: walk up vehicle_id's vehicle chain; if we encounter passenger_id, reject.
        let mut cursor = Some(vehicle_id);
        let mut steps = 0usize;
        let limit = self.entities.len().saturating_add(1);
        while let Some(curr) = cursor {
            let next = self.get(curr).and_then(|e| e.entity().vehicle);
            if next == Some(passenger_id) {
                return false;
            }
            cursor = next;
            steps += 1;
            if steps > limit {
                return false;
            }
        }

        if !force && (!can_ride || !can_add) {
            return false;
        }

        if passenger_valid && self.cancel_mount {
            return false;
        }

        if has_old_vehicle {
            self.stop_riding(passenger_id, false);
        }

        if let Some(p) = self.get_mut(passenger_id) {
            let pe = p.entity_mut();
            pe.set_pose(Pose::Standing);
            pe.vehicle = Some(vehicle_id);
        }

        // Paper Entity#addPassenger: if passenger is Player and first passenger is not Player,
        // insert at index 0; otherwise append.
        let first_is_player = self
            .get(vehicle_id)
            .and_then(|v| v.entity().first_passenger())
            .and_then(|fid| self.get(fid))
            .is_some_and(|fe| fe.entity().entity_type.is_player);

        if let Some(v) = self.get_mut(vehicle_id) {
            let passengers = &mut v.entity_mut().passengers;
            if passenger_is_player && !first_is_player {
                passengers.insert(0, passenger_id);
            } else {
                passengers.push(passenger_id);
            }
        }

        if emit_event {
            self.record_event(LifecycleEvent::Mount {
                passenger_id,
                vehicle_id,
            });
        }

        // Paper `Mob#startRiding` override: a successfully mounted leashed mob fires
        // `EntityUnleashEvent` (never cancelled here) and drops or removes its leash; both
        // clear the leash data.
        if let Some(mob) = self
            .get_mut(passenger_id)
            .and_then(LifecycleEntity::mob_mut)
            && mob.leashed
        {
            mob.leashed = false;
        }
        true
    }

    /// Paper `Entity#stopRiding(boolean suppressCancellation)` / `removeVehicle`.
    ///
    /// The vehicle side follows `Entity#removePassenger`: the dismount is cancelled
    /// (restoring `passenger.vehicle`, returning `false`) only when `cancel_dismount` is set,
    /// `!suppress_cancellation`, and the vehicle is still `valid`. A purged vehicle counts as
    /// invalid. The `ENTITY_DISMOUNT` game event is emitted either way when the passenger is
    /// not removed or was removed for a destroying reason.
    pub fn stop_riding(&mut self, passenger_id: i32, suppress_cancellation: bool) -> bool {
        let (vehicle_id, removal_reason) = {
            let Some(p) = self.get(passenger_id) else {
                return false;
            };
            let Some(vid) = p.entity().vehicle else {
                return false;
            };
            (vid, p.entity().removal_reason)
        };

        // Clear vehicle first (matching Paper removeVehicle -> removePassenger order)
        if let Some(p) = self.get_mut(passenger_id) {
            p.entity_mut().vehicle = None;
        }

        let cancel_dismount = self.cancel_dismount;
        let removed = self.get_mut(vehicle_id).is_none_or(|v| {
            Self::vehicle_remove_passenger(
                cancel_dismount,
                v.entity_mut(),
                passenger_id,
                suppress_cancellation,
            )
        });

        if let Some(p) = self.get_mut(passenger_id) {
            let pe = p.entity_mut();
            if removed {
                pe.boarding_cooldown = BOARDING_COOLDOWN;
            } else {
                pe.vehicle = Some(vehicle_id);
            }
        }

        self.record_dismount(passenger_id, vehicle_id, removal_reason);
        removed
    }

    /// Paper `Entity#stopRiding(boolean suppressCancellation)` for a currently-taken passenger
    /// whose vehicle is resident (or purged), with the same semantics as [`Self::stop_riding`].
    pub(crate) fn stop_riding_taken(
        &mut self,
        passenger: &mut Entity,
        suppress_cancellation: bool,
    ) -> bool {
        let Some(vehicle_id) = passenger.vehicle else {
            return false;
        };
        let passenger_id = passenger.id;

        passenger.vehicle = None;

        let cancel_dismount = self.cancel_dismount;
        let removed = self.get_mut(vehicle_id).is_none_or(|v| {
            Self::vehicle_remove_passenger(
                cancel_dismount,
                v.entity_mut(),
                passenger_id,
                suppress_cancellation,
            )
        });
        if removed {
            passenger.boarding_cooldown = BOARDING_COOLDOWN;
        } else {
            passenger.vehicle = Some(vehicle_id);
        }

        self.record_dismount(passenger_id, vehicle_id, passenger.removal_reason);
        removed
    }

    /// Vehicle side of Paper `Entity#removePassenger(Entity, boolean suppressCancellation)`.
    ///
    /// CraftBukkit fires `VehicleExitEvent` / `EntityDismountEvent` only `if (this.valid)`
    /// (`this` being the vehicle), so `cancel_dismount` applies only to a valid vehicle.
    /// Returns whether `passenger_id` was removed from `vehicle.passengers`.
    fn vehicle_remove_passenger(
        cancel_dismount: bool,
        vehicle: &mut Entity,
        passenger_id: i32,
        suppress_cancellation: bool,
    ) -> bool {
        if vehicle.valid && cancel_dismount && !suppress_cancellation {
            return false;
        }
        vehicle.passengers.retain(|&pid| pid != passenger_id);
        true
    }

    /// `level().gameEvent(passenger, GameEvent.ENTITY_DISMOUNT, ..)` from
    /// `Entity#removeVehicle`, gated on the passenger's removal reason.
    fn record_dismount(
        &mut self,
        passenger_id: i32,
        vehicle_id: i32,
        removal_reason: Option<EntityRemovalReason>,
    ) {
        if removal_reason.is_none_or(EntityRemovalReason::should_destroy) {
            self.record_event(LifecycleEvent::Dismount {
                passenger_id,
                vehicle_id,
            });
        }
    }

    /// Paper `Entity#ejectPassengers()` — iterates passengers in reverse order.
    pub fn eject_passengers(&mut self, vehicle_id: i32) {
        let passengers = match self.get(vehicle_id) {
            Some(v) => v.entity().passengers.clone(),
            None => return,
        };
        for passenger_id in passengers.into_iter().rev() {
            self.stop_riding_listed_passenger(vehicle_id, passenger_id);
        }
    }

    /// Paper `Entity#setRemoved`: `getPassengers().forEach(Entity::stopRiding)` (forward order).
    fn stop_riding_passengers_of_removed(&mut self, vehicle_id: i32) {
        let passengers = match self.get(vehicle_id) {
            Some(v) => v.entity().passengers.clone(),
            None => return,
        };
        for passenger_id in passengers {
            self.stop_riding_listed_passenger(vehicle_id, passenger_id);
        }
    }

    /// `passenger.stopRiding()` for an entry of `vehicle_id`'s passenger list.
    fn stop_riding_listed_passenger(&mut self, vehicle_id: i32, passenger_id: i32) {
        if self.get(passenger_id).is_some() {
            self.stop_riding(passenger_id, false);
        } else {
            self.stop_riding_purged_passenger(vehicle_id, passenger_id);
        }
    }

    /// Detaches a passenger ID whose entity was already purged from storage but is still
    /// listed by `vehicle_id` (dismount cancellation kept it attached when it was removed).
    fn stop_riding_purged_passenger(&mut self, vehicle_id: i32, passenger_id: i32) {
        let cancel_dismount = self.cancel_dismount;
        let Some(v) = self.get_mut(vehicle_id) else {
            return;
        };
        if Self::vehicle_remove_passenger(cancel_dismount, v.entity_mut(), passenger_id, false) {
            self.record_event(LifecycleEvent::Dismount {
                passenger_id,
                vehicle_id,
            });
        }
    }

    /// Paper `Entity#setRemoved`: `getPassengers().forEach(Entity::stopRiding)` for a
    /// currently-taken vehicle (removal requested during its own tick or despawn check).
    pub(crate) fn stop_riding_passengers_of_taken(&mut self, vehicle: &mut Entity) {
        for passenger_id in vehicle.passengers.clone() {
            let actual_vehicle = match self.get(passenger_id) {
                Some(p) => p.entity().vehicle,
                None => {
                    let cancel_dismount = self.cancel_dismount;
                    if Self::vehicle_remove_passenger(cancel_dismount, vehicle, passenger_id, false)
                    {
                        self.record_event(LifecycleEvent::Dismount {
                            passenger_id,
                            vehicle_id: vehicle.id,
                        });
                    }
                    continue;
                }
            };
            match actual_vehicle {
                Some(vid) if vid == vehicle.id => {
                    self.stop_riding_of_taken_vehicle(vehicle, passenger_id);
                }
                Some(_) => {
                    self.stop_riding(passenger_id, false);
                }
                None => {}
            }
        }
    }

    /// `passenger.stopRiding()` for a resident passenger whose vehicle is currently taken.
    fn stop_riding_of_taken_vehicle(&mut self, vehicle: &mut Entity, passenger_id: i32) {
        let cancel_dismount = self.cancel_dismount;
        let Some(p) = self.get_mut(passenger_id) else {
            return;
        };
        let pe = p.entity_mut();
        pe.vehicle = None;
        let removed = Self::vehicle_remove_passenger(cancel_dismount, vehicle, passenger_id, false);
        if removed {
            pe.boarding_cooldown = BOARDING_COOLDOWN;
        } else {
            pe.vehicle = Some(vehicle.id);
        }
        let removal_reason = pe.removal_reason;
        self.record_dismount(passenger_id, vehicle.id, removal_reason);
    }

    /// Moonrise `EntityLookup` callback `onRemove(reason)`: emits
    /// `LifecycleEvent::EntityRemoved` and queues the storage purge. The caller has already
    /// cleared `valid` (`ServerLevel` `onTrackingEnd`). Later `onRemove` calls are no-ops
    /// (`NoOpCallback`), so callers invoke this only for the first removal.
    pub(crate) fn on_entity_removed(&mut self, id: i32, reason: EntityRemovalReason) {
        self.record_event(LifecycleEvent::EntityRemoved { id, reason });
        self.purge_queue.push(id);
    }

    /// Drops the storage slots of removed entities that are not currently taken.
    fn purge_removed(&mut self) {
        let queue = std::mem::take(&mut self.purge_queue);
        for id in queue {
            if self.taken_ids.contains(&id) {
                self.purge_queue.push(id);
            } else {
                self.entities.shift_remove(&id);
            }
        }
    }

    /// Positions a currently-taken rider relative to its resident vehicle (`Entity#positionRider`).
    pub(crate) fn position_taken_rider(&mut self, vehicle_id: i32, passenger: &mut Entity) {
        let Some(vehicle) = self.get(vehicle_id) else {
            return;
        };
        let ve = vehicle.entity();
        let Some(idx) = ve.passengers.iter().position(|&pid| pid == passenger.id) else {
            return;
        };
        let riding_pos = ve.passenger_riding_position(idx as i32);
        let attach = passenger.vehicle_attachment_point();
        let new_pos = Vec3::new(
            riding_pos.x - attach.x,
            riding_pos.y - attach.y,
            riding_pos.z - attach.z,
        );
        passenger.set_pos(new_pos.x, new_pos.y, new_pos.z);
        self.record_event(LifecycleEvent::PositionRider {
            vehicle_id,
            passenger_id: passenger.id,
            pos: new_pos,
        });
    }

    // -------------------------------------------------------------------------
    // Removal (exact Paper Entity#setRemoved semantics)
    // -------------------------------------------------------------------------

    /// Paper `Entity#setRemoved(RemovalReason)` for a resident entity.
    ///
    /// - Respects Moonrise `section_update_lock` (`canRemoveEntity` guard).
    /// - Preserves the first `removal_reason`.
    /// - Calls `stop_riding` when `reason.should_destroy()`.
    /// - Stops every passenger riding (forward order) when `reason != UnloadedToChunk`.
    /// - On the first removal only (`levelCallback.onRemove`), marks `valid = false` after the
    ///   dismounts above, emits `LifecycleEvent::EntityRemoved`, and purges the entity.
    pub fn remove_entity(&mut self, id: i32, reason: EntityRemovalReason) -> bool {
        if self.section_update_lock.contains(&id) {
            return false;
        }
        let (first_time, effective_reason) = {
            let Some(entity) = self.get_mut(id) else {
                return false;
            };
            let e = entity.entity_mut();
            let first_time = e.removal_reason.is_none();
            if first_time {
                e.removal_reason = Some(reason);
            }
            (first_time, e.removal_reason.unwrap_or(reason))
        };

        if effective_reason.should_destroy() {
            self.stop_riding(id, false);
        }
        if effective_reason != EntityRemovalReason::UnloadedToChunk {
            self.stop_riding_passengers_of_removed(id);
        }

        if first_time {
            if let Some(entity) = self.get_mut(id) {
                entity.entity_mut().valid = false;
            }
            self.on_entity_removed(id, reason);
        }
        self.purge_removed();
        true
    }
}
