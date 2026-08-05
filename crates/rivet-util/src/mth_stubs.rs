//! STUB(mc.util.mth) — minimal cross-unit types referenced by `mth.rs`, owned
//! by other units. Only the surface `Mth` actually calls is provided here.
//! These become real ports when their owning units land.
//!
//! - `Vec3` / `AABB` — owned by `net.minecraft.world.phys` (rivet-util).
//! - `Vec3f` / `Quaternionf` — owned by JOML (`org.joml`).
//! - `Fraction` — owned by Apache Commons Lang (`org.apache.commons.lang3.math`).
//!
//! `net.minecraft.util.RandomSource` is NOT stubbed here — `mth.rs` uses the
//! real trait from `random.rs` (unit `mc.util.random`).

/// `net.minecraft.world.phys.Vec3` — the double-precision 3-vector.
#[derive(Clone, Copy, Debug)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    /// `Vec3(double, double, double)`.
    pub fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }
}

/// `net.minecraft.world.phys.AABB` — axis-aligned bounding box.
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// `org.joml.Vector3f` — the float 3-vector (Mth reads it through the
/// `Vector3fc` immutable view).
#[derive(Clone, Copy, Debug)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3f {
    /// `Vector3f.dot(float x, float y, float z)` — JOML uses fused `Math.fma`
    /// (verified against joml-1.10.8 bytecode), so this uses `mul_add`.
    pub fn dot(&self, x: f32, y: f32, z: f32) -> f32 {
        self.x.mul_add(x, self.y.mul_add(y, self.z * z))
    }
}

/// `org.joml.Quaternionf`.
#[derive(Clone, Copy, Debug)]
pub struct Quaternionf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternionf {
    /// `Quaternionf.normalize()` — JOML computes `invsqrt(fma(x,x, fma(y,y,
    /// fma(z,z, w*w))))` with `Math.invsqrt = 1.0f / (float)Math.sqrt`, then
    /// scales each component by it (verified against joml-1.10.8 bytecode).
    pub fn normalized(&self) -> Quaternionf {
        let len2 = self.x.mul_add(
            self.x,
            self.y
                .mul_add(self.y, self.z.mul_add(self.z, self.w * self.w)),
        );
        let inv_norm = 1.0f32 / len2.sqrt();
        Quaternionf {
            x: self.x * inv_norm,
            y: self.y * inv_norm,
            z: self.z * inv_norm,
            w: self.w * inv_norm,
        }
    }
}

/// `org.apache.commons.lang3.math.Fraction`.
pub struct Fraction {
    pub numerator: i32,
    pub denominator: i32,
}
