//! Port of `net.minecraft.server.level.ColumnPos` (record, 26.2) — the
//! `(x, z)` block-column identity used by `NoiseChunk`'s
//! `preliminarySurfaceLevelCache` (a `Long2IntMap` keyed on
//! `ColumnPos.asLong`).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/ColumnPos.java`.
//!
//! The record lives in `net.minecraft.server.level`, but the only current
//! consumer is the `mc.world.level.levelgen.noisegen` unit's `NoiseChunk` (the
//! `server.level` package itself is not yet ported), so the value leaf is
//! ported here as the noisegen unit's minimal prerequisite; it moves with the
//! server-level package when that unit lands.

use rivet_registry::core::ChunkPos;

/// `net.minecraft.server.level.ColumnPos(int x, int z)` — the block-column
/// identity record. `COORD_BITS = 32L`, `COORD_MASK = 4294967295L`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnPos {
    x: i32,
    z: i32,
}

impl ColumnPos {
    /// `ColumnPos(int x, int z)` — the record constructor.
    pub fn new(x: i32, z: i32) -> ColumnPos {
        ColumnPos { x, z }
    }

    /// `x()` (record accessor).
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `z()` (record accessor).
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `ColumnPos.toChunkPos()` — `new ChunkPos(SectionPos.blockToSectionCoord(x),
    /// SectionPos.blockToSectionCoord(z))`.
    pub fn to_chunk_pos(&self) -> ChunkPos {
        ChunkPos::new(
            rivet_registry::core::SectionPos::block_to_section_coord(self.x),
            rivet_registry::core::SectionPos::block_to_section_coord(self.z),
        )
    }

    /// `ColumnPos.toLong()` — `asLong(this.x, this.z)`.
    pub fn to_long(&self) -> i64 {
        Self::as_long(self.x, self.z)
    }

    /// `ColumnPos.asLong(int x, int z)` — `x & COORD_MASK | (z & COORD_MASK) <<
    /// 32`. The `& COORD_MASK` and `<<` are Java `long` arithmetic over the
    /// widened `int` operands — exact in Rust (no wrapping concern at 64-bit).
    pub fn as_long(x: i32, z: i32) -> i64 {
        (x as i64 & 0xFFFF_FFFF) | ((z as i64 & 0xFFFF_FFFF) << 32)
    }

    /// `ColumnPos.getX(long pos)` — `(int)(pos & COORD_MASK)`.
    pub fn get_x(pos: i64) -> i32 {
        (pos & 0xFFFF_FFFF) as i32
    }

    /// `ColumnPos.getZ(long pos)` — `(int)(pos >>> 32 & COORD_MASK)` — the
    /// logical shift (`>>>`).
    pub fn get_z(pos: i64) -> i32 {
        ((pos as u64 >> 32) & 0xFFFF_FFFF) as i32
    }

    /// `ColumnPos.hashCode()` — `ChunkPos.hash(this.x, this.z)`.
    pub fn hash_code(&self) -> i32 {
        ChunkPos::hash_coords(self.x, self.z)
    }
}

/// `ColumnPos.toString()` — `"[" + this.x + ", " + this.z + "]"`.
impl std::fmt::Display for ColumnPos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.x, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_long_round_trips_get_x_get_z() {
        for (x, z) in [
            (0, 0),
            (1, -1),
            (-64, 320),
            (i32::MAX, i32::MIN),
            (i32::MIN, i32::MAX),
            (123456789, -987654321),
        ] {
            let packed = ColumnPos::as_long(x, z);
            assert_eq!(ColumnPos::get_x(packed), x);
            assert_eq!(ColumnPos::get_z(packed), z);
            assert_eq!(ColumnPos::new(x, z).to_long(), packed);
        }
    }

    #[test]
    fn bit_layout_matches_java_constant() {
        // Java: `asLong(1, 2) = 1 & MASK | (2 & MASK) << 32`.
        assert_eq!(ColumnPos::as_long(1, 2), 1 | (2_i64 << 32));
        // Negative z occupies the high 32 bits (sign-extended by the `<< 32`).
        assert_eq!(ColumnPos::as_long(0, -1), 0xFFFF_FFFF_i64 << 32);
    }

    #[test]
    fn to_chunk_pos_uses_section_coord() {
        // SectionPos.blockToSectionCoord(1) = 0, (15) = 0, (16) = 1.
        let pos = ColumnPos::new(1, 16);
        let chunk = pos.to_chunk_pos();
        assert_eq!(chunk.x(), 0);
        assert_eq!(chunk.z(), 1);
    }

    #[test]
    fn hash_code_delegates_to_chunk_pos() {
        let pos = ColumnPos::new(3, -7);
        assert_eq!(pos.hash_code(), ChunkPos::hash_coords(3, -7));
        // The record's hashCode is position-dependent (never a constant).
        assert_ne!(
            ColumnPos::new(3, -7).hash_code(),
            ColumnPos::new(7, 3).hash_code()
        );
    }
}
