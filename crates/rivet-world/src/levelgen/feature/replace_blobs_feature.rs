//! Port of `net.minecraft.world.level.levelgen.feature.ReplaceBlobsFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.replaceblobs`
//! manifest unit.
//!
//! Java: `Feature<ReplaceSphereConfiguration>` that turns netherrack blobs
//! into basalt/blackstone. `place` first searches downward from the
//! Y-clamped origin (`[getMinY() + 1, getMaxY()]`) for the first cell whose
//! block is `config.targetState.getBlock()`, moving `DOWN` one cell per step
//! and never inspecting `getMinY() + 1` itself. If none is found it returns
//! `false`. Otherwise it draws `radiusX/Y/Z` from `config.radius()`, iterates
//! `BlockPos.withinManhattan(center, radiusX, radiusY, radiusZ)`, breaking
//! once `distManhattan(center)` exceeds `max(radiusX, max(radiusY, radiusZ))`,
//! and replaces every cell whose state `is(targetBlock)` with
//! `config.replaceState`. Returns whether any replacement landed.
//!
//! The world reads (`get_block_state`) and writes (`set_block` with
//! `Block.UPDATE_ALL`) go through the `WorldGenLevel` seams (RivetTodo
//! #232); the test double overrides them.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::ReplaceSphereConfiguration;
use rivet_registry::core::{Axis, BlockPos, Direction, MutableBlockPos};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_ALL` — the write-flag constant `Feature.setBlock` reduces
/// to (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`), in contrast to `safeSetBlock`'s
/// `Block.UPDATE_CLIENTS`.
const UPDATE_ALL: u32 = 3;

/// `net.minecraft.world.level.levelgen.feature.ReplaceBlobsFeature`.
#[derive(Debug)]
pub struct ReplaceBlobsFeature;

/// `Feature.REPLACE_BLOBS` — the registered `minecraft:netherrack_replace_blobs`
/// singleton.
pub const REPLACE_BLOBS: ReplaceBlobsFeature = ReplaceBlobsFeature;

/// `ReplaceBlobsFeature.findTarget(LevelAccessor, BlockPos.MutableBlockPos,
/// Block)` — `@Nullable`; the first `is(target)` cell scanning `DOWN` from the
/// cursor, stopping above `getMinY() + 1` (`None` when the scan exhausts).
fn find_target(
    level: &dyn WorldGenLevel,
    cursor: &mut MutableBlockPos,
    target: BlockId,
) -> Option<BlockPos> {
    while cursor.get_y() > level.get_min_y().wrapping_add(1) {
        if level.get_block_state(&cursor.immutable()).block() == target {
            return Some(cursor.immutable());
        }
        cursor.move_dir(&Direction::Down);
    }
    None
}

impl FeatureBehavior<ReplaceSphereConfiguration> for ReplaceBlobsFeature {
    /// `ReplaceBlobsFeature.place(FeaturePlaceContext<ReplaceSphereConfiguration>)`.
    ///
    /// ```java
    /// Block targetBlock = config.targetState.getBlock();
    /// BlockPos centerPos = findTarget(level,
    ///     context.origin().mutable().clamp(Direction.Axis.Y,
    ///         level.getMinY() + 1, level.getMaxY()), targetBlock);
    /// if (centerPos == null) return false;
    /// int radiusX = config.radius().sample(random);
    /// int radiusY = config.radius().sample(random);
    /// int radiusZ = config.radius().sample(random);
    /// int maximumRadius = Math.max(radiusX, Math.max(radiusY, radiusZ));
    /// boolean replacedAny = false;
    /// for (BlockPos pos : BlockPos.withinManhattan(centerPos, radiusX, radiusY, radiusZ)) {
    ///     if (pos.distManhattan(centerPos) > maximumRadius) break;
    ///     BlockState blockState = level.getBlockState(pos);
    ///     if (blockState.is(targetBlock)) {
    ///         this.setBlock(level, pos, config.replaceState);
    ///         replacedAny = true;
    ///     }
    /// }
    /// return replacedAny;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, ReplaceSphereConfiguration, R>,
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
        let target_block = config.target_state.block();
        let mut cursor = origin.mutable();
        cursor.clamp(
            &Axis::Y,
            level.get_min_y().wrapping_add(1),
            level.get_max_y(),
        );
        let center_pos = match find_target(level, &mut cursor, target_block) {
            Some(center) => center,
            None => return false,
        };
        let radius_x = config.radius().sample(random);
        let radius_y = config.radius().sample(random);
        let radius_z = config.radius().sample(random);
        let maximum_radius = radius_x.max(radius_y.max(radius_z));
        let mut replaced_any = false;
        for pos in BlockPos::within_manhattan(&center_pos, radius_x, radius_y, radius_z) {
            if pos.dist_manhattan(&center_pos) > maximum_radius {
                break;
            }
            let block_state = level.get_block_state(&pos);
            if block_state.block() == target_block {
                level.set_block(&pos, config.replace_state, UPDATE_ALL);
                replaced_any = true;
            }
        }
        replaced_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_util::random::LegacyPositionalRandomFactory;
    use rivet_util::valueproviders::constant_int::ConstantInt;

    fn stone() -> BlockState {
        BlockState::of(Blocks::STONE.id())
    }

    fn granite() -> BlockState {
        BlockState::of(Blocks::GRANITE.id())
    }

    /// `radius = ConstantInt(0)` — the walk is the single found center cell.
    fn config_zero() -> ReplaceSphereConfiguration {
        ReplaceSphereConfiguration::new(
            stone(),
            granite(),
            rivet_util::valueproviders::int_provider::IntProvider::Constant(ConstantInt::of(0)),
        )
    }

    /// A `RandomSource` that draws `0` for every bound — the constant-radius
    /// configs never sample anyway.
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
        config: &ReplaceSphereConfiguration,
    ) -> bool {
        let generator = TestGenerator;
        REPLACE_BLOBS.place(&mut FeaturePlaceContext::new(
            None, level, &generator, random, &origin, config,
        ))
    }

    /// The origin is itself a target cell: `findTarget` stops immediately, the
    /// zero-radius walk replaces it, and the write is `config.replaceState`.
    #[test]
    fn replaces_target_at_origin() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        level.states.insert(origin, stone());
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, origin, &mut random, &config_zero()));
        assert_eq!(level.writes, vec![(origin, granite())]);
    }

    /// The target sits below the origin: `findTarget` scans `DOWN` from the
    /// Y-clamped origin until it hits the stone cell and replaces it there.
    #[test]
    fn scans_down_to_target() {
        let mut level = TestLevel::over(access());
        let target = BlockPos::new(0, 60, 0);
        level.states.insert(target, stone());
        let mut random = ZeroRandom;
        assert!(place_with(
            &mut level,
            BlockPos::new(0, 64, 0),
            &mut random,
            &config_zero()
        ));
        assert_eq!(level.writes, vec![(target, granite())]);
    }

    /// A hostile world with no target in the column: `findTarget` returns
    /// `None` and the feature returns `false` with no writes.
    #[test]
    fn no_target_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = ZeroRandom;
        assert!(!place_with(
            &mut level,
            BlockPos::new(0, 64, 0),
            &mut random,
            &config_zero()
        ));
        assert!(level.writes.is_empty());
    }

    /// `radius = ConstantInt(1)`: the manhattan walk (all three reach 1) covers
    /// the center and its six manhattan-neighbours; only the target cells are
    /// replaced — the three stone cells, air everywhere else.
    #[test]
    fn replaces_all_targets_in_radius() {
        let mut level = TestLevel::over(access());
        let center = BlockPos::new(0, 64, 0);
        for p in [center, center.offset(1, 0, 0), center.offset(0, 0, 1)] {
            level.states.insert(p, stone());
        }
        let config = ReplaceSphereConfiguration::new(
            stone(),
            granite(),
            rivet_util::valueproviders::int_provider::IntProvider::Constant(ConstantInt::of(1)),
        );
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, center, &mut random, &config));
        // The center and both stone neighbours are replaced; no air cell is.
        for p in [center, center.offset(1, 0, 0), center.offset(0, 0, 1)] {
            assert!(level.writes.contains(&(p, granite())));
        }
        assert_eq!(level.writes.len(), 3);
    }
}
