//! Port of `net.minecraft.world.level.levelgen.feature.HugeBrownMushroomFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.mushroom`
//! manifest unit.
//!
//! Java: the brown-mushroom `AbstractHugeMushroomFeature` subclass. Its cap
//! (`makeCap`) is a flat disc at `y = treeHeight` spanning `-radius..=radius`
//! on both horizontal axes, carving out the four corner cells (`!xEdge ||
//! !zEdge` writes every cell that is not simultaneously on an x-edge and a
//! z-edge). Each surviving cell draws the cap provider's state and — when it
//! carries all four horizontal `HugeMushroomBlock` properties — sets
//! `WEST`/`EAST`/`NORTH`/`SOUTH` from the edge booleans. The radius-per-layer
//! is `yo <= 3 ? 0 : leafRadius`, so the occupancy check in
//! `isValidPosition` only scans the trunk column.
//!
//! The RNG order is load-bearing: `getTreeHeight`'s two draws, then one cap
//! provider `getState` draw per surviving cap cell (in `dx`-major, `dz`-minor
//! order), then one stem provider draw per trunk cell. The port keeps that
//! exactly, including the `&&`/`||` short-circuiting of the property guards.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::abstract_huge_mushroom_feature::{
    place_mushroom, place_mushroom_block,
};
use crate::levelgen::feature::configurations::HugeMushroomFeatureConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider_get_state;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, MutableBlockPos, Vec3i};
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.HugeBrownMushroomFeature`.
#[derive(Debug)]
pub struct HugeBrownMushroomFeature;

/// `Feature.HUGE_BROWN_MUSHROOM` — the registered
/// `minecraft:huge_brown_mushroom` singleton.
pub const HUGE_BROWN_MUSHROOM: HugeBrownMushroomFeature = HugeBrownMushroomFeature;

/// `getTreeRadiusForHeight(int, int, int, int)` — `yo <= 3 ? 0 : leafRadius`.
fn get_tree_radius_for_height(
    _trunk_height: i32,
    _tree_height: i32,
    leaf_radius: i32,
    yo: i32,
) -> i32 {
    if yo <= 3 { 0 } else { leaf_radius }
}

/// `makeCap` — the flat brown cap disc.
fn make_cap<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    tree_height: i32,
    block_pos: &mut MutableBlockPos,
    config: &HugeMushroomFeatureConfiguration,
) {
    let radius = config.foliage_radius();
    let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());

    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let min_x = dx == -radius;
            let max_x = dx == radius;
            let min_z = dz == -radius;
            let max_z = dz == radius;
            let x_edge = min_x || max_x;
            let z_edge = min_z || max_z;
            if !x_edge || !z_edge {
                block_pos.set_with_offset_xyz(&origin_vec, dx, tree_height, dz);
                let west = min_x || z_edge && dx == 1i32.wrapping_sub(radius);
                let east = max_x || z_edge && dx == radius.wrapping_sub(1);
                let north = min_z || x_edge && dz == 1i32.wrapping_sub(radius);
                let south = max_z || x_edge && dz == radius.wrapping_sub(1);
                let state =
                    block_state_provider_get_state(&**config.cap_provider(), level, random, origin);
                let state = if state.has_property(BlockStateProperties::WEST)
                    && state.has_property(BlockStateProperties::EAST)
                    && state.has_property(BlockStateProperties::NORTH)
                    && state.has_property(BlockStateProperties::SOUTH)
                {
                    state
                        .set_value(BlockStateProperties::WEST, west)
                        .expect("brown mushroom cap carries the west property")
                        .set_value(BlockStateProperties::EAST, east)
                        .expect("brown mushroom cap carries the east property")
                        .set_value(BlockStateProperties::NORTH, north)
                        .expect("brown mushroom cap carries the north property")
                        .set_value(BlockStateProperties::SOUTH, south)
                        .expect("brown mushroom cap carries the south property")
                } else {
                    state
                };

                place_mushroom_block(level, block_pos, state);
            }
        }
    }
}

impl FeatureBehavior<HugeMushroomFeatureConfiguration> for HugeBrownMushroomFeature {
    /// `HugeBrownMushroomFeature.place(FeaturePlaceContext<...>)` — the shared
    /// `AbstractHugeMushroomFeature.place` walk with this subclass's cap and
    /// radius.
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, HugeMushroomFeatureConfiguration, R>,
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
        place_mushroom(
            level,
            random,
            &origin,
            config,
            get_tree_radius_for_height,
            make_cap::<R>,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;
    use std::sync::Arc;

    fn config() -> HugeMushroomFeatureConfiguration {
        HugeMushroomFeatureConfiguration::new(
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:brown_mushroom_block").unwrap(),
            ))),
            Arc::new(simple(BlockState::of(
                BlockId::from_name("minecraft:mushroom_stem").unwrap(),
            ))),
            2,
            always_true(),
        )
    }

    fn place(level: &mut TestLevel, random: &mut LegacyRandomSource) -> bool {
        let origin = BlockPos::new(0, 0, 0);
        let config = config();
        HUGE_BROWN_MUSHROOM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &crate::levelgen::feature::test_support::TestGenerator,
            random,
            &origin,
            &config,
        ))
    }

    /// The cap's y level is the max written y (the trunk occupies
    /// `0..treeHeight`, the disc sits at `treeHeight`).
    fn tree_height(level: &TestLevel) -> i32 {
        level
            .writes
            .iter()
            .map(|(pos, _)| pos.get_y())
            .max()
            .unwrap()
    }

    /// The brown cap is a radius-2 flat disc at `treeHeight`: every cell of
    /// the 5x5 footprint except the four corners (`±radius, ±radius`) is
    /// written — 21 cells — and the corners are carved out. The trunk is the
    /// stem provider's column below it, so the two blocks are distinct.
    #[test]
    fn brown_cap_writes_the_disc_without_corners() {
        let mut level = TestLevel::over(access());
        let mut random = LegacyRandomSource::new(1);
        assert!(place(&mut level, &mut random));
        let h = tree_height(&level);
        let cap_writes: Vec<_> = level
            .writes
            .iter()
            .filter(|(pos, _)| pos.get_y() == h)
            .collect();
        // 5x5 = 25, minus the 4 corners.
        assert_eq!(cap_writes.len(), 21);
        // The centre and the edge-non-corner cells are written.
        assert!(
            level
                .writes
                .iter()
                .any(|(pos, _)| *pos == BlockPos::new(0, h, 0))
        );
        assert!(
            level
                .writes
                .iter()
                .any(|(pos, _)| *pos == BlockPos::new(-2, h, 0))
        );
        // The four corners are not written.
        for (dx, dz) in [(-2, -2), (-2, 2), (2, -2), (2, 2)] {
            assert!(
                !level
                    .writes
                    .iter()
                    .any(|(pos, _)| *pos == BlockPos::new(dx, h, dz)),
                "corner ({dx}, {h}, {dz}) must be carved out"
            );
        }
        // All cap cells carry the cap provider's block.
        for (_, state) in cap_writes {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:brown_mushroom_block").unwrap()
            );
        }
    }

    /// The trunk is the stem provider's column from the origin up to (not
    /// including) the cap disc.
    #[test]
    fn brown_trunk_is_the_stem_column() {
        let mut level = TestLevel::over(access());
        let mut random = LegacyRandomSource::new(1);
        assert!(place(&mut level, &mut random));
        let h = tree_height(&level);
        for y in 0..h {
            let pos = BlockPos::new(0, y, 0);
            let state = level.states.get(&pos).copied().unwrap();
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:mushroom_stem").unwrap(),
                "trunk cell at {pos:?}"
            );
        }
    }

    /// The `WEST`/`EAST`/`NORTH`/`SOUTH` edge booleans are set on the cap:
    /// the west-edge column `dx == -radius` yields `WEST = true`, and a cell
    /// there that is not on the north/south edges leaves `NORTH`/`SOUTH`
    /// `false`.
    #[test]
    fn brown_cap_sets_edge_direction_properties() {
        let mut level = TestLevel::over(access());
        let mut random = LegacyRandomSource::new(1);
        assert!(place(&mut level, &mut random));
        let h = tree_height(&level);
        let west_cell = BlockPos::new(-2, h, 0);
        let state = level.states.get(&west_cell).copied().unwrap();
        assert_eq!(
            state.get_value(BlockStateProperties::WEST),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::EAST),
            Some(PropertyValue::Bool(false))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::NORTH),
            Some(PropertyValue::Bool(false))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::SOUTH),
            Some(PropertyValue::Bool(false))
        );
    }
}
