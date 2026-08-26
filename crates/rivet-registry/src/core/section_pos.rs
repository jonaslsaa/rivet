//! `net.minecraft.core.SectionPos` — a 16-block section position.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/SectionPos.java`.
//! Preserves the packed-long bit layout (22-bit X/Z, 20-bit Y) and the wrapping
//! section arithmetic exactly.
//!
//! RivetTodo(#126): `STREAM_CODEC`/`CODEC` (codec surface →
//! rivet-protocol). `of(EntityAccess)` and `bottomOf(ChunkAccess)` need
//! `Entity`/`ChunkAccess` and defer with them. `aroundChunk` is ported — it
//! only needs the pure-value `ChunkPos`/bounds, not a level.

use super::block_pos::BlockPos;
use super::cursor3d::Cursor3D;
use super::direction::Direction;
use super::vec3i::{Vec3iLike, compare_coords};
use crate::core::chunk_pos::ChunkPos;
use rivet_util::mth;

/// `SectionPos` packing constants (Paper-inlined bit operations).
const PACKED_X_MASK: i64 = 4194303; // 22 bits
const PACKED_Y_MASK: i64 = 1048575; // 20 bits
const PACKED_Z_MASK: i64 = 4194303; // 22 bits

/// `SectionPos` — a 16-block section position (also a `Vec3i`).
#[derive(Clone, Copy, Debug)]
pub struct SectionPos {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

impl Vec3iLike for SectionPos {
    fn coords(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }
}

impl PartialEq for SectionPos {
    fn eq(&self, other: &Self) -> bool {
        self.coords() == other.coords()
    }
}

impl Eq for SectionPos {}

impl PartialOrd for SectionPos {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SectionPos {
    /// Lexicographic `(y, z, x)` ordering (see `vec3i::cmp_lexicographic_yzx`):
    /// a total order consistent with `Eq`, equal to the sign of Java
    /// `Vec3i.compareTo` whenever the coordinate subtractions do not overflow.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        super::vec3i::cmp_lexicographic_yzx((self.x, self.y, self.z), (other.x, other.y, other.z))
    }
}

impl std::hash::Hash for SectionPos {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl SectionPos {
    /// `SectionPos.SECTION_BITS`.
    pub const SECTION_BITS: i32 = 4;
    /// `SectionPos.SECTION_SIZE`.
    pub const SECTION_SIZE: i32 = 16;
    /// `SectionPos.SECTION_BLOCK_COUNT`.
    pub const SECTION_BLOCK_COUNT: i32 = 4096;
    /// `SectionPos.SECTION_MASK`.
    pub const SECTION_MASK: i32 = 15;
    /// `SectionPos.SECTION_HALF_SIZE`.
    pub const SECTION_HALF_SIZE: i32 = 8;
    /// `SectionPos.SECTION_MAX_INDEX`.
    pub const SECTION_MAX_INDEX: i32 = 15;

    /// `SectionPos.of(int, int, int)`.
    pub fn of(x: i32, y: i32, z: i32) -> SectionPos {
        SectionPos { x, y, z }
    }

    /// `SectionPos.of(BlockPos)`.
    pub fn of_block_pos(pos: &BlockPos) -> SectionPos {
        SectionPos::of(pos.get_x() >> 4, pos.get_y() >> 4, pos.get_z() >> 4)
    }

    /// `SectionPos.of(ChunkPos, int sectionY)`.
    pub fn of_chunk_pos(pos: &ChunkPos, section_y: i32) -> SectionPos {
        SectionPos::of(pos.x(), section_y, pos.z())
    }

    /// `SectionPos.of(Position)`.
    pub fn of_position(pos: &dyn super::Position) -> SectionPos {
        SectionPos::of(
            Self::block_to_section_coord_f64(pos.x()),
            Self::block_to_section_coord_f64(pos.y()),
            Self::block_to_section_coord_f64(pos.z()),
        )
    }

    /// `SectionPos.of(long sectionNode)`.
    pub fn of_long(section_node: i64) -> SectionPos {
        SectionPos::of(
            Self::x_of(section_node),
            Self::y_of(section_node),
            Self::z_of(section_node),
        )
    }

    /// `SectionPos.offset(long, Direction)`.
    pub fn offset_dir(section_node: i64, offset: &Direction) -> i64 {
        Self::offset(
            section_node,
            offset.step_x(),
            offset.step_y(),
            offset.step_z(),
        )
    }

    /// `SectionPos.getAdjacentFromBlockPos(int, int, int, Direction)` (Paper).
    pub fn get_adjacent_from_block_pos(x: i32, y: i32, z: i32, direction: &Direction) -> i64 {
        Self::as_long(
            (x >> 4).wrapping_add(direction.step_x()),
            (y >> 4).wrapping_add(direction.step_y()),
            (z >> 4).wrapping_add(direction.step_z()),
        )
    }

    /// `SectionPos.getAdjacentFromSectionPos(int, int, int, Direction)` (Paper).
    pub fn get_adjacent_from_section_pos(x: i32, y: i32, z: i32, direction: &Direction) -> i64 {
        Self::as_long(
            x.wrapping_add(direction.step_x()),
            y.wrapping_add(direction.step_y()),
            z.wrapping_add(direction.step_z()),
        )
    }

    /// `SectionPos.offset(long, int, int, int)`.
    pub fn offset(section_node: i64, step_x: i32, step_y: i32, step_z: i32) -> i64 {
        Self::as_long(
            Self::x_of(section_node).wrapping_add(step_x),
            Self::y_of(section_node).wrapping_add(step_y),
            Self::z_of(section_node).wrapping_add(step_z),
        )
    }

    /// `SectionPos.posToSectionCoord(double)`.
    pub fn pos_to_section_coord(pos: f64) -> i32 {
        Self::block_to_section_coord(mth::floor_d(pos))
    }

    /// `SectionPos.blockToSectionCoord(int)`.
    pub fn block_to_section_coord(block_coord: i32) -> i32 {
        block_coord >> 4
    }

    /// `SectionPos.blockToSectionCoord(double)`.
    pub fn block_to_section_coord_f64(coord: f64) -> i32 {
        mth::floor_d(coord) >> 4
    }

    /// `SectionPos.sectionRelative(int)`.
    pub fn section_relative(block_coord: i32) -> i32 {
        block_coord & 15
    }

    /// `SectionPos.sectionRelativePos(BlockPos)` — a packed `short`.
    pub fn section_relative_pos(pos: &BlockPos) -> i16 {
        (((pos.get_x() & 15) << 8) | ((pos.get_z() & 15) << 4) | (pos.get_y() & 15)) as i16
    }

    /// `SectionPos.sectionRelativeX(short)`.
    pub fn section_relative_x(relative: i16) -> i32 {
        (relative as i32 >> 8) & 15
    }

    /// `SectionPos.sectionRelativeY(short)`.
    pub fn section_relative_y(relative: i16) -> i32 {
        (relative as i32) & 15
    }

    /// `SectionPos.sectionRelativeZ(short)`.
    pub fn section_relative_z(relative: i16) -> i32 {
        (relative as i32 >> 4) & 15
    }

    /// `SectionPos.relativeToBlockX(short)`.
    pub fn relative_to_block_x(&self, relative: i16) -> i32 {
        self.min_block_x() + Self::section_relative_x(relative)
    }

    /// `SectionPos.relativeToBlockY(short)`.
    pub fn relative_to_block_y(&self, relative: i16) -> i32 {
        self.min_block_y() + Self::section_relative_y(relative)
    }

    /// `SectionPos.relativeToBlockZ(short)`.
    pub fn relative_to_block_z(&self, relative: i16) -> i32 {
        self.min_block_z() + Self::section_relative_z(relative)
    }

    /// `SectionPos.relativeToBlockPos(short)`.
    pub fn relative_to_block_pos(&self, relative: i16) -> BlockPos {
        BlockPos::new(
            self.relative_to_block_x(relative),
            self.relative_to_block_y(relative),
            self.relative_to_block_z(relative),
        )
    }

    /// `SectionPos.sectionToBlockCoord(int)`.
    pub fn section_to_block_coord(section_coord: i32) -> i32 {
        section_coord.wrapping_shl(Self::SECTION_BITS as u32)
    }

    /// `SectionPos.sectionToBlockCoord(int, int)`.
    pub fn section_to_block_coord_offset(section_coord: i32, offset: i32) -> i32 {
        Self::section_to_block_coord(section_coord).wrapping_add(offset)
    }

    /// `SectionPos.x(long)` — `(int)(sectionNode >> 42)` (Java writes
    /// `<< 0 >> 42`; the `<< 0` is an identity).
    pub fn x_of(section_node: i64) -> i32 {
        (section_node >> 42) as i32
    }

    /// `SectionPos.y(long)`.
    pub fn y_of(section_node: i64) -> i32 {
        ((section_node << 44) >> 44) as i32
    }

    /// `SectionPos.z(long)`.
    pub fn z_of(section_node: i64) -> i32 {
        ((section_node << 22) >> 42) as i32
    }

    /// `SectionPos.x()`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `SectionPos.y()`.
    pub fn y(&self) -> i32 {
        self.y
    }

    /// `SectionPos.z()`.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `SectionPos.getX()`.
    pub fn get_x(&self) -> i32 {
        self.x
    }

    /// `SectionPos.getY()`.
    pub fn get_y(&self) -> i32 {
        self.y
    }

    /// `SectionPos.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.z
    }

    /// `Vec3i.hashCode()` — `(y + z * 31) * 31 + x` (wrapping).
    pub fn hash_code(&self) -> i32 {
        (self.y.wrapping_add(self.z.wrapping_mul(31)))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// `Vec3i.compareTo(Vec3i)` — the exact int result.
    pub fn compare_to(&self, pos: &SectionPos) -> i32 {
        compare_coords(self.x, self.y, self.z, pos.x, pos.y, pos.z)
    }

    /// `SectionPos.minBlockX()`.
    pub fn min_block_x(&self) -> i32 {
        self.x << 4
    }

    /// `SectionPos.minBlockY()`.
    pub fn min_block_y(&self) -> i32 {
        self.y << 4
    }

    /// `SectionPos.minBlockZ()`.
    pub fn min_block_z(&self) -> i32 {
        self.z << 4
    }

    /// `SectionPos.maxBlockX()`.
    pub fn max_block_x(&self) -> i32 {
        Self::section_to_block_coord_offset(self.x, 15)
    }

    /// `SectionPos.maxBlockY()`.
    pub fn max_block_y(&self) -> i32 {
        Self::section_to_block_coord_offset(self.y, 15)
    }

    /// `SectionPos.maxBlockZ()`.
    pub fn max_block_z(&self) -> i32 {
        Self::section_to_block_coord_offset(self.z, 15)
    }

    /// `SectionPos.blockToSection(long blockNode)`.
    pub fn block_to_section(block_node: i64) -> i64 {
        let a = (block_node >> 42) as i32 & 4194303;
        let b = ((block_node << 52) >> 56) as i32 & 1048575;
        let c = ((block_node << 26) >> 42) as i32 & 4194303;
        Self::as_long(a, b, c)
    }

    /// `SectionPos.getZeroNode(int, int)`.
    pub fn get_zero_node(x: i32, z: i32) -> i64 {
        Self::get_zero_node_long(Self::as_long(x, 0, z))
    }

    /// `SectionPos.getZeroNode(long)`.
    pub fn get_zero_node_long(section_node: i64) -> i64 {
        section_node & -1048576
    }

    /// `SectionPos.sectionToChunk(long)`.
    pub fn section_to_chunk(section_node: i64) -> i64 {
        ChunkPos::pack_coords(Self::x_of(section_node), Self::z_of(section_node))
    }

    /// `SectionPos.origin()`.
    pub fn origin(&self) -> BlockPos {
        BlockPos::new(
            Self::section_to_block_coord(self.x),
            Self::section_to_block_coord(self.y),
            Self::section_to_block_coord(self.z),
        )
    }

    /// `SectionPos.center()`.
    pub fn center(&self) -> BlockPos {
        self.origin().offset(8, 8, 8)
    }

    /// `SectionPos.chunk()`.
    pub fn chunk(&self) -> ChunkPos {
        ChunkPos::new(self.x, self.z)
    }

    /// `SectionPos.asLong(BlockPos)`.
    pub fn as_long_block_pos(pos: &BlockPos) -> i64 {
        Self::as_long(
            Self::block_to_section_coord(pos.get_x()),
            Self::block_to_section_coord(pos.get_y()),
            Self::block_to_section_coord(pos.get_z()),
        )
    }

    /// `SectionPos.blockPosAsSectionLong(int, int, int)` (Paper).
    pub fn block_pos_as_section_long(x: i32, y: i32, z: i32) -> i64 {
        Self::as_long(x >> 4, y >> 4, z >> 4)
    }

    /// `SectionPos.asLong(int, int, int)`.
    pub const fn as_long(x: i32, y: i32, z: i32) -> i64 {
        ((x as i64 & PACKED_X_MASK) << 42)
            | (y as i64 & PACKED_Y_MASK)
            | ((z as i64 & PACKED_Z_MASK) << 20)
    }

    /// `SectionPos.asLong()`.
    pub fn as_long_self(&self) -> i64 {
        Self::as_long(self.x, self.y, self.z)
    }

    /// `SectionPos.offset(int, int, int)` — returns a new `SectionPos`.
    pub fn offset_sections(&self, x: i32, y: i32, z: i32) -> SectionPos {
        if x == 0 && y == 0 && z == 0 {
            *self
        } else {
            SectionPos::of(
                self.x.wrapping_add(x),
                self.y.wrapping_add(y),
                self.z.wrapping_add(z),
            )
        }
    }

    /// `SectionPos.blocksInside()` — the 16×16×16 blocks of this section.
    pub fn blocks_inside(&self) -> Vec<BlockPos> {
        BlockPos::between_closed(
            self.min_block_x(),
            self.min_block_y(),
            self.min_block_z(),
            self.max_block_x(),
            self.max_block_y(),
            self.max_block_z(),
        )
    }

    /// `SectionPos.cube(SectionPos, int radius)` — the axis-aligned cube of
    /// sections around `center`.
    pub fn cube(center: &SectionPos, radius: i32) -> Vec<SectionPos> {
        Self::between_closed_stream(
            center.x.wrapping_sub(radius),
            center.y.wrapping_sub(radius),
            center.z.wrapping_sub(radius),
            center.x.wrapping_add(radius),
            center.y.wrapping_add(radius),
            center.z.wrapping_add(radius),
        )
    }

    /// `SectionPos.aroundChunk(ChunkPos, int radius, int minSection, int
    /// maxSection)` — the sections between `(x±radius, minSection, z±radius)`
    /// and `(x±radius, maxSection, z±radius)`.
    pub fn around_chunk(
        center: &ChunkPos,
        radius: i32,
        min_section: i32,
        max_section: i32,
    ) -> Vec<SectionPos> {
        let x = center.x();
        let z = center.z();
        Self::between_closed_stream(
            x.wrapping_sub(radius),
            min_section,
            z.wrapping_sub(radius),
            x.wrapping_add(radius),
            max_section,
            z.wrapping_add(radius),
        )
    }

    /// `SectionPos.betweenClosedStream(int, int, int, int, int, int)`.
    pub fn between_closed_stream(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Vec<SectionPos> {
        let mut cursor = Cursor3D::new(min_x, min_y, min_z, max_x, max_y, max_z);
        let mut out = Vec::new();
        while cursor.advance() {
            out.push(SectionPos::of(
                cursor.next_x(),
                cursor.next_y(),
                cursor.next_z(),
            ));
        }
        out
    }

    /// `SectionPos.aroundAndAtBlockPos(BlockPos, LongConsumer)` — the section
    /// longs for a block position and its immediate neighbors, X/Y/Z-major.
    pub fn around_and_at_block_pos(block_pos: &BlockPos) -> Vec<i64> {
        Self::around_and_at_block_pos_xyz(block_pos.get_x(), block_pos.get_y(), block_pos.get_z())
    }

    /// `SectionPos.aroundAndAtBlockPos(long, LongConsumer)`.
    pub fn around_and_at_block_long(block_pos: i64) -> Vec<i64> {
        Self::around_and_at_block_pos_xyz(
            BlockPos::get_x_long(block_pos),
            BlockPos::get_y_long(block_pos),
            BlockPos::get_z_long(block_pos),
        )
    }

    /// `SectionPos.aroundAndAtBlockPos(int, int, int, LongConsumer)`.
    pub fn around_and_at_block_pos_xyz(block_x: i32, block_y: i32, block_z: i32) -> Vec<i64> {
        let min_section_x = Self::block_to_section_coord(block_x.wrapping_sub(1));
        let max_section_x = Self::block_to_section_coord(block_x.wrapping_add(1));
        let min_section_y = Self::block_to_section_coord(block_y.wrapping_sub(1));
        let max_section_y = Self::block_to_section_coord(block_y.wrapping_add(1));
        let min_section_z = Self::block_to_section_coord(block_z.wrapping_sub(1));
        let max_section_z = Self::block_to_section_coord(block_z.wrapping_add(1));
        if min_section_x == max_section_x
            && min_section_y == max_section_y
            && min_section_z == max_section_z
        {
            return vec![Self::as_long(min_section_x, min_section_y, min_section_z)];
        }
        let mut out = Vec::new();
        for section_x in min_section_x..=max_section_x {
            for section_y in min_section_y..=max_section_y {
                for section_z in min_section_z..=max_section_z {
                    out.push(Self::as_long(section_x, section_y, section_z));
                }
            }
        }
        out
    }
}

impl std::fmt::Display for SectionPos {
    /// `SectionPos.toString()` — Guava `MoreObjects.toStringHelper`, using the
    /// concrete `SectionPos` class name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            super::vec3i::format_helper("SectionPos", self.x, self.y, self.z)
        )
    }
}

// `Vec3i.relative` is defined on `Vec3i`; `SectionPos` inherits coordinate math
// through `Vec3iLike` so it can be passed to cross-type arithmetic.
