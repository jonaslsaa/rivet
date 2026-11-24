//! `net.minecraft.world.phys.Vec3` — the double-precision 3-vector.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/Vec3.java`.
//!
//! Home note (D15, `scripts/coverage_ledger.py` CLASS_CRATES): `net.minecraft.core.
//! Direction`/`BlockPos` and `rivet-protocol`'s `FriendlyByteBuf` take a `Vec3`,
//! and `rivet-world` depends on `rivet-protocol`, so the one definition cannot
//! live above `rivet-registry`.
//!
//! Floating-point order is Java's: every expression below keeps Java's
//! operand order and its float/double widening points (`Mth.cos(float)` yields
//! a float that is widened where it meets a double). `Math.atan2`/`Math.asin`
//! are `StrictMath` (fdlibm) in the JDK; `rotation()` uses
//! [`rivet_util::java_math`], an fdlibm port.
//!
//! The wire codecs (`STREAM_CODEC`, `LP_STREAM_CODEC`) live in `rivet-protocol`,
//! like every other `StreamCodec`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::float_format::java_double_to_string;
use rivet_util::mth;
use rivet_util::mth_stubs::Vec3f;
use rivet_util::random::RandomSource;
use rivet_util::util::fixed_size;

use super::vec2::Vec2;
use crate::core::{Axis, Direction, Position, Vec3i, Vec3iLike};

/// `Vec3` — an immutable `(x, y, z)` of doubles.
///
/// Java `equals` is `Double.compare(..) == 0` per component, so `NaN` equals
/// `NaN` and `0.0` differs from `-0.0`; `PartialEq`/`Hash` follow that.
#[derive(Clone, Copy, Debug)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `Vec3.ZERO`.
pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
/// `Vec3.X_AXIS`.
pub const X_AXIS: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// `Vec3.Y_AXIS`.
pub const Y_AXIS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// `Vec3.Z_AXIS`.
pub const Z_AXIS: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// `Double.doubleToLongBits` — every NaN maps to the canonical `0x7ff8...`.
fn double_to_long_bits(value: f64) -> i64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits() as i64
    }
}

/// `(int)(temp ^ temp >>> 32)` of `Vec3.hashCode`.
fn fold_bits(temp: i64) -> i32 {
    (temp ^ (((temp as u64) >> 32) as i64)) as i32
}

impl Vec3 {
    pub const ZERO: Vec3 = ZERO;
    pub const X_AXIS: Vec3 = X_AXIS;
    pub const Y_AXIS: Vec3 = Y_AXIS;
    pub const Z_AXIS: Vec3 = Z_AXIS;

    /// `Vec3(double, double, double)`.
    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// `Vec3(Vector3fc)` — the float components widen to double.
    pub fn from_vector3fc(vec: &Vec3f) -> Vec3 {
        Vec3::new(vec.x as f64, vec.y as f64, vec.z as f64)
    }

    /// `Vec3(Vec3i)`.
    pub fn from_vec3i(vec: &impl Vec3iLike) -> Vec3 {
        let (x, y, z) = vec.coords();
        Vec3::new(x as f64, y as f64, z as f64)
    }

    /// `Vec3.atLowerCornerOf(Vec3i)`.
    pub fn at_lower_corner_of(pos: &impl Vec3iLike) -> Vec3 {
        let (x, y, z) = pos.coords();
        Vec3::new(x as f64, y as f64, z as f64)
    }

    /// `Vec3.atLowerCornerWithOffset(Vec3i, double, double, double)`.
    pub fn at_lower_corner_with_offset(pos: &impl Vec3iLike, x: f64, y: f64, z: f64) -> Vec3 {
        let (px, py, pz) = pos.coords();
        Vec3::new(px as f64 + x, py as f64 + y, pz as f64 + z)
    }

    /// `Vec3.atCenterOf(Vec3i)`.
    pub fn at_center_of(pos: &impl Vec3iLike) -> Vec3 {
        Vec3::at_lower_corner_with_offset(pos, 0.5, 0.5, 0.5)
    }

    /// `Vec3.atBottomCenterOf(Vec3i)`.
    pub fn at_bottom_center_of(pos: &impl Vec3iLike) -> Vec3 {
        Vec3::at_lower_corner_with_offset(pos, 0.5, 0.0, 0.5)
    }

    /// `Vec3.upFromBottomCenterOf(Vec3i, double)`.
    pub fn up_from_bottom_center_of(pos: &impl Vec3iLike, y_offset: f64) -> Vec3 {
        Vec3::at_lower_corner_with_offset(pos, 0.5, y_offset, 0.5)
    }

    /// `Vec3.vectorTo(Vec3)`.
    pub fn vector_to(&self, vec: &Vec3) -> Vec3 {
        Vec3::new(vec.x - self.x, vec.y - self.y, vec.z - self.z)
    }

    /// `Vec3.normalize()` — `dist < 1.0E-5F` compares against the widened
    /// float constant.
    pub fn normalize(&self) -> Vec3 {
        let dist = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if dist < 1.0E-5f32 as f64 {
            ZERO
        } else {
            Vec3::new(self.x / dist, self.y / dist, self.z / dist)
        }
    }

    /// `Vec3.dot(Vec3)`.
    pub fn dot(&self, vec: &Vec3) -> f64 {
        self.x * vec.x + self.y * vec.y + self.z * vec.z
    }

    /// `Vec3.cross(Vec3)`.
    pub fn cross(&self, vec: &Vec3) -> Vec3 {
        Vec3::new(
            self.y * vec.z - self.z * vec.y,
            self.z * vec.x - self.x * vec.z,
            self.x * vec.y - self.y * vec.x,
        )
    }

    /// `Vec3.subtract(Vec3)`.
    pub fn subtract(&self, vec: &Vec3) -> Vec3 {
        self.subtract_xyz(vec.x, vec.y, vec.z)
    }

    /// `Vec3.subtract(double)`.
    pub fn subtract_value(&self, value: f64) -> Vec3 {
        self.subtract_xyz(value, value, value)
    }

    /// `Vec3.subtract(double, double, double)`.
    pub fn subtract_xyz(&self, x: f64, y: f64, z: f64) -> Vec3 {
        self.add_xyz(-x, -y, -z)
    }

    /// `Vec3.add(double)`.
    pub fn add_value(&self, value: f64) -> Vec3 {
        self.add_xyz(value, value, value)
    }

    /// `Vec3.add(Vec3)`.
    pub fn add(&self, vec: &Vec3) -> Vec3 {
        self.add_xyz(vec.x, vec.y, vec.z)
    }

    /// `Vec3.add(double, double, double)`.
    pub fn add_xyz(&self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x + x, self.y + y, self.z + z)
    }

    /// `Vec3.closerThan(Position, double)`.
    pub fn closer_than(&self, pos: &impl Position, distance: f64) -> bool {
        self.distance_to_sqr_xyz(pos.x(), pos.y(), pos.z()) < distance * distance
    }

    /// `Vec3.distanceTo(Vec3)`.
    pub fn distance_to(&self, vec: &Vec3) -> f64 {
        let xd = vec.x - self.x;
        let yd = vec.y - self.y;
        let zd = vec.z - self.z;
        (xd * xd + yd * yd + zd * zd).sqrt()
    }

    /// `Vec3.distanceToSqr(Vec3)`.
    pub fn distance_to_sqr(&self, vec: &Vec3) -> f64 {
        let xd = vec.x - self.x;
        let yd = vec.y - self.y;
        let zd = vec.z - self.z;
        xd * xd + yd * yd + zd * zd
    }

    /// `Vec3.distanceToSqr(double, double, double)`.
    pub fn distance_to_sqr_xyz(&self, x: f64, y: f64, z: f64) -> f64 {
        let xd = x - self.x;
        let yd = y - self.y;
        let zd = z - self.z;
        xd * xd + yd * yd + zd * zd
    }

    /// `Vec3.closerThan(Vec3, double, double)`.
    pub fn closer_than_xz_y(&self, vec: &Vec3, distance_xz: f64, distance_y: f64) -> bool {
        let dx = vec.x - self.x;
        let dy = vec.y - self.y;
        let dz = vec.z - self.z;
        mth::length_squared(dx, dz) < mth::square_f64(distance_xz) && dy.abs() < distance_y
    }

    /// `Vec3.scale(double)`.
    pub fn scale(&self, scale: f64) -> Vec3 {
        self.multiply_xyz(scale, scale, scale)
    }

    /// `Vec3.reverse()`.
    pub fn reverse(&self) -> Vec3 {
        self.scale(-1.0)
    }

    /// `Vec3.multiply(Vec3)`.
    pub fn multiply(&self, scale: &Vec3) -> Vec3 {
        self.multiply_xyz(scale.x, scale.y, scale.z)
    }

    /// `Vec3.multiply(double, double, double)`.
    pub fn multiply_xyz(&self, x_scale: f64, y_scale: f64, z_scale: f64) -> Vec3 {
        Vec3::new(self.x * x_scale, self.y * y_scale, self.z * z_scale)
    }

    /// `Vec3.horizontal()`.
    pub fn horizontal(&self) -> Vec3 {
        Vec3::new(self.x, 0.0, self.z)
    }

    /// `Vec3.offsetRandom(RandomSource, float)` — three `nextFloat` draws in
    /// x, y, z order; each `(nextFloat - 0.5F) * offset` is float arithmetic.
    pub fn offset_random<R: RandomSource>(&self, random: &mut R, offset: f32) -> Vec3 {
        let x = ((random.next_float() - 0.5f32) * offset) as f64;
        let y = ((random.next_float() - 0.5f32) * offset) as f64;
        let z = ((random.next_float() - 0.5f32) * offset) as f64;
        self.add_xyz(x, y, z)
    }

    /// `Vec3.offsetRandomXZ(RandomSource, float)`.
    pub fn offset_random_xz<R: RandomSource>(&self, random: &mut R, offset: f32) -> Vec3 {
        let x = ((random.next_float() - 0.5f32) * offset) as f64;
        let z = ((random.next_float() - 0.5f32) * offset) as f64;
        self.add_xyz(x, 0.0, z)
    }

    /// `Vec3.length()`.
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// `Vec3.lengthSqr()`.
    pub fn length_sqr(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// `Vec3.horizontalDistance()`.
    pub fn horizontal_distance(&self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }

    /// `Vec3.horizontalDistanceSqr()`.
    pub fn horizontal_distance_sqr(&self) -> f64 {
        self.x * self.x + self.z * self.z
    }

    /// `Vec3.hashCode()`.
    pub fn hash_code(&self) -> i32 {
        let mut result = fold_bits(double_to_long_bits(self.x));
        result = result
            .wrapping_mul(31)
            .wrapping_add(fold_bits(double_to_long_bits(self.y)));
        result
            .wrapping_mul(31)
            .wrapping_add(fold_bits(double_to_long_bits(self.z)))
    }

    /// `Vec3.lerp(Vec3, double)`.
    pub fn lerp(&self, vec: &Vec3, a: f64) -> Vec3 {
        Vec3::new(
            mth::lerp(a, self.x, vec.x),
            mth::lerp(a, self.y, vec.y),
            mth::lerp(a, self.z, vec.z),
        )
    }

    /// `Vec3.xRot(float)`.
    pub fn x_rot(&self, radians: f32) -> Vec3 {
        let cos = mth::cos(radians as f64);
        let sin = mth::sin(radians as f64);
        let xx = self.x;
        let yy = self.y * cos as f64 + self.z * sin as f64;
        let zz = self.z * cos as f64 - self.y * sin as f64;
        Vec3::new(xx, yy, zz)
    }

    /// `Vec3.yRot(float)`.
    pub fn y_rot(&self, radians: f32) -> Vec3 {
        let cos = mth::cos(radians as f64);
        let sin = mth::sin(radians as f64);
        let xx = self.x * cos as f64 + self.z * sin as f64;
        let yy = self.y;
        let zz = self.z * cos as f64 - self.x * sin as f64;
        Vec3::new(xx, yy, zz)
    }

    /// `Vec3.zRot(float)`.
    pub fn z_rot(&self, radians: f32) -> Vec3 {
        let cos = mth::cos(radians as f64);
        let sin = mth::sin(radians as f64);
        let xx = self.x * cos as f64 + self.y * sin as f64;
        let yy = self.y * cos as f64 - self.x * sin as f64;
        let zz = self.z;
        Vec3::new(xx, yy, zz)
    }

    /// `Vec3.rotateClockwise90()`.
    pub fn rotate_clockwise90(&self) -> Vec3 {
        Vec3::new(-self.z, self.y, self.x)
    }

    /// `Vec3.directionFromRotation(Vec2)`.
    pub fn direction_from_rotation(rotation: &Vec2) -> Vec3 {
        Vec3::direction_from_rotation_xy(rotation.x, rotation.y)
    }

    /// `Vec3.directionFromRotation(float, float)`.
    pub fn direction_from_rotation_xy(rot_x: f32, rot_y: f32) -> Vec3 {
        let y_cos = mth::cos((-rot_y * mth::DEG_TO_RAD - mth::PI) as f64);
        let y_sin = mth::sin((-rot_y * mth::DEG_TO_RAD - mth::PI) as f64);
        let x_cos = -mth::cos((-rot_x * mth::DEG_TO_RAD) as f64);
        let x_sin = mth::sin((-rot_x * mth::DEG_TO_RAD) as f64);
        Vec3::new((y_sin * x_cos) as f64, x_sin as f64, (y_cos * x_cos) as f64)
    }

    /// `Vec3.rotation()` — `(float)Math.atan2(..) * RAD_TO_DEG`.
    pub fn rotation(&self) -> Vec2 {
        let yaw = rivet_util::java_math::atan2(-self.x, self.z) as f32 * mth::RAD_TO_DEG;
        let pitch = rivet_util::java_math::asin(
            -self.y / (self.x * self.x + self.y * self.y + self.z * self.z).sqrt(),
        ) as f32
            * mth::RAD_TO_DEG;
        Vec2::new(pitch, yaw)
    }

    /// `Vec3.align(EnumSet<Direction.Axis>)` — the set is passed as its
    /// members (membership is all `align` reads).
    pub fn align(&self, axes: &[Axis]) -> Vec3 {
        let x = if axes.contains(&Axis::X) {
            mth::floor_d(self.x) as f64
        } else {
            self.x
        };
        let y = if axes.contains(&Axis::Y) {
            mth::floor_d(self.y) as f64
        } else {
            self.y
        };
        let z = if axes.contains(&Axis::Z) {
            mth::floor_d(self.z) as f64
        } else {
            self.z
        };
        Vec3::new(x, y, z)
    }

    /// `Vec3.get(Direction.Axis)`.
    pub fn get(&self, axis: Axis) -> f64 {
        axis.choose_f64(self.x, self.y, self.z)
    }

    /// `Vec3.with(Direction.Axis, double)`.
    pub fn with(&self, axis: Axis, value: f64) -> Vec3 {
        let x = if axis == Axis::X { value } else { self.x };
        let y = if axis == Axis::Y { value } else { self.y };
        let z = if axis == Axis::Z { value } else { self.z };
        Vec3::new(x, y, z)
    }

    /// `Vec3.relative(Direction, double)`.
    pub fn relative(&self, direction: Direction, distance: f64) -> Vec3 {
        let normal: Vec3i = direction.get_unit_vec3i();
        Vec3::new(
            self.x + distance * normal.get_x() as f64,
            self.y + distance * normal.get_y() as f64,
            self.z + distance * normal.get_z() as f64,
        )
    }

    /// `Vec3.toVector3f()`.
    pub fn to_vector3f(&self) -> Vec3f {
        Vec3f {
            x: self.x as f32,
            y: self.y as f32,
            z: self.z as f32,
        }
    }

    /// `Vec3.projectedOn(Vec3)`.
    pub fn projected_on(&self, onto: &Vec3) -> Vec3 {
        if onto.length_sqr() == 0.0 {
            *onto
        } else {
            onto.scale(self.dot(onto)).scale(1.0 / onto.length_sqr())
        }
    }

    /// `Vec3.applyLocalCoordinatesToRotation(Vec2, Vec3)`.
    pub fn apply_local_coordinates_to_rotation(rotation: &Vec2, direction: &Vec3) -> Vec3 {
        let y_cos = mth::cos(((rotation.y + 90.0f32) * mth::DEG_TO_RAD) as f64);
        let y_sin = mth::sin(((rotation.y + 90.0f32) * mth::DEG_TO_RAD) as f64);
        let x_cos = mth::cos((-rotation.x * mth::DEG_TO_RAD) as f64);
        let x_sin = mth::sin((-rotation.x * mth::DEG_TO_RAD) as f64);
        let x_cos_up = mth::cos(((-rotation.x + 90.0f32) * mth::DEG_TO_RAD) as f64);
        let x_sin_up = mth::sin(((-rotation.x + 90.0f32) * mth::DEG_TO_RAD) as f64);
        let forwards = Vec3::new((y_cos * x_cos) as f64, x_sin as f64, (y_sin * x_cos) as f64);
        let up = Vec3::new(
            (y_cos * x_cos_up) as f64,
            x_sin_up as f64,
            (y_sin * x_cos_up) as f64,
        );
        let left = forwards.cross(&up).scale(-1.0);
        let xa = forwards.x * direction.z + up.x * direction.y + left.x * direction.x;
        let ya = forwards.y * direction.z + up.y * direction.y + left.y * direction.x;
        let za = forwards.z * direction.z + up.z * direction.y + left.z * direction.x;
        Vec3::new(xa, ya, za)
    }

    /// `Vec3.addLocalCoordinates(Vec3)`.
    pub fn add_local_coordinates(&self, direction: &Vec3) -> Vec3 {
        Vec3::apply_local_coordinates_to_rotation(&self.rotation(), direction)
    }

    /// `Vec3.isFinite()`.
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Position for Vec3 {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn z(&self) -> f64 {
        self.z
    }
}

impl PartialEq for Vec3 {
    fn eq(&self, other: &Vec3) -> bool {
        double_to_long_bits(self.x) == double_to_long_bits(other.x)
            && double_to_long_bits(self.y) == double_to_long_bits(other.y)
            && double_to_long_bits(self.z) == double_to_long_bits(other.z)
    }
}

impl Eq for Vec3 {}

impl Hash for Vec3 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl fmt::Display for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {})",
            java_double_to_string(self.x),
            java_double_to_string(self.y),
            java_double_to_string(self.z)
        )
    }
}

/// `Vec3.CODEC` — `Codec.DOUBLE.listOf().comapFlatMap(Util.fixedSize(input, 3)
/// -> new Vec3(..), pos -> List.of(x, y, z))`.
pub fn vec3_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec3, Ops>> {
    codec::comap_flat_map::<Vec<f64>, Vec3, Ops>(
        codec::list(codec::double_codec::<Ops>()),
        Arc::new(|doubles: &Vec<f64>| fixed_size(doubles, 3).map(|d| Vec3::new(d[0], d[1], d[2]))),
        Arc::new(|pos: &Vec3| vec![pos.x, pos.y, pos.z]),
    )
}
