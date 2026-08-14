//! STUB(mc.world.level.block): `ChorusFlowerBlock.generatePlant` +
//! `ChorusPlantBlock.getStateWithConnections` + `allNeighborsEmpty` — the
//! cross-unit chorus-growth logic `ChorusPlantFeature` consumes before every
//! placement. Owned by the pending `mc.world.level.block` manifest unit (row
//! 454); the end-leaves wave ports it here faithfully so the feature's reach
//! is complete (the same cross-unit STUB pattern as `tree_feature.rs`'s
//! `valid_tree_pos`).
//!
//! `generatePlant(level, target, random, maxHorizontalSpread)` writes the
//! origin stem (the `CHORUS_PLANT` state with its connections), then grows a
//! recursive tree: each node draws a stem height (`nextInt(4) + 1`, plus one
//! at depth 0), writes each stem cell and its below-neighbor with their
//! connection states, then — at depths < 4 — draws `nextInt(4)` (plus one at
//! depth 0) horizontal branch directions (`Direction.Plane.HORIZONTAL`, i.e.
//! `NORTH, EAST, SOUTH, WEST` via `Util.getRandom`), placing a branch cell
//! only when it is within `maxHorizontalSpread` of the start on both axes and
//! the cell/below-neighbor are empty with all horizontal neighbors empty. If no
//! stem branch was placed, the terminal cell gets a dead `CHORUS_FLOWER`
//! (`AGE_5` = 5). Every write uses `Block.UPDATE_CLIENTS` (2), exactly as the
//! Java `level.setBlock(...)` calls.
//!
//! `getStateWithConnections(level, pos, defaultState)` reads the six neighbor
//! states and sets the `DOWN/UP/NORTH/EAST/SOUTH/WEST` bool properties to
//! "is a `CHORUS_PLANT`/`CHORUS_FLOWER`", with `DOWN` additionally true for the
//! `SUPPORTS_CHORUS_PLANT` tag. The values go through `trySetValue`, which
//! leaves a state unchanged when the property is absent (the `CHORUS_PLANT`
//! block carries all six).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::is_block;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::core::Plane;
use rivet_util::RandomSource;
use rivet_util::mth;

/// `Block.UPDATE_CLIENTS` — the write-flag constant every chorus write uses.
const UPDATE_CLIENTS: u32 = 2;

/// `ChorusPlantBlock.getStateWithConnections(BlockGetter, BlockPos,
/// BlockState)` — the connection state for a cell given its six neighbors.
/// `trySetValue` returns `self` unchanged for an absent property; the
/// `CHORUS_PLANT` block carries all six bool properties, so each set applies.
fn get_state_with_connections(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    default_state: BlockState,
) -> BlockState {
    let down = level.get_block_state(&pos.below());
    let up = level.get_block_state(&pos.above());
    let north = level.get_block_state(&pos.north());
    let east = level.get_block_state(&pos.east());
    let south = level.get_block_state(&pos.south());
    let west = level.get_block_state(&pos.west());
    // `Block block = defaultState.getBlock()` — the connection tests key off
    // the passed-in default state's block (Java's `getStateWithConnections`),
    // not a hardcoded constant.
    let block = crate::block::Block::new(default_state.block());
    let connect =
        |state: BlockState| is_block(state, block) || is_block(state, Blocks::CHORUS_FLOWER);
    // `trySetValue` leaves the state unchanged for an absent property, so
    // every link is infallible for the `CHORUS_PLANT` block (it carries all
    // six bool properties) — the `expect` only satisfies the `Result`.
    default_state
        .try_set_value(
            BlockStateProperties::DOWN,
            connect(down) || down.is_in_tag("minecraft:supports_chorus_plant"),
        )
        .expect("chorus_plant carries the connection properties")
        .try_set_value(BlockStateProperties::UP, connect(up))
        .expect("chorus_plant carries the connection properties")
        .try_set_value(BlockStateProperties::NORTH, connect(north))
        .expect("chorus_plant carries the connection properties")
        .try_set_value(BlockStateProperties::EAST, connect(east))
        .expect("chorus_plant carries the connection properties")
        .try_set_value(BlockStateProperties::SOUTH, connect(south))
        .expect("chorus_plant carries the connection properties")
        .try_set_value(BlockStateProperties::WEST, connect(west))
        .expect("chorus_plant carries the connection properties")
}

/// `ChorusFlowerBlock.allNeighborsEmpty(LevelReader, BlockPos, @Nullable
/// Direction)` — every horizontal neighbor (except the ignored direction) is
/// empty. The `@Nullable` ignored direction is `None` at the worldgen call
/// sites the feature reaches.
fn all_neighbors_empty(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    ignore: Option<Direction>,
) -> bool {
    for direction in Plane::Horizontal.faces() {
        if Some(*direction) != ignore && !level.is_empty_block(&pos.relative(direction)) {
            return false;
        }
    }
    true
}

/// `ChorusFlowerBlock.generatePlant(LevelAccessor, BlockPos, RandomSource,
/// int)` — `level.setBlock(target, getStateWithConnections(...), UPDATE_CLIENTS)`
/// then `growTreeRecursive(level, target, random, target, maxHorizontalSpread,
/// 0)`.
pub fn generate_plant<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    target: &BlockPos,
    random: &mut R,
    max_horizontal_spread: i32,
) {
    level.set_block(
        target,
        get_state_with_connections(level, target, Blocks::CHORUS_PLANT.default_block_state()),
        UPDATE_CLIENTS,
    );
    grow_tree_recursive(level, target, random, target, max_horizontal_spread, 0);
}

/// `ChorusFlowerBlock.growTreeRecursive(...)` — the recursive tree growth.
fn grow_tree_recursive<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    current: &BlockPos,
    random: &mut R,
    start_pos: &BlockPos,
    max_horizontal_spread: i32,
    depth: i32,
) {
    let mut height = random.next_int_bound(4).wrapping_add(1);
    if depth == 0 {
        height = height.wrapping_add(1);
    }
    for i in 0..height {
        let target = current.above_steps(i.wrapping_add(1));
        if !all_neighbors_empty(level, &target, None) {
            return;
        }
        level.set_block(
            &target,
            get_state_with_connections(level, &target, Blocks::CHORUS_PLANT.default_block_state()),
            UPDATE_CLIENTS,
        );
        level.set_block(
            &target.below(),
            get_state_with_connections(
                level,
                &target.below(),
                Blocks::CHORUS_PLANT.default_block_state(),
            ),
            UPDATE_CLIENTS,
        );
    }
    let mut placed_stem = false;
    if depth < 4 {
        let mut stems = random.next_int_bound(4);
        if depth == 0 {
            stems = stems.wrapping_add(1);
        }
        for _ in 0..stems {
            let direction = Plane::Horizontal.faces()
                [random.next_int_bound(Plane::Horizontal.faces().len() as i32) as usize];
            let target = current.above_steps(height).relative(&direction);
            if mth::abs_i32(target.get_x().wrapping_sub(start_pos.get_x())) < max_horizontal_spread
                && mth::abs_i32(target.get_z().wrapping_sub(start_pos.get_z()))
                    < max_horizontal_spread
                && level.is_empty_block(&target)
                && level.is_empty_block(&target.below())
                && all_neighbors_empty(level, &target, Some(direction.get_opposite()))
            {
                placed_stem = true;
                level.set_block(
                    &target,
                    get_state_with_connections(
                        level,
                        &target,
                        Blocks::CHORUS_PLANT.default_block_state(),
                    ),
                    UPDATE_CLIENTS,
                );
                let opposite = target.relative(&direction.get_opposite());
                level.set_block(
                    &opposite,
                    get_state_with_connections(
                        level,
                        &opposite,
                        Blocks::CHORUS_PLANT.default_block_state(),
                    ),
                    UPDATE_CLIENTS,
                );
                grow_tree_recursive(
                    level,
                    &target,
                    random,
                    start_pos,
                    max_horizontal_spread,
                    depth + 1,
                );
            }
        }
    }
    if !placed_stem {
        level.set_block(
            &current.above_steps(height),
            Blocks::CHORUS_FLOWER
                .default_block_state()
                .set_value(BlockStateProperties::AGE_5, 5)
                .expect("chorus_flower carries the age property"),
            UPDATE_CLIENTS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{RecordingRandom, RngCall, TestLevel, access};
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    fn grow(level: &mut TestLevel, random: &mut RecordingRandom, origin: BlockPos) {
        generate_plant(level, &origin, random, 8);
    }

    fn grow_with_spread(
        level: &mut TestLevel,
        random: &mut RecordingRandom,
        origin: BlockPos,
        max_spread: i32,
    ) {
        generate_plant(level, &origin, random, max_spread);
    }

    /// `getStateWithConnections` sets the six connection bools from the
    /// neighbor states. A plant over `END_STONE` gets `DOWN` true and the
    /// horizontal/up connections false.
    #[test]
    fn connections_reflect_neighbors() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            Blocks::END_STONE.default_block_state(),
        );
        let state = get_state_with_connections(
            &level,
            &BlockPos::new(0, 0, 0),
            Blocks::CHORUS_PLANT.default_block_state(),
        );
        assert_eq!(
            state.get_value(BlockStateProperties::DOWN),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(false))
        );
    }

    /// `generatePlant` writes the origin stem first, then grows the recursive
    /// tree. Every write is `CHORUS_PLANT`/`CHORUS_FLOWER` with `UPDATE_CLIENTS`.
    #[test]
    fn generate_plant_writes_stem_then_grows() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            Blocks::END_STONE.default_block_state(),
        );
        let mut random = RecordingRandom::new(5);
        grow(&mut level, &mut random, BlockPos::new(0, 0, 0));
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert!(!level.writes.is_empty());
        let chorus = BlockId::from_name("minecraft:chorus_plant").unwrap();
        let flower = BlockId::from_name("minecraft:chorus_flower").unwrap();
        for (_, state) in &level.writes {
            assert!(state.block() == chorus || state.block() == flower);
        }
        // The draws match `growTreeRecursive`'s shape: a `nextInt(4)` height
        // (plus one at depth 0), then a `nextInt(4)` stem count (plus one at
        // depth 0), then one `nextInt(4)` per horizontal-direction pick.
        assert!(
            random.calls.iter().all(|c| *c == RngCall::IntBound(4)),
            "chorus growth draws only nextInt(4): {:?}",
            random.calls
        );
    }

    /// A `maxHorizontalSpread` of 0 forbids every horizontal branch: each
    /// node's `abs(target.x - start.x) < maxHorizontalSpread` gate fails, so
    /// every node writes the dead `AGE_5` flower atop its stem — and no write
    /// ever leaves the start column. This pins the spread gate and the
    /// `ChorusFlowerBlock.DEAD_AGE` terminal.
    #[test]
    fn zero_spread_blocks_branches_and_terminates_each_stem_in_age5_flower() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            Blocks::END_STONE.default_block_state(),
        );
        let mut random = RecordingRandom::new(7);
        grow_with_spread(&mut level, &mut random, BlockPos::new(0, 0, 0), 0);
        assert!(!level.writes.is_empty());
        let flower = BlockId::from_name("minecraft:chorus_flower").unwrap();
        let chorus = BlockId::from_name("minecraft:chorus_plant").unwrap();
        for (pos, state) in &level.writes {
            // No branch can escape the start column when the spread is 0.
            assert_eq!(pos.get_x(), 0);
            assert_eq!(pos.get_z(), 0);
            if state.block() == flower {
                assert_eq!(
                    state.get_value(BlockStateProperties::AGE_5),
                    Some(PropertyValue::Int(5))
                );
            } else {
                assert_eq!(state.block(), chorus);
            }
        }
    }
}
