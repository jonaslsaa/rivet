//! Port of `net.minecraft.world.level.levelgen.feature.BasaltColumnsFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.basaltcolumns`
//! manifest unit.
//!
//! Java: `Feature<ColumnFeatureConfiguration>` that first reads the lava sea
//! level from the chunk generator and gates on the origin standing on
//! placeable ground (`canPlaceAt` — air/lava ocean with a non-forbidden solid
//! below). It then samples `height`, rolls the clustered `nextFloat < 0.9F`
//! branch (reach 5/count 50 vs reach 8/count 15), draws `count` random cells
//! from the `±reach` box around the origin, and grows a basalt column at each
//! drawn cell whose manhattan distance leaves room, using `config.reach()` for
//! the column's surrounding scan (`placeColumn`): for every cell in the `±reach`
//! box it locates the air/lava-ocean surface (or an air pocket, when the cell
//! is not air/lava) by walking down/up, then writes `BASALT` upward from that
//! surface for `columnHeight - stepLimit / 2` cells — stopping when a
//! non-basalt block blocks the column. Writes use `Feature.setBlock`
//! (`Block.UPDATE_ALL`, 3).
//!
//! The RNG order is load-bearing: `height().sample` first, then the clustered
//! `nextFloat`, then the `count` box draws (three `nextInt` per cell —
//! width/height/depth) INTERLEAVED with the body, which samples
//! `config.reach()` once per drawn cell that passes the manhattan gate
//! (`[cell1.xyz][reach1?][cell2.xyz][reach2?]...`, Java's lazy
//! `AbstractIterator`). The port keeps that exactly, including the
//! `||`/`&&` short-circuiting.
//!
//! `CANNOT_PLACE_ON` is the ten-block forbid list; the block-identity checks
//! (`is(Blocks.LAVA)`, `is(Blocks.BASALT)`, `isAir`) read the block id.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::ColumnFeatureConfiguration;
use rivet_registry::core::{BlockPos, Direction, MutableBlockPos};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Feature.setBlock` — `level.setBlock(pos, state, Block.UPDATE_ALL)`.
const UPDATE_ALL: u32 = 3;

/// `BasaltColumnsFeature.CANNOT_PLACE_ON` — the ten blocks a column may not
/// stand on (or find air through).
const CANNOT_PLACE_ON: &[BlockId] = &[
    Blocks::LAVA.id(),
    Blocks::BEDROCK.id(),
    Blocks::MAGMA_BLOCK.id(),
    Blocks::SOUL_SAND.id(),
    Blocks::NETHER_BRICKS.id(),
    Blocks::NETHER_BRICK_FENCE.id(),
    Blocks::NETHER_BRICK_STAIRS.id(),
    Blocks::NETHER_WART.id(),
    Blocks::CHEST.id(),
    Blocks::SPAWNER.id(),
];

/// `isAirOrLavaOcean(LevelAccessor, int, BlockPos)` — `state.isAir() ||
/// state.is(Blocks.LAVA) && pos.getY() <= lavaSeaLevel`.
fn is_air_or_lava_ocean(level: &dyn WorldGenLevel, lava_sea_level: i32, pos: &BlockPos) -> bool {
    let state = level.get_block_state(pos);
    state.is_air() || (state.block() == Blocks::LAVA.id() && pos.get_y() <= lava_sea_level)
}

/// `canPlaceAt(LevelAccessor, int, MutableBlockPos)` — the cell is air/lava
/// ocean and the block below is non-air and not on the forbid list.
fn can_place_at(
    level: &dyn WorldGenLevel,
    lava_sea_level: i32,
    cursor: &mut MutableBlockPos,
) -> bool {
    if !is_air_or_lava_ocean(level, lava_sea_level, &cursor.immutable()) {
        return false;
    }

    cursor.move_dir(&Direction::Down);
    let below = level.get_block_state(&cursor.immutable());
    cursor.move_dir(&Direction::Up);
    !below.is_air() && !CANNOT_PLACE_ON.contains(&below.block())
}

/// `findSurface(LevelAccessor, int, MutableBlockPos, int)` — walk down at most
/// `limit` cells to the first `canPlaceAt` cell (`null` when the walk exhausts
/// the limit or hits the bottom).
fn find_surface(
    level: &dyn WorldGenLevel,
    lava_sea_level: i32,
    cursor: &mut MutableBlockPos,
    mut limit: i32,
) -> Option<BlockPos> {
    while cursor.get_y() > level.get_min_y().wrapping_add(1) && limit > 0 {
        limit = limit.wrapping_sub(1);
        if can_place_at(level, lava_sea_level, cursor) {
            return Some(cursor.immutable());
        }
        cursor.move_dir(&Direction::Down);
    }

    None
}

/// `findAir(LevelAccessor, MutableBlockPos, int)` — walk up at most `limit`
/// cells to the first air cell (`null` when the walk exhausts the limit, tops
/// out, or crosses a forbid-listed block).
fn find_air(
    level: &dyn WorldGenLevel,
    cursor: &mut MutableBlockPos,
    mut limit: i32,
) -> Option<BlockPos> {
    while cursor.get_y() <= level.get_max_y() && limit > 0 {
        limit = limit.wrapping_sub(1);
        let state = level.get_block_state(&cursor.immutable());
        if CANNOT_PLACE_ON.contains(&state.block()) {
            return None;
        }
        if state.is_air() {
            return Some(cursor.immutable());
        }
        cursor.move_dir(&Direction::Up);
    }

    None
}

/// `net.minecraft.world.level.levelgen.feature.BasaltColumnsFeature`.
#[derive(Debug)]
pub struct BasaltColumnsFeature;

/// `Feature.BASALT_COLUMNS` — the registered `minecraft:basalt_columns`
/// singleton.
pub const BASALT_COLUMNS: BasaltColumnsFeature = BasaltColumnsFeature;

impl FeatureBehavior<ColumnFeatureConfiguration> for BasaltColumnsFeature {
    /// `BasaltColumnsFeature.place(FeaturePlaceContext<ColumnFeatureConfiguration>)`.
    ///
    /// ```java
    /// int lavaSeaLevel = context.chunkGenerator().getSeaLevel();
    /// BlockPos origin = context.origin();
    /// WorldGenLevel level = context.level();
    /// RandomSource random = context.random();
    /// ColumnFeatureConfiguration config = context.config();
    /// if (!canPlaceAt(level, lavaSeaLevel, origin.mutable())) {
    ///     return false;
    /// }
    ///
    /// int columnHeight = config.height().sample(random);
    /// boolean genereteClustered = random.nextFloat() < 0.9F;
    /// int reach = Math.min(columnHeight, genereteClustered ? 5 : 8);
    /// int count = genereteClustered ? 50 : 15;
    /// boolean placed = false;
    ///
    /// for (BlockPos pos : BlockPos.randomBetweenClosed(
    ///     random, count, origin.getX() - reach, origin.getY(), origin.getZ() - reach,
    ///     origin.getX() + reach, origin.getY(), origin.getZ() + reach
    /// )) {
    ///     int blocksToPlaceY = columnHeight - pos.distManhattan(origin);
    ///     if (blocksToPlaceY >= 0) {
    ///         placed |= this.placeColumn(level, lavaSeaLevel, pos, blocksToPlaceY, config.reach().sample(random));
    ///     }
    /// }
    ///
    /// return placed;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, ColumnFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            chunk_generator,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = **origin;
        let config = *config;
        let lava_sea_level = chunk_generator.get_sea_level();
        if !can_place_at(level, lava_sea_level, &mut origin.mutable()) {
            return false;
        }

        let column_height = config.height().sample(random);
        let clustered = random.next_float() < 0.9;
        let reach = column_height.min(if clustered { 5 } else { 8 });
        let count = if clustered { 50 } else { 15 };
        let mut placed = false;

        // `BlockPos.randomBetweenClosed` — inlined because Rivet's version
        // materializes the box up front, which would draw all `count * 3`
        // nextInts BEFORE the loop body and diverge from Java's lazy
        // `AbstractIterator` (three `nextInt` per cell, interleaved with the
        // body's `reach().sample`). `width`/`height`/`depth` are
        // `max - min + 1`; the box spans `±reach` in x/z at `origin.get_y()`.
        let min_x = origin.get_x().wrapping_sub(reach);
        let min_y = origin.get_y();
        let min_z = origin.get_z().wrapping_sub(reach);
        let max_x = origin.get_x().wrapping_add(reach);
        let max_z = origin.get_z().wrapping_add(reach);
        let width = max_x.wrapping_sub(min_x).wrapping_add(1);
        let height = 1;
        let depth = max_z.wrapping_sub(min_z).wrapping_add(1);
        let mut counter: i32 = count;
        while counter > 0 {
            counter = counter.wrapping_sub(1);
            let pos = BlockPos::new(
                min_x.wrapping_add(random.next_int_bound(width)),
                min_y.wrapping_add(random.next_int_bound(height)),
                min_z.wrapping_add(random.next_int_bound(depth)),
            );
            let blocks_to_place_y = column_height.wrapping_sub(pos.dist_manhattan(&origin));
            if blocks_to_place_y >= 0 {
                placed |= self.place_column(
                    level,
                    lava_sea_level,
                    &pos,
                    blocks_to_place_y,
                    config.reach().sample(random),
                );
            }
        }

        placed
    }
}

impl BasaltColumnsFeature {
    /// `placeColumn(LevelAccessor, int, BlockPos, int, int)` — scan the
    /// `±reach` box around `origin`, find each column's start, and grow it up.
    fn place_column(
        &self,
        level: &mut dyn WorldGenLevel,
        lava_sea_level: i32,
        origin: &BlockPos,
        column_height: i32,
        reach: i32,
    ) -> bool {
        let mut placed_any = false;

        for pos in BlockPos::between_closed(
            origin.get_x().wrapping_sub(reach),
            origin.get_y(),
            origin.get_z().wrapping_sub(reach),
            origin.get_x().wrapping_add(reach),
            origin.get_y(),
            origin.get_z().wrapping_add(reach),
        ) {
            let step_limit = pos.dist_manhattan(origin);
            let column_pos = if is_air_or_lava_ocean(level, lava_sea_level, &pos) {
                find_surface(level, lava_sea_level, &mut pos.mutable(), step_limit)
            } else {
                find_air(level, &mut pos.mutable(), step_limit)
            };
            if let Some(column_pos) = column_pos {
                let mut blocks_y = column_height.wrapping_sub(step_limit.wrapping_div(2));
                let mut cursor = column_pos.mutable();

                while blocks_y >= 0 {
                    if is_air_or_lava_ocean(level, lava_sea_level, &cursor.immutable()) {
                        level.set_block(
                            &cursor.immutable(),
                            Blocks::BASALT.default_block_state(),
                            UPDATE_ALL,
                        );
                        cursor.move_dir(&Direction::Up);
                        placed_any = true;
                    } else if level.get_block_state(&cursor.immutable()).block()
                        != Blocks::BASALT.id()
                    {
                        break;
                    } else {
                        cursor.move_dir(&Direction::Up);
                    }

                    blocks_y = blocks_y.wrapping_sub(1);
                }
            }
        }

        placed_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::FeaturePlaceContext;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, SeaLevelGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;
    use rivet_util::valueproviders::uniform_int::UniformInt;

    fn place(
        level: &mut TestLevel,
        generator: &SeaLevelGenerator,
        random: &mut RecordingRandom,
    ) -> bool {
        let config = ColumnFeatureConfiguration::new(
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Constant(ConstantInt::of(1)),
        );
        let origin = BlockPos::new(0, 0, 0);
        BASALT_COLUMNS.place(&mut FeaturePlaceContext::new(
            None, level, generator, random, &origin, &config,
        ))
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    /// A stone bed at `y = -1` over a wide area so any drawn cell in the `±1`
    /// box stands on placeable ground (`findSurface` succeeds one cell down).
    fn fill_stone_bed(level: &mut TestLevel) {
        for x in -3..=3 {
            for z in -3..=3 {
                level.states.insert(BlockPos::new(x, -1, z), stone());
            }
        }
    }

    /// The gate fails when the origin is not air/lava ocean (here a stone
    /// origin) — `false` with no RNG draws.
    #[test]
    fn blocked_origin_returns_false_without_draws() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), stone());
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &generator, &mut random));
        assert!(random.calls.is_empty());
    }

    /// The gate fails when the block below the origin is forbid-listed (lava) —
    /// `false` with no RNG draws.
    #[test]
    fn forbidden_bed_returns_false_without_draws() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            BlockState::of(BlockId::from_name("minecraft:lava").unwrap()),
        );
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &generator, &mut random));
        assert!(random.calls.is_empty());
    }

    /// A placeable origin grows basalt columns above the stone bed. The draw
    /// stream is pinned: one `nextFloat` (the clustered roll), then the
    /// `count` box draws — three `nextInt` per cell (`width` 3, `height` 1,
    /// `depth` 3) — for the clustered `count = 50`. Every write is basalt.
    #[test]
    fn clustered_placement_draws_the_box_and_writes_basalt() {
        let mut level = TestLevel::over(access());
        fill_stone_bed(&mut level);
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &generator, &mut random));
        // 1 nextFloat + 50 cells × 3 nextIntBound.
        assert_eq!(random.calls.len(), 1 + 150);
        assert_eq!(random.calls[0], RngCall::Float);
        assert_eq!(random.calls[1], RngCall::IntBound(3));
        assert_eq!(random.calls[2], RngCall::IntBound(1));
        assert_eq!(random.calls[3], RngCall::IntBound(3));
        assert!(!level.writes.is_empty());
        for (_, state) in &level.writes {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:basalt").unwrap()
            );
        }
    }

    /// `BlockPos.randomBetweenClosed` returns a LAZY `AbstractIterator` that
    /// draws each cell's three `nextInt` (width/height/depth) interleaved with
    /// the loop body — `[cell1.xyz][reach1?][cell2.xyz][reach2?]...`. Rivet's
    /// `random_between_closed` materializes the whole box up front (all cell
    /// draws before any body call), so the port inlines the lazy iteration to
    /// keep the exact Java draw order. This test pins that interleaving: a
    /// non-constant `reach` (`UniformInt(0, 4)` samples `nextInt(5)`, distinct
    /// from the box `width`/`depth` `nextInt(3)` and `height` `nextInt(1)`)
    /// with `height = 1` (box `±1`, width 3) records a box draw AFTER a reach
    /// sample — impossible if all box draws ran first.
    #[test]
    fn reach_samples_interleave_with_the_box_draws() {
        use crate::levelgen::feature::test_support::RngCall;
        let mut level = TestLevel::over(access());
        fill_stone_bed(&mut level);
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        let config = ColumnFeatureConfiguration::new(
            IntProvider::Uniform(UniformInt::of(0, 4)),
            IntProvider::Constant(ConstantInt::of(1)),
        );
        let origin = BlockPos::new(0, 0, 0);
        assert!(BASALT_COLUMNS.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));

        // The box draws: width 3, height 1, depth 3 over the clustered `count
        // = 50` cells — `IntBound(3)` x 100 (2 per cell), `IntBound(1)` x 50.
        // 35 cells pass the manhattan gate and each samples `reach`
        // (`IntBound(5)`).
        assert_eq!(
            random
                .calls
                .iter()
                .filter(|c| **c == RngCall::IntBound(3))
                .count(),
            100
        );
        assert_eq!(
            random
                .calls
                .iter()
                .filter(|c| **c == RngCall::IntBound(1))
                .count(),
            50
        );
        assert_eq!(
            random
                .calls
                .iter()
                .filter(|c| **c == RngCall::IntBound(5))
                .count(),
            35
        );
        // The stream opens with the clustered `nextFloat`, then the first
        // cell's width/height/depth box draws, then the first passing cell's
        // reach sample — the lazy `[cell][reach]` interleaving, not an
        // all-cells-first draw.
        assert_eq!(
            &random.calls[0..5],
            &[
                RngCall::Float,
                RngCall::IntBound(3),
                RngCall::IntBound(1),
                RngCall::IntBound(3),
                RngCall::IntBound(5),
            ]
        );
        // A box draw follows the first reach sample — impossible if all cell
        // draws ran before any body call (the eager-Vec divergence).
        let first_reach = random
            .calls
            .iter()
            .position(|c| *c == RngCall::IntBound(5))
            .expect("a reach sample per passing cell");
        assert!(
            random.calls[first_reach + 1..]
                .iter()
                .any(|c| *c == RngCall::IntBound(3))
        );
    }
}
