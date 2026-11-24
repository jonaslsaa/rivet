//! Unit tests for `world::level::entity` and `world::entity::entity_record` grounded in Paper 26.2 behavior.

use crate::world::entity::entity_record::{
    EntityRemovalReason, EntitySaveLimits, EntityStorageError, MAX_HORIZONTAL_POSITION,
    MAX_VERTICAL_POSITION, NetworkIdAllocator, StoredEntityRecord, TAG_DATA_VERSION, TAG_ENTITIES,
    TAG_ID, TAG_PASSENGERS, TAG_POSITION, TAG_UUID, decode_chunk_entities_nbt,
    encode_chunk_entities_nbt,
};
use crate::world::level::chunk::storage::entity_storage::{
    EntityStorage, InMemoryEntityRegionBackend,
};
use crate::world::level::entity::chunk_entities::ChunkEntities;
use crate::world::level::entity::entity_lookup::EntityLookup;
use crate::world::level::entity::entity_section::EntitySectionStorage;
use crate::world::level::entity::persistent_entity_section_manager::{
    ChunkLoadStatus, EntityLifecycleEvent, PersistentEntitySectionManager,
};
use crate::world::level::entity::visibility::{FullChunkStatus, Visibility};
use rivet_core::shared_constants::WORLD_VERSION;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::double_tag::DoubleTag;
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;
use rivet_registry::core::{ChunkPos, SectionPos};

use rivet_registry::world::phys::AABB;
use rivet_registry::world::phys::Vec3;
use rivet_util::mth::Uuid;

fn make_uuid(hi: i64, lo: i64) -> Uuid {
    Uuid {
        most: hi,
        least: lo,
    }
}

fn make_entity(
    id: &str,
    network_id: i32,
    uuid: Uuid,
    x: f64,
    y: f64,
    z: f64,
) -> StoredEntityRecord {
    StoredEntityRecord::new(id, network_id, uuid, Vec3::new(x, y, z))
}

#[test]
fn visibility_from_full_chunk_status_matches_paper() {
    assert_eq!(
        Visibility::from_full_chunk_status(FullChunkStatus::Inaccessible),
        Visibility::Hidden
    );
    assert_eq!(
        Visibility::from_full_chunk_status(FullChunkStatus::Full),
        Visibility::Tracked
    );
    assert_eq!(
        Visibility::from_full_chunk_status(FullChunkStatus::BlockTicking),
        Visibility::Tracked
    );
    assert_eq!(
        Visibility::from_full_chunk_status(FullChunkStatus::EntityTicking),
        Visibility::Ticking
    );

    assert!(!Visibility::Hidden.is_accessible());
    assert!(!Visibility::Hidden.is_ticking());
    assert!(Visibility::Tracked.is_accessible());
    assert!(!Visibility::Tracked.is_ticking());
    assert!(Visibility::Ticking.is_accessible());
    assert!(Visibility::Ticking.is_ticking());
}

#[test]
fn entity_lookup_preserves_insertion_order_and_rejects_duplicates() {
    let mut lookup = EntityLookup::new();
    let u1 = make_uuid(1, 10);
    let u2 = make_uuid(2, 20);
    let u3 = make_uuid(3, 30);

    assert!(lookup.add(42, u1));
    assert!(lookup.add(7, u2));
    assert!(lookup.add(99, u3));

    // Duplicate UUID or duplicate network ID must be rejected without mutation.
    assert!(!lookup.add(100, u1));
    assert!(!lookup.add(7, make_uuid(4, 40)));
    assert_eq!(lookup.count(), 3);
    assert_eq!(lookup.iter_ids().collect::<Vec<_>>(), vec![42, 7, 99]);

    assert_eq!(lookup.id_by_uuid(&u2), Some(7));
    assert_eq!(lookup.uuid_by_id(7), Some(u2));

    assert!(lookup.remove(7, u2));
    assert_eq!(lookup.iter_ids().collect::<Vec<_>>(), vec![42, 99]);
    assert_eq!(lookup.id_by_uuid(&u2), None);
    assert_eq!(lookup.uuid_by_id(7), None);
}

#[test]
fn current_format_nbt_round_trip_clamping_and_extra_tags() {
    let pos = ChunkPos::new(2, -3);
    let uuid = make_uuid(0x1122_3344_5566_7788, -0x0123_4567_89ab_cdef);
    let world_uuid = make_uuid(100, 200);

    let mut entity = make_entity("minecraft:pig", 1, uuid, 35.5, 64.25, -40.75);
    entity.motion = Vec3::new(0.5, -12.0, 2.0); // |y| > 10.0 must be zeroed on load
    entity.y_rot = f32::NAN; // NaN yaw must be saved as 0.0
    entity.x_rot = -15.5;
    entity.fall_distance = 3.25;
    entity.fire_ticks = 40;
    entity.air_supply = 150;
    entity.on_ground = true;
    entity.invulnerable = true;
    entity.portal_cooldown = 12;
    entity.world_uuid = Some(world_uuid);
    entity.extra_tags.put_float("Health", 10.0);
    entity.extra_tags.put_string("CustomTag", "preserved");

    let limits = EntitySaveLimits::new();
    let encoded = encode_chunk_entities_nbt(pos, &[entity], &limits);

    assert_eq!(encoded.get_int(TAG_DATA_VERSION), Some(WORLD_VERSION));
    assert_eq!(encoded.get_int_array(TAG_POSITION), Some(&vec![2, -3]));

    let mut allocator = NetworkIdAllocator::starting_after(50);
    let decoded = decode_chunk_entities_nbt(pos, &encoded, &mut allocator, &limits).unwrap();
    assert_eq!(decoded.len(), 1);

    let restored = &decoded[0];
    assert_eq!(restored.id, "minecraft:pig");
    assert_eq!(restored.network_id, 51);
    assert_eq!(restored.uuid, uuid);
    assert_eq!(restored.world_uuid, Some(world_uuid));
    assert_eq!(restored.pos, Vec3::new(35.5, 64.25, -40.75));
    assert_eq!(restored.motion, Vec3::new(0.5, 0.0, 2.0));
    assert_eq!(restored.y_rot, 0.0);
    assert_eq!(restored.x_rot, -15.5);
    assert_eq!(restored.fall_distance, 3.25);
    assert_eq!(restored.fire_ticks, 40);
    assert_eq!(restored.air_supply, 150);
    assert!(restored.on_ground);
    assert!(restored.invulnerable);
    assert_eq!(restored.portal_cooldown, 12);
    assert_eq!(restored.extra_tags.get_float("Health"), Some(10.0));
    assert_eq!(
        restored
            .extra_tags
            .get_string("CustomTag")
            .map(String::as_str),
        Some("preserved")
    );
}

#[test]
fn position_is_clamped_to_paper_world_bounds_on_load() {
    let pos = ChunkPos::new(0, 0);
    let mut entity = make_entity("minecraft:cow", 1, make_uuid(1, 2), 9.0e8, -5.0e8, -9.0e8);
    entity.motion = Vec3::new(15.0, -15.0, 5.0);

    let encoded = encode_chunk_entities_nbt(pos, &[entity], &EntitySaveLimits::new());
    let mut allocator = NetworkIdAllocator::new();
    let decoded =
        decode_chunk_entities_nbt(pos, &encoded, &mut allocator, &EntitySaveLimits::new()).unwrap();

    assert_eq!(decoded[0].pos.x, MAX_HORIZONTAL_POSITION);
    assert_eq!(decoded[0].pos.y, -MAX_VERTICAL_POSITION);
    assert_eq!(decoded[0].pos.z, -MAX_HORIZONTAL_POSITION);
    assert_eq!(decoded[0].motion, Vec3::new(0.0, 0.0, 5.0));
}

#[test]
fn recursive_passengers_save_and_restore_vehicle_links_and_positions() {
    let pos = ChunkPos::new(0, 0);
    let boat_uuid = make_uuid(1, 1);
    let zombie_uuid = make_uuid(2, 2);
    let chicken_uuid = make_uuid(3, 3);
    let baby_uuid = make_uuid(4, 4);

    let mut boat = make_entity("minecraft:oak_boat", 10, boat_uuid, 8.0, 64.0, 8.0);
    let mut zombie = make_entity("minecraft:zombie", 11, zombie_uuid, 12.0, 64.5, 14.0);
    let mut chicken = make_entity("minecraft:chicken", 12, chicken_uuid, 99.0, 65.0, 99.0);
    let baby = make_entity("minecraft:zombie", 13, baby_uuid, 50.0, 65.7, 50.0);

    chicken.passengers.push(baby);
    zombie.vehicle_id = Some(10);
    boat.passengers.push(zombie);
    boat.passengers.push(chicken);

    let encoded = encode_chunk_entities_nbt(pos, &[boat], &EntitySaveLimits::new());

    // Verify `Passengers` is written on boat and omitted on leaf baby.
    let entities_list = encoded.get_list(TAG_ENTITIES).unwrap();
    assert_eq!(entities_list.list.len(), 1);
    let Tag::Compound(boat_tag) = &entities_list.list[0] else {
        panic!("expected compound");
    };
    let boat_passengers = boat_tag.get_list(TAG_PASSENGERS).unwrap();
    assert_eq!(boat_passengers.list.len(), 2);

    let mut allocator = NetworkIdAllocator::new();
    let decoded =
        decode_chunk_entities_nbt(pos, &encoded, &mut allocator, &EntitySaveLimits::new()).unwrap();

    // Pre-order flattening: boat (1), zombie (2), chicken (3), baby (4).
    assert_eq!(decoded.len(), 4);
    assert_eq!(decoded[0].uuid, boat_uuid);
    assert_eq!(decoded[0].network_id, 1);
    assert_eq!(decoded[0].vehicle_id, None);
    assert_eq!(decoded[0].passenger_ids, vec![2, 3]);

    // Passenger X/Z on save matches vehicle X/Z while preserving passenger Y.
    assert_eq!(decoded[1].uuid, zombie_uuid);
    assert_eq!(decoded[1].network_id, 2);
    assert_eq!(decoded[1].vehicle_id, Some(1));
    assert_eq!(decoded[1].pos, Vec3::new(8.0, 64.5, 8.0));

    assert_eq!(decoded[2].uuid, chicken_uuid);
    assert_eq!(decoded[2].network_id, 3);
    assert_eq!(decoded[2].vehicle_id, Some(1));
    assert_eq!(decoded[2].passenger_ids, vec![4]);
    assert_eq!(decoded[2].pos, Vec3::new(8.0, 65.0, 8.0));

    assert_eq!(decoded[3].uuid, baby_uuid);
    assert_eq!(decoded[3].network_id, 4);
    assert_eq!(decoded[3].vehicle_id, Some(3));
    assert_eq!(decoded[3].pos, Vec3::new(8.0, 65.7, 8.0));
}

#[test]
fn paper_per_chunk_save_and_load_limits_are_enforced() {
    let pos = ChunkPos::new(1, 1);
    let arrows: Vec<StoredEntityRecord> = (0..5)
        .map(|i| {
            make_entity(
                "minecraft:arrow",
                i + 1,
                make_uuid(100, i64::from(i)),
                18.0 + f64::from(i),
                64.0,
                18.0,
            )
        })
        .collect();

    let mut save_limits = EntitySaveLimits::new();
    save_limits.set_limit("minecraft:arrow", 3);
    let encoded = encode_chunk_entities_nbt(pos, &arrows, &save_limits);
    assert_eq!(encoded.get_list(TAG_ENTITIES).unwrap().list.len(), 3);

    // Tighter load limit caps decoding further to 2.
    let mut load_limits = EntitySaveLimits::new();
    load_limits.set_limit("minecraft:arrow", 2);
    let mut allocator = NetworkIdAllocator::new();
    let decoded = decode_chunk_entities_nbt(pos, &encoded, &mut allocator, &load_limits).unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].uuid, make_uuid(100, 0));
    assert_eq!(decoded[1].uuid, make_uuid(100, 1));
}

#[test]
fn malformed_and_historical_current_format_payloads_fail_hard() {
    let pos = ChunkPos::new(0, 0);
    let limits = EntitySaveLimits::new();
    let mut allocator = NetworkIdAllocator::new();

    // 1. Missing DataVersion
    let mut tag = CompoundTag::new();
    tag.put_int_array(TAG_POSITION, vec![0, 0]);
    assert_eq!(
        decode_chunk_entities_nbt(pos, &tag, &mut allocator, &limits),
        Err(EntityStorageError::MissingDataVersion)
    );

    // 2. Historical DataVersion (e.g. 1.21 / 3953) must fail hard, not silently coerce
    tag.put_int(TAG_DATA_VERSION, 3953);
    assert_eq!(
        decode_chunk_entities_nbt(pos, &tag, &mut allocator, &limits),
        Err(EntityStorageError::UnsupportedDataVersion {
            found: 3953,
            expected: WORLD_VERSION,
        })
    );

    // 3. Misplaced Position
    let mut misplaced = CompoundTag::new();
    misplaced.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    misplaced.put_int_array(TAG_POSITION, vec![5, 6]);
    assert_eq!(
        decode_chunk_entities_nbt(pos, &misplaced, &mut allocator, &limits),
        Err(EntityStorageError::MisplacedChunk {
            expected: pos,
            stored: ChunkPos::new(5, 6),
        })
    );

    // 4. Malformed Position length
    let mut bad_pos = CompoundTag::new();
    bad_pos.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    bad_pos.put_int_array(TAG_POSITION, vec![0]);
    assert_eq!(
        decode_chunk_entities_nbt(pos, &bad_pos, &mut allocator, &limits),
        Err(EntityStorageError::MalformedPosition { found: 1 })
    );

    // 5. Malformed Entities tag (wrong type)
    let mut bad_entities = CompoundTag::new();
    bad_entities.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    bad_entities.put_int_array(TAG_POSITION, vec![0, 0]);
    bad_entities.put_string(TAG_ENTITIES, "not-a-list");
    assert_eq!(
        decode_chunk_entities_nbt(pos, &bad_entities, &mut allocator, &limits),
        Err(EntityStorageError::MalformedEntitiesList)
    );

    // 6. Malformed UUID length inside an entity
    let mut bad_uuid_entity = CompoundTag::new();
    bad_uuid_entity.put_string(TAG_ID, "minecraft:pig");
    bad_uuid_entity.put_int_array(TAG_UUID, vec![1, 2, 3]);
    let mut list = ListTag::new();
    list.add(Tag::Compound(bad_uuid_entity));
    let mut chunk_with_bad_uuid = CompoundTag::new();
    chunk_with_bad_uuid.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    chunk_with_bad_uuid.put_int_array(TAG_POSITION, vec![0, 0]);
    chunk_with_bad_uuid.put(TAG_ENTITIES.to_string(), Tag::List(list));
    assert_eq!(
        decode_chunk_entities_nbt(pos, &chunk_with_bad_uuid, &mut allocator, &limits),
        Err(EntityStorageError::MalformedUuid)
    );

    // 7. Non-finite position
    let mut nan_pos_list = ListTag::new();
    nan_pos_list.add(Tag::Double(DoubleTag::value_of(f64::NAN)));
    nan_pos_list.add(Tag::Double(DoubleTag::value_of(64.0)));
    nan_pos_list.add(Tag::Double(DoubleTag::value_of(0.0)));
    let mut nan_entity = CompoundTag::new();
    nan_entity.put_string(TAG_ID, "minecraft:pig");
    nan_entity.put("Pos".to_string(), Tag::List(nan_pos_list));
    let mut list2 = ListTag::new();
    list2.add(Tag::Compound(nan_entity));
    let mut chunk_nan_pos = CompoundTag::new();
    chunk_nan_pos.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    chunk_nan_pos.put_int_array(TAG_POSITION, vec![0, 0]);
    chunk_nan_pos.put(TAG_ENTITIES.to_string(), Tag::List(list2));
    assert_eq!(
        decode_chunk_entities_nbt(pos, &chunk_nan_pos, &mut allocator, &limits),
        Err(EntityStorageError::NonFinitePosition)
    );

    // 8. Non-finite rotation
    let mut inf_rot_list = ListTag::new();
    inf_rot_list.add(Tag::Float(FloatTag::value_of(f32::INFINITY)));
    inf_rot_list.add(Tag::Float(FloatTag::value_of(0.0)));
    let mut inf_rot_entity = CompoundTag::new();
    inf_rot_entity.put_string(TAG_ID, "minecraft:pig");
    inf_rot_entity.put("Rotation".to_string(), Tag::List(inf_rot_list));
    let mut list3 = ListTag::new();
    list3.add(Tag::Compound(inf_rot_entity));
    let mut chunk_inf_rot = CompoundTag::new();
    chunk_inf_rot.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    chunk_inf_rot.put_int_array(TAG_POSITION, vec![0, 0]);
    chunk_inf_rot.put(TAG_ENTITIES.to_string(), Tag::List(list3));
    assert_eq!(
        decode_chunk_entities_nbt(pos, &chunk_inf_rot, &mut allocator, &limits),
        Err(EntityStorageError::NonFiniteRotation)
    );
}

#[test]
fn section_manager_visibility_transitions_and_callback_ordering() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(0, 0);

    // Chunk starts Hidden -> adding entity creates it without tracking or ticking.
    let e1_id = manager.allocate_network_id();
    let e1_uuid = make_uuid(10, 1);
    assert!(
        manager
            .add_new_entity(make_entity(
                "minecraft:sheep",
                e1_id,
                e1_uuid,
                4.0,
                64.0,
                4.0
            ))
            .unwrap()
    );
    assert!(manager.is_chunk_dirty(chunk));
    assert!(manager.get_visible_by_id(e1_id).is_none());
    assert_eq!(
        manager.drain_events(),
        vec![EntityLifecycleEvent::Created(e1_id)]
    );

    // Hidden -> Tracked: fires TrackingStart, now visible by ID and UUID.
    manager
        .update_chunk_status(chunk, Visibility::Tracked)
        .unwrap();
    assert_eq!(
        manager.drain_events(),
        vec![EntityLifecycleEvent::TrackingStart(e1_id)]
    );
    assert!(manager.get_visible_by_id(e1_id).is_some());
    assert!(manager.get_visible_by_uuid(&e1_uuid).is_some());

    // Tracked -> Ticking: fires TickingStart.
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    assert_eq!(
        manager.drain_events(),
        vec![EntityLifecycleEvent::TickingStart(e1_id)]
    );

    // Ticking -> Hidden: fires TickingEnd BEFORE TrackingEnd (Paper order).
    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    assert_eq!(
        manager.drain_events(),
        vec![
            EntityLifecycleEvent::TickingEnd(e1_id),
            EntityLifecycleEvent::TrackingEnd(e1_id),
        ]
    );
    assert!(manager.get_visible_by_id(e1_id).is_none());
}

#[test]
fn always_ticking_entity_remains_tracked_and_ticking_in_hidden_section() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(0, 0);

    let id = manager.allocate_network_id();
    let mut entity = make_entity("minecraft:player", id, make_uuid(99, 1), 2.0, 64.0, 2.0);
    entity.always_ticking = true;
    manager.add_new_entity(entity).unwrap();

    assert_eq!(
        manager.drain_events(),
        vec![
            EntityLifecycleEvent::Created(id),
            EntityLifecycleEvent::TrackingStart(id),
            EntityLifecycleEvent::TickingStart(id),
        ]
    );
    assert!(manager.get_visible_by_id(id).is_some());

    // Transitioning chunk to Tracked and back to Hidden must not stop tracking/ticking.
    manager
        .update_chunk_status(chunk, Visibility::Tracked)
        .unwrap();
    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    assert!(manager.drain_events().is_empty());
    assert!(manager.get_visible_by_id(id).is_some());
}

#[test]
fn cross_chunk_movement_updates_sections_visibility_and_dirty_flags() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk_a = ChunkPos::new(0, 0);
    let chunk_b = ChunkPos::new(1, 0);

    manager
        .update_chunk_status(chunk_a, Visibility::Ticking)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let id = manager.allocate_network_id();
    manager
        .add_new_entity(make_entity(
            "minecraft:cow",
            id,
            make_uuid(7, 1),
            8.0,
            64.0,
            8.0,
        ))
        .unwrap();
    let old_section = SectionPos::of(0, 4, 0).as_long_self();
    let new_section = SectionPos::of(1, 4, 0).as_long_self();
    assert_eq!(manager.entity_section_key(id), Some(old_section));
    manager.drain_events();

    // Move into chunk_b (which is Hidden): old section pruned, visibility drops to Hidden.
    assert!(manager.move_entity(id, Vec3::new(20.0, 64.0, 8.0)).unwrap());
    assert_eq!(manager.entity_section_key(id), Some(new_section));
    assert!(manager.section_storage().get_section(old_section).is_none());
    assert!(manager.is_chunk_dirty(chunk_a));
    assert!(manager.is_chunk_dirty(chunk_b));
    assert!(manager.get_visible_by_id(id).is_none());
    assert_eq!(
        manager.drain_events(),
        vec![
            EntityLifecycleEvent::SectionChange {
                entity_id: id,
                old_section,
                new_section,
            },
            EntityLifecycleEvent::TickingEnd(id),
            EntityLifecycleEvent::TrackingEnd(id),
        ]
    );
}

#[test]
fn spatial_bounds_query_respects_accessibility_and_grace_margins() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let visible_chunk = ChunkPos::new(0, 0);
    let hidden_chunk = ChunkPos::new(1, 0);

    manager
        .update_chunk_status(visible_chunk, Visibility::Tracked)
        .unwrap();
    manager.process_pending_loads().unwrap();

    // Entity in visible chunk near section border (x = 15.5) with wide AABB overlapping x = 16.5.
    let e_vis = manager.allocate_network_id();
    let mut big_entity = make_entity(
        "minecraft:ravager",
        e_vis,
        make_uuid(50, 1),
        15.5,
        64.0,
        8.0,
    );
    big_entity.bounding_box = AABB::new(14.5, 64.0, 7.0, 17.0, 66.0, 9.0);
    manager.add_new_entity(big_entity).unwrap();

    // Entity in hidden chunk inside query box must NOT be returned.
    let e_hid = manager.allocate_network_id();
    manager
        .add_new_entity(make_entity(
            "minecraft:pig",
            e_hid,
            make_uuid(50, 2),
            16.5,
            64.0,
            8.0,
        ))
        .unwrap();
    assert_eq!(manager.chunk_visibility(hidden_chunk), Visibility::Hidden);

    // Query entirely inside x=[16.1, 17.5]: reaches section x=0 via CHONKY_ENTITY_SEARCH_GRACE (2).
    let hits = manager.get_entities_in_bounds(AABB::new(16.1, 64.0, 7.5, 17.5, 65.5, 8.5));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].network_id, e_vis);
}

#[test]
fn full_unload_persist_and_reload_restores_uuids_passengers_and_fresh_network_ids() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(3, -2);

    // Load empty chunk into Ticking state.
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Pending);
    manager.tick().unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Loaded);
    assert!(manager.permanent_storage().is_known_empty(chunk));

    // Spawn a boat with a zombie passenger plus a non-persistent marker entity.
    let boat_id = manager.allocate_network_id();
    let zombie_id = manager.allocate_network_id();
    let transient_id = manager.allocate_network_id();
    let boat_uuid = make_uuid(200, 1);
    let zombie_uuid = make_uuid(200, 2);
    let transient_uuid = make_uuid(200, 3);

    let mut boat = make_entity("minecraft:oak_boat", boat_id, boat_uuid, 52.0, 64.0, -24.0);
    let mut zombie = make_entity(
        "minecraft:zombie",
        zombie_id,
        zombie_uuid,
        52.0,
        64.5,
        -24.0,
    );
    zombie.vehicle_id = Some(boat_id);
    boat.passenger_ids.push(zombie_id);
    boat.passengers.push(zombie);

    manager.add_new_entity_with_passengers(boat).unwrap();

    let mut transient = make_entity(
        "minecraft:marker",
        transient_id,
        transient_uuid,
        53.0,
        64.0,
        -24.0,
    );
    transient.persist = false;
    manager.add_new_entity(transient).unwrap();

    assert_eq!(manager.loaded_entity_count(), 3);
    assert!(manager.is_chunk_dirty(chunk));

    // Hide chunk and process unloads: persists boat+zombie, skips non-persistent marker, unloads all 3.
    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    let unloaded = manager.process_unloads().unwrap();
    assert_eq!(unloaded, 1);
    assert_eq!(manager.loaded_entity_count(), 0);
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Fresh);
    assert!(!manager.is_chunk_dirty(chunk));
    assert!(!manager.permanent_storage().is_known_empty(chunk));
    assert_eq!(manager.permanent_storage().backend().write_count(), 1);

    // Reload chunk into Tracked state: boat + zombie restored with original UUIDs and fresh network IDs.
    manager
        .update_chunk_status(chunk, Visibility::Tracked)
        .unwrap();
    manager.process_pending_loads().unwrap();

    assert_eq!(manager.loaded_entity_count(), 2);
    assert!(!manager.is_chunk_dirty(chunk));

    let restored_boat = manager.get_visible_by_uuid(&boat_uuid).unwrap();
    let restored_zombie = manager.get_visible_by_uuid(&zombie_uuid).unwrap();
    assert!(manager.get_visible_by_uuid(&transient_uuid).is_none());

    assert_ne!(restored_boat.network_id, boat_id);
    assert_ne!(restored_zombie.network_id, zombie_id);
    assert_eq!(
        restored_boat.passenger_ids,
        vec![restored_zombie.network_id]
    );
    assert_eq!(restored_zombie.vehicle_id, Some(restored_boat.network_id));
}

#[test]
fn failed_write_on_unload_retains_ownership_until_retry_succeeds() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(0, 0);

    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let id = manager.allocate_network_id();
    let uuid = make_uuid(300, 1);
    manager
        .add_new_entity(make_entity("minecraft:pig", id, uuid, 4.0, 64.0, 4.0))
        .unwrap();

    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    manager
        .permanent_storage_mut()
        .backend_mut()
        .fail_next_write();

    // First unload fails: entity must remain owned in section storage and chunk stays dirty.
    let err = manager.process_unloads().unwrap_err();
    assert!(matches!(err, EntityStorageError::Backend { .. }));
    assert_eq!(manager.loaded_entity_count(), 1);
    assert!(manager.is_chunk_dirty(chunk));
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Loaded);

    // Retry succeeds and cleanly unloads.
    let unloaded = manager.process_unloads().unwrap();
    assert_eq!(unloaded, 1);
    assert_eq!(manager.loaded_entity_count(), 0);
    assert!(!manager.is_chunk_dirty(chunk));
}

#[test]
fn remove_entity_destroys_dirties_and_emits_empty_chunk_tombstone_on_save() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(0, 0);

    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let id = manager.allocate_network_id();
    let uuid = make_uuid(400, 1);
    manager
        .add_new_entity(make_entity("minecraft:pig", id, uuid, 4.0, 64.0, 4.0))
        .unwrap();
    manager.save_all().unwrap();
    assert_eq!(manager.permanent_storage().backend().write_count(), 1);
    assert_eq!(manager.permanent_storage().backend().flush_count(), 1);
    assert!(!manager.is_chunk_dirty(chunk));
    manager.drain_events();

    // Kill the entity: fires TickingEnd, TrackingEnd, Destroyed, marks chunk dirty.
    assert!(manager.remove_entity(id, EntityRemovalReason::Killed));
    assert!(manager.is_chunk_dirty(chunk));
    assert_eq!(
        manager.drain_events(),
        vec![
            EntityLifecycleEvent::TickingEnd(id),
            EntityLifecycleEvent::TrackingEnd(id),
            EntityLifecycleEvent::Destroyed(id),
        ]
    );

    // Save writes STORE_EMPTY (delete_count == 1) and marks chunk known empty.
    manager.save_all().unwrap();
    assert_eq!(manager.permanent_storage().backend().delete_count(), 1);
    assert!(manager.permanent_storage().is_known_empty(chunk));
    assert!(!manager.is_chunk_dirty(chunk));

    // Repeated empty store is a no-op thanks to empty_chunks cache.
    manager
        .permanent_storage_mut()
        .store_entities(&ChunkEntities::empty(chunk))
        .unwrap();
    assert_eq!(manager.permanent_storage().backend().delete_count(), 1);
}

#[test]
fn section_storage_orders_chunk_sections_deterministically_across_negative_and_positive_y() {
    let mut storage = EntitySectionStorage::new();
    let chunk = ChunkPos::new(-2, 5);
    let y_values = [-4, 0, 3, -1, 12];
    for y in y_values {
        let key = SectionPos::of(chunk.x(), y, chunk.z()).as_long_self();
        storage
            .get_or_create_section(key, Visibility::Tracked)
            .add(y);
    }

    let ordered = storage.existing_section_positions_in_chunk(chunk.pack());
    assert_eq!(ordered.len(), y_values.len());
    // Must be strictly ascending by packed SectionPos i64 (matching Paper LongAVLTreeSet).
    for pair in ordered.windows(2) {
        assert!(pair[0] < pair[1]);
    }
    // Non-negative Y precedes negative Y in packed 20-bit low-bits order (0, 3, 12, -4, -1).
    let ys: Vec<i32> = ordered.into_iter().map(SectionPos::y_of).collect();
    assert_eq!(ys, vec![0, 3, 12, -4, -1]);
}

#[test]
fn uuid_less_entities_receive_unique_uuids_across_fresh_allocators() {
    let pos = ChunkPos::new(0, 0);
    let mut entity_tag = CompoundTag::new();
    entity_tag.put_string(TAG_ID, "minecraft:pig");
    let mut list = ListTag::new();
    list.add(Tag::Compound(entity_tag));
    let mut chunk_tag = CompoundTag::new();
    chunk_tag.put_int(TAG_DATA_VERSION, WORLD_VERSION);
    chunk_tag.put_int_array(TAG_POSITION, vec![0, 0]);
    chunk_tag.put(TAG_ENTITIES.to_string(), Tag::List(list));

    let limits = EntitySaveLimits::new();
    let mut alloc_a = NetworkIdAllocator::new();
    let mut alloc_b = NetworkIdAllocator::new();
    let first = decode_chunk_entities_nbt(pos, &chunk_tag, &mut alloc_a, &limits).unwrap();
    let second = decode_chunk_entities_nbt(pos, &chunk_tag, &mut alloc_b, &limits).unwrap();

    assert_ne!(first[0].uuid, second[0].uuid);
}

#[test]
fn storage_load_then_store_round_trip_preserves_passengers() {
    let pos = ChunkPos::new(0, 0);
    let boat_uuid = make_uuid(501, 1);
    let rider_uuid = make_uuid(501, 2);

    let mut boat = make_entity("minecraft:oak_boat", 1, boat_uuid, 4.0, 64.0, 4.0);
    let rider = make_entity("minecraft:zombie", 2, rider_uuid, 4.0, 64.5, 4.0);
    boat.passengers.push(rider);

    let mut storage =
        EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    storage
        .store_entities(&ChunkEntities::new(pos, vec![boat]))
        .unwrap();

    // Load returns flattened [boat, rider]; storing that ChunkEntities back must keep rider.
    let loaded = storage.load_entities(pos).unwrap();
    assert_eq!(loaded.len(), 2);
    storage.store_entities(&loaded).unwrap();

    let reloaded = storage.load_entities(pos).unwrap();
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded.entities()[0].uuid, boat_uuid);
    assert_eq!(reloaded.entities()[1].uuid, rider_uuid);
    assert_eq!(
        reloaded.entities()[0].passenger_ids,
        vec![reloaded.entities()[1].network_id]
    );
    assert_eq!(
        reloaded.entities()[1].vehicle_id,
        Some(reloaded.entities()[0].network_id)
    );
}

#[test]
fn add_new_entity_with_passengers_wires_riding_links_automatically() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(0, 0);
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let boat_id = manager.allocate_network_id();
    let rider_id = manager.allocate_network_id();
    let boat_uuid = make_uuid(502, 1);
    let rider_uuid = make_uuid(502, 2);

    // Caller only nests `rider` in `boat.passengers` without pre-populating ID links.
    let mut boat = make_entity("minecraft:oak_boat", boat_id, boat_uuid, 4.0, 64.0, 4.0);
    let rider = make_entity("minecraft:zombie", rider_id, rider_uuid, 4.0, 64.5, 4.0);
    boat.passengers.push(rider);

    manager.add_new_entity_with_passengers(boat).unwrap();

    let live_boat = manager.get_visible_by_id(boat_id).unwrap();
    let live_rider = manager.get_visible_by_id(rider_id).unwrap();
    assert_eq!(live_boat.passenger_ids, vec![rider_id]);
    assert_eq!(live_rider.vehicle_id, Some(boat_id));

    // Verify the relationship survives save + unload + reload.
    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    manager.process_unloads().unwrap();
    manager
        .update_chunk_status(chunk, Visibility::Tracked)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let reloaded_boat = manager.get_visible_by_uuid(&boat_uuid).unwrap();
    let reloaded_rider = manager.get_visible_by_uuid(&rider_uuid).unwrap();
    assert_eq!(reloaded_boat.passenger_ids, vec![reloaded_rider.network_id]);
    assert_eq!(reloaded_rider.vehicle_id, Some(reloaded_boat.network_id));
}

#[test]
fn pending_chunk_unload_waits_for_load_and_does_not_overwrite_disk_entities() {
    let chunk = ChunkPos::new(0, 0);
    let disk_uuid = make_uuid(601, 1);
    let spawned_uuid = make_uuid(601, 2);

    // Pre-populate disk with one entity.
    let mut storage =
        EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    storage
        .store_entities(&ChunkEntities::new(
            chunk,
            vec![make_entity("minecraft:pig", 1, disk_uuid, 4.0, 64.0, 4.0)],
        ))
        .unwrap();

    let mut manager = PersistentEntitySectionManager::new(storage);
    // Queue load (status = Pending), spawn a new entity before load drains, then hide chunk.
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Pending);

    let spawned_id = manager.allocate_network_id();
    manager
        .add_new_entity(make_entity(
            "minecraft:cow",
            spawned_id,
            spawned_uuid,
            6.0,
            64.0,
            6.0,
        ))
        .unwrap();
    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();

    // Direct process_unloads while Pending must defer without overwriting disk.
    assert_eq!(manager.process_unloads().unwrap(), 0);
    assert_eq!(manager.loaded_entity_count(), 1);

    // tick() drains pending loads first, merges disk + spawned entities, then saves & unloads both.
    manager.tick().unwrap();
    assert_eq!(manager.loaded_entity_count(), 0);
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Fresh);

    // Reloading must contain BOTH the original disk entity and the newly spawned entity.
    manager
        .update_chunk_status(chunk, Visibility::Tracked)
        .unwrap();
    manager.process_pending_loads().unwrap();
    assert_eq!(manager.loaded_entity_count(), 2);
    assert!(manager.get_visible_by_uuid(&disk_uuid).is_some());
    assert!(manager.get_visible_by_uuid(&spawned_uuid).is_some());
}

#[test]
fn fresh_chunk_unload_cleans_up_nonpersistent_entities() {
    let storage = EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    let mut manager = PersistentEntitySectionManager::new(storage);
    let chunk = ChunkPos::new(2, 2);

    let id = manager.allocate_network_id();
    let mut transient = make_entity("minecraft:marker", id, make_uuid(602, 1), 36.0, 64.0, 36.0);
    transient.persist = false;
    manager.add_new_entity(transient).unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Fresh);
    assert_eq!(manager.loaded_entity_count(), 1);

    manager
        .update_chunk_status(chunk, Visibility::Hidden)
        .unwrap();
    assert_eq!(manager.process_unloads().unwrap(), 1);
    assert_eq!(manager.loaded_entity_count(), 0);
    assert!(!manager.is_chunk_dirty(chunk));
}

#[test]
fn save_all_loads_merges_and_persists_newly_populated_fresh_chunks() {
    let chunk = ChunkPos::new(4, -1);
    let existing_uuid = make_uuid(603, 1);
    let fresh_uuid = make_uuid(603, 2);

    let mut storage =
        EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    storage
        .store_entities(&ChunkEntities::new(
            chunk,
            vec![make_entity(
                "minecraft:pig",
                1,
                existing_uuid,
                68.0,
                64.0,
                -8.0,
            )],
        ))
        .unwrap();

    let mut manager = PersistentEntitySectionManager::new(storage);
    // Chunk is Fresh; spawn a new entity into it and immediately call save_all().
    let fresh_id = manager.allocate_network_id();
    manager
        .add_new_entity(make_entity(
            "minecraft:sheep",
            fresh_id,
            fresh_uuid,
            70.0,
            64.0,
            -8.0,
        ))
        .unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Fresh);

    let saved = manager.save_all().unwrap();
    assert_eq!(saved, 1);
    assert!(!manager.is_chunk_dirty(chunk));
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Loaded);

    // Reloading directly from backend must contain both the prior disk entity and the fresh one.
    let persisted = manager
        .permanent_storage_mut()
        .load_entities(chunk)
        .unwrap();
    assert_eq!(persisted.len(), 2);
    let uuids: Vec<Uuid> = persisted.entities().iter().map(|e| e.uuid).collect();
    assert!(uuids.contains(&existing_uuid));
    assert!(uuids.contains(&fresh_uuid));
}

#[test]
fn failed_chunk_read_leaves_status_fresh_and_recovers_on_retry() {
    let chunk = ChunkPos::new(1, -4);
    let uuid = make_uuid(604, 1);

    let mut storage =
        EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    storage
        .store_entities(&ChunkEntities::new(
            chunk,
            vec![make_entity("minecraft:cow", 1, uuid, 20.0, 64.0, -60.0)],
        ))
        .unwrap();
    storage.backend_mut().fail_next_read();

    let mut manager = PersistentEntitySectionManager::new(storage);
    let err = manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap_err();
    assert!(matches!(err, EntityStorageError::Backend { .. }));
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Fresh);

    // Retry once backend recovers: chunk loads normally and restores entity.
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Pending);
    manager.process_pending_loads().unwrap();
    assert_eq!(manager.chunk_load_status(chunk), ChunkLoadStatus::Loaded);
    assert!(manager.get_visible_by_uuid(&uuid).is_some());
}

#[test]
fn duplicate_vehicle_uuid_on_load_detaches_unique_passenger_for_saving() {
    let chunk = ChunkPos::new(0, 0);
    let boat_uuid = make_uuid(605, 1);
    let rider_uuid = make_uuid(605, 2);

    // Pre-populate disk with a boat carrying a zombie passenger.
    let mut disk_boat = make_entity("minecraft:oak_boat", 1, boat_uuid, 4.0, 64.0, 4.0);
    let disk_rider = make_entity("minecraft:zombie", 2, rider_uuid, 4.0, 64.5, 4.0);
    disk_boat.passengers.push(disk_rider);

    let mut storage =
        EntityStorage::new(InMemoryEntityRegionBackend::new(), EntitySaveLimits::new());
    storage
        .store_entities(&ChunkEntities::new(chunk, vec![disk_boat]))
        .unwrap();

    let mut manager = PersistentEntitySectionManager::new(storage);
    // Register a boat with the SAME UUID in another chunk so the disk boat is skipped on load.
    let other_chunk = ChunkPos::new(5, 5);
    let existing_boat_id = manager.allocate_network_id();
    manager
        .add_new_entity(make_entity(
            "minecraft:oak_boat",
            existing_boat_id,
            boat_uuid,
            84.0,
            64.0,
            84.0,
        ))
        .unwrap();
    assert_eq!(manager.chunk_visibility(other_chunk), Visibility::Hidden);

    // Load chunk: disk boat is skipped as duplicate, unique passenger is admitted detached.
    manager
        .update_chunk_status(chunk, Visibility::Ticking)
        .unwrap();
    manager.process_pending_loads().unwrap();

    let rider = manager.get_visible_by_uuid(&rider_uuid).unwrap();
    assert_eq!(rider.vehicle_id, None);

    // Saving and reloading chunk must preserve the detached passenger rather than dropping it.
    manager.save_all().unwrap();
    let persisted = manager
        .permanent_storage_mut()
        .load_entities(chunk)
        .unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted.entities()[0].uuid, rider_uuid);
}
