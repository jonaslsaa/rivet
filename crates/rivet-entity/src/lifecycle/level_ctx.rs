//! Single-threaded, tick-thread-owned `LevelCtx` using take-tick-putback without
//! `Arc<RwLock>` game state (per `OWNERSHIP.md` and Paper 26.2 `ServerLevel.java`).

use super::dimensions::Pose;
use super::entity::{BOARDING_COOLDOWN, Entity, removal_reason_should_destroy};
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

    /// Dispatches `inactiveTick()`.
    pub fn inactive_tick(&mut self, ctx: &mut LevelCtx) {
        self.entity_mut().entity_inactive_tick(ctx);
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
    pub nearest_player_pos: Option<Vec3>,
    /// Paper Moonrise `canRemoveEntity` guard: IDs locked during section status updates.
    pub section_update_lock: HashSet<i32>,
    /// Test/plugin hook: cancels non-forced mount attempts (`EntityMountEvent`).
    pub cancel_mount: bool,
    /// Test/plugin hook: cancels non-suppressed dismount attempts (`EntityDismountEvent`).
    pub cancel_dismount: bool,
    pub events: Vec<LifecycleEvent>,
    pending_removals: Vec<(i32, EntityRemovalReason)>,
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
            nearest_player_pos: None,
            section_update_lock: HashSet::new(),
            cancel_mount: false,
            cancel_dismount: false,
            events: Vec::new(),
            pending_removals: Vec::new(),
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
        self.flush_pending_removals();

        for passenger_id in passengers {
            self.tick_passenger(id, passenger_id, is_active);
        }
    }

    /// Runs `ServerLevel#tickPassenger(Entity vehicle, Entity passenger, boolean isActive)`
    /// using take-tick-putback, detaching invalid/removed passengers and recursing
    /// into nested passengers.
    pub fn tick_passenger(&mut self, vehicle_id: i32, passenger_id: i32, is_active: bool) {
        let should_detach = match self.get(passenger_id) {
            None => true,
            Some(p) => p.entity().is_removed() || p.entity().vehicle != Some(vehicle_id),
        };
        if should_detach {
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
        self.flush_pending_removals();

        for sub_id in sub_passengers {
            self.tick_passenger(passenger_id, sub_id, is_active);
        }
    }

    /// Ticks all non-passenger, non-removed entities in `IndexMap` order
    /// (matching Paper `ServerLevel` entity tick loop).
    pub fn tick_all(&mut self, is_active: bool) {
        self.game_time = self.game_time.wrapping_add(1);
        let ids: Vec<i32> = self.entities.keys().copied().collect();
        for id in ids {
            let should_tick = self
                .get(id)
                .is_some_and(|e| !e.entity().is_removed() && !e.entity().is_passenger());
            if should_tick {
                self.tick_non_passenger(id, is_active);
            }
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

    /// Returns whether entity `id` is a player (`EntityTypeMeta::is_player`).
    #[must_use]
    pub fn is_entity_player(&self, id: i32) -> bool {
        self.get(id)
            .is_some_and(|e| e.entity().entity_type.is_player)
    }

    /// Paper `Mob#hasControllingPassenger()` check used by `Mob#updateControlFlags()`.
    #[must_use]
    pub fn has_mob_controlling_passenger(&self, vehicle_id: i32) -> bool {
        let Some(first_passenger) = self
            .get(vehicle_id)
            .and_then(|e| e.entity().first_passenger())
        else {
            return false;
        };
        self.is_entity_player(first_passenger)
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
    /// - Rejects if `cancel_mount` is set (CraftBukkit `EntityMountEvent`).
    /// - Stops existing riding before attaching to the new vehicle.
    /// - Resets passenger pose to `Pose::Standing` and attaches with player-priority order.
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
        let (already_same, can_ride, has_old_vehicle, passenger_is_player) = {
            let Some(p) = self.get(passenger_id) else {
                return false;
            };
            let pe = p.entity();
            (
                pe.vehicle == Some(vehicle_id),
                pe.can_ride(),
                pe.is_passenger(),
                pe.entity_type.is_player,
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

        if emit_event && self.cancel_mount {
            return false;
        }

        if has_old_vehicle && !self.stop_riding(passenger_id, false) {
            return false;
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
        true
    }

    /// Paper `Entity#stopRiding(boolean suppressCancellation)` / `removeVehicle`.
    ///
    /// If dismount is cancelled and `!suppress_cancellation`, restores `passenger.vehicle`
    /// and returns `false` (exact Paper CraftBukkit cancellation recovery).
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

        if self.cancel_dismount && !suppress_cancellation {
            // Restore vehicle on cancellation
            if let Some(p) = self.get_mut(passenger_id) {
                p.entity_mut().vehicle = Some(vehicle_id);
            }
            return false;
        }

        if let Some(v) = self.get_mut(vehicle_id) {
            v.entity_mut().passengers.retain(|&pid| pid != passenger_id);
        }

        if let Some(p) = self.get_mut(passenger_id) {
            p.entity_mut().boarding_cooldown = BOARDING_COOLDOWN;
        }

        let should_emit = removal_reason.is_none_or(removal_reason_should_destroy);
        if should_emit {
            self.record_event(LifecycleEvent::Dismount {
                passenger_id,
                vehicle_id,
            });
        }
        true
    }

    /// Paper `Entity#ejectPassengers()` — iterates passengers in reverse order.
    pub fn eject_passengers(&mut self, vehicle_id: i32) {
        let passengers = match self.get(vehicle_id) {
            Some(v) => v.entity().passengers.clone(),
            None => return,
        };
        for passenger_id in passengers.into_iter().rev() {
            self.stop_riding(passenger_id, false);
        }
    }

    pub(crate) fn queue_pending_removal(&mut self, id: i32, reason: EntityRemovalReason) {
        self.pending_removals.push((id, reason));
    }

    pub(crate) fn flush_pending_removals(&mut self) {
        let pending = std::mem::take(&mut self.pending_removals);
        for (id, reason) in pending {
            if reason != EntityRemovalReason::UnloadedToChunk {
                self.eject_passengers(id);
            }
            self.record_event(LifecycleEvent::EntityRemoved { id, reason });
        }
    }

    /// Paper `Entity#stopRiding(boolean suppressCancellation)` for a currently-taken passenger.
    ///
    /// Respects `cancel_dismount` (restoring `passenger.vehicle` when cancelled) and emits
    /// `LifecycleEvent::Dismount` on success, matching `stop_riding`.
    pub(crate) fn stop_riding_taken(
        &mut self,
        passenger: &mut Entity,
        suppress_cancellation: bool,
    ) -> bool {
        let Some(vehicle_id) = passenger.vehicle else {
            return false;
        };
        let passenger_id = passenger.id;
        let removal_reason = passenger.removal_reason;

        passenger.vehicle = None;

        if self.cancel_dismount && !suppress_cancellation {
            passenger.vehicle = Some(vehicle_id);
            return false;
        }

        if let Some(v) = self.get_mut(vehicle_id) {
            v.entity_mut().passengers.retain(|&pid| pid != passenger_id);
        }

        passenger.boarding_cooldown = BOARDING_COOLDOWN;

        let should_emit = removal_reason.is_none_or(removal_reason_should_destroy);
        if should_emit {
            self.record_event(LifecycleEvent::Dismount {
                passenger_id,
                vehicle_id,
            });
        }
        true
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

    /// Paper `Entity#setRemoved(RemovalReason)`.
    ///
    /// - Respects Moonrise `section_update_lock` (`canRemoveEntity` guard).
    /// - Preserves the first `removal_reason`.
    /// - Calls `stop_riding` when `reason.should_destroy()`.
    /// - Calls `eject_passengers` when `reason != UnloadedToChunk`.
    /// - Marks `valid = false` and emits `LifecycleEvent::EntityRemoved`.
    pub fn remove_entity(&mut self, id: i32, reason: EntityRemovalReason) -> bool {
        if self.section_update_lock.contains(&id) {
            return false;
        }
        let effective_reason = {
            let Some(entity) = self.get_mut(id) else {
                return false;
            };
            let e = entity.entity_mut();
            if e.removal_reason.is_none() {
                e.removal_reason = Some(reason);
            }
            e.valid = false;
            e.removal_reason.unwrap_or(reason)
        };

        if removal_reason_should_destroy(effective_reason) {
            self.stop_riding(id, false);
        }

        if effective_reason != EntityRemovalReason::UnloadedToChunk {
            self.eject_passengers(id);
        }

        self.record_event(LifecycleEvent::EntityRemoved {
            id,
            reason: effective_reason,
        });
        true
    }
}
