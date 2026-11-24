//! `net.minecraft.world.phys` classes of `rivet-world`: the shape classes
//! (`shapes`) and the `AABB` members that take `rivet-world` types. `Vec3`,
//! `Vec2`, `AABB`, `HitResult` and `BlockHitResult` live in `rivet-registry`
//! (`rivet_registry::world::phys`, D15: `net.minecraft.core` and
//! `rivet-protocol` need them); `EntityHitResult` holds an entity and lives in
//! `rivet-server`.

// Java's `!(a > b)` is not `a <= b` for NaN; the ports keep the negated form so NaN boxes behave alike.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod aabb;
pub mod shapes;
