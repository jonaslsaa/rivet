//! Port of `net.minecraft.world.level.levelgen.feature.GlowstoneFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.glowstone`
//! manifest unit.
//!
//! Java: `Feature<NoneFeatureConfiguration>` that places a glowstone blob on
//! the underside of netherrack/basalt/blackstone. The origin cell must be empty
//! and its above neighbor one of the three host blocks, else `false`; the
//! origin is written with `Block.UPDATE_CLIENTS`, then up to 1500 attempts
//! place glowstone at a `nextInt(8)-nextInt(8), -nextInt(12), nextInt(8)-
//! nextInt(8)` offset cell that is air and touches exactly one existing
//! glowstone neighbor. Always returns `true` once the gate passes.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::Direction;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `GlowstoneFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `Blocks.GLOWSTONE.defaultBlockState()` — the single block this feature
/// writes (a method call, so a helper rather than a `const`).
fn glowstone() -> BlockState {
    Blocks::GLOWSTONE.default_block_state()
}

/// The host blocks `GlowstoneFeature` may attach to, and the block it writes.
/// Hoisted as ids so the 1500-attempt loop compares against constants instead
/// of re-resolving `BlockId::from_name` every iteration.
const GLOWSTONE_ID: rivet_registry::generated::blocks::BlockId = Blocks::GLOWSTONE.id();
const NETHERRACK_ID: rivet_registry::generated::blocks::BlockId = Blocks::NETHERRACK.id();
const BASALT_ID: rivet_registry::generated::blocks::BlockId = Blocks::BASALT.id();
const BLACKSTONE_ID: rivet_registry::generated::blocks::BlockId = Blocks::BLACKSTONE.id();

/// `net.minecraft.world.level.levelgen.feature.GlowstoneFeature`.
#[derive(Debug)]
pub struct GlowstoneFeature;

/// `Feature.GLOWSTONE_BLOB` — the registered `minecraft:glowstone_blob`
/// singleton.
pub const GLOWSTONE_BLOB: GlowstoneFeature = GlowstoneFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for GlowstoneFeature {
    /// `GlowstoneFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// if (!level.isEmptyBlock(origin)) return false;
    /// BlockState aboveState = level.getBlockState(origin.above());
    /// if (!aboveState.is(Blocks.NETHERRACK) && !aboveState.is(Blocks.BASALT)
    ///     && !aboveState.is(Blocks.BLACKSTONE)) return false;
    /// level.setBlock(origin, Blocks.GLOWSTONE.defaultBlockState(), Block.UPDATE_CLIENTS);
    /// for (int i = 0; i < 1500; i++) {
    ///     BlockPos placePos = origin.offset(
    ///         random.nextInt(8) - random.nextInt(8), -random.nextInt(12), random.nextInt(8) - random.nextInt(8));
    ///     if (level.getBlockState(placePos).isAir()) {
    ///         int neighbours = 0;
    ///         for (Direction direction : Direction.values()) {
    ///             if (level.getBlockState(placePos.relative(direction)).is(Blocks.GLOWSTONE)) neighbours++;
    ///             if (neighbours > 1) break;
    ///         }
    ///         if (neighbours == 1) level.setBlock(placePos, Blocks.GLOWSTONE.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///     }
    /// }
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = **origin;
        if !level.is_empty_block(&origin) {
            return false;
        }
        let above_state = level.get_block_state(&origin.above());
        if above_state.block() != NETHERRACK_ID
            && above_state.block() != BASALT_ID
            && above_state.block() != BLACKSTONE_ID
        {
            return false;
        }
        level.set_block(&origin, glowstone(), UPDATE_CLIENTS);
        for _ in 0..1500 {
            let place_pos = origin.offset(
                random
                    .next_int_bound(8)
                    .wrapping_sub(random.next_int_bound(8)),
                random.next_int_bound(12).wrapping_neg(),
                random
                    .next_int_bound(8)
                    .wrapping_sub(random.next_int_bound(8)),
            );
            if level.get_block_state(&place_pos).is_air() {
                let mut neighbours = 0;
                for direction in Direction::VALUES {
                    if level
                        .get_block_state(&place_pos.relative(&direction))
                        .block()
                        == GLOWSTONE_ID
                    {
                        neighbours += 1;
                    }
                    if neighbours > 1 {
                        break;
                    }
                }
                if neighbours == 1 {
                    level.set_block(&place_pos, glowstone(), UPDATE_CLIENTS);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access,
    };
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    fn place_with<R: rivet_util::RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        GLOWSTONE_BLOB.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration,
        ))
    }

    fn netherrack() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:netherrack").unwrap())
    }

    /// A non-empty origin fails the gate before any draw or write.
    #[test]
    fn non_empty_origin_returns_false_without_writes() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), netherrack());
        let mut random = RecordingRandom::new(7);
        assert!(!place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// An origin whose above neighbor is not a host block fails the gate with
    /// no draws (the origin's air read and above-block read only).
    #[test]
    fn wrong_above_block_returns_false_without_writes() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(7);
        assert!(!place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// The gate passes, the origin is written, and the 1500 attempts run. The
    /// origin write is recorded first; the recorded draws pin the exact Java
    /// draw order — the first attempt's `nextInt(8), nextInt(8), nextInt(12),
    /// nextInt(8), nextInt(8)` — and every recorded write is glowstone.
    #[test]
    fn writes_origin_then_attempts_1500_cells_with_pinned_draw_order() {
        use crate::levelgen::feature::test_support::RngCall;
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 1, 0), netherrack());
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        // The origin write always happens, and it is the first write.
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        // The blob attempts recorded after the origin: at most 1500.
        assert!(level.writes.len() <= 1500);
        for (_, state) in &level.writes {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:glowstone").unwrap()
            );
        }
        // Draw order: the gate does not draw, so the first five calls are the
        // first attempt's `nextInt(8) - nextInt(8)`, `-nextInt(12)`,
        // `nextInt(8) - nextInt(8)`.
        assert_eq!(
            &random.calls[0..5],
            &[
                RngCall::IntBound(8),
                RngCall::IntBound(8),
                RngCall::IntBound(12),
                RngCall::IntBound(8),
                RngCall::IntBound(8),
            ]
        );
    }
}
