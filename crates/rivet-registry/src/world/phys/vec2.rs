//! `net.minecraft.world.phys.Vec2` — the float 2-vector.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/Vec2.java`.
//! Component math is `float` (f32) throughout, as in Java.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::mth;
use rivet_util::util::fixed_size;

/// `Vec2` — an immutable `(x, y)` of floats.
///
/// Java `equals` is `==` on the primitives, so `NaN != NaN` and
/// `0.0 == -0.0`; `PartialEq` derives exactly that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// `Vec2.ZERO`.
pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);
/// `Vec2.ONE`.
pub const ONE: Vec2 = Vec2::new(1.0, 1.0);
/// `Vec2.UNIT_X`.
pub const UNIT_X: Vec2 = Vec2::new(1.0, 0.0);
/// `Vec2.NEG_UNIT_X`.
pub const NEG_UNIT_X: Vec2 = Vec2::new(-1.0, 0.0);
/// `Vec2.UNIT_Y`.
pub const UNIT_Y: Vec2 = Vec2::new(0.0, 1.0);
/// `Vec2.NEG_UNIT_Y`.
pub const NEG_UNIT_Y: Vec2 = Vec2::new(0.0, -1.0);
/// `Vec2.MAX` — `Float.MAX_VALUE` twice.
pub const MAX: Vec2 = Vec2::new(f32::MAX, f32::MAX);
/// `Vec2.MIN` — `Float.MIN_VALUE` (the smallest positive subnormal) twice.
pub const MIN: Vec2 = Vec2::new(f32::from_bits(1), f32::from_bits(1));

impl Vec2 {
    pub const ZERO: Vec2 = ZERO;
    pub const ONE: Vec2 = ONE;
    pub const UNIT_X: Vec2 = UNIT_X;
    pub const NEG_UNIT_X: Vec2 = NEG_UNIT_X;
    pub const UNIT_Y: Vec2 = UNIT_Y;
    pub const NEG_UNIT_Y: Vec2 = NEG_UNIT_Y;
    pub const MAX: Vec2 = MAX;
    pub const MIN: Vec2 = MIN;

    /// `Vec2(float, float)`.
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    /// `Vec2.scale(float)`.
    pub fn scale(&self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }

    /// `Vec2.dot(Vec2)`.
    pub fn dot(&self, v: &Vec2) -> f32 {
        self.x * v.x + self.y * v.y
    }

    /// `Vec2.add(Vec2)`.
    pub fn add(&self, rhs: &Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }

    /// `Vec2.add(float)`.
    pub fn add_value(&self, v: f32) -> Vec2 {
        Vec2::new(self.x + v, self.y + v)
    }

    /// `Vec2.hashCode()` — `Objects.hash(x, y)` = `31 * (31 * 1 + h(x)) + h(y)`
    /// with `Float.hashCode` = `floatToIntBits`.
    pub fn hash_code(&self) -> i32 {
        fn float_hash(value: f32) -> i32 {
            if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits() as i32
            }
        }
        31i32
            .wrapping_mul(31i32.wrapping_add(float_hash(self.x)))
            .wrapping_add(float_hash(self.y))
    }

    /// `Vec2.normalized()` — `dist < 1.0E-4F`.
    pub fn normalized(&self) -> Vec2 {
        let dist = mth::sqrt(self.x * self.x + self.y * self.y);
        if dist < 1.0E-4f32 {
            ZERO
        } else {
            Vec2::new(self.x / dist, self.y / dist)
        }
    }

    /// `Vec2.length()`.
    pub fn length(&self) -> f32 {
        mth::sqrt(self.x * self.x + self.y * self.y)
    }

    /// `Vec2.lengthSquared()`.
    pub fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// `Vec2.distanceToSqr(Vec2)`.
    pub fn distance_to_sqr(&self, p: &Vec2) -> f32 {
        let xd = p.x - self.x;
        let yd = p.y - self.y;
        xd * xd + yd * yd
    }

    /// `Vec2.negated()`.
    pub fn negated(&self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }

    /// `Vec2.rotate(double)`.
    pub fn rotate(&self, angle_radians: f64) -> Vec2 {
        let cosine = mth::cos(angle_radians);
        let sine = mth::sin(angle_radians);
        Vec2::new(
            self.x * cosine - self.y * sine,
            self.y * cosine + self.x * sine,
        )
    }
}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

/// `Vec2.CODEC` — `Codec.FLOAT.listOf().comapFlatMap(Util.fixedSize(input, 2)
/// -> new Vec2(..), vec -> List.of(x, y))`.
pub fn vec2_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec2, Ops>> {
    codec::comap_flat_map::<Vec<f32>, Vec2, Ops>(
        codec::list(codec::float_codec::<Ops>()),
        Arc::new(|floats: &Vec<f32>| fixed_size(floats, 2).map(|f| Vec2::new(f[0], f[1]))),
        Arc::new(|vec: &Vec2| vec![vec.x, vec.y]),
    )
}
