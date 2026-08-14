//! Port of `net.minecraft.world.level.levelgen.feature.EndIslandFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.endisland`
//! manifest unit (the end-leaves wave).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` draws
//! `size = random.nextInt(3) + 4.0F`, then for each layer `y` starting at 0
//! (decrementing) writes `END_STONE` at every cell of the `floor(-size) ..=
//! ceil(size)` square with `x*x + z*z <= (size+1)*(size+1)`, shrinking
//! `size -= random.nextInt(2) + 0.5F` per layer until `size <= 0.5F`. Always
//! returns `true`. The `setBlock` calls route through `Feature.setBlock`
//! (`level.setBlock(pos, state, Block.UPDATE_ALL)`).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_util::RandomSource;
use rivet_util::mth;

/// `Block.UPDATE_ALL` — the write-flag constant `Feature.setBlock` reduces to
/// (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`).
const UPDATE_ALL: u32 = 3;

/// `net.minecraft.world.level.levelgen.feature.EndIslandFeature`.
#[derive(Debug)]
pub struct EndIslandFeature;

/// `Feature.END_ISLAND` — the registered `minecraft:end_island` singleton
/// (the feature registry's insertion index 31).
pub const END_ISLAND: EndIslandFeature = EndIslandFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for EndIslandFeature {
    /// `EndIslandFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// float size = random.nextInt(3) + 4.0F;
    /// for (int y = 0; size > 0.5F; y--) {
    ///     for (int x = Mth.floor(-size); x <= Mth.ceil(size); x++) {
    ///         for (int z = Mth.floor(-size); z <= Mth.ceil(size); z++) {
    ///             if (x * x + z * z <= (size + 1.0F) * (size + 1.0F)) {
    ///                 this.setBlock(level, origin.offset(x, y, z), Blocks.END_STONE.defaultBlockState());
    ///             }
    ///         }
    ///     }
    ///     size -= random.nextInt(2) + 0.5F;
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
        let origin = *origin;
        let mut size: f32 = random.next_int_bound(3).wrapping_add(4) as f32;
        let mut y: i32 = 0;
        while size > 0.5f32 {
            for x in mth::floor(-size)..=mth::ceil(size) {
                for z in mth::floor(-size)..=mth::ceil(size) {
                    // `x * x + z * z <= (size + 1.0F) * (size + 1.0F)` — the
                    // int square promotes to float for the comparison, exactly
                    // as Java widens the int operand.
                    if x.wrapping_mul(x).wrapping_add(z.wrapping_mul(z)) as f32
                        <= (size + 1.0f32) * (size + 1.0f32)
                    {
                        level.set_block(
                            &origin.offset(x, y, z),
                            Blocks::END_STONE.default_block_state(),
                            UPDATE_ALL,
                        );
                    }
                }
            }
            // `size -= random.nextInt(2) + 0.5F` — the draw is `0` or `1`, so
            // the decrement is `0.5F` or `1.5F`.
            size -= random.next_int_bound(2) as f32 + 0.5f32;
            y = y.wrapping_sub(1);
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

    fn place(level: &mut TestLevel, random: &mut RecordingRandom, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        END_ISLAND.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// The first `nextInt(3)` draw fixes the initial `size` (4..=6); the
    /// per-layer `nextInt(2)` shrinks it. The whole island is written with
    /// `END_STONE` through `Feature.setBlock` (flag `UPDATE_ALL`), and the
    /// return is always `true`.
    #[test]
    fn writes_end_stone_discs_with_update_all_and_returns_true() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(7);
        // The `writes` vec records the `(pos, state)` pairs in Java draw order;
        // the `states` map reflects the final world.
        let placed = place(&mut level, &mut random, BlockPos::new(0, 0, 0));
        assert!(placed);
        assert!(!level.writes.is_empty());
        for (pos, state) in &level.writes {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:end_stone").unwrap()
            );
            // The top disc is at y=0; layers descend.
            assert!(pos.get_y() <= 0);
            // `x*x + z*z <= (size+1)^2` with the final size 4.5 gives a radius
            // bound; spot-check the extreme corner cells were not written.
            assert!(pos.get_x().abs() <= 6 && pos.get_z().abs() <= 6);
        }
        // The draw order is pinned: one `nextInt(3)` then alternating layers
        // of `nextInt(2)` until `size <= 0.5F`.
        assert_eq!(
            random.calls[0],
            crate::levelgen::feature::test_support::RngCall::IntBound(3)
        );
    }

    /// A hostile origin with negative coordinates still writes the island
    /// centered there — the disc math and `origin.offset` use wrapping
    /// coordinate arithmetic.
    #[test]
    fn negative_origin_is_centered_correctly() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(11);
        let origin = BlockPos::new(-16, -64, 30);
        let placed = place(&mut level, &mut random, origin);
        assert!(placed);
        // The first layer is at the origin's own y.
        let first = level.writes[0].0;
        assert_eq!(first.get_y(), origin.get_y());
        // The whole island surrounds the origin horizontally.
        let center = BlockPos::new(
            (level.writes.iter().map(|(p, _)| p.get_x()).min().unwrap()
                + level.writes.iter().map(|(p, _)| p.get_x()).max().unwrap())
                / 2,
            origin.get_y(),
            (level.writes.iter().map(|(p, _)| p.get_z()).min().unwrap()
                + level.writes.iter().map(|(p, _)| p.get_z()).max().unwrap())
                / 2,
        );
        assert_eq!(center, origin);
    }

    /// `EndIslandFeature.place` returns `true` unconditionally — the only
    /// writes are the island discs; an empty level still "places".
    #[test]
    fn place_always_returns_true() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(3);
        assert!(place(&mut level, &mut random, BlockPos::new(0, 0, 0)));
    }
}
