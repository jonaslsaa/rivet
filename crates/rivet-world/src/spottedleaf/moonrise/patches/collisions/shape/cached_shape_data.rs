//! `ca.spottedleaf.moonrise.patches.collisions.shape.CachedShapeData` — the
//! flat, immutable view of a `DiscreteVoxelShape`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/moonrise/patches/collisions/shape/CachedShapeData.java`.

/// `CachedShapeData` (a Java record). `voxelSet` is indexed
/// `z + y*sizeZ + x*(sizeZ*sizeY)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedShapeData {
    pub size_x: i32,
    pub size_y: i32,
    pub size_z: i32,
    pub voxel_set: Vec<u64>,
    pub min_full_x: i32,
    pub min_full_y: i32,
    pub min_full_z: i32,
    pub max_full_x: i32,
    pub max_full_y: i32,
    pub max_full_z: i32,
    pub is_empty: bool,
    pub has_single_aabb: bool,
}
