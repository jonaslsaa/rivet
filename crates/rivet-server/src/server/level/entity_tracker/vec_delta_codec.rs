//! Port of `net.minecraft.network.protocol.game.VecDeltaCodec` (Paper 26.2).
//!
//! Encodes entity position deltas relative to a synchronized base position at
//! `1 / 4096` block resolution using Java's `Math.round(double)` semantics.

use rivet_registry::core::Vec3;

/// `VecDeltaCodec.TRUNCATION_STEPS`.
pub const TRUNCATION_STEPS: f64 = 4096.0;

/// Exact Java `Math.round(double)` semantics (IEEE 754 round-half-toward-+inf,
/// NaN -> 0, saturated to `i64::MIN..=i64::MAX`).
#[inline]
pub fn java_math_round(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    if value >= i64::MAX as f64 {
        return i64::MAX;
    }
    if value <= i64::MIN as f64 {
        return i64::MIN;
    }
    // Java's Math.round(double): floor(a + 0.5d), except for values in
    // [-0.5, -0.0] where IEEE 754 `a + 0.5` is exact >= 0.0, and values whose
    // magnitude is >= 2^52 (already integral, where adding 0.5 could round away
    // from exactness in edge ULPs, handled by checking fract == 0.0).
    if value.abs() >= (1_i64 << 52) as f64 {
        return value as i64;
    }
    // Special case: the largest double strictly less than 0.5 (0x1.fffffffffffffp-2)
    // rounds to 0 in Java Math.round, whereas `value + 0.5` rounds to 1.0 in f64.
    const JUST_BELOW_HALF: f64 = f64::from_bits(0x3fdfffffffffffff);
    if value == JUST_BELOW_HALF {
        return 0;
    }
    (value + 0.5).floor() as i64
}

/// `net.minecraft.network.protocol.game.VecDeltaCodec`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VecDeltaCodec {
    pub base: Vec3,
}

impl Default for VecDeltaCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl VecDeltaCodec {
    /// Creates a codec with `base = (0, 0, 0)`.
    pub fn new() -> Self {
        Self {
            base: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    /// `VecDeltaCodec.encode(double)`.
    #[inline]
    pub fn encode(input: f64) -> i64 {
        java_math_round(input * TRUNCATION_STEPS)
    }

    /// `VecDeltaCodec.decode(long)`.
    #[inline]
    pub fn decode_coord(v: i64) -> f64 {
        (v as f64) / TRUNCATION_STEPS
    }

    /// `VecDeltaCodec.decode(long xa, long ya, long za)`.
    pub fn decode(&self, xa: i64, ya: i64, za: i64) -> Vec3 {
        if xa == 0 && ya == 0 && za == 0 {
            return self.base;
        }
        let x = if xa == 0 {
            self.base.x
        } else {
            Self::decode_coord(Self::encode(self.base.x).wrapping_add(xa))
        };
        let y = if ya == 0 {
            self.base.y
        } else {
            Self::decode_coord(Self::encode(self.base.y).wrapping_add(ya))
        };
        let z = if za == 0 {
            self.base.z
        } else {
            Self::decode_coord(Self::encode(self.base.z).wrapping_add(za))
        };
        Vec3::new(x, y, z)
    }

    /// `VecDeltaCodec.encodeX(Vec3)`.
    #[inline]
    pub fn encode_x(&self, pos: Vec3) -> i64 {
        Self::encode(pos.x).wrapping_sub(Self::encode(self.base.x))
    }

    /// `VecDeltaCodec.encodeY(Vec3)`.
    #[inline]
    pub fn encode_y(&self, pos: Vec3) -> i64 {
        Self::encode(pos.y).wrapping_sub(Self::encode(self.base.y))
    }

    /// `VecDeltaCodec.encodeZ(Vec3)`.
    #[inline]
    pub fn encode_z(&self, pos: Vec3) -> i64 {
        Self::encode(pos.z).wrapping_sub(Self::encode(self.base.z))
    }

    /// `VecDeltaCodec.delta(Vec3)`.
    #[inline]
    pub fn delta(&self, pos: Vec3) -> Vec3 {
        Vec3::new(
            pos.x - self.base.x,
            pos.y - self.base.y,
            pos.z - self.base.z,
        )
    }

    /// `VecDeltaCodec.setBase(Vec3)`.
    #[inline]
    pub fn set_base(&mut self, base: Vec3) {
        self.base = base;
    }

    /// `VecDeltaCodec.getBase()`.
    #[inline]
    pub fn get_base(&self) -> Vec3 {
        self.base
    }
}
