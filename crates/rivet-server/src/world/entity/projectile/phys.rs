//! No Java counterpart of its own: the projectile seam over the canonical
//! `net.minecraft.world.phys` types (`rivet_registry::world::phys`).
//!
//! `Vec3`, `AABB`, `BlockHitResult`, `HitResultBehavior`, `BlockPos` and
//! `Direction` are the canonical definitions; what stays here is the clip-mode
//! enum and the `UUID` int-array form.

use rivet_registry::world::phys::{AABB, Vec3};

/// 128-bit entity UUID stored in Minecraft's four-`i32` big-endian NBT order (`[I; a, b, c, d]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityUuid(pub [i32; 4]);

impl EntityUuid {
    pub const fn new(a: i32, b: i32, c: i32, d: i32) -> Self {
        Self([a, b, c, d])
    }

    pub fn to_int_array(self) -> Vec<i32> {
        self.0.to_vec()
    }

    pub fn from_int_array(ints: &[i32]) -> Option<Self> {
        if ints.len() == 4 {
            Some(Self([ints[0], ints[1], ints[2], ints[3]]))
        } else {
            None
        }
    }
}

/// `net.minecraft.world.level.ClipContext.Block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipBlock {
    Collider,
    Outline,
    Visual,
    FallDamageResetting,
}

/// Java `Math.signum(double)`: returns `0.0` for `±0.0`, `NaN` for `NaN`, `-1.0` for `< 0`, `1.0` for `> 0`.
pub fn java_signum(v: f64) -> f64 {
    if v.is_nan() || v == 0.0 {
        v
    } else if v > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `EntityDimensions.makeBoundingBox(Vec3)` for a `width` x `height` entity:
/// `new AABB(x - w, y, z - w, x + w, y + h, z + w)` with `w = width / 2`.
pub fn bounding_box_at(pos: Vec3, width: f64, height: f64) -> AABB {
    let half = width / 2.0;
    AABB::new(
        pos.x - half,
        pos.y,
        pos.z - half,
        pos.x + half,
        pos.y + height,
        pos.z + half,
    )
}
