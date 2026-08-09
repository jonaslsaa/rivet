//! Port of `net.minecraft.world.level.levelgen.placement.PlacementContext`
//! (class, 26.2).
//!
//! Java: `PlacementContext extends WorldGenerationContext`, holding the
//! `WorldGenLevel`, `ChunkGenerator` and the optional top `PlacedFeature`.
//! Paper's constructor passes the level through to `WorldGenerationContext`'s
//! 3-arg form (`super(generator, level, level.getLevel())`, "Flat bedrock
//! generator settings"); the `Level` field is only consumed by the Paper
//! `level()` accessor, which is omitted here (the `Level` type is the world
//! unit's), so the superclass window is built from the plain 2-arg constructor.
//!
//! Per PORTING.md the superclass becomes an embedded struct field: the port
//! composes the `WorldGenerationContext` port (world_generation_context.rs),
//! built from `(generator, level)`. The inherited accessors read the composed
//! constructor-computed window — `getGenDepth()` the `height`
//! (`min(level.getHeight(), generator.getGenDepth())`) and `getMinGenY()` the
//! `min_y` (`max(level.getMinY(), generator.getMinY())`) — while
//! `PlacementContext.getMinY()` is a standalone accessor (not an override;
//! `WorldGenerationContext` has no `getMinY`) delegating to
//! `this.level.getMinY()`. So the two min accessors are distinct: the
//! inherited `min_y` also folds in `generator.getMinY()` via the superclass
//! constructor, the standalone reads the `WorldGenLevel` directly, and they
//! can differ (exactly as in Java).
//!
//! RivetTodo(#228): `getHeight(Heightmap.Types,int,int)`,
//! `getCarvingMask(ChunkPos)`, and `getBlockState(BlockPos)` reach into the
//! `WorldGenLevel` read-write surface (heightmaps, chunks, `BlockState`) and
//! defer with the block-state worldgen slice.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::placement::PlacedFeature;
use crate::levelgen::world_generation_context::WorldGenerationContext;

/// `net.minecraft.world.level.levelgen.placement.PlacementContext` — the
/// placement-time world/generator/top-feature context.
pub struct PlacementContext<'a> {
    /// The superclass window — Java `super(generator, level, level.getLevel())`.
    world_generation_context: WorldGenerationContext,
    /// `level` — the world generation level.
    level: &'a mut dyn WorldGenLevel,
    /// `generator` — the chunk generator.
    generator: &'a dyn ChunkGenerator,
    /// `topFeature` — `Optional<PlacedFeature>`, set by `placeWithBiomeCheck`.
    top_feature: Option<&'a PlacedFeature>,
}

impl<'a> PlacementContext<'a> {
    /// `new PlacementContext(WorldGenLevel, ChunkGenerator, Optional<PlacedFeature>)`
    /// — `super(generator, level, level.getLevel())`.
    pub fn new(
        level: &'a mut dyn WorldGenLevel,
        generator: &'a dyn ChunkGenerator,
        top_feature: Option<&'a PlacedFeature>,
    ) -> Self {
        let world_generation_context = WorldGenerationContext::new(generator, &*level);
        PlacementContext {
            world_generation_context,
            level,
            generator,
            top_feature,
        }
    }

    /// `getGenDepth()` — the inherited `WorldGenerationContext` window height.
    pub fn get_gen_depth(&self) -> i32 {
        self.world_generation_context.get_gen_depth()
    }

    /// `getMinGenY()` — inherited from `WorldGenerationContext`; Java's
    /// `PlacementContext` also declares a standalone `getMinY` (not an override
    /// — `WorldGenerationContext` has no `getMinY`) delegating to
    /// `this.level.getMinY()`, so the two accessors can differ.
    pub fn get_min_gen_y(&self) -> i32 {
        self.world_generation_context.get_min_gen_y()
    }

    /// `getMinY()` — a standalone accessor (not an override) delegating to
    /// `this.level.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.level.get_min_y()
    }

    /// `getLevel()` — the world generation level.
    pub fn get_level(&self) -> &dyn WorldGenLevel {
        self.level
    }

    /// `topFeature()` — the enclosing `PlacedFeature`, if any.
    pub fn top_feature(&self) -> Option<&'a PlacedFeature> {
        self.top_feature
    }

    /// `generator()`.
    pub fn generator(&self) -> &dyn ChunkGenerator {
        self.generator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};

    /// A minimal `WorldGenLevel` double over the overworld window
    /// (`minY -64`, `height 384`).
    struct TestLevel(SimpleLevelHeightAccessor);

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }

    impl crate::level::WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }
    }

    /// A `ChunkGenerator` double with a configurable generation depth.
    struct TestGenerator {
        depth: i32,
    }

    impl crate::chunk::ChunkGenerator for TestGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    #[test]
    fn window_is_min_of_level_height_and_generator_depth() {
        // `Math.min(level.getHeight(), generator.getGenDepth())`.
        let mut level = TestLevel(create(-64, 384));
        let generator = TestGenerator { depth: 384 };
        let context = PlacementContext::new(&mut level, &generator, None);
        assert_eq!(context.get_gen_depth(), 384);

        let mut level = TestLevel(create(-64, 384));
        let generator = TestGenerator { depth: 100 };
        let context = PlacementContext::new(&mut level, &generator, None);
        assert_eq!(context.get_gen_depth(), 100);
    }

    #[test]
    fn min_y_delegates_to_the_level() {
        // `PlacementContext.getMinY()` — a standalone accessor (not an
        // override; `WorldGenerationContext` has no `getMinY`) delegating to
        // `this.level.getMinY()`.
        let mut level = TestLevel(create(-64, 384));
        let generator = TestGenerator { depth: 384 };
        let context = PlacementContext::new(&mut level, &generator, None);
        assert_eq!(context.get_min_y(), -64);
    }

    #[test]
    fn accessors_expose_level_and_generator() {
        let mut level = TestLevel(create(-64, 384));
        let generator = TestGenerator { depth: 128 };
        let context = PlacementContext::new(&mut level, &generator, None);
        assert_eq!(context.get_level().get_height(), 384);
        assert_eq!(context.generator().get_gen_depth(), 128);
        assert!(context.top_feature().is_none());
    }
}
