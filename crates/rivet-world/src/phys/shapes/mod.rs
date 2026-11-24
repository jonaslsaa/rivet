//! `net.minecraft.world.phys.shapes` — voxel shapes, `Shapes` and the
//! collision-context interface. `EntityCollisionContext` and
//! `MinecartCollisionContext` hold an `Entity`, so those classes and the
//! members of `CollisionContext`/`PositionCollisionContext` that need live
//! types (`BlockState`, `FluidState`, `CollisionGetter`, `Entity`) are in
//! `rivet-server` at the same mirrored path (`CollisionContextLive`).

pub mod array_voxel_shape;
pub mod bit_set_discrete_voxel_shape;
pub mod boolean_op;
pub mod collision_context;
pub mod cube_point_range;
pub mod cube_voxel_shape;
pub mod discrete_cube_merger;
pub mod discrete_voxel_shape;
pub mod identical_merger;
pub mod index_merger;
pub mod indirect_merger;
pub mod non_overlapping_merger;
pub mod offset_double_list;
pub mod position_collision_context;
#[allow(clippy::module_inception)] // `Shapes` is the class that gives the package its name
pub mod shapes;
pub mod slice_shape;
pub mod sub_shape;
pub mod voxel_shape;

#[cfg(test)]
mod tests;
