//! `net.minecraft.world.phys` — the live members (D15 split rule): the types
//! that hold or take an `Entity`. `Vec3`, `Vec2`, `AABB`, `HitResult` and
//! `BlockHitResult` are `rivet_registry::world::phys`; the shape classes are
//! `rivet_world::phys::shapes`.

pub mod entity_hit_result;
pub mod hit_result;
pub mod shapes;
