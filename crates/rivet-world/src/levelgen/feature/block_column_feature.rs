//! Port of `net.minecraft.world.level.levelgen.feature.BlockColumnFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.blockcolumn`
//! manifest unit.
//!
//! Java: `Feature<BlockColumnConfiguration>` that first samples every layer's
//! height (`config.layers().get(i).height().sample(random)`, accumulated into
//! `totalHeight` — the sample draws happen up front, in layer order) and returns
//! `false` when nothing is to be placed. It then walks the column upward from
//! the origin, testing `allowedPlacement` at each next cell; the first cell the
//! predicate rejects truncates the sampled heights (`truncate` removes the
//! excess from the tip-first or base-first layers, per `prioritizeTip`) and the
//! walk stops. Finally it writes each layer's sampled count of its provider
//! state, advancing the cursor one `direction` per cell, with
//! `Block.UPDATE_CLIENTS`.
//!
//! The RNG order is load-bearing: all `height().sample` draws happen before any
//! predicate test, and the provider `getState` draws happen per written cell in
//! layer/step order. The port keeps that exactly.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::BlockColumnConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider_get_state;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `BlockColumnFeature.truncate(int[], int, int, boolean)` — remove
/// `totalHeight - newHeight` cells from the sampled layer heights, walking the
/// layers tip-first (index 0 up) when `prioritizeTip` is set, base-first
/// otherwise.
fn truncate(layer_heights: &mut [i32], total_height: i32, new_height: i32, prioritize_tip: bool) {
    let mut amount_to_remove = total_height.wrapping_sub(new_height);
    let direction: i32 = if prioritize_tip { 1 } else { -1 };
    let start: i32 = if prioritize_tip {
        0
    } else {
        layer_heights.len() as i32 - 1
    };
    let end: i32 = if prioritize_tip {
        layer_heights.len() as i32
    } else {
        -1
    };

    let mut i = start;
    while i != end && amount_to_remove > 0 {
        let this_layer = layer_heights[i as usize];
        let to_remove_from_layer = this_layer.min(amount_to_remove);
        amount_to_remove = amount_to_remove.wrapping_sub(to_remove_from_layer);
        layer_heights[i as usize] = layer_heights[i as usize].wrapping_sub(to_remove_from_layer);
        i = i.wrapping_add(direction);
    }
}

/// `net.minecraft.world.level.levelgen.feature.BlockColumnFeature`.
#[derive(Debug)]
pub struct BlockColumnFeature;

/// `Feature.BLOCK_COLUMN` — the registered `minecraft:block_column` singleton.
pub const BLOCK_COLUMN: BlockColumnFeature = BlockColumnFeature;

impl FeatureBehavior<BlockColumnConfiguration> for BlockColumnFeature {
    /// `BlockColumnFeature.place(FeaturePlaceContext<BlockColumnConfiguration>)`.
    ///
    /// ```java
    /// WorldGenLevel level = context.level();
    /// BlockColumnConfiguration config = context.config();
    /// RandomSource random = context.random();
    /// int layerCount = config.layers().size();
    /// int[] layerHeights = new int[layerCount];
    /// int totalHeight = 0;
    ///
    /// for (int i = 0; i < layerCount; i++) {
    ///     layerHeights[i] = config.layers().get(i).height().sample(random);
    ///     totalHeight += layerHeights[i];
    /// }
    ///
    /// if (totalHeight == 0) {
    ///     return false;
    /// }
    ///
    /// BlockPos.MutableBlockPos placePos = context.origin().mutable();
    /// BlockPos.MutableBlockPos nextPos = placePos.mutable().move(config.direction());
    ///
    /// for (int y = 0; y < totalHeight; y++) {
    ///     if (!config.allowedPlacement().test(level, nextPos)) {
    ///         truncate(layerHeights, totalHeight, y, config.prioritizeTip());
    ///         break;
    ///     }
    ///
    ///     nextPos.move(config.direction());
    /// }
    ///
    /// for (int i = 0; i < layerCount; i++) {
    ///     int count = layerHeights[i];
    ///     if (count != 0) {
    ///         BlockColumnConfiguration.Layer layer = config.layers().get(i);
    ///
    ///         for (int y = 0; y < count; y++) {
    ///             level.setBlock(placePos, layer.state().getState(level, random, placePos), Block.UPDATE_CLIENTS);
    ///             placePos.move(config.direction());
    ///         }
    ///     }
    /// }
    ///
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, BlockColumnConfiguration, R>,
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
        let origin = **origin;
        let config = *config;
        let layer_count = config.layers().len();
        let mut layer_heights = vec![0i32; layer_count];
        let mut total_height = 0i32;

        for (i, layer) in config.layers().iter().enumerate() {
            layer_heights[i] = layer.height().sample(random);
            total_height = total_height.wrapping_add(layer_heights[i]);
        }

        if total_height == 0 {
            return false;
        }

        let mut place_pos = origin.mutable();
        // Java: `placePos.mutable().move(config.direction())` copies `placePos`
        // (which stays at the origin) and moves the copy, so the probe cursor
        // starts one step ahead of the write cursor.
        let mut next_pos = place_pos;
        next_pos.move_dir(&config.direction());

        for y in 0..total_height {
            if !config
                .allowed_placement()
                .test(level, &next_pos.immutable())
            {
                truncate(&mut layer_heights, total_height, y, config.prioritize_tip());
                break;
            }

            next_pos.move_dir(&config.direction());
        }

        for (i, layer) in config.layers().iter().enumerate() {
            let count = layer_heights[i];
            if count != 0 {
                for _y in 0..count {
                    let state = block_state_provider_get_state(
                        &**layer.state(),
                        level,
                        random,
                        &place_pos.immutable(),
                    );
                    level.set_block(&place_pos.immutable(), state, UPDATE_CLIENTS);
                    place_pos.move_dir(&config.direction());
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::configurations::block_column_configuration::{
        Layer, only_in_air_predicate,
    };
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use crate::levelgen::feature::{BlockColumnConfiguration, FeatureBehavior};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::core::Direction;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;
    use std::sync::Arc;

    fn place(level: &mut TestLevel, config: &BlockColumnConfiguration) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(1);
        BLOCK_COLUMN.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            config,
        ))
    }

    fn layer(height: i32, name: &str) -> Layer {
        Layer::new(
            IntProvider::Constant(ConstantInt::of(height)),
            Arc::new(simple(BlockState::of(BlockId::from_name(name).unwrap()))),
        )
    }

    /// A column with `ConstantInt(0)` layer heights samples nothing and returns
    /// `false` with no writes (`totalHeight == 0`).
    #[test]
    fn zero_total_height_returns_false() {
        let mut level = TestLevel::over(access());
        let config = BlockColumnConfiguration::new(
            vec![layer(0, "minecraft:stone")],
            Direction::Up,
            crate::levelgen::blockpredicates::always_true(),
            false,
        );
        assert!(!place(&mut level, &config));
        assert!(level.writes.is_empty());
    }

    /// With an always-true predicate the column writes every sampled cell:
    /// layer 0 fills its count from the origin, then layer 1 continues from
    /// where layer 0 stopped (the cursor advances one `direction` per write).
    #[test]
    fn column_writes_every_layer_in_order() {
        let mut level = TestLevel::over(access());
        let config = BlockColumnConfiguration::new(
            vec![layer(2, "minecraft:stone"), layer(1, "minecraft:dirt")],
            Direction::Up,
            crate::levelgen::blockpredicates::always_true(),
            false,
        );
        assert!(place(&mut level, &config));
        assert_eq!(level.writes.len(), 3);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:stone").unwrap()
        );
        assert_eq!(level.writes[1].0, BlockPos::new(0, 1, 0));
        assert_eq!(level.writes[2].0, BlockPos::new(0, 2, 0));
        assert_eq!(
            level.writes[2].1.block(),
            BlockId::from_name("minecraft:dirt").unwrap()
        );
    }

    /// The `allowedPlacement` predicate (here the real `ONLY_IN_AIR_PREDICATE`
    /// over the test level's `get_block_state`) rejects a stone cell partway up
    /// the column: the walk stops and `truncate` removes the excess from the
    /// (single) layer, so only the cells below the rejection are written.
    #[test]
    fn predicate_rejection_truncates_the_column() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 2, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        let config = BlockColumnConfiguration::new(
            vec![layer(4, "minecraft:stone")],
            Direction::Up,
            only_in_air_predicate(),
            false,
        );
        assert!(place(&mut level, &config));
        // totalHeight 4, rejected at newHeight 1 → 3 cells removed from the
        // 4-cell layer, so exactly the origin cell is written.
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
    }

    /// `prioritizeTip` selects which layer absorbs the truncation: with two
    /// 2-cell layers rejected after two cells, `prioritizeTip = false` trims the
    /// *top* layer (dirt) and `prioritizeTip = true` trims the *bottom* (stone)
    /// — both columns write two cells, but from opposite layers.
    #[test]
    fn prioritize_tip_selects_the_truncated_layer() {
        let base = only_in_air_predicate();
        for (prioritize_tip, expected_block) in
            [(false, "minecraft:stone"), (true, "minecraft:dirt")]
        {
            let mut level = TestLevel::over(access());
            level.states.insert(
                BlockPos::new(0, 3, 0),
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            );
            let config = BlockColumnConfiguration::new(
                vec![layer(2, "minecraft:stone"), layer(2, "minecraft:dirt")],
                Direction::Up,
                base.clone(),
                prioritize_tip,
            );
            assert!(place(&mut level, &config));
            assert_eq!(level.writes.len(), 2);
            // Both truncated columns start at the origin; the surviving layer
            // differs by prioritizeTip.
            assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
            assert_eq!(
                level.writes[0].1.block(),
                BlockId::from_name(expected_block).unwrap()
            );
            assert_eq!(level.writes[1].0, BlockPos::new(0, 1, 0));
        }
    }
}
