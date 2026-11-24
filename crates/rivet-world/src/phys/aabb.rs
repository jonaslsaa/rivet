//! The `net.minecraft.world.phys.AABB` members that take `rivet-world` types
//! (D15 split rule): `AABB` itself is `rivet_registry::world::phys::AABB`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/AABB.java`.

use rivet_registry::world::phys::AABB;

use crate::levelgen::structure::bounding_box::BoundingBox;

/// `AABB.of(BoundingBox)`.
pub fn of(bounding_box: &BoundingBox) -> AABB {
    AABB::new(
        bounding_box.min_x as f64,
        bounding_box.min_y as f64,
        bounding_box.min_z as f64,
        bounding_box.max_x.wrapping_add(1) as f64,
        bounding_box.max_y.wrapping_add(1) as f64,
        bounding_box.max_z.wrapping_add(1) as f64,
    )
}
