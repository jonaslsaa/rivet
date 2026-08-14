//! Port of `net.minecraft.world.level.levelgen.feature.NetherForestVegetationFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.vegetation`
//! manifest unit (issue #600).
//!
//! Java: `Feature<NetherForestVegetationConfig>` whose `place` gates on the
//! cell below the origin being a `#minecraft:nylium` block and the origin
//! `y` within `[getMinY() + 1, getMaxY() - 1]`; it then attempts
//! `spreadWidth * spreadWidth` placements, each offset by
//! `nextInt(spreadWidth) - nextInt(spreadWidth)` on x and z and
//! `nextInt(spreadHeight) - nextInt(spreadHeight)` on y, drawing a state from
//! `config.stateProvider` and writing it (with `Block.UPDATE_CLIENTS`, 2) when
//! the cell is empty, above `getMinY()`, and the state survives. Returns
//! `true` iff at least one placement wrote.
//!
//! The `stateProvider.getState` call dispatches through the
//! `block_state_provider_get_state` hub (the `#181` dispatch surface); the
//! nylium membership reads the block-tag table through `BlockState::is_in_tag`
//! (unknown tags read as empty, matching Paper's `is(TagKey)` on an unbound
//! registry).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NetherForestVegetationConfig;
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_get_state;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.NetherForestVegetationFeature`.
#[derive(Debug)]
pub struct NetherForestVegetationFeature;

/// `Feature.NETHER_FOREST_VEGETATION` — the registered
/// `minecraft:nether_forest_vegetation` singleton.
pub const NETHER_FOREST_VEGETATION: NetherForestVegetationFeature = NetherForestVegetationFeature;

impl FeatureBehavior<NetherForestVegetationConfig> for NetherForestVegetationFeature {
    /// `NetherForestVegetationFeature.place(FeaturePlaceContext<NetherForestVegetationConfig>)`.
    ///
    /// ```java
    /// BlockState belowState = level.getBlockState(origin.below());
    /// if (!belowState.is(BlockTags.NYLIUM)) return false;
    /// int y = origin.getY();
    /// if (y >= level.getMinY() + 1 && y + 1 <= level.getMaxY()) {
    ///     int placed = 0;
    ///     for (int i = 0; i < config.spreadWidth * config.spreadWidth; i++) {
    ///         BlockPos finalPos = origin.offset(
    ///             random.nextInt(config.spreadWidth) - random.nextInt(config.spreadWidth),
    ///             random.nextInt(config.spreadHeight) - random.nextInt(config.spreadHeight),
    ///             random.nextInt(config.spreadWidth) - random.nextInt(config.spreadWidth));
    ///         BlockState state = config.stateProvider.getState(level, random, finalPos);
    ///         if (level.isEmptyBlock(finalPos) && finalPos.getY() > level.getMinY()
    ///                 && state.canSurvive(level, finalPos)) {
    ///             level.setBlock(finalPos, state, Block.UPDATE_CLIENTS);
    ///             placed++;
    ///         }
    ///     }
    ///     return placed > 0;
    /// } else {
    ///     return false;
    /// }
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NetherForestVegetationConfig, R>,
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
        let origin = *origin;
        let config = *config;
        let below_state = level.get_block_state(&origin.below());
        if !below_state.is_in_tag("minecraft:nylium") {
            return false;
        }
        let y = origin.get_y();
        if y >= level.get_min_y().wrapping_add(1) && y.wrapping_add(1) <= level.get_max_y() {
            let mut placed: i32 = 0;
            let attempts = config.spread_width.wrapping_mul(config.spread_width);
            for _ in 0..attempts {
                let final_pos = origin.offset(
                    random
                        .next_int_bound(config.spread_width)
                        .wrapping_sub(random.next_int_bound(config.spread_width)),
                    random
                        .next_int_bound(config.spread_height)
                        .wrapping_sub(random.next_int_bound(config.spread_height)),
                    random
                        .next_int_bound(config.spread_width)
                        .wrapping_sub(random.next_int_bound(config.spread_width)),
                );
                let state = block_state_provider_get_state(
                    config.state_provider().as_ref(),
                    level,
                    random,
                    &final_pos,
                );
                if level.is_empty_block(&final_pos)
                    && final_pos.get_y() > level.get_min_y()
                    && level.can_survive(&state, &final_pos)
                {
                    level.set_block(&final_pos, state, UPDATE_CLIENTS);
                    placed = placed.wrapping_add(1);
                }
            }
            placed > 0
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::simple;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use std::sync::Arc;

    fn config() -> NetherForestVegetationConfig {
        NetherForestVegetationConfig::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:warped_roots").unwrap(),
            ))),
            3,
            2,
        )
    }

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 60, 0);
        NETHER_FOREST_VEGETATION.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// A non-nylium block below returns `false` before any draw.
    #[test]
    fn non_nylium_below_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// Nylium below + a writable in-range cell: 9 attempts (`3*3`) each draw
    /// `[IntBound(3), IntBound(3), IntBound(2), IntBound(2), IntBound(3),
    /// IntBound(3)]` — each `nextInt - nextInt` offset draws the bound twice
    /// (x, y, z in Java's argument order), and the write lands.
    #[test]
    fn nylium_below_places_some_attempts() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 59, 0),
            BlockState::of(BlockId::from_name("minecraft:crimson_nylium").unwrap()),
        );
        let mut random = RecordingRandom::new(4);
        assert!(place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            [
                RngCall::IntBound(3),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
                RngCall::IntBound(2),
                RngCall::IntBound(3),
                RngCall::IntBound(3),
            ]
            .repeat(9)
        );
        assert!(!level.writes.is_empty());
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:warped_roots").unwrap()
        );
    }

    /// An origin outside the feature's build-height window returns `false` even
    /// with nylium below (the `y` window check short-circuits before any draw).
    ///
    /// TestGenerator is minY=-64/depth=384, so the feature window is
    /// [minY+1, maxY-1] = [-63, 318]. Origin y=-64 sits at minY, one below the
    /// window's lower bound: nylium below (at y=-65) passes the gate first, then
    /// the window check fails and returns `false` with no draws.
    #[test]
    fn out_of_build_height_window_returns_false() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -65, 0),
            BlockState::of(BlockId::from_name("minecraft:crimson_nylium").unwrap()),
        );
        let generator = TestGenerator;
        let origin = BlockPos::new(0, -64, 0);
        let mut random = RecordingRandom::new(4);
        let placed = NETHER_FOREST_VEGETATION.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config(),
        ));
        assert!(!placed);
        assert!(random.calls.is_empty());
    }
}
