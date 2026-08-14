//! Port of `net.minecraft.world.level.levelgen.feature.ChorusPlantFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.chorusplant`
//! manifest unit (the end-leaves wave).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` gates on the origin
//! being empty and the cell below being in `BlockTags.SUPPORTS_CHORUS_PLANT`
//! (the tag reads through `BlockState::is_in_tag`, which resolves the
//! `minecraft:supports_chorus_plant` tag — `["minecraft:end_stone"]`). When the
//! gates pass it delegates to `ChorusFlowerBlock.generatePlant(level, origin,
//! random, 8)` and returns `true`; otherwise returns `false`. The growth logic
//! itself is owned by the pending `mc.world.level.block` manifest unit and
//! lives in the `chorus_growth` STUB (see [`chorus_growth`]).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::chorus_growth::generate_plant;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.ChorusPlantFeature`.
#[derive(Debug)]
pub struct ChorusPlantFeature;

/// `Feature.CHORUS_PLANT` — the registered `minecraft:chorus_plant` singleton
/// (the feature registry's insertion index 5).
pub const CHORUS_PLANT: ChorusPlantFeature = ChorusPlantFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for ChorusPlantFeature {
    /// `ChorusPlantFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// if (level.isEmptyBlock(origin) && level.getBlockState(origin.below()).is(BlockTags.SUPPORTS_CHORUS_PLANT)) {
    ///     ChorusFlowerBlock.generatePlant(level, origin, random, 8);
    ///     return true;
    /// } else {
    ///     return false;
    /// }
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
        if level.is_empty_block(origin)
            && level
                .get_block_state(&origin.below())
                .is_in_tag("minecraft:supports_chorus_plant")
        {
            generate_plant(level, origin, random, 8);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access,
    };
    use rivet_registry::core::BlockPos;

    fn place(level: &mut TestLevel, random: &mut RecordingRandom, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        CHORUS_PLANT.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// A non-empty origin fails the gate before any draw and returns `false`.
    #[test]
    fn non_empty_origin_returns_false_before_drawing() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 0, 0),
            Blocks::END_STONE.default_block_state(),
        );
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random, BlockPos::new(0, 0, 0)));
        assert!(random.calls.is_empty());
    }

    /// An origin whose cell below is not `SUPPORTS_CHORUS_PLANT` returns
    /// `false` before drawing.
    #[test]
    fn non_support_below_returns_false_before_drawing() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, -1, 0), Blocks::AIR.default_block_state());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random, BlockPos::new(0, 0, 0)));
        assert!(random.calls.is_empty());
    }

    /// The gates pass on an empty origin over `END_STONE`: the growth writes
    /// the plant stem and the feature returns `true`. The first write is the
    /// origin cell itself — `generatePlant`'s initial stem, written with
    /// `Block.UPDATE_CLIENTS`.
    #[test]
    fn supported_empty_origin_grows_and_returns_true() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            Blocks::END_STONE.default_block_state(),
        );
        let mut random = RecordingRandom::new(3);
        let placed = place(&mut level, &mut random, BlockPos::new(0, 0, 0));
        assert!(placed);
        assert!(!level.writes.is_empty());
        // The origin stem was written first.
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
    }
}
