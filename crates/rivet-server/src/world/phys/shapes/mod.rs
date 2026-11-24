//! `net.minecraft.world.phys.shapes` — the live members (D15 split rule).
//! `EntityCollisionContext` and `MinecartCollisionContext` hold an `Entity`;
//! the voxel shapes, `Shapes`, `CollisionContext` and
//! `PositionCollisionContext` are `rivet_world::phys::shapes`.

pub mod collision_context;
pub mod entity_collision_context;
pub mod minecart_collision_context;
