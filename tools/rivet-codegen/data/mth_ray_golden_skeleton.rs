// GENERATED golden tests for the `Mth.rayIntersectsAABB` split member vs the Java Mth oracle
// (tools/rivet-codegen `mth-gen`, data/mth_ray_golden_skeleton.rs). Do not hand-edit.
#![allow(clippy::all)]

#[test]
fn golden_ray_intersects_aabb() {
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1112@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1113@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(5.0, 5.0, 5.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1114@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, -1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1115@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1116@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1117@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(2.0, 2.0, 2.0),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1118@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.0, 0.0, 0.0),
            crate::world::phys::Vec3::new(0.0, 0.0, 1.0),
            crate::world::phys::AABB::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0)
        ),
@@1119@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
@@1120@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(1.0, 1.0, 1.0),
            crate::world::phys::Vec3::new(0.0, 0.0, -1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
@@1121@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(3.0, 3.0, 3.0),
            crate::world::phys::Vec3::new(-1.0, -1.0, -1.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
@@1122@@);
    assert_eq!(
        super::ray_intersects_aabb(
            crate::world::phys::Vec3::new(0.5, 0.5, 0.5),
            crate::world::phys::Vec3::new(1.0, 0.0, 0.0),
            crate::world::phys::AABB::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0)
        ),
@@1123@@);
}
