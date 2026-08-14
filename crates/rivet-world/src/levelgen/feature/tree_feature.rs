//! STUB(mc.world.level.levelgen.feature.tree): `TreeFeature` /
//! `FallenTreeFeature` are owned by the pending `feature.tree` manifest unit
//! (MANIFEST.tsv row 569, task #1327). The foliage-placer slice calls the
//! static helper `TreeFeature.validTreePos` before every leaf placement
//! (`FoliagePlacer.tryPlaceLeaf`), so this stub carries exactly that one free
//! function; the owning unit replaces this file when it lands. The
//! trunk/root/decorator consumers of this helper live on the preserved
//! `feature/worldgen-tree-scaffolding` branch.

use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;

/// `TreeFeature.validTreePos(LevelSimulatedReader, BlockPos)` —
/// `level.isStateAtPosition(pos, state -> state.isAir() ||
/// state.is(BlockTags.REPLACEABLE_BY_TREES))`. The tag test translates to
/// `BlockState.is_in_tag("minecraft:replaceable_by_trees")`, matching the
/// `BlockTags.REPLACEABLE_BY_TREES` holder's `#minecraft:replaceable_by_trees`
/// id.
pub fn valid_tree_pos(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.is_state_at_position(pos, &|state: &BlockState| {
        state.is_air() || state.is_in_tag("minecraft:replaceable_by_trees")
    })
}

/// STUB(mc.world.level.levelgen.feature.tree): `TreeFeature.isAirOrLeaves(
/// LevelSimulatedReader, BlockPos)` — `level.isStateAtPosition(pos, state ->
/// state.isAir() || state.is(BlockTags.LEAVES))`, consumed by
/// `DarkOakTrunkPlacer.placeTrunk`. The owning unit replaces this file.
pub fn is_air_or_leaves(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.is_state_at_position(pos, &|state: &BlockState| {
        state.is_air() || state.is_in_tag("minecraft:leaves")
    })
}
