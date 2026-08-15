//! Port of `net.minecraft.world.level.levelgen.feature.DiskFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.disk` manifest unit.
//!
//! Java: `Feature<DiskConfiguration>` that grows a disk of blocks around the
//! origin's horizontal column. `place` computes `top = originY + halfHeight` and
//! `bottom = originY - halfHeight - 1`, draws the radius from `config.radius()`,
//! and walks `BlockPos.betweenClosed(origin.offset(-r, 0, -r),
//! origin.offset(r, 0, r))` — the radius`r` square at the origin's Y — writing a
//! column wherever `xd*xd + zd*zd <= r*r`. Each column scans `y` from `top` down
//! to (exclusive) `bottom`; when `config.target().test` holds, the
//! `stateProvider`'s optional state is written (`Block.UPDATE_CLIENTS`) and —
//! once per contiguous run of placed cells, reset whenever a non-target cell
//! breaks the run — `markAboveForPostProcessing` marks the cells two up.
//! `placeColumn` returns whether any write landed; `place` ORs the verdicts.
//!
//! The `markAboveForPostProcessing` helper (the target of
//! `ChunkAccess.markPosForPostProcessing`) and the scheduled-tick/`get_chunk`
//! seams go through the `WorldGenLevel` seams (RivetTodo #232); the test double
//! overrides them. The optional-state dispatch mirrors
//! `BlockStateProvider.getOptionalState` (only `RuleBasedStateProvider` can
//! return `None` — the `state != null` guard).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::DiskConfiguration;
use crate::levelgen::feature::mark_above_for_post_processing;
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_get_optional_state;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `DiskFeature.placeColumn`
/// passes to `level.setBlock` directly (Java `Block.UPDATE_CLIENTS`), in
/// contrast to `Feature.setBlock`'s `Block.UPDATE_ALL`.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.DiskFeature`.
#[derive(Debug)]
pub struct DiskFeature;

/// `Feature.DISK` — the registered `minecraft:disk` singleton.
pub const DISK: DiskFeature = DiskFeature;

/// `DiskFeature.placeColumn(LevelAccessor, RandomSource, int, int,
/// BlockPos.MutableBlockPos)` — write one column of the disk, `true` when any
/// cell landed. Java mutates the shared `MutableBlockPos` in place; the port
/// takes the immutable column base and rebuilds each cell (the same
/// `betweenClosed`/`setY` shape).
fn place_column<R: RandomSource>(
    config: &DiskConfiguration,
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    top: i32,
    bottom: i32,
    pos: &BlockPos,
) -> bool {
    let mut placed_any = false;
    let mut placed_above = false;
    for y in (bottom.wrapping_add(1)..=top).rev() {
        let cell = BlockPos::new(pos.get_x(), y, pos.get_z());
        if config.target().test(level, &cell) {
            if let Some(state) = block_state_provider_get_optional_state(
                config.state_provider().as_ref(),
                level,
                random,
                &cell,
            ) {
                level.set_block(&cell, state, UPDATE_CLIENTS);
                if !placed_above {
                    mark_above_for_post_processing(level, &cell);
                }
                placed_any = true;
                placed_above = true;
            }
        } else {
            placed_above = false;
        }
    }
    placed_any
}

impl FeatureBehavior<DiskConfiguration> for DiskFeature {
    /// `DiskFeature.place(FeaturePlaceContext<DiskConfiguration>)`.
    ///
    /// ```java
    /// int originY = origin.getY();
    /// int top = originY + config.halfHeight();
    /// int bottom = originY - config.halfHeight() - 1;
    /// int r = config.radius().sample(random);
    /// BlockPos.MutableBlockPos mutablePos = new BlockPos.MutableBlockPos();
    /// for (BlockPos columnPos : BlockPos.betweenClosed(
    ///         origin.offset(-r, 0, -r), origin.offset(r, 0, r))) {
    ///     int xd = columnPos.getX() - origin.getX();
    ///     int zd = columnPos.getZ() - origin.getZ();
    ///     if (xd * xd + zd * zd <= r * r) {
    ///         placedAny |= this.placeColumn(config, level, random, top, bottom,
    ///             mutablePos.set(columnPos));
    ///     }
    /// }
    /// return placedAny;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, DiskConfiguration, R>,
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
        let config = *config;
        let origin = *origin;
        let mut placed_any = false;
        let origin_y = origin.get_y();
        let top = origin_y.wrapping_add(config.half_height());
        let bottom = origin_y.wrapping_sub(config.half_height()).wrapping_sub(1);
        let r = config.radius().sample(random);
        for column_pos in
            BlockPos::between_closed_pos(&origin.offset(-r, 0, -r), &origin.offset(r, 0, r))
        {
            let xd = column_pos.get_x().wrapping_sub(origin.get_x());
            let zd = column_pos.get_z().wrapping_sub(origin.get_z());
            if xd.wrapping_mul(xd).wrapping_add(zd.wrapping_mul(zd)) <= r.wrapping_mul(r) {
                placed_any |= place_column(config, level, random, top, bottom, &column_pos);
            }
        }
        placed_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_util::random::LegacyPositionalRandomFactory;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;
    use std::sync::Arc;

    fn sand() -> BlockState {
        BlockState::of(Blocks::SAND.id())
    }

    fn dirt() -> BlockState {
        BlockState::of(Blocks::DIRT.id())
    }

    /// `radius = ConstantInt(1)`, `halfHeight = 1`: the disk covers the origin
    /// column and its four orthogonal neighbours, from `originY + 1` down to
    /// `originY - 1` (exclusive).
    fn config() -> DiskConfiguration {
        DiskConfiguration::new(
            Arc::new(simple(sand())),
            always_true(),
            IntProvider::Constant(ConstantInt::of(1)),
            1,
        )
    }

    /// A `RandomSource` that draws `0` for every bounded draw.
    #[derive(Clone, Copy)]
    struct ZeroRandom;

    impl RandomSource for ZeroRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            ZeroRandom
        }
        fn fork_positional(&mut self) -> Self::Positional {
            LegacyPositionalRandomFactory::new(0)
        }
        fn set_seed(&mut self, _seed: i64) {}
        fn next_int(&mut self) -> i32 {
            0
        }
        fn next_int_bound(&mut self, _bound: i32) -> i32 {
            0
        }
        fn next_long(&mut self) -> i64 {
            0
        }
        fn next_boolean(&mut self) -> bool {
            false
        }
        fn next_float(&mut self) -> f32 {
            0.0
        }
        fn next_double(&mut self) -> f64 {
            0.0
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    fn place_with<R: RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        DISK.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// The five in-circle columns (`r = 1`) each scan `top = originY + 1` down
    /// to `bottom = originY - 2` (exclusive): three cells per column, `15`
    /// writes, all sand. The `always_true` target and simple provider give
    /// `Some` for every cell, so the first cell of each column (the `top` cell)
    /// is marked for post-processing — five marks. `markAboveForPostProcessing`
    /// early-returns on air, so the two cells above each top cell are filled
    /// non-air (dirt) to let the marks land at the moved-up positions (`y + 1`
    /// and `y + 2`).
    #[test]
    fn disk_writes_five_columns_three_cells_each() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        // The two cells above each column's top cell (`y = 66/67`) must be
        // non-air for the post-processing marks to land.
        for x in -1..=1 {
            for z in -1..=1 {
                if x * x + z * z <= 1 {
                    level.states.insert(BlockPos::new(x, 66, z), dirt());
                    level.states.insert(BlockPos::new(x, 67, z), dirt());
                }
            }
        }
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, origin, &mut random));
        assert_eq!(level.writes.len(), 15);
        // Center column: top, middle, bottom cells (top = 65, bottom+1 = 63).
        for (y, expected) in [(65, sand()), (64, sand()), (63, sand())] {
            let p = BlockPos::new(0, y, 0);
            assert!(
                level.writes.contains(&(p, expected)),
                "missing write at {p:?}"
            );
        }
        // The disk is a plus: the diagonals (`|dx| == |dz| == 1`, distance
        // squared 2 > 1) are outside the circle.
        assert!(
            !level
                .writes
                .iter()
                .any(|(p, _)| p.get_x().abs() == 1 && p.get_z().abs() == 1)
        );
        // Post-processing marks the top cell of each of the five columns, moved
        // up two cells (`y + 1` and `y + 2`, both non-air).
        let mut marked = level.post_processing.to_vec();
        marked.sort_by_key(|p| (p.get_x(), p.get_z()));
        let mut expected_marks: Vec<_> = [0, -1, 1]
            .iter()
            .flat_map(|x| {
                [0, -1, 1].iter().map(move |z| {
                    if x * x + z * z <= 1 {
                        Some([BlockPos::new(*x, 66, *z), BlockPos::new(*x, 67, *z)])
                    } else {
                        None
                    }
                })
            })
            .flatten()
            .flatten()
            .collect();
        expected_marks.sort_by_key(|p| (p.get_x(), p.get_z()));
        assert_eq!(marked, expected_marks);
    }

    /// A target predicate that matches every position except a given `y` — the
    /// non-target gap cell that makes `placeColumn` reset `placedAbove` (the
    /// `else` branch of `config.target().test`).
    #[derive(Debug)]
    struct NotAtY(i32);

    impl crate::levelgen::blockpredicates::BlockPredicate for NotAtY {
        fn test(&self, _level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
            pos.get_y() != self.0
        }

        fn type_id(
            &self,
        ) -> crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypeId {
            crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes::TRUE
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// `placeColumn` marks only once per contiguous run: a non-target gap cell
    /// between two placed cells resets `placedAbove` (the `else` branch — a
    /// `None` from `get_optional_state` does NOT reset it), so the second run's
    /// first cell is marked too. The single column (`radius 0`, `halfHeight 2`)
    /// scans `y = 66..=62` (top-down); the target predicate rejects `y = 65`
    /// and the simple provider yields `Some(sand)` for every target cell, so
    /// `y = 66` writes (marking `(0,67)/(0,68)`), `y = 65` resets
    /// `placedAbove`, and `y = 64` writes again (marking `(0,65)/(0,66)`) — the
    /// reset path. A regression that kept `placedAbove` true across the gap
    /// would miss the `y = 64` mark.
    #[test]
    fn post_processing_marks_each_contiguous_run() {
        let config = DiskConfiguration::new(
            Arc::new(simple(sand())),
            Arc::new(NotAtY(65)),
            IntProvider::Constant(ConstantInt::of(0)),
            2,
        );
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        // The mark-helper early-returns when the cell one/two above is air:
        // the `y = 66` run mark reads `(0,67)/(0,68)` and the `y = 64` run mark
        // reads `(0,65)/(0,66)` (`(0,66)` is the sand written at the first
        // run). Fill `(0,65)/(0,67)/(0,68)` non-air so both marks land.
        for y in [65, 67, 68] {
            level.states.insert(BlockPos::new(0, y, 0), dirt());
        }
        let generator = TestGenerator;
        let mut random = ZeroRandom;
        assert!(DISK.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));
        // The four target cells (`y = 66`, `64`, `63`, `62`) are written sand
        // in scan order; the gap cell `y = 65` is not (the target rejected it,
        // so it was never a write candidate).
        assert_eq!(
            level.writes,
            vec![
                (BlockPos::new(0, 66, 0), sand()),
                (BlockPos::new(0, 64, 0), sand()),
                (BlockPos::new(0, 63, 0), sand()),
                (BlockPos::new(0, 62, 0), sand()),
            ]
        );
        // Both runs' first cells are marked (moved up two), in call order:
        // `y = 66` -> `(0,67)/(0,68)`, and the reset `y = 64` ->
        // `(0,65)/(0,66)`.
        assert_eq!(
            level.post_processing,
            vec![
                BlockPos::new(0, 67, 0),
                BlockPos::new(0, 68, 0),
                BlockPos::new(0, 65, 0),
                BlockPos::new(0, 66, 0),
            ]
        );
    }

    /// The `markAboveForPostProcessing` air-early-return: with a solid cell two
    /// up, the mark lands; with an air cell (the default), the helper returns
    /// before marking. Here the origin's `top` cell is written and the two cells
    /// above it are non-air (dirt), so the mark records both marked positions.
    #[test]
    fn mark_above_requires_non_air_above() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        // Only the center column: radius 0, halfHeight 0 (top = bottom + 1,
        // a single cell at y = 64). Make the two cells above non-air so the
        // mark is not early-returned.
        level.states.insert(origin.offset(0, 1, 0), dirt());
        level.states.insert(origin.offset(0, 2, 0), dirt());
        let config = DiskConfiguration::new(
            Arc::new(simple(sand())),
            always_true(),
            IntProvider::Constant(ConstantInt::of(0)),
            0,
        );
        let generator = TestGenerator;
        let mut random = ZeroRandom;
        assert!(DISK.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));
        assert_eq!(level.writes, vec![(origin, sand())]);
        // Both moved-up positions are non-air, so both are marked.
        assert_eq!(
            level.post_processing,
            vec![origin.offset(0, 1, 0), origin.offset(0, 2, 0)]
        );
    }
}
