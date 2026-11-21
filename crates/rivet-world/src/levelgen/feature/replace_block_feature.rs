//! Port of `net.minecraft.world.level.levelgen.feature.ReplaceBlockFeature` (26.2).
//!
//! Iterates `config.target_states` in order and tests each target rule test
//! against `level.get_block_state(&origin)`. The first matching entry writes
//! `target_state.state` with `Block.UPDATE_CLIENTS` (`2`) and breaks. Always
//! returns `true`.

use crate::levelgen::feature::configurations::ReplaceBlockConfiguration;
use crate::levelgen::feature::{FeatureBehavior, FeaturePlaceContext};
use crate::levelgen::structure::templatesystem::rule_test::erased_test;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` (`2`) — the write-flag constant `ReplaceBlockFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.ReplaceBlockFeature`.
#[derive(Debug)]
pub struct ReplaceBlockFeature;

/// The registered `minecraft:replace_single_block` (`Feature.REPLACE_SINGLE_BLOCK`) leaf.
pub const REPLACE_SINGLE_BLOCK: ReplaceBlockFeature = ReplaceBlockFeature;

impl FeatureBehavior<ReplaceBlockConfiguration> for ReplaceBlockFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, ReplaceBlockConfiguration, R>,
    ) -> bool {
        let origin = *context.origin;
        let current = context.level.get_block_state(&origin);

        for target_state in &context.config.target_states {
            if erased_test(&target_state.target, &current, context.random) {
                context
                    .level
                    .set_block(&origin, target_state.state, UPDATE_CLIENTS);
                break;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::configurations::TargetBlockState;
    use crate::levelgen::feature::{FeatureId, feature_place};
    use crate::levelgen::structure::templatesystem::block_match_test::BlockMatchTest;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_util::random::LegacyRandomSource;
    use std::collections::HashMap;
    use std::sync::Arc;

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
            256
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

        fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
            self.blocks.insert(*pos, state);
            self.writes.push((*pos, state, flags));
            true
        }
    }

    struct StubGenerator;
    impl ChunkGenerator for StubGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            256
        }
    }

    #[test]
    fn replaces_first_matching_target_with_update_clients_and_breaks() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(4, 64, -2);
        level
            .blocks
            .insert(origin, Blocks::STONE.default_block_state());

        let config = ReplaceBlockConfiguration::new(vec![
            TargetBlockState::new(
                Arc::new(BlockMatchTest::new(Blocks::DIRT)),
                Blocks::COAL_ORE.default_block_state(),
            ),
            TargetBlockState::new(
                Arc::new(BlockMatchTest::new(Blocks::STONE)),
                Blocks::IRON_ORE.default_block_state(),
            ),
            TargetBlockState::new(
                Arc::new(BlockMatchTest::new(Blocks::STONE)),
                Blocks::GOLD_ORE.default_block_state(),
            ),
        ]);

        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(42);
        let placed = feature_place(
            FeatureId::new(6),
            &config,
            &mut level,
            &generator,
            &mut random,
            &origin,
        );

        assert!(placed);
        assert_eq!(level.writes.len(), 1);
        assert_eq!(
            level.writes[0],
            (
                origin,
                Blocks::IRON_ORE.default_block_state(),
                UPDATE_CLIENTS
            )
        );
    }

    #[test]
    fn returns_true_without_writing_when_no_target_matches() {
        let mut level = MapLevel::new();
        let origin = BlockPos::new(0, 10, 0);
        level
            .blocks
            .insert(origin, Blocks::GRANITE.default_block_state());

        let config = ReplaceBlockConfiguration::new(vec![TargetBlockState::new(
            Arc::new(BlockMatchTest::new(Blocks::STONE)),
            Blocks::IRON_ORE.default_block_state(),
        )]);

        let generator = StubGenerator;
        let mut random = LegacyRandomSource::new(7);
        let mut ctx =
            FeaturePlaceContext::new(None, &mut level, &generator, &mut random, &origin, &config);
        assert!(REPLACE_SINGLE_BLOCK.place(&mut ctx));
        assert!(level.writes.is_empty());
    }
}
