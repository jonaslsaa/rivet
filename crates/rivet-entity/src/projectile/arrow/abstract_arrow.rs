//! Port of `net.minecraft.world.entity.projectile.arrow.AbstractArrow` (Paper 26.2).
//!
//! Preserves exact Paper semantics for:
//! - Deterministic `stepMoveAndHit`: block clip first, multi-entity sweep with
//!   `ProjectileUtil::compute_margin`, stable sort by `initial_position.distance_to_sqr(entity.position)`
//! - Piercing (`pierce_level`, `pierced_entity_ids` preserving insertion order via `IndexSet`)
//! - Damage calculation (`Mth::ceil_d(Mth::clamp_f64(pow * base_damage, 0.0, i32::MAX))` + crit bonus)
//! - Block sticking (`in_ground`, `last_block_state_id`, backoff by `signum(movement) * 0.05`,
//!   `shake_time = 7`, `should_fall` / `start_falling`)
//! - Air drag (`0.99`), water inertia (`0.6` default, `0.99` for tridents), gravity (`0.05`)
//! - Pickup rules (`Pickup::Disallowed`, `Allowed`, `CreativeOnly`) and `player_touch`
//! - Despawn after `1200` in-ground ticks
//! - NBT persistence (`life`, `inGround`, `shake`, `crit`, `PierceLevel`, `damage`, `pickup`,
//!   `inBlockState`, `item`, `weapon`)

use indexmap::IndexSet;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::mth;
use rivet_util::random::RandomSource;

use crate::projectile::phys::{
    BlockHitResult, BlockPos, ClipBlock, EntityHitResult, EntityId, HitResult, Vec3, java_signum,
};
use crate::projectile::projectile::ProjectileState;
use crate::projectile::projectile_deflection::ProjectileDeflection;
use crate::projectile::projectile_util;
use crate::projectile::world_view::{
    DamageHandoff, DamageKind, EntitySnapshot, PickupItem, ProjectileWorld,
};

/// `AbstractArrow.ARROW_BASE_DAMAGE`.
pub const ARROW_BASE_DAMAGE: f64 = 2.0;
/// `AbstractArrow.SHAKE_TIME`.
pub const SHAKE_TIME: i32 = 7;
/// `AbstractArrow.getWaterInertia()` default (`0.6F`).
pub const DEFAULT_WATER_INERTIA: f32 = 0.6;
/// `AbstractArrow` air inertia (`0.99F`).
pub const AIR_INERTIA: f32 = 0.99;
/// `AbstractArrow.getDefaultGravity()`.
pub const DEFAULT_GRAVITY: f64 = 0.05;
/// `AbstractArrow.tickDespawn()` threshold (`1200`).
pub const DEFAULT_DESPAWN_TICKS: i32 = 1200;
/// Standard arrow bounding box width/height (`EntityType.ARROW`: `0.5 x 0.5`).
pub const ARROW_BB_SIZE: f64 = 0.5;

/// `AbstractArrow.Pickup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Pickup {
    #[default]
    Disallowed = 0,
    Allowed = 1,
    CreativeOnly = 2,
}

impl Pickup {
    pub const fn ordinal(self) -> u8 {
        self as u8
    }

    /// `AbstractArrow.Pickup.byOrdinal(int ordinal)`.
    ///
    /// Out-of-range ordinals fall back to `Pickup::Disallowed` (ordinal 0).
    pub fn by_ordinal(ordinal: i32) -> Self {
        match ordinal {
            1 => Self::Allowed,
            2 => Self::CreativeOnly,
            _ => Self::Disallowed,
        }
    }
}

/// Optional per-subclass hooks invoked during `AbstractArrow` ticking and entity hits.
pub trait ArrowHooks {
    fn water_inertia(&self) -> f32 {
        DEFAULT_WATER_INERTIA
    }

    fn damage_kind(&self) -> DamageKind {
        DamageKind::Arrow
    }

    fn post_hurt_effects<W: ProjectileWorld>(
        &mut self,
        _arrow: &AbstractArrow,
        _world: &mut W,
        _target_id: EntityId,
    ) {
    }
}

/// Default no-op hooks for plain `AbstractArrow` usage.
pub struct DefaultArrowHooks;
impl ArrowHooks for DefaultArrowHooks {}

/// Port of `net.minecraft.world.entity.projectile.arrow.AbstractArrow`.
#[derive(Debug, Clone, PartialEq)]
pub struct AbstractArrow {
    pub projectile: ProjectileState,
    pub last_block_state_id: Option<u32>,
    pub in_ground: bool,
    pub in_ground_time: i32,
    pub pickup: Pickup,
    pub shake_time: i32,
    pub life: i32,
    pub base_damage: f64,
    pub crit_arrow: bool,
    pub pierce_level: u8,
    pub pickup_item: PickupItem,
    pub fired_from_weapon: Option<String>,
    /// Preserves insertion order for deterministic inspection (`IndexSet`).
    pub pierced_entity_ids: IndexSet<EntityId>,
    /// Tracks entities killed during a single piercing shot.
    pub killed_entity_ids: IndexSet<EntityId>,
}

impl AbstractArrow {
    pub fn new(id: EntityId, position: Vec3, pickup_item: PickupItem) -> Self {
        Self {
            projectile: ProjectileState::new(id, position, ARROW_BB_SIZE, ARROW_BB_SIZE),
            last_block_state_id: None,
            in_ground: false,
            in_ground_time: 0,
            pickup: Pickup::Disallowed,
            shake_time: 0,
            life: 0,
            base_damage: ARROW_BASE_DAMAGE,
            crit_arrow: false,
            pierce_level: 0,
            pickup_item,
            fired_from_weapon: None,
            pierced_entity_ids: IndexSet::new(),
            killed_entity_ids: IndexSet::new(),
        }
    }

    pub fn set_base_damage(&mut self, base_damage: f64) {
        self.base_damage = base_damage;
    }

    pub fn set_crit_arrow(&mut self, crit: bool) {
        self.crit_arrow = crit;
    }

    pub fn set_pierce_level(&mut self, pierce_level: u8) {
        self.pierce_level = pierce_level;
    }

    pub fn set_no_physics(&mut self, no_physics: bool) {
        self.projectile.no_physics = no_physics;
    }

    pub fn is_no_physics(&self) -> bool {
        self.projectile.no_physics
    }

    pub fn reset_pierced_entities(&mut self) {
        self.pierced_entity_ids.clear();
        self.killed_entity_ids.clear();
    }

    /// `AbstractArrow.canHitEntity(Entity)`.
    pub fn can_hit_entity<W: ProjectileWorld>(&self, world: &W, entity: &EntitySnapshot) -> bool {
        if !self.projectile.can_hit_entity(world, entity) {
            return false;
        }
        !self.pierced_entity_ids.contains(&entity.id)
    }

    /// `AbstractArrow.shouldFall()`.
    pub fn should_fall<W: ProjectileWorld>(&self, world: &W) -> bool {
        self.in_ground && world.has_no_collision_around(self.projectile.position, 0.06)
    }

    /// `AbstractArrow.startFalling()`.
    pub fn start_falling<R: RandomSource>(&mut self, random: &mut R) {
        self.in_ground = false;
        let delta = self.projectile.delta_movement;
        self.projectile.delta_movement = delta.multiply(
            (random.next_float() * 0.2_f32) as f64,
            (random.next_float() * 0.2_f32) as f64,
            (random.next_float() * 0.2_f32) as f64,
        );
        self.life = 0;
    }

    /// `AbstractArrow.tickDespawn()`.
    pub fn tick_despawn(&mut self) {
        self.life = self.life.wrapping_add(1);
        if self.life >= DEFAULT_DESPAWN_TICKS {
            self.projectile.discard();
        }
    }

    /// Advances one server tick using `DefaultArrowHooks`.
    pub fn tick<W: ProjectileWorld, R: RandomSource>(&mut self, world: &mut W, random: &mut R) {
        let mut hooks = DefaultArrowHooks;
        self.tick_with_hooks(world, random, &mut hooks);
    }

    /// `AbstractArrow.tick()` with subclass hooks (`Arrow`, `SpectralArrow`, `ThrownTrident`).
    pub fn tick_with_hooks<W: ProjectileWorld, R: RandomSource, H: ArrowHooks>(
        &mut self,
        world: &mut W,
        random: &mut R,
        hooks: &mut H,
    ) {
        if !self.projectile.alive {
            return;
        }

        let physics_enabled = !self.is_no_physics();
        let mut movement = self.projectile.delta_movement;

        if self.projectile.x_rot_o == 0.0
            && self.projectile.y_rot_o == 0.0
            && movement.length_sqr() > 0.0
        {
            let sd = movement.horizontal_distance();
            self.projectile.y_rot = (mth::atan2(movement.x, movement.z)
                * (180.0_f32 / std::f32::consts::PI) as f64)
                as f32;
            self.projectile.x_rot =
                (mth::atan2(movement.y, sd) * (180.0_f32 / std::f32::consts::PI) as f64) as f32;
            self.projectile.y_rot_o = self.projectile.y_rot;
            self.projectile.x_rot_o = self.projectile.x_rot;
        }

        let block_pos = BlockPos::containing(self.projectile.position);
        let current_state = world.get_block_state_id(block_pos);

        if self.shake_time > 0 {
            self.shake_time = self.shake_time.wrapping_sub(1);
        }

        if self.in_ground && physics_enabled {
            if self.last_block_state_id != current_state && self.should_fall(world) {
                self.start_falling(random);
            } else {
                self.tick_despawn();
            }
            self.in_ground_time = self.in_ground_time.wrapping_add(1);
            self.projectile.tick_base(world);
            return;
        }

        self.in_ground_time = 0;

        // Water inertia is applied before clipping when in water.
        let mut inertia = AIR_INERTIA;
        if self.projectile.in_water {
            inertia = hooks.water_inertia();
        }

        self.projectile.check_left_owner(world);

        let initial_position = self.projectile.position;
        let next_position = initial_position.add(movement);
        let block_hit = if physics_enabled {
            world.clip_including_border(
                initial_position,
                next_position,
                ClipBlock::Collider,
                self.projectile.id,
            )
        } else {
            BlockHitResult::miss(
                next_position,
                crate::projectile::phys::Direction::Up,
                BlockPos::containing(next_position),
            )
        };

        self.step_move_and_hit(world, random, hooks, block_hit);

        if !self.projectile.alive {
            return;
        }

        if !self.in_ground {
            movement = self.projectile.delta_movement.scale(inertia as f64);
            if physics_enabled {
                movement = movement.add_xyz(0.0, -DEFAULT_GRAVITY, 0.0);
            }
            self.projectile.delta_movement = movement;
            self.projectile.update_rotation();
        }

        self.projectile.tick_base(world);
    }

    /// `AbstractArrow.stepMoveAndHit(BlockHitResult)`.
    ///
    /// Exact Paper collision ordering:
    /// 1. Start with `block_hit` from `initial_position` to `initial_position + delta`.
    /// 2. If `block_hit` is not a miss, truncate `target_pos` to `block_hit.location`.
    /// 3. Collect all candidate entities along `[initial_position, target_pos]` with
    ///    `compute_margin(tick_count)` and `include_inside = !is_no_physics()`.
    /// 4. Sort `entities_hit` stably by `initial_position.distance_to_sqr(entity.position)`
    ///    (Java `List.sort` / TimSort).
    /// 5. Process each entity hit in sorted order until discarded, deflected, or non-piercing hit.
    /// 6. If still alive, not deflected, and `block_hit` was not a miss (and no entity stopped it),
    ///    process `block_hit`.
    /// 7. Move to `target_pos` (if not stopped in ground / deflected).
    pub fn step_move_and_hit<W: ProjectileWorld, R: RandomSource, H: ArrowHooks>(
        &mut self,
        world: &mut W,
        random: &mut R,
        hooks: &mut H,
        block_hit: BlockHitResult,
    ) {
        let initial_position = self.projectile.position;
        let movement = self.projectile.delta_movement;
        let target_pos = if block_hit.miss {
            initial_position.add(movement)
        } else {
            block_hit.location
        };

        let search_area = self
            .projectile
            .bounding_box
            .expand_towards(movement)
            .inflate(1.0);
        let margin = projectile_util::compute_margin(self.projectile.tick_count);

        // Paper `AbstractArrow.findHitEntities` passes `includeFromEntity = false`.
        let mut entities_hit = projectile_util::get_many_entity_hit_result(
            world,
            &self.projectile,
            initial_position,
            target_pos,
            search_area,
            |e| self.can_hit_entity(world, e),
            margin,
            false,
        );

        // Paper sorts entity hits by `initialPosition.distanceToSqr(entity.position())`.
        // Rust's `slice::sort_by` is stable (matching Java's TimSort on equal distances).
        entities_hit.sort_by(|a, b| {
            let da = world
                .get_entity(a.entity_id)
                .map_or(f64::MAX, |e| initial_position.distance_to_sqr(e.position));
            let db = world
                .get_entity(b.entity_id)
                .map_or(f64::MAX, |e| initial_position.distance_to_sqr(e.position));
            da.total_cmp(&db)
        });

        let mut hit_entity_stopped = false;
        let mut deflected = false;

        for entity_hit in entities_hit {
            if !self.projectile.alive {
                break;
            }
            let hit = HitResult::Entity(entity_hit);
            let deflection = self
                .projectile
                .evaluate_deflection_or_world_border(world, &hit, true, random);
            if deflection != ProjectileDeflection::None {
                deflected = true;
                break;
            }

            self.projectile.set_pos(entity_hit.location);
            self.on_hit_entity(world, random, hooks, entity_hit);
            self.projectile.on_hit_common(world, &hit);

            if !self.projectile.alive || self.pierce_level == 0 {
                hit_entity_stopped = true;
                break;
            }
        }

        if !self.projectile.alive || deflected || hit_entity_stopped {
            return;
        }

        if !block_hit.miss {
            let hit = HitResult::Block(block_hit);
            let deflection = self
                .projectile
                .evaluate_deflection_or_world_border(world, &hit, true, random);
            if deflection != ProjectileDeflection::None {
                return;
            }
            self.on_hit_block(world, block_hit);
            self.projectile.on_hit_common(world, &hit);
        } else {
            self.projectile.set_pos(target_pos);
        }
    }

    /// Computes Paper's exact arrow damage for current velocity, `base_damage`, and `crit_arrow`.
    ///
    /// ```java
    /// float pow = (float) this.getDeltaMovement().length();
    /// int damage = Mth.ceil(Mth.clamp(pow * this.baseDamage, 0.0, 2.147483647E9));
    /// if (this.isCritArrow()) {
    ///     long extra = this.random.nextInt(damage / 2 + 2);
    ///     damage = (int) Math.min(extra + damage, 2147483647L);
    /// }
    /// ```
    pub fn compute_arrow_damage<R: RandomSource>(&self, random: &mut R) -> i32 {
        let pow = self.projectile.delta_movement.length() as f32;
        let mut damage = mth::ceil_d(mth::clamp_f64(
            pow as f64 * self.base_damage,
            0.0,
            2_147_483_647.0,
        ));
        if self.crit_arrow {
            let bound = (damage / 2).wrapping_add(2);
            let extra = random.next_int_bound(bound) as i64;
            damage = (extra + damage as i64).min(i32::MAX as i64) as i32;
        }
        damage
    }

    /// `AbstractArrow.onHitEntity(EntityHitResult)`.
    pub fn on_hit_entity<W: ProjectileWorld, R: RandomSource, H: ArrowHooks>(
        &mut self,
        world: &mut W,
        random: &mut R,
        hooks: &mut H,
        entity_hit: EntityHitResult,
    ) {
        let damage = self.compute_arrow_damage(random);

        if self.pierce_level > 0 {
            if self.pierced_entity_ids.len() > (self.pierce_level as usize) {
                self.projectile.discard();
                return;
            }
            self.pierced_entity_ids.insert(entity_hit.entity_id);
        }

        let causing_entity = self.projectile.get_effect_source(world);
        let handoff = DamageHandoff {
            kind: hooks.damage_kind(),
            direct_entity: self.projectile.id,
            causing_entity,
            amount: damage as f32,
            knockback_x: self.projectile.delta_movement.x,
            knockback_z: self.projectile.delta_movement.z,
        };

        let outcome = world.hurt_entity(entity_hit.entity_id, handoff);
        if outcome.hurt_applied {
            if outcome.target_is_enderman {
                return;
            }
            if outcome.target_is_living {
                hooks.post_hurt_effects(self, world, entity_hit.entity_id);
            }
            if self.pierce_level == 0 {
                self.projectile.discard();
            }
        } else {
            // Paper: on failed hurt, deflect in reverse and scale by 0.2.
            self.projectile.deflect(
                ProjectileDeflection::Reverse,
                world.get_entity(entity_hit.entity_id).as_ref(),
                None,
                false,
                random,
            );
            self.projectile.delta_movement = self.projectile.delta_movement.scale(0.2);
            if self.projectile.delta_movement.length_sqr() < 1.0E-7 {
                if self.pickup == Pickup::Allowed {
                    world.spawn_dropped_item(
                        self.projectile.position,
                        self.pickup_item.clone(),
                        0.1,
                    );
                }
                self.projectile.discard();
            }
        }
    }

    /// `AbstractArrow.onHitBlock(BlockHitResult)`.
    pub fn on_hit_block<W: ProjectileWorld>(&mut self, world: &W, block_hit: BlockHitResult) {
        self.last_block_state_id = world.get_block_state_id(block_hit.block_pos);
        let movement = block_hit.location.subtract(self.projectile.position);
        self.projectile.delta_movement = movement;
        let offset = Vec3::new(
            java_signum(movement.x),
            java_signum(movement.y),
            java_signum(movement.z),
        )
        .scale(0.05);
        self.projectile.set_pos(block_hit.location.subtract(offset));
        self.in_ground = true;
        self.shake_time = SHAKE_TIME;
        self.crit_arrow = false;
        self.pierce_level = 0;
        self.reset_pierced_entities();
    }

    /// `AbstractArrow.playerTouch(Player)`.
    ///
    /// Returns `true` if the arrow was picked up and discarded.
    pub fn player_touch<W: ProjectileWorld>(
        &mut self,
        world: &mut W,
        player: &EntitySnapshot,
    ) -> bool {
        if !self.projectile.alive || !player.is_player {
            return false;
        }
        if (self.in_ground || self.is_no_physics())
            && self.shake_time <= 0
            && self.try_pickup(world, player)
        {
            self.projectile.discard();
            return true;
        }
        false
    }

    /// `AbstractArrow.tryPickup(Player)`.
    pub fn try_pickup<W: ProjectileWorld>(&self, world: &mut W, player: &EntitySnapshot) -> bool {
        match self.pickup {
            Pickup::Disallowed => false,
            Pickup::Allowed => world.try_add_to_player_inventory(player.id, &self.pickup_item),
            Pickup::CreativeOnly => player.has_infinite_materials,
        }
    }

    /// `AbstractArrow.addAdditionalSaveData(ValueOutput)`.
    pub fn save_nbt(&self, tag: &mut CompoundTag) {
        self.projectile.save_nbt(tag);
        tag.put_short("life", self.life as i16);
        if let Some(state_id) = self.last_block_state_id {
            tag.put_int("inBlockState", state_id as i32);
        }
        tag.put_byte("shake", self.shake_time as i8);
        tag.put_boolean("inGround", self.in_ground);
        tag.put_byte("pickup", self.pickup.ordinal() as i8);
        tag.put_double("damage", self.base_damage);
        tag.put_boolean("crit", self.crit_arrow);
        tag.put_byte("PierceLevel", self.pierce_level as i8);
        tag.put_string("item", &self.pickup_item.item_id);
        if let Some(weapon) = &self.fired_from_weapon {
            tag.put_string("weapon", weapon);
        }
    }

    /// `AbstractArrow.readAdditionalSaveData(ValueInput)`.
    pub fn load_nbt(&mut self, tag: &CompoundTag) {
        self.projectile.load_nbt(tag);
        self.life = tag.get_short_or("life", 0) as i32;
        self.last_block_state_id = tag.get_int("inBlockState").map(|v| v as u32);
        self.shake_time = (tag.get_byte_or("shake", 0) as u8) as i32;
        self.in_ground = tag.get_boolean_or("inGround", false);
        self.base_damage = tag.get_double_or("damage", ARROW_BASE_DAMAGE);
        self.pickup = Pickup::by_ordinal(tag.get_byte_or("pickup", 0) as i32);
        self.crit_arrow = tag.get_boolean_or("crit", false);
        self.pierce_level = tag.get_byte_or("PierceLevel", 0) as u8;
        if let Some(item_id) = tag.get_string("item") {
            self.pickup_item.item_id = item_id.to_owned();
        }
        self.fired_from_weapon = tag.get_string("weapon").cloned();
    }
}
