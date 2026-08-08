//! `net.minecraft.world.level.LevelHeightAccessor` — the world's vertical
//! extent.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! LevelHeightAccessor.java`. This is the value root of the `BlockGetter`/
//! `LevelReader`/`LevelAccessor` interface chain (#232 value slice): height and
//! section arithmetic only, no block/chunk state. Section coordinates route
//! through `SectionPos.blockToSectionCoord` (an arithmetic `>> 4`) exactly as
//! Java.
//!
//! `LevelHeightAccessor.create` is the Java static factory returning an
//! anonymous implementation; `SimpleLevelHeightAccessor` mirrors that instance.

use rivet_registry::core::{BlockPos, SectionPos};

/// `LevelHeightAccessor` — height and section access for a level.
pub trait LevelHeightAccessor {
    /// `getHeight()`.
    fn get_height(&self) -> i32;

    /// `getMinY()`.
    fn get_min_y(&self) -> i32;

    /// `getMaxY()` — `getMinY() + getHeight() - 1`.
    fn get_max_y(&self) -> i32 {
        self.get_min_y()
            .wrapping_add(self.get_height())
            .wrapping_sub(1)
    }

    /// `getSectionsCount()` — `getMaxSectionY() - getMinSectionY() + 1`.
    fn get_sections_count(&self) -> i32 {
        self.get_max_section_y()
            .wrapping_sub(self.get_min_section_y())
            .wrapping_add(1)
    }

    /// `getMinSectionY()` — `SectionPos.blockToSectionCoord(getMinY())`.
    fn get_min_section_y(&self) -> i32 {
        SectionPos::block_to_section_coord(self.get_min_y())
    }

    /// `getMaxSectionY()` — `SectionPos.blockToSectionCoord(getMaxY())`.
    fn get_max_section_y(&self) -> i32 {
        SectionPos::block_to_section_coord(self.get_max_y())
    }

    /// `isInsideBuildHeight(BlockPos)`.
    fn is_inside_build_height_pos(&self, pos: &BlockPos) -> bool {
        self.is_inside_build_height(pos.get_y())
    }

    /// `isInsideBuildHeight(int blockY)` — `blockY >= getMinY() && blockY <=
    /// getMaxY()` (inclusive bounds).
    fn is_inside_build_height(&self, block_y: i32) -> bool {
        block_y >= self.get_min_y() && block_y <= self.get_max_y()
    }

    /// `isOutsideBuildHeight(BlockPos)`.
    fn is_outside_build_height_pos(&self, pos: &BlockPos) -> bool {
        self.is_outside_build_height(pos.get_y())
    }

    /// `isOutsideBuildHeight(int blockY)`.
    fn is_outside_build_height(&self, block_y: i32) -> bool {
        block_y < self.get_min_y() || block_y > self.get_max_y()
    }

    /// `getSectionIndex(int blockY)` — the section of `blockY` indexed from the
    /// world's min section.
    fn get_section_index(&self, block_y: i32) -> i32 {
        self.get_section_index_from_section_y(SectionPos::block_to_section_coord(block_y))
    }

    /// `getSectionIndexFromSectionY(int sectionY)`.
    fn get_section_index_from_section_y(&self, section_y: i32) -> i32 {
        section_y.wrapping_sub(self.get_min_section_y())
    }

    /// `getSectionYFromSectionIndex(int sectionIndex)`.
    fn get_section_y_from_section_index(&self, section_index: i32) -> i32 {
        section_index.wrapping_add(self.get_min_section_y())
    }
}

/// `LevelHeightAccessor.create(int minY, int height)` — the concrete value
/// Java's static factory wraps in an anonymous `LevelHeightAccessor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleLevelHeightAccessor {
    min_y: i32,
    height: i32,
}

impl SimpleLevelHeightAccessor {
    /// `LevelHeightAccessor.create(minY, height)`.
    pub fn new(min_y: i32, height: i32) -> Self {
        SimpleLevelHeightAccessor { min_y, height }
    }
}

impl LevelHeightAccessor for SimpleLevelHeightAccessor {
    fn get_height(&self) -> i32 {
        self.height
    }

    fn get_min_y(&self) -> i32 {
        self.min_y
    }
}

/// `LevelHeightAccessor.create(int minY, int height)`.
pub fn create(min_y: i32, height: i32) -> SimpleLevelHeightAccessor {
    SimpleLevelHeightAccessor::new(min_y, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overworld's superflat height access (`min_y = -64`, `height = 384`,
    /// so `max_y = 319`, sections `-4..=19` — 24 sections).
    fn overworld() -> SimpleLevelHeightAccessor {
        SimpleLevelHeightAccessor::new(-64, 384)
    }

    #[test]
    fn overworld_height_math() {
        let h = overworld();
        assert_eq!(h.get_min_y(), -64);
        assert_eq!(h.get_height(), 384);
        assert_eq!(h.get_max_y(), 319); // -64 + 384 - 1
        assert_eq!(h.get_min_section_y(), -4); // -64 >> 4
        assert_eq!(h.get_max_section_y(), 19); // 319 >> 4
        assert_eq!(h.get_sections_count(), 24); // 19 - (-4) + 1
    }

    #[test]
    fn section_index_math() {
        let h = overworld();
        // Section 0 (block y in [0,15]) is index 0 - (-4) = 4.
        assert_eq!(h.get_section_index(0), 4);
        assert_eq!(h.get_section_index(15), 4);
        assert_eq!(h.get_section_index(16), 5);
        // The bottom section (y in [-64,-49]) is index 0.
        assert_eq!(h.get_section_index(-64), 0);
        assert_eq!(h.get_section_index_from_section_y(-4), 0);
        assert_eq!(h.get_section_y_from_section_index(0), -4);
    }

    #[test]
    fn build_height_bounds_are_inclusive() {
        let h = overworld();
        assert!(h.is_inside_build_height(-64));
        assert!(h.is_inside_build_height(319));
        assert!(h.is_inside_build_height(0));
        assert!(!h.is_inside_build_height(-65));
        assert!(!h.is_inside_build_height(320));
        assert!(h.is_outside_build_height(-65));
        assert!(h.is_outside_build_height(320));
        assert!(!h.is_outside_build_height(-64));
        // The BlockPos overload delegates to the int overload.
        let pos = BlockPos::new(0, -64, 0);
        assert!(h.is_inside_build_height_pos(&pos));
        assert!(h.is_outside_build_height_pos(&BlockPos::new(0, -65, 0)));
    }

    #[test]
    fn create_factory_matches_java() {
        // `LevelHeightAccessor.create(0, 256)`.
        let h = create(0, 256);
        assert_eq!(h.get_min_y(), 0);
        assert_eq!(h.get_height(), 256);
        assert_eq!(h.get_max_y(), 255);
    }
}
