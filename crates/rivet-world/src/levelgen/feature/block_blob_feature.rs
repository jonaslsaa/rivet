//! Port of `net.minecraft.world.level.levelgen.feature.BlockBlobFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.blockblob`
//! manifest unit (issue #600).
//!
//! Java: `Feature<BlockBlobConfiguration>` whose `place` first walks the origin
//! down while it is above `getMinY() + 3` and `config.canPlaceOn().test(level,
//! origin.below())` is false; if it walked to `getMinY() + 3` or below it
//! returns `false`. It then places three blobs: for each, draws `xr/yr/zr =
//! nextInt(2)`, computes `tr = (xr + yr + zr) * 0.333F + 0.5F`, and writes
//! `config.state()` with `Block.UPDATE_ALL` (3) over every cell of the
//! `-xr..=xr` x `-yr..=yr` y `-zr..=zr` z box whose `distSqr(origin) <= tr*tr`
//! (Java `Vec3i.distSqr`, a `double`), before re-offsetting the origin by
//! `(-1 + nextInt(2), -nextInt(2), -1 + nextInt(2))`. Always returns `true`
//! once the descent gate passes.
//!
//! The `canPlaceOn` predicate dispatches through the erased
//! `BlockPredicate::test` (the `#399` dispatch surface); `distSqr` reads the
//! `Vec3i::dist_sqr` helper via the coordinate projection.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::BlockBlobConfiguration;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_util::RandomSource;

/// `Block.UPDATE_ALL` — the write-flag constant `BlockBlobFeature` uses.
const UPDATE_ALL: u32 = 3;

/// `net.minecraft.world.level.levelgen.feature.BlockBlobFeature`.
#[derive(Debug)]
pub struct BlockBlobFeature;

/// `Feature.BLOCK_BLOB` — the registered `minecraft:block_blob` singleton.
pub const BLOCK_BLOB: BlockBlobFeature = BlockBlobFeature;

impl FeatureBehavior<BlockBlobConfiguration> for BlockBlobFeature {
    /// `BlockBlobFeature.place(FeaturePlaceContext<BlockBlobConfiguration>)`.
    ///
    /// ```java
    /// while (origin.getY() > level.getMinY() + 3 && !config.canPlaceOn().test(level, origin.below())) {
    ///     origin = origin.below();
    /// }
    /// if (origin.getY() <= level.getMinY() + 3) {
    ///     return false;
    /// }
    /// for (int c = 0; c < 3; c++) {
    ///     int xr = random.nextInt(2);
    ///     int yr = random.nextInt(2);
    ///     int zr = random.nextInt(2);
    ///     float tr = (xr + yr + zr) * 0.333F + 0.5F;
    ///     for (BlockPos blockPos : BlockPos.betweenClosed(origin.offset(-xr, -yr, -zr), origin.offset(xr, yr, zr))) {
    ///         if (blockPos.distSqr(origin) <= tr * tr) {
    ///             level.setBlock(blockPos, config.state(), Block.UPDATE_ALL);
    ///         }
    ///     }
    ///     origin = origin.offset(-1 + random.nextInt(2), -random.nextInt(2), -1 + random.nextInt(2));
    /// }
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, BlockBlobConfiguration, R>,
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
        let mut origin = **origin;
        let min_y_plus_three = level.get_min_y().wrapping_add(3);
        while origin.get_y() > min_y_plus_three
            && !config.can_place_on().test(level, &origin.below())
        {
            origin = origin.below();
        }
        if origin.get_y() <= min_y_plus_three {
            return false;
        }
        for _ in 0..3 {
            let xr = random.next_int_bound(2);
            let yr = random.next_int_bound(2);
            let zr = random.next_int_bound(2);
            let tr = (xr.wrapping_add(yr).wrapping_add(zr)) as f32 * 0.333f32 + 0.5f32;
            let radius_sq = tr * tr;
            for block_pos in BlockPos::between_closed(
                origin.get_x().wrapping_sub(xr),
                origin.get_y().wrapping_sub(yr),
                origin.get_z().wrapping_sub(zr),
                origin.get_x().wrapping_add(xr),
                origin.get_y().wrapping_add(yr),
                origin.get_z().wrapping_add(zr),
            ) {
                let dist_sq = Vec3i::new(block_pos.get_x(), block_pos.get_y(), block_pos.get_z())
                    .dist_sqr(&Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z()));
                if dist_sq <= radius_sq as f64 {
                    level.set_block(&block_pos, config.state(), UPDATE_ALL);
                }
            }
            origin = origin.offset(
                -1i32.wrapping_add(random.next_int_bound(2)),
                random.next_int_bound(2).wrapping_neg(),
                -1i32.wrapping_add(random.next_int_bound(2)),
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyPositionalRandomFactory;

    fn config() -> BlockBlobConfiguration {
        BlockBlobConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:moss_block").unwrap()),
            always_true(),
        )
    }

    /// A `RandomSource` that always draws `nextInt(2) == 0` — the zero-radius
    /// blob: `xr = yr = zr = 0`, `tr = 0.5`, box is the single origin cell
    /// whose `distSqr = 0 <= 0.25` — and `-1 + nextInt(2) == -1` re-offsets.
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
        BLOCK_BLOB.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// An origin at `getMinY() + 3` returns `false` before the descent walks
    /// (the gate reads `> minY + 3`).
    #[test]
    fn at_min_y_plus_three_returns_false_without_draws() {
        let mut level = TestLevel::over(access());
        let mut random = ZeroRandom;
        // TestGenerator min_y = -64, so min_y + 3 = -61.
        assert!(!place_with(
            &mut level,
            BlockPos::new(0, -61, 0),
            &mut random
        ));
        assert!(level.writes.is_empty());
    }

    /// With `always_true` the descent never moves, so the three blobs place at
    /// the origin. Each zero-radius blob writes the single origin cell, then
    /// re-offsets by `(-1, 0, -1)`: blob writes land at `(0,0,0)`, `(-1,0,-1)`,
    /// `(-2,0,-2)` — the origin drift is load-bearing.
    #[test]
    fn zero_radius_blobs_track_the_drifting_origin() {
        let mut level = TestLevel::over(access());
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert_eq!(level.writes.len(), 3);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(level.writes[1].0, BlockPos::new(-1, 0, -1));
        assert_eq!(level.writes[2].0, BlockPos::new(-2, 0, -2));
        for (_, state) in &level.writes {
            assert_eq!(
                *state,
                BlockState::of(BlockId::from_name("minecraft:moss_block").unwrap())
            );
        }
    }
}
