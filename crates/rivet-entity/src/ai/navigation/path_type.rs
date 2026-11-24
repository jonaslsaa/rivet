//! Port of `net.minecraft.world.level.pathfinder.PathType`, `PathComputationType`,
//! `PathTypeCache`, and cardinal/vertical `Direction` helpers used by pathfinding.

use crate::ai::goal::context::BlockPos;

/// Mirrors `net.minecraft.world.level.pathfinder.PathType` in Paper 26.2.
///
/// Discriminants are dense (`0..PathType::COUNT`) so mob malus tables can be
/// stored in a fixed-size stack array without hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PathType {
    Blocked = 0,
    Open = 1,
    Walkable = 2,
    WalkableDoor = 3,
    Trapdoor = 4,
    PowderSnow = 5,
    DangerPowderSnow = 6,
    Fence = 7,
    Lava = 8,
    Water = 9,
    WaterBorder = 10,
    Rail = 11,
    UnpassableRail = 12,
    DangerFire = 13,
    DamageFire = 14,
    DangerOther = 15,
    DamageOther = 16,
    DoorOpen = 17,
    DoorWoodClosed = 18,
    DoorIronClosed = 19,
    Breach = 20,
    Leaves = 21,
    StickyHoney = 22,
    Cocoa = 23,
    DamageCautious = 24,
    DangerTrapdoor = 25,
    BigMobsCloseToDanger = 26,
}

impl PathType {
    pub const COUNT: usize = 27;

    /// All variants in Paper 26.2 `EnumSet<PathType>` ordinal order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Blocked,
        Self::Open,
        Self::Walkable,
        Self::WalkableDoor,
        Self::Trapdoor,
        Self::PowderSnow,
        Self::DangerPowderSnow,
        Self::Fence,
        Self::Lava,
        Self::Water,
        Self::WaterBorder,
        Self::Rail,
        Self::UnpassableRail,
        Self::DangerFire,
        Self::DamageFire,
        Self::DangerOther,
        Self::DamageOther,
        Self::DoorOpen,
        Self::DoorWoodClosed,
        Self::DoorIronClosed,
        Self::Breach,
        Self::Leaves,
        Self::StickyHoney,
        Self::Cocoa,
        Self::DamageCautious,
        Self::DangerTrapdoor,
        Self::BigMobsCloseToDanger,
    ];

    /// Exact default malus from Paper 26.2 `PathType.getMalus()`.
    #[must_use]
    pub const fn malus(self) -> f32 {
        match self {
            Self::Blocked => -1.0,
            Self::Open => 0.0,
            Self::Walkable => 0.0,
            Self::WalkableDoor => 0.0,
            Self::Trapdoor => 0.0,
            Self::PowderSnow => -1.0,
            Self::DangerPowderSnow => 0.0,
            Self::Fence => -1.0,
            Self::Lava => -1.0,
            Self::Water => 8.0,
            Self::WaterBorder => 8.0,
            Self::Rail => 0.0,
            Self::UnpassableRail => -1.0,
            Self::DangerFire => 8.0,
            Self::DamageFire => 16.0,
            Self::DangerOther => 8.0,
            Self::DamageOther => -1.0,
            Self::DoorOpen => 0.0,
            Self::DoorWoodClosed => -1.0,
            Self::DoorIronClosed => -1.0,
            Self::Breach => 4.0,
            Self::Leaves => -1.0,
            Self::StickyHoney => 8.0,
            Self::Cocoa => 0.0,
            Self::DamageCautious => 0.0,
            Self::DangerTrapdoor => 0.0,
            Self::BigMobsCloseToDanger => 4.0,
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Mirrors `net.minecraft.world.level.pathfinder.PathComputationType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathComputationType {
    Land,
    Water,
    Air,
}

/// Six-way axis-aligned direction matching `net.minecraft.core.Direction`
/// for pathfinding neighbor enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    /// Exact iteration order of `Direction.values()` in Minecraft 26.2.
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    /// Exact iteration order of `Direction.Plane.HORIZONTAL` (`NORTH`, `EAST`, `SOUTH`, `WEST`).
    pub const HORIZONTAL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    #[must_use]
    pub const fn step_x(self) -> i32 {
        match self {
            Self::West => -1,
            Self::East => 1,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn step_y(self) -> i32 {
        match self {
            Self::Down => -1,
            Self::Up => 1,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn step_z(self) -> i32 {
        match self {
            Self::North => -1,
            Self::South => 1,
            _ => 0,
        }
    }

    /// Mirrors `Direction.getClockWise()` for horizontal directions.
    #[must_use]
    pub const fn clock_wise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            other => other,
        }
    }
}

/// Packs a [`BlockPos`] into the exact 64-bit representation of
/// `net.minecraft.core.BlockPos.asLong(int x, int y, int z)` in MC 26.2.
///
/// Layout: `x` (26 bits at bit 38), `z` (26 bits at bit 12), `y` (12 bits at bit 0).
#[must_use]
pub const fn block_pos_as_long(x: i32, y: i32, z: i32) -> i64 {
    const PACKED_HORIZONTAL_MASK: i64 = (1_i64 << 26) - 1;
    const PACKED_Y_MASK: i64 = (1_i64 << 12) - 1;
    let mut node: i64 = 0;
    node |= ((x as i64) & PACKED_HORIZONTAL_MASK) << 38;
    node |= (y as i64) & PACKED_Y_MASK;
    node |= ((z as i64) & PACKED_HORIZONTAL_MASK) << 12;
    node
}

/// Exact port of `it.unimi.dsi.fastutil.HashCommon.mix(long)` used by
/// `PathTypeCache.index(long)`.
#[must_use]
pub const fn fastutil_mix_long(mut x: i64) -> i64 {
    let mut h = x.wrapping_mul(-7_046_029_254_386_353_131_i64);
    h ^= ((h as u64) >> 32) as i64;
    x = h ^ (((h as u64) >> 16) as i64);
    x
}

/// Port of `net.minecraft.world.level.pathfinder.PathTypeCache` in Paper 26.2.
///
/// Fixed 4096-entry direct-mapped cache keyed by `BlockPos.asLong()` mixed with
/// `HashCommon.mix(long) & 4095`.
#[derive(Debug, Clone)]
pub struct PathTypeCache {
    positions: Box<[i64; Self::SIZE]>,
    path_types: Box<[Option<PathType>; Self::SIZE]>,
}

impl Default for PathTypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PathTypeCache {
    pub const SIZE: usize = 4096;
    const MASK: usize = Self::SIZE - 1;

    #[must_use]
    pub fn new() -> Self {
        Self {
            positions: Box::new([0; Self::SIZE]),
            path_types: Box::new([None; Self::SIZE]),
        }
    }

    #[must_use]
    pub const fn index(pos_key: i64) -> usize {
        (fastutil_mix_long(pos_key) as usize) & Self::MASK
    }

    /// Mirrors `PathTypeCache.getOrCompute(BlockGetter, BlockPos)`.
    pub fn get_or_compute<F>(&mut self, pos: BlockPos, compute: F) -> PathType
    where
        F: FnOnce(BlockPos) -> PathType,
    {
        let key = block_pos_as_long(pos.x, pos.y, pos.z);
        let index = Self::index(key);
        if let Some(cached) = self.get(index, key) {
            return cached;
        }
        let computed = compute(pos);
        self.positions[index] = key;
        self.path_types[index] = Some(computed);
        computed
    }

    fn get(&self, index: usize, key: i64) -> Option<PathType> {
        if self.positions[index] == key {
            self.path_types[index]
        } else {
            None
        }
    }

    /// Mirrors `PathTypeCache.invalidate(BlockPos)`.
    pub fn invalidate(&mut self, pos: BlockPos) {
        let key = block_pos_as_long(pos.x, pos.y, pos.z);
        let index = Self::index(key);
        if self.positions[index] == key {
            self.path_types[index] = None;
        }
    }
}
