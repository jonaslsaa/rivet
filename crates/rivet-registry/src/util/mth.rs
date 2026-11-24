//! The `net.minecraft.util.Mth` members that take `net.minecraft.world.phys`
//! types. `Mth` lives in `rivet-util`, below `rivet-registry`, where `Vec3` and
//! `AABB` live (they are needed by `core.Direction`/`BlockPos` and
//! `rivet-protocol`), so the two members that name them sit at the same
//! mirrored path here (D15's split rule): the rest of `Mth` is
//! `rivet_util::mth`.
//!
//! The twelve `rayIntersectsAABB` golden vectors that `MthGen` once held, and
//! seeded `rayIntersectsAABB`/`lerp(double, Vec3, Vec3)` cases, are `shapes.eval`
//! oracle inputs (`aabb.mthRayIntersects.*`, `vec3.mthLerp.*`), recorded from
//! Paper and replayed by `rivet-parity ops check`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/util/Mth.java`.

use rivet_util::mth::lerp;

use crate::world::phys::{AABB, Vec3};

/// `Mth.rayIntersectsAABB(Vec3 rayStart, Vec3 rayDir, AABB aabb)` — the
/// slab/Andrew-Woo test.
pub fn ray_intersects_aabb(ray_start: Vec3, ray_dir: Vec3, aabb: AABB) -> bool {
    let center_x = (aabb.min_x + aabb.max_x) * 0.5;
    let box_extent_x = (aabb.max_x - aabb.min_x) * 0.5;
    let diff_x = ray_start.x - center_x;
    if diff_x.abs() > box_extent_x && diff_x * ray_dir.x >= 0.0 {
        return false;
    }
    let center_y = (aabb.min_y + aabb.max_y) * 0.5;
    let box_extent_y = (aabb.max_y - aabb.min_y) * 0.5;
    let diff_y = ray_start.y - center_y;
    if diff_y.abs() > box_extent_y && diff_y * ray_dir.y >= 0.0 {
        return false;
    }
    let center_z = (aabb.min_z + aabb.max_z) * 0.5;
    let box_extent_z = (aabb.max_z - aabb.min_z) * 0.5;
    let diff_z = ray_start.z - center_z;
    if diff_z.abs() > box_extent_z && diff_z * ray_dir.z >= 0.0 {
        return false;
    }
    let andrew_woo_diff_x = ray_dir.x.abs();
    let andrew_woo_diff_y = ray_dir.y.abs();
    let andrew_woo_diff_z = ray_dir.z.abs();
    let mut f = ray_dir.y * diff_z - ray_dir.z * diff_y;
    if f.abs() > box_extent_y * andrew_woo_diff_z + box_extent_z * andrew_woo_diff_y {
        return false;
    }
    f = ray_dir.z * diff_x - ray_dir.x * diff_z;
    if f.abs() > box_extent_x * andrew_woo_diff_z + box_extent_z * andrew_woo_diff_x {
        return false;
    }
    f = ray_dir.x * diff_y - ray_dir.y * diff_x;
    f.abs() < box_extent_x * andrew_woo_diff_y + box_extent_y * andrew_woo_diff_x
}

/// `Mth.lerp(double alpha, Vec3 p1, Vec3 p2)`.
pub fn lerp_vec3(alpha: f64, p1: Vec3, p2: Vec3) -> Vec3 {
    Vec3::new(
        lerp(alpha, p1.x, p2.x),
        lerp(alpha, p1.y, p2.y),
        lerp(alpha, p1.z, p2.z),
    )
}
