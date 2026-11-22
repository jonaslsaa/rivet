//! Port of `net.minecraft.world.entity.projectile.Projectile` (Paper 26.2).
//!
//! Embedded by concrete projectile types (`AbstractArrow`, `Arrow`, `SpectralArrow`,
//! `ThrownTrident`, etc.) per `OWNERSHIP.md` composition rules.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::mth;
use rivet_util::random::RandomSource;

use crate::projectile::phys::{Aabb, BlockHitResult, EntityId, EntityUuid, HitResult, Vec3};
use crate::projectile::projectile_deflection::ProjectileDeflection;
use crate::projectile::world_view::{EntitySnapshot, ProjectileGameEvent, ProjectileWorld};

/// Paper `EntityReference<Entity>`: stores persistent UUID and optional cached runtime ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityReference {
    pub uuid: EntityUuid,
    pub cached_id: Option<EntityId>,
}

impl EntityReference {
    pub const fn new(uuid: EntityUuid, cached_id: Option<EntityId>) -> Self {
        Self { uuid, cached_id }
    }

    pub fn from_snapshot(snapshot: &EntitySnapshot) -> Self {
        Self {
            uuid: snapshot.uuid,
            cached_id: Some(snapshot.id),
        }
    }

    pub fn matches_snapshot(&self, snapshot: &EntitySnapshot) -> bool {
        self.cached_id == Some(snapshot.id) || self.uuid == snapshot.uuid
    }
}

/// Embedded base state for `net.minecraft.world.entity.projectile.Projectile`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileState {
    pub id: EntityId,
    pub position: Vec3,
    pub delta_movement: Vec3,
    pub bb_width: f64,
    pub bb_height: f64,
    pub bounding_box: Aabb,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub tick_count: i32,
    pub alive: bool,
    pub in_water: bool,
    pub no_physics: bool,
    pub needs_sync: bool,
    pub owner: Option<EntityReference>,
    pub left_owner: bool,
    pub left_owner_checked: bool,
    pub has_been_shot: bool,
    pub last_deflected_by: Option<EntityId>,
    /// Paper `hitCancelled` flag for cancellable hit events.
    pub hit_cancelled: bool,
}

impl ProjectileState {
    pub fn new(id: EntityId, position: Vec3, bb_width: f64, bb_height: f64) -> Self {
        let bounding_box = Aabb::from_entity_pos(position, bb_width, bb_height);
        Self {
            id,
            position,
            delta_movement: Vec3::ZERO,
            bb_width,
            bb_height,
            bounding_box,
            y_rot: 0.0,
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            tick_count: 0,
            alive: true,
            in_water: false,
            no_physics: false,
            needs_sync: false,
            owner: None,
            left_owner: false,
            left_owner_checked: false,
            has_been_shot: false,
            last_deflected_by: None,
            hit_cancelled: false,
        }
    }

    /// Updates `position` and recomputes `bounding_box` around it (`Entity.setPos`).
    pub fn set_pos(&mut self, pos: Vec3) {
        self.position = pos;
        self.bounding_box = Aabb::from_entity_pos(pos, self.bb_width, self.bb_height);
    }

    /// Marks the projectile discarded (`Entity.discard`).
    pub fn discard(&mut self) {
        self.alive = false;
    }

    /// `Projectile.setOwner(EntityReference)`.
    pub fn set_owner(&mut self, owner: Option<EntityReference>) {
        self.owner = owner;
    }

    /// `Projectile.setOwner(Entity)` from a snapshot.
    pub fn set_owner_snapshot(&mut self, owner: &EntitySnapshot) {
        self.owner = Some(EntityReference::from_snapshot(owner));
    }

    /// `Projectile.getOwner()`.
    pub fn get_owner<W: ProjectileWorld>(&self, world: &W) -> Option<EntitySnapshot> {
        let owner_ref = self.owner.as_ref()?;
        world.resolve_owner(owner_ref)
    }

    /// `Projectile.ownedBy(Entity)`.
    pub fn owned_by<W: ProjectileWorld>(&self, world: &W, entity: &EntitySnapshot) -> bool {
        if let Some(owner) = self.get_owner(world) {
            return owner.id == entity.id;
        }
        self.owner
            .as_ref()
            .is_some_and(|r| r.matches_snapshot(entity))
    }

    /// `Projectile.getEffectSource()`: returns owner if present, otherwise this projectile's ID.
    pub fn get_effect_source<W: ProjectileWorld>(&self, world: &W) -> EntityId {
        self.get_owner(world).map_or(self.id, |o| o.id)
    }

    /// `Projectile.getMovementToShoot(double xd, double yd, double zd, float pow, float uncertainty)`.
    ///
    /// Exact Paper constant: `0.0172275 * uncertainty` with `RandomSource::triangle_f64`.
    pub fn get_movement_to_shoot<R: RandomSource>(
        xd: f64,
        yd: f64,
        zd: f64,
        pow: f32,
        uncertainty: f32,
        random: &mut R,
    ) -> Vec3 {
        let spread = 0.0172275_f64 * uncertainty as f64;
        Vec3::new(xd, yd, zd)
            .normalize()
            .add_xyz(
                random.triangle_f64(0.0, spread),
                random.triangle_f64(0.0, spread),
                random.triangle_f64(0.0, spread),
            )
            .scale(pow as f64)
    }

    /// `Projectile.shoot(double xd, double yd, double zd, float pow, float uncertainty)`.
    pub fn shoot<R: RandomSource>(
        &mut self,
        xd: f64,
        yd: f64,
        zd: f64,
        pow: f32,
        uncertainty: f32,
        random: &mut R,
    ) {
        let movement = Self::get_movement_to_shoot(xd, yd, zd, pow, uncertainty, random);
        self.delta_movement = movement;
        self.needs_sync = true;
        let sd = movement.horizontal_distance();
        self.y_rot =
            (mth::atan2(movement.x, movement.z) * (180.0_f32 / std::f32::consts::PI) as f64) as f32;
        self.x_rot =
            (mth::atan2(movement.y, sd) * (180.0_f32 / std::f32::consts::PI) as f64) as f32;
        self.y_rot_o = self.y_rot;
        self.x_rot_o = self.x_rot;
    }

    /// `Projectile.shootFromRotation(Entity shooter, float xRot, float yRot, float yOffset, float pow, float uncertainty)`.
    ///
    /// Preserves Paper's `disableRelativeProjectileVelocity` config toggle,
    /// `shooter.onGround() ? 0.0 : shooterMovement.y`, and `Vec3.isFinite()` guard.
    #[allow(clippy::too_many_arguments)]
    pub fn shoot_from_rotation<W: ProjectileWorld, R: RandomSource>(
        &mut self,
        world: &W,
        shooter: &EntitySnapshot,
        x_rot: f32,
        y_rot: f32,
        y_offset: f32,
        pow: f32,
        uncertainty: f32,
        random: &mut R,
    ) {
        let xd = -mth::sin((y_rot * mth::DEG_TO_RAD) as f64)
            * mth::cos((x_rot * mth::DEG_TO_RAD) as f64);
        let yd = -mth::sin(((x_rot + y_offset) * mth::DEG_TO_RAD) as f64);
        let zd =
            mth::cos((y_rot * mth::DEG_TO_RAD) as f64) * mth::cos((x_rot * mth::DEG_TO_RAD) as f64);
        self.shoot(xd as f64, yd as f64, zd as f64, pow, uncertainty, random);

        if !world.disable_relative_projectile_velocity() {
            let shooter_movement = shooter.delta_movement;
            let added = self.delta_movement.add_xyz(
                shooter_movement.x,
                if shooter.on_ground {
                    0.0
                } else {
                    shooter_movement.y
                },
                shooter_movement.z,
            );
            if added.is_finite() {
                self.delta_movement = added;
            }
        }
    }

    /// `Projectile.tick()`: emits `PROJECTILE_SHOOT` once on first tick and updates `left_owner`.
    pub fn tick_base<W: ProjectileWorld>(&mut self, world: &mut W) {
        if !self.has_been_shot {
            let owner_id = self.get_owner(world).map(|o| o.id);
            world.emit_game_event(ProjectileGameEvent::ProjectileShoot {
                projectile_id: self.id,
                owner_id,
            });
            self.has_been_shot = true;
        }

        self.check_left_owner(world);
        self.left_owner_checked = false;
        self.tick_count = self.tick_count.wrapping_add(1);
    }

    /// `Projectile.checkLeftOwner()`.
    pub fn check_left_owner<W: ProjectileWorld>(&mut self, world: &W) {
        if !self.left_owner && !self.left_owner_checked {
            self.left_owner = self.is_outside_owner_collision_range(world);
            self.left_owner_checked = true;
        }
    }

    /// `Projectile.isOutsideOwnerCollisionRange()`.
    pub fn is_outside_owner_collision_range<W: ProjectileWorld>(&self, world: &W) -> bool {
        let Some(owner) = self.get_owner(world) else {
            return true;
        };

        let search = self
            .bounding_box
            .expand_towards(self.delta_movement)
            .inflate(1.0);
        for entity in world.get_entities(self.id, search) {
            if !entity.spectator
                && entity.can_be_picked
                && entity.root_vehicle_id == owner.root_vehicle_id
            {
                return false;
            }
        }
        true
    }

    /// `Projectile.canHitEntity(Entity entity)`.
    pub fn can_hit_entity<W: ProjectileWorld>(&self, world: &W, entity: &EntitySnapshot) -> bool {
        if !entity.can_be_hit_by_projectile {
            return false;
        }
        let Some(owner) = self.get_owner(world) else {
            return true;
        };
        if self.left_owner {
            return true;
        }
        // `!owner.isPassengerOfSameVehicle(entity)`
        owner.root_vehicle_id != entity.root_vehicle_id
    }

    /// `Projectile.deflect(ProjectileDeflection, Entity, EntityReference, boolean byAttack)`.
    pub fn deflect<R: RandomSource>(
        &mut self,
        deflection: ProjectileDeflection,
        entity: Option<&EntitySnapshot>,
        new_owner: Option<EntityReference>,
        _by_attack: bool,
        random: &mut R,
    ) -> bool {
        deflection.apply(self, entity, random);
        if deflection != ProjectileDeflection::None {
            self.owner = new_owner;
            self.last_deflected_by = entity.map(|e| e.id);
            true
        } else {
            false
        }
    }

    /// Paper `Projectile.hitTargetOrDeflectSelf(HitResult)`:
    /// handles entity deflection and world border bounce before invoking `on_hit`.
    /// Returns the `ProjectileDeflection` that occurred (`None` if normal hit proceeded).
    pub fn evaluate_deflection_or_world_border<W: ProjectileWorld, R: RandomSource>(
        &mut self,
        world: &mut W,
        hit_result: &HitResult,
        should_bounce_on_world_border: bool,
        random: &mut R,
    ) -> ProjectileDeflection {
        match hit_result {
            HitResult::Entity(entity_hit) => {
                if let Some(target) = world.get_entity(entity_hit.entity_id)
                    && target.deflection != ProjectileDeflection::None
                {
                    let new_owner = Some(EntityReference::from_snapshot(&target));
                    if self.deflect(target.deflection, Some(&target), new_owner, false, random) {
                        return target.deflection;
                    }
                }
                ProjectileDeflection::None
            }
            HitResult::Block(block_hit) => {
                if should_bounce_on_world_border && block_hit.world_border_hit {
                    let current_owner = self.owner;
                    if self.deflect(
                        ProjectileDeflection::Reverse,
                        None,
                        current_owner,
                        false,
                        random,
                    ) {
                        self.delta_movement = self.delta_movement.scale(0.2);
                        return ProjectileDeflection::Reverse;
                    }
                }
                ProjectileDeflection::None
            }
            HitResult::Miss(_) => ProjectileDeflection::None,
        }
    }

    /// `Projectile.onHit(HitResult)` base side effects: emits `PROJECTILE_LAND`
    /// and redirects redirectable projectiles when hitting one.
    pub fn on_hit_common<W: ProjectileWorld>(&self, world: &mut W, hit_result: &HitResult) {
        match hit_result {
            HitResult::Entity(entity_hit) => {
                if let Some(target) = world.get_entity(entity_hit.entity_id)
                    && target.is_redirectable_projectile
                {
                    let owner_id = self.get_owner(world).map(|o| o.id);
                    world.redirect_projectile(target.id, owner_id, self.owner);
                }
                world.emit_game_event(ProjectileGameEvent::ProjectileLandEntity {
                    projectile_id: self.id,
                    location: entity_hit.location,
                });
            }
            HitResult::Block(block_hit) => {
                let state_id = world.get_block_state_id(block_hit.block_pos);
                world.emit_game_event(ProjectileGameEvent::ProjectileLandBlock {
                    projectile_id: self.id,
                    block_pos: block_hit.block_pos,
                    block_state_id: state_id,
                });
            }
            HitResult::Miss(_) => {}
        }
    }

    /// `Projectile.updateRotation()`.
    pub fn update_rotation(&mut self) {
        let movement = self.delta_movement;
        let sd = movement.horizontal_distance();
        self.x_rot = Self::lerp_rotation(
            self.x_rot_o,
            (mth::atan2(movement.y, sd) * (180.0_f32 / std::f32::consts::PI) as f64) as f32,
        );
        self.y_rot = Self::lerp_rotation(
            self.y_rot_o,
            (mth::atan2(movement.x, movement.z) * (180.0_f32 / std::f32::consts::PI) as f64) as f32,
        );
    }

    /// `Projectile.lerpRotation(float rotO, float rot)`.
    ///
    /// Exact Paper loop-free formulation:
    /// `rotO += Math.round((rot - rotO) / 360.0F) * 360.0F; return Mth.lerp(0.2F, rotO, rot);`
    pub fn lerp_rotation(mut rot_o: f32, rot: f32) -> f32 {
        // Java `Math.round(float)` is `floor(x + 0.5)` for finite numbers.
        let turns = java_round_f32((rot - rot_o) / 360.0_f32) as f32;
        rot_o += turns * 360.0_f32;
        mth::lerp_f32(0.2_f32, rot_o, rot)
    }

    /// `Projectile.addAdditionalSaveData(ValueOutput)`.
    pub fn save_nbt(&self, tag: &mut CompoundTag) {
        if let Some(owner) = &self.owner {
            tag.put_int_array("Owner", owner.uuid.to_int_array());
        }
        if self.left_owner {
            tag.put_boolean("LeftOwner", true);
        }
        tag.put_boolean("HasBeenShot", self.has_been_shot);
    }

    /// `Projectile.readAdditionalSaveData(ValueInput)`.
    pub fn load_nbt(&mut self, tag: &CompoundTag) {
        self.owner = tag
            .get_int_array("Owner")
            .and_then(|v| EntityUuid::from_int_array(v))
            .map(|uuid| EntityReference::new(uuid, None));
        self.left_owner = tag.get_boolean_or("LeftOwner", false);
        self.has_been_shot = tag.get_boolean_or("HasBeenShot", false);
    }
}

/// Java `Math.round(float)`: `floor(x + 0.5)` (towards +infinity on half-way ties).
fn java_round_f32(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    (x + 0.5_f32).floor() as i32
}

/// Helper to construct a block hit result in tests or callers.
pub fn block_hit_to_result(hit: BlockHitResult) -> HitResult {
    if hit.miss {
        HitResult::Miss(hit)
    } else {
        HitResult::Block(hit)
    }
}
