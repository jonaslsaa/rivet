//! Port of `net.minecraft.world.level.levelgen.feature.HugeRedMushroomFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.mushroom`
//! manifest unit.
//!
//! Java: the red-mushroom `AbstractHugeMushroomFeature` subclass. Its cap
//! (`makeCap`) runs four layers from `treeHeight - 3` up to `treeHeight`; the
//! lower three draw a ring of radius `foliageRadius` (only the edge cells whose
//! `xEdge != zEdge` — the non-corner ring — are written), and the top layer
//! draws a disc of radius `foliageRadius - 1` in full. Each surviving cell
//! draws the cap provider's state and — when it carries the four horizontal
//! `HugeMushroomBlock` properties plus `UP` — sets `UP` on the top two layers
//! and `WEST`/`EAST`/`NORTH`/`SOUTH` from the sign of the offset from
//! `center = foliageRadius - 2`. The radius-per-layer is `leafRadius` for
//! `yo` in `[treeHeight - 3, treeHeight]`, so the occupancy check in
//! `isValidPosition` scans exactly the cap footprint.
//!
//! The RNG order is load-bearing: `getTreeHeight`'s two draws, then one cap
//! provider `getState` draw per surviving cap cell (layer `dy` ascending, then
//! `dx`-major, `dz`-minor within each layer), then one stem provider draw per
//! trunk cell. The port keeps that exactly, including the `&&`/`||`
//! short-circuiting of the property guards.

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

/// `net.minecraft.world.level.levelgen.feature.HugeRedMushroomFeature`.
#[derive(Debug)]
pub struct HugeRedMushroomFeature;

/// `Feature.HUGE_RED_MUSHROOM` — the registered
/// `minecraft:huge_red_mushroom` singleton.
pub const HUGE_RED_MUSHROOM: HugeRedMushroomFeature = HugeRedMushroomFeature;

/// `getTreeRadiusForHeight(int, int, int, int)` — `leafRadius` when `yo` is in
/// `[treeHeight - 3, treeHeight]`, `0` otherwise.
fn get_tree_radius_for_height(
    _trunk_height: i32,
    tree_height: i32,
    leaf_radius: i32,
    yo: i32,
) -> i32 {
    let mut radius = 0;
    if yo < tree_height && yo >= tree_height.wrapping_sub(3) {
        radius = leaf_radius;
    } else if yo == tree_height {
        radius = leaf_radius;
    }
    radius
}

/// `makeCap` — the four-layer red cap.
fn make_cap<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    tree_height: i32,
    block_pos: &mut MutableBlockPos,
    config: &HugeMushroomFeatureConfiguration,
) {
    for dy in tree_height.wrapping_sub(3)..=tree_height {
        let radius = if dy < tree_height {
            config.foliage_radius()
        } else {
            config.foliage_radius().wrapping_sub(1)
        };
        let center = config.foliage_radius().wrapping_sub(2);
        let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());

        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let min_x = dx == -radius;
                let max_x = dx == radius;
                let min_z = dz == -radius;
                let max_z = dz == radius;
                let x_edge = min_x || max_x;
                let z_edge = min_z || max_z;
                if dy >= tree_height || x_edge != z_edge {
                    block_pos.set_with_offset_xyz(&origin_vec, dx, dy, dz);
                    let state = block_state_provider_get_state(
                        &**config.cap_provider(),
                        level,
                        random,
                        origin,
                    );
                    let state = if state.has_property(BlockStateProperties::WEST)
                        && state.has_property(BlockStateProperties::EAST)
                        && state.has_property(BlockStateProperties::NORTH)
                        && state.has_property(BlockStateProperties::SOUTH)
                        && state.has_property(BlockStateProperties::UP)
                    {
                        state
                            .set_value(BlockStateProperties::UP, dy >= tree_height.wrapping_sub(1))
                            .expect("red mushroom cap carries the up property")
                            .set_value(BlockStateProperties::WEST, dx < center.wrapping_neg())
                            .expect("red mushroom cap carries the west property")
                            .set_value(BlockStateProperties::EAST, dx > center)
                            .expect("red mushroom cap carries the east property")
                            .set_value(BlockStateProperties::NORTH, dz < center.wrapping_neg())
                            .expect("red mushroom cap carries the north property")
                            .set_value(BlockStateProperties::SOUTH, dz > center)
                            .expect("red mushroom cap carries the south property")
                    } else {
                        state
                    };

                    place_mushroom_block(level, block_pos, state);
                }
            }
        }
    }
}

impl FeatureBehavior<HugeMushroomFeatureConfiguration> for HugeRedMushroomFeature {
    /// `HugeRedMushroomFeature.place(FeaturePlaceContext<...>)` — the shared
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
            &config,
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
                BlockId::from_name("minecraft:red_mushroom_block").unwrap(),
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
        HUGE_RED_MUSHROOM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &crate::levelgen::feature::test_support::TestGenerator,
            random,
            &origin,
            &config,
        ))
    }

    fn tree_height(level: &TestLevel) -> i32 {
        level
            .writes
            .iter()
            .map(|(pos, _)| pos.get_y())
            .max()
            .unwrap()
    }

    /// The red cap is four layers: three ring layers of radius 2 (only the 12
    /// non-corner edge cells, `xEdge != zEdge`) below a full radius-1 disc
    /// (9 cells) at `treeHeight`. That is 3×12 + 9 = 45 cap cells. The cap
    /// cells all carry the cap provider's block.
    #[test]
    fn red_cap_writes_three_rings_and_a_top_disc() {
        let mut level = TestLevel::over(access());
        let mut random = LegacyRandomSource::new(1);
        assert!(place(&mut level, &mut random));
        let h = tree_height(&level);
        let cap_block = BlockId::from_name("minecraft:red_mushroom_block").unwrap();
        // The `y >= h-3` window also catches the three trunk stem cells at the
        // centre column (`placeTrunk` writes the stem at every cell from the
        // origin up `treeHeight`), so count only the cap-block cells.
        let cap_writes: Vec<_> = level
            .writes
            .iter()
            .filter(|(pos, state)| pos.get_y() >= h.wrapping_sub(3) && state.block() == cap_block)
            .collect();
        assert_eq!(cap_writes.len(), 45);
        // The top disc is the full 3x3 at y = h.
        let top: Vec<_> = cap_writes
            .iter()
            .filter(|(pos, _)| pos.get_y() == h)
            .collect();
        assert_eq!(top.len(), 9);
        // The centre of a ring layer is carved out (xEdge == zEdge).
        assert!(
            !cap_writes
                .iter()
                .any(|(pos, _)| *pos == BlockPos::new(0, h.wrapping_sub(1), 0))
        );
        for (_, state) in cap_writes {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:red_mushroom_block").unwrap()
            );
        }
    }

    /// The `UP` property marks the top two layers: `dy >= treeHeight - 1`. At
    /// the top disc the horizontal properties are false (the top radius-1
    /// disc's centre is `dx == dz == 0`, neither `< -center` nor `> center`).
    #[test]
    fn red_cap_sets_up_on_the_top_two_layers() {
        let mut level = TestLevel::over(access());
        let mut random = LegacyRandomSource::new(1);
        assert!(place(&mut level, &mut random));
        let h = tree_height(&level);
        let top_centre = BlockPos::new(0, h, 0);
        let state = level.states.get(&top_centre).copied().unwrap();
        assert_eq!(
            state.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::WEST),
            Some(PropertyValue::Bool(false))
        );
        // A lower ring layer is not the top: UP false, and the offset sign
        // drives the horizontal property (dx = +2 > center 0 → EAST). `h-2` is
        // genuinely below `treeHeight - 1`, the top-two-layers boundary.
        let ring_cell = BlockPos::new(2, h.wrapping_sub(2), 0);
        let state = level.states.get(&ring_cell).copied().unwrap();
        assert_eq!(
            state.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(false))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::EAST),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::WEST),
            Some(PropertyValue::Bool(false))
        );
    }
}
