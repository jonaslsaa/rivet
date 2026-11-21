//! Single-owner synchronous world seam consumed by projectiles.
//!
//! Follows `OWNERSHIP.md`: entities are addressed by `EntityId` / `EntityUuid`,
//! and all queries/side effects cross an explicit synchronous trait boundary
//! without `Arc<RwLock>` shared state.

use crate::projectile::arrow::PotionContents;
use crate::projectile::phys::{
    Aabb, BlockHitResult, BlockPos, ClipBlock, EntityId, EntityUuid, Vec3,
};
use crate::projectile::projectile::EntityReference;
use crate::projectile::projectile_deflection::ProjectileDeflection;

/// Snapshot of an entity in the world used for projectile collision, ownership, and return logic.
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySnapshot {
    pub id: EntityId,
    pub uuid: EntityUuid,
    pub position: Vec3,
    pub eye_position: Vec3,
    pub delta_movement: Vec3,
    pub look_angle: Vec3,
    pub bounding_box: Aabb,
    pub bb_width: f64,
    pub on_ground: bool,
    pub alive: bool,
    pub spectator: bool,
    pub is_player: bool,
    pub has_infinite_materials: bool,
    pub can_be_hit_by_projectile: bool,
    pub can_be_picked: bool,
    pub root_vehicle_id: EntityId,
    pub deflection: ProjectileDeflection,
    pub is_redirectable_projectile: bool,
}

/// Damage kind handed off from a projectile to the target entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageKind {
    Arrow,
    Trident,
}

/// Structured damage handoff produced by `AbstractArrow::on_hit_entity` / `ThrownTrident::on_hit_entity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageHandoff {
    pub kind: DamageKind,
    pub direct_entity: EntityId,
    /// Causing entity (`owner` when present, otherwise `direct_entity`).
    pub causing_entity: EntityId,
    pub amount: f32,
    pub knockback_x: f64,
    pub knockback_z: f64,
}

/// Result of calling `ProjectileWorld::hurt_entity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HurtOutcome {
    pub hurt_applied: bool,
    pub target_is_living: bool,
    /// True for entities like Endermen that dodge arrows (`EntityType.ENDERMAN`).
    pub target_is_enderman: bool,
}

impl HurtOutcome {
    pub const fn living_hurt() -> Self {
        Self {
            hurt_applied: true,
            target_is_living: true,
            target_is_enderman: false,
        }
    }

    pub const fn ignored() -> Self {
        Self {
            hurt_applied: false,
            target_is_living: true,
            target_is_enderman: false,
        }
    }
}

/// Post-hurt mob effect handoff (tipped arrows, spectral arrows).
#[derive(Debug, Clone, PartialEq)]
pub struct PostHurtEffect {
    pub effect_id: String,
    pub duration: i32,
    pub amplifier: i32,
    pub effect_source: EntityId,
}

/// Lightweight pickup item representation for arrows and tridents.
#[derive(Debug, Clone, PartialEq)]
pub struct PickupItem {
    pub item_id: String,
    pub potion_contents: Option<PotionContents>,
    pub loyalty: u8,
    pub foil: bool,
}

impl PickupItem {
    pub fn simple(item_id: impl Into<String>) -> Self {
        Self {
            item_id: item_id.into(),
            potion_contents: None,
            loyalty: 0,
            foil: false,
        }
    }
}

/// Game events emitted by `Projectile` / `AbstractArrow`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectileGameEvent {
    ProjectileShoot {
        projectile_id: EntityId,
        owner_id: Option<EntityId>,
    },
    ProjectileLandEntity {
        projectile_id: EntityId,
        location: Vec3,
    },
    ProjectileLandBlock {
        projectile_id: EntityId,
        block_pos: BlockPos,
        block_state_id: Option<u32>,
    },
}

/// Synchronous world interface consumed by projectiles.
pub trait ProjectileWorld {
    /// `Level.clipIncludingBorder(ClipContext)`.
    fn clip_including_border(
        &self,
        from: Vec3,
        to: Vec3,
        clip_block: ClipBlock,
        source_id: EntityId,
    ) -> BlockHitResult;

    /// Returns candidate entities in `search_area` (excluding `source_id`) in deterministic
    /// level iteration order.
    fn get_entities(&self, source_id: EntityId, search_area: Aabb) -> Vec<EntitySnapshot>;

    /// Looks up a single entity by runtime ID.
    fn get_entity(&self, id: EntityId) -> Option<EntitySnapshot>;

    /// Resolves an `EntityReference` (by cached ID or UUID) in the current level.
    fn resolve_owner(&self, owner: &EntityReference) -> Option<EntitySnapshot>;

    /// Returns `owner.getRootVehicle().getSelfAndPassengers()` for `isOutsideOwnerCollisionRange`.
    fn get_owner_vehicle_and_passengers(&self, owner_id: EntityId) -> Vec<EntitySnapshot>;

    /// Returns the block state ID at `pos` (`Level.getBlockState(pos)`).
    fn get_block_state_id(&self, pos: BlockPos) -> Option<u32>;

    /// Returns true if `level.noCollision(new AABB(pos, pos).inflate(0.06))` (Paper `AbstractArrow.shouldFall`).
    fn has_no_collision_around(&self, pos: Vec3, inflate: f64) -> bool;

    /// Paper config `misc.disableRelativeProjectileVelocity`.
    fn disable_relative_projectile_velocity(&self) -> bool {
        false
    }

    /// Applies damage handoff (`Entity.hurtOrSimulate`).
    fn hurt_entity(&mut self, target_id: EntityId, handoff: DamageHandoff) -> HurtOutcome;

    /// Applies a post-hurt status effect (`LivingEntity.addEffect`).
    fn apply_post_hurt_effect(&mut self, target_id: EntityId, effect: PostHurtEffect);

    /// Triggers `AIM_DEFLECT` on a redirectable projectile hit by another projectile.
    fn redirect_projectile(
        &mut self,
        _target_id: EntityId,
        _owner_id: Option<EntityId>,
        _owner_ref: Option<EntityReference>,
    ) {
    }

    /// Emits a game event (`PROJECTILE_SHOOT`, `PROJECTILE_LAND`).
    fn emit_game_event(&mut self, event: ProjectileGameEvent);

    /// Attempts to add a picked-up projectile item to a player's inventory.
    fn try_add_to_player_inventory(&mut self, player_id: EntityId, item: &PickupItem) -> bool;

    /// Spawns a dropped item entity in the world (e.g. when a loyal trident's owner dies/spectates).
    fn spawn_dropped_item(&mut self, pos: Vec3, item: PickupItem, y_offset: f32);
}
