//! `net.minecraft.world.level.ChunkPos` — a chunk position.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/ChunkPos.java`.
//! Paper keeps the class a record with an explicit `longKey`; Rivet mirrors that
//! with a struct whose `long_key` is the packed long. The `isValid` bound moves
//! `ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE` to a `const` in `core/mod.rs`
//! (OWNERSHIP.md — ChunkPos lives here as a pure value type).
//!
//! RivetTodo(#126): `CODEC`/`STREAM_CODEC` — the codec surface lives in
//! `rivet-protocol` (issue #126 tracks the registry-wired codecs).

use super::block_pos::BlockPos;
use super::section_pos::SectionPos;
use crate::core::MAX_CHUNK_COORDINATE_VALUE;
use rivet_util::mth;

const COORD_MASK: i64 = 4294967295;
const REGION_BITS: i32 = 5;
const REGION_MASK: i32 = 31;
const HASH_A: i32 = 1664525;
const HASH_C: i32 = 1013904223;
const HASH_Z_XOR: i32 = -559038737;

/// `ChunkPos` — a chunk position (`x`, `z` chunk coords plus the packed long).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkPos {
    x: i32,
    z: i32,
    long_key: i64,
}

impl std::hash::Hash for ChunkPos {
    /// Java `ChunkPos.hashCode()` — `hash(x, z)`, not the derived `(x, z,
    /// longKey)` combination. `long_key` is a function of `(x, z)` so this
    /// stays consistent with `Eq`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(Self::hash_coords(self.x, self.z));
    }
}

impl ChunkPos {
    /// `ChunkPos.SAFETY_MARGIN` (chunks).
    pub const SAFETY_MARGIN: i32 = 1056;
    /// `ChunkPos.INVALID_CHUNK_POS`.
    pub const INVALID_CHUNK_POS: i64 = pack_const(1875066, 1875066);
    /// `ChunkPos.ZERO`.
    pub const ZERO: ChunkPos = ChunkPos {
        x: 0,
        z: 0,
        long_key: 0,
    };
    /// `ChunkPos.REGION_SIZE`.
    pub const REGION_SIZE: i32 = 32;
    /// `ChunkPos.REGION_MAX_INDEX`.
    pub const REGION_MAX_INDEX: i32 = 31;

    /// `new ChunkPos(int x, int z)` — packs `longKey`.
    pub fn new(x: i32, z: i32) -> ChunkPos {
        ChunkPos {
            x,
            z,
            long_key: pack_const(x, z),
        }
    }

    /// `ChunkPos.containing(BlockPos)`.
    pub fn containing(pos: &BlockPos) -> ChunkPos {
        ChunkPos::new(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
    }

    /// `ChunkPos.unpack(long)`.
    pub fn unpack(key: i64) -> ChunkPos {
        ChunkPos::new(Self::get_x(key), Self::get_z(key))
    }

    /// `ChunkPos.minFromRegion(int, int)`.
    pub fn min_from_region(region_x: i32, region_z: i32) -> ChunkPos {
        ChunkPos::new(region_x << REGION_BITS, region_z << REGION_BITS)
    }

    /// `ChunkPos.maxFromRegion(int, int)`.
    pub fn max_from_region(region_x: i32, region_z: i32) -> ChunkPos {
        ChunkPos::new(
            (region_x << REGION_BITS).wrapping_add(31),
            (region_z << REGION_BITS).wrapping_add(31),
        )
    }

    /// `ChunkPos.isValid()`.
    pub fn is_valid(&self) -> bool {
        Self::is_valid_coords(self.x, self.z)
    }

    /// `ChunkPos.isValid(int, int)` — `Mth.absMax(x, z) <=
    /// MAX_CHUNK_COORDINATE_VALUE`.
    pub fn is_valid_coords(x: i32, z: i32) -> bool {
        mth::abs_max(x, z) <= MAX_CHUNK_COORDINATE_VALUE
    }

    /// `ChunkPos.x()`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `ChunkPos.z()`.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `ChunkPos.pack()` — the `longKey`.
    pub fn pack(&self) -> i64 {
        self.long_key
    }

    /// `ChunkPos.pack(int, int)`.
    pub fn pack_coords(x: i32, z: i32) -> i64 {
        pack_const(x, z)
    }

    /// `ChunkPos.fromSectionNode(long)`.
    pub fn from_section_node(section_node: i64) -> i64 {
        pack_const(
            SectionPos::x_of(section_node),
            SectionPos::z_of(section_node),
        )
    }

    /// `ChunkPos.pack(BlockPos)`.
    pub fn pack_block_pos(pos: &BlockPos) -> i64 {
        pack_const(
            SectionPos::block_to_section_coord(pos.get_x()),
            SectionPos::block_to_section_coord(pos.get_z()),
        )
    }

    /// `ChunkPos.getX(long)`.
    pub fn get_x(pos: i64) -> i32 {
        (pos & COORD_MASK) as i32
    }

    /// `ChunkPos.getZ(long)`.
    pub fn get_z(pos: i64) -> i32 {
        ((pos >> 32) & COORD_MASK) as i32
    }

    /// `ChunkPos.hashCode()` — `hash(x, z)`.
    pub fn hash_code(&self) -> i32 {
        Self::hash_coords(self.x, self.z)
    }

    /// `ChunkPos.hash(int, int)`.
    pub fn hash_coords(x: i32, z: i32) -> i32 {
        let x_transform = HASH_A.wrapping_mul(x).wrapping_add(HASH_C);
        let z_transform = HASH_A.wrapping_mul(z ^ HASH_Z_XOR).wrapping_add(HASH_C);
        x_transform ^ z_transform
    }

    /// `ChunkPos.getMiddleBlockX()`.
    pub fn get_middle_block_x(&self) -> i32 {
        self.get_block_x(8)
    }

    /// `ChunkPos.getMiddleBlockZ()`.
    pub fn get_middle_block_z(&self) -> i32 {
        self.get_block_z(8)
    }

    /// `ChunkPos.getMinBlockX()`.
    pub fn get_min_block_x(&self) -> i32 {
        SectionPos::section_to_block_coord(self.x)
    }

    /// `ChunkPos.getMinBlockZ()`.
    pub fn get_min_block_z(&self) -> i32 {
        SectionPos::section_to_block_coord(self.z)
    }

    /// `ChunkPos.getMaxBlockX()`.
    pub fn get_max_block_x(&self) -> i32 {
        self.get_block_x(15)
    }

    /// `ChunkPos.getMaxBlockZ()`.
    pub fn get_max_block_z(&self) -> i32 {
        self.get_block_z(15)
    }

    /// `ChunkPos.getRegionX()`.
    pub fn get_region_x(&self) -> i32 {
        self.x >> REGION_BITS
    }

    /// `ChunkPos.getRegionZ()`.
    pub fn get_region_z(&self) -> i32 {
        self.z >> REGION_BITS
    }

    /// `ChunkPos.getRegionX(long)`.
    pub fn get_region_x_long(pos: i64) -> i32 {
        Self::get_x(pos) >> REGION_BITS
    }

    /// `ChunkPos.getRegionZ(long)`.
    pub fn get_region_z_long(pos: i64) -> i32 {
        Self::get_z(pos) >> REGION_BITS
    }

    /// `ChunkPos.getRegionLocalX()`.
    pub fn get_region_local_x(&self) -> i32 {
        self.x & REGION_MASK
    }

    /// `ChunkPos.getRegionLocalZ()`.
    pub fn get_region_local_z(&self) -> i32 {
        self.z & REGION_MASK
    }

    /// `ChunkPos.getBlockAt(int, int, int)`.
    pub fn get_block_at(&self, x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(self.get_block_x(x), y, self.get_block_z(z))
    }

    /// `ChunkPos.getBlockX(int)`.
    pub fn get_block_x(&self, offset: i32) -> i32 {
        SectionPos::section_to_block_coord_offset(self.x, offset)
    }

    /// `ChunkPos.getBlockZ(int)`.
    pub fn get_block_z(&self, offset: i32) -> i32 {
        SectionPos::section_to_block_coord_offset(self.z, offset)
    }

    /// `ChunkPos.getMiddleBlockPosition(int)`.
    pub fn get_middle_block_position(&self, y: i32) -> BlockPos {
        BlockPos::new(self.get_middle_block_x(), y, self.get_middle_block_z())
    }

    /// `ChunkPos.contains(BlockPos)`.
    pub fn contains(&self, pos: &BlockPos) -> bool {
        pos.get_x() >= self.get_min_block_x()
            && pos.get_z() >= self.get_min_block_z()
            && pos.get_x() <= self.get_max_block_x()
            && pos.get_z() <= self.get_max_block_z()
    }

    /// `ChunkPos.getWorldPosition()`.
    pub fn get_world_position(&self) -> BlockPos {
        BlockPos::new(self.get_min_block_x(), 0, self.get_min_block_z())
    }

    /// `ChunkPos.getChessboardDistance(ChunkPos)`.
    pub fn get_chessboard_distance(&self, pos: &ChunkPos) -> i32 {
        mth::chessboard_distance(pos.x, pos.z, self.x, self.z)
    }

    /// `ChunkPos.getChessboardDistance(int, int)` — `Mth.chessboardDistance(x,
    /// z, this.x, this.z)`.
    pub fn get_chessboard_distance_coords(&self, x: i32, z: i32) -> i32 {
        mth::chessboard_distance(x, z, self.x, self.z)
    }

    /// `ChunkPos.distanceSquared(ChunkPos)`.
    pub fn distance_squared(&self, pos: &ChunkPos) -> i32 {
        self.distance_squared_coords(pos.x, pos.z)
    }

    /// `ChunkPos.distanceSquared(long)`.
    pub fn distance_squared_long(&self, pos: i64) -> i32 {
        self.distance_squared_coords(Self::get_x(pos), Self::get_z(pos))
    }

    fn distance_squared_coords(&self, x: i32, z: i32) -> i32 {
        let delta_x = x.wrapping_sub(self.x);
        let delta_z = z.wrapping_sub(self.z);
        delta_x
            .wrapping_mul(delta_x)
            .wrapping_add(delta_z.wrapping_mul(delta_z))
    }

    /// `ChunkPos.rangeClosed(ChunkPos, int)`.
    pub fn range_closed(center: &ChunkPos, radius: i32) -> Vec<ChunkPos> {
        Self::range_closed_pos(
            &ChunkPos::new(center.x.wrapping_sub(radius), center.z.wrapping_sub(radius)),
            &ChunkPos::new(center.x.wrapping_add(radius), center.z.wrapping_add(radius)),
        )
    }

    /// `ChunkPos.rangeClosed(ChunkPos, ChunkPos)` — X-major, then Z.
    pub fn range_closed_pos(from: &ChunkPos, to: &ChunkPos) -> Vec<ChunkPos> {
        let x_size = (from.x.wrapping_sub(to.x)).wrapping_abs() + 1;
        let z_size = (from.z.wrapping_sub(to.z)).wrapping_abs() + 1;
        let x_diff = if from.x < to.x { 1 } else { -1 };
        let z_diff = if from.z < to.z { 1 } else { -1 };
        let mut out = Vec::with_capacity((x_size * z_size) as usize);
        let mut pos = *from;
        loop {
            out.push(pos);
            let (x, z) = (pos.x, pos.z);
            if x == to.x {
                if z == to.z {
                    break;
                }
                pos = ChunkPos::new(from.x, z.wrapping_add(z_diff));
            } else {
                pos = ChunkPos::new(x.wrapping_add(x_diff), z);
            }
        }
        out
    }
}

impl std::fmt::Display for ChunkPos {
    /// `ChunkPos.toString()` — `"[x, z]"`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.x, self.z)
    }
}

const fn pack_const(x: i32, z: i32) -> i64 {
    (x as i64 & COORD_MASK) | ((z as i64 & COORD_MASK) << 32)
}
