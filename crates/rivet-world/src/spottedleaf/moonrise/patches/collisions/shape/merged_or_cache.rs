//! `ca.spottedleaf.moonrise.patches.collisions.shape.MergedORCache` — one slot
//! of `VoxelShape`'s identity-keyed `orUnoptimized` cache.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/moonrise/patches/collisions/shape/MergedORCache.java`.
//!
//! Java's GC reclaims two shapes that cache each other. Strong `Arc`s in both
//! directions would leak the pair, so the key is a `Weak` reference: a `Weak`
//! keeps its allocation (so its address is never reused) and compares equal
//! only to the shape it was made from.

use std::sync::Weak;

use crate::phys::shapes::voxel_shape::{VoxelShape, VoxelShapeData};

/// `MergedORCache(VoxelShape key, VoxelShape result)`.
#[derive(Clone, Debug)]
pub struct MergedORCache {
    key: Weak<VoxelShapeData>,
    result: VoxelShape,
}

impl MergedORCache {
    /// `new MergedORCache(key, result)`.
    pub fn new(key: &VoxelShape, result: VoxelShape) -> MergedORCache {
        MergedORCache {
            key: key.downgrade(),
            result,
        }
    }

    /// `cache.key() == shape` (reference identity).
    pub fn key_is(&self, shape: &VoxelShape) -> bool {
        Weak::ptr_eq(&self.key, &shape.downgrade())
    }

    /// `MergedORCache.result()`.
    pub fn result(&self) -> VoxelShape {
        self.result.clone()
    }
}
