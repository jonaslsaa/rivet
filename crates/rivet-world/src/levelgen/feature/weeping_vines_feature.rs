//! Port of `net.minecraft.world.level.levelgen.feature.WeepingVinesFeature` (26.2).
//!
//! Places a hanging nether wart roof cluster (200 offset attempts) and up to
//! 100 weeping vine columns below `NETHERRACK` or `NETHER_WART_BLOCK` ceiling
//! blocks, matching Paper's exact RNG draw sequence, bounds, and `UPDATE_CLIENTS`
//! write flags.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::{FeatureBehavior, FeaturePlaceContext};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::{RandomSource, mth};

/// `Block.UPDATE_CLIENTS` (`2`) — write flag used by `WeepingVinesFeature`.
const UPDATE_CLIENTS: u32 = 2;

fn netherrack_id() -> BlockId {
    BlockId::from_name("minecraft:netherrack").expect("minecraft:netherrack exists")
}

fn nether_wart_block_id() -> BlockId {
    BlockId::from_name("minecraft:nether_wart_block").expect("minecraft:nether_wart_block exists")
}

fn weeping_vines_id() -> BlockId {
    BlockId::from_name("minecraft:weeping_vines").expect("minecraft:weeping_vines exists")
}

fn weeping_vines_plant_id() -> BlockId {
    BlockId::from_name("minecraft:weeping_vines_plant")
        .expect("minecraft:weeping_vines_plant exists")
}

/// `net.minecraft.world.level.levelgen.feature.WeepingVinesFeature`.
#[derive(Debug)]
pub struct WeepingVinesFeature;

/// The registered `minecraft:weeping_vines` (`Feature.WEEPING_VINES`) leaf.
pub const WEEPING_VINES: WeepingVinesFeature = WeepingVinesFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for WeepingVinesFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let origin = *context.origin;
        if !context.level.is_empty_block(&origin) {
            return false;
        }

        let state_above = context.level.get_block_state(&origin.above());
        if state_above.block() != netherrack_id() && state_above.block() != nether_wart_block_id() {
            return false;
        }

        place_roof_nether_wart(context.level, context.random, &origin);
        place_roof_weeping_vines(context.level, context.random, &origin);
        true
    }
}

fn place_roof_nether_wart<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
) {
    let netherrack = netherrack_id();
    let wart_id = nether_wart_block_id();
    let wart_state = BlockState::of(wart_id);
    level.set_block(origin, wart_state, UPDATE_CLIENTS);

    for _ in 0..200 {
        let dx = random
            .next_int_bound(6)
            .wrapping_sub(random.next_int_bound(6));
        let dy = random
            .next_int_bound(2)
            .wrapping_sub(random.next_int_bound(5));
        let dz = random
            .next_int_bound(6)
            .wrapping_sub(random.next_int_bound(6));
        let place_pos = origin.offset(dx, dy, dz);
        if level.is_empty_block(&place_pos) {
            let mut neighbours = 0i32;

            for direction in Direction::VALUES {
                let neighbour_state = level.get_block_state(&place_pos.relative(&direction));
                if neighbour_state.block() == netherrack || neighbour_state.block() == wart_id {
                    neighbours = neighbours.wrapping_add(1);
                }

                if neighbours > 1 {
                    break;
                }
            }

            if neighbours == 1 {
                level.set_block(&place_pos, wart_state, UPDATE_CLIENTS);
            }
        }
    }
}

fn place_roof_weeping_vines<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
) {
    let netherrack = netherrack_id();
    let wart_id = nether_wart_block_id();

    for _ in 0..100 {
        let dx = random
            .next_int_bound(8)
            .wrapping_sub(random.next_int_bound(8));
        let dy = random
            .next_int_bound(2)
            .wrapping_sub(random.next_int_bound(7));
        let dz = random
            .next_int_bound(8)
            .wrapping_sub(random.next_int_bound(8));
        let mut place_pos = origin.offset(dx, dy, dz);
        if level.is_empty_block(&place_pos) {
            let state_above = level.get_block_state(&place_pos.above());
            if state_above.block() == netherrack || state_above.block() == wart_id {
                let mut vine_height = mth::next_int(random, 1, 8);
                if random.next_int_bound(6) == 0 {
                    vine_height = vine_height.wrapping_mul(2);
                }

                if random.next_int_bound(5) == 0 {
                    vine_height = 1;
                }

                place_weeping_vines_column(level, random, &mut place_pos, vine_height, 17, 25);
            }
        }
    }
}

/// `WeepingVinesFeature.placeWeepingVinesColumn(LevelAccessor, RandomSource,
/// BlockPos.MutableBlockPos, int, int, int)`.
///
/// Public because `HugeFungusFeature.tryPlaceWeepingVines` calls it directly.
pub fn place_weeping_vines_column<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    place_pos: &mut BlockPos,
    total_height: i32,
    min_age: i32,
    max_age: i32,
) {
    let head_base = BlockState::of(weeping_vines_id());
    let plant_state = BlockState::of(weeping_vines_plant_id());

    for height in 0..=total_height {
        if level.is_empty_block(place_pos) {
            if height == total_height || !level.is_empty_block(&place_pos.below()) {
                let age = mth::next_int(random, min_age, max_age);
                let head = head_base
                    .set_value(BlockStateProperties::AGE_25, age)
                    .expect("weeping_vines has the age property");
                level.set_block(place_pos, head, UPDATE_CLIENTS);
                break;
            }

            level.set_block(place_pos, plant_state, UPDATE_CLIENTS);
        }

        *place_pos = place_pos.relative(&Direction::Down);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::configurations::NONE;
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
    fn rejects_non_empty_origin_or_invalid_ceiling() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 64, 0);
        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(1);

        // Air above -> invalid ceiling.
        assert!(!feature_place(
            FeatureId::new(43),
            &NONE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));

        // Stone above -> still invalid ceiling.
        level
            .blocks
            .insert(origin.above(), Blocks::STONE.default_block_state());
        assert!(!feature_place(
            FeatureId::new(43),
            &NONE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));

        // Non-empty origin with valid ceiling -> rejected.
        level
            .blocks
            .insert(origin.above(), Blocks::NETHERRACK.default_block_state());
        level
            .blocks
            .insert(origin, Blocks::NETHERRACK.default_block_state());
        assert!(!feature_place(
            FeatureId::new(43),
            &NONE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));
    }

    #[test]
    fn places_roof_nether_wart_and_weeping_vines_with_update_clients() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 64, 0);
        for x in -10..=10 {
            for z in -10..=10 {
                level.blocks.insert(
                    BlockPos::new(x, 65, z),
                    Blocks::NETHERRACK.default_block_state(),
                );
            }
        }

        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(12345);
        assert!(feature_place(
            FeatureId::new(43),
            &NONE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        ));

        assert_eq!(
            level.get_block_state(&origin).block(),
            nether_wart_block_id()
        );
        assert!(level.flags.iter().all(|&f| f == UPDATE_CLIENTS));
        let has_vine_head = level
            .blocks
            .values()
            .any(|s| s.block() == weeping_vines_id());
        assert!(has_vine_head, "expected at least one weeping vine head");
    }

    #[test]
    fn weeping_vines_column_stops_above_solid_block_with_age_in_range() {
        let mut level = MapLevel::new();
        let floor = BlockPos::new(0, 60, 0);
        level
            .blocks
            .insert(floor, Blocks::NETHERRACK.default_block_state());

        let mut pos = BlockPos::new(0, 63, 0);
        let mut random = LegacyRandomSource::new(99);
        place_weeping_vines_column(&mut level, &mut random, &mut pos, 8, 23, 25);

        assert_eq!(
            level.get_block_state(&BlockPos::new(0, 63, 0)).block(),
            weeping_vines_plant_id()
        );
        assert_eq!(
            level.get_block_state(&BlockPos::new(0, 62, 0)).block(),
            weeping_vines_plant_id()
        );
        let head = level.get_block_state(&BlockPos::new(0, 61, 0));
        assert_eq!(head.block(), weeping_vines_id());
        let Some(PropertyValue::Int(age)) = head.get_value(BlockStateProperties::AGE_25) else {
            panic!("expected integer AGE_25 property");
        };
        assert!((23..=25).contains(&age));
    }
}
