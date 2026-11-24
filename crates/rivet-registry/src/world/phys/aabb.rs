//! `net.minecraft.world.phys.AABB` — the double-precision axis-aligned box.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/AABB.java`
//! (Paper's `getDirection` is `public`).
//!
//! `AABB.of(BoundingBox)` takes a `rivet-world` type, so it lives at the same
//! mirrored path there (`rivet_world::phys::aabb::of`, D15's split rule);
//! `AABB.clip(Iterable<AABB>, ..)` takes a slice. `getDirection`'s
//! `double[] scaleReference` out-parameter is a `&mut f64`.
//!
//! Java `Math.min`/`Math.max` (NaN and signed-zero rules) are
//! `rivet_util::mth::{min_f64, max_f64}`.

use std::fmt;
use std::hash::{Hash, Hasher};

use rivet_serialization::float_format::java_double_to_string;
use rivet_util::mth;
use rivet_util::mth_stubs::Vec3f;

use super::block_hit_result::BlockHitResult;
use super::vec3::Vec3;
use crate::core::{Axis, BlockPos, Direction};

/// `AABB` — `min* <= max*` per axis after the constructor.
///
/// Java `equals` is `Double.compare(..) == 0` per component.
#[derive(Clone, Copy, Debug)]
pub struct AABB {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// `Double.doubleToLongBits` — every NaN maps to the canonical `0x7ff8...`.
fn double_to_long_bits(value: f64) -> i64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits() as i64
    }
}

/// `(int)(temp ^ temp >>> 32)` of `AABB.hashCode`.
fn fold_bits(temp: i64) -> i32 {
    (temp ^ (((temp as u64) >> 32) as i64)) as i32
}

impl AABB {
    /// `AABB(double, double, double, double, double, double)` — normalizes the
    /// corners with `Math.min`/`Math.max`.
    pub fn new(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> AABB {
        AABB {
            min_x: mth::min_f64(min_x, max_x),
            min_y: mth::min_f64(min_y, max_y),
            min_z: mth::min_f64(min_z, max_z),
            max_x: mth::max_f64(min_x, max_x),
            max_y: mth::max_f64(min_y, max_y),
            max_z: mth::max_f64(min_z, max_z),
        }
    }

    /// `AABB(BlockPos)`.
    pub fn from_block_pos(pos: &BlockPos) -> AABB {
        AABB::new(
            pos.get_x() as f64,
            pos.get_y() as f64,
            pos.get_z() as f64,
            pos.get_x().wrapping_add(1) as f64,
            pos.get_y().wrapping_add(1) as f64,
            pos.get_z().wrapping_add(1) as f64,
        )
    }

    /// `AABB(Vec3, Vec3)`.
    pub fn from_vec3s(begin: &Vec3, end: &Vec3) -> AABB {
        AABB::new(begin.x, begin.y, begin.z, end.x, end.y, end.z)
    }

    /// `AABB.unitCubeFromLowerCorner(Vec3)`.
    pub fn unit_cube_from_lower_corner(pos: &Vec3) -> AABB {
        AABB::new(pos.x, pos.y, pos.z, pos.x + 1.0, pos.y + 1.0, pos.z + 1.0)
    }

    /// `AABB.encapsulatingFullBlocks(BlockPos, BlockPos)`.
    pub fn encapsulating_full_blocks(pos0: &BlockPos, pos1: &BlockPos) -> AABB {
        AABB::new(
            pos0.get_x().min(pos1.get_x()) as f64,
            pos0.get_y().min(pos1.get_y()) as f64,
            pos0.get_z().min(pos1.get_z()) as f64,
            pos0.get_x().max(pos1.get_x()).wrapping_add(1) as f64,
            pos0.get_y().max(pos1.get_y()).wrapping_add(1) as f64,
            pos0.get_z().max(pos1.get_z()).wrapping_add(1) as f64,
        )
    }

    /// `AABB.setMinX(double)`.
    pub fn set_min_x(&self, min_x: f64) -> AABB {
        AABB::new(
            min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMinY(double)`.
    pub fn set_min_y(&self, min_y: f64) -> AABB {
        AABB::new(
            self.min_x, min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMinZ(double)`.
    pub fn set_min_z(&self, min_z: f64) -> AABB {
        AABB::new(
            self.min_x, self.min_y, min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMaxX(double)`.
    pub fn set_max_x(&self, max_x: f64) -> AABB {
        AABB::new(
            self.min_x, self.min_y, self.min_z, max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMaxY(double)`.
    pub fn set_max_y(&self, max_y: f64) -> AABB {
        AABB::new(
            self.min_x, self.min_y, self.min_z, self.max_x, max_y, self.max_z,
        )
    }

    /// `AABB.setMaxZ(double)`.
    pub fn set_max_z(&self, max_z: f64) -> AABB {
        AABB::new(
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, max_z,
        )
    }

    /// `AABB.min(Direction.Axis)`.
    pub fn min(&self, axis: Axis) -> f64 {
        axis.choose_f64(self.min_x, self.min_y, self.min_z)
    }

    /// `AABB.max(Direction.Axis)`.
    pub fn max(&self, axis: Axis) -> f64 {
        axis.choose_f64(self.max_x, self.max_y, self.max_z)
    }

    /// `AABB.hashCode()`.
    pub fn hash_code(&self) -> i32 {
        let mut result = fold_bits(double_to_long_bits(self.min_x));
        for value in [self.min_y, self.min_z, self.max_x, self.max_y, self.max_z] {
            result = result
                .wrapping_mul(31)
                .wrapping_add(fold_bits(double_to_long_bits(value)));
        }
        result
    }

    /// `AABB.contract(double, double, double)`.
    pub fn contract(&self, xa: f64, ya: f64, za: f64) -> AABB {
        let mut min_x = self.min_x;
        let mut min_y = self.min_y;
        let mut min_z = self.min_z;
        let mut max_x = self.max_x;
        let mut max_y = self.max_y;
        let mut max_z = self.max_z;
        if xa < 0.0 {
            min_x -= xa;
        } else if xa > 0.0 {
            max_x -= xa;
        }

        if ya < 0.0 {
            min_y -= ya;
        } else if ya > 0.0 {
            max_y -= ya;
        }

        if za < 0.0 {
            min_z -= za;
        } else if za > 0.0 {
            max_z -= za;
        }

        AABB::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.expandTowards(Vec3)`.
    pub fn expand_towards(&self, delta: &Vec3) -> AABB {
        self.expand_towards_xyz(delta.x, delta.y, delta.z)
    }

    /// `AABB.expandTowards(double, double, double)`.
    pub fn expand_towards_xyz(&self, xa: f64, ya: f64, za: f64) -> AABB {
        let mut min_x = self.min_x;
        let mut min_y = self.min_y;
        let mut min_z = self.min_z;
        let mut max_x = self.max_x;
        let mut max_y = self.max_y;
        let mut max_z = self.max_z;
        if xa < 0.0 {
            min_x += xa;
        } else if xa > 0.0 {
            max_x += xa;
        }

        if ya < 0.0 {
            min_y += ya;
        } else if ya > 0.0 {
            max_y += ya;
        }

        if za < 0.0 {
            min_z += za;
        } else if za > 0.0 {
            max_z += za;
        }

        AABB::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.inflate(double, double, double)`.
    pub fn inflate_xyz(&self, x_add: f64, y_add: f64, z_add: f64) -> AABB {
        let min_x = self.min_x - x_add;
        let min_y = self.min_y - y_add;
        let min_z = self.min_z - z_add;
        let max_x = self.max_x + x_add;
        let max_y = self.max_y + y_add;
        let max_z = self.max_z + z_add;
        AABB::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.inflate(double)`.
    pub fn inflate(&self, amount_to_add_in_all_directions: f64) -> AABB {
        self.inflate_xyz(
            amount_to_add_in_all_directions,
            amount_to_add_in_all_directions,
            amount_to_add_in_all_directions,
        )
    }

    /// `AABB.intersect(AABB)`.
    pub fn intersect(&self, other: &AABB) -> AABB {
        let min_x = mth::max_f64(self.min_x, other.min_x);
        let min_y = mth::max_f64(self.min_y, other.min_y);
        let min_z = mth::max_f64(self.min_z, other.min_z);
        let max_x = mth::min_f64(self.max_x, other.max_x);
        let max_y = mth::min_f64(self.max_y, other.max_y);
        let max_z = mth::min_f64(self.max_z, other.max_z);
        AABB::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.minmax(AABB)`.
    pub fn minmax(&self, other: &AABB) -> AABB {
        let min_x = mth::min_f64(self.min_x, other.min_x);
        let min_y = mth::min_f64(self.min_y, other.min_y);
        let min_z = mth::min_f64(self.min_z, other.min_z);
        let max_x = mth::max_f64(self.max_x, other.max_x);
        let max_y = mth::max_f64(self.max_y, other.max_y);
        let max_z = mth::max_f64(self.max_z, other.max_z);
        AABB::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.move(double, double, double)`.
    pub fn move_xyz(&self, xa: f64, ya: f64, za: f64) -> AABB {
        AABB::new(
            self.min_x + xa,
            self.min_y + ya,
            self.min_z + za,
            self.max_x + xa,
            self.max_y + ya,
            self.max_z + za,
        )
    }

    /// `AABB.move(BlockPos)`.
    pub fn move_block_pos(&self, pos: &BlockPos) -> AABB {
        AABB::new(
            self.min_x + pos.get_x() as f64,
            self.min_y + pos.get_y() as f64,
            self.min_z + pos.get_z() as f64,
            self.max_x + pos.get_x() as f64,
            self.max_y + pos.get_y() as f64,
            self.max_z + pos.get_z() as f64,
        )
    }

    /// `AABB.move(Vec3)`.
    pub fn move_vec3(&self, pos: &Vec3) -> AABB {
        self.move_xyz(pos.x, pos.y, pos.z)
    }

    /// `AABB.move(Vector3f)` — the float components widen to double.
    pub fn move_vector3f(&self, pos: &Vec3f) -> AABB {
        self.move_xyz(pos.x as f64, pos.y as f64, pos.z as f64)
    }

    /// `AABB.intersects(AABB)`.
    pub fn intersects(&self, aabb: &AABB) -> bool {
        self.intersects_xyz(
            aabb.min_x, aabb.min_y, aabb.min_z, aabb.max_x, aabb.max_y, aabb.max_z,
        )
    }

    /// `AABB.intersects(double, double, double, double, double, double)`.
    pub fn intersects_xyz(
        &self,
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> bool {
        self.min_x < max_x
            && self.max_x > min_x
            && self.min_y < max_y
            && self.max_y > min_y
            && self.min_z < max_z
            && self.max_z > min_z
    }

    /// `AABB.intersects(Vec3, Vec3)`.
    pub fn intersects_vec3s(&self, min: &Vec3, max: &Vec3) -> bool {
        self.intersects_xyz(
            mth::min_f64(min.x, max.x),
            mth::min_f64(min.y, max.y),
            mth::min_f64(min.z, max.z),
            mth::max_f64(min.x, max.x),
            mth::max_f64(min.y, max.y),
            mth::max_f64(min.z, max.z),
        )
    }

    /// `AABB.intersects(BlockPos)`.
    pub fn intersects_block_pos(&self, pos: &BlockPos) -> bool {
        self.intersects_xyz(
            pos.get_x() as f64,
            pos.get_y() as f64,
            pos.get_z() as f64,
            pos.get_x().wrapping_add(1) as f64,
            pos.get_y().wrapping_add(1) as f64,
            pos.get_z().wrapping_add(1) as f64,
        )
    }

    /// `AABB.contains(Vec3)`.
    pub fn contains(&self, vec: &Vec3) -> bool {
        self.contains_xyz(vec.x, vec.y, vec.z)
    }

    /// `AABB.contains(double, double, double)`.
    pub fn contains_xyz(&self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }

    /// `AABB.getSize()`.
    pub fn get_size(&self) -> f64 {
        let xs = self.get_xsize();
        let ys = self.get_ysize();
        let zs = self.get_zsize();
        (xs + ys + zs) / 3.0
    }

    /// `AABB.getXsize()`.
    pub fn get_xsize(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// `AABB.getYsize()`.
    pub fn get_ysize(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// `AABB.getZsize()`.
    pub fn get_zsize(&self) -> f64 {
        self.max_z - self.min_z
    }

    /// `AABB.deflate(double, double, double)`.
    pub fn deflate_xyz(&self, x_substract: f64, y_subtract: f64, z_subtract: f64) -> AABB {
        self.inflate_xyz(-x_substract, -y_subtract, -z_subtract)
    }

    /// `AABB.deflate(double)`.
    pub fn deflate(&self, amount: f64) -> AABB {
        self.inflate(-amount)
    }

    /// `AABB.clip(Vec3, Vec3)`.
    pub fn clip(&self, from: &Vec3, to: &Vec3) -> Option<Vec3> {
        AABB::clip_bounds(
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z, from, to,
        )
    }

    /// `AABB.clip(double, double, double, double, double, double, Vec3, Vec3)`
    /// (the static overload).
    #[allow(clippy::too_many_arguments)]
    pub fn clip_bounds(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
        from: &Vec3,
        to: &Vec3,
    ) -> Option<Vec3> {
        let mut scale_reference = 1.0;
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;
        let direction = AABB::get_direction_bounds(
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
            from,
            &mut scale_reference,
            None,
            dx,
            dy,
            dz,
        );
        direction?;

        let scale = scale_reference;
        Some(from.add_xyz(scale * dx, scale * dy, scale * dz))
    }

    /// `AABB.clip(Iterable<AABB>, Vec3, Vec3, BlockPos)`.
    pub fn clip_all(
        aabbs: &[AABB],
        from: &Vec3,
        to: &Vec3,
        pos: &BlockPos,
    ) -> Option<BlockHitResult> {
        let mut scale_reference = 1.0;
        let mut direction = None;
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;

        for aabb in aabbs {
            direction = AABB::get_direction(
                &aabb.move_block_pos(pos),
                from,
                &mut scale_reference,
                direction,
                dx,
                dy,
                dz,
            );
        }

        let direction = direction?;

        let scale = scale_reference;
        Some(BlockHitResult::new(
            from.add_xyz(scale * dx, scale * dy, scale * dz),
            direction,
            *pos,
            false,
        ))
    }

    /// `AABB.getDirection(AABB, Vec3, double[], Direction, double, double,
    /// double)` — public in Paper (optimise collisions).
    pub fn get_direction(
        aabb: &AABB,
        from: &Vec3,
        scale_reference: &mut f64,
        direction: Option<Direction>,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> Option<Direction> {
        AABB::get_direction_bounds(
            aabb.min_x,
            aabb.min_y,
            aabb.min_z,
            aabb.max_x,
            aabb.max_y,
            aabb.max_z,
            from,
            scale_reference,
            direction,
            dx,
            dy,
            dz,
        )
    }

    /// The private `getDirection(double x6, Vec3, double[], Direction, double
    /// x3)` overload.
    #[allow(clippy::too_many_arguments)]
    fn get_direction_bounds(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
        from: &Vec3,
        scale_reference: &mut f64,
        mut direction: Option<Direction>,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> Option<Direction> {
        if dx > 1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dx,
                dy,
                dz,
                min_x,
                min_y,
                max_y,
                min_z,
                max_z,
                Direction::West,
                from.x,
                from.y,
                from.z,
            );
        } else if dx < -1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dx,
                dy,
                dz,
                max_x,
                min_y,
                max_y,
                min_z,
                max_z,
                Direction::East,
                from.x,
                from.y,
                from.z,
            );
        }

        if dy > 1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dy,
                dz,
                dx,
                min_y,
                min_z,
                max_z,
                min_x,
                max_x,
                Direction::Down,
                from.y,
                from.z,
                from.x,
            );
        } else if dy < -1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dy,
                dz,
                dx,
                max_y,
                min_z,
                max_z,
                min_x,
                max_x,
                Direction::Up,
                from.y,
                from.z,
                from.x,
            );
        }

        if dz > 1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dz,
                dx,
                dy,
                min_z,
                min_x,
                max_x,
                min_y,
                max_y,
                Direction::North,
                from.z,
                from.x,
                from.y,
            );
        } else if dz < -1.0E-7 {
            direction = AABB::clip_point(
                scale_reference,
                direction,
                dz,
                dx,
                dy,
                max_z,
                min_x,
                max_x,
                min_y,
                max_y,
                Direction::South,
                from.z,
                from.x,
                from.y,
            );
        }

        direction
    }

    #[allow(clippy::too_many_arguments)]
    fn clip_point(
        scale_reference: &mut f64,
        direction: Option<Direction>,
        da: f64,
        db: f64,
        dc: f64,
        point: f64,
        min_b: f64,
        max_b: f64,
        min_c: f64,
        max_c: f64,
        new_direction: Direction,
        from_a: f64,
        from_b: f64,
        from_c: f64,
    ) -> Option<Direction> {
        let s = (point - from_a) / da;
        let pb = from_b + s * db;
        let pc = from_c + s * dc;
        if 0.0 < s
            && s < *scale_reference
            && min_b - 1.0E-7 < pb
            && pb < max_b + 1.0E-7
            && min_c - 1.0E-7 < pc
            && pc < max_c + 1.0E-7
        {
            *scale_reference = s;
            Some(new_direction)
        } else {
            direction
        }
    }

    /// `AABB.collidedAlongVector(Vec3, List<AABB>)`.
    pub fn collided_along_vector(&self, vector: &Vec3, aabbs: &[AABB]) -> bool {
        let from = self.get_center();
        let to = from.add(vector);

        for shape_part in aabbs {
            let inflated = shape_part.inflate_xyz(
                self.get_xsize() * 0.5 - 1.0E-7,
                self.get_ysize() * 0.5 - 1.0E-7,
                self.get_zsize() * 0.5 - 1.0E-7,
            );
            if inflated.contains(&to) || inflated.contains(&from) {
                return true;
            }

            if inflated.clip(&from, &to).is_some() {
                return true;
            }
        }

        false
    }

    /// `AABB.distanceToSqr(Vec3)`.
    pub fn distance_to_sqr(&self, point: &Vec3) -> f64 {
        let dx = mth::max_f64(
            mth::max_f64(self.min_x - point.x, point.x - self.max_x),
            0.0,
        );
        let dy = mth::max_f64(
            mth::max_f64(self.min_y - point.y, point.y - self.max_y),
            0.0,
        );
        let dz = mth::max_f64(
            mth::max_f64(self.min_z - point.z, point.z - self.max_z),
            0.0,
        );
        mth::length_squared_xyz(dx, dy, dz)
    }

    /// `AABB.distanceToSqr(AABB)`.
    pub fn distance_to_sqr_aabb(&self, bounding_box: &AABB) -> f64 {
        let dx = mth::max_f64(
            mth::max_f64(
                self.min_x - bounding_box.max_x,
                bounding_box.min_x - self.max_x,
            ),
            0.0,
        );
        let dy = mth::max_f64(
            mth::max_f64(
                self.min_y - bounding_box.max_y,
                bounding_box.min_y - self.max_y,
            ),
            0.0,
        );
        let dz = mth::max_f64(
            mth::max_f64(
                self.min_z - bounding_box.max_z,
                bounding_box.min_z - self.max_z,
            ),
            0.0,
        );
        mth::length_squared_xyz(dx, dy, dz)
    }

    /// `AABB.hasNaN()`.
    pub fn has_nan(&self) -> bool {
        self.min_x.is_nan()
            || self.min_y.is_nan()
            || self.min_z.is_nan()
            || self.max_x.is_nan()
            || self.max_y.is_nan()
            || self.max_z.is_nan()
    }

    /// `AABB.getCenter()`.
    pub fn get_center(&self) -> Vec3 {
        Vec3::new(
            mth::lerp(0.5, self.min_x, self.max_x),
            mth::lerp(0.5, self.min_y, self.max_y),
            mth::lerp(0.5, self.min_z, self.max_z),
        )
    }

    /// `AABB.getBottomCenter()`.
    pub fn get_bottom_center(&self) -> Vec3 {
        Vec3::new(
            mth::lerp(0.5, self.min_x, self.max_x),
            self.min_y,
            mth::lerp(0.5, self.min_z, self.max_z),
        )
    }

    /// `AABB.getMinPosition()`.
    pub fn get_min_position(&self) -> Vec3 {
        Vec3::new(self.min_x, self.min_y, self.min_z)
    }

    /// `AABB.getMaxPosition()`.
    pub fn get_max_position(&self) -> Vec3 {
        Vec3::new(self.max_x, self.max_y, self.max_z)
    }

    /// `AABB.ofSize(Vec3, double, double, double)`.
    pub fn of_size(center: &Vec3, size_x: f64, size_y: f64, size_z: f64) -> AABB {
        AABB::new(
            center.x - size_x / 2.0,
            center.y - size_y / 2.0,
            center.z - size_z / 2.0,
            center.x + size_x / 2.0,
            center.y + size_y / 2.0,
            center.z + size_z / 2.0,
        )
    }
}

impl PartialEq for AABB {
    fn eq(&self, other: &AABB) -> bool {
        double_to_long_bits(self.min_x) == double_to_long_bits(other.min_x)
            && double_to_long_bits(self.min_y) == double_to_long_bits(other.min_y)
            && double_to_long_bits(self.min_z) == double_to_long_bits(other.min_z)
            && double_to_long_bits(self.max_x) == double_to_long_bits(other.max_x)
            && double_to_long_bits(self.max_y) == double_to_long_bits(other.max_y)
            && double_to_long_bits(self.max_z) == double_to_long_bits(other.max_z)
    }
}

impl Eq for AABB {}

impl Hash for AABB {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl fmt::Display for AABB {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AABB[{}, {}, {}] -> [{}, {}, {}]",
            java_double_to_string(self.min_x),
            java_double_to_string(self.min_y),
            java_double_to_string(self.min_z),
            java_double_to_string(self.max_x),
            java_double_to_string(self.max_y),
            java_double_to_string(self.max_z)
        )
    }
}

/// `AABB.Builder` — accumulates float bounds (`Float.MIN/MAX` seeds) over
/// `Vector3fc` points.
#[derive(Clone, Copy, Debug)]
pub struct Builder {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
    defined: bool,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            min_z: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            max_z: f32::NEG_INFINITY,
            defined: false,
        }
    }
}

impl Builder {
    /// `AABB.Builder.include(Vector3fc)`.
    pub fn include(&mut self, v: &Vec3f) {
        self.min_x = mth::min_f32(self.min_x, v.x);
        self.min_y = mth::min_f32(self.min_y, v.y);
        self.min_z = mth::min_f32(self.min_z, v.z);
        self.max_x = mth::max_f32(self.max_x, v.x);
        self.max_y = mth::max_f32(self.max_y, v.y);
        self.max_z = mth::max_f32(self.max_z, v.z);
        self.defined = true;
    }

    /// `AABB.Builder.isDefined()`.
    pub fn is_defined(&self) -> bool {
        self.defined
    }

    /// `AABB.Builder.build()` — `IllegalStateException` when nothing was
    /// included.
    pub fn build(&self) -> Result<AABB, UndefinedAabb> {
        if !self.defined {
            Err(UndefinedAabb)
        } else {
            Ok(AABB::new(
                self.min_x as f64,
                self.min_y as f64,
                self.min_z as f64,
                self.max_x as f64,
                self.max_y as f64,
                self.max_z as f64,
            ))
        }
    }
}

/// `IllegalStateException("Cannot build an undefined AABB. Include at least
/// one point.")`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndefinedAabb;

impl fmt::Display for UndefinedAabb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cannot build an undefined AABB. Include at least one point.")
    }
}

impl std::error::Error for UndefinedAabb {}
