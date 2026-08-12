//! `net.minecraft.core.QuartPos` — a 4-block quart coordinate.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/QuartPos.java`.
//! Pure-value bit helpers (`>> 2` arithmetic shift, `& 3`, `<< 2`); the
//! `fromSection`/`toSection` pair converts section (16-block) coordinates to
//! quart coordinates. Ported here because the `noise` unit's `NoiseSettings`
//! (`getCellHeight`/`getCellWidth`) needs it in `rivet-world`, and
//! `rivet-registry::core` already hosts the sibling `SectionPos` value type.

/// `net.minecraft.core.QuartPos`.
pub struct QuartPos;

impl QuartPos {
    /// `QuartPos.fromBlock(int blockCoord)` — `blockCoord >> 2`.
    pub fn from_block(block_coord: i32) -> i32 {
        block_coord >> 2
    }

    /// `QuartPos.quartLocal(int blockCoord)` — `blockCoord & 3`.
    pub fn quart_local(block_coord: i32) -> i32 {
        block_coord & 3
    }

    /// `QuartPos.toBlock(int quart)` — `quart << 2`.
    pub fn to_block(quart: i32) -> i32 {
        quart << 2
    }

    /// `QuartPos.fromSection(int section)` — `section << 2`.
    pub fn from_section(section: i32) -> i32 {
        section << 2
    }

    /// `QuartPos.toSection(int quart)` — `quart >> 2`.
    pub fn to_section(quart: i32) -> i32 {
        quart >> 2
    }
}

#[cfg(test)]
mod tests {
    use super::QuartPos;

    #[test]
    fn from_block_shifts_right_by_two() {
        assert_eq!(QuartPos::from_block(0), 0);
        assert_eq!(QuartPos::from_block(4), 1);
        assert_eq!(QuartPos::from_block(15), 3);
        // Negative block coords round down (Java `>>` arithmetic shift).
        assert_eq!(QuartPos::from_block(-4), -1);
        assert_eq!(QuartPos::from_block(-1), -1);
    }

    #[test]
    fn to_block_shifts_left_by_two() {
        assert_eq!(QuartPos::to_block(0), 0);
        assert_eq!(QuartPos::to_block(1), 4);
        assert_eq!(QuartPos::to_block(-1), -4);
    }

    #[test]
    fn quart_local_masks_low_two_bits() {
        assert_eq!(QuartPos::quart_local(0), 0);
        assert_eq!(QuartPos::quart_local(4), 0);
        assert_eq!(QuartPos::quart_local(5), 1);
        assert_eq!(QuartPos::quart_local(7), 3);
    }

    #[test]
    fn section_round_trips() {
        assert_eq!(QuartPos::to_section(QuartPos::from_section(3)), 3);
        assert_eq!(QuartPos::from_section(2), 8);
        assert_eq!(QuartPos::to_section(8), 2);
    }
}
