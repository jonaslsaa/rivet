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

    let mut entity = Entity::new(1, zombie_meta(), 42);
    entity.first_tick = false;
    assert!(!entity.set_pose(Pose::Sleeping));
    assert!((entity.dimensions.height - 0.2_f32).abs() < 1e-6);
    assert!((entity.eye_height - 0.2_f32).abs() < 1e-6);

    // Growing back from Sleeping (0.2x0.2) to Standing (0.6x1.95) triggers growth flag immediately
    assert!(entity.set_pose(Pose::Standing));
    assert!((entity.dimensions.height - 1.95_f32).abs() < 1e-6);
    assert!((entity.eye_height - 1.95_f32 * 0.85_f32).abs() < 1e-5);

    // Players never trigger the non-player growth fudge
    let mut player = Entity::new(2, EntityTypeMeta::player(), 43);
    player.first_tick = false;
    assert!(!player.set_pose(Pose::Sleeping));
    assert!(!player.set_pose(Pose::Standing));

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

    // Mounting a player onto the mob must disable move/jump/look flags during tick_all
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
    let flags = ctx.get(id).unwrap().mob().unwrap().control_flags;
    assert!(!flags.move_enabled);
    assert!(!flags.jump_enabled);
    assert!(!flags.look_enabled);
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

    // Player was positioned on top of pig (y = pig_pos.y + pig height 0.9)
    let player_pos = ctx.get(player_id).unwrap().entity().pos;
    assert!((player_pos.y - (pig_pos.y + f64::from(0.9_f32))).abs() < 1e-6);
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
    assert!(
        !despawn_evs
            .iter()
            .any(|e| matches!(e, LifecycleEvent::EntityTickStart { id, .. } if *id == mob_id))
    );
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
    assert!(!ctx.take_events().iter().any(|e| matches!(
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
