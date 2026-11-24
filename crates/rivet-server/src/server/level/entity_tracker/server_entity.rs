//! Port of `net.minecraft.server.level.ServerEntity` (Paper 26.2).
//!
//! Handles pairing packet construction (`addPairing` / `sendPairingData`),
//! unpairing (`removePairing`), and per-tick dirty-state / movement / passenger
//! / syncher / attribute / head-rotation / velocity emission (`sendChanges`).

use crate::server::level::entity_tracker::packets::{
    EntityTrackerPacket, EquipmentEntry, EquipmentSlot, MinecartStep,
};
use crate::server::level::entity_tracker::vec_delta_codec::VecDeltaCodec;
use rivet_protocol::game::{
    AttributeSnapshot, ClientboundAddEntityPacket, ClientboundRemoveEntitiesPacket,
    ClientboundSetEntityDataPacket, ClientboundSetPassengersPacket,
    ClientboundUpdateAttributesPacket,
};
use rivet_protocol::syncher::{DataValue, SynchedEntityData};
use rivet_registry::core::{BlockPos, Vec3};
use rivet_util::mth::{Uuid, pack_degrees, unpack_degrees};

/// `ServerEntity.TOLERANCE_LEVEL_ROTATION`.
pub const TOLERANCE_LEVEL_ROTATION: i32 = 1;
/// `ServerEntity.TOLERANCE_LEVEL_POSITION` (`7.62939453125E-6D` = 2^-17).
pub const TOLERANCE_LEVEL_POSITION: f64 = 7.629_394_531_25e-6;
/// `ServerEntity.FORCED_POS_UPDATE_PERIOD`.
pub const FORCED_POS_UPDATE_PERIOD: i32 = 60;
/// `ServerEntity.FORCED_TELEPORT_PERIOD`.
pub const FORCED_TELEPORT_PERIOD: i32 = 400;
/// `ServerEntity.sendChanges` hard-colliding teleport delay override (Paper moonrise).
pub const HARD_COLLIDING_TELEPORT_DELAY: i32 = 9999;

/// `ClientboundAddEntityPacket` constructor chosen by `Entity.getAddEntityPacket(ServerEntity)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddEntityPacketKind {
    /// `new ClientboundAddEntityPacket(entity, serverEntity, data)`: position from
    /// `entity.trackingPosition()`, rotations and movement from the `ServerEntity`'s
    /// last-sent values. Used by `Entity` (`data` 0) and the overrides in `Warden`,
    /// `Projectile`, `FishingHook` and `FallingBlockEntity`, which pass their own `data`.
    Tracked { data: i32 },
    /// `new ClientboundAddEntityPacket(entity, data, pos)`: integer block position,
    /// the entity's current rotations and `getDeltaMovement()`. Used by `ItemFrame`,
    /// `Painting` and `LeashFenceKnotEntity` with `pos = getPos()`.
    BlockPos { data: i32, pos: BlockPos },
}

/// Entity view passed to `ServerEntity` without re-owning entity storage.
pub trait TrackedEntityState {
    fn entity_id(&self) -> i32;
    fn uuid(&self) -> Uuid;
    fn entity_type_id(&self) -> i32;
    fn is_removed(&self) -> bool;
    fn is_server_player(&self) -> bool;
    fn is_living(&self) -> bool;
    fn is_abstract_arrow(&self) -> bool;
    fn is_item_frame(&self) -> bool;
    fn is_fall_flying(&self) -> bool;
    fn is_hard_colliding(&self) -> bool;
    fn requires_precise_position(&self) -> bool;
    fn uses_new_minecart_behavior(&self) -> bool;

    /// `Entity.position()`.
    fn position(&self) -> Vec3;
    /// `Entity.trackingPosition()`.
    fn tracking_position(&self) -> Vec3 {
        self.position()
    }
    /// `Entity.getDeltaMovement()`.
    fn delta_movement(&self) -> Vec3;
    /// `Entity.setDeltaMovement(Vec3)` (used when `PlayerVelocityEvent` modifies velocity).
    fn set_delta_movement(&mut self, movement: Vec3);

    fn y_rot(&self) -> f32;
    fn x_rot(&self) -> f32;
    fn y_head_rot(&self) -> f32;
    fn on_ground(&self) -> bool;
    /// `Entity.getAddEntityPacket(ServerEntity)`: which constructor the entity's
    /// override uses, with its `data` argument.
    fn add_entity_packet_kind(&self) -> AddEntityPacketKind {
        AddEntityPacketKind::Tracked { data: 0 }
    }

    /// `Entity.needsSync` flag.
    fn needs_sync(&self) -> bool;
    fn set_needs_sync(&mut self, value: bool);

    /// `Entity.syncPosition` flag (Paper).
    fn sync_position(&self) -> bool {
        false
    }
    fn set_sync_position(&mut self, _value: bool) {}

    /// `Entity.hurtMarked` flag.
    fn hurt_marked(&self) -> bool;
    fn set_hurt_marked(&mut self, value: bool);

    /// `Entity.updateDataBeforeSync()`.
    fn update_data_before_sync(&mut self) {}

    /// `Entity.getEntityData()`.
    fn entity_data(&self) -> &SynchedEntityData;
    /// Mutable access to `Entity.getEntityData()` for `pack_dirty()`.
    fn entity_data_mut(&mut self) -> &mut SynchedEntityData;

    /// `Entity.getPassengers()` as entity IDs in passenger order.
    fn passengers(&self) -> Vec<i32>;
    /// `Entity.isPassenger()` -> `Some((vehicle_id, vehicle_passengers))`.
    fn vehicle_passengers(&self) -> Option<(i32, Vec<i32>)>;

    /// `Leashable.isLeashed()` -> `Some(leash_holder_entity_id)`.
    fn leash_holder_id(&self) -> Option<i32> {
        None
    }

    /// `AbstractHurtingProjectile.accelerationPower`.
    fn hurting_projectile_acceleration_power(&self) -> Option<f64> {
        None
    }

    /// `LivingEntity.getAttributes().getSyncableAttributes()` (for pairing).
    fn syncable_attributes(&self, _for_self_player: bool) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// `LivingEntity.getAttributes().getAttributesToSync()` + clear (for dirty sync).
    fn take_dirty_attributes(&mut self) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// `LivingEntity.getItemBySlot(slot)` (for pairing).
    fn equipment_in_slot(&self, _slot: EquipmentSlot) -> Option<EquipmentEntry> {
        None
    }

    /// `LivingEntity.detectEquipmentUpdates()` — returns dirty equipment changes
    /// if any, and updates the entity's last-sent equipment snapshot.
    fn detect_equipment_updates(&mut self) -> Vec<EquipmentEntry> {
        Vec::new()
    }

    /// `NewMinecartBehavior.lerpSteps` — drains pending steps when non-empty.
    fn take_minecart_lerp_steps(&mut self) -> Vec<MinecartStep> {
        Vec::new()
    }

    /// For `ItemFrame` holding a `MapItem`, returns the map ID if present.
    fn item_frame_map_id(&self) -> Option<i32> {
        None
    }

    /// Ticks map tracking for `player_id` and returns true if a map update packet
    /// should be sent.
    fn tick_item_frame_map(&mut self, _map_id: i32, _player_id: i32) -> bool {
        false
    }
}

/// Callback surface mirroring `ServerEntity.Synchronizer` plus direct player
/// delivery and entity visibility hooks (`startSeenByPlayer` / `stopSeenByPlayer`).
pub trait ServerEntitySynchronizer {
    fn send_to_tracking_players(&mut self, packet: EntityTrackerPacket);
    fn send_to_tracking_players_and_self(&mut self, packet: EntityTrackerPacket);
    fn send_to_tracking_players_filtered(
        &mut self,
        packet: EntityTrackerPacket,
        predicate: &dyn Fn(i32) -> bool,
    );
    fn send_to_player(&mut self, player_id: i32, packet: EntityTrackerPacket);
    fn start_seen_by_player(&mut self, entity_id: i32, player_id: i32);
    fn stop_seen_by_player(&mut self, entity_id: i32, player_id: i32);

    /// CraftBukkit `PlayerVelocityEvent` hook for `hurtMarked` players.
    ///
    /// Returns `Some(velocity)` to proceed (potentially modified), or `None` if
    /// the event was cancelled.
    fn on_player_velocity_event(&mut self, _player_id: i32, velocity: Vec3) -> Option<Vec3> {
        Some(velocity)
    }
}

/// Port of `net.minecraft.server.level.ServerEntity`.
#[derive(Debug, Clone)]
pub struct ServerEntity {
    update_interval: i32,
    track_delta: bool,
    position_codec: VecDeltaCodec,
    last_sent_y_rot: i8,
    last_sent_x_rot: i8,
    last_sent_y_head_rot: i8,
    last_sent_movement: Vec3,
    tick_count: i32,
    teleport_delay: i32,
    last_passengers: Vec<i32>,
    was_riding: bool,
    was_on_ground: bool,
    tracked_data_values: Option<Vec<DataValue>>,
    force_state_resync: bool,
}

impl ServerEntity {
    /// `new ServerEntity(ServerLevel, Entity, int updateInterval, boolean trackDelta, Synchronizer, Set<ServerPlayerConnection>)`.
    pub fn new(entity: &dyn TrackedEntityState, update_interval: i32, track_delta: bool) -> Self {
        let mut position_codec = VecDeltaCodec::new();
        position_codec.set_base(entity.tracking_position());
        Self {
            update_interval: update_interval.max(1),
            track_delta,
            position_codec,
            last_sent_y_rot: pack_degrees(entity.y_rot()),
            last_sent_x_rot: pack_degrees(entity.x_rot()),
            last_sent_y_head_rot: pack_degrees(entity.y_head_rot()),
            last_sent_movement: entity.delta_movement(),
            tick_count: 0,
            teleport_delay: 0,
            last_passengers: Vec::new(),
            was_riding: false,
            was_on_ground: entity.on_ground(),
            tracked_data_values: entity.entity_data().get_non_default_values(),
            force_state_resync: false,
        }
    }

    /// Paper `ServerEntity.onPlayerAdd()` — forces an absolute state resync on
    /// the next `sendChanges()` call.
    pub fn on_player_add(&mut self) {
        self.force_state_resync = true;
    }

    pub fn tick_count(&self) -> i32 {
        self.tick_count
    }

    pub fn teleport_delay(&self) -> i32 {
        self.teleport_delay
    }

    pub fn position_base(&self) -> Vec3 {
        self.position_codec.get_base()
    }

    /// `ServerEntity.getLastSentMovement()`.
    pub fn last_sent_movement(&self) -> Vec3 {
        self.last_sent_movement
    }

    /// `ServerEntity.getLastSentXRot()`.
    pub fn last_sent_x_rot(&self) -> f32 {
        unpack_degrees(self.last_sent_x_rot)
    }

    /// `ServerEntity.getLastSentYRot()`.
    pub fn last_sent_y_rot(&self) -> f32 {
        unpack_degrees(self.last_sent_y_rot)
    }

    /// `ServerEntity.getLastSentYHeadRot()`.
    pub fn last_sent_y_head_rot(&self) -> f32 {
        unpack_degrees(self.last_sent_y_head_rot)
    }

    pub fn last_passengers(&self) -> &[i32] {
        &self.last_passengers
    }

    pub fn tracked_data_values(&self) -> Option<&[DataValue]> {
        self.tracked_data_values.as_deref()
    }

    pub fn force_state_resync(&self) -> bool {
        self.force_state_resync
    }

    /// `ServerEntity.sendChanges()`.
    pub fn send_changes(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        tracked_players: &[i32],
        item_frame_cursor_update_interval: i32,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) {
        // Paper moonrise: hard-colliding entities force teleport sync.
        if entity.is_hard_colliding() {
            self.teleport_delay = HARD_COLLIDING_TELEPORT_DELAY;
        }

        entity.update_data_before_sync();

        let entity_id = entity.entity_id();

        // 1. Passenger change detection (runs every tick, before updateInterval gate).
        let passengers = entity.passengers();
        if passengers != self.last_passengers {
            let packet = EntityTrackerPacket::SetPassengers(ClientboundSetPassengersPacket::new(
                entity_id,
                passengers.clone(),
            ));
            let last_passengers = &self.last_passengers;
            synchronizer.send_to_tracking_players_filtered(packet.clone(), &|player_id| {
                passengers.contains(&player_id) == last_passengers.contains(&player_id)
            });
            // Paper: allow riding players — send updated passengers to the ridden player itself.
            if entity.is_server_player() {
                synchronizer.send_to_player(entity_id, packet);
            }
            self.last_passengers = passengers;
        }

        // 2. ItemFrame map cursor update + dirty entity data when tracked by any player.
        if !tracked_players.is_empty() && entity.is_item_frame() {
            if item_frame_cursor_update_interval > 0
                && self.tick_count % item_frame_cursor_update_interval == 0
                && let Some(map_id) = entity.item_frame_map_id()
            {
                for &player_id in tracked_players {
                    if entity.tick_item_frame_map(map_id, player_id) {
                        synchronizer.send_to_player(
                            player_id,
                            EntityTrackerPacket::MapItemUpdate {
                                map_id,
                                target_player_id: player_id,
                            },
                        );
                    }
                }
            }
            self.send_dirty_entity_data(entity, synchronizer);
        }

        // Paper: align tickCount to the next updateInterval boundary if syncPosition is set.
        if entity.sync_position() {
            self.tick_count = (self.tick_count / self.update_interval)
                .wrapping_mul(self.update_interval)
                .wrapping_add(self.update_interval);
            entity.set_sync_position(false);
        }

        // 3. Main movement / rotation / metadata / head-rotation sync gate.
        if self.force_state_resync
            || self.tick_count % self.update_interval == 0
            || entity.needs_sync()
            || entity.entity_data().is_dirty()
        {
            let y_rotn = pack_degrees(entity.y_rot());
            let x_rotn = pack_degrees(entity.x_rot());
            let should_send_rotation = (i32::from(y_rotn) - i32::from(self.last_sent_y_rot)).abs()
                >= TOLERANCE_LEVEL_ROTATION
                || (i32::from(x_rotn) - i32::from(self.last_sent_x_rot)).abs()
                    >= TOLERANCE_LEVEL_ROTATION;

            if entity.vehicle_passengers().is_some() {
                if should_send_rotation {
                    synchronizer.send_to_tracking_players(EntityTrackerPacket::MoveEntityRot {
                        entity_id,
                        y_rot: y_rotn,
                        x_rot: x_rotn,
                        on_ground: entity.on_ground(),
                    });
                    self.last_sent_y_rot = y_rotn;
                    self.last_sent_x_rot = x_rotn;
                }
                self.position_codec.set_base(entity.tracking_position());
                self.send_dirty_entity_data(entity, synchronizer);
                self.was_riding = true;
            } else if entity.uses_new_minecart_behavior() {
                self.handle_minecart_pos_rot(
                    entity,
                    synchronizer,
                    y_rotn,
                    x_rotn,
                    should_send_rotation,
                );
            } else {
                self.teleport_delay = self.teleport_delay.wrapping_add(1);
                let current_position = entity.tracking_position();
                let position_changed = vec3_length_sqr(self.position_codec.delta(current_position))
                    >= TOLERANCE_LEVEL_POSITION;
                let mut packet: Option<EntityTrackerPacket> = None;
                let pos = position_changed || self.tick_count % FORCED_POS_UPDATE_PERIOD == 0;
                let mut sent_position = false;
                let mut sent_rotation = false;
                let xa = self.position_codec.encode_x(current_position);
                let ya = self.position_codec.encode_y(current_position);
                let za = self.position_codec.encode_z(current_position);
                let delta_too_big = xa < i64::from(i16::MIN)
                    || xa > i64::from(i16::MAX)
                    || ya < i64::from(i16::MIN)
                    || ya > i64::from(i16::MAX)
                    || za < i64::from(i16::MIN)
                    || za > i64::from(i16::MAX);

                if self.force_state_resync
                    || entity.requires_precise_position()
                    || delta_too_big
                    || self.teleport_delay > FORCED_TELEPORT_PERIOD
                    || self.was_riding
                    || self.was_on_ground != entity.on_ground()
                {
                    self.was_on_ground = entity.on_ground();
                    self.teleport_delay = 0;
                    packet = Some(EntityTrackerPacket::EntityPositionSync {
                        entity_id,
                        position: entity.position(),
                        delta_movement: entity.delta_movement(),
                        y_rot: entity.y_rot(),
                        x_rot: entity.x_rot(),
                        on_ground: entity.on_ground(),
                    });
                    sent_position = true;
                    sent_rotation = true;
                } else if (!pos || !should_send_rotation) && !entity.is_abstract_arrow() {
                    if pos {
                        packet = Some(EntityTrackerPacket::MoveEntityPos {
                            entity_id,
                            xa: xa as i16,
                            ya: ya as i16,
                            za: za as i16,
                            on_ground: entity.on_ground(),
                        });
                        sent_position = true;
                    } else if should_send_rotation {
                        packet = Some(EntityTrackerPacket::MoveEntityRot {
                            entity_id,
                            y_rot: y_rotn,
                            x_rot: x_rotn,
                            on_ground: entity.on_ground(),
                        });
                        sent_rotation = true;
                    }
                } else {
                    packet = Some(EntityTrackerPacket::MoveEntityPosRot {
                        entity_id,
                        xa: xa as i16,
                        ya: ya as i16,
                        za: za as i16,
                        y_rot: y_rotn,
                        x_rot: x_rotn,
                        on_ground: entity.on_ground(),
                    });
                    sent_position = true;
                    sent_rotation = true;
                }

                // Velocity delta check (runs BEFORE sending the movement packet).
                if entity.needs_sync() || self.track_delta || entity.is_fall_flying() {
                    let movement = entity.delta_movement();
                    let diff = vec3_distance_to_sqr(movement, self.last_sent_movement);
                    if diff > 1.0e-7 || (diff > 0.0 && vec3_length_sqr(movement) == 0.0) {
                        self.last_sent_movement = movement;
                        if let Some(acceleration_power) =
                            entity.hurting_projectile_acceleration_power()
                        {
                            synchronizer.send_to_tracking_players(EntityTrackerPacket::Bundle(
                                vec![
                                    EntityTrackerPacket::SetEntityMotion {
                                        entity_id,
                                        movement: self.last_sent_movement,
                                    },
                                    EntityTrackerPacket::ProjectilePower {
                                        entity_id,
                                        acceleration_power,
                                    },
                                ],
                            ));
                        } else {
                            synchronizer.send_to_tracking_players(
                                EntityTrackerPacket::SetEntityMotion {
                                    entity_id,
                                    movement: self.last_sent_movement,
                                },
                            );
                        }
                    }
                }

                if let Some(pkt) = packet {
                    synchronizer.send_to_tracking_players(pkt);
                }

                self.send_dirty_entity_data(entity, synchronizer);
                if sent_position {
                    self.position_codec.set_base(current_position);
                }
                if sent_rotation {
                    self.last_sent_y_rot = y_rotn;
                    self.last_sent_x_rot = x_rotn;
                }
                self.was_riding = false;
            }

            // Head rotation check.
            let y_head_rot = pack_degrees(entity.y_head_rot());
            if (i32::from(y_head_rot) - i32::from(self.last_sent_y_head_rot)).abs()
                >= TOLERANCE_LEVEL_ROTATION
            {
                synchronizer.send_to_tracking_players(EntityTrackerPacket::RotateHead {
                    entity_id,
                    y_head_rot,
                });
                self.last_sent_y_head_rot = y_head_rot;
            }

            entity.set_needs_sync(false);
            self.force_state_resync = false;
        }

        self.tick_count = self.tick_count.wrapping_add(1);

        // 4. `hurtMarked` velocity emission (with CraftBukkit `PlayerVelocityEvent`).
        if entity.hurt_marked() {
            if entity.is_server_player() {
                let current_vel = entity.delta_movement();
                match synchronizer.on_player_velocity_event(entity_id, current_vel) {
                    None => {
                        // Cancelled: Paper returns without clearing hurtMarked or sending motion.
                        return;
                    }
                    Some(modified_vel) => {
                        if !bukkit_vector_equals(current_vel, modified_vel) {
                            entity.set_delta_movement(modified_vel);
                        }
                    }
                }
            }
            entity.set_hurt_marked(false);
            synchronizer.send_to_tracking_players_and_self(EntityTrackerPacket::SetEntityMotion {
                entity_id,
                movement: entity.delta_movement(),
            });
        }
    }

    /// `ServerEntity.handleMinecartPosRot`.
    fn handle_minecart_pos_rot(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        synchronizer: &mut dyn ServerEntitySynchronizer,
        y_rotn: i8,
        x_rotn: i8,
        should_send_rotation: bool,
    ) {
        self.send_dirty_entity_data(entity, synchronizer);
        let entity_id = entity.entity_id();
        let lerp_steps = entity.take_minecart_lerp_steps();
        if lerp_steps.is_empty() {
            let diff = vec3_distance_to_sqr(entity.delta_movement(), self.last_sent_movement);
            let current_position = entity.tracking_position();
            let position_changed = vec3_length_sqr(self.position_codec.delta(current_position))
                >= TOLERANCE_LEVEL_POSITION;
            let should_send_position = position_changed || self.tick_count % 60 == 0;
            if should_send_position || should_send_rotation || diff > 1.0e-7 {
                synchronizer.send_to_tracking_players(EntityTrackerPacket::MoveMinecart {
                    entity_id,
                    steps: vec![MinecartStep {
                        position: entity.position(),
                        movement: entity.delta_movement(),
                        y_rot: entity.y_rot(),
                        x_rot: entity.x_rot(),
                        weight: 1.0,
                    }],
                });
            }
        } else {
            synchronizer.send_to_tracking_players(EntityTrackerPacket::MoveMinecart {
                entity_id,
                steps: lerp_steps,
            });
        }
        self.last_sent_y_rot = y_rotn;
        self.last_sent_x_rot = x_rotn;
        self.position_codec.set_base(entity.position());
    }

    /// `ServerEntity.removePairing(ServerPlayer)`.
    pub fn remove_pairing(
        &self,
        entity_id: i32,
        player_id: i32,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) {
        synchronizer.stop_seen_by_player(entity_id, player_id);
        synchronizer.send_to_player(
            player_id,
            EntityTrackerPacket::RemoveEntities(ClientboundRemoveEntitiesPacket::new(vec![
                entity_id,
            ])),
        );
    }

    /// `ServerEntity.addPairing(ServerPlayer)`.
    pub fn add_pairing(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        player_id: i32,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) {
        let mut packets = Vec::new();
        self.send_pairing_data(entity, player_id, &mut packets, synchronizer);
        synchronizer.send_to_player(player_id, EntityTrackerPacket::Bundle(packets));
        synchronizer.start_seen_by_player(entity.entity_id(), player_id);
    }

    /// `ServerEntity.sendPairingData(ServerPlayer, Consumer<Packet>)`.
    pub fn send_pairing_data(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        player_id: i32,
        packets: &mut Vec<EntityTrackerPacket>,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) {
        entity.update_data_before_sync();
        if entity.is_removed() {
            return;
        }

        let entity_id = entity.entity_id();

        // 1. AddEntity (`entity.getAddEntityPacket(this)`)
        packets.push(EntityTrackerPacket::AddEntity(
            self.get_add_entity_packet(entity),
        ));

        // 2. Non-default SynchedEntityData values
        if let Some(values) = &self.tracked_data_values {
            packets.push(EntityTrackerPacket::SetEntityData(
                ClientboundSetEntityDataPacket::new(entity_id, values.clone()),
            ));
        }

        // 3. LivingEntity syncable attributes
        if entity.is_living() {
            let attributes = entity.syncable_attributes(entity_id == player_id);
            if !attributes.is_empty() {
                packets.push(EntityTrackerPacket::UpdateAttributes(
                    ClientboundUpdateAttributesPacket::new(entity_id, attributes),
                ));
            }
        }

        // 4. LivingEntity non-empty equipment (in EquipmentSlot::VALUES order, sanitize = true)
        //    followed by `detectEquipmentUpdates()` (CraftBukkit SPIGOT-3789).
        if entity.is_living() {
            let mut slots = Vec::new();
            for slot in EquipmentSlot::VALUES {
                if let Some(entry) = entity.equipment_in_slot(slot)
                    && !entry.is_empty()
                {
                    slots.push(entry);
                }
            }
            if !slots.is_empty() {
                packets.push(EntityTrackerPacket::SetEquipment {
                    entity_id,
                    slots,
                    sanitize: true,
                });
            }
            let dirty_equipment = entity.detect_equipment_updates();
            if !dirty_equipment.is_empty() {
                // `LivingEntity.handleEquipmentChanges`: Paper - data sanitization.
                synchronizer.send_to_tracking_players(EntityTrackerPacket::SetEquipment {
                    entity_id,
                    slots: dirty_equipment,
                    sanitize: true,
                });
            }
        }

        // 5. Passengers of this entity
        let passengers = entity.passengers();
        if !passengers.is_empty() {
            packets.push(EntityTrackerPacket::SetPassengers(
                ClientboundSetPassengersPacket::new(entity_id, passengers),
            ));
        }

        // 6. Vehicle's passengers if this entity is riding a vehicle
        if let Some((vehicle_id, vehicle_passengers)) = entity.vehicle_passengers() {
            packets.push(EntityTrackerPacket::SetPassengers(
                ClientboundSetPassengersPacket::new(vehicle_id, vehicle_passengers),
            ));
        }

        // 7. Leash link if leashed
        if let Some(holder_id) = entity.leash_holder_id() {
            packets.push(EntityTrackerPacket::SetEntityLink {
                source_id: entity_id,
                dest_id: holder_id,
            });
        }
    }

    /// `Entity.getAddEntityPacket(ServerEntity)` with the constructor the entity's
    /// override selects.
    fn get_add_entity_packet(&self, entity: &dyn TrackedEntityState) -> ClientboundAddEntityPacket {
        match entity.add_entity_packet_kind() {
            AddEntityPacketKind::Tracked { data } => {
                // Paper - fix entity tracker desync: `entity.trackingPosition()`.
                let pos = entity.tracking_position();
                ClientboundAddEntityPacket {
                    id: entity.entity_id(),
                    uuid: entity.uuid(),
                    entity_type_id: entity.entity_type_id(),
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    movement: self.last_sent_movement(),
                    x_rot: pack_degrees(self.last_sent_x_rot()),
                    y_rot: pack_degrees(self.last_sent_y_rot()),
                    y_head_rot: pack_degrees(self.last_sent_y_head_rot()),
                    data,
                }
            }
            AddEntityPacketKind::BlockPos { data, pos } => ClientboundAddEntityPacket {
                id: entity.entity_id(),
                uuid: entity.uuid(),
                entity_type_id: entity.entity_type_id(),
                x: f64::from(pos.get_x()),
                y: f64::from(pos.get_y()),
                z: f64::from(pos.get_z()),
                movement: entity.delta_movement(),
                x_rot: pack_degrees(entity.x_rot()),
                y_rot: pack_degrees(entity.y_rot()),
                y_head_rot: pack_degrees(entity.y_head_rot()),
                data,
            },
        }
    }

    /// `ServerEntity.sendDirtyEntityData()`.
    pub fn send_dirty_entity_data(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) {
        let entity_id = entity.entity_id();
        if let Some(packed_values) = entity.entity_data_mut().pack_dirty() {
            self.tracked_data_values = entity.entity_data().get_non_default_values();
            synchronizer.send_to_tracking_players_and_self(EntityTrackerPacket::SetEntityData(
                ClientboundSetEntityDataPacket::new(entity_id, packed_values),
            ));
        }

        if entity.is_living() {
            let dirty_attributes = entity.take_dirty_attributes();
            if !dirty_attributes.is_empty() {
                synchronizer.send_to_tracking_players_and_self(
                    EntityTrackerPacket::UpdateAttributes(ClientboundUpdateAttributesPacket::new(
                        entity_id,
                        dirty_attributes,
                    )),
                );
            }
        }
    }

    /// Emits dirty equipment updates for a `LivingEntity` to tracking players
    /// (`LivingEntity::detectEquipmentUpdates` -> `sendToTrackingPlayers(SetEquipment)`).
    pub fn flush_equipment_updates(
        &self,
        entity: &mut dyn TrackedEntityState,
        synchronizer: &mut dyn ServerEntitySynchronizer,
    ) -> bool {
        if !entity.is_living() {
            return false;
        }
        let dirty_equipment = entity.detect_equipment_updates();
        if dirty_equipment.is_empty() {
            return false;
        }
        synchronizer.send_to_tracking_players(EntityTrackerPacket::SetEquipment {
            entity_id: entity.entity_id(),
            slots: dirty_equipment,
            // `LivingEntity.handleEquipmentChanges`: Paper - data sanitization.
            sanitize: true,
        });
        true
    }
}

/// Bukkit `Vector.equals`: per-component `Math.abs(a - b) < epsilon` with
/// `Vector.epsilon = 0.000001`.
fn bukkit_vector_equals(a: Vec3, b: Vec3) -> bool {
    const EPSILON: f64 = 0.000001;
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON && (a.z - b.z).abs() < EPSILON
}

#[inline]
fn vec3_length_sqr(v: Vec3) -> f64 {
    v.x * v.x + v.y * v.y + v.z * v.z
}

#[inline]
fn vec3_distance_to_sqr(a: Vec3, b: Vec3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dx * dx + dy * dy + dz * dz
}
