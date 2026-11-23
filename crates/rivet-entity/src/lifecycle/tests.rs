//! Comprehensive unit tests for `Entity`, `LivingEntity`, `Mob`, and passenger lifecycle
//! grounded in Paper 26.2 (`0a993450f129c4942c2a9ed45ba047412b4667cf`).

use super::*;
use crate::damage::Difficulty;
use crate::storage::{Aabb, EntityRemovalReason};
use rivet_registry::core::{BlockPos, Vec3};

fn pig_meta() -> EntityTypeMeta {
    EntityTypeMeta::new("minecraft:pig", 0.9, 0.9)
}

fn zombie_meta() -> EntityTypeMeta {
    EntityTypeMeta::hostile("minecraft:zombie", 0.6, 1.95)
}

// ============================================================================
// 1. Dimensions, Pose, and Attachments
// ============================================================================

#[test]
fn dimensions_bounding_box_eye_height_and_scaling() {
    let dim = EntityDimensions::scalable(0.6, 1.8);
    assert!((dim.eye_height - 1.53_f32).abs() < 1e-6);
    assert_eq!(
        dim.make_bounding_box(Vec3::new(10.0, 64.0, -5.0)),
        Aabb::new(
            10.0 - f64::from(0.3_f32),
            64.0,
            -5.0 - f64::from(0.3_f32),
            10.0 + f64::from(0.3_f32),
            64.0 + f64::from(1.8_f32),
            -5.0 + f64::from(0.3_f32),
        )
    );

    let scaled = dim.scale(0.5);
    assert!((scaled.width - 0.3_f32).abs() < 1e-6);
    assert!((scaled.height - 0.9_f32).abs() < 1e-6);
    assert!((scaled.eye_height - 0.765_f32).abs() < 1e-6);

    // Fixed dimensions must NOT scale (Paper EntityDimensions#scale)
    let fixed = EntityDimensions::fixed(0.2, 0.2);
    assert_eq!(fixed.scale(2.0), fixed);
}

#[test]
fn attachments_fallback_and_y_rotation() {
    let dim = EntityDimensions::scalable(1.0, 2.0);
    assert_eq!(
        dim.attachments.get(EntityAttachment::Passenger, 0, 0.0),
        Vec3::new(0.0, 2.0, 0.0)
    );
    assert_eq!(
        dim.attachments.get(EntityAttachment::Vehicle, 0, 0.0),
        Vec3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        dim.attachments.get(EntityAttachment::WardenChest, 0, 0.0),
        Vec3::new(0.0, 1.0, 0.0)
    );
    assert!(
        dim.attachments
            .get_nullable(EntityAttachment::Passenger, 1, 0.0)
            .is_none()
    );

    // Custom attachment rotated by 90 degrees yaw: point (0, 0, 1) -> yRot(-90 deg) -> (-1, 0, 0)
    let custom = EntityAttachments::builder()
        .attach_xyz(EntityAttachment::Passenger, 0.0, 0.5, 1.0)
        .build(1.0, 2.0);
    let rotated = custom.get(EntityAttachment::Passenger, 0, 90.0);
    assert!((rotated.x - (-1.0)).abs() < 1e-3);
    assert!((rotated.y - 0.5).abs() < 1e-6);
    assert!(rotated.z.abs() < 1e-3);
}

#[test]
fn pose_ids_and_refresh_dimensions_growth() {
    assert_eq!(Pose::by_id(0), Pose::Standing);
    assert_eq!(Pose::by_id(5), Pose::Crouching);
    assert_eq!(Pose::by_id(17), Pose::Inhaling);
    assert_eq!(Pose::by_id(999), Pose::Standing);
    assert_eq!(Pose::by_id(-1), Pose::Standing);

    // SLEEPING_DIMENSIONS is a LivingEntity#getDimensions override.
    let mut living = LivingEntity::new(1, zombie_meta(), 20.0, 42);
    let entity = &mut living.entity;
    entity.first_tick = false;
    assert!(!entity.set_pose(Pose::Sleeping));
    assert!((entity.dimensions.height - 0.2_f32).abs() < 1e-6);
    assert!((entity.eye_height - 0.2_f32).abs() < 1e-6);

    // Growing back from Sleeping (0.2x0.2) to Standing (0.6x1.95) triggers growth flag immediately
    assert!(entity.set_pose(Pose::Standing));
    assert!((entity.dimensions.height - 1.95_f32).abs() < 1e-6);
    assert!((entity.eye_height - 1.95_f32 * 0.85_f32).abs() < 1e-5);

    // Base Entity#getDimensions(pose) always returns the type dimensions.
    let mut boat = Entity::new(3, EntityTypeMeta::boat(), 44);
    boat.first_tick = false;
    assert!(!boat.set_pose(Pose::Sleeping));
    assert!((boat.dimensions.width - 1.375_f32).abs() < 1e-6);
    assert!((boat.dimensions.height - 0.5625_f32).abs() < 1e-6);
    assert!((boat.eye_height - 0.5625_f32).abs() < 1e-6);

    // Players never trigger the non-player growth fudge
    let mut player = LivingEntity::new(2, EntityTypeMeta::player(), 20.0, 43);
    player.entity.first_tick = false;
    assert!(!player.entity.set_pose(Pose::Sleeping));
    assert!((player.entity.dimensions.height - 0.2_f32).abs() < 1e-6);
    assert!(!player.entity.set_pose(Pose::Standing));

    // start_riding resets a sleeping entity to Standing and refreshes its dimensions immediately
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let rider_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        10,
    )));
    let mut rider = Mob::new(rider_id, zombie_meta(), 20.0, 11);
    rider.living.entity.set_pose(Pose::Sleeping);
    assert!((rider.living.entity.dimensions.height - 0.2_f32).abs() < 1e-6);
    ctx.add_entity(LifecycleEntity::Mob(rider));
    assert!(ctx.start_riding(rider_id, boat_id, false, false));
    let rider_after = ctx.get(rider_id).unwrap().entity();
    assert_eq!(rider_after.pose, Pose::Standing);
    assert!((rider_after.dimensions.height - 1.95_f32).abs() < 1e-6);
    assert!((rider_after.eye_height - 1.95_f32 * 0.85_f32).abs() < 1e-5);
}

// ============================================================================
// 2. Shared Flags and Mob Flags
// ============================================================================

#[test]
fn entity_and_mob_flags_bitwise_semantics() {
    let mut mob = Mob::new(1, zombie_meta(), 20.0, 100);
    mob.living.entity.set_sprinting(true);
    mob.living.entity.set_glowing_tag(true);
    mob.living.entity.set_shift_key_down(true);
    assert!(mob.living.entity.is_sprinting());
    assert!(mob.living.entity.is_currently_glowing());
    assert!(mob.living.entity.is_shift_key_down());
    assert!(!mob.living.entity.is_invisible());
    assert_eq!(
        mob.living.entity.shared_flags,
        (1 << FLAG_SPRINTING) | (1 << FLAG_GLOWING) | (1 << FLAG_SHIFT_KEY_DOWN)
    );
    mob.living.entity.set_sprinting(false);
    assert!(!mob.living.entity.is_sprinting());
    assert!(mob.living.entity.is_currently_glowing());

    // Mob flags: NO_AI=1, LEFTHANDED=2, AGGRESSIVE=4
    assert_eq!(mob.main_arm(), HumanoidArm::Right);
    mob.set_left_handed(true);
    assert_eq!(mob.main_arm(), HumanoidArm::Left);
    mob.set_no_ai(true);
    mob.set_aggressive(true);
    assert_eq!(
        mob.mob_flags,
        MOB_FLAG_NO_AI | MOB_FLAG_LEFTHANDED | MOB_FLAG_AGGRESSIVE
    );
    assert!(!mob.is_effective_ai());
    mob.set_no_ai(false);
    assert!(mob.is_effective_ai());
    assert!(mob.is_aggressive());
    assert!(mob.is_left_handed());
}

// ============================================================================
// 3. Parent-Call Order: Mob -> LivingEntity -> Entity
// ============================================================================

#[test]
fn mob_tick_preserves_exact_paper_parent_call_order() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mob = Mob::new(id, zombie_meta(), 20.0, 7);
    ctx.add_entity(LifecycleEntity::Mob(mob));
    ctx.take_events();

    ctx.tick_all(true);
    let events = ctx.take_events();

    // Expected exact order for tick 1:
    // EntityTickStart -> EntityBaseTick -> LivingBaseTick -> ChangedBlock -> MobBaseTick
    // -> LivingTick -> LivingAiStep -> MobServerAiStep -> MobTick -> EntityPostTick
    let idx = |pred: fn(&LifecycleEvent) -> bool| -> usize {
        events
            .iter()
            .position(pred)
            .expect("expected lifecycle event missing")
    };
    let i_start = idx(|e| {
        matches!(
            e,
            LifecycleEvent::EntityTickStart {
                as_passenger: false,
                ..
            }
        )
    });
    let i_ebase = idx(|e| matches!(e, LifecycleEvent::EntityBaseTick { .. }));
    let i_lbase = idx(|e| matches!(e, LifecycleEvent::LivingBaseTick { .. }));
    let i_mbase = idx(|e| matches!(e, LifecycleEvent::MobBaseTick { .. }));
    let i_ltick = idx(|e| matches!(e, LifecycleEvent::LivingTick { .. }));
    let i_ai = idx(|e| matches!(e, LifecycleEvent::LivingAiStep { .. }));
    let i_sai = idx(|e| matches!(e, LifecycleEvent::MobServerAiStep { .. }));
    let i_mtick = idx(|e| matches!(e, LifecycleEvent::MobTick { .. }));
    let i_post = idx(|e| matches!(e, LifecycleEvent::EntityPostTick { .. }));

    assert!(i_start < i_ebase);
    assert!(i_ebase < i_lbase);
    assert!(i_lbase < i_mbase);
    assert!(i_mbase < i_ltick);
    assert!(i_ltick < i_ai);
    assert!(i_ai < i_sai);
    assert!(i_sai < i_mtick);
    assert!(i_mtick < i_post);
}

#[test]
fn mob_selector_alternation_and_control_flags_every_five_ticks() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id(); // id == 1
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(id, zombie_meta(), 20.0, 99)));
    ctx.take_events();

    let mut selector_ticks = Vec::new();
    let mut control_flag_ticks = Vec::new();

    for tick in 1..=6 {
        ctx.tick_all(true);
        for ev in ctx.take_events() {
            match ev {
                LifecycleEvent::MobServerAiStep {
                    full_selector_tick, ..
                } => selector_ticks.push((tick, full_selector_tick)),
                LifecycleEvent::MobControlFlagsUpdated { .. } => control_flag_ticks.push(tick),
                _ => {}
            }
        }
    }

    // For id = 1:
    // tick 1: tickCount <= 1 -> full (true)
    // tick 2: (2 + 1) % 2 == 1 -> running only (false)
    // tick 3: (3 + 1) % 2 == 0 -> full (true)
    // tick 4: (4 + 1) % 2 == 1 -> running only (false)
    // tick 5: (5 + 1) % 2 == 0 -> full (true)
    // tick 6: (6 + 1) % 2 == 1 -> running only (false)
    assert_eq!(
        selector_ticks,
        vec![
            (1, true),
            (2, false),
            (3, true),
            (4, false),
            (5, true),
            (6, false)
        ]
    );
    // updateControlFlags runs only when tickCount % 5 == 0
    assert_eq!(control_flag_ticks, vec![5]);

    // A player rider is not a Mob, so Mob#getControllingPassenger() is null and
    // move/jump/look stay enabled.
    let player_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Living(LivingEntity::new(
        player_id,
        EntityTypeMeta::player(),
        20.0,
        2,
    )));
    assert!(ctx.start_riding(player_id, id, false, false));
    for _ in 0..4 {
        ctx.tick_all(true);
    }
    assert_eq!(
        ctx.get(id).unwrap().mob().unwrap().control_flags,
        MobControlFlags::default()
    );

    // A Mob rider that can control vehicles disables move/jump/look.
    assert!(ctx.stop_riding(player_id, false));
    let rider_mob_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(
        rider_mob_id,
        zombie_meta(),
        20.0,
        3,
    )));
    assert!(ctx.start_riding(rider_mob_id, id, false, false));
    for _ in 0..5 {
        ctx.tick_all(true);
    }
    let flags = ctx.get(id).unwrap().mob().unwrap().control_flags;
    assert!(!flags.move_enabled);
    assert!(!flags.jump_enabled);
    assert!(!flags.look_enabled);

    // A NoAI vehicle has no controlling passenger.
    ctx.get_mut(id).unwrap().mob_mut().unwrap().set_no_ai(true);
    for _ in 0..5 {
        ctx.tick_all(true);
    }
    assert_eq!(
        ctx.get(id).unwrap().mob().unwrap().control_flags,
        MobControlFlags::default()
    );
}

// ============================================================================
// 4. Equipment Hooks and Hand Swap Detection
// ============================================================================

#[test]
fn equipment_slot_ids_and_hand_swap_detection() {
    assert_eq!(EquipmentSlot::MainHand.id(), 0);
    assert_eq!(EquipmentSlot::Feet.id(), 1);
    assert_eq!(EquipmentSlot::OffHand.id(), 5);
    assert_eq!(EquipmentSlot::Saddle.id(), 7);
    assert_eq!(EquipmentSlot::by_id(5), EquipmentSlot::OffHand);
    assert_eq!(EquipmentSlot::by_name("chest"), Some(EquipmentSlot::Chest));
    assert!(EquipmentSlot::Chest.is_armor());
    assert!(!EquipmentSlot::Saddle.can_increase_experience());

    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mut living = LivingEntity::new(id, zombie_meta(), 20.0, 11);
    living.entity.first_tick = false;

    let sword = EquipmentItem::new("minecraft:iron_sword", 1);
    let shield = EquipmentItem::new("minecraft:shield", 1);
    living.set_item_slot(EquipmentSlot::MainHand, sword.clone(), false, &mut ctx);
    living.set_item_slot(EquipmentSlot::OffHand, shield.clone(), true, &mut ctx);
    ctx.add_entity(LifecycleEntity::Living(living));
    ctx.take_events();

    // First tick: detectEquipmentUpdates emits EquipmentChanged for MainHand + OffHand
    ctx.tick_all(true);
    let evs1 = ctx.take_events();
    let changed_ev = evs1
        .iter()
        .find_map(|e| match e {
            LifecycleEvent::EquipmentChanged { changes, .. } => Some(changes.clone()),
            _ => None,
        })
        .expect("expected EquipmentChanged");
    assert_eq!(changed_ev.len(), 2);
    assert_eq!(changed_ev[0].slot, EquipmentSlot::MainHand);
    assert_eq!(changed_ev[1].slot, EquipmentSlot::OffHand);

    // Now swap MainHand and OffHand: should emit HandSwap and NOT EquipmentChanged
    if let Some(LifecycleEntity::Living(l)) = ctx.get_mut(id) {
        l.equipment.set(EquipmentSlot::MainHand, shield);
        l.equipment.set(EquipmentSlot::OffHand, sword);
    }
    ctx.tick_all(true);
    let evs2 = ctx.take_events();
    assert!(
        evs2.iter()
            .any(|e| matches!(e, LifecycleEvent::HandSwap { id: eid } if *eid == id))
    );
    assert!(
        !evs2
            .iter()
            .any(|e| matches!(e, LifecycleEvent::EquipmentChanged { .. }))
    );
}

// ============================================================================
// 5. Passengers, Vehicles, Multi-Level Traversal, and Edge Ordering
// ============================================================================

#[test]
fn multi_level_passenger_traversal_and_position_rider() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let pig_id = ctx.next_entity_id();
    let player_id = ctx.next_entity_id();

    let mut boat = Entity::new(boat_id, EntityTypeMeta::boat(), 1);
    boat.set_pos(10.0, 64.0, 20.0);
    let pig = Mob::new(pig_id, pig_meta(), 10.0, 2);
    let player = LivingEntity::new(player_id, EntityTypeMeta::player(), 20.0, 3);

    ctx.add_entity(LifecycleEntity::Base(boat));
    ctx.add_entity(LifecycleEntity::Mob(pig));
    ctx.add_entity(LifecycleEntity::Living(player));

    assert!(ctx.start_riding(pig_id, boat_id, false, true));
    // Nested passenger: player rides pig while pig rides boat
    assert!(ctx.start_riding(player_id, pig_id, false, true));

    assert_eq!(ctx.root_vehicle(player_id), Some(boat_id));
    assert!(ctx.is_passenger_of_same_vehicle(pig_id, player_id));
    assert!(ctx.has_indirect_passenger(boat_id, player_id));
    assert_eq!(
        ctx.self_and_passengers(boat_id),
        vec![boat_id, pig_id, player_id]
    );

    ctx.take_events();
    ctx.tick_all(true);
    let events = ctx.take_events();

    // Tick start order must be: boat (non-passenger) -> pig (passenger) -> player (passenger)
    let tick_starts: Vec<(i32, bool)> = events
        .iter()
        .filter_map(|e| match e {
            LifecycleEvent::EntityTickStart { id, as_passenger } => Some((*id, *as_passenger)),
            _ => None,
        })
        .collect();
    assert_eq!(
        tick_starts,
        vec![(boat_id, false), (pig_id, true), (player_id, true)]
    );

    // Pig was positioned on top of boat (y = 64.0 + boat height 0.5625)
    let pig_pos = ctx.get(pig_id).unwrap().entity().pos;
    assert!((pig_pos.x - 10.0).abs() < 1e-6);
    assert!((pig_pos.y - (64.0 + f64::from(0.5625_f32))).abs() < 1e-6);
    assert!((pig_pos.z - 20.0).abs() < 1e-6);

    // Player was positioned on top of pig (y = pig_pos.y + pig height 0.9), minus the player
    // VEHICLE attachment (Avatar.DEFAULT_VEHICLE_ATTACHMENT = (0, 0.6, 0))
    let player_pos = ctx.get(player_id).unwrap().entity().pos;
    assert!((player_pos.y - (pig_pos.y + f64::from(0.9_f32) - 0.6)).abs() < 1e-6);
}

#[test]
fn player_priority_insertion_and_reverse_order_eject() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let mob_id = ctx.next_entity_id();
    let player_id = ctx.next_entity_id();

    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(mob_id, pig_meta(), 10.0, 2)));
    ctx.add_entity(LifecycleEntity::Living(LivingEntity::new(
        player_id,
        EntityTypeMeta::player(),
        20.0,
        3,
    )));

    // Mob boards first, then player boards: Paper inserts player at index 0
    assert!(ctx.start_riding(mob_id, boat_id, false, true));
    assert!(ctx.start_riding(player_id, boat_id, false, true));
    assert_eq!(
        ctx.get(boat_id).unwrap().entity().passengers,
        vec![player_id, mob_id]
    );

    ctx.take_events();
    ctx.eject_passengers(boat_id);
    let dismounts: Vec<i32> = ctx
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            LifecycleEvent::Dismount { passenger_id, .. } => Some(passenger_id),
            _ => None,
        })
        .collect();
    // Paper Entity#ejectPassengers iterates from end to start
    assert_eq!(dismounts, vec![mob_id, player_id]);
    assert_eq!(
        ctx.get(mob_id).unwrap().entity().boarding_cooldown,
        BOARDING_COOLDOWN
    );
}

#[test]
fn removal_preserves_passengers_on_chunk_unload_but_ejects_on_discard() {
    let mut ctx = LevelCtx::new();
    let boat1 = ctx.next_entity_id();
    let pig1 = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat1,
        EntityTypeMeta::boat(),
        1,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig1, pig_meta(), 10.0, 2)));
    assert!(ctx.start_riding(pig1, boat1, false, false));

    // UnloadedToChunk must NOT eject passengers, and removes the unloaded boat from level storage
    assert!(ctx.remove_entity(boat1, EntityRemovalReason::UnloadedToChunk));
    assert_eq!(ctx.get(pig1).unwrap().entity().vehicle, Some(boat1));
    assert!(ctx.get(boat1).is_none());

    // Discarded MUST eject passengers and purge the discarded boat from level storage
    let boat2 = ctx.next_entity_id();
    let pig2 = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat2,
        EntityTypeMeta::boat(),
        3,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig2, pig_meta(), 10.0, 4)));
    assert!(ctx.start_riding(pig2, boat2, false, false));

    assert!(ctx.remove_entity(boat2, EntityRemovalReason::Discarded));
    assert_eq!(ctx.get(pig2).unwrap().entity().vehicle, None);
    assert!(ctx.get(boat2).is_none());
    assert_eq!(ctx.entity_count(), 2);

    // If a passenger is removed while cancel_dismount is active, once cancellation clears
    // the next tick detaches the missing passenger ID so the vehicle seat is freed.
    let boat3 = ctx.next_entity_id();
    let pig3 = ctx.next_entity_id();
    let pig4 = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat3,
        EntityTypeMeta::boat(),
        5,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig3, pig_meta(), 10.0, 6)));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig4, pig_meta(), 10.0, 7)));
    assert!(ctx.start_riding(pig3, boat3, false, false));

    ctx.cancel_dismount = true;
    assert!(ctx.remove_entity(pig3, EntityRemovalReason::Discarded));
    assert!(ctx.get(pig3).is_none());
    assert_eq!(ctx.get(boat3).unwrap().entity().passengers, vec![pig3]);

    ctx.cancel_dismount = false;
    ctx.take_events();
    ctx.tick_all(true);
    assert!(ctx.get(boat3).unwrap().entity().passengers.is_empty());
    assert!(ctx.start_riding(pig4, boat3, false, false));
}

// ============================================================================
// 6. Drowning, Fire, Death, Despawn, and Rotation Wrapping
// ============================================================================

#[test]
fn drowning_death_timer_and_peaceful_despawn() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mut living = LivingEntity::new(id, zombie_meta(), 20.0, 5);
    living.entity.eye_in_water = true;
    living.entity.air_supply = -19; // one tick away from -20
    ctx.add_entity(LifecycleEntity::Living(living));
    ctx.take_events();

    ctx.tick_all(true);
    let evs = ctx.take_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, LifecycleEvent::DrownParticles { id: eid } if *eid == id))
    );
    assert!(
        evs.iter().any(
            |e| matches!(e, LifecycleEvent::Hurt { id: eid, source: "drown", .. } if *eid == id)
        )
    );
    assert_eq!(ctx.get(id).unwrap().entity().air_supply, 0);

    // Peaceful despawn for hostile mob runs automatically during tick_all
    ctx.difficulty = Difficulty::Peaceful;
    let mob_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(
        mob_id,
        zombie_meta(),
        20.0,
        6,
    )));
    ctx.take_events();
    ctx.tick_all(true);
    let despawn_evs = ctx.take_events();
    assert!(
        despawn_evs
            .iter()
            .any(|e| matches!(e, LifecycleEvent::Despawn { id } if *id == mob_id))
    );
    assert!(despawn_evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::EntityRemoved {
            id,
            reason: EntityRemovalReason::Discarded,
        } if *id == mob_id
    )));
    // Paper's entity loop does not re-check isRemoved() after checkDespawn(): the
    // discarded mob still gets this tick, after its removal, and is purged afterwards.
    let removed_idx = despawn_evs
        .iter()
        .position(|e| matches!(e, LifecycleEvent::EntityRemoved { id, .. } if *id == mob_id))
        .unwrap();
    let tick_idx = despawn_evs
        .iter()
        .position(|e| matches!(e, LifecycleEvent::EntityTickStart { id, .. } if *id == mob_id))
        .expect("despawned mob still ticks once");
    assert!(removed_idx < tick_idx);
    assert!(ctx.get(mob_id).is_none());
}

#[test]
fn automatic_removal_runs_lifecycle_cleanup_and_ejects_passengers() {
    let mut ctx = LevelCtx::new();
    let pig_id = ctx.next_entity_id();
    let rider_id = ctx.next_entity_id();

    let mut pig = Mob::new(pig_id, pig_meta(), 10.0, 30);
    pig.living.health = 0.0;
    pig.living.death_time = 19;
    ctx.add_entity(LifecycleEntity::Mob(pig));
    ctx.add_entity(LifecycleEntity::Living(LivingEntity::new(
        rider_id,
        zombie_meta(),
        20.0,
        31,
    )));
    assert!(ctx.start_riding(rider_id, pig_id, false, false));
    ctx.take_events();

    ctx.tick_all(true);

    // Dying vehicle at death_time == 20 must eject its passenger, emit EntityRemoved, and be purged
    assert_eq!(ctx.get(rider_id).unwrap().entity().vehicle, None);
    assert!(ctx.get(pig_id).is_none());
    assert_eq!(ctx.entity_count(), 1);
    let evs = ctx.take_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::Dismount { passenger_id, vehicle_id }
            if *passenger_id == rider_id && *vehicle_id == pig_id
    )));
    assert!(evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::EntityRemoved {
            id,
            reason: EntityRemovalReason::Killed,
        } if *id == pig_id
    )));

    // Paper setRemoved runs synchronously inside tickDeath: DeathTick, then the rider's
    // dismount, then the removal callback, and only then the rest of LivingEntity#tick.
    let pos = |pred: &dyn Fn(&LifecycleEvent) -> bool| evs.iter().position(pred).unwrap();
    let death = pos(&|e| matches!(e, LifecycleEvent::DeathTick { id, .. } if *id == pig_id));
    let dismount =
        pos(&|e| matches!(e, LifecycleEvent::Dismount { vehicle_id, .. } if *vehicle_id == pig_id));
    let removed = pos(&|e| matches!(e, LifecycleEvent::EntityRemoved { id, .. } if *id == pig_id));
    let living_tick = pos(&|e| matches!(e, LifecycleEvent::LivingTick { id } if *id == pig_id));
    assert!(death < dismount && dismount < removed && removed < living_tick);
}

#[test]
fn sleeping_snap_on_first_tick_and_bed_loss_wakes_entity() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mut living = LivingEntity::new(id, EntityTypeMeta::player(), 20.0, 8);
    living.sleeping_pos = Some(BlockPos::new(4, 64, -2));
    living.bed_exists = true;
    living.entity.x_rot = 45.0;
    ctx.add_entity(LifecycleEntity::Living(living));

    ctx.tick_all(true);
    let e = ctx.get(id).unwrap().entity();
    assert!((e.pos.x - 4.5).abs() < 1e-6);
    assert!((e.pos.y - 64.6875).abs() < 1e-6);
    assert!((e.pos.z - (-1.5)).abs() < 1e-6);
    assert_eq!(e.x_rot, 0.0_f32);

    // Losing the bed wakes the entity on the next tick
    ctx.get_mut(id).unwrap().living_mut().unwrap().bed_exists = false;
    ctx.tick_all(true);
    assert!(!ctx.get(id).unwrap().living().unwrap().is_sleeping());
}

// ============================================================================
// 7. Controlled Negative Cases
// ============================================================================

#[test]
fn negative_riding_cycle_self_ride_cooldown_and_full_vehicle_rejected() {
    let mut ctx = LevelCtx::new();
    let a = ctx.next_entity_id();
    let b = ctx.next_entity_id();
    let c = ctx.next_entity_id();

    ctx.add_entity(LifecycleEntity::Mob(Mob::new(a, pig_meta(), 10.0, 1)));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(b, pig_meta(), 10.0, 2)));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(c, pig_meta(), 10.0, 3)));

    // Self-ride rejected
    assert!(!ctx.start_riding(a, a, true, true));

    // A rides B; B trying to ride A would create a cycle -> rejected even with force=true
    assert!(ctx.start_riding(a, b, false, true));
    assert!(!ctx.start_riding(b, a, true, true));

    // Vehicle full (pig max_passengers == 1): C cannot ride B without force
    assert!(!ctx.start_riding(c, b, false, true));

    // Boarding cooldown blocks riding without force
    ctx.get_mut(c).unwrap().entity_mut().boarding_cooldown = 10;
    assert!(!ctx.start_riding(c, a, false, true));

    // Shift-key-down blocks riding without force
    ctx.get_mut(c).unwrap().entity_mut().boarding_cooldown = 0;
    ctx.get_mut(c)
        .unwrap()
        .entity_mut()
        .set_shift_key_down(true);
    assert!(!ctx.start_riding(c, a, false, true));
}

#[test]
fn negative_cancelled_dismount_moonrise_lock_no_ai_and_double_take() {
    let mut ctx = LevelCtx::new();
    let boat = ctx.next_entity_id();
    let mob_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat,
        EntityTypeMeta::boat(),
        1,
    )));
    let mut mob = Mob::new(mob_id, zombie_meta(), 20.0, 2);
    mob.set_no_ai(true);
    ctx.add_entity(LifecycleEntity::Mob(mob));

    assert!(ctx.start_riding(mob_id, boat, false, true));

    // Cancelled dismount restores passenger.vehicle when suppress_cancellation=false
    ctx.cancel_dismount = true;
    assert!(!ctx.stop_riding(mob_id, false));
    assert_eq!(ctx.get(mob_id).unwrap().entity().vehicle, Some(boat));

    // Underwater dismount in LivingEntity::baseTick must also respect cancel_dismount
    let pig_id = ctx.next_entity_id();
    let rider_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig_id, pig_meta(), 10.0, 20)));
    let mut submerged_rider = LivingEntity::new(rider_id, zombie_meta(), 20.0, 21);
    submerged_rider.entity.eye_in_water = true;
    ctx.add_entity(LifecycleEntity::Living(submerged_rider));
    assert!(ctx.start_riding(rider_id, pig_id, false, false));
    ctx.take_events();
    ctx.tick_all(true);
    assert_eq!(ctx.get(rider_id).unwrap().entity().vehicle, Some(pig_id));
    assert_eq!(ctx.get(pig_id).unwrap().entity().passengers, vec![rider_id]);
    // Entity#removeVehicle still fires GameEvent.ENTITY_DISMOUNT after a cancelled dismount.
    assert!(ctx.take_events().iter().any(|e| matches!(
        e,
        LifecycleEvent::Dismount { passenger_id, .. } if *passenger_id == rider_id
    )));

    // Once cancellation is cleared, underwater dismount succeeds and emits Dismount
    ctx.cancel_dismount = false;
    ctx.tick_all(true);
    assert_eq!(ctx.get(rider_id).unwrap().entity().vehicle, None);
    assert!(ctx.take_events().iter().any(|e| matches!(
        e,
        LifecycleEvent::Dismount { passenger_id, vehicle_id }
            if *passenger_id == rider_id && *vehicle_id == pig_id
    )));

    // suppress_cancellation=true overrides cancellation
    ctx.cancel_dismount = true;
    assert!(ctx.stop_riding(mob_id, true));
    assert_eq!(ctx.get(mob_id).unwrap().entity().vehicle, None);
    ctx.cancel_dismount = false;

    // Moonrise section_update_lock prevents removal
    ctx.section_update_lock.insert(mob_id);
    assert!(!ctx.remove_entity(mob_id, EntityRemovalReason::Discarded));
    assert!(!ctx.get(mob_id).unwrap().entity().is_removed());
    ctx.section_update_lock.remove(&mob_id);

    // NoAI mob skips MobServerAiStep during tick
    ctx.take_events();
    ctx.tick_all(true);
    let evs = ctx.take_events();
    assert!(
        !evs.iter()
            .any(|e| matches!(e, LifecycleEvent::MobServerAiStep { id, .. } if *id == mob_id))
    );

    // Double-take protection in LevelCtx preserves IndexMap order on putback
    let order_before = ctx.entity_ids();
    let taken = ctx.take_entity(boat).expect("first take succeeds");
    assert!(ctx.is_taken(boat));
    assert!(ctx.take_entity(boat).is_none());
    ctx.put_back_entity(taken);
    assert!(!ctx.is_taken(boat));
    assert_eq!(ctx.entity_ids(), order_before);
}

#[test]
fn living_base_tick_preserves_self_hurt_ids_while_taken() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mut living = LivingEntity::new(id, zombie_meta(), 20.0, 40);
    living.last_hurt_mob = Some(id);
    living.set_last_hurt_by_mob(Some(id));
    ctx.add_entity(LifecycleEntity::Living(living));

    ctx.tick_all(true);
    let l = ctx.get(id).unwrap().living().unwrap();
    assert_eq!(l.last_hurt_mob, Some(id));
    assert_eq!(l.last_hurt_by_mob, Some(id));

    // Once dead, self-referencing hurt IDs clear on baseTick
    ctx.get_mut(id).unwrap().living_mut().unwrap().health = 0.0;
    ctx.tick_all(true);
    let l = ctx.get(id).unwrap().living().unwrap();
    assert_eq!(l.last_hurt_mob, None);
    assert_eq!(l.last_hurt_by_mob, None);
}

#[test]
fn tick_passenger_stops_stale_and_removed_passengers_like_paper() {
    let mut ctx = LevelCtx::new();
    let vehicle_a = ctx.next_entity_id();
    let vehicle_b = ctx.next_entity_id();
    let rider = ctx.next_entity_id();
    let missing_id = 9999;

    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        vehicle_a,
        EntityTypeMeta::boat(),
        1,
    )));
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        vehicle_b,
        EntityTypeMeta::boat(),
        2,
    )));
    ctx.add_entity(LifecycleEntity::Living(LivingEntity::new(
        rider,
        zombie_meta(),
        20.0,
        3,
    )));

    // Rider is legitimately mounted on vehicle_b, while vehicle_a holds stale entries
    // for both `missing_id` and `rider`.
    assert!(ctx.start_riding(rider, vehicle_b, false, false));
    ctx.get_mut(vehicle_a)
        .unwrap()
        .entity_mut()
        .passengers
        .extend_from_slice(&[missing_id, rider]);

    // The purged ID is detached from vehicle_a.
    ctx.tick_passenger(vehicle_a, missing_id, true);
    assert_eq!(ctx.get(vehicle_a).unwrap().entity().passengers, vec![rider]);

    // ServerLevel#tickPassenger calls rider.stopRiding() when rider.getVehicle() != vehicle_a,
    // which dismounts the rider from its actual vehicle (vehicle_b); vehicle_a's stale entry
    // is left untouched.
    ctx.tick_passenger(vehicle_a, rider, true);
    assert_eq!(ctx.get(rider).unwrap().entity().vehicle, None);
    assert!(ctx.get(vehicle_b).unwrap().entity().passengers.is_empty());
    assert_eq!(ctx.get(vehicle_a).unwrap().entity().passengers, vec![rider]);

    // A removed passenger gets a plain (cancellable) stopRiding().
    ctx.get_mut(rider).unwrap().entity_mut().boarding_cooldown = 0;
    assert!(ctx.start_riding(rider, vehicle_b, false, false));
    ctx.get_mut(rider).unwrap().entity_mut().removal_reason = Some(EntityRemovalReason::Discarded);
    ctx.cancel_dismount = true;
    ctx.tick_passenger(vehicle_b, rider, true);
    assert_eq!(ctx.get(rider).unwrap().entity().vehicle, Some(vehicle_b));
    assert_eq!(ctx.get(vehicle_b).unwrap().entity().passengers, vec![rider]);

    ctx.cancel_dismount = false;
    ctx.tick_passenger(vehicle_b, rider, true);
    assert_eq!(ctx.get(rider).unwrap().entity().vehicle, None);
    assert!(ctx.get(vehicle_b).unwrap().entity().passengers.is_empty());
}

#[test]
fn living_rotation_wrap_uses_jdk_6430675_rounding() {
    let mut ctx = LevelCtx::new();
    let id = ctx.next_entity_id();
    let mut living = LivingEntity::new(id, zombie_meta(), 20.0, 50);
    // 0.49999997_f32 (0x3eff_ffff) must round to 0, not 1
    let just_under_half = f32::from_bits(0x3eff_ffff);
    living.entity.y_rot_o = 0.0_f32;
    living.entity.y_rot = just_under_half * 360.0_f32;
    ctx.add_entity(LifecycleEntity::Living(living));

    // Tick directly through living_tick (skipping set_old_pos_and_rot) to exercise the wrap
    let mut taken = ctx.take_entity(id).unwrap();
    if let LifecycleEntity::Living(ref mut l) = taken {
        l.living_tick_with_hooks(
            &mut ctx,
            |_this, _c| {},
            |_this, _c| {},
            |this, y_body_rot_t, _c| this.tick_head_turn(y_body_rot_t),
        );
        assert_eq!(l.entity.y_rot_o, 0.0_f32);
    }
    ctx.put_back_entity(taken);

    assert_eq!(
        crate::survival::player_survival::java_round_f32(just_under_half),
        0
    );
    assert_eq!(
        crate::survival::player_survival::java_round_f32(8_388_609.0_f32),
        8_388_609
    );
}

#[test]
fn distance_despawn_runs_via_tick_all_and_removed_vehicle_sets_cooldown() {
    let mut ctx = LevelCtx::new();
    ctx.difficulty = Difficulty::Normal;
    ctx.nearest_player_pos = Some(Vec3::new(0.0, 0.0, 0.0));

    let far_mob_id = ctx.next_entity_id();
    let mut far_mob = Mob::new(far_mob_id, zombie_meta(), 20.0, 60);
    far_mob.living.entity.set_pos(200.0, 0.0, 0.0);
    ctx.add_entity(LifecycleEntity::Mob(far_mob));

    ctx.take_events();
    ctx.tick_all(true);
    assert!(ctx.get(far_mob_id).is_none());
    let evs = ctx.take_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, LifecycleEvent::Despawn { id } if *id == far_mob_id))
    );
    assert!(evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::EntityRemoved {
            id,
            reason: EntityRemovalReason::Discarded,
        } if *id == far_mob_id
    )));

    // Removed-vehicle detach in Entity::entity_base_tick sets boarding_cooldown = 60 and emits Dismount
    let vehicle_id = ctx.next_entity_id();
    let rider_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        vehicle_id,
        EntityTypeMeta::boat(),
        61,
    )));
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        rider_id,
        zombie_meta(),
        62,
    )));
    assert!(ctx.start_riding(rider_id, vehicle_id, false, false));
    ctx.get_mut(vehicle_id).unwrap().entity_mut().removal_reason =
        Some(EntityRemovalReason::UnloadedToChunk);
    ctx.take_events();

    let mut taken_rider = ctx.take_entity(rider_id).unwrap();
    taken_rider.entity_mut().entity_base_tick(&mut ctx);
    assert_eq!(taken_rider.entity().vehicle, None);
    // stop_riding sets boarding_cooldown = 60, then entity_base_tick decrements it to 59
    assert_eq!(taken_rider.entity().boarding_cooldown, 59);
    ctx.put_back_entity(taken_rider);

    let detach_evs = ctx.take_events();
    assert!(detach_evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::Dismount { passenger_id, vehicle_id: vid }
            if *passenger_id == rider_id && *vid == vehicle_id
    )));
}

// ============================================================================
// 8. Paper-fidelity regressions (PR #767 review)
// ============================================================================

#[test]
fn entity_fire_state_matches_paper_server_semantics() {
    let mut ctx = LevelCtx::new();
    let fresh = Entity::new(1, pig_meta(), 1);
    assert_eq!(fresh.remaining_fire_ticks, 0);

    // A fire-immune entity clears its fire ticks in one baseTick.
    let mut immune_meta = pig_meta();
    immune_meta.fire_immune = true;
    let mut immune = Entity::new(2, immune_meta, 2);
    immune.remaining_fire_ticks = 100;
    immune.entity_base_tick(&mut ctx);
    assert_eq!(immune.remaining_fire_ticks, 0);

    // clearFire() keeps negative (fire-protection) ticks; a zero count stays zero.
    let mut e = Entity::new(3, pig_meta(), 3);
    e.remaining_fire_ticks = -20;
    e.clear_fire();
    assert_eq!(e.remaining_fire_ticks, -20);
    let mut e = Entity::new(4, pig_meta(), 4);
    e.entity_base_tick(&mut ctx);
    assert_eq!(e.remaining_fire_ticks, 0);

    // visualFire FALSE forces the flag off; the server's isOnFire() ignores the flag.
    let mut e = Entity::new(5, pig_meta(), 5);
    e.remaining_fire_ticks = 40;
    e.visual_fire = Some(false);
    e.entity_base_tick(&mut ctx);
    assert!(!e.get_shared_flag(FLAG_ONFIRE));
    assert!(e.is_on_fire());

    let mut e = Entity::new(6, pig_meta(), 6);
    e.visual_fire = Some(true);
    e.entity_base_tick(&mut ctx);
    assert!(e.get_shared_flag(FLAG_ONFIRE));
    assert!(!e.is_on_fire());
}

#[test]
fn living_below_world_takes_void_damage_instead_of_discard() {
    let mut ctx = LevelCtx::new();
    let living_id = ctx.next_entity_id();
    let mut living = LivingEntity::new(living_id, zombie_meta(), 20.0, 1);
    living.entity.set_pos(0.0, -200.0, 0.0);
    ctx.add_entity(LifecycleEntity::Living(living));
    let base_id = ctx.next_entity_id();
    let mut base = Entity::new(base_id, pig_meta(), 2);
    base.set_pos(0.0, -200.0, 0.0);
    ctx.add_entity(LifecycleEntity::Base(base));
    ctx.take_events();

    ctx.tick_all(true);
    let evs = ctx.take_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        LifecycleEvent::Hurt { id, amount, source: "fell_out_of_world" }
            if *id == living_id && *amount == 4.0
    )));
    assert!(!ctx.get(living_id).unwrap().entity().is_removed());
    // Base Entity#onBelowWorld discards.
    assert!(ctx.get(base_id).is_none());
}

#[test]
fn inactive_ticks_accumulate_no_action_time_for_soft_despawn() {
    let mut ctx = LevelCtx::new();
    let mob_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(mob_id, pig_meta(), 10.0, 1)));
    let living_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Living(LivingEntity::new(
        living_id,
        zombie_meta(),
        20.0,
        2,
    )));
    for _ in 0..3 {
        ctx.tick_all(false);
    }
    assert_eq!(ctx.get(mob_id).unwrap().living().unwrap().no_action_time, 3);
    assert_eq!(
        ctx.get(living_id).unwrap().living().unwrap().no_action_time,
        3
    );
}

#[test]
fn passenger_mob_requires_custom_persistence() {
    let mut ctx = LevelCtx::new();
    ctx.nearest_player_pos = Some(Vec3::new(0.0, 0.0, 0.0));
    let boat_id = ctx.next_entity_id();
    let mut boat = Entity::new(boat_id, EntityTypeMeta::boat(), 1);
    boat.set_pos(200.0, 0.0, 0.0);
    ctx.add_entity(LifecycleEntity::Base(boat));
    let mob_id = ctx.next_entity_id();
    let mut mob = Mob::new(mob_id, zombie_meta(), 20.0, 2);
    mob.living.entity.set_pos(200.0, 0.0, 0.0);
    mob.living.no_action_time = 50;
    ctx.add_entity(LifecycleEntity::Mob(mob));
    assert!(ctx.start_riding(mob_id, boat_id, false, false));

    ctx.tick_all(true);
    let mob = ctx.get(mob_id).expect("riding mob is persistent");
    assert!(!mob.entity().is_removed());
    assert_eq!(mob.living().unwrap().no_action_time, 0);
}

#[test]
fn passenger_of_purged_vehicle_stops_riding_and_keeps_ticking() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let pig_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig_id, pig_meta(), 10.0, 2)));
    assert!(ctx.start_riding(pig_id, boat_id, false, false));
    assert!(ctx.remove_entity(boat_id, EntityRemovalReason::UnloadedToChunk));
    assert_eq!(ctx.get(pig_id).unwrap().entity().vehicle, Some(boat_id));
    ctx.take_events();

    ctx.tick_all(true);
    assert_eq!(ctx.get(pig_id).unwrap().entity().vehicle, None);
    assert!(ctx.take_events().iter().any(|e| matches!(
        e,
        LifecycleEvent::EntityTickStart { id, as_passenger: false } if *id == pig_id
    )));
}

#[test]
fn living_tick_head_turn_matches_paper_26_2() {
    let mut living = LivingEntity::new(1, zombie_meta(), 20.0, 1);
    living.entity.y_rot = 60.0;
    living.tick_head_turn(0.0);
    assert_eq!(living.y_body_rot, 10.0);

    let mut living = LivingEntity::new(1, zombie_meta(), 20.0, 1);
    living.entity.y_rot = -30.0;
    living.tick_head_turn(0.0);
    assert_eq!(living.y_body_rot, 0.0);
}

#[test]
fn start_riding_mount_cancellation_and_ignored_dismount_result() {
    let mut ctx = LevelCtx::new();
    let p = ctx.next_entity_id();
    let v1 = ctx.next_entity_id();
    let v2 = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(p, pig_meta(), 10.0, 1)));
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        v1,
        EntityTypeMeta::boat(),
        2,
    )));
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        v2,
        EntityTypeMeta::boat(),
        3,
    )));

    // EntityMountEvent fires (and can cancel) even when sendEventAndTriggers is false.
    ctx.cancel_mount = true;
    assert!(!ctx.start_riding(p, v1, false, false));
    assert!(!ctx.start_riding(p, v1, true, false));
    ctx.cancel_mount = false;

    // Paper ignores a cancelled dismount from the old vehicle and still mounts the new one.
    assert!(ctx.start_riding(p, v1, false, false));
    ctx.cancel_dismount = true;
    assert!(ctx.start_riding(p, v2, true, false));
    assert_eq!(ctx.get(p).unwrap().entity().vehicle, Some(v2));
    assert_eq!(ctx.get(v2).unwrap().entity().passengers, vec![p]);
    assert_eq!(ctx.get(v1).unwrap().entity().passengers, vec![p]);
}

#[test]
fn equip_event_ignores_count_changes() {
    let mut ctx = LevelCtx::new();
    let mut living = LivingEntity::new(1, zombie_meta(), 20.0, 1);
    living.entity.first_tick = false;
    living.set_item_slot(
        EquipmentSlot::MainHand,
        EquipmentItem::new("minecraft:stone", 1),
        false,
        &mut ctx,
    );
    ctx.take_events();
    living.set_item_slot(
        EquipmentSlot::MainHand,
        EquipmentItem::new("minecraft:stone", 2),
        false,
        &mut ctx,
    );
    assert!(
        !ctx.take_events()
            .iter()
            .any(|e| matches!(e, LifecycleEvent::EquipmentEquipped { .. }))
    );
    living.set_item_slot(
        EquipmentSlot::MainHand,
        EquipmentItem::empty(),
        false,
        &mut ctx,
    );
    assert!(
        ctx.take_events()
            .iter()
            .any(|e| matches!(e, LifecycleEvent::EquipmentEquipped { .. }))
    );
}

#[test]
fn removal_reason_save_and_destroy_flags_match_paper() {
    use EntityRemovalReason::*;
    let flags = |r: EntityRemovalReason| (r.should_destroy(), r.should_save());
    assert_eq!(flags(Killed), (true, false));
    assert_eq!(flags(Discarded), (true, false));
    assert_eq!(flags(UnloadedToChunk), (false, true));
    assert_eq!(flags(UnloadedWithPlayer), (false, false));
    assert_eq!(flags(ChangedDimension), (false, false));
}

#[test]
fn dismount_cancellation_requires_valid_vehicle() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let pig_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    ctx.add_entity(LifecycleEntity::Mob(Mob::new(pig_id, pig_meta(), 10.0, 2)));
    assert!(ctx.start_riding(pig_id, boat_id, false, false));

    // The boat is still valid inside setRemoved, so the passenger dismount is cancelled.
    ctx.cancel_dismount = true;
    assert!(ctx.remove_entity(boat_id, EntityRemovalReason::Discarded));
    assert!(ctx.get(boat_id).is_none());
    assert_eq!(ctx.get(pig_id).unwrap().entity().vehicle, Some(boat_id));

    // Once removed the boat is invalid: EntityDismountEvent no longer fires, so the
    // pig detaches on the next tick even though cancellation is still requested.
    ctx.tick_all(true);
    let pig = ctx.get(pig_id).unwrap();
    assert_eq!(pig.entity().vehicle, None);
    assert!(!pig.mob().unwrap().requires_custom_persistence());

    // A removed-but-stored (invalid) vehicle cannot cancel either.
    let boat2 = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat2,
        EntityTypeMeta::boat(),
        3,
    )));
    ctx.cancel_dismount = false;
    assert!(ctx.start_riding(pig_id, boat2, true, false));
    ctx.cancel_dismount = true;
    ctx.get_mut(boat2).unwrap().entity_mut().valid = false;
    assert!(ctx.stop_riding(pig_id, false));
    assert!(ctx.get(boat2).unwrap().entity().passengers.is_empty());
}

#[test]
fn ride_tick_resets_fall_distance_for_living_passengers() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let mob_id = ctx.next_entity_id();
    let living_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    let mut mob = Mob::new(mob_id, zombie_meta(), 20.0, 2);
    mob.living.entity.fall_distance = 5.0;
    ctx.add_entity(LifecycleEntity::Mob(mob));
    let mut living = LivingEntity::new(living_id, zombie_meta(), 20.0, 3);
    living.entity.fall_distance = 5.0;
    ctx.add_entity(LifecycleEntity::Living(living));
    assert!(ctx.start_riding(mob_id, boat_id, false, false));
    assert!(ctx.start_riding(living_id, boat_id, false, false));

    ctx.tick_all(true);
    assert_eq!(ctx.get(mob_id).unwrap().entity().fall_distance, 0.0);
    assert_eq!(ctx.get(living_id).unwrap().entity().fall_distance, 0.0);
}

#[test]
fn entity_type_presets_match_paper_type_data() {
    // EntityTypeTags.DISMOUNTS_UNDERWATER membership comes from the vanilla tag.
    assert!(pig_meta().dismounts_underwater);
    assert!(!zombie_meta().dismounts_underwater);
    assert!(!EntityTypeMeta::boat().dismounts_underwater);
    assert!(!EntityTypeMeta::player().dismounts_underwater);
    assert!(EntityTypeMeta::new("minecraft:slime", 0.52, 0.52).non_controlling_rider);
    assert!(!pig_meta().non_controlling_rider);

    // OAK_BOAT `.eyeHeight(0.5625F)`; PLAYER `.vehicleAttachment((0, 0.6, 0))`.
    assert_eq!(EntityTypeMeta::boat().dimensions.eye_height, 0.5625_f32);
    let player = EntityTypeMeta::player();
    assert_eq!(
        player
            .dimensions
            .attachments
            .get(EntityAttachment::Vehicle, 0, 0.0),
        Vec3::new(0.0, 0.6, 0.0)
    );

    // A rider with its eyes in water stays on a boat (not in DISMOUNTS_UNDERWATER).
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let rider_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    let mut rider = LivingEntity::new(rider_id, zombie_meta(), 20.0, 2);
    rider.entity.eye_in_water = true;
    ctx.add_entity(LifecycleEntity::Living(rider));
    assert!(ctx.start_riding(rider_id, boat_id, false, false));
    ctx.tick_all(true);
    assert_eq!(ctx.get(rider_id).unwrap().entity().vehicle, Some(boat_id));
}

#[test]
fn start_riding_accepts_removed_vehicle_and_unleashes_mob() {
    let mut ctx = LevelCtx::new();
    let boat_id = ctx.next_entity_id();
    let mob_id = ctx.next_entity_id();
    ctx.add_entity(LifecycleEntity::Base(Entity::new(
        boat_id,
        EntityTypeMeta::boat(),
        1,
    )));
    let mut mob = Mob::new(mob_id, zombie_meta(), 20.0, 2);
    mob.leashed = true;
    ctx.add_entity(LifecycleEntity::Mob(mob));

    // Entity#couldAcceptPassenger() is unconditionally true for a base entity.
    ctx.get_mut(boat_id).unwrap().entity_mut().removal_reason =
        Some(EntityRemovalReason::Discarded);
    assert!(ctx.start_riding(mob_id, boat_id, false, false));

    // Mob#startRiding drops the leash of a mob that mounts successfully.
    let mob = ctx.get(mob_id).unwrap().mob().unwrap();
    assert!(!mob.leashed);
    assert_eq!(mob.living.entity.vehicle, Some(boat_id));
}
