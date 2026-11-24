//! `ca.spottedleaf.moonrise.patches.collisions.shape.CachedToAABBs` — the
//! cached `VoxelShape.toAabbs()` list, optionally carrying a lazily applied
//! offset (so `move()` does not rebuild the list).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/moonrise/patches/collisions/shape/CachedToAABBs.java`.

use std::sync::Arc;

use rivet_registry::world::phys::AABB;

/// `CachedToAABBs` (a Java record).
#[derive(Clone, Debug)]
pub struct CachedToAABBs {
    pub aabbs: Arc<Vec<AABB>>,
    pub is_offset: bool,
    pub off_x: f64,
    pub off_y: f64,
    pub off_z: f64,
}

impl CachedToAABBs {
    /// `CachedToAABBs.removeOffset()`.
    pub fn remove_offset(&self) -> CachedToAABBs {
        let ret: Vec<AABB> = self
            .aabbs
            .iter()
            .map(|aabb| aabb.move_xyz(self.off_x, self.off_y, self.off_z))
            .collect();
        CachedToAABBs {
            aabbs: Arc::new(ret),
            is_offset: false,
            off_x: 0.0,
            off_y: 0.0,
            off_z: 0.0,
        }
    }

    /// `CachedToAABBs.offset(cache, offX, offY, offZ)`.
    pub fn offset(cache: &CachedToAABBs, off_x: f64, off_y: f64, off_z: f64) -> CachedToAABBs {
        if off_x == 0.0 && off_y == 0.0 && off_z == 0.0 {
            return cache.clone();
        }

        let res_x = cache.off_x + off_x;
        let res_y = cache.off_y + off_y;
        let res_z = cache.off_z + off_z;
        CachedToAABBs {
            aabbs: Arc::clone(&cache.aabbs),
            is_offset: true,
            off_x: res_x,
            off_y: res_y,
            off_z: res_z,
        }
    }
}
