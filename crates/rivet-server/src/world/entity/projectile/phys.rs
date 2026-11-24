//! No Java counterpart of its own: a stand-in for `net.minecraft.world.phys` and `net.minecraft.core` classes, kept out of the mirrored `world::phys` path until their canonical ports land.
//!
//! Minimal Paper-exact physics & hit-result seam for `net.minecraft.world.entity.projectile`.
//!
//! Mirrors `net.minecraft.world.phys.{Vec3, AABB, HitResult, BlockHitResult, EntityHitResult}`
//! and `net.minecraft.core.{BlockPos, Direction}` until `mc.world.phys` is ported as its own unit.

use rivet_util::mth;

/// Unique runtime entity identifier (single-owner arena key per `OWNERSHIP.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u64);

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

/// `net.minecraft.core.BlockPos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `BlockPos.containing(double x, double y, double z)`.
    pub fn containing(v: Vec3) -> Self {
        Self {
            x: mth::floor_d(v.x),
            y: mth::floor_d(v.y),
            z: mth::floor_d(v.z),
        }
    }
}

/// `net.minecraft.core.Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

/// `net.minecraft.world.level.ClipContext.Block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipBlock {
    Collider,
    Outline,
    Visual,
    FallDamageResetting,
}

/// `net.minecraft.world.phys.Vec3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn add_xyz(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(self.x + x, self.y + y, self.z + z)
    }

    pub fn subtract(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub fn multiply(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(self.x * x, self.y * y, self.z * z)
    }

    pub fn length_sqr(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f64 {
        self.length_sqr().sqrt()
    }

    pub fn horizontal_distance(self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }

    pub fn distance_to_sqr(self, other: Self) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        dx * dx + dy * dy + dz * dz
    }

    pub fn distance_to(self, other: Self) -> f64 {
        self.distance_to_sqr(other).sqrt()
    }

    /// Paper `Vec3.normalize()`: `dist < 1.0E-5F ? ZERO : new Vec3(x / dist, y / dist, z / dist)`.
    pub fn normalize(self) -> Self {
        let dist = self.length();
        if dist < (1.0E-5_f32 as f64) {
            Self::ZERO
        } else {
            Self::new(self.x / dist, self.y / dist, self.z / dist)
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
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

/// `net.minecraft.world.phys.AABB`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        Self {
            min_x: rivet_util::mth::min_f64(x1, x2),
            min_y: rivet_util::mth::min_f64(y1, y2),
            min_z: rivet_util::mth::min_f64(z1, z2),
            max_x: rivet_util::mth::max_f64(x1, x2),
            max_y: rivet_util::mth::max_f64(y1, y2),
            max_z: rivet_util::mth::max_f64(z1, z2),
        }
    }

    /// Creates a unit-centered AABB for an entity at `pos` with `width` and `height`
    /// (`EntityDimensions.makeBoundingBox`).
    pub fn from_entity_pos(pos: Vec3, width: f64, height: f64) -> Self {
        let half = width / 2.0;
        Self {
            min_x: pos.x - half,
            min_y: pos.y,
            min_z: pos.z - half,
            max_x: pos.x + half,
            max_y: pos.y + height,
            max_z: pos.z + half,
        }
    }

    pub fn inflate(self, amount: f64) -> Self {
        self.inflate_xyz(amount, amount, amount)
    }

    pub fn inflate_xyz(self, x: f64, y: f64, z: f64) -> Self {
        Self {
            min_x: self.min_x - x,
            min_y: self.min_y - y,
            min_z: self.min_z - z,
            max_x: self.max_x + x,
            max_y: self.max_y + y,
            max_z: self.max_z + z,
        }
    }

    pub fn expand_towards(self, delta: Vec3) -> Self {
        let mut min_x = self.min_x;
        let mut min_y = self.min_y;
        let mut min_z = self.min_z;
        let mut max_x = self.max_x;
        let mut max_y = self.max_y;
        let mut max_z = self.max_z;
        if delta.x < 0.0 {
            min_x += delta.x;
        } else if delta.x > 0.0 {
            max_x += delta.x;
        }
        if delta.y < 0.0 {
            min_y += delta.y;
        } else if delta.y > 0.0 {
            max_y += delta.y;
        }
        if delta.z < 0.0 {
            min_z += delta.z;
        } else if delta.z > 0.0 {
            max_z += delta.z;
        }
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    pub fn move_offset(self, delta: Vec3) -> Self {
        Self {
            min_x: self.min_x + delta.x,
            min_y: self.min_y + delta.y,
            min_z: self.min_z + delta.z,
            max_x: self.max_x + delta.x,
            max_y: self.max_y + delta.y,
            max_z: self.max_z + delta.z,
        }
    }

    /// Paper `AABB.intersects(AABB)`: strict inequalities on all three axes.
    pub fn intersects(self, other: Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }

    /// Paper `AABB.contains(Vec3)`: `min <= v < max`.
    pub fn contains(self, point: Vec3) -> bool {
        point.x >= self.min_x
            && point.x < self.max_x
            && point.y >= self.min_y
            && point.y < self.max_y
            && point.z >= self.min_z
            && point.z < self.max_z
    }

    /// Exact port of Paper `AABB.clip(Vec3 from, Vec3 to)` with `1.0E-7` tolerance
    /// and X -> Y -> Z face testing order.
    pub fn clip(self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let mut scale = 1.0_f64;
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;
        let dir = get_direction(
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z, from,
            &mut scale, None, dx, dy, dz,
        );
        dir?;
        Some(from.add_xyz(scale * dx, scale * dy, scale * dz))
    }
}

#[allow(clippy::too_many_arguments)]
fn get_direction(
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
    from: Vec3,
    scale: &mut f64,
    mut direction: Option<Direction>,
    dx: f64,
    dy: f64,
    dz: f64,
) -> Option<Direction> {
    if dx > 1.0E-7 {
        direction = clip_point(
            scale,
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
        direction = clip_point(
            scale,
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
        direction = clip_point(
            scale,
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
        direction = clip_point(
            scale,
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
        direction = clip_point(
            scale,
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
        direction = clip_point(
            scale,
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
    scale: &mut f64,
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
        && s < *scale
        && min_b - 1.0E-7 < pb
        && pb < max_b + 1.0E-7
        && min_c - 1.0E-7 < pc
        && pc < max_c + 1.0E-7
    {
        *scale = s;
        Some(new_direction)
    } else {
        direction
    }
}

/// `net.minecraft.world.phys.BlockHitResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHitResult {
    pub location: Vec3,
    pub direction: Direction,
    pub block_pos: BlockPos,
    pub miss: bool,
    pub inside: bool,
    pub world_border_hit: bool,
}

impl BlockHitResult {
    pub fn miss(location: Vec3, direction: Direction, block_pos: BlockPos) -> Self {
        Self {
            location,
            direction,
            block_pos,
            miss: true,
            inside: false,
            world_border_hit: false,
        }
    }

    pub fn hit(location: Vec3, direction: Direction, block_pos: BlockPos, inside: bool) -> Self {
        Self {
            location,
            direction,
            block_pos,
            miss: false,
            inside,
            world_border_hit: false,
        }
    }

    pub fn world_border_hit(location: Vec3, direction: Direction, block_pos: BlockPos) -> Self {
        Self {
            location,
            direction,
            block_pos,
            miss: false,
            inside: false,
            world_border_hit: true,
        }
    }
}

/// `net.minecraft.world.phys.EntityHitResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityHitResult {
    pub entity_id: EntityId,
    pub location: Vec3,
}

impl EntityHitResult {
    pub const fn new(entity_id: EntityId, location: Vec3) -> Self {
        Self {
            entity_id,
            location,
        }
    }
}

/// `net.minecraft.world.phys.HitResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitResult {
    Miss(BlockHitResult),
    Block(BlockHitResult),
    Entity(EntityHitResult),
}

impl HitResult {
    pub fn location(&self) -> Vec3 {
        match self {
            Self::Miss(b) | Self::Block(b) => b.location,
            Self::Entity(e) => e.location,
        }
    }

    pub fn is_miss(&self) -> bool {
        matches!(self, Self::Miss(_))
    }
}
