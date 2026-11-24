//! `ca.spottedleaf.moonrise.patches.collisions.shape` — the cache records that
//! `VoxelShape` and `DiscreteVoxelShape` embed (Paper's "optimise collisions").
//! `CollisionVoxelShape` and `CollisionDiscreteVoxelShape` are interfaces whose
//! only implementors are `VoxelShape` and `DiscreteVoxelShape`; their
//! `moonrise$*` members are those classes' methods.

pub mod cached_shape_data;
pub mod cached_to_aabbs;
pub mod merged_or_cache;
