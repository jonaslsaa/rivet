//! Port of `net.minecraft.world.entity.projectile.ProjectileUtil` (Paper 26.2).
//!
//! Preserves exact Paper collision ordering:
//! 1. Clip blocks (including world border) along `[from, from + delta]`.
//! 2. Truncate `to` to the block hit location if blocks were hit.
//! 3. Search entities in `source.bounding_box.expand_towards(&delta).inflate(1.0)`
//!    with `entity.bounding_box.inflate(entity_margin)`.
//! 4. Single-hit picks strict nearest `from.distance_to_sqr(&hit_loc)` (`<`, first wins ties).
//! 5. Multi-hit collects all intersecting entities in level iteration order (caller sorts).

use rivet_registry::world::phys::HitResultBehavior;
use rivet_registry::world::phys::hit_result::Type as HitResultType;
use rivet_util::mth;

use crate::world::entity::projectile::phys::ClipBlock;
use crate::world::entity::projectile::projectile::ProjectileState;
use crate::world::entity::projectile::world_view::{EntitySnapshot, ProjectileWorld};
use crate::world::phys::entity_hit_result::EntityHitResult;
use rivet_registry::world::phys::AABB;
use rivet_registry::world::phys::Vec3;

/// Paper `ProjectileUtil.computeMargin(Entity source)`:
/// `Math.max(0.0F, Math.min(0.3F, (source.tickCount - 2) / 20.0F))`.
pub fn compute_margin(tick_count: i32) -> f32 {
    let raw = tick_count.wrapping_sub(2) as f32 / 20.0_f32;
    raw.clamp(0.0_f32, 0.3_f32)
}

/// Paper `ProjectileUtil.getHitResultOnMoveVector(Entity source, Predicate<Entity> matching)`.
pub fn get_hit_result_on_move_vector<W, F>(
    world: &W,
    source: &ProjectileState,
    matching: F,
) -> Box<dyn HitResultBehavior>
where
    W: ProjectileWorld,
    F: Fn(&EntitySnapshot) -> bool,
{
    get_hit_result(
        source.position,
        source,
        matching,
        source.delta_movement,
        world,
        compute_margin(source.tick_count),
        ClipBlock::Collider,
    )
}

/// Paper `ProjectileUtil.getHitResult(...)`.
pub fn get_hit_result<W, F>(
    from: Vec3,
    source: &ProjectileState,
    matching: F,
    delta: Vec3,
    world: &W,
    entity_margin: f32,
    clip_block: ClipBlock,
) -> Box<dyn HitResultBehavior>
where
    W: ProjectileWorld,
    F: Fn(&EntitySnapshot) -> bool,
{
    let mut to = from.add(&delta);
    let block_hit = world.clip_including_border(from, to, clip_block, source.id);
    if block_hit.get_type() != HitResultType::Miss {
        to = block_hit.get_location();
    }
    let mut hit_result: Box<dyn HitResultBehavior> = Box::new(block_hit);

    let search_area = source.bounding_box.expand_towards(&delta).inflate(1.0);
    if let Some(entity_hit) = get_entity_hit_result(
        world,
        source,
        from,
        to,
        search_area,
        matching,
        entity_margin,
    ) {
        hit_result = Box::new(entity_hit);
    }

    hit_result
}

/// Paper `ProjectileUtil.getEntityHitResult(Level, Entity, Vec3 from, Vec3 to, AABB, Predicate, float margin)`.
///
/// Uses strict `<` comparison on `from.distance_to_sqr(&location)` so earlier entities in
/// deterministic level iteration order win exact distance ties.
pub fn get_entity_hit_result<W, F>(
    world: &W,
    source: &ProjectileState,
    from: Vec3,
    to: Vec3,
    target_search_area: AABB,
    matching: F,
    entity_margin: f32,
) -> Option<EntityHitResult>
where
    W: ProjectileWorld,
    F: Fn(&EntitySnapshot) -> bool,
{
    let mut nearest = f64::MAX;
    let mut nearest_location: Option<Vec3> = None;
    let mut hit_entity: Option<EntitySnapshot> = None;

    for entity in world.get_entities(source.id, target_search_area) {
        if !matching(&entity) {
            continue;
        }
        let bb = entity.bounding_box.inflate(entity_margin as f64);
        if let Some(location) = bb.clip(&from, &to) {
            let dd = from.distance_to_sqr(&location);
            if dd < nearest {
                hit_entity = Some(entity);
                nearest = dd;
                nearest_location = Some(location);
            }
        }
    }

    match (hit_entity, nearest_location) {
        (Some(e), Some(loc)) => Some(EntityHitResult::new(e.id, loc)),
        _ => None,
    }
}

/// Paper `ProjectileUtil.getManyEntityHitResult(...)`.
///
/// Preserves level iteration order; callers such as `AbstractArrow::step_move_and_hit`
/// sort the collected results by `initial_position.distance_to_sqr(&entity.position)`.
#[allow(clippy::too_many_arguments)]
pub fn get_many_entity_hit_result<W, F>(
    world: &W,
    source: &ProjectileState,
    from: Vec3,
    to: Vec3,
    target_search_area: AABB,
    matching: F,
    entity_margin: f32,
    include_inside: bool,
) -> Vec<EntityHitResult>
where
    W: ProjectileWorld,
    F: Fn(&EntitySnapshot) -> bool,
{
    let mut collector = Vec::new();
    for entity in world.get_entities(source.id, target_search_area) {
        if !matching(&entity) {
            continue;
        }
        let bb = entity.bounding_box.inflate(entity_margin as f64);
        if include_inside && bb.contains(&from) {
            collector.push(EntityHitResult::new(entity.id, from));
        } else if let Some(location) = bb.clip(&from, &to) {
            collector.push(EntityHitResult::new(entity.id, location));
        }
    }
    collector
}

/// Paper `ProjectileUtil.rotateTowardsMovement(Entity projectile, float rotationSpeed)`.
pub fn rotate_towards_movement(projectile: &mut ProjectileState, rotation_speed: f32) {
    let movement = projectile.delta_movement;
    if movement.length_sqr() != 0.0 {
        let sd = movement.horizontal_distance();
        projectile.y_rot = (mth::atan2(movement.z, movement.x)
            * (180.0_f32 / std::f32::consts::PI) as f64) as f32
            + 90.0_f32;
        projectile.x_rot = (mth::atan2(sd, movement.y) * (180.0_f32 / std::f32::consts::PI) as f64)
            as f32
            - 90.0_f32;

        while projectile.x_rot - projectile.x_rot_o < -180.0_f32 {
            projectile.x_rot_o -= 360.0_f32;
        }
        while projectile.x_rot - projectile.x_rot_o >= 180.0_f32 {
            projectile.x_rot_o += 360.0_f32;
        }
        while projectile.y_rot - projectile.y_rot_o < -180.0_f32 {
            projectile.y_rot_o -= 360.0_f32;
        }
        while projectile.y_rot - projectile.y_rot_o >= 180.0_f32 {
            projectile.y_rot_o += 360.0_f32;
        }

        projectile.x_rot = mth::lerp_f32(rotation_speed, projectile.x_rot_o, projectile.x_rot);
        projectile.y_rot = mth::lerp_f32(rotation_speed, projectile.y_rot_o, projectile.y_rot);
    }
}
