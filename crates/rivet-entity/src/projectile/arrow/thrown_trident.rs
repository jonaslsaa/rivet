//! Port of `net.minecraft.world.entity.projectile.arrow.ThrownTrident` (Paper 26.2).
//!
//! Preserves exact Paper semantics for:
//! - `TridentItem.BASE_DAMAGE` (`8.0F`)
//! - `ThrownTrident.getWaterInertia()` (`0.99F`)
//! - `in_ground_time > 4` marking `dealt_damage = true`
//! - Loyalty return flight (`no_physics = true`, `y += vec.y * 0.015 * loyalty`,
//!   `delta = delta * 0.95 + vec.normalize() * (0.05 * loyalty)`)
//! - Owner death/spectator handling (drop if `Pickup::Allowed`, then discard)
//! - Non-player owner return discard when within `bb_width + 1.0` of owner eye position
//! - `onHitEntity`: fixed `8.0F` damage, `dealt_damage = true`, reverse deflection scaled by `(0.02, 0.2, 0.02)`
//! - `playerTouch` & `tryPickup`: only owner (or ownerless) can touch; `no_physics && owned_by` pickup
//! - `DealtDamage` NBT persistence

use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::random::RandomSource;

use crate::projectile::arrow::abstract_arrow::{AbstractArrow, ArrowHooks, Pickup};
use crate::projectile::phys::{
    BlockHitResult, ClipBlock, Direction, EntityHitResult, EntityId, HitResult, Vec3,
};
use crate::projectile::projectile_deflection::ProjectileDeflection;
use crate::projectile::projectile_util;
use crate::projectile::world_view::{
    DamageHandoff, DamageKind, EntitySnapshot, PickupItem, ProjectileWorld,
};

/// `TridentItem.BASE_DAMAGE`.
pub const TRIDENT_BASE_DAMAGE: f32 = 8.0;
/// `ThrownTrident.getWaterInertia()` (`0.99F`).
pub const TRIDENT_WATER_INERTIA: f32 = 0.99;

/// Port of `net.minecraft.world.entity.projectile.arrow.ThrownTrident`.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrownTrident {
    pub abstract_arrow: AbstractArrow,
    pub dealt_damage: bool,
    pub loyalty: u8,
    pub foil: bool,
    pub client_side_return_trident_tick_count: i32,
}

impl ThrownTrident {
    pub fn new(id: EntityId, position: Vec3, loyalty: u8, foil: bool) -> Self {
        let mut item = PickupItem::simple("minecraft:trident");
        item.loyalty = loyalty;
        item.foil = foil;
        let mut abstract_arrow = AbstractArrow::new(id, position, item);
        abstract_arrow.base_damage = TRIDENT_BASE_DAMAGE as f64;
        Self {
            abstract_arrow,
            dealt_damage: false,
            loyalty,
            foil,
            client_side_return_trident_tick_count: 0,
        }
    }

    pub fn is_acceptable_return_owner(owner: Option<&EntitySnapshot>) -> bool {
        match owner {
            Some(e) if e.alive => !(e.is_player && e.spectator),
            _ => false,
        }
    }

    /// `ThrownTrident.tick()`.
    pub fn tick<W: ProjectileWorld, R: RandomSource>(&mut self, world: &mut W, random: &mut R) {
        if !self.abstract_arrow.projectile.alive {
            return;
        }

        if self.abstract_arrow.in_ground_time > 4 {
            self.dealt_damage = true;
        }

        let owner = self.abstract_arrow.projectile.get_owner(world);
        let loyalty = self.loyalty;
        if loyalty > 0
            && (self.dealt_damage || self.abstract_arrow.is_no_physics())
            && let Some(owner) = owner.as_ref()
        {
            if !Self::is_acceptable_return_owner(Some(owner)) {
                if self.abstract_arrow.pickup == Pickup::Allowed {
                    world.spawn_dropped_item(
                        self.abstract_arrow.projectile.position,
                        self.abstract_arrow.pickup_item.clone(),
                        0.1,
                    );
                }
                self.abstract_arrow.projectile.discard();
                return;
            }

            // Non-player owners collect returning tridents within owner.bb_width + 1.0 of eye pos.
            if !owner.is_player
                && self
                    .abstract_arrow
                    .projectile
                    .position
                    .distance_to(owner.eye_position)
                    < owner.bb_width + 1.0
            {
                self.abstract_arrow.projectile.discard();
                return;
            }

            self.abstract_arrow.set_no_physics(true);
            let vec = owner
                .eye_position
                .subtract(self.abstract_arrow.projectile.position);
            let new_pos = self.abstract_arrow.projectile.position.add_xyz(
                0.0,
                vec.y * 0.015 * loyalty as f64,
                0.0,
            );
            self.abstract_arrow.projectile.set_pos(new_pos);

            let acc = 0.05 * loyalty as f64;
            self.abstract_arrow.projectile.delta_movement = self
                .abstract_arrow
                .projectile
                .delta_movement
                .scale(0.95)
                .add(vec.normalize().scale(acc));
            self.client_side_return_trident_tick_count =
                self.client_side_return_trident_tick_count.wrapping_add(1);
        }

        // Trident uses single-entity hit (not piercing) and skips entity hits once `dealt_damage` is true.
        self.tick_trident_physics(world, random);
    }

    fn tick_trident_physics<W: ProjectileWorld, R: RandomSource>(
        &mut self,
        world: &mut W,
        random: &mut R,
    ) {
        let physics_enabled = !self.abstract_arrow.is_no_physics();
        if self.abstract_arrow.shake_time > 0 {
            self.abstract_arrow.shake_time = self.abstract_arrow.shake_time.wrapping_sub(1);
        }

        if self.abstract_arrow.in_ground && physics_enabled {
            let block_pos = crate::projectile::phys::BlockPos::containing(
                self.abstract_arrow.projectile.position,
            );
            let current_state = world.get_block_state_id(block_pos);
            if self.abstract_arrow.last_block_state_id != current_state
                && self.abstract_arrow.should_fall(world)
            {
                self.abstract_arrow.start_falling(random);
            } else {
                self.tick_despawn();
            }
            self.abstract_arrow.in_ground_time = self.abstract_arrow.in_ground_time.wrapping_add(1);
            self.abstract_arrow.projectile.tick_base(world);
            return;
        }

        self.abstract_arrow.in_ground_time = 0;
        self.abstract_arrow.projectile.check_left_owner(world);

        let initial_position = self.abstract_arrow.projectile.position;
        let movement = self.abstract_arrow.projectile.delta_movement;
        let next_position = initial_position.add(movement);

        let block_hit = if physics_enabled {
            world.clip_including_border(
                initial_position,
                next_position,
                ClipBlock::Collider,
                self.abstract_arrow.projectile.id,
            )
        } else {
            BlockHitResult::miss(
                next_position,
                Direction::Up,
                crate::projectile::phys::BlockPos::containing(next_position),
            )
        };

        let target_pos = if block_hit.miss {
            next_position
        } else {
            block_hit.location
        };

        let entity_hit = if self.dealt_damage {
            None
        } else {
            let search_area = self
                .abstract_arrow
                .projectile
                .bounding_box
                .expand_towards(movement)
                .inflate(1.0);
            let margin = projectile_util::compute_margin(self.abstract_arrow.projectile.tick_count);
            projectile_util::get_entity_hit_result(
                world,
                &self.abstract_arrow.projectile,
                initial_position,
                target_pos,
                search_area,
                |e| self.abstract_arrow.can_hit_entity(world, e),
                margin,
            )
        };

        if let Some(eh) = entity_hit {
            let hit = HitResult::Entity(eh);
            let deflection = self
                .abstract_arrow
                .projectile
                .evaluate_deflection_or_world_border(world, &hit, true, random);
            if deflection == ProjectileDeflection::None {
                self.abstract_arrow.projectile.set_pos(eh.location);
                self.on_hit_entity(world, random, eh);
                self.abstract_arrow.projectile.on_hit_common(world, &hit);
            }
        } else if !block_hit.miss {
            let hit = HitResult::Block(block_hit);
            let deflection = self
                .abstract_arrow
                .projectile
                .evaluate_deflection_or_world_border(world, &hit, true, random);
            if deflection == ProjectileDeflection::None {
                self.abstract_arrow.on_hit_block(world, block_hit);
                self.abstract_arrow.projectile.on_hit_common(world, &hit);
            }
        } else {
            self.abstract_arrow.projectile.set_pos(target_pos);
        }

        if self.abstract_arrow.projectile.alive && !self.abstract_arrow.in_ground {
            let inertia = if self.abstract_arrow.projectile.in_water {
                TRIDENT_WATER_INERTIA
            } else {
                crate::projectile::arrow::abstract_arrow::AIR_INERTIA
            };
            let mut next_delta = self
                .abstract_arrow
                .projectile
                .delta_movement
                .scale(inertia as f64);
            if physics_enabled {
                next_delta = next_delta.add_xyz(
                    0.0,
                    -crate::projectile::arrow::abstract_arrow::DEFAULT_GRAVITY,
                    0.0,
                );
            }
            self.abstract_arrow.projectile.delta_movement = next_delta;
            self.abstract_arrow.projectile.update_rotation();
        }

        // Paper `AbstractArrow.tick()` always calls `super.tick()` (`Projectile.tick()`).
        self.abstract_arrow.projectile.tick_base(world);
    }

    /// `ThrownTrident.onHitEntity(EntityHitResult)`.
    pub fn on_hit_entity<W: ProjectileWorld, R: RandomSource>(
        &mut self,
        world: &mut W,
        random: &mut R,
        entity_hit: EntityHitResult,
    ) {
        let causing_entity = self.abstract_arrow.projectile.get_effect_source(world);
        self.dealt_damage = true;

        let handoff = DamageHandoff {
            kind: DamageKind::Trident,
            direct_entity: self.abstract_arrow.projectile.id,
            causing_entity,
            amount: self.abstract_arrow.base_damage as f32,
            knockback_x: self.abstract_arrow.projectile.delta_movement.x,
            knockback_z: self.abstract_arrow.projectile.delta_movement.z,
        };

        let outcome = world.hurt_entity(entity_hit.entity_id, handoff);
        if outcome.hurt_applied && outcome.target_is_enderman {
            return;
        }

        // Paper: trident always reverse-deflects and scales movement by (0.02, 0.2, 0.02).
        self.abstract_arrow.projectile.deflect(
            ProjectileDeflection::Reverse,
            world.get_entity(entity_hit.entity_id).as_ref(),
            None,
            false,
            random,
        );
        self.abstract_arrow.projectile.delta_movement = self
            .abstract_arrow
            .projectile
            .delta_movement
            .multiply(0.02, 0.2, 0.02);
    }

    /// `ThrownTrident.tickDespawn()`: loyal tridents with `Pickup::Allowed` never despawn.
    pub fn tick_despawn(&mut self) {
        if self.abstract_arrow.pickup != Pickup::Allowed || self.loyalty == 0 {
            self.abstract_arrow.tick_despawn();
        }
    }

    /// `ThrownTrident.playerTouch(Player)`: only the owner (or ownerless trident) can pick it up.
    pub fn player_touch<W: ProjectileWorld>(
        &mut self,
        world: &mut W,
        player: &EntitySnapshot,
    ) -> bool {
        if !self.abstract_arrow.projectile.alive || !player.is_player {
            return false;
        }
        let owned_or_unowned = self.abstract_arrow.projectile.owned_by(world, player)
            || self.abstract_arrow.projectile.owner.is_none();
        if !owned_or_unowned {
            return false;
        }
        if (self.abstract_arrow.in_ground || self.abstract_arrow.is_no_physics())
            && self.abstract_arrow.shake_time <= 0
            && self.try_pickup(world, player)
        {
            self.abstract_arrow.projectile.discard();
            return true;
        }
        false
    }

    /// `ThrownTrident.tryPickup(Player)`.
    pub fn try_pickup<W: ProjectileWorld>(&self, world: &mut W, player: &EntitySnapshot) -> bool {
        self.abstract_arrow.try_pickup(world, player)
            || (self.abstract_arrow.is_no_physics()
                && self.abstract_arrow.projectile.owned_by(world, player)
                && world.try_add_to_player_inventory(player.id, &self.abstract_arrow.pickup_item))
    }

    /// `ThrownTrident.addAdditionalSaveData(ValueOutput)`.
    pub fn save_nbt(&self, tag: &mut CompoundTag) {
        self.abstract_arrow.save_nbt(tag);
        tag.put_boolean("DealtDamage", self.dealt_damage);
        tag.put_byte("Loyalty", self.loyalty as i8);
        tag.put_boolean("Foil", self.foil);
    }

    /// `ThrownTrident.readAdditionalSaveData(ValueInput)`.
    pub fn load_nbt(&mut self, tag: &CompoundTag) {
        self.abstract_arrow.load_nbt(tag);
        self.dealt_damage = tag.get_boolean_or("DealtDamage", false);
        self.loyalty = tag.get_byte_or("Loyalty", self.loyalty as i8) as u8;
        self.foil = tag.get_boolean_or("Foil", self.foil);
        self.abstract_arrow.pickup_item.loyalty = self.loyalty;
        self.abstract_arrow.pickup_item.foil = self.foil;
    }
}

/// Optional hooks adapter if `ThrownTrident` is driven via `AbstractArrow::tick_with_hooks`.
pub struct TridentHooks;
impl ArrowHooks for TridentHooks {
    fn water_inertia(&self) -> f32 {
        TRIDENT_WATER_INERTIA
    }

    fn damage_kind(&self) -> DamageKind {
        DamageKind::Trident
    }
}
