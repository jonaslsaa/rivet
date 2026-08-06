//! `net.minecraft.core.Vec3i` — an immutable integer vector.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/Vec3i.java`.
//! Preserves Java wrapping arithmetic, `compareTo`, `equals`/`hashCode`, and
//! Guava `MoreObjects.toStringHelper` formatting exactly.
//!
//! Paper's Perf notes: fields are `protected` so subclasses (`BlockPos`,
//! `SectionPos`) can inline; the getters are `final`. In Rust the fields are
//! `pub(crate)` so sibling modules in `rivet_registry::core` can inline, and
//! the getters are free functions returning the field directly.
//!
//! Deferred (leaf types owned by other units, PORTING.md): `CODEC`/
//! `offsetCodec`/`STREAM_CODEC` (codec surface → rivet-protocol/#holder),
//! `toMutable` (JOML `Vector3i`), `isInsideBuildHeightAndWorldBoundsHorizontal`
//! (needs `Level`/`LevelHeightAccessor`).

use super::block_pos::{BlockPos, MutableBlockPos};
use super::direction::{Axis, Direction};
use super::section_pos::SectionPos;

/// `Vec3i` — an immutable integer vector (`net.minecraft.core.Vec3i`).
///
/// Java `equals` compares across the whole hierarchy (`this == o || o instanceof
/// Vec3i && coords equal`), so `BlockPos`/`SectionPos`/`MutableBlockPos` are
/// mutually equal with `Vec3i` when their coordinates match. Rust's `PartialEq`
/// derive would *not* do that (different concrete types never compare equal),
/// so we implement `PartialEq` via a marker trait and compare the `(x, y, z)`
/// projection (see the cross-type impls at the bottom of this file).
#[derive(Clone, Copy, Debug)]
pub struct Vec3i {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

/// Marker for types whose value equality is the `(x, y, z)` projection (the
/// Java `Vec3i.equals` contract). `Vec3i`, `BlockPos`, `MutableBlockPos` and
/// `SectionPos` all implement it, so `BlockPos == SectionPos` compares
/// coordinates — matching Java's `o instanceof Vec3i` check.
pub trait Vec3iLike: Copy {
    /// The coordinate projection used by Java `equals`.
    fn coords(&self) -> (i32, i32, i32);
}

impl PartialEq for Vec3i {
    fn eq(&self, other: &Self) -> bool {
        self.coords() == other.coords()
    }
}

impl Eq for Vec3i {}

impl Vec3iLike for Vec3i {
    fn coords(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }
}

/// The `Vec3i.compareTo` decision: `y` first, then `z`, then `x`, each returning
/// Java's wrapping int subtraction (`this.y - pos.y` etc.).
pub(crate) fn compare_coords(ax: i32, ay: i32, az: i32, bx: i32, by: i32, bz: i32) -> i32 {
    if ay == by {
        if az == bz {
            ax.wrapping_sub(bx)
        } else {
            az.wrapping_sub(bz)
        }
    } else {
        ay.wrapping_sub(by)
    }
}

/// The lexicographic `(y, z, x)` ordering, **without** Java's wrapping
/// subtraction. This is a genuine total order consistent with `Eq`, and equals
/// the sign of Java `compareTo` for every input where the coordinate
/// subtractions do not overflow (`|a - b| <= i32::MAX`, i.e. all realistic
/// game coordinates). On overflow inputs Java's `compareTo` is not transitive
/// (the wrapping subtraction inverts sign), so no law-abiding `Ord` can equal
/// it there; `Vec3i::compare_to` exposes the exact wrapping int separately.
pub(crate) fn cmp_lexicographic_yzx(a: (i32, i32, i32), b: (i32, i32, i32)) -> std::cmp::Ordering {
    a.1.cmp(&b.1)
        .then_with(|| a.2.cmp(&b.2))
        .then_with(|| a.0.cmp(&b.0))
}

impl Ord for Vec3i {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        cmp_lexicographic_yzx((self.x, self.y, self.z), (other.x, other.y, other.z))
    }
}

impl PartialOrd for Vec3i {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for Vec3i {
    /// Java `hashCode` — a `BlockPos` and a `SectionPos` with equal coordinates
    /// hash identically, matching Java's `equals`/`hashCode` contract.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl Vec3i {
    /// `new Vec3i(x, y, z)`.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `Vec3i.ZERO`.
    pub const ZERO: Vec3i = Vec3i { x: 0, y: 0, z: 0 };

    /// `Vec3i.getX()`.
    pub fn get_x(&self) -> i32 {
        self.x
    }

    /// `Vec3i.getY()`.
    pub fn get_y(&self) -> i32 {
        self.y
    }

    /// `Vec3i.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.z
    }

    /// `Vec3i.hashCode()` — `(y + z * 31) * 31 + x` (wrapping).
    pub fn hash_code(&self) -> i32 {
        (self.y.wrapping_add(self.z.wrapping_mul(31)))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// `Vec3i.compareTo(Vec3i)` — the exact int result (`y`, then `z`, then `x`
    /// wrapping int subtraction). `Ord::cmp` is its sign.
    pub fn compare_to(&self, pos: &Vec3i) -> i32 {
        compare_coords(self.x, self.y, self.z, pos.x, pos.y, pos.z)
    }

    /// `Vec3i.offset(x, y, z)` — returns `this` when all steps are zero.
    pub fn offset(&self, x: i32, y: i32, z: i32) -> Vec3i {
        if x == 0 && y == 0 && z == 0 {
            *self
        } else {
            Vec3i::new(
                self.x.wrapping_add(x),
                self.y.wrapping_add(y),
                self.z.wrapping_add(z),
            )
        }
    }

    /// `Vec3i.offset(Vec3i)`.
    pub fn offset_vec(&self, vec: &Vec3i) -> Vec3i {
        self.offset(vec.x, vec.y, vec.z)
    }

    /// `Vec3i.subtract(Vec3i)` — Java `-vec.getX()` wraps for `MIN_VALUE`.
    pub fn subtract(&self, vec: &Vec3i) -> Vec3i {
        self.offset(
            vec.x.wrapping_neg(),
            vec.y.wrapping_neg(),
            vec.z.wrapping_neg(),
        )
    }

    /// `Vec3i.multiply(int)` — wrapping; returns `this` for scale 1, `ZERO`
    /// for scale 0.
    pub fn multiply(&self, scale: i32) -> Vec3i {
        if scale == 1 {
            *self
        } else if scale == 0 {
            Self::ZERO
        } else {
            Vec3i::new(
                self.x.wrapping_mul(scale),
                self.y.wrapping_mul(scale),
                self.z.wrapping_mul(scale),
            )
        }
    }

    /// `Vec3i.multiply(xScale, yScale, zScale)`.
    pub fn multiply_xyz(&self, x_scale: i32, y_scale: i32, z_scale: i32) -> Vec3i {
        Vec3i::new(
            self.x.wrapping_mul(x_scale),
            self.y.wrapping_mul(y_scale),
            self.z.wrapping_mul(z_scale),
        )
    }

    /// `Vec3i.above()`.
    pub fn above(&self) -> Vec3i {
        self.relative(&Direction::Up, 1)
    }

    /// `Vec3i.above(int)`.
    pub fn above_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::Up, steps)
    }

    /// `Vec3i.below()`.
    pub fn below(&self) -> Vec3i {
        self.relative(&Direction::Down, 1)
    }

    /// `Vec3i.below(int)`.
    pub fn below_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::Down, steps)
    }

    /// `Vec3i.north()`.
    pub fn north(&self) -> Vec3i {
        self.relative(&Direction::North, 1)
    }

    /// `Vec3i.north(int)`.
    pub fn north_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::North, steps)
    }

    /// `Vec3i.south()`.
    pub fn south(&self) -> Vec3i {
        self.relative(&Direction::South, 1)
    }

    /// `Vec3i.south(int)`.
    pub fn south_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::South, steps)
    }

    /// `Vec3i.west()`.
    pub fn west(&self) -> Vec3i {
        self.relative(&Direction::West, 1)
    }

    /// `Vec3i.west(int)`.
    pub fn west_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::West, steps)
    }

    /// `Vec3i.east()`.
    pub fn east(&self) -> Vec3i {
        self.relative(&Direction::East, 1)
    }

    /// `Vec3i.east(int)`.
    pub fn east_steps(&self, steps: i32) -> Vec3i {
        self.relative(&Direction::East, steps)
    }

    /// `Vec3i.relative(Direction, int)`.
    pub fn relative(&self, direction: &Direction, steps: i32) -> Vec3i {
        if steps == 0 {
            *self
        } else {
            Vec3i::new(
                self.x.wrapping_add(direction.step_x().wrapping_mul(steps)),
                self.y.wrapping_add(direction.step_y().wrapping_mul(steps)),
                self.z.wrapping_add(direction.step_z().wrapping_mul(steps)),
            )
        }
    }

    /// `Vec3i.relative(Direction.Axis, int)`.
    pub fn relative_axis(&self, axis: &Axis, steps: i32) -> Vec3i {
        if steps == 0 {
            return *self;
        }
        let (x_step, y_step, z_step) = match axis {
            Axis::X => (steps, 0, 0),
            Axis::Y => (0, steps, 0),
            Axis::Z => (0, 0, steps),
        };
        Vec3i::new(
            self.x.wrapping_add(x_step),
            self.y.wrapping_add(y_step),
            self.z.wrapping_add(z_step),
        )
    }

    /// `Vec3i.cross(Vec3i)` — wrapping cross product.
    pub fn cross(&self, up_vector: &Vec3i) -> Vec3i {
        Vec3i::new(
            self.y
                .wrapping_mul(up_vector.z)
                .wrapping_sub(self.z.wrapping_mul(up_vector.y)),
            self.z
                .wrapping_mul(up_vector.x)
                .wrapping_sub(self.x.wrapping_mul(up_vector.z)),
            self.x
                .wrapping_mul(up_vector.y)
                .wrapping_sub(self.y.wrapping_mul(up_vector.x)),
        )
    }

    /// `Vec3i.closerThan(Vec3i, double)` — `distSqr(pos) < square(distance)`.
    pub fn closer_than(&self, pos: &Vec3i, distance: f64) -> bool {
        self.dist_sqr(pos) < distance * distance
    }

    /// `Vec3i.closerToCenterThan(Position, double)`.
    pub fn closer_to_center_than(&self, pos: &dyn super::Position, distance: f64) -> bool {
        self.dist_to_center_sqr(pos.x(), pos.y(), pos.z()) < distance * distance
    }

    /// `Vec3i.distSqr(Vec3i)`.
    pub fn dist_sqr(&self, pos: &Vec3i) -> f64 {
        self.dist_to_low_corner_sqr(pos.x as f64, pos.y as f64, pos.z as f64)
    }

    /// `Vec3i.distToCenterSqr(Position)`.
    pub fn dist_to_center_sqr_pos(&self, pos: &dyn super::Position) -> f64 {
        self.dist_to_center_sqr(pos.x(), pos.y(), pos.z())
    }

    /// `Vec3i.distToCenterSqr(double, double, double)`.
    pub fn dist_to_center_sqr(&self, x: f64, y: f64, z: f64) -> f64 {
        let dx = self.x as f64 + 0.5 - x;
        let dy = self.y as f64 + 0.5 - y;
        let dz = self.z as f64 + 0.5 - z;
        dx * dx + dy * dy + dz * dz
    }

    /// `Vec3i.distToLowCornerSqr(double, double, double)`.
    pub fn dist_to_low_corner_sqr(&self, x: f64, y: f64, z: f64) -> f64 {
        let dx = self.x as f64 - x;
        let dy = self.y as f64 - y;
        let dz = self.z as f64 - z;
        dx * dx + dy * dy + dz * dz
    }

    /// `Vec3i.distManhattan(Vec3i)` — float absolute values summed then cast
    /// to int (Java: `(int)(xd + yd + zd)`; the float sum truncates).
    pub fn dist_manhattan(&self, pos: &Vec3i) -> i32 {
        let xd = (pos.x.wrapping_sub(self.x)).wrapping_abs() as f32;
        let yd = (pos.y.wrapping_sub(self.y)).wrapping_abs() as f32;
        let zd = (pos.z.wrapping_sub(self.z)).wrapping_abs() as f32;
        (xd + yd + zd) as i32
    }

    /// `Vec3i.distChessboard(Vec3i)`.
    pub fn dist_chessboard(&self, pos: &Vec3i) -> i32 {
        let xd = self.x.wrapping_sub(pos.x).wrapping_abs();
        let yd = self.y.wrapping_sub(pos.y).wrapping_abs();
        let zd = self.z.wrapping_sub(pos.z).wrapping_abs();
        xd.max(yd).max(zd)
    }

    /// `Vec3i.get(Direction.Axis)` — `axis.choose(x, y, z)`.
    pub fn get(&self, axis: &Axis) -> i32 {
        axis.choose(self.x, self.y, self.z)
    }

    /// `Vec3i.toShortString()` — `"x, y, z"`.
    pub fn to_short_string(&self) -> String {
        format!("{}, {}, {}", self.x, self.y, self.z)
    }
}

impl std::fmt::Display for Vec3i {
    /// `Vec3i.toString()` — Guava `MoreObjects.toStringHelper(this)`, which
    /// prints the concrete class name then `{x=…, y=…, z=…}`. Subclasses
    /// (`BlockPos`, `SectionPos`) override `Display` with their own name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_helper("Vec3i", self.x, self.y, self.z))
    }
}

pub(crate) fn format_helper(name: &str, x: i32, y: i32, z: i32) -> String {
    format!("{name}{{x={x}, y={y}, z={z}}}")
}

/// Cross-type `PartialEq` for the `Vec3i` hierarchy (Java `o instanceof Vec3i`):
/// every combination of `Vec3i`/`BlockPos`/`MutableBlockPos`/`SectionPos`
/// compares equal exactly when the `(x, y, z)` projections match.
macro_rules! impl_cross_eq {
    ($a:ty, $b:ty) => {
        impl PartialEq<$b> for $a {
            fn eq(&self, other: &$b) -> bool {
                <$a as Vec3iLike>::coords(self) == <$b as Vec3iLike>::coords(other)
            }
        }
        impl PartialEq<$a> for $b {
            fn eq(&self, other: &$a) -> bool {
                <$b as Vec3iLike>::coords(self) == <$a as Vec3iLike>::coords(other)
            }
        }
    };
}
impl_cross_eq!(Vec3i, BlockPos);
impl_cross_eq!(Vec3i, MutableBlockPos);
impl_cross_eq!(Vec3i, SectionPos);
impl_cross_eq!(BlockPos, MutableBlockPos);
impl_cross_eq!(BlockPos, SectionPos);
impl_cross_eq!(MutableBlockPos, SectionPos);
