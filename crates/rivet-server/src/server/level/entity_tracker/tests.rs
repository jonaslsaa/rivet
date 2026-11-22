use super::*;
use rivet_protocol::game::{
    AttributeModifier, AttributeSnapshot, ClientboundAddEntityPacket,
    ClientboundRemoveEntitiesPacket, ClientboundSetEntityDataPacket,
    ClientboundSetPassengersPacket, ClientboundUpdateAttributesPacket, Operation,
};
use rivet_protocol::syncher::{
    DataValue, EntityDataAccessor, SerializedValue, SerializerId, SynchedEntityData,
    SynchedEntityDataBuilder, serializers,
};
use rivet_registry::Identifier;
use rivet_registry::core::Vec3;
use rivet_registry::holder::Holder;
use rivet_registry::registries::Attribute;
use rivet_util::mth::Uuid;
use std::collections::HashMap;

use crate::server::level::full_chunk_status::FullChunkStatus;

fn data_flags_accessor() -> EntityDataAccessor<i8> {
    EntityDataAccessor::new(0, serializers::BYTE)
}

fn data_health_accessor() -> EntityDataAccessor<f32> {
    EntityDataAccessor::new(1, serializers::FLOAT)
}

fn make_syncher(flags: i8, health: f32) -> SynchedEntityData {
    let mut builder = SynchedEntityDataBuilder::new(2);
    builder.define(data_flags_accessor(), 0_i8);
    builder.define(data_health_accessor(), 20.0_f32);
    let mut data = builder.build();
    if flags != 0 {
        data.set(data_flags_accessor(), flags);
    }
    if health != 20.0 {
        data.set(data_health_accessor(), health);
    }
    data
}

struct MockEntity {
    entity_id: i32,
    uuid: Uuid,
    entity_type_id: i32,
    removed: bool,
    server_player: bool,
    living: bool,
    abstract_arrow: bool,
    item_frame: bool,
    fall_flying: bool,
    hard_colliding: bool,
    precise_position: bool,
    new_minecart: bool,
    position: Vec3,
    delta_movement: Vec3,
    y_rot: f32,
    x_rot: f32,
    y_head_rot: f32,
    on_ground: bool,
    needs_sync: bool,
    sync_position: bool,
    hurt_marked: bool,
    entity_data: SynchedEntityData,
    passengers: Vec<i32>,
    vehicle_passengers: Option<(i32, Vec<i32>)>,
    leash_holder_id: Option<i32>,
    projectile_power: Option<f64>,
    syncable_attrs: Vec<AttributeSnapshot>,
    dirty_attrs: Vec<AttributeSnapshot>,
    equipment: HashMap<EquipmentSlot, EquipmentEntry>,
    dirty_equipment: Vec<EquipmentEntry>,
    minecart_steps: Vec<MinecartStep>,
    map_id: Option<i32>,
}

impl MockEntity {
    fn new(entity_id: i32) -> Self {
        Self {
            entity_id,
            uuid: Uuid {
                most: 0,
                least: entity_id as i64 + 1,
            },
            entity_type_id: 42,
            removed: false,
            server_player: false,
            living: true,
            abstract_arrow: false,
            item_frame: false,
            fall_flying: false,
            hard_colliding: false,
            precise_position: false,
            new_minecart: false,
            position: Vec3::new(10.0, 64.0, 20.0),
            delta_movement: Vec3::new(0.0, 0.0, 0.0),
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            on_ground: true,
            needs_sync: false,
            sync_position: false,
            hurt_marked: false,
            entity_data: make_syncher(0, 20.0),
            passengers: Vec::new(),
            vehicle_passengers: None,
            leash_holder_id: None,
            projectile_power: None,
            syncable_attrs: Vec::new(),
            dirty_attrs: Vec::new(),
            equipment: HashMap::new(),
            dirty_equipment: Vec::new(),
            minecart_steps: Vec::new(),
            map_id: None,
        }
    }
}

impl TrackedEntityState for MockEntity {
    fn entity_id(&self) -> i32 {
        self.entity_id
    }
    fn uuid(&self) -> Uuid {
        self.uuid
    }
    fn entity_type_id(&self) -> i32 {
        self.entity_type_id
    }
    fn is_removed(&self) -> bool {
        self.removed
    }
    fn is_server_player(&self) -> bool {
        self.server_player
    }
    fn is_living(&self) -> bool {
        self.living
    }
    fn is_abstract_arrow(&self) -> bool {
        self.abstract_arrow
    }
    fn is_item_frame(&self) -> bool {
        self.item_frame
    }
    fn is_fall_flying(&self) -> bool {
        self.fall_flying
    }
    fn is_hard_colliding(&self) -> bool {
        self.hard_colliding
    }
    fn requires_precise_position(&self) -> bool {
        self.precise_position
    }
    fn uses_new_minecart_behavior(&self) -> bool {
        self.new_minecart
    }
    fn position(&self) -> Vec3 {
        self.position
    }
    fn delta_movement(&self) -> Vec3 {
        self.delta_movement
    }
    fn set_delta_movement(&mut self, movement: Vec3) {
        self.delta_movement = movement;
    }
    fn y_rot(&self) -> f32 {
        self.y_rot
    }
    fn x_rot(&self) -> f32 {
        self.x_rot
    }
    fn y_head_rot(&self) -> f32 {
        self.y_head_rot
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn needs_sync(&self) -> bool {
        self.needs_sync
    }
    fn set_needs_sync(&mut self, value: bool) {
        self.needs_sync = value;
    }
    fn sync_position(&self) -> bool {
        self.sync_position
    }
    fn set_sync_position(&mut self, value: bool) {
        self.sync_position = value;
    }
    fn hurt_marked(&self) -> bool {
        self.hurt_marked
    }
    fn set_hurt_marked(&mut self, value: bool) {
        self.hurt_marked = value;
    }
    fn entity_data(&self) -> &SynchedEntityData {
        &self.entity_data
    }
    fn entity_data_mut(&mut self) -> &mut SynchedEntityData {
        &mut self.entity_data
    }
    fn passengers(&self) -> Vec<i32> {
        self.passengers.clone()
    }
    fn vehicle_passengers(&self) -> Option<(i32, Vec<i32>)> {
        self.vehicle_passengers.clone()
    }
    fn leash_holder_id(&self) -> Option<i32> {
        self.leash_holder_id
    }
    fn hurting_projectile_acceleration_power(&self) -> Option<f64> {
        self.projectile_power
    }
    fn syncable_attributes(&self, _for_self_player: bool) -> Vec<AttributeSnapshot> {
        self.syncable_attrs.clone()
    }
    fn take_dirty_attributes(&mut self) -> Vec<AttributeSnapshot> {
        std::mem::take(&mut self.dirty_attrs)
    }
    fn equipment_in_slot(&self, slot: EquipmentSlot) -> Option<EquipmentEntry> {
        self.equipment.get(&slot).cloned()
    }
    fn detect_equipment_updates(&mut self) -> Vec<EquipmentEntry> {
        std::mem::take(&mut self.dirty_equipment)
    }
    fn take_minecart_lerp_steps(&mut self) -> Vec<MinecartStep> {
        std::mem::take(&mut self.minecart_steps)
    }
    fn item_frame_map_id(&self) -> Option<i32> {
        self.map_id
    }
    fn tick_item_frame_map(&mut self, _map_id: i32, _player_id: i32) -> bool {
        self.map_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum HostEvent {
    Packet(i32, EntityTrackerPacket),
    StartSeen(i32, i32),
    StopSeen(i32, i32),
    TrackEvent(i32, i32),
    UntrackEvent(i32, i32),
    DebugRegister(i32),
    DebugStartTracking(i32, i32),
    DebugDrop(i32),
    PlayerStatus(i32, bool),
}

struct RecordingHost {
    events: Vec<HostEvent>,
    allow_track: bool,
    velocity_override: Option<Option<Vec3>>,
    hidden_pairs: Vec<(i32, i32)>,
}

impl RecordingHost {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            allow_track: true,
            velocity_override: None,
            hidden_pairs: Vec::new(),
        }
    }

    fn packets_for(&self, player_id: i32) -> Vec<&EntityTrackerPacket> {
        self.events
            .iter()
            .filter_map(|e| match e {
                HostEvent::Packet(pid, pkt) if *pid == player_id => Some(pkt),
                _ => None,
            })
            .collect()
    }

    fn clear(&mut self) {
        self.events.clear();
    }
}

impl EntityTrackerHost for RecordingHost {
    fn send_packet(&mut self, player_id: i32, packet: EntityTrackerPacket) {
        self.events.push(HostEvent::Packet(player_id, packet));
    }
    fn start_seen_by_player(&mut self, entity_id: i32, player_id: i32) {
        self.events.push(HostEvent::StartSeen(entity_id, player_id));
    }
    fn stop_seen_by_player(&mut self, entity_id: i32, player_id: i32) {
        self.events.push(HostEvent::StopSeen(entity_id, player_id));
    }
    fn on_player_track_entity(&mut self, player_id: i32, entity_id: i32) -> bool {
        self.events
            .push(HostEvent::TrackEvent(player_id, entity_id));
        self.allow_track
    }
    fn on_player_untrack_entity(&mut self, player_id: i32, entity_id: i32) {
        self.events
            .push(HostEvent::UntrackEvent(player_id, entity_id));
    }
    fn on_debug_register_entity(&mut self, entity_id: i32) {
        self.events.push(HostEvent::DebugRegister(entity_id));
    }
    fn on_debug_start_tracking_entity(&mut self, player_id: i32, entity_id: i32) {
        self.events
            .push(HostEvent::DebugStartTracking(player_id, entity_id));
    }
    fn on_debug_drop_entity(&mut self, entity_id: i32) {
        self.events.push(HostEvent::DebugDrop(entity_id));
    }
    fn on_player_velocity_event(&mut self, _player_id: i32, velocity: Vec3) -> Option<Vec3> {
        match self.velocity_override {
            Some(ov) => ov,
            None => Some(velocity),
        }
    }
    fn update_player_status(&mut self, player_id: i32, track: bool) {
        self.events.push(HostEvent::PlayerStatus(player_id, track));
    }

    fn can_see(&self, viewer_id: i32, entity_id: i32) -> bool {
        !self.hidden_pairs.contains(&(viewer_id, entity_id))
    }
}

fn viewer(player_id: i32, pos: Vec3) -> ViewerInfo {
    ViewerInfo {
        player_id,
        position: pos,
        view_distance_chunks: 10,
        broadcast_to_player: true,
        chunk_tracked: true,
        can_see: true,
    }
}

fn default_reg() -> EntityTrackRegistration {
    EntityTrackRegistration {
        valid: true,
        level_id: 1,
        is_ender_dragon_part: false,
        suppress_tracker_for_login: false,
        kind: TrackingEntityKind::Other,
        activation_type: ActivationType::Animal,
        client_tracking_range_chunks: 8,
        update_interval: 3,
        track_delta: true,
        indirect_passenger_ranges: Vec::new(),
    }
}

#[test]
fn vec_delta_codec_matches_java_math_round_and_encode_decode() {
    // Java Math.round halves round toward +infinity.
    assert_eq!(java_math_round(0.5), 1);
    assert_eq!(java_math_round(-0.5), 0);
    assert_eq!(java_math_round(-1.5), -1);
    assert_eq!(java_math_round(f64::NAN), 0);
    assert_eq!(java_math_round(f64::from_bits(0x3fdfffffffffffff)), 0);

    let mut codec = VecDeltaCodec::new();
    codec.set_base(Vec3::new(10.0, 64.0, -5.25));
    let target = Vec3::new(10.5, 64.25, -5.0);
    assert_eq!(codec.encode_x(target), 2048);
    assert_eq!(codec.encode_y(target), 1024);
    assert_eq!(codec.encode_z(target), 1024);
    assert_eq!(codec.decode(2048, 1024, 1024), target);
    assert_eq!(codec.decode(0, 0, 0), codec.get_base());
}

#[test]
fn tracking_range_and_visibility_predicates_match_paper() {
    let cfg = TrackingRangeConfig {
        server_view_distance_chunks: 12,
        entity_broadcast_range_percentage: 50,
        tracking_range_y: TrackingRangeYConfig {
            enabled: true,
            animal: 16.0,
            ..TrackingRangeYConfig::default()
        },
        ..TrackingRangeConfig::default()
    };

    // Spigot TrackingRange rules
    assert_eq!(
        get_entity_tracking_range(TrackingEntityKind::Other, ActivationType::Animal, 0, &cfg),
        0
    );
    assert_eq!(
        get_entity_tracking_range(
            TrackingEntityKind::ServerPlayer,
            ActivationType::Misc,
            64,
            &cfg
        ),
        cfg.player_tracking_range
    );
    assert_eq!(
        get_entity_tracking_range(
            TrackingEntityKind::EnderDragon,
            ActivationType::Monster,
            64,
            &cfg
        ),
        12 << 4
    );
    assert_eq!(
        get_entity_tracking_range(
            TrackingEntityKind::MiscTracked,
            ActivationType::Misc,
            64,
            &cfg
        ),
        cfg.misc_tracking_range
    );

    // Effective range expands to indirect passenger max then scales by percentage.
    assert_eq!(get_effective_range(64, &[96, 128], 50), 64);

    // Visibility boundary checks
    let entity_pos = Vec3::new(0.0, 64.0, 0.0);
    let mut v = viewer(2, Vec3::new(32.0, 80.0, 0.0));
    v.view_distance_chunks = 2; // 32 blocks cap
    // Exactly on XZ (32) and Y (16) boundary -> visible
    assert!(is_visible_to_player(1, entity_pos, 64, 16.0, &v));
    // Self is never visible
    assert!(!is_visible_to_player(2, entity_pos, 64, 16.0, &v));
    // Just past Y boundary -> invisible
    v.position.y = 80.1;
    assert!(!is_visible_to_player(1, entity_pos, 64, 16.0, &v));
    // Y disabled (-1.0) -> visible again
    assert!(is_visible_to_player(1, entity_pos, 64, -1.0, &v));
    // Vanished -> invisible
    v.can_see = false;
    assert!(!is_visible_to_player(1, entity_pos, 64, -1.0, &v));
    // Unloaded chunk -> invisible
    v.can_see = true;
    v.chunk_tracked = false;
    assert!(!is_visible_to_player(1, entity_pos, 64, -1.0, &v));

    // TrackingRangeY defaults are unrestricted (-1.0) even when enabled, and EnderDragon is always exempt.
    let mut y_cfg = TrackingRangeYConfig::default();
    y_cfg.enabled = true;
    y_cfg.monster = 32.0;
    assert_eq!(
        y_cfg.range_for(TrackingEntityKind::Other, ActivationType::Monster),
        32.0
    );
    assert_eq!(
        y_cfg.range_for(TrackingEntityKind::Other, ActivationType::Animal),
        -1.0
    );
    assert_eq!(
        y_cfg.range_for(TrackingEntityKind::ServerPlayer, ActivationType::Misc),
        -1.0
    );
    assert_eq!(
        y_cfg.range_for(TrackingEntityKind::EnderDragon, ActivationType::Monster),
        -1.0
    );
}

#[test]
fn pairing_bundle_emits_exact_paper_order_and_sets_force_resync() {
    let mut entity = MockEntity::new(100);
    entity.entity_data = make_syncher(0x01, 20.0); // non-default flags
    entity.syncable_attrs = vec![AttributeSnapshot {
        attribute: Holder::direct(Attribute),
        base: 20.0,
        modifiers: vec![AttributeModifier {
            id: Identifier::with_default_namespace("bonus"),
            amount: 4.0,
            operation: Operation::AddValue,
        }],
    }];
    // Insert equipment out of slot order; pairing must emit in EquipmentSlot::VALUES order
    // and skip empty entries.
    entity.equipment.insert(
        EquipmentSlot::Head,
        EquipmentEntry::new(EquipmentSlot::Head, "minecraft:iron_helmet", 1),
    );
    entity.equipment.insert(
        EquipmentSlot::MainHand,
        EquipmentEntry::new(EquipmentSlot::MainHand, "minecraft:diamond_sword", 1),
    );
    entity.equipment.insert(
        EquipmentSlot::OffHand,
        EquipmentEntry::new(EquipmentSlot::OffHand, "", 0),
    );
    entity.passengers = vec![201, 202];
    entity.vehicle_passengers = Some((300, vec![100]));
    entity.leash_holder_id = Some(400);

    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();
    let v = viewer(10, Vec3::new(12.0, 64.0, 20.0));

    let added = tracker
        .add_entity_simple(&mut entity, &default_reg(), &[v], &mut host)
        .unwrap();
    assert!(added);
    assert!(tracker.get(100).unwrap().server_entity.force_state_resync());

    // Verify exact event ordering: TrackEvent -> Bundle -> StartSeen -> DebugRegister -> DebugStartTracking
    assert_eq!(host.events.len(), 5);
    assert_eq!(host.events[0], HostEvent::TrackEvent(10, 100));
    assert_eq!(host.events[2], HostEvent::StartSeen(100, 10));
    assert_eq!(host.events[3], HostEvent::DebugRegister(100));
    assert_eq!(host.events[4], HostEvent::DebugStartTracking(10, 100));

    let HostEvent::Packet(10, EntityTrackerPacket::Bundle(bundle)) = &host.events[1] else {
        panic!(
            "expected pairing bundle for player 10, got {:?}",
            host.events[1]
        );
    };
    assert_eq!(bundle.len(), 7);
    assert!(matches!(
        &bundle[0],
        EntityTrackerPacket::AddEntity(ClientboundAddEntityPacket { id: 100, .. })
    ));
    assert!(matches!(
        &bundle[1],
        EntityTrackerPacket::SetEntityData(ClientboundSetEntityDataPacket { id: 100, .. })
    ));
    assert!(matches!(
        &bundle[2],
        EntityTrackerPacket::UpdateAttributes(ClientboundUpdateAttributesPacket {
            entity_id: 100,
            ..
        })
    ));
    assert_eq!(
        bundle[3],
        EntityTrackerPacket::SetEquipment {
            entity_id: 100,
            slots: vec![
                EquipmentEntry::new(EquipmentSlot::MainHand, "minecraft:diamond_sword", 1),
                EquipmentEntry::new(EquipmentSlot::Head, "minecraft:iron_helmet", 1),
            ],
            sanitize: true,
        }
    );
    assert_eq!(
        bundle[4],
        EntityTrackerPacket::SetPassengers(ClientboundSetPassengersPacket::new(
            100,
            vec![201, 202]
        ))
    );
    assert_eq!(
        bundle[5],
        EntityTrackerPacket::SetPassengers(ClientboundSetPassengersPacket::new(300, vec![100]))
    );
    assert_eq!(
        bundle[6],
        EntityTrackerPacket::SetEntityLink {
            source_id: 100,
            dest_id: 400,
        }
    );
}

#[test]
fn cancelled_track_event_skips_pairing_but_keeps_seen_by_and_force_resync() {
    let mut entity = MockEntity::new(100);
    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();
    host.allow_track = false;

    let v = viewer(10, Vec3::new(10.0, 64.0, 20.0));
    tracker
        .add_entity_simple(&mut entity, &default_reg(), &[v], &mut host)
        .unwrap();

    // Only TrackEvent fired; no pairing bundle or debug callbacks.
    assert_eq!(host.events, vec![HostEvent::TrackEvent(10, 100)]);
    let tracked = tracker.get(100).unwrap();
    assert!(tracked.seen_by().contains(&10));
    assert!(tracked.server_entity.force_state_resync());
}

#[test]
fn send_changes_dirty_emission_ordering_motion_move_data_attributes_head() {
    let mut entity = MockEntity::new(100);
    let mut tracked = TrackedEntity::new(&entity, 64, 3, true);
    let mut host = RecordingHost::new();
    let v = viewer(10, Vec3::new(10.0, 64.0, 20.0));

    // Pair player 10 and consume initial forced resync tick.
    tracked.update_player(&mut entity, &v, &[], 100, -1.0, &mut host);
    tracked.send_changes(&mut entity, 10, &mut host);
    host.clear();

    // Now mutate position, velocity, syncher metadata, attributes, and head yaw on an off-interval tick.
    // Because syncher is dirty, the sync gate opens even when tick_count % 3 != 0.
    entity.position = Vec3::new(10.5, 64.0, 20.0);
    entity.delta_movement = Vec3::new(0.25, 0.0, 0.0);
    entity.entity_data.set(data_flags_accessor(), 0x02);
    entity.dirty_attrs.push(AttributeSnapshot {
        attribute: Holder::direct(Attribute),
        base: 0.3,
        modifiers: Vec::new(),
    });
    entity.y_head_rot = 90.0;

    tracked.send_changes(&mut entity, 10, &mut host);

    let pkts = host.packets_for(10);
    assert_eq!(pkts.len(), 5);
    // Exact Paper order: Motion -> MovePos -> SetEntityData -> UpdateAttributes -> RotateHead
    assert!(matches!(
        pkts[0],
        EntityTrackerPacket::SetEntityMotion { .. }
    ));
    assert!(matches!(
        pkts[1],
        EntityTrackerPacket::MoveEntityPos {
            xa: 2048,
            ya: 0,
            za: 0,
            ..
        }
    ));
    assert_eq!(
        pkts[2],
        &EntityTrackerPacket::SetEntityData(ClientboundSetEntityDataPacket::new(
            100,
            vec![DataValue {
                id: 0,
                serializer: SerializerId::Byte,
                value: SerializedValue::Byte(0x02),
            }]
        ))
    );
    assert!(matches!(pkts[3], EntityTrackerPacket::UpdateAttributes(_)));
    assert!(matches!(
        pkts[4],
        EntityTrackerPacket::RotateHead { y_head_rot: 64, .. }
    ));
}

#[test]
fn passenger_change_filters_transitioning_players_and_notifies_ridden_player() {
    let mut ridden_player = MockEntity::new(1);
    ridden_player.server_player = true;
    let mut tracked = TrackedEntity::new(&ridden_player, 64, 3, true);
    let mut host = RecordingHost::new();

    // Players 2 (will mount) and 3 (bystander) both see player 1.
    tracked.update_player(
        &mut ridden_player,
        &viewer(2, Vec3::new(10.0, 64.0, 20.0)),
        &[],
        100,
        -1.0,
        &mut host,
    );
    tracked.update_player(
        &mut ridden_player,
        &viewer(3, Vec3::new(10.0, 64.0, 20.0)),
        &[],
        100,
        -1.0,
        &mut host,
    );
    tracked.send_changes(&mut ridden_player, 10, &mut host);
    host.clear();

    // Player 2 mounts player 1.
    ridden_player.passengers = vec![2];
    tracked.send_changes(&mut ridden_player, 10, &mut host);

    // Bystander 3 receives SetPassengers; mounting player 2 is filtered out
    // (passengers.contains(2) != lastPassengers.contains(2)); ridden player 1 gets direct send.
    let expected =
        EntityTrackerPacket::SetPassengers(ClientboundSetPassengersPacket::new(1, vec![2]));
    assert_eq!(host.packets_for(2), Vec::<&EntityTrackerPacket>::new());
    assert_eq!(host.packets_for(3), vec![&expected]);
    assert_eq!(host.packets_for(1), vec![&expected]);
}

#[test]
fn riding_sends_rotation_only_and_dismount_triggers_position_sync() {
    let mut entity = MockEntity::new(50);
    let mut tracked = TrackedEntity::new(&entity, 64, 1, true);
    let mut host = RecordingHost::new();
    tracked.update_player(
        &mut entity,
        &viewer(10, Vec3::new(10.0, 64.0, 20.0)),
        &[],
        100,
        -1.0,
        &mut host,
    );
    tracked.send_changes(&mut entity, 10, &mut host);
    host.clear();

    // While riding: moving position + changing yaw emits ONLY MoveEntityRot (no pos packet).
    entity.vehicle_passengers = Some((99, vec![50]));
    entity.position = Vec3::new(15.0, 64.0, 20.0);
    entity.y_rot = 45.0;
    tracked.send_changes(&mut entity, 10, &mut host);
    let pkts = host.packets_for(10);
    assert_eq!(pkts.len(), 1);
    assert!(matches!(pkts[0], EntityTrackerPacket::MoveEntityRot { .. }));
    host.clear();

    // Dismounting triggers EntityPositionSync because was_riding was true.
    entity.vehicle_passengers = None;
    tracked.send_changes(&mut entity, 10, &mut host);
    let pkts = host.packets_for(10);
    assert!(matches!(
        pkts.as_slice(),
        [EntityTrackerPacket::EntityPositionSync { entity_id: 50, .. }]
    ));
}

#[test]
fn new_minecart_emits_dirty_data_before_move_minecart() {
    let mut cart = MockEntity::new(77);
    cart.living = false;
    cart.new_minecart = true;
    let mut tracked = TrackedEntity::new(&cart, 64, 1, true);
    let mut host = RecordingHost::new();
    tracked.update_player(
        &mut cart,
        &viewer(10, Vec3::new(10.0, 64.0, 20.0)),
        &[],
        100,
        -1.0,
        &mut host,
    );
    tracked.send_changes(&mut cart, 10, &mut host);
    host.clear();

    cart.entity_data.set(data_health_accessor(), 12.5);
    cart.minecart_steps.push(MinecartStep {
        position: Vec3::new(11.0, 64.0, 20.0),
        movement: Vec3::new(0.1, 0.0, 0.0),
        y_rot: 0.0,
        x_rot: 0.0,
        weight: 1.0,
    });
    tracked.send_changes(&mut cart, 10, &mut host);

    let pkts = host.packets_for(10);
    assert_eq!(pkts.len(), 2);
    assert!(matches!(pkts[0], EntityTrackerPacket::SetEntityData(_)));
    assert!(matches!(
        pkts[1],
        EntityTrackerPacket::MoveMinecart { entity_id: 77, .. }
    ));
}

#[test]
fn hurt_marked_player_velocity_event_cancellation_and_modification() {
    let mut player = MockEntity::new(1);
    player.server_player = true;
    let mut tracked = TrackedEntity::new(&player, 64, 20, true);
    let mut host = RecordingHost::new();
    tracked.update_player(
        &mut player,
        &viewer(2, Vec3::new(10.0, 64.0, 20.0)),
        &[],
        100,
        -1.0,
        &mut host,
    );
    tracked.send_changes(&mut player, 10, &mut host);
    host.clear();

    // Cancelled PlayerVelocityEvent: hurtMarked stays true and no packet is sent.
    player.hurt_marked = true;
    player.delta_movement = Vec3::new(0.0, 0.5, 0.0);
    host.velocity_override = Some(None);
    tracked.send_changes(&mut player, 10, &mut host);
    assert!(player.hurt_marked);
    assert!(host.events.is_empty());

    // Modified velocity: entity velocity updated, hurtMarked cleared, sent to viewer 2 and self 1.
    let modified = Vec3::new(1.0, 2.0, 3.0);
    host.velocity_override = Some(Some(modified));
    tracked.send_changes(&mut player, 10, &mut host);
    assert!(!player.hurt_marked);
    assert_eq!(player.delta_movement, modified);
    let expected = EntityTrackerPacket::SetEntityMotion {
        entity_id: 1,
        movement: modified,
    };
    assert_eq!(host.packets_for(2), vec![&expected]);
    assert_eq!(host.packets_for(1), vec![&expected]);
}

#[test]
fn remove_entity_and_moonrise_tick_transitions() {
    let mut mob = MockEntity::new(200);
    let mut p1 = MockEntity::new(1);
    p1.server_player = true;
    let mut p2 = MockEntity::new(2);
    p2.server_player = true;

    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();

    let v1 = viewer(1, Vec3::new(10.0, 64.0, 20.0));
    let v2 = viewer(2, Vec3::new(11.0, 64.0, 20.0));

    let mut p_reg = default_reg();
    p_reg.kind = TrackingEntityKind::ServerPlayer;
    tracker
        .add_entity_simple(&mut p1, &p_reg, &[v1, v2], &mut host)
        .unwrap();
    tracker
        .add_entity_simple(&mut p2, &p_reg, &[v1, v2], &mut host)
        .unwrap();
    tracker
        .add_entity_simple(&mut mob, &default_reg(), &[v1, v2], &mut host)
        .unwrap();
    assert_eq!(tracker.entities_tracked_by(1), vec![2, 200]);
    host.clear();

    // Removing player 1 updates player status, untracks player 1 from p2 and mob,
    // and broadcasts RemoveEntities(1) to p2 (player 1's viewer).
    assert!(tracker.remove_entity(1, true, &mut host));
    assert!(!tracker.has_entity_with_id(1));
    assert!(!tracker.get(200).unwrap().seen_by().contains(&1));
    assert!(tracker.get(200).unwrap().seen_by().contains(&2));
    assert_eq!(host.events[0], HostEvent::PlayerStatus(1, false));
    assert!(host.events.contains(&HostEvent::UntrackEvent(2, 1)));
    assert!(
        host.packets_for(2)
            .contains(&&EntityTrackerPacket::RemoveEntities(
                ClientboundRemoveEntitiesPacket::new(vec![1])
            ))
    );
    host.clear();

    // moonrise_tick with empty nearby chunk clears remaining viewer 2 and fires DebugDrop.
    let mut entities = HashMap::from([(200, mob)]);
    let ctx = TrackerTickContext {
        entity_id: 200,
        kind: TrackingEntityKind::Other,
        activation_type: ActivationType::Animal,
        chunk_status: FullChunkStatus::Full,
        nearby: None,
        indirect_passenger_ranges: Vec::new(),
    };
    tracker.new_tracker_tick(
        &[ctx],
        |id, f| {
            if let Some(e) = entities.get_mut(&id) {
                f(e);
            }
        },
        &mut host,
    );
    assert!(!tracker.is_tracked_by_any_player(200));
    assert!(host.events.contains(&HostEvent::DebugDrop(200)));
    // Because no players remain and chunk status is Full (< EntityTicking), send_changes did not tick.
    assert_eq!(tracker.get(200).unwrap().server_entity.tick_count(), 0);
}

#[test]
fn equipment_and_entity_link_dirty_emission() {
    let mut mob = MockEntity::new(300);
    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();
    let v1 = viewer(1, Vec3::new(10.0, 64.0, 20.0));
    tracker
        .add_entity_simple(&mut mob, &default_reg(), &[v1], &mut host)
        .unwrap();
    host.clear();

    mob.dirty_equipment.push(EquipmentEntry::new(
        EquipmentSlot::Chest,
        "minecraft:iron_chestplate",
        1,
    ));
    assert!(tracker.flush_equipment_updates(&mut mob, &mut host));
    assert!(!tracker.flush_equipment_updates(&mut mob, &mut host));

    tracker.send_entity_link_update(300, 1, &mut host);
    tracker.send_entity_link_update(300, 0, &mut host);

    let pkts = host.packets_for(1);
    assert_eq!(pkts.len(), 3);
    assert_eq!(
        pkts[0],
        &EntityTrackerPacket::SetEquipment {
            entity_id: 300,
            slots: vec![EquipmentEntry::new(
                EquipmentSlot::Chest,
                "minecraft:iron_chestplate",
                1
            )],
            sanitize: false,
        }
    );
    assert_eq!(
        pkts[1],
        &EntityTrackerPacket::SetEntityLink {
            source_id: 300,
            dest_id: 1,
        }
    );
    assert_eq!(
        pkts[2],
        &EntityTrackerPacket::SetEntityLink {
            source_id: 300,
            dest_id: 0,
        }
    );
}

#[test]
fn controlled_negatives_reject_illegal_adds_and_removed_pairing() {
    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();

    // 1. Invalid entity -> Err
    let mut e = MockEntity::new(10);
    let mut invalid_reg = default_reg();
    invalid_reg.valid = false;
    assert!(matches!(
        tracker.add_entity_simple(&mut e, &invalid_reg, &[], &mut host),
        Err(EntityTrackerError::IllegalAddEntity { entity_id: 10, .. })
    ));

    // 2. Wrong level -> Err
    let mut wrong_level = default_reg();
    wrong_level.level_id = 999;
    assert!(matches!(
        tracker.add_entity_simple(&mut e, &wrong_level, &[], &mut host),
        Err(EntityTrackerError::IllegalAddEntity { entity_id: 10, .. })
    ));

    // 3. EnderDragonPart -> Ok(false), not tracked
    let mut dragon_part = default_reg();
    dragon_part.is_ender_dragon_part = true;
    assert_eq!(
        tracker.add_entity_simple(&mut e, &dragon_part, &[], &mut host),
        Ok(false)
    );
    assert!(!tracker.has_entity_with_id(10));

    // 4. Duplicate entity ID -> Err
    assert_eq!(
        tracker.add_entity_simple(&mut e, &default_reg(), &[], &mut host),
        Ok(true)
    );
    assert!(matches!(
        tracker.add_entity_simple(&mut e, &default_reg(), &[], &mut host),
        Err(EntityTrackerError::IllegalAddEntity { entity_id: 10, .. })
    ));

    // 5. Removed entity produces empty pairing bundle.
    let mut removed = MockEntity::new(11);
    removed.removed = true;
    let mut server_entity = ServerEntity::new(&removed, 3, true);
    let mut packets = Vec::new();
    let mut dummy_sync = NoopSync;
    server_entity.send_pairing_data(&mut removed, 1, &mut packets, &mut dummy_sync);
    assert!(packets.is_empty());
}

struct NoopSync;

impl ServerEntitySynchronizer for NoopSync {
    fn send_to_tracking_players(&mut self, _packet: EntityTrackerPacket) {}
    fn send_to_tracking_players_and_self(&mut self, _packet: EntityTrackerPacket) {}
    fn send_to_tracking_players_filtered(
        &mut self,
        _packet: EntityTrackerPacket,
        _predicate: &dyn Fn(i32) -> bool,
    ) {
    }
    fn send_to_player(&mut self, _player_id: i32, _packet: EntityTrackerPacket) {}
    fn start_seen_by_player(&mut self, _entity_id: i32, _player_id: i32) {}
    fn stop_seen_by_player(&mut self, _entity_id: i32, _player_id: i32) {}
}

#[test]
fn update_player_against_all_respects_per_entity_can_see() {
    let mut tracker = ChunkMapEntityTracker::new(1, TrackingRangeConfig::default());
    let mut host = RecordingHost::new();
    // Joining player 1 cannot see hidden entity 20, but can see entity 21.
    host.hidden_pairs.push((1, 20));

    let mut hidden = MockEntity::new(20);
    hidden.position = Vec3::new(4.0, 64.0, 4.0);
    let mut visible = MockEntity::new(21);
    visible.position = Vec3::new(6.0, 64.0, 6.0);

    tracker
        .add_entity_simple(&mut hidden, &default_reg(), &[], &mut host)
        .unwrap();
    tracker
        .add_entity_simple(&mut visible, &default_reg(), &[], &mut host)
        .unwrap();

    let v1 = viewer(1, Vec3::new(0.0, 64.0, 0.0));
    tracker.update_player_against_all(
        &v1,
        |id, apply| {
            if id == 20 {
                apply(
                    &mut hidden,
                    TrackingEntityKind::Other,
                    ActivationType::Animal,
                    &[],
                );
            } else if id == 21 {
                apply(
                    &mut visible,
                    TrackingEntityKind::Other,
                    ActivationType::Animal,
                    &[],
                );
            }
        },
        &mut host,
    );

    assert!(!tracker.get(20).unwrap().seen_by().contains(&1));
    assert!(tracker.get(21).unwrap().seen_by().contains(&1));
}
