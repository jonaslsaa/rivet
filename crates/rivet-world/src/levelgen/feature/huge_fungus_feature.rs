//! Port of `net.minecraft.world.level.levelgen.feature.HugeFungusFeature` (26.2).
//!
//! Generates crimson and warped huge fungi (both worldgen and sapling-planted)
//! with Paper's exact height sampling, huge-stem roll, stem/hat geometry,
//! shroomlight decor chances, weeping-vine hanging checks, and block update
//! flags (`Block.UPDATE_NONE` = `260`, `Block.UPDATE_ALL` = `3`).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::huge_fungus_configuration::HugeFungusConfiguration;
use crate::levelgen::feature::weeping_vines_feature::place_weeping_vines_column;
use crate::levelgen::feature::{FeatureBehavior, FeaturePlaceContext};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::{RandomSource, mth};

/// `Block.UPDATE_ALL` (`UPDATE_NEIGHBORS | UPDATE_CLIENTS` = `3`).
const UPDATE_ALL: u32 = 3;

/// `Block.UPDATE_NONE` (`UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` = `260`).
const UPDATE_NONE: u32 = 260;

const HUGE_PROBABILITY: f32 = 0.06;

fn nether_wart_block_id() -> BlockId {
    BlockId::from_name("minecraft:nether_wart_block").expect("minecraft:nether_wart_block exists")
}

/// `net.minecraft.world.level.levelgen.feature.HugeFungusFeature`.
#[derive(Debug)]
pub struct HugeFungusFeature;

/// The registered `minecraft:huge_fungus` (`Feature.HUGE_FUNGUS`) leaf.
pub const HUGE_FUNGUS: HugeFungusFeature = HugeFungusFeature;

impl FeatureBehavior<HugeFungusConfiguration> for HugeFungusFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, HugeFungusConfiguration, R>,
    ) -> bool {
        let origin = *context.origin;
        let config = context.config;
        let valid_base_block = config.valid_base_state.block();
        let below_state = context.level.get_block_state(&origin.below());
        if below_state.block() != valid_base_block {
            return false;
        }

        let mut total_height = mth::next_int(context.random, 4, 13);
        if context.random.next_int_bound(12) == 0 {
            total_height = total_height.wrapping_mul(2);
        }

        if !config.planted {
            let max_generation_y = context.chunk_generator.get_gen_depth();
            if origin.get_y().wrapping_add(total_height).wrapping_add(1) >= max_generation_y {
                return false;
            }
        }

        let is_huge = !config.planted && context.random.next_float() < HUGE_PROBABILITY;
        context
            .level
            .set_block(&origin, Blocks::AIR.default_block_state(), UPDATE_NONE);
        place_stem(
            context.level,
            context.random,
            config,
            &origin,
            total_height,
            is_huge,
        );
        place_hat(
            context.level,
            context.random,
            config,
            &origin,
            total_height,
            is_huge,
        );
        true
    }
}

fn is_replaceable(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    config: &HugeFungusConfiguration,
    check_config: bool,
) -> bool {
    if level.get_block_state(pos).can_be_replaced() {
        true
    } else {
        check_config && config.replaceable_blocks.test(level, pos)
    }
}

fn place_stem<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    config: &HugeFungusConfiguration,
    surface_origin: &BlockPos,
    total_height: i32,
    is_huge: bool,
) {
    let block_state = config.stem_state;
    let radius: i32 = if is_huge { 1 } else { 0 };

    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let corner_of_huge_stem = is_huge && dx.abs() == radius && dz.abs() == radius;

            for dy in 0..total_height {
                let block_pos = surface_origin.offset(dx, dy, dz);
                if is_replaceable(level, &block_pos, config, true) {
                    if config.planted {
                        if !level.get_block_state(&block_pos.below()).is_air() {
                            level.set_block(
                                &block_pos,
                                Blocks::AIR.default_block_state(),
                                UPDATE_ALL,
                            );
                        }

                        level.set_block(&block_pos, block_state, UPDATE_ALL);
                    } else if corner_of_huge_stem {
                        if random.next_float() < 0.1 {
                            level.set_block(&block_pos, block_state, UPDATE_ALL);
                        }
                    } else {
                        level.set_block(&block_pos, block_state, UPDATE_ALL);
                    }
                }
            }
        }
    }
}

fn place_hat<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    config: &HugeFungusConfiguration,
    surface_origin: &BlockPos,
    total_height: i32,
    is_huge: bool,
) {
    let is_crimson = config.hat_state.block() == nether_wart_block_id();
    let hat_height = (random
        .next_int_bound(1i32.wrapping_add(total_height / 3))
        .wrapping_add(5))
    .min(total_height);
    let hat_start_y = total_height.wrapping_sub(hat_height);

    for dy in hat_start_y..=total_height {
        let mut radius: i32 = if dy < total_height.wrapping_sub(random.next_int_bound(3)) {
            2
        } else {
            1
        };
        if hat_height > 8 && dy < hat_start_y.wrapping_add(4) {
            radius = 3;
        }

        if is_huge {
            radius = radius.wrapping_add(1);
        }

        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let is_edge_x = dx == -radius || dx == radius;
                let is_edge_z = dz == -radius || dz == radius;
                let inside = !is_edge_x && !is_edge_z && dy != total_height;
                let corner = is_edge_x && is_edge_z;
                let is_hat_bottom = dy < hat_start_y.wrapping_add(3);
                let block_pos = surface_origin.offset(dx, dy, dz);
                if is_replaceable(level, &block_pos, config, false) {
                    if config.planted && !level.get_block_state(&block_pos.below()).is_air() {
                        level.set_block(&block_pos, Blocks::AIR.default_block_state(), UPDATE_ALL);
                    }

                    if is_hat_bottom {
                        if !inside {
                            place_hat_drop_block(
                                level,
                                random,
                                &block_pos,
                                config.hat_state,
                                is_crimson,
                            );
                        }
                    } else if inside {
                        place_hat_block(
                            level,
                            random,
                            config,
                            &block_pos,
                            0.1,
                            0.2,
                            if is_crimson { 0.1 } else { 0.0 },
                        );
                    } else if corner {
                        place_hat_block(
                            level,
                            random,
                            config,
                            &block_pos,
                            0.01,
                            0.7,
                            if is_crimson { 0.083 } else { 0.0 },
                        );
                    } else {
                        place_hat_block(
                            level,
                            random,
                            config,
                            &block_pos,
                            5.0e-4,
                            0.98,
                            if is_crimson { 0.07 } else { 0.0 },
                        );
                    }
                }
            }
        }
    }
}

fn place_hat_block<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    config: &HugeFungusConfiguration,
    block_pos: &BlockPos,
    decor_block_probability: f32,
    hat_block_probability: f32,
    weeping_vine_probability: f32,
) {
    if random.next_float() < decor_block_probability {
        level.set_block(block_pos, config.decor_state, UPDATE_ALL);
    } else if random.next_float() < hat_block_probability {
        level.set_block(block_pos, config.hat_state, UPDATE_ALL);
        if random.next_float() < weeping_vine_probability {
            try_place_weeping_vines(block_pos, level, random);
        }
    }
}

fn place_hat_drop_block<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    block_pos: &BlockPos,
    hat_state: BlockState,
    is_crimson: bool,
) {
    if level.get_block_state(&block_pos.below()).block() == hat_state.block() {
        level.set_block(block_pos, hat_state, UPDATE_ALL);
    } else if (random.next_float() as f64) < 0.15 {
        level.set_block(block_pos, hat_state, UPDATE_ALL);
        if is_crimson && random.next_int_bound(11) == 0 {
            try_place_weeping_vines(block_pos, level, random);
        }
    }
}

fn try_place_weeping_vines<R: RandomSource>(
    pos: &BlockPos,
    level: &mut dyn WorldGenLevel,
    random: &mut R,
) {
    let mut place_pos = pos.relative(&Direction::Down);
    if level.is_empty_block(&place_pos) {
        let mut height = mth::next_int(random, 1, 5);
        if random.next_int_bound(7) == 0 {
            height = height.wrapping_mul(2);
        }

        place_weeping_vines_column(level, random, &mut place_pos, height, 23, 25);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::block_predicate::always_true;
    use crate::levelgen::feature::{FeatureId, feature_place};
    use rivet_util::random::LegacyRandomSource;
    use std::collections::HashMap;

    struct MapLevel {
        blocks: HashMap<BlockPos, BlockState>,
        writes: Vec<(BlockPos, BlockState, u32)>,
    }

    impl MapLevel {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
                writes: Vec::new(),
            }
        }
    }

    impl LevelHeightAccessor for MapLevel {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_height(&self) -> i32 {
            128
        }
    }

    impl WorldGenLevel for MapLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.blocks
                .get(pos)
                .copied()
                .unwrap_or_else(|| Blocks::AIR.default_block_state())
        }

        fn is_empty_block(&self, pos: &BlockPos) -> bool {
            self.get_block_state(pos).is_air()
        }

        fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
            self.blocks.insert(*pos, state);
            self.writes.push((*pos, state, flags));
            true
        }
    }

    struct StubGenerator(i32);
    impl ChunkGenerator for StubGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            self.0
        }
    }

    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).expect("block exists"))
    }

    fn crimson_config(planted: bool) -> HugeFungusConfiguration {
        HugeFungusConfiguration::new(
            state("minecraft:crimson_nylium"),
            state("minecraft:crimson_stem"),
            state("minecraft:nether_wart_block"),
            state("minecraft:shroomlight"),
            always_true(),
            planted,
        )
    }

    #[test]
    fn rejects_invalid_base_block() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 32, 0);
        level
            .blocks
            .insert(origin.below(), Blocks::NETHERRACK.default_block_state());
        let config = crimson_config(false);
        let generator = StubGenerator(128);
        let mut random = LegacyRandomSource::new(1);

        assert!(!feature_place(
            FeatureId::new(41),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));
        assert!(level.writes.is_empty());
    }

    #[test]
    fn non_planted_rejects_when_exceeding_gen_depth() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 125, 0);
        level
            .blocks
            .insert(origin.below(), state("minecraft:crimson_nylium"));
        let config = crimson_config(false);
        let generator = StubGenerator(128);
        let mut random = LegacyRandomSource::new(1);

        assert!(!feature_place(
            FeatureId::new(41),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));
    }

    #[test]
    fn places_crimson_stem_and_hat_clearing_origin_with_update_none() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 32, 0);
        level
            .blocks
            .insert(origin.below(), state("minecraft:crimson_nylium"));
        let config = crimson_config(false);
        let generator = StubGenerator(128);
        let mut random = LegacyRandomSource::new(42);

        assert!(feature_place(
            FeatureId::new(41),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));

        // First write clears origin to AIR with UPDATE_NONE (260).
        assert_eq!(
            level.writes[0],
            (origin, Blocks::AIR.default_block_state(), UPDATE_NONE)
        );
        // Stem is placed at origin.
        assert_eq!(
            level.get_block_state(&origin).block(),
            BlockId::from_name("minecraft:crimson_stem").unwrap()
        );
        // Hat blocks are placed.
        assert!(
            level
                .blocks
                .values()
                .any(|s| s.block() == nether_wart_block_id())
        );
    }
}
