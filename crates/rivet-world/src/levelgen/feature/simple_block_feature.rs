//! Port of `net.minecraft.world.level.levelgen.feature.SimpleBlockFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.simpleblock`
//! manifest unit.
//!
//! Java: `Feature<SimpleBlockConfiguration>` that resolves
//! `config.toPlace().getOptionalState(level, random, origin)` and — when a
//! state survives — places it at the origin through the block-class-specific
//! write (`DoublePlantBlock.placeAt` for double plants, `MossyCarpetBlock.
//! placeAt` for pale moss carpet, plain `setBlock` otherwise), then schedules a
//! 1-tick update when `config.scheduleTick()`. The `getOptionalState` surface
//! is the `block_state_provider_get_optional_state` dispatch (only
//! `RuleBasedStateProvider` can return `None`, so a state comes back `None`
//! exactly when Java's would).
//!
//! The block-class `instanceof` splits are modelled as an id-identity set for
//! the double-plant subclasses (every block whose registered instance is a
//! `DoublePlantBlock` subclass — built via `DoublePlantBlock::new`,
//! `TallFlowerBlock::new`, `TallSeagrassBlock::new`, `PitcherCropBlock::new`
//! or `SmallDripleafBlock::new` — the `mc.world.level.block` unit owns the
//! classes) and a single-id check for `MossyCarpetBlock`
//! (`minecraft:pale_moss_carpet` is its only registered instance).

use crate::block::blocks::Blocks;
use crate::block::double_plant_block;
use crate::block::mossy_carpet_block;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::SimpleBlockConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider_get_optional_state;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `SimpleBlockFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `stateToPlace.getBlock() instanceof DoublePlantBlock` — the block-identity
/// set of every double-plant block (any block whose registered instance is a
/// `DoublePlantBlock` subclass: tall grass, large fern, pitcher plant and the
/// four tall flowers via `DoublePlantBlock::new`/`TallFlowerBlock::new`, tall
/// seagrass via `TallSeagrassBlock::new`, pitcher crop via
/// `PitcherCropBlock::new` and small dripleaf via `SmallDripleafBlock::new`).
fn is_double_plant(block: BlockId) -> bool {
    const DOUBLE_PLANTS: &[&str] = &[
        "minecraft:tall_grass",
        "minecraft:large_fern",
        "minecraft:pitcher_plant",
        "minecraft:sunflower",
        "minecraft:lilac",
        "minecraft:rose_bush",
        "minecraft:peony",
        "minecraft:tall_seagrass",
        "minecraft:pitcher_crop",
        "minecraft:small_dripleaf",
    ];
    DOUBLE_PLANTS
        .iter()
        .any(|name| BlockId::from_name(name).is_some_and(|id| id == block))
}

/// `net.minecraft.world.level.levelgen.feature.SimpleBlockFeature`.
#[derive(Debug)]
pub struct SimpleBlockFeature;

/// `Feature.SIMPLE_BLOCK` — the registered `minecraft:simple_block` singleton.
pub const SIMPLE_BLOCK: SimpleBlockFeature = SimpleBlockFeature;

impl FeatureBehavior<SimpleBlockConfiguration> for SimpleBlockFeature {
    /// `SimpleBlockFeature.place(FeaturePlaceContext<SimpleBlockConfiguration>)`.
    ///
    /// ```java
    /// SimpleBlockConfiguration config = context.config();
    /// WorldGenLevel level = context.level();
    /// BlockPos origin = context.origin();
    /// BlockState stateToPlace = config.toPlace().getOptionalState(level, context.random(), origin);
    /// if (stateToPlace == null) {
    ///     return false;
    /// }
    ///
    /// if (stateToPlace.canSurvive(level, origin)) {
    ///     if (stateToPlace.getBlock() instanceof DoublePlantBlock) {
    ///         if (!level.isEmptyBlock(origin.above())) {
    ///             return false;
    ///         }
    ///
    ///         DoublePlantBlock.placeAt(level, stateToPlace, origin, Block.UPDATE_CLIENTS);
    ///     } else if (stateToPlace.getBlock() instanceof MossyCarpetBlock) {
    ///         MossyCarpetBlock.placeAt(level, origin, level.getRandom(), Block.UPDATE_CLIENTS);
    ///     } else {
    ///         level.setBlock(origin, stateToPlace, Block.UPDATE_CLIENTS);
    ///     }
    ///
    ///     if (config.scheduleTick()) {
    ///         level.scheduleTick(origin, level.getBlockState(origin).getBlock(), 1);
    ///     }
    ///
    ///     return true;
    /// } else {
    ///     return false;
    /// }
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, SimpleBlockConfiguration, R>,
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
        let state_to_place =
            block_state_provider_get_optional_state(&**config.to_place(), level, random, &origin);
        let Some(state_to_place) = state_to_place else {
            return false;
        };

        if level.can_survive(&state_to_place, &origin) {
            let block = state_to_place.block();
            if is_double_plant(block) {
                if !level.is_empty_block(&origin.above()) {
                    return false;
                }
                double_plant_block::place_at(level, state_to_place, &origin, UPDATE_CLIENTS);
            } else if block == Blocks::PALE_MOSS_CARPET.id() {
                // Java: `MossyCarpetBlock.placeAt(level, origin, level.getRandom(),
                // Block.UPDATE_CLIENTS)` — the topper's `nextBoolean` side draws come from
                // the LEVEL's random source (`WorldGenRegion.getRandom()`, the positional
                // `worldgen_region_random` factory at the chunk centre), never the
                // feature-context `random`. The feature-context RNG must NOT be consumed
                // on this path — so no `random` is threaded into the seam; a future
                // wiring must thread the level RNG through a `get_random` accessor
                // (RivetTodo #232), not this `random`. The seam writes the base carpet
                // (a defined result) and defers the face-negotiated topper to #232.
                mossy_carpet_block::place_at(level, &origin, UPDATE_CLIENTS);
            } else {
                level.set_block(&origin, state_to_place, UPDATE_CLIENTS);
            }

            if config.schedule_tick() {
                let placed = level.get_block_state(&origin);
                level.schedule_block_tick(&origin, crate::block::Block::new(placed.block()), 1);
            }

            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_properties::{BlockStateProperties, DoubleBlockHalf};
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;
    use std::sync::Arc;

    fn place_with(
        level: &mut TestLevel,
        origin: BlockPos,
        to_place: BlockState,
        schedule_tick: bool,
    ) -> bool {
        let config = SimpleBlockConfiguration::new(Arc::new(simple(to_place)), schedule_tick);
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        SIMPLE_BLOCK.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &config,
        ))
    }

    /// A `SimpleStateProvider` never returns `None`, so on a level where
    /// nothing survives the feature returns `false` with no writes.
    #[test]
    fn cannot_survive_returns_false() {
        let mut level = TestLevel::over(access());
        level.survive = false;
        assert!(!place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            false,
        ));
        assert!(level.writes.is_empty());
    }

    /// A non-double, non-carpet block that survives writes the plain `setBlock`
    /// at the origin with `UPDATE_CLIENTS`.
    #[test]
    fn plain_block_writes_at_origin() {
        let mut level = TestLevel::over(access());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), stone, false));
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(level.writes[0].1, stone);
    }

    /// With `scheduleTick`, after the write the feature schedules a 1-tick
    /// update of the placed block at the origin (`getBlockState(origin)` reads
    /// the just-written state back, so the scheduled block is the stone).
    #[test]
    fn schedule_tick_schedules_the_placed_block() {
        let mut level = TestLevel::over(access());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), stone, true));
        assert_eq!(level.writes.len(), 1);
        assert_eq!(
            level.block_ticks,
            vec![(
                BlockPos::new(0, 0, 0),
                crate::block::Block::new(BlockId::from_name("minecraft:stone").unwrap()),
                1,
            )]
        );
    }

    /// Without `scheduleTick` no tick is scheduled.
    #[test]
    fn no_schedule_tick_means_no_tick() {
        let mut level = TestLevel::over(access());
        assert!(place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            false,
        ));
        assert!(level.block_ticks.is_empty());
    }

    /// A double plant (tall grass) requires the cell above empty: with a
    /// non-empty upper cell the feature returns `false` with no write.
    #[test]
    fn double_plant_with_occupied_upper_cell_returns_false() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 1, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        assert!(!place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:tall_grass").unwrap()),
            false,
        ));
        assert!(level.writes.is_empty());
    }

    /// A double plant with an empty upper cell writes both halves via
    /// `DoublePlantBlock.placeAt`: the lower `HALF=lower` then the upper
    /// `HALF=upper`, each with `UPDATE_CLIENTS`. The default `TestLevel` has no
    /// water, so neither copy is waterlogged.
    #[test]
    fn double_plant_writes_lower_then_upper_half() {
        let mut level = TestLevel::over(access());
        assert!(place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:tall_grass").unwrap()),
            false,
        ));
        assert_eq!(level.writes.len(), 2);
        let (lower_pos, lower) = &level.writes[0];
        assert_eq!(*lower_pos, BlockPos::new(0, 0, 0));
        assert_eq!(
            lower.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF),
            Some(PropertyValue::Enum(DoubleBlockHalf::Lower.serialized()))
        );
        let (upper_pos, upper) = &level.writes[1];
        assert_eq!(*upper_pos, BlockPos::new(0, 1, 0));
        assert_eq!(
            upper.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF),
            Some(PropertyValue::Enum(DoubleBlockHalf::Upper.serialized()))
        );
    }

    /// A double plant that carries the `WATERLOGGED` property (small dripleaf
    /// is a waterlogged `DoublePlantBlock`) copies the waterlogged flag from
    /// the cell: the default `TestLevel` reads air (no water), so both halves
    /// are waterlogged `false`.
    #[test]
    fn double_plant_copies_waterlogged_from_the_cell() {
        let mut level = TestLevel::over(access());
        assert!(place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:small_dripleaf").unwrap()),
            false,
        ));
        assert_eq!(level.writes.len(), 2);
        for (_, state) in &level.writes {
            assert_eq!(
                state.get_value(BlockStateProperties::WATERLOGGED),
                Some(PropertyValue::Bool(false))
            );
        }
    }
}
