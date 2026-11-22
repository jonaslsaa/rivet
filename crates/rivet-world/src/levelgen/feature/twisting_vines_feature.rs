//! Port of `net.minecraft.world.level.levelgen.feature.TwistingVinesFeature` (26.2).
//!
//! Scatters `spread_width * spread_width` attempts around `origin`, scanning
//! downward to the first air block above ground, checking that the supporting
//! block is `NETHERRACK`, `WARPED_NYLIUM`, or `WARPED_WART_BLOCK`, and growing
//! upward columns of `TWISTING_VINES_PLANT` capped with an aged `TWISTING_VINES`
//! head (`age` in `17..=25`).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TwistingVinesConfig;
use crate::levelgen::feature::{FeatureBehavior, FeaturePlaceContext};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::{RandomSource, mth};

/// `Block.UPDATE_CLIENTS` (`2`) — write flag used by `TwistingVinesFeature`.
const UPDATE_CLIENTS: u32 = 2;

fn netherrack_id() -> BlockId {
    BlockId::from_name("minecraft:netherrack").expect("minecraft:netherrack exists")
}

fn warped_nylium_id() -> BlockId {
    BlockId::from_name("minecraft:warped_nylium").expect("minecraft:warped_nylium exists")
}

fn warped_wart_block_id() -> BlockId {
    BlockId::from_name("minecraft:warped_wart_block").expect("minecraft:warped_wart_block exists")
}

fn twisting_vines_id() -> BlockId {
    BlockId::from_name("minecraft:twisting_vines").expect("minecraft:twisting_vines exists")
}

fn twisting_vines_plant_id() -> BlockId {
    BlockId::from_name("minecraft:twisting_vines_plant")
        .expect("minecraft:twisting_vines_plant exists")
}

/// `net.minecraft.world.level.levelgen.feature.TwistingVinesFeature`.
#[derive(Debug)]
pub struct TwistingVinesFeature;

/// The registered `minecraft:twisting_vines` (`Feature.TWISTING_VINES`) leaf.
pub const TWISTING_VINES: TwistingVinesFeature = TwistingVinesFeature;

impl FeatureBehavior<TwistingVinesConfig> for TwistingVinesFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, TwistingVinesConfig, R>,
    ) -> bool {
        let origin = *context.origin;
        if is_invalid_placement_location(context.level, &origin) {
            return false;
        }

        let spread_width = context.config.spread_width();
        let spread_height = context.config.spread_height();
        let max_height = context.config.max_height();
        let attempts = spread_width.wrapping_mul(spread_width);

        for _ in 0..attempts {
            let dx = mth::next_int(context.random, spread_width.wrapping_neg(), spread_width);
            let dy = mth::next_int(context.random, spread_height.wrapping_neg(), spread_height);
            let dz = mth::next_int(context.random, spread_width.wrapping_neg(), spread_width);
            let mut place_pos = origin.offset(dx, dy, dz);
            if find_first_air_block_above_ground(context.level, &mut place_pos)
                && !is_invalid_placement_location(context.level, &place_pos)
            {
                let mut vine_height = mth::next_int(context.random, 1, max_height);
                if context.random.next_int_bound(6) == 0 {
                    vine_height = vine_height.wrapping_mul(2);
                }

                if context.random.next_int_bound(5) == 0 {
                    vine_height = 1;
                }

                place_weeping_vines_column(
                    context.level,
                    context.random,
                    &mut place_pos,
                    vine_height,
                    17,
                    25,
                );
            }
        }

        true
    }
}

fn find_first_air_block_above_ground(level: &dyn WorldGenLevel, place_pos: &mut BlockPos) -> bool {
    loop {
        *place_pos = place_pos.offset(0, -1, 0);
        if level.is_outside_build_height_pos(place_pos) {
            return false;
        }
        if !level.get_block_state(place_pos).is_air() {
            break;
        }
    }

    *place_pos = place_pos.offset(0, 1, 0);
    true
}

/// `TwistingVinesFeature.placeWeepingVinesColumn(LevelAccessor, RandomSource,
/// BlockPos.MutableBlockPos, int, int, int)` (preserving Paper's exact method name).
pub fn place_weeping_vines_column<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    place_pos: &mut BlockPos,
    total_height: i32,
    min_age: i32,
    max_age: i32,
) {
    let head_base = BlockState::of(twisting_vines_id());
    let plant_state = BlockState::of(twisting_vines_plant_id());

    for height in 1..=total_height {
        if level.is_empty_block(place_pos) {
            if height == total_height || !level.is_empty_block(&place_pos.above()) {
                let age = mth::next_int(random, min_age, max_age);
                let head = head_base
                    .set_value(BlockStateProperties::AGE_25, age)
                    .expect("twisting_vines has the age property");
                level.set_block(place_pos, head, UPDATE_CLIENTS);
                break;
            }

            level.set_block(place_pos, plant_state, UPDATE_CLIENTS);
        }

        *place_pos = place_pos.relative(&Direction::Up);
    }
}

fn is_invalid_placement_location(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    if !level.is_empty_block(pos) {
        return true;
    }

    let state_below = level.get_block_state(&pos.below()).block();
    state_below != netherrack_id()
        && state_below != warped_nylium_id()
        && state_below != warped_wart_block_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::{FeatureId, feature_place};
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_util::random::LegacyRandomSource;
    use std::collections::HashMap;

    struct MapLevel {
        blocks: HashMap<BlockPos, BlockState>,
        flags: Vec<u32>,
    }

    impl MapLevel {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
                flags: Vec::new(),
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
            self.flags.push(flags);
            true
        }
    }

    struct StubGenerator;
    impl ChunkGenerator for StubGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            128
        }
    }

    #[test]
    fn rejects_invalid_origin_support() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 32, 0);
        level
            .blocks
            .insert(origin.below(), Blocks::STONE.default_block_state());
        let config = TwistingVinesConfig::new(8, 4, 8);
        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(1);

        assert!(!feature_place(
            FeatureId::new(44),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));
    }

    #[test]
    fn grows_twisting_vines_above_warped_nylium() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 32, 0);
        for x in -10..=10 {
            for z in -10..=10 {
                level
                    .blocks
                    .insert(BlockPos::new(x, 31, z), BlockState::of(warped_nylium_id()));
            }
        }

        let config = TwistingVinesConfig::new(4, 2, 8);
        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(777);
        assert!(feature_place(
            FeatureId::new(44),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));

        assert!(level.flags.iter().all(|&f| f == UPDATE_CLIENTS));
        let heads: Vec<BlockState> = level
            .blocks
            .values()
            .copied()
            .filter(|s| s.block() == twisting_vines_id())
            .collect();
        assert!(!heads.is_empty());
        for head in heads {
            let Some(PropertyValue::Int(age)) = head.get_value(BlockStateProperties::AGE_25) else {
                panic!("expected integer AGE_25 property");
            };
            assert!((17..=25).contains(&age));
        }
    }

    #[test]
    fn column_stops_below_ceiling_block() {
        let mut level = MapLevel::new();
        level.blocks.insert(
            BlockPos::new(0, 35, 0),
            Blocks::NETHERRACK.default_block_state(),
        );

        let mut pos = BlockPos::new(0, 32, 0);
        let mut random = LegacyRandomSource::new(5);
        place_weeping_vines_column(&mut level, &mut random, &mut pos, 8, 17, 25);

        assert_eq!(
            level.get_block_state(&BlockPos::new(0, 32, 0)).block(),
            twisting_vines_plant_id()
        );
        assert_eq!(
            level.get_block_state(&BlockPos::new(0, 33, 0)).block(),
            twisting_vines_plant_id()
        );
        assert_eq!(
            level.get_block_state(&BlockPos::new(0, 34, 0)).block(),
            twisting_vines_id()
        );
    }
}
