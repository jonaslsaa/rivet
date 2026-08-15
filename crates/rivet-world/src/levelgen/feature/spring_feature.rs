//! Port of `net.minecraft.world.level.levelgen.feature.SpringFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.spring` manifest unit.
//!
//! Java: `Feature<SpringConfiguration>` that places a fluid spring. `place`
//! first gates on the block above (and, when `requiresBlockBelow`, the block
//! below) being in `config.validBlocks`, and the origin cell itself being air
//! or in `validBlocks`. It then counts the five orthogonal neighbours
//! (west/east/north/south/below) whose state is in `validBlocks` (rockCount)
//! and the same five whose state is empty (holeCount); only when both counts
//! match the config's `rockCount`/`holeCount` does it write the fluid's legacy
//! block (`Block.UPDATE_CLIENTS`) and schedule the fluid to flow
//! (`scheduleTick` with delay 0). Returns `placed > 0`.
//!
//! The `state.is(HolderSet)` test becomes `set.contains_id(state.block().id())`
//! (the `MatchingBlocksPredicate`/`WorldCarver` mapping); the fluid's legacy
//! block and type come from the `FluidState` stub in the
//! `spring_configuration` unit. All world reads/writes go through the
//! `WorldGenLevel` seams (RivetTodo #232); the test double overrides them.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::SpringConfiguration;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `SpringFeature.place`
/// passes to `level.setBlock` directly (Java `Block.UPDATE_CLIENTS`), in
/// contrast to `Feature.setBlock`'s `Block.UPDATE_ALL`.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.SpringFeature`.
#[derive(Debug)]
pub struct SpringFeature;

/// `Feature.SPRING` — the registered `minecraft:spring_feature` singleton.
pub const SPRING: SpringFeature = SpringFeature;

/// `BlockStateBase.is(HolderSet<Block>)` — the `state.is(config.validBlocks)`
/// gate/count test (`set.contains_id(state.block().id())`).
fn state_is_in(
    state: &rivet_registry::block_state::BlockState,
    set: &rivet_registry::holder_set::HolderSet<rivet_registry::registries::BlockType>,
) -> bool {
    set.contains_id(state.block().id() as u32)
}

impl FeatureBehavior<SpringConfiguration> for SpringFeature {
    /// `SpringFeature.place(FeaturePlaceContext<SpringConfiguration>)`.
    ///
    /// ```java
    /// if (!level.getBlockState(origin.above()).is(config.validBlocks)) return false;
    /// if (config.requiresBlockBelow && !level.getBlockState(origin.below()).is(config.validBlocks)) return false;
    /// BlockState currentState = level.getBlockState(origin);
    /// if (!currentState.isAir() && !currentState.is(config.validBlocks)) return false;
    /// int placed = 0;
    /// int rockCount = 0;
    /// if (level.getBlockState(origin.west()).is(config.validBlocks)) rockCount++;
    /// if (level.getBlockState(origin.east()).is(config.validBlocks)) rockCount++;
    /// if (level.getBlockState(origin.north()).is(config.validBlocks)) rockCount++;
    /// if (level.getBlockState(origin.south()).is(config.validBlocks)) rockCount++;
    /// if (level.getBlockState(origin.below()).is(config.validBlocks)) rockCount++;
    /// int holeCount = 0;
    /// if (level.isEmptyBlock(origin.west())) holeCount++;
    /// if (level.isEmptyBlock(origin.east())) holeCount++;
    /// if (level.isEmptyBlock(origin.north())) holeCount++;
    /// if (level.isEmptyBlock(origin.south())) holeCount++;
    /// if (level.isEmptyBlock(origin.below())) holeCount++;
    /// if (rockCount == config.rockCount && holeCount == config.holeCount) {
    ///     level.setBlock(origin, config.state.createLegacyBlock(), Block.UPDATE_CLIENTS);
    ///     level.scheduleTick(origin, config.state.getType(), 0);
    ///     placed++;
    /// }
    /// return placed > 0;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, SpringConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let config = *config;
        let origin = *origin;
        if !state_is_in(
            &level.get_block_state(&origin.above()),
            &config.valid_blocks,
        ) {
            return false;
        }
        if config.requires_block_below
            && !state_is_in(
                &level.get_block_state(&origin.below()),
                &config.valid_blocks,
            )
        {
            return false;
        }
        let current_state = level.get_block_state(origin);
        if !current_state.is_air() && !state_is_in(&current_state, &config.valid_blocks) {
            return false;
        }
        let mut rock_count = 0;
        if state_is_in(&level.get_block_state(&origin.west()), &config.valid_blocks) {
            rock_count += 1;
        }
        if state_is_in(&level.get_block_state(&origin.east()), &config.valid_blocks) {
            rock_count += 1;
        }
        if state_is_in(
            &level.get_block_state(&origin.north()),
            &config.valid_blocks,
        ) {
            rock_count += 1;
        }
        if state_is_in(
            &level.get_block_state(&origin.south()),
            &config.valid_blocks,
        ) {
            rock_count += 1;
        }
        if state_is_in(
            &level.get_block_state(&origin.below()),
            &config.valid_blocks,
        ) {
            rock_count += 1;
        }
        let mut hole_count = 0;
        if level.is_empty_block(&origin.west()) {
            hole_count += 1;
        }
        if level.is_empty_block(&origin.east()) {
            hole_count += 1;
        }
        if level.is_empty_block(&origin.north()) {
            hole_count += 1;
        }
        if level.is_empty_block(&origin.south()) {
            hole_count += 1;
        }
        if level.is_empty_block(&origin.below()) {
            hole_count += 1;
        }
        let mut placed = 0;
        if rock_count == config.rock_count && hole_count == config.hole_count {
            level.set_block(origin, config.state.create_legacy_block(), UPDATE_CLIENTS);
            level.schedule_tick(origin, config.state.get_type(), 0);
            placed += 1;
        }
        placed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::configurations::spring_configuration::FluidState;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::RegistryId;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::fluid_id::FluidId;
    use rivet_registry::holder::Holder;
    use rivet_registry::holder_set::HolderSet;
    use rivet_registry::registries::BlockType;
    use rivet_util::random::LegacyRandomSource;

    /// The test `validBlocks` (stone + netherrack) as registry-reference
    /// holders over the real raw ids. `state.is(validBlocks)` maps to
    /// `set.contains_id(state.block().id())`, which matches `Reference`
    /// members by element id (the matching-registry contract in
    /// `holder_set.rs`).
    fn valid_blocks() -> HolderSet<BlockType> {
        HolderSet::direct(vec![
            Holder::reference(RegistryId(0), Blocks::STONE.id().0 as u32),
            Holder::reference(RegistryId(0), Blocks::NETHERRACK.id().0 as u32),
        ])
    }

    fn config() -> SpringConfiguration {
        SpringConfiguration::new(FluidState::new(FluidId::WATER), true, 4, 1, valid_blocks())
    }

    fn place_with(level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        SPRING.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &config(),
        ))
    }

    fn stone() -> BlockState {
        BlockState::of(Blocks::STONE.id())
    }

    /// A spring at the origin with stone above and below (the gates), the
    /// origin air, and all five rock neighbours stone with no empty hole
    /// neighbours: `rockCount = 5 == 5` fails (`config.rockCount = 4`), so no
    /// write — the exact-count gate is load-bearing.
    #[test]
    fn exact_rock_count_gate_blocks_when_counts_mismatch() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        level.states.insert(origin.above(), stone());
        level.states.insert(origin.below(), stone());
        level.states.insert(origin.west(), stone());
        level.states.insert(origin.east(), stone());
        level.states.insert(origin.north(), stone());
        level.states.insert(origin.south(), stone());
        assert!(!place_with(&mut level, origin));
        assert!(level.writes.is_empty());
        assert!(level.ticks.is_empty());
    }

    /// With `rockCount = 4` and `holeCount = 1` matching the default config:
    /// four stone rock neighbours (east/north/south/below) and one empty hole
    /// (west), the origin air, stone above/below — the spring writes the water
    /// legacy block and schedules the water fluid tick.
    #[test]
    fn spring_places_water_and_schedules_tick() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        level.states.insert(origin.above(), stone());
        level.states.insert(origin.below(), stone());
        // Rock neighbours (in `valid_blocks`): east, north, south, below = 4.
        level.states.insert(origin.east(), stone());
        level.states.insert(origin.north(), stone());
        level.states.insert(origin.south(), stone());
        level.states.insert(origin.below(), stone());
        // Hole neighbours: west is air (the default) = 1.
        assert!(place_with(&mut level, origin));
        assert_eq!(
            level.writes,
            vec![(origin, BlockState::of(Blocks::WATER.id()))]
        );
        assert_eq!(level.ticks, vec![(origin, FluidId::WATER, 0)]);
    }

    /// The `requiresBlockBelow` gate: with the below block NOT in
    /// `validBlocks`, the spring returns false before any count.
    #[test]
    fn requires_block_below_gate() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        level.states.insert(origin.above(), stone());
        level
            .states
            .insert(origin.below(), BlockState::of(Blocks::DIRT.id()));
        assert!(!place_with(&mut level, origin));
        assert!(level.writes.is_empty());
    }

    /// The `currentState` gate: a non-air origin not in `validBlocks` returns
    /// false (Java `!currentState.isAir() && !currentState.is(validBlocks)`).
    #[test]
    fn non_valid_origin_state_is_rejected() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        level.states.insert(origin.above(), stone());
        level
            .states
            .insert(origin, BlockState::of(Blocks::DIRT.id()));
        assert!(!place_with(&mut level, origin));
    }
}
