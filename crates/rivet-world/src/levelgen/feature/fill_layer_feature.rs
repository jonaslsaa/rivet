//! Port of `net.minecraft.world.level.levelgen.feature.FillLayerFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.filllayer`
//! manifest unit.
//!
//! Java: `Feature<LayerConfiguration>` whose `place` walks the 16x16 chunk
//! column at `getMinY() + config.height`, replacing air cells with
//! `config.state` (`Block.UPDATE_CLIENTS`). Always returns `true`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::LayerConfiguration;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `FillLayerFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.FillLayerFeature`.
#[derive(Debug)]
pub struct FillLayerFeature;

/// `Feature.FILL_LAYER` — the registered `minecraft:fill_layer` singleton.
pub const FILL_LAYER: FillLayerFeature = FillLayerFeature;

impl FeatureBehavior<LayerConfiguration> for FillLayerFeature {
    /// `FillLayerFeature.place(FeaturePlaceContext<LayerConfiguration>)`.
    ///
    /// ```java
    /// for (int dx = 0; dx < 16; dx++) {
    ///     for (int dz = 0; dz < 16; dz++) {
    ///         int x = origin.getX() + dx;
    ///         int z = origin.getZ() + dz;
    ///         int y = level.getMinY() + config.height;
    ///         pos.set(x, y, z);
    ///         if (level.getBlockState(pos).isAir()) {
    ///             level.setBlock(pos, config.state, Block.UPDATE_CLIENTS);
    ///         }
    ///     }
    /// }
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, LayerConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let origin = **origin;
        let config = *config;
        let mut pos = BlockPos::ZERO.mutable();
        let layer_y = level.get_min_y().wrapping_add(config.height);
        for dx in 0..16 {
            for dz in 0..16 {
                let x = origin.get_x().wrapping_add(dx);
                let z = origin.get_z().wrapping_add(dz);
                pos.set(x, layer_y, z);
                if level.get_block_state(&pos.immutable()).is_air() {
                    level.set_block(&pos.immutable(), config.state, UPDATE_CLIENTS);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;

    fn place_with(level: &mut TestLevel, origin: BlockPos, height: i32) -> bool {
        let config = LayerConfiguration::new(
            height,
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        FILL_LAYER.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &config,
        ))
    }

    /// A height of `0` lays the layer at `getMinY() + 0 = -64` over the full
    /// 16x16 column, writing every air cell (a default `TestLevel` is all air).
    #[test]
    fn fills_the_16x16_column_at_min_y_plus_height() {
        let mut level = TestLevel::over(access());
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), 0));
        assert_eq!(level.writes.len(), 16 * 16);
        let expected = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let mut seen = std::collections::BTreeSet::new();
        for (pos, state) in &level.writes {
            assert_eq!(pos.get_y(), -64);
            assert_eq!(*state, expected);
            assert!(seen.insert(*pos), "each cell written once");
        }
        for x in 0..16 {
            for z in 0..16 {
                assert!(seen.contains(&BlockPos::new(x, -64, z)));
            }
        }
    }

    /// The layer height is added to the world's `getMinY` (wrapping `int`
    /// arithmetic), so `height = 1` lands at `-63` — not an offset from the
    /// origin's y.
    #[test]
    fn height_is_relative_to_world_min_y_not_origin() {
        let mut level = TestLevel::over(access());
        assert!(place_with(&mut level, BlockPos::new(0, 100, 0), 1));
        assert_eq!(level.writes.len(), 16 * 16);
        assert!(level.writes.iter().all(|(pos, _)| pos.get_y() == -63));
    }

    /// A non-air cell is left untouched: pre-filling the layer row at one cell
    /// reduces the write count by one (the `isAir` gate).
    #[test]
    fn existing_blocks_are_not_replaced() {
        let mut level = TestLevel::over(access());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        level.states.insert(BlockPos::new(3, -64, 5), stone);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), 0));
        assert_eq!(level.writes.len(), 16 * 16 - 1);
        assert!(
            !level
                .writes
                .iter()
                .any(|(pos, _)| *pos == BlockPos::new(3, -64, 5))
        );
    }
}
