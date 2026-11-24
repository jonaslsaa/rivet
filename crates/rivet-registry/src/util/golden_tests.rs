// Golden tests for the `Mth.rayIntersectsAABB` split member. The twelve vectors
// and their expected values are moved unchanged from
// `rivet-util/src/mth_golden_tests.rs` (`golden_22`), where they were computed
// by the real Paper `Mth` class via `tools/rivet-codegen`'s `MthGen`; only the
// `Vec3`/`AABB` constructors changed to the canonical `world.phys` types.
#![allow(clippy::all)]

#[test]
fn golden_ray_intersects_aabb() {
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(5.0, 5.0, 5.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, -1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(2.0, 2.0, 2.0),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, -1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
        false
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(3.0, 3.0, 3.0),
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
        true
    );
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.5, 0.5, 0.5),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
        false
    );
}
