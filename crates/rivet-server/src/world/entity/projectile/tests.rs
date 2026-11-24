//! Paper-grounded unit tests for `world::entity::projectile`.
//!
//! Covers normal behavior, deterministic collision edge ordering, arrows,
//! spectral arrows, tridents, pickup, damage handoff, NBT persistence,
//! and controlled negative cases.

use std::collections::BTreeMap;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::random::{LegacyRandomSource, RandomSource};

use super::arrow::abstract_arrow::{
    AIR_INERTIA, ARROW_BASE_DAMAGE, DEFAULT_DESPAWN_TICKS, DEFAULT_GRAVITY, DEFAULT_WATER_INERTIA,
    SHAKE_TIME,
};
use super::arrow::arrow::{EXPOSED_POTION_DECAY_TIME, NO_EFFECT_COLOR};
use super::arrow::spectral_arrow::DEFAULT_DURATION;
use super::arrow::thrown_trident::{TRIDENT_BASE_DAMAGE, TRIDENT_WATER_INERTIA};
use super::phys::{ClipBlock, EntityUuid};
use super::projectile::EntityReference;
use super::projectile_deflection::ProjectileDeflection;
use super::projectile_util;
use super::world_view::{
    DamageHandoff, DamageKind, EntitySnapshot, HurtOutcome, PickupItem, PostHurtEffect,
    ProjectileGameEvent, ProjectileWorld,
};
use super::{
    AbstractArrow, Arrow, Pickup, PotionContents, PotionEffect, SpectralArrow, ThrownTrident,
};
use crate::world::entity::entity_id::EntityId;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::world::phys::{AABB, BlockHitResult, Vec3};

/// Deterministic in-memory test world implementing `ProjectileWorld`.
#[derive(Default)]
struct TestWorld {
    entities: Vec<EntitySnapshot>,
    blocks: BTreeMap<BlockPos, u32>,
    block_hit_override: Option<BlockHitResult>,
    no_collision_override: bool,
    disable_relative_velocity: bool,
    hurt_outcomes: BTreeMap<EntityId, HurtOutcome>,
    damage_log: Vec<(EntityId, DamageHandoff)>,
    effect_log: Vec<(EntityId, PostHurtEffect)>,
    event_log: Vec<ProjectileGameEvent>,
    inventory_adds: Vec<(EntityId, PickupItem)>,
    inventory_accepts: bool,
    dropped_items: Vec<(Vec3, PickupItem, f32)>,
}

impl TestWorld {
    fn new() -> Self {
        Self {
            inventory_accepts: true,
            ..Self::default()
        }
    }

    fn add_entity(&mut self, snapshot: EntitySnapshot) {
        self.entities.push(snapshot);
    }

    fn make_entity(id: u64, pos: Vec3) -> EntitySnapshot {
        let eid = EntityId(id);
        EntitySnapshot {
            id: eid,
            uuid: EntityUuid::new(id as i32, 0, 0, 1),
            position: pos,
            eye_position: pos.add_xyz(0.0, 1.62, 0.0),
            delta_movement: Vec3::ZERO,
            look_angle: Vec3::new(0.0, 0.0, 1.0),
            bounding_box: crate::world::entity::projectile::phys::bounding_box_at(pos, 0.6, 1.8),
            bb_width: 0.6,
            on_ground: true,
            alive: true,
            spectator: false,
            is_player: false,
            has_infinite_materials: false,
            can_be_hit_by_projectile: true,
            can_be_picked: true,
            root_vehicle_id: eid,
            deflection: ProjectileDeflection::None,
            is_redirectable_projectile: false,
        }
    }
}

impl ProjectileWorld for TestWorld {
    fn clip_including_border(
        &self,
        from: Vec3,
        to: Vec3,
        _clip_block: ClipBlock,
        _source_id: EntityId,
    ) -> BlockHitResult {
        if let Some(hit) = self.block_hit_override {
            return hit;
        }
        // Simple AABB clip against all solid blocks in `self.blocks`, picking nearest hit.
        let mut best: Option<(f64, BlockPos, Vec3)> = None;
        for &pos in self.blocks.keys() {
            let bb = AABB::new(
                pos.get_x() as f64,
                pos.get_y() as f64,
                pos.get_z() as f64,
                (pos.get_x() + 1) as f64,
                (pos.get_y() + 1) as f64,
                (pos.get_z() + 1) as f64,
            );
            if let Some(loc) = bb.clip(&from, &to) {
                let d = from.distance_to_sqr(&loc);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, pos, loc));
                }
            }
        }
        if let Some((_, pos, loc)) = best {
            BlockHitResult::new(loc, Direction::West, pos, false)
        } else {
            BlockHitResult::miss(to, Direction::Up, BlockPos::containing(to.x, to.y, to.z))
        }
    }

    fn get_entities(&self, source_id: EntityId, search_area: AABB) -> Vec<EntitySnapshot> {
        self.entities
            .iter()
            .filter(|e| e.id != source_id && e.bounding_box.intersects(&search_area))
            .cloned()
            .collect()
    }

    fn get_entity(&self, id: EntityId) -> Option<EntitySnapshot> {
        self.entities.iter().find(|e| e.id == id).cloned()
    }

    fn resolve_owner(&self, owner: &EntityReference) -> Option<EntitySnapshot> {
        self.entities
            .iter()
            .find(|e| owner.matches_snapshot(e))
            .cloned()
    }

    fn get_owner_vehicle_and_passengers(&self, owner_id: EntityId) -> Vec<EntitySnapshot> {
        let Some(owner) = self.get_entity(owner_id) else {
            return Vec::new();
        };
        self.entities
            .iter()
            .filter(|e| e.root_vehicle_id == owner.root_vehicle_id)
            .cloned()
            .collect()
    }

    fn get_block_state_id(&self, pos: BlockPos) -> Option<u32> {
        self.blocks.get(&pos).copied()
    }

    fn has_no_collision_around(&self, _pos: Vec3, _inflate: f64) -> bool {
        self.no_collision_override
    }

    fn disable_relative_projectile_velocity(&self) -> bool {
        self.disable_relative_velocity
    }

    fn hurt_entity(&mut self, target_id: EntityId, handoff: DamageHandoff) -> HurtOutcome {
        self.damage_log.push((target_id, handoff));
        self.hurt_outcomes
            .get(&target_id)
            .copied()
            .unwrap_or_else(HurtOutcome::living_hurt)
    }

    fn apply_post_hurt_effect(&mut self, target_id: EntityId, effect: PostHurtEffect) {
        self.effect_log.push((target_id, effect));
    }

    fn emit_game_event(&mut self, event: ProjectileGameEvent) {
        self.event_log.push(event);
    }

    fn try_add_to_player_inventory(&mut self, player_id: EntityId, item: &PickupItem) -> bool {
        if self.inventory_accepts {
            self.inventory_adds.push((player_id, item.clone()));
            true
        } else {
            false
        }
    }

    fn spawn_dropped_item(&mut self, pos: Vec3, item: PickupItem, y_offset: f32) {
        self.dropped_items.push((pos, item, y_offset));
    }
}

#[test]
fn compute_margin_matches_paper_formula() {
    assert_eq!(projectile_util::compute_margin(0), 0.0);
    assert_eq!(projectile_util::compute_margin(2), 0.0);
    assert!((projectile_util::compute_margin(5) - 0.15).abs() < 1.0e-6);
    assert!((projectile_util::compute_margin(8) - 0.3).abs() < 1.0e-6);
    assert_eq!(projectile_util::compute_margin(100), 0.3);
}

#[test]
fn shoot_from_rotation_respects_relative_velocity_and_on_ground() {
    let mut world = TestWorld::new();
    let mut shooter = TestWorld::make_entity(1, Vec3::ZERO);
    shooter.delta_movement = Vec3::new(0.5, -0.4, 0.25);
    shooter.on_ground = true;

    let mut arrow = Arrow::new(EntityId(10), Vec3::new(0.0, 1.5, 0.0));
    let mut rng1 = LegacyRandomSource::new(42);
    arrow
        .abstract_arrow
        .projectile
        .shoot_from_rotation(&world, &shooter, 0.0, 0.0, 0.0, 3.0, 0.0, &mut rng1);
    // y_rot=0, x_rot=0 shoots along +Z with pow=3.0, plus shooter (0.5, 0.0 because on_ground, 0.25).
    assert!((arrow.abstract_arrow.projectile.delta_movement.x - 0.5).abs() < 1.0e-5);
    assert!((arrow.abstract_arrow.projectile.delta_movement.y - 0.0).abs() < 1.0e-5);
    assert!((arrow.abstract_arrow.projectile.delta_movement.z - 3.25).abs() < 1.0e-5);

    // Paper config `disableRelativeProjectileVelocity = true` omits shooter velocity.
    world.disable_relative_velocity = true;
    let mut arrow2 = Arrow::new(EntityId(11), Vec3::new(0.0, 1.5, 0.0));
    let mut rng2 = LegacyRandomSource::new(42);
    arrow2
        .abstract_arrow
        .projectile
        .shoot_from_rotation(&world, &shooter, 0.0, 0.0, 0.0, 3.0, 0.0, &mut rng2);
    assert!((arrow2.abstract_arrow.projectile.delta_movement.x).abs() < 1.0e-5);
    assert!((arrow2.abstract_arrow.projectile.delta_movement.z - 3.0).abs() < 1.0e-5);
}

#[test]
fn owner_and_same_vehicle_cannot_be_hit_until_left_owner() {
    let mut world = TestWorld::new();
    let owner = TestWorld::make_entity(1, Vec3::ZERO);
    let mut passenger = TestWorld::make_entity(2, Vec3::new(0.0, 0.0, 0.5));
    passenger.root_vehicle_id = EntityId(1);
    let target = TestWorld::make_entity(3, Vec3::new(0.0, 0.0, 2.0));
    world.add_entity(owner.clone());
    world.add_entity(passenger.clone());
    world.add_entity(target.clone());

    let mut arrow = AbstractArrow::new(
        EntityId(10),
        Vec3::ZERO,
        PickupItem::simple("minecraft:arrow"),
    );
    arrow.projectile.set_owner_snapshot(&owner);

    assert!(!arrow.can_hit_entity(&world, &owner));
    assert!(!arrow.can_hit_entity(&world, &passenger));
    assert!(arrow.can_hit_entity(&world, &target));

    // Once projectile leaves owner collision range, owner and passenger become hittable.
    arrow.projectile.set_pos(Vec3::new(0.0, 0.0, 10.0));
    arrow.projectile.check_left_owner(&world);
    assert!(arrow.projectile.left_owner);
    assert!(arrow.can_hit_entity(&world, &owner));
    assert!(arrow.can_hit_entity(&world, &passenger));
}

#[test]
fn collision_ordering_block_clips_ray_before_entity_behind_wall() {
    let mut world = TestWorld::new();
    // Solid block at x=3, y=1, z=0 ([3,4] x [1,2] x [0,1]).
    world.blocks.insert(BlockPos::new(3, 1, 0), 1);
    // Target entity behind the wall at x=5.
    let target_behind = TestWorld::make_entity(2, Vec3::new(5.0, 0.5, 0.5));
    world.add_entity(target_behind);

    let mut arrow = Arrow::new(EntityId(10), Vec3::new(0.5, 1.5, 0.5));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(8.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(1);
    arrow.tick(&mut world, &mut rng);

    // Arrow must hit the block and NOT damage the entity behind it (controlled negative).
    assert!(world.damage_log.is_empty());
    assert!(arrow.abstract_arrow.in_ground);
    assert_eq!(arrow.abstract_arrow.last_block_state_id, Some(1));
    assert_eq!(arrow.abstract_arrow.shake_time, SHAKE_TIME);
}

#[test]
fn collision_ordering_piercing_sorts_by_distance_from_initial_position() {
    let mut world = TestWorld::new();
    // Deliberately insert entities in REVERSE distance order (far, mid, near)
    // to prove `step_move_and_hit` sorts by `initial_position.distance_to_sqr(&entity.position)`.
    let far = TestWorld::make_entity(30, Vec3::new(6.0, 0.5, 0.5));
    let mid = TestWorld::make_entity(20, Vec3::new(4.0, 0.5, 0.5));
    let near = TestWorld::make_entity(10, Vec3::new(2.0, 0.5, 0.5));
    world.add_entity(far);
    world.add_entity(mid);
    world.add_entity(near);

    let mut arrow = Arrow::new(EntityId(99), Vec3::new(0.5, 1.0, 0.5));
    arrow.abstract_arrow.set_pierce_level(1); // Hits up to 2 entities (pierce_level + 1)
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(8.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(7);
    arrow.tick(&mut world, &mut rng);

    // Must hit near (10) first, then mid (20), then discard on far (30) without damaging far.
    let hit_ids: Vec<EntityId> = world.damage_log.iter().map(|(id, _)| *id).collect();
    assert_eq!(hit_ids, vec![EntityId(10), EntityId(20)]);
    assert!(!arrow.abstract_arrow.projectile.alive);
}

#[test]
fn collision_ordering_exact_tie_preserves_first_entity_in_iteration_order() {
    let mut world = TestWorld::new();
    let first = TestWorld::make_entity(101, Vec3::new(3.0, 0.5, 0.5));
    let second = TestWorld::make_entity(102, Vec3::new(3.0, 0.5, 0.5));
    world.add_entity(first);
    world.add_entity(second);

    let arrow = AbstractArrow::new(
        EntityId(50),
        Vec3::new(0.5, 1.0, 0.5),
        PickupItem::simple("minecraft:arrow"),
    );
    let mut source = arrow.projectile.clone();
    source.delta_movement = Vec3::new(5.0, 0.0, 0.0);

    let hit = projectile_util::get_hit_result_on_move_vector(&world, &source, |_| true);
    match hit {
        super::HitResult::Entity(e) => assert_eq!(e.get_entity(), EntityId(101)),
        other => panic!("expected EntityHitResult for first entity, got {other:?}"),
    }
}

#[test]
fn controlled_negative_non_hittable_entity_is_ignored() {
    let mut world = TestWorld::new();
    let mut ghost = TestWorld::make_entity(5, Vec3::new(2.0, 0.5, 0.5));
    ghost.can_be_hit_by_projectile = false;
    world.add_entity(ghost);

    let mut arrow = Arrow::new(EntityId(10), Vec3::new(0.5, 1.0, 0.5));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(11);
    arrow.tick(&mut world, &mut rng);

    assert!(world.damage_log.is_empty());
    assert!(arrow.abstract_arrow.projectile.alive);
    assert!((arrow.abstract_arrow.projectile.position.x - 4.5).abs() < 1.0e-6);
}

#[test]
fn arrow_damage_formula_and_crit_bonus_match_paper() {
    let mut arrow = AbstractArrow::new(
        EntityId(1),
        Vec3::ZERO,
        PickupItem::simple("minecraft:arrow"),
    );
    arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    arrow.base_damage = ARROW_BASE_DAMAGE; // 2.0
    arrow.crit_arrow = false;

    let mut rng = LegacyRandomSource::new(12345);
    // ceil(3.0 * 2.0) = 6
    assert_eq!(arrow.compute_arrow_damage(&mut rng), 6);

    arrow.crit_arrow = true;
    let mut rng_a = LegacyRandomSource::new(12345);
    let mut rng_b = LegacyRandomSource::new(12345);
    let expected_extra = rng_b.next_int_bound(6 / 2 + 2);
    assert_eq!(arrow.compute_arrow_damage(&mut rng_a), 6 + expected_extra);
}

#[test]
fn arrow_deflects_on_world_border_and_failed_hurt() {
    let mut world = TestWorld::new();
    world.block_hit_override = Some(BlockHitResult::new_with_world_border_hit(
        Vec3::new(2.0, 1.0, 0.0),
        Direction::West,
        BlockPos::new(2, 1, 0),
        false,
        true,
    ));

    let mut arrow = Arrow::new(EntityId(10), Vec3::new(0.0, 1.0, 0.0));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);
    let mut rng = LegacyRandomSource::new(99);
    arrow.tick(&mut world, &mut rng);

    // Reverse deflection scales by -0.5, world border scales by 0.2 => -0.1 of original,
    // then air drag 0.99 and gravity -0.05.
    assert!(!arrow.abstract_arrow.in_ground);
    assert!(arrow.abstract_arrow.projectile.delta_movement.x < 0.0);
    assert!(arrow.abstract_arrow.projectile.needs_sync);
}

#[test]
fn tipped_arrow_applies_scaled_effects_and_decays_in_ground() {
    let mut world = TestWorld::new();
    let target = TestWorld::make_entity(2, Vec3::new(2.0, 0.5, 0.5));
    world.add_entity(target);

    let mut arrow = Arrow::new(EntityId(10), Vec3::new(0.5, 1.0, 0.5));
    arrow.add_effect(PotionEffect {
        effect_id: "minecraft:poison".to_owned(),
        duration: 400,
        amplifier: 1,
    });
    assert_ne!(arrow.effect_color, NO_EFFECT_COLOR);
    assert_eq!(
        arrow.abstract_arrow.pickup_item.item_id,
        "minecraft:tipped_arrow"
    );

    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);
    let mut rng = LegacyRandomSource::new(5);
    arrow.tick(&mut world, &mut rng);

    assert_eq!(world.effect_log.len(), 1);
    assert_eq!(world.effect_log[0].0, EntityId(2));
    assert_eq!(world.effect_log[0].1.effect_id, "minecraft:poison");
    assert_eq!(world.effect_log[0].1.duration, 50); // 400 * 0.125 = 50
    assert_eq!(world.effect_log[0].1.amplifier, 1);

    // Ground decay after EXPOSED_POTION_DECAY_TIME (600) ticks.
    let mut stuck = Arrow::new(EntityId(11), Vec3::ZERO);
    stuck.set_potion_contents(Some(PotionContents {
        effects: vec![PotionEffect {
            effect_id: "minecraft:slowness".to_owned(),
            duration: 200,
            amplifier: 0,
        }],
        custom_color: Some(0x112233),
    }));
    stuck.abstract_arrow.in_ground = true;
    stuck.abstract_arrow.in_ground_time = EXPOSED_POTION_DECAY_TIME - 1;
    let mut world2 = TestWorld::new();
    stuck.tick(&mut world2, &mut rng);
    assert!(stuck.get_potion_contents().is_none());
    assert_eq!(stuck.effect_color, NO_EFFECT_COLOR);
    assert_eq!(stuck.abstract_arrow.pickup_item.item_id, "minecraft:arrow");
}

#[test]
fn spectral_arrow_applies_glowing_and_persists_duration() {
    let mut world = TestWorld::new();
    let target = TestWorld::make_entity(2, Vec3::new(2.0, 0.5, 0.5));
    world.add_entity(target);

    let mut spectral = SpectralArrow::new(EntityId(10), Vec3::new(0.5, 1.0, 0.5));
    assert_eq!(spectral.duration, DEFAULT_DURATION);
    spectral.duration = 320;
    spectral.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(3);
    spectral.tick(&mut world, &mut rng);

    assert_eq!(world.effect_log.len(), 1);
    assert_eq!(world.effect_log[0].1.effect_id, "minecraft:glowing");
    assert_eq!(world.effect_log[0].1.duration, 320);

    let mut tag = CompoundTag::new();
    spectral.save_nbt(&mut tag);
    let mut loaded = SpectralArrow::new(EntityId(11), Vec3::ZERO);
    loaded.load_nbt(&tag);
    assert_eq!(loaded.duration, 320);
}

#[test]
fn arrow_pickup_and_despawn_rules() {
    let mut world = TestWorld::new();
    let mut survival_player = TestWorld::make_entity(1, Vec3::ZERO);
    survival_player.is_player = true;
    let mut creative_player = TestWorld::make_entity(2, Vec3::ZERO);
    creative_player.is_player = true;
    creative_player.has_infinite_materials = true;

    let mut arrow = AbstractArrow::new(
        EntityId(10),
        Vec3::ZERO,
        PickupItem::simple("minecraft:arrow"),
    );
    arrow.in_ground = true;
    arrow.shake_time = 2;
    arrow.pickup = Pickup::Allowed;

    // Cannot pick up while shake_time > 0.
    assert!(!arrow.player_touch(&mut world, &survival_player));
    arrow.shake_time = 0;

    // Pickup::Disallowed rejects everyone.
    arrow.pickup = Pickup::Disallowed;
    assert!(!arrow.player_touch(&mut world, &survival_player));
    assert!(!arrow.player_touch(&mut world, &creative_player));

    // Pickup::CreativeOnly accepts only creative players.
    arrow.pickup = Pickup::CreativeOnly;
    assert!(!arrow.player_touch(&mut world, &survival_player));
    assert!(arrow.player_touch(&mut world, &creative_player));
    assert!(!arrow.projectile.alive);

    // Despawn after 1200 in-ground ticks.
    let mut despawn_arrow = AbstractArrow::new(
        EntityId(11),
        Vec3::ZERO,
        PickupItem::simple("minecraft:arrow"),
    );
    despawn_arrow.in_ground = true;
    despawn_arrow.life = DEFAULT_DESPAWN_TICKS - 1;
    despawn_arrow.tick_despawn();
    assert!(!despawn_arrow.projectile.alive);
}

#[test]
fn trident_damage_water_inertia_loyalty_return_and_owner_only_pickup() {
    let mut world = TestWorld::new();
    let mut owner = TestWorld::make_entity(1, Vec3::new(0.0, 0.0, 0.0));
    owner.is_player = true;
    let mut other_player = TestWorld::make_entity(2, Vec3::new(0.0, 0.0, 0.0));
    other_player.is_player = true;
    let target = TestWorld::make_entity(3, Vec3::new(3.0, 0.5, 0.5));
    world.add_entity(owner.clone());
    world.add_entity(other_player.clone());
    world.add_entity(target);

    // Verify water inertia difference: trident keeps 0.99 vs arrow 0.6.
    assert_eq!(TRIDENT_WATER_INERTIA, 0.99);
    assert_eq!(DEFAULT_WATER_INERTIA, 0.6);

    let mut trident = ThrownTrident::new(EntityId(20), Vec3::new(0.5, 1.0, 0.5), 3, true);
    trident.abstract_arrow.projectile.set_owner_snapshot(&owner);
    trident.abstract_arrow.projectile.left_owner = true;
    trident.abstract_arrow.pickup = Pickup::Allowed;
    trident.abstract_arrow.projectile.delta_movement = Vec3::new(5.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(77);
    trident.tick(&mut world, &mut rng);

    // Hit target with fixed 8.0F trident damage and set dealt_damage = true.
    assert!(trident.dealt_damage);
    assert_eq!(world.damage_log.len(), 1);
    assert_eq!(world.damage_log[0].0, EntityId(3));
    assert_eq!(world.damage_log[0].1.kind, DamageKind::Trident);
    assert_eq!(world.damage_log[0].1.amount, TRIDENT_BASE_DAMAGE);
    assert_eq!(world.damage_log[0].1.causing_entity, EntityId(1));

    // Next tick engages Loyalty return (no_physics = true, accelerates toward owner).
    trident.tick(&mut world, &mut rng);
    assert!(trident.abstract_arrow.is_no_physics());
    assert!(trident.client_side_return_trident_tick_count > 0);
    // Does not hit target a second time because dealt_damage == true.
    assert_eq!(world.damage_log.len(), 1);

    // Other player cannot pick up the trident; owner can.
    assert!(!trident.player_touch(&mut world, &other_player));
    assert!(trident.player_touch(&mut world, &owner));
    assert!(!trident.abstract_arrow.projectile.alive);
    assert_eq!(world.inventory_adds.len(), 1);
    assert_eq!(world.inventory_adds[0].0, EntityId(1));
}

#[test]
fn loyal_trident_drops_item_when_owner_dies_or_spectates() {
    let mut world = TestWorld::new();
    let mut owner = TestWorld::make_entity(1, Vec3::ZERO);
    owner.is_player = true;
    owner.spectator = true;
    world.add_entity(owner.clone());

    let mut trident = ThrownTrident::new(EntityId(21), Vec3::new(5.0, 2.0, 0.0), 2, false);
    trident.abstract_arrow.projectile.set_owner_snapshot(&owner);
    trident.abstract_arrow.pickup = Pickup::Allowed;
    trident.dealt_damage = true;

    let mut rng = LegacyRandomSource::new(12);
    trident.tick(&mut world, &mut rng);

    assert!(!trident.abstract_arrow.projectile.alive);
    assert_eq!(world.dropped_items.len(), 1);
    assert_eq!(world.dropped_items[0].1.item_id, "minecraft:trident");
}

#[test]
fn arrow_and_trident_nbt_round_trip() {
    let mut trident = ThrownTrident::new(EntityId(30), Vec3::new(1.0, 2.0, 3.0), 3, true);
    trident.abstract_arrow.projectile.owner =
        Some(EntityReference::new(EntityUuid::new(1, 2, 3, 4), None));
    trident.abstract_arrow.projectile.left_owner = true;
    trident.abstract_arrow.projectile.has_been_shot = true;
    trident.abstract_arrow.life = 45;
    trident.abstract_arrow.in_ground = true;
    trident.abstract_arrow.shake_time = 3;
    trident.abstract_arrow.pickup = Pickup::CreativeOnly;
    trident.abstract_arrow.base_damage = 9.5;
    trident.abstract_arrow.crit_arrow = true;
    trident.abstract_arrow.pierce_level = 2;
    trident.abstract_arrow.last_block_state_id = Some(42);
    trident.abstract_arrow.fired_from_weapon = Some("minecraft:trident".to_owned());
    trident.dealt_damage = true;

    let mut tag = CompoundTag::new();
    trident.save_nbt(&mut tag);

    let mut loaded = ThrownTrident::new(EntityId(31), Vec3::ZERO, 0, false);
    loaded.load_nbt(&tag);

    assert_eq!(
        loaded.abstract_arrow.projectile.owner.map(|o| o.uuid),
        Some(EntityUuid::new(1, 2, 3, 4))
    );
    assert!(loaded.abstract_arrow.projectile.left_owner);
    assert!(loaded.abstract_arrow.projectile.has_been_shot);
    assert_eq!(loaded.abstract_arrow.life, 45);
    assert!(loaded.abstract_arrow.in_ground);
    assert_eq!(loaded.abstract_arrow.shake_time, 3);
    assert_eq!(loaded.abstract_arrow.pickup, Pickup::CreativeOnly);
    assert_eq!(loaded.abstract_arrow.base_damage, 9.5);
    assert!(loaded.abstract_arrow.crit_arrow);
    assert_eq!(loaded.abstract_arrow.pierce_level, 2);
    assert_eq!(loaded.abstract_arrow.last_block_state_id, Some(42));
    assert_eq!(
        loaded.abstract_arrow.fired_from_weapon.as_deref(),
        Some("minecraft:trident")
    );
    assert!(loaded.dealt_damage);
    assert_eq!(loaded.loyalty, 3);
    assert!(loaded.foil);

    // Verify out-of-range Pickup ordinal falls back to Disallowed (Paper Pickup.byOrdinal).
    assert_eq!(Pickup::by_ordinal(-1), Pickup::Disallowed);
    assert_eq!(Pickup::by_ordinal(3), Pickup::Disallowed);
    assert_eq!(Pickup::by_ordinal(99), Pickup::Disallowed);
}

#[test]
fn air_drag_and_gravity_match_paper_constants() {
    let mut world = TestWorld::new();
    let mut arrow = Arrow::new(EntityId(40), Vec3::new(0.0, 10.0, 0.0));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(1.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(1);
    arrow.tick(&mut world, &mut rng);

    assert!((arrow.abstract_arrow.projectile.position.x - 1.0).abs() < 1.0e-9);
    assert!((arrow.abstract_arrow.projectile.delta_movement.x - AIR_INERTIA as f64).abs() < 1.0e-6);
    assert!((arrow.abstract_arrow.projectile.delta_movement.y - (-DEFAULT_GRAVITY)).abs() < 1.0e-9);
}

#[test]
fn tipped_arrow_nbt_round_trip_preserves_effects_and_duration_scale() {
    let mut arrow = Arrow::new(EntityId(41), Vec3::ZERO);
    arrow.potion_duration_scale = 0.25;
    arrow.set_potion_contents(Some(PotionContents {
        effects: vec![
            PotionEffect {
                effect_id: "minecraft:poison".to_owned(),
                duration: 200,
                amplifier: 1,
            },
            PotionEffect {
                effect_id: "minecraft:slowness".to_owned(),
                duration: -1,
                amplifier: 0,
            },
        ],
        custom_color: Some(0x123456),
    }));

    let mut tag = CompoundTag::new();
    arrow.save_nbt(&mut tag);

    let mut loaded = Arrow::new(EntityId(42), Vec3::ZERO);
    loaded.load_nbt(&tag);

    assert_eq!(loaded.potion_duration_scale, 0.25);
    assert_eq!(loaded.effect_color, 0x123456);
    assert_eq!(
        loaded.abstract_arrow.pickup_item.item_id,
        "minecraft:tipped_arrow"
    );
    let contents = loaded
        .get_potion_contents()
        .expect("potion contents restored");
    assert_eq!(contents.custom_color, Some(0x123456));
    assert_eq!(contents.effects.len(), 2);
    assert_eq!(contents.effects[0].effect_id, "minecraft:poison");
    assert_eq!(contents.effects[0].duration, 200);
    assert_eq!(contents.effects[0].amplifier, 1);
    assert_eq!(contents.effects[1].effect_id, "minecraft:slowness");
    assert_eq!(contents.effects[1].duration, -1);
}

#[test]
fn rejected_non_piercing_arrow_moves_away_from_min_hitbox_face() {
    let mut world = TestWorld::new();
    // Width 0.6 centered at x = 3.0 -> min X face is at x = 2.7.
    world.add_entity(TestWorld::make_entity(70, Vec3::new(3.0, 0.0, 0.0)));
    world
        .hurt_outcomes
        .insert(EntityId(70), HurtOutcome::ignored());

    let mut arrow = Arrow::new(EntityId(71), Vec3::new(0.0, 1.0, 0.0));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(1);
    arrow.tick(&mut world, &mut rng);

    // First tick hits target at min X face (x = 2.7), is rejected, and reverses velocity.
    assert_eq!(world.damage_log.len(), 1);
    assert!((arrow.abstract_arrow.projectile.position.x - 2.7).abs() < 1.0e-9);
    assert!(arrow.abstract_arrow.projectile.delta_movement.x < 0.0);

    // Second tick must move away from the hitbox instead of re-hitting the same entity.
    arrow.tick(&mut world, &mut rng);
    assert_eq!(world.damage_log.len(), 1);
    assert!(arrow.abstract_arrow.projectile.position.x < 2.7);
}

#[test]
fn thrown_trident_uses_custom_base_damage() {
    let mut world = TestWorld::new();
    world.add_entity(TestWorld::make_entity(80, Vec3::new(2.0, 0.0, 0.0)));

    let mut trident = ThrownTrident::new(EntityId(81), Vec3::new(0.0, 1.0, 0.0), 0, false);
    trident.abstract_arrow.base_damage = 13.5;
    trident.abstract_arrow.projectile.delta_movement = Vec3::new(4.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(2);
    trident.tick(&mut world, &mut rng);

    assert_eq!(world.damage_log.len(), 1);
    assert_eq!(world.damage_log[0].1.kind, DamageKind::Trident);
    assert_eq!(world.damage_log[0].1.amount, 13.5);
}

#[test]
fn loyal_trident_non_player_collection_uses_owner_bb_width() {
    let mut world = TestWorld::new();
    let mut mob_owner = TestWorld::make_entity(90, Vec3::ZERO);
    mob_owner.bb_width = 2.0;
    mob_owner.eye_position = Vec3::new(0.0, 1.5, 0.0);
    world.add_entity(mob_owner.clone());

    // Distance 2.5 from owner eye position: >= trident.bb_width + 1.0 (1.5),
    // but < owner.bb_width + 1.0 (3.0).
    let mut trident = ThrownTrident::new(EntityId(91), Vec3::new(2.5, 1.5, 0.0), 3, false);
    trident
        .abstract_arrow
        .projectile
        .set_owner(Some(EntityReference::new(
            mob_owner.uuid,
            Some(mob_owner.id),
        )));
    trident.dealt_damage = true;

    let mut rng = LegacyRandomSource::new(3);
    trident.tick(&mut world, &mut rng);

    assert!(!trident.abstract_arrow.projectile.alive);
}

#[test]
fn arrow_discarded_on_first_tick_still_emits_projectile_shoot() {
    let mut world = TestWorld::new();
    world.add_entity(TestWorld::make_entity(95, Vec3::new(1.5, 0.0, 0.0)));

    let mut arrow = Arrow::new(EntityId(96), Vec3::new(0.0, 1.0, 0.0));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(4);
    arrow.tick(&mut world, &mut rng);

    // Non-piercing arrow hits and is discarded on tick 0, but still emits ProjectileShoot first.
    assert!(!arrow.abstract_arrow.projectile.alive);
    assert!(arrow.abstract_arrow.projectile.has_been_shot);
    let shoot_idx = world
        .event_log
        .iter()
        .position(|e| matches!(e, ProjectileGameEvent::ProjectileShoot { .. }))
        .expect("ProjectileShoot should be emitted");
    let land_idx = world
        .event_log
        .iter()
        .position(|e| matches!(e, ProjectileGameEvent::ProjectileLandEntity { .. }))
        .expect("ProjectileLandEntity should be emitted");
    assert!(
        shoot_idx < land_idx,
        "ProjectileShoot must be emitted before ProjectileLandEntity (super.tick() ordering)"
    );
}

#[test]
fn entity_driven_deflection_transfers_owner() {
    let mut world = TestWorld::new();
    let original_owner = TestWorld::make_entity(110, Vec3::new(-5.0, 0.0, 0.0));
    let mut deflector = TestWorld::make_entity(111, Vec3::new(1.5, 0.0, 0.0));
    deflector.deflection = ProjectileDeflection::Reverse;
    world.add_entity(original_owner.clone());
    world.add_entity(deflector.clone());

    let mut arrow = Arrow::new(EntityId(112), Vec3::new(0.0, 1.0, 0.0));
    arrow
        .abstract_arrow
        .projectile
        .set_owner(Some(EntityReference::from_snapshot(&original_owner)));
    arrow.abstract_arrow.projectile.left_owner = true;
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(6);
    arrow.tick(&mut world, &mut rng);

    assert_eq!(
        arrow
            .abstract_arrow
            .projectile
            .owner
            .as_ref()
            .and_then(|o| o.cached_id),
        Some(deflector.id)
    );
    assert_eq!(
        arrow.abstract_arrow.projectile.last_deflected_by,
        Some(deflector.id)
    );
}

#[test]
fn reverse_bounces_preserve_current_owner() {
    // 1. World border bounce
    let mut world = TestWorld::new();
    let owner = TestWorld::make_entity(120, Vec3::new(-5.0, 0.0, 0.0));
    world.add_entity(owner.clone());
    world.block_hit_override = Some(BlockHitResult::new(
        Vec3::new(2.0, 1.0, 0.0),
        Direction::West,
        BlockPos::new(2, 1, 0),
        true,
    ));

    let mut arrow = Arrow::new(EntityId(121), Vec3::new(0.0, 1.0, 0.0));
    arrow
        .abstract_arrow
        .projectile
        .set_owner(Some(EntityReference::from_snapshot(&owner)));
    arrow.abstract_arrow.projectile.left_owner = true;
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    let mut rng = LegacyRandomSource::new(7);
    arrow.tick(&mut world, &mut rng);
    assert_eq!(
        arrow
            .abstract_arrow
            .projectile
            .owner
            .as_ref()
            .and_then(|o| o.cached_id),
        Some(owner.id)
    );

    // 2. Arrow failed-hurt bounce
    let mut world = TestWorld::new();
    world.add_entity(owner.clone());
    world.add_entity(TestWorld::make_entity(122, Vec3::new(1.5, 0.0, 0.0)));
    world.hurt_outcomes.insert(
        EntityId(122),
        HurtOutcome {
            hurt_applied: false,
            target_is_living: true,
            target_is_enderman: false,
        },
    );
    let mut arrow = Arrow::new(EntityId(123), Vec3::new(0.0, 1.0, 0.0));
    arrow
        .abstract_arrow
        .projectile
        .set_owner(Some(EntityReference::from_snapshot(&owner)));
    arrow.abstract_arrow.projectile.left_owner = true;
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    arrow.tick(&mut world, &mut rng);
    assert_eq!(
        arrow
            .abstract_arrow
            .projectile
            .owner
            .as_ref()
            .and_then(|o| o.cached_id),
        Some(owner.id)
    );

    // 3. ThrownTrident hit bounce
    let mut world = TestWorld::new();
    world.add_entity(owner.clone());
    world.add_entity(TestWorld::make_entity(124, Vec3::new(1.5, 0.0, 0.0)));
    let mut trident = ThrownTrident::new(EntityId(125), Vec3::new(0.0, 1.0, 0.0), 0, false);
    trident
        .abstract_arrow
        .projectile
        .set_owner(Some(EntityReference::from_snapshot(&owner)));
    trident.abstract_arrow.projectile.left_owner = true;
    trident.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    trident.tick(&mut world, &mut rng);
    assert_eq!(
        trident
            .abstract_arrow
            .projectile
            .owner
            .as_ref()
            .and_then(|o| o.cached_id),
        Some(owner.id)
    );
}

#[test]
fn tick_base_increments_tick_count_before_margin_hit_detection() {
    // Target AABB covers x in [1.2, 1.8], y in [0.0, 1.8], z in [-0.3, 0.3].
    // Ray at z = 0.33 misses with margin = 0.0 (tick_count == 2) but hits with
    // margin = 0.05 (tick_count == 3 after super.tick() increments tick_count).
    let mut world = TestWorld::new();
    world.add_entity(TestWorld::make_entity(130, Vec3::new(1.5, 0.0, 0.0)));

    let mut arrow = Arrow::new(EntityId(131), Vec3::new(0.0, 1.0, 0.33));
    arrow.abstract_arrow.projectile.tick_count = 2;
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    let mut rng = LegacyRandomSource::new(8);
    arrow.tick(&mut world, &mut rng);

    assert_eq!(world.damage_log.len(), 1);
    assert_eq!(world.damage_log[0].0, EntityId(130));

    // Same check for ThrownTrident.
    let mut world = TestWorld::new();
    world.add_entity(TestWorld::make_entity(132, Vec3::new(1.5, 0.0, 0.0)));

    let mut trident = ThrownTrident::new(EntityId(133), Vec3::new(0.0, 1.0, 0.33), 0, false);
    trident.abstract_arrow.projectile.tick_count = 2;
    trident.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);
    trident.tick(&mut world, &mut rng);

    assert_eq!(world.damage_log.len(), 1);
    assert_eq!(world.damage_log[0].0, EntityId(132));
}

#[test]
fn tipped_arrow_preserves_infinite_effect_duration() {
    let mut world = TestWorld::new();
    world.add_entity(TestWorld::make_entity(100, Vec3::new(1.5, 0.0, 0.0)));

    let mut arrow = Arrow::new(EntityId(101), Vec3::new(0.0, 1.0, 0.0));
    arrow.set_potion_contents(Some(PotionContents {
        effects: vec![PotionEffect {
            effect_id: "minecraft:weakness".to_owned(),
            duration: -1,
            amplifier: 0,
        }],
        custom_color: None,
    }));
    arrow.abstract_arrow.projectile.delta_movement = Vec3::new(3.0, 0.0, 0.0);

    let mut rng = LegacyRandomSource::new(5);
    arrow.tick(&mut world, &mut rng);

    assert_eq!(world.effect_log.len(), 1);
    assert_eq!(world.effect_log[0].1.effect_id, "minecraft:weakness");
    assert_eq!(world.effect_log[0].1.duration, -1);
}

#[test]
fn aabb_new_matches_java_math_min_max_for_nan_and_signed_zero() {
    let nan_box = AABB::new(f64::NAN, -0.0, 1.0, 2.0, 0.0, f64::NAN);
    assert!(nan_box.min_x.is_nan());
    assert!(nan_box.max_x.is_nan());
    assert!(nan_box.min_y.is_sign_negative() && nan_box.min_y == 0.0);
    assert!(nan_box.max_y.is_sign_positive() && nan_box.max_y == 0.0);
    assert!(nan_box.min_z.is_nan());
    assert!(nan_box.max_z.is_nan());
}
