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
//! `getHeight(Heightmap.Types,int,int)` delegates to the `WorldGenLevel`
//! trait-default heightmap-read seam (see that method), which defers with the
//! block-state worldgen slice (#228).
//!
//! The remaining two Java accessors are omitted as intentional partial ports.
//! RivetTodo(#399): `getBlockState(BlockPos)` routes through
//! `WorldGenLevel.getBlockState`, the `#399` world-access seam — today the
//! reachable consumers call `context.get_level().get_block_state(...)` directly
//! (e.g. `CountOnEveryLayerPlacement`) instead of a `PlacementContext` accessor.
//! RivetTodo(#228): `getCarvingMask(ChunkPos)` routes through
//! `WorldGenLevel.getChunk(pos.x(), pos.z())` plus
//! `ProtoChunk.getOrCreateCarvingMask()`; the `WorldGenLevel` trait declares no
//! `get_chunk` read (it defers with the `ChunkAccess` spine, #228), so the
//! carving accessor has no replacement seam and no `CarvingMask` is reachable
//! through `PlacementContext` until a chunk read exists.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::heightmap::Types;
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

    /// The composed superclass window — `this` viewed as the
    /// `WorldGenerationContext` Java's `super(...)` built. Consumed by
    /// `HeightRangePlacement.getPositions` (`this.height.sample(random, this)`).
    pub fn world_generation_context(&self) -> &WorldGenerationContext {
        &self.world_generation_context
    }

    /// `getHeight(Heightmap.Types, int, int)` — `this.level.getHeight(type, x, z)`
    /// — the `LevelReader.getHeight` heightmap read, delegating to the
    /// `WorldGenLevel` trait-default seam.
    ///
    /// Consumed by the surface-relative placement filters
    /// (`SurfaceRelativeThresholdFilter`, `SurfaceWaterDepthFilter`),
    /// `HeightmapPlacement`, `CountOnEveryLayerPlacement`, and by
    /// `PlacedFeature.placeWithContext`-style callers. The read reaches into
    /// the worldgen `LevelReader` heightmap surface, which defers with the
    /// block-state worldgen slice (#228); the `WorldGenLevel` default fails
    /// explicitly rather than fabricating a surface, so a level that answers
    /// `getHeight` (a test double or a real world once #228 lands) keeps the
    /// concrete filter bodies executable.
    pub fn get_height(&self, ty: Types, x: i32, z: i32) -> i32 {
        self.level.get_height_at(ty, x, z)
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

        fn get_block_state(
            &self,
            _pos: &rivet_registry::core::BlockPos,
        ) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): no real world-access implementation is present —
            // the state-testing predicates surface the unavailable capability
            // explicitly (see `StateTestingPredicate::test`).
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    /// A `ChunkGenerator` double with a configurable generation depth.
    struct TestGenerator {
        depth: i32,
    }

    impl crate::chunk::ChunkGenerator for TestGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
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
    fn get_min_gen_y_is_max_of_level_and_generator_min_y() {
        // `getMinGenY()` — inherited `WorldGenerationContext` window:
        // `Math.max(level.getMinY(), generator.getMinY())`. When the
        // generator's minY exceeds the level's, the composed-window max-branch
        // wins and `get_min_gen_y()` differs from the standalone `get_min_y()`.
        let mut level = TestLevel(create(-64, 384));
        let generator = TestGenerator { depth: 384 };
        let context = PlacementContext::new(&mut level, &generator, None);
        assert_eq!(context.get_min_gen_y(), 0);
        assert_eq!(context.get_min_y(), -64);
        assert_ne!(context.get_min_gen_y(), context.get_min_y());
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
