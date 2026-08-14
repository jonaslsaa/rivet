//! Port of `net.minecraft.world.level.levelgen.feature.BlockPileFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.blockpile`
//! manifest unit (issue #600).
//!
//! Java: `Feature<BlockPileConfiguration>` whose `place` gates on the origin
//! `y` being at least `getMinY() + 5`, then draws `xr = 2 + nextInt(2)` and
//! `zr = 2 + nextInt(2)` and iterates the `-xr..=xr` x `0..=1` y `-zr..=zr` z
//! box (two layers). For each cell it draws `random.nextFloat() * 10.0F -
//! random.nextFloat() * 6.0F` (two `nextFloat` draws, unconditionally) and
//! `tryPlaceBlock`s when `xd² + zd²` fits; otherwise it draws one more
//! `nextFloat` and `tryPlaceBlock`s when that is `< 0.031`. `tryPlaceBlock`
//! writes `config.stateProvider.getState(level, random, pos)` with
//! `Block.UPDATE_NONE` (260) when the cell is empty and `mayPlaceOn` passes —
//! `below.is(Blocks.DIRT_PATH) ? nextBoolean() : below.isFaceSturdy(level,
//! below, Direction.UP)` (the `WorldGenLevel::is_face_sturdy` seam, RivetTodo
//! #232). Always returns `true` once the `y` gate passes.
//!
//! The `stateProvider.getState` call dispatches through the
//! `block_state_provider_get_state` hub (the `#181` dispatch surface); the
//! `is(Blocks.DIRT_PATH)` identity check reads `get_block_state(...).block()`
//! against the generated id.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::BlockPileConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_get_state;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_NONE` — the write-flag constant `BlockPileFeature` uses
/// (`setBlock` with no client update). Paper defines it as
/// `UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` (`4 | 256`), NOT
/// zero: the "no client update" combination still suppresses block-entity
/// lifecycle side effects and hidden-block client updates.
const UPDATE_NONE: u32 = 260;

/// `BlockStateBase.is(Blocks.DIRT_PATH)` — the block identity check
/// `mayPlaceOn` branches on.
#[inline]
fn is_dirt_path(state: BlockState) -> bool {
    state.block()
        == BlockId::from_name("minecraft:dirt_path").expect("dirt_path is a generated block")
}

/// `mayPlaceOn` — `below.is(Blocks.DIRT_PATH) ? random.nextBoolean() :
/// below.isFaceSturdy(level, below, Direction.UP)`.
fn may_place_on<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    block_pos: &BlockPos,
    random: &mut R,
) -> bool {
    let below = block_pos.below();
    let below_state = level.get_block_state(&below);
    if is_dirt_path(below_state) {
        random.next_boolean()
    } else {
        level.is_face_sturdy(&below, &below_state, &Direction::Up)
    }
}

/// `tryPlaceBlock` — write the provider state with `UPDATE_NONE` when the cell
/// is empty and `mayPlaceOn` passes.
fn try_place_block<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    block_pos: &BlockPos,
    random: &mut R,
    config: &BlockPileConfiguration,
) {
    if level.is_empty_block(block_pos) && may_place_on(level, block_pos, random) {
        let state = block_state_provider_get_state(
            config.state_provider.as_ref(),
            level,
            random,
            block_pos,
        );
        level.set_block(block_pos, state, UPDATE_NONE);
    }
}

/// `net.minecraft.world.level.levelgen.feature.BlockPileFeature`.
#[derive(Debug)]
pub struct BlockPileFeature;

/// `Feature.BLOCK_PILE` — the registered `minecraft:block_pile` singleton.
pub const BLOCK_PILE: BlockPileFeature = BlockPileFeature;

impl FeatureBehavior<BlockPileConfiguration> for BlockPileFeature {
    /// `BlockPileFeature.place(FeaturePlaceContext<BlockPileConfiguration>)`.
    ///
    /// ```java
    /// if (origin.getY() < level.getMinY() + 5) {
    ///     return false;
    /// }
    /// int xr = 2 + random.nextInt(2);
    /// int zr = 2 + random.nextInt(2);
    /// for (BlockPos blockPos : BlockPos.betweenClosed(origin.offset(-xr, 0, -zr), origin.offset(xr, 1, zr))) {
    ///     int xd = origin.getX() - blockPos.getX();
    ///     int zd = origin.getZ() - blockPos.getZ();
    ///     if (xd * xd + zd * zd <= random.nextFloat() * 10.0F - random.nextFloat() * 6.0F) {
    ///         this.tryPlaceBlock(level, blockPos, random, config);
    ///     } else if (random.nextFloat() < 0.031) {
    ///         this.tryPlaceBlock(level, blockPos, random, config);
    ///     }
    /// }
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, BlockPileConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = *origin;
        let config = *config;
        if origin.get_y() < level.get_min_y().wrapping_add(5) {
            return false;
        }
        let xr = 2i32.wrapping_add(random.next_int_bound(2));
        let zr = 2i32.wrapping_add(random.next_int_bound(2));
        for block_pos in BlockPos::between_closed(
            origin.get_x().wrapping_sub(xr),
            origin.get_y(),
            origin.get_z().wrapping_sub(zr),
            origin.get_x().wrapping_add(xr),
            origin.get_y().wrapping_add(1),
            origin.get_z().wrapping_add(zr),
        ) {
            let xd = origin.get_x().wrapping_sub(block_pos.get_x());
            let zd = origin.get_z().wrapping_sub(block_pos.get_z());
            let radius_sq = random.next_float() * 10.0f32 - random.next_float() * 6.0f32;
            let dist_sq = xd.wrapping_mul(xd).wrapping_add(zd.wrapping_mul(zd)) as f32;
            // The two arms are intentionally identical: Java's
            // `if (distSq <= f) { tryPlaceBlock(...) } else if (nextFloat() < 0.031)
            // { tryPlaceBlock(...) }` calls the same helper from both branches
            // (the else-if is a second, low-probability placement chance). The
            // call site here differs only in the preceding RNG draw, which the
            // faithful port preserves. `#[allow]` (rather than factoring the
            // shared call out, which would read as a merged condition and lose
            // the two-distinct-checks shape) is the narrow, documented seam.
            #[allow(clippy::if_same_then_else)]
            if dist_sq <= radius_sq {
                try_place_block(level, &block_pos, random, config);
            } else if random.next_float() < 0.031f32 {
                try_place_block(level, &block_pos, random, config);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::stateproviders::simple;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_util::random::LegacyPositionalRandomFactory;
    use std::sync::Arc;

    fn stone_config() -> BlockPileConfiguration {
        BlockPileConfiguration::new(Arc::new(simple(Blocks::STONE.default_block_state())))
    }

    /// A fully scripted `RandomSource` so the tests can force each branch of
    /// the `place` RNG walk deterministically (the block pile draws two
    /// `nextInt(2)` bounds then a float per cell).
    #[derive(Clone, Copy)]
    struct ScriptedRandom {
        bound_value: i32,
        float_value: f32,
        bool_value: bool,
    }

    impl RandomSource for ScriptedRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            *self
        }
        fn fork_positional(&mut self) -> Self::Positional {
            LegacyPositionalRandomFactory::new(0)
        }
        fn set_seed(&mut self, _seed: i64) {}
        fn next_int(&mut self) -> i32 {
            0
        }
        fn next_int_bound(&mut self, _bound: i32) -> i32 {
            self.bound_value
        }
        fn next_long(&mut self) -> i64 {
            0
        }
        fn next_boolean(&mut self) -> bool {
            self.bool_value
        }
        fn next_float(&mut self) -> f32 {
            self.float_value
        }
        fn next_double(&mut self) -> f64 {
            self.float_value as f64
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    fn place(
        level: &mut TestLevel,
        origin: BlockPos,
        bound: i32,
        float: f32,
        bool_value: bool,
    ) -> bool {
        let generator = TestGenerator;
        let mut random = ScriptedRandom {
            bound_value: bound,
            float_value: float,
            bool_value,
        };
        BLOCK_PILE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &stone_config(),
        ))
    }

    /// An origin below `getMinY() + 5` returns `false` before any draw.
    #[test]
    fn below_min_y_plus_five_returns_false() {
        let mut level = TestLevel::over(access());
        // TestGenerator min_y = -64, so min_y + 5 = -59; origin y = -60 fails.
        assert!(!place(&mut level, BlockPos::new(0, -60, 0), 0, 1.0, true));
        assert!(level.writes.is_empty());
    }

    /// Bounds `0, 0` give `xr = zr = 2`: a 5x5 box per layer, two layers, 50
    /// cells. Floats of `0.0` make the circle expression `0.0 * 10 - 0.0 * 6 =
    /// 0.0`, so only the origin cell (`xd = zd = 0`, `distSq = 0`) fits the
    /// circle; every other cell falls to the `nextFloat < 0.031` else-if
    /// (`0.0 < 0.031` holds) — so every cell is reached. `mayPlaceOn` (below
    /// air, not dirt path) calls `isFaceSturdy`, defaulting true, and every
    /// cell writes the provider's stone. This pins the else-if path firing for
    /// the non-origin cells.
    #[test]
    fn every_cell_reached_and_writes_provider_state() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        assert!(place(&mut level, origin, 0, 0.0, true));
        assert_eq!(level.writes.len(), 50);
        for (pos, state) in &level.writes {
            assert_eq!(*state, Blocks::STONE.default_block_state());
            // Every write is inside the `-2..=2` x/z, `0..=1` y box.
            assert!((-2..=2).contains(&pos.get_x()));
            assert!((0..=1).contains(&pos.get_y()));
            assert!((-2..=2).contains(&pos.get_z()));
        }
        // The origin cell and its immediate axis neighbors are included.
        assert!(
            level
                .writes
                .iter()
                .any(|(p, _)| *p == BlockPos::new(0, 0, 0))
        );
        assert!(
            level
                .writes
                .iter()
                .any(|(p, _)| *p == BlockPos::new(2, 1, 2))
        );
    }

    /// Floats of `1.0` give `1.0 * 10 - 1.0 * 6 = 4.0`, so cells with
    /// `distSq <= 4` hit the circle (the `xd² + zd² <= 4` diamond: 13 cells per
    /// layer, 26 total) and the else-if never fires. Every circle cell writes.
    /// This pins the circle path firing with exactly the radius-2 diamond.
    #[test]
    fn circle_path_writes_the_radius_two_diamond() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        assert!(place(&mut level, origin, 0, 1.0, true));
        // 13 diamond cells per layer (distSq <= 4), two layers.
        assert_eq!(level.writes.len(), 26);
        for (pos, state) in &level.writes {
            assert_eq!(*state, Blocks::STONE.default_block_state());
            let dx = pos.get_x();
            let dz = pos.get_z();
            assert!(
                dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) <= 4,
                "circle cell ({dx}, {}) must satisfy distSq <= 4",
                pos.get_z()
            );
        }
    }

    /// `mayPlaceOn` over a `DIRT_PATH` below draws `nextBoolean` instead of
    /// consulting `isFaceSturdy`. The box's first layer (y=0) is itself seeded
    /// as dirt path, so those cells are non-empty and unplaceable; the second
    /// layer (y=1) sits on that dirt path and is the 5x5 = 25 cells that
    /// exercise the `nextBoolean` gate. With `bool_value = false` nothing may
    /// be placed; with `true` all 25 place.
    #[test]
    fn dirt_path_below_gates_writes_on_next_boolean() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        // Dirt path under the whole -2..=2 box (y=-1) and as the box's own
        // first layer (y=0), so every y=1 cell has a dirt-path below.
        for dx in -2..=2 {
            for dz in -2..=2 {
                level.states.insert(
                    BlockPos::new(dx, -1, dz),
                    Blocks::DIRT_PATH.default_block_state(),
                );
                level.states.insert(
                    BlockPos::new(dx, 0, dz),
                    Blocks::DIRT_PATH.default_block_state(),
                );
            }
        }
        // `nextBoolean = false` → nothing may be placed.
        assert!(place(&mut level, origin, 0, 0.0, false));
        assert!(level.writes.is_empty());
        // `nextBoolean = true` → every one of the 25 second-layer cells places.
        let mut level = TestLevel::over(access());
        for dx in -2..=2 {
            for dz in -2..=2 {
                level.states.insert(
                    BlockPos::new(dx, -1, dz),
                    Blocks::DIRT_PATH.default_block_state(),
                );
                level.states.insert(
                    BlockPos::new(dx, 0, dz),
                    Blocks::DIRT_PATH.default_block_state(),
                );
            }
        }
        assert!(place(&mut level, origin, 0, 0.0, true));
        assert_eq!(level.writes.len(), 25);
    }
}
