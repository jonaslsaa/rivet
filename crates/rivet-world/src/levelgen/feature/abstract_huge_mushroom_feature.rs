//! Port of `net.minecraft.world.level.levelgen.feature.AbstractHugeMushroomFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.mushroom`
//! manifest unit.
//!
//! Java: the abstract `Feature<HugeMushroomFeatureConfiguration>` shared by
//! `HugeBrownMushroomFeature`/`HugeRedMushroomFeature`. `place` samples the
//! tree height (`getTreeHeight`), rejects the origin when the position check
//! fails, then builds the cap (`makeCap`) followed by the trunk
//! (`placeTrunk`). The trunk writes the stem provider's state at every cell
//! from `origin` up `treeHeight`; the cap is the subclass's `makeCap`, both
//! routing through `placeMushroomBlock`, which only writes when the current
//! cell is air or a `#minecraft:replaceable_by_mushrooms` block. The
//! subclass-specific radius-per-layer (`getTreeRadiusForHeight`) is folded
//! into `isValidPosition`'s occupancy scan.
//!
//! The RNG order is load-bearing: `getTreeHeight` draws `nextInt(3)` then a
//! conditional `nextInt(12)`, and the per-cell provider `getState` draws
//! happen in cap/trunk write order (cap first, then stem). The port keeps that
//! exactly, including the `&&`/`||` short-circuiting and the wrapping
//! coordinate arithmetic. Writes use `Feature.setBlock`
//! (`Block.UPDATE_ALL`, 3).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::HugeMushroomFeatureConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider_get_state;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction, MutableBlockPos, Vec3i};
use rivet_util::RandomSource;

/// `Feature.setBlock` — `level.setBlock(pos, state, Block.UPDATE_ALL)`.
const UPDATE_ALL: u32 = 3;

/// `AbstractHugeMushroomFeature.MIN_MUSHROOM_HEIGHT` — the minimum sampled
/// trunk height (`getTreeHeight` returns `nextInt(3) + 4`, so 4..=6, doubled
/// on a `nextInt(12) == 0`).
pub const MIN_MUSHROOM_HEIGHT: i32 = 4;

/// The cap/trunk writing helper `AbstractHugeMushroomFeature.placeMushroomBlock`
/// and the trunk loop share: both are the behaviour-shared seam of this abstract
/// class, exposed for the two concrete features.
///
/// `placeMushroomBlock(LevelAccessor, MutableBlockPos, BlockState)` — write
/// `new_state` at `block_pos` when the current state is air or
/// `#minecraft:replaceable_by_mushrooms`, via `Feature.setBlock`.
pub(crate) fn place_mushroom_block(
    level: &mut dyn WorldGenLevel,
    block_pos: &MutableBlockPos,
    new_state: BlockState,
) {
    let current_state = level.get_block_state(&block_pos.immutable());
    if current_state.is_air() || current_state.is_in_tag("minecraft:replaceable_by_mushrooms") {
        level.set_block(&block_pos.immutable(), new_state, UPDATE_ALL);
    }
}

/// `placeTrunk(WorldGenLevel, RandomSource, BlockPos, config, treeHeight,
/// MutableBlockPos)` — write the stem provider's state at `origin.move(UP, dy)`
/// for each `dy < treeHeight`.
pub(crate) fn place_trunk<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    config: &HugeMushroomFeatureConfiguration,
    tree_height: i32,
    block_pos: &mut MutableBlockPos,
) {
    for dy in 0..tree_height {
        block_pos
            .set_vec(&Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z()))
            .move_dir_steps(&Direction::Up, dy);
        let state =
            block_state_provider_get_state(&**config.stem_provider(), level, random, origin);
        place_mushroom_block(level, block_pos, state);
    }
}

/// `getTreeHeight(RandomSource)` — `random.nextInt(3) + 4`, doubled when the
/// following `random.nextInt(12)` is `0`.
pub(crate) fn get_tree_height<R: RandomSource>(random: &mut R) -> i32 {
    let mut tree_height = random.next_int_bound(3).wrapping_add(4);
    if random.next_int_bound(12) == 0 {
        tree_height = tree_height.wrapping_mul(2);
    }
    tree_height
}

/// `isValidPosition(WorldGenLevel, BlockPos, int, MutableBlockPos, config)` —
/// the origin must sit at `minY + 1` or above with the whole tree within the
/// build height, stand on `config.canPlaceOn`, and every cell of the
/// radius-per-layer cap footprint must be air or leaves.
pub(crate) fn is_valid_position(
    level: &dyn WorldGenLevel,
    origin: &BlockPos,
    tree_height: i32,
    block_pos: &mut MutableBlockPos,
    config: &HugeMushroomFeatureConfiguration,
    get_tree_radius_for_height: &dyn Fn(i32, i32, i32, i32) -> i32,
) -> bool {
    let y = origin.get_y();
    if y >= level.get_min_y().wrapping_add(1)
        && y.wrapping_add(tree_height).wrapping_add(1) <= level.get_max_y()
    {
        if !config.can_place_on().test(level, &origin.below()) {
            return false;
        }

        let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());

        for dy in 0..=tree_height {
            let radius = get_tree_radius_for_height(-1, -1, config.foliage_radius(), dy);

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let state = level.get_block_state(
                        &block_pos
                            .set_with_offset_xyz(&origin_vec, dx, dy, dz)
                            .immutable(),
                    );
                    if !state.is_air() && !state.is_in_tag("minecraft:leaves") {
                        return false;
                    }
                }
            }
        }

        true
    } else {
        false
    }
}

/// The cap `makeCap(WorldGenLevel, RandomSource, BlockPos, int, MutableBlockPos,
/// config)` — the subclass-specific cap writing, defined in the two concrete
/// features.
pub(crate) type MakeCap<R> = fn(
    &mut dyn WorldGenLevel,
    &mut R,
    &BlockPos,
    i32,
    &mut MutableBlockPos,
    &HugeMushroomFeatureConfiguration,
);

/// `net.minecraft.world.level.levelgen.feature.AbstractHugeMushroomFeature` —
/// the shared placement: sample height, validate, cap, then trunk.
///
/// Java's abstract `makeCap`/`getTreeRadiusForHeight` are modelled as the
/// closure/`MakeCap` arguments so the two concrete features share this exact
/// placement walk without a trait object.
pub(crate) fn place_mushroom<R: RandomSource, F: Fn(i32, i32, i32, i32) -> i32>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    config: &HugeMushroomFeatureConfiguration,
    get_tree_radius_for_height: F,
    make_cap: MakeCap<R>,
) -> bool {
    let tree_height = get_tree_height(random);
    let mut block_pos = MutableBlockPos::new(0, 0, 0);
    if !is_valid_position(
        level,
        origin,
        tree_height,
        &mut block_pos,
        config,
        &get_tree_radius_for_height,
    ) {
        return false;
    }

    make_cap(level, random, origin, tree_height, &mut block_pos, config);
    place_trunk(level, random, origin, config, tree_height, &mut block_pos);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestLevel, access};
    use rivet_registry::block_state::BlockState;
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

    fn radius_trivial(_trunk_height: i32, _tree_height: i32, _leaf_radius: i32, _yo: i32) -> i32 {
        0
    }

    fn cap_trivial<R: RandomSource>(
        _level: &mut dyn WorldGenLevel,
        _random: &mut R,
        _origin: &BlockPos,
        _tree_height: i32,
        _block_pos: &mut MutableBlockPos,
        _config: &HugeMushroomFeatureConfiguration,
    ) {
    }

    /// `place` samples `getTreeHeight` (a `nextInt(3)` then a conditional
    /// `nextInt(12)`), then writes the trunk — one stem-provider draw and one
    /// `setBlock` per layer, from the origin up. The cap is a no-op here, so
    /// the writes are exactly the stem cells.
    #[test]
    fn place_writes_stem_from_origin_up() {
        let mut level = TestLevel::over(access());
        let config = config();
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(1);
        let placed = place_mushroom(
            &mut level,
            &mut random,
            &origin,
            &config,
            radius_trivial,
            cap_trivial,
        );
        assert!(placed);
        // The tree height for seed 1 is not pinned — just that every layer up
        // from the origin wrote the stem.
        assert!(!level.writes.is_empty());
        for (i, (pos, state)) in level.writes.iter().enumerate() {
            assert_eq!(
                *pos,
                BlockPos::new(0, i as i32, 0),
                "stem cell {} must be at origin.up({})",
                i,
                i
            );
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:mushroom_stem").unwrap()
            );
        }
    }

    /// `placeMushroomBlock` refuses to overwrite a non-air, non-replaceable
    /// cell: a stone cell blocks the write (empty writes), while an air cell
    /// at the same position writes through.
    #[test]
    fn place_mushroom_block_skips_solid_cells() {
        let mushroom =
            BlockState::of(BlockId::from_name("minecraft:brown_mushroom_block").unwrap());
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 1, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        let block_pos = MutableBlockPos::new(0, 1, 0);
        place_mushroom_block(&mut level, &block_pos, mushroom);
        assert!(level.writes.is_empty());
        // The same position reads air now, so the write goes through.
        level.states.clear();
        place_mushroom_block(&mut level, &block_pos, mushroom);
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 1, 0));
        assert_eq!(level.writes[0].1, mushroom);
    }

    /// `is_valid_position` rejects an origin outside the build height (below
    /// `minY + 1`) — `false` with no writes and no RNG draws beyond the height
    /// sample.
    #[test]
    fn outside_build_height_returns_false_without_writes() {
        let mut level = TestLevel::over(access());
        let config = config();
        let origin = BlockPos::new(0, -70, 0); // below minY + 1 = -63
        let mut random = LegacyRandomSource::new(1);
        assert!(!place_mushroom(
            &mut level,
            &mut random,
            &origin,
            &config,
            radius_trivial,
            cap_trivial,
        ));
        assert!(level.writes.is_empty());
    }
}
