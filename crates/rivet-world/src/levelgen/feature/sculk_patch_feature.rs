//! Port of `net.minecraft.world.level.levelgen.feature.SculkPatchFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.sculk_patch` manifest
//! unit.
//!
//! Java: `Feature<SculkPatchConfiguration>` that grows a patch of sculk. `place`
//! first gates on `canSpreadFrom` (the origin cell is a `SculkBehaviour` block,
//! or is air / a water-source cell with at least one full-block neighbour),
//! then runs `spreadRounds() + growthRounds()` rounds of `SculkSpreader`
//! `addCursors`/`updateCursors` growth, optionally converts the origin's below
//! cell to a `SCULK_CATALYST` (when `catalystChance` and the below cell is a
//! full block), and finally grows `extraRareGrowths()` `SCULK_SHRIEKER`s on
//! face-sturdy air cells two away. Returns `true` unconditionally.
//!
//! This unit declares the reachable surface — the `SculkPatchFeature` struct
//! and its `Feature.SCULK_PATCH` singleton — but `place` DEFERS (RivetTodo
//! #232): the `canSpreadFrom` gate is not ported (its first disjunct needs the
//! `SculkBehaviour` interface, and its full-block neighbour test needs
//! `isCollisionShapeFullBlock`), and the growth loop drives
//! `SculkSpreader.createWorldGenSpreader()/addCursors/updateCursors/clear`
//! (in `net.minecraft.world.level.block`, outside this unit). The loop's
//! `addCursors`/`updateCursors` calls also interleave with the config draws
//! and precede the RNG-drawing catalyst/extra-growth sections, so skipping it
//! would break the placement draw order. The port therefore cannot return a
//! verdict without fabricating one — placement fails explicitly (a panic
//! naming the seam), the same capability-unavailable pattern as the `#232`
//! world seams and the `#399`/`#400` leaf deferrals. The `#181` dispatch arm
//! (id 62) is wired to that honest failure.

use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::SculkPatchConfiguration;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.SculkPatchFeature`.
#[derive(Debug)]
pub struct SculkPatchFeature;

/// `Feature.SCULK_PATCH` — the registered `minecraft:sculk_patch` singleton.
pub const SCULK_PATCH: SculkPatchFeature = SculkPatchFeature;

impl FeatureBehavior<SculkPatchConfiguration> for SculkPatchFeature {
    /// `SculkPatchFeature.place(FeaturePlaceContext<SculkPatchConfiguration>)`.
    ///
    /// DEFERS: the growth loop's `SculkSpreader` behavior is not ported
    /// (RivetTodo #232), and its calls interleave with the config draws and
    /// precede the RNG-drawing catalyst/extra-growth sections — so the draw
    /// stream cannot be reproduced without the spreader. The faithful body is:
    ///
    /// ```java
    /// WorldGenLevel level = context.level();
    /// BlockPos origin = context.origin();
    /// if (!this.canSpreadFrom(level, origin)) return false;
    /// SculkPatchConfiguration config = context.config();
    /// RandomSource random = context.random();
    /// SculkSpreader spreader = SculkSpreader.createWorldGenSpreader();
    /// int totalRounds = config.spreadRounds() + config.growthRounds();
    /// for (int round = 0; round < totalRounds; round++) {
    ///     for (int i = 0; i < config.chargeCount(); i++) {
    ///         spreader.addCursors(origin, config.amountPerCharge());
    ///     }
    ///     boolean spreadVeins = round < config.spreadRounds();
    ///     for (int i = 0; i < config.spreadAttempts(); i++) {
    ///         spreader.updateCursors(level, origin, random, spreadVeins);
    ///     }
    ///     spreader.clear();
    /// }
    /// BlockPos below = origin.below();
    /// if (random.nextFloat() <= config.catalystChance()
    ///         && level.getBlockState(below).isCollisionShapeFullBlock(level, below)) {
    ///     level.setBlock(origin, Blocks.SCULK_CATALYST.defaultBlockState(), Block.UPDATE_ALL);
    /// }
    /// int extraGrowths = config.extraRareGrowths().sample(random);
    /// for (int i = 0; i < extraGrowths; i++) {
    ///     BlockPos candidate = origin.offset(random.nextInt(5) - 2, 0, random.nextInt(5) - 2);
    ///     if (level.getBlockState(candidate).isAir()
    ///             && level.getBlockState(candidate.below()).isFaceSturdy(level, candidate.below(), Direction.UP)) {
    ///         level.setBlock(candidate,
    ///             Blocks.SCULK_SHRIEKER.defaultBlockState().setValue(SculkShriekerBlock.CAN_SUMMON, true),
    ///             Block.UPDATE_ALL);
    ///     }
    /// }
    /// return true;
    /// ```
    ///
    /// The `canSpreadFrom` gate itself is portable once `isCollisionShapeFullBlock`
    /// lands; until then the port cannot return a verdict without fabricating
    /// it, so placement fails explicitly (never `true`, never a silent no-op).
    fn place<R: RandomSource>(
        &self,
        _context: &mut FeaturePlaceContext<'_, SculkPatchConfiguration, R>,
    ) -> bool {
        panic!(
            "SculkPatchFeature.place is not implemented (RivetTodo #232: the SculkSpreader addCursors/updateCursors growth loop and isCollisionShapeFullBlock are not ported)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::configurations::SculkPatchConfiguration;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::core::BlockPos;
    use rivet_util::random::LegacyRandomSource;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;

    /// The `#232` deferral is honest: placing a sculk-patch feature panics
    /// with the seam message rather than returning a fabricated verdict.
    #[test]
    #[should_panic(expected = "RivetTodo #232")]
    fn place_fails_explicitly_until_sculk_spreader_lands() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let config = SculkPatchConfiguration::new(
            5,
            10,
            3,
            2,
            1,
            IntProvider::Constant(ConstantInt::of(0)),
            0.5,
        );
        let mut random = LegacyRandomSource::new(1);
        let _ = SCULK_PATCH.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
    }
}
