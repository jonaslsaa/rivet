//! Port of `net.minecraft.world.level.levelgen.WorldGenerationContext` (class,
//! 26.2) — the minY/depth window a placement derives from the generator and
//! height accessor.
//!
//! Java: `minY = max(heightAccessor.getMinY(), generator.getMinY())` and
//! `height = min(heightAccessor.getHeight(), generator.getGenDepth())`, both
//! computed once in the constructor. `PlacementContext extends
//! WorldGenerationContext`.
//!
//! RivetTodo(#232): the Paper `level()` accessor (nullable `Level` field, plus
//! its `NullPointerException` on a null level) is omitted — the `Level` type is
//! the world unit's and no current consumer reads it.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::height_accessor::LevelHeightAccessor;
use std::cmp;

/// `net.minecraft.world.level.levelgen.WorldGenerationContext`.
pub struct WorldGenerationContext {
    /// `minY` — `Math.max(heightAccessor.getMinY(), generator.getMinY())`.
    min_y: i32,
    /// `height` — `Math.min(heightAccessor.getHeight(), generator.getGenDepth())`.
    height: i32,
}

impl WorldGenerationContext {
    /// `new WorldGenerationContext(ChunkGenerator, LevelHeightAccessor)`.
    pub fn new(generator: &dyn ChunkGenerator, height_accessor: &dyn LevelHeightAccessor) -> Self {
        WorldGenerationContext {
            min_y: cmp::max(height_accessor.get_min_y(), generator.get_min_y()),
            height: cmp::min(height_accessor.get_height(), generator.get_gen_depth()),
        }
    }

    /// `getMinGenY()`.
    pub fn get_min_gen_y(&self) -> i32 {
        self.min_y
    }

    /// `getGenDepth()`.
    pub fn get_gen_depth(&self) -> i32 {
        self.height
    }
}
