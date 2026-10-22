//! Paper StructureTemplate.updateShapeAtEdge and source-owned block updates.

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::{Property, PropertyValue};
use rivet_registry::core::{BlockPos, Direction, Plane};
use rivet_registry::fluid_id::FluidId;
use rivet_util::worldgen_random::AlgorithmRandomSource;
use rivet_world::block::Block;
use rivet_world::block::blocks::Blocks;
use rivet_world::level::WorldGenLevel;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[cfg(test)]
#[path = "block_shape_update_fixture.rs"]
mod paper_fixture;

/// Reflection of the actual declaring updateShape implementation in pinned
/// Paper, not an inference from tags, collision geometry, or block names.
fn update_owner(state: BlockState) -> &'static str {
    static OWNERS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let owners = OWNERS.get_or_init(|| {
        include_str!("../../../../../tools/rivet-oracle/fixtures/block-update-shape-owners.tsv")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                line.split_once('\t')
                    .expect("generated updateShape owner row")
            })
            .collect()
    });
    owners
        .get(state.block().name())
        .copied()
        .expect("generated block updateShape owner")
}

fn vine_face(direction: Direction) -> Property {
    match direction {
        Direction::Up => BlockStateProperties::UP,
        Direction::North => BlockStateProperties::NORTH,
        Direction::East => BlockStateProperties::EAST,
        Direction::South => BlockStateProperties::SOUTH,
        Direction::West => BlockStateProperties::WEST,
        Direction::Down => unreachable!("vines have no down face"),
    }
}

fn acceptable_vine_neighbor(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    direction: Direction,
) -> bool {
    level.can_attach_to(pos, &level.get_block_state(pos), &direction.get_opposite())
}

fn updated_vine(mut state: BlockState, level: &dyn WorldGenLevel, pos: &BlockPos) -> BlockState {
    let above = pos.above();
    if state.get_value(BlockStateProperties::UP) == Some(PropertyValue::Bool(true)) {
        state = state
            .set_value(
                BlockStateProperties::UP,
                acceptable_vine_neighbor(level, &above, Direction::Down),
            )
            .unwrap();
    }
    for direction in Plane::Horizontal.faces() {
        let property = vine_face(*direction);
        if state.get_value(property) == Some(PropertyValue::Bool(true)) {
            let supports = acceptable_vine_neighbor(level, &pos.relative(direction), *direction)
                || {
                    let above_state = level.get_block_state(&above);
                    above_state.block() == state.block()
                        && above_state.get_value(property) == Some(PropertyValue::Bool(true))
                };
            state = state.set_value(property, supports).unwrap();
        }
    }
    if [
        Direction::Up,
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ]
    .into_iter()
    .any(|direction| state.get_value(vine_face(direction)) == Some(PropertyValue::Bool(true)))
    {
        state
    } else {
        Blocks::AIR.default_block_state()
    }
}

/// `DiscreteVoxelShape.forAllFaces`: Z, Y, X axis scans, with negative
/// then positive transitions in each column. Only occupancy is unordered.
pub(super) fn boundary_faces(filled: &HashSet<BlockPos>) -> Vec<(Direction, BlockPos)> {
    let Some(first) = filled.iter().next() else {
        return Vec::new();
    };
    let mut min = [first.get_x(), first.get_y(), first.get_z()];
    let mut max = min;
    for pos in filled {
        for (axis, value) in [pos.get_x(), pos.get_y(), pos.get_z()]
            .into_iter()
            .enumerate()
        {
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    let mut faces = Vec::new();
    for (axes, negative, positive) in [
        ([0, 1, 2], Direction::North, Direction::South),
        ([2, 0, 1], Direction::Down, Direction::Up),
        ([1, 2, 0], Direction::West, Direction::East),
    ] {
        let [aa, bb, cc] = axes;
        for a in min[aa]..=max[aa] {
            for b in min[bb]..=max[bb] {
                let mut last_full = false;
                for c in min[cc]..=max[cc] {
                    let mut coordinates = [0; 3];
                    coordinates[aa] = a;
                    coordinates[bb] = b;
                    coordinates[cc] = c;
                    let pos = BlockPos::new(coordinates[0], coordinates[1], coordinates[2]);
                    let full = filled.contains(&pos);
                    if !last_full && full {
                        faces.push((negative, pos));
                    }
                    if last_full && !full {
                        faces.push((positive, pos.relative(&negative)));
                    }
                    last_full = full;
                }
                // Java scans a relative voxel range including an empty
                // sentinel after its last cell. Emit that final transition
                // without materializing max+1 in absolute coordinates: a
                // valid voxel at i32::MAX still has its positive face there.
                if last_full {
                    let mut coordinates = [0; 3];
                    coordinates[aa] = a;
                    coordinates[bb] = b;
                    coordinates[cc] = max[cc];
                    faces.push((
                        positive,
                        BlockPos::new(coordinates[0], coordinates[1], coordinates[2]),
                    ));
                }
            }
        }
    }
    faces
}

/// The random argument is the REGION's mutable stream, never the feature's.
/// The supported declaring implementations below do not consume random draws.
fn update_shape(
    level: &mut dyn WorldGenLevel,
    _random: &mut AlgorithmRandomSource,
    state: BlockState,
    pos: &BlockPos,
    direction: Direction,
    neighbor: BlockState,
) -> BlockState {
    match update_owner(state) {
        "BlockBehaviour" => state,
        "LeavesBlock" => {
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            let distance = if neighbor.is_in_tag("minecraft:prevents_nearby_leaf_decay") {
                0
            } else if let Some(PropertyValue::Int(distance)) =
                neighbor.get_value(BlockStateProperties::DISTANCE)
            {
                distance
            } else {
                7
            };
            let distance_from_neighbor = distance + 1;
            if distance_from_neighbor != 1
                || state.get_value(BlockStateProperties::DISTANCE)
                    != Some(PropertyValue::Int(distance_from_neighbor))
            {
                level.schedule_block_tick(pos, Block::new(state.block()), 1);
            }
            state
        }
        "VegetationBlock" => {
            if level.can_survive(&state, pos) {
                state
            } else {
                Blocks::AIR.default_block_state()
            }
        }
        "MangroveRootsBlock" => {
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            state
        }
        "MangrovePropaguleBlock" => {
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            if direction == Direction::Up && !level.can_survive(&state, pos) {
                Blocks::AIR.default_block_state()
            } else if !level.can_survive(&state, pos) {
                // SaplingBlock inherits VegetationBlock.updateShape.
                Blocks::AIR.default_block_state()
            } else {
                state
            }
        }
        "SmallDripleafBlock" | "DoublePlantBlock" => {
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            let half = state.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF);
            let lower = half == Some(PropertyValue::Enum("lower"));
            let vertical = direction == Direction::Up || direction == Direction::Down;
            let matching_half = neighbor.block() == state.block()
                && neighbor.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF) != half;
            if (vertical && lower == (direction == Direction::Up) && !matching_half)
                || !level.can_survive(&state, pos)
            {
                Blocks::AIR.default_block_state()
            } else {
                state
            }
        }
        "BigDripleafBlock" => {
            // The support failure returns before the water tick is scheduled.
            if direction == Direction::Down && !level.can_survive(&state, pos) {
                return Blocks::AIR.default_block_state();
            }
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            if direction == Direction::Up && neighbor.block() == state.block() {
                let stem = BlockState::of(
                    rivet_registry::generated::blocks::BlockId::from_name(
                        "minecraft:big_dripleaf_stem",
                    )
                    .expect("registered dripleaf stem"),
                );
                stem.set_value(
                    BlockStateProperties::WATERLOGGED,
                    state
                        .get_value(BlockStateProperties::WATERLOGGED)
                        .expect("dripleaf waterlogged"),
                )
                .expect("stem waterlogged")
                .set_value(
                    BlockStateProperties::HORIZONTAL_FACING,
                    state
                        .get_value(BlockStateProperties::HORIZONTAL_FACING)
                        .expect("dripleaf facing"),
                )
                .expect("stem facing")
            } else {
                state
            }
        }
        "BigDripleafStemBlock" => {
            if matches!(direction, Direction::Down | Direction::Up)
                && !level.can_survive(&state, pos)
            {
                level.schedule_block_tick(pos, Block::new(state.block()), 1);
            }
            if state.get_value(BlockStateProperties::WATERLOGGED) == Some(PropertyValue::Bool(true))
            {
                level.schedule_tick(pos, FluidId::WATER, 5);
            }
            state
        }
        "FallingBlock" => {
            // FallingBlock.updateShape always schedules, independent of the
            // changed face or support. DragonEggBlock is the only pinned
            // getDelayAfterPlace override; falling itself belongs to tick().
            let delay = if state.block().name() == "minecraft:dragon_egg" {
                5
            } else {
                2
            };
            level.schedule_block_tick(pos, Block::new(state.block()), delay);
            state
        }
        "CreakingHeartBlock" => {
            level.schedule_block_tick(pos, Block::new(state.block()), 1);
            state
        }
        "HangingMossBlock" => {
            let above_pos = pos.above();
            let above = level.get_block_state(&above_pos);
            if !level.can_attach_to(&above_pos, &above, &Direction::Down)
                && above.block().name() != "minecraft:pale_hanging_moss"
            {
                level.schedule_block_tick(pos, Block::new(state.block()), 1);
            }
            state
                .set_value(
                    BlockStateProperties::TIP,
                    level.get_block_state(&pos.below()).block().name()
                        != "minecraft:pale_hanging_moss",
                )
                .expect("hanging moss tip")
        }
        "MossyCarpetBlock" => {
            rivet_world::block::mossy_carpet_block::update_shape(level, state, pos)
        }
        "CarpetBlock" => {
            if level.get_block_state(&pos.below()).is_air() {
                Blocks::AIR.default_block_state()
            } else {
                state
            }
        }
        "LiquidBlock" => {
            let own_fluid = FluidId::from_id(state.fluid_id());
            let neighbor_fluid = FluidId::from_id(neighbor.fluid_id());
            let is_source = |fluid| fluid == FluidId::WATER || fluid == FluidId::LAVA;
            if is_source(own_fluid) || is_source(neighbor_fluid) {
                if state.block().name() != "minecraft:water" {
                    panic!(
                        "Block.updateShape is not implemented for lava fluid tick delay without dimension context"
                    );
                }
                level.schedule_tick(pos, own_fluid, 5);
            }
            if direction == Direction::Down
                && own_fluid == FluidId::WATER
                && (neighbor.is_in_tag("minecraft:enables_bubble_column_drag_down")
                    || neighbor.is_in_tag("minecraft:enables_bubble_column_push_up"))
            {
                level.schedule_block_tick(pos, Block::new(state.block()), 20);
            }
            state
        }
        "SnowyBlock" => {
            if direction == Direction::Up {
                state
                    .set_value(
                        BlockStateProperties::SNOWY,
                        neighbor.is_in_tag("minecraft:snow"),
                    )
                    .expect("SnowyBlock has snowy property")
            } else {
                state
            }
        }
        "VineBlock" => {
            if direction == Direction::Down {
                state
            } else {
                updated_vine(state, level, pos)
            }
        }
        "BeehiveBlock" => {
            // Paper re-reads this neighbor and checks specifically FireBlock
            // (not SoulFireBlock) before requesting emergency entity release.
            if level
                .get_block_state(&pos.relative(&direction))
                .block()
                .name()
                == "minecraft:fire"
                && level.has_beehive_block_entity(pos)
            {
                panic!("Block.updateShape is not implemented for beehive emergency bee release");
            }
            state
        }
        "CocoaBlock" => {
            let Some(PropertyValue::Enum(facing)) =
                state.get_value(BlockStateProperties::HORIZONTAL_FACING)
            else {
                unreachable!("cocoa has horizontal facing")
            };
            if direction.get_serialized_name() == facing
                && !level
                    .get_block_state(&pos.relative(&direction))
                    .is_in_tag("minecraft:supports_cocoa")
            {
                Blocks::AIR.default_block_state()
            } else {
                state
            }
        }
        owner => panic!(
            "Block.updateShape is not implemented for {} (declared by {owner})",
            state.block().name()
        ),
    }
}

pub(super) fn update_shape_at_edge(
    level: &mut dyn WorldGenLevel,
    random: &mut AlgorithmRandomSource,
    filled: &HashSet<BlockPos>,
) {
    for (direction, pos) in boundary_faces(filled) {
        let neighbor_pos = pos.relative(&direction);
        let state = level.get_block_state(&pos);
        let neighbor_state = level.get_block_state(&neighbor_pos);
        let new_state = update_shape(level, random, state, &pos, direction, neighbor_state);
        if state != new_state {
            level.set_block(&pos, new_state, 2);
        }
        let new_neighbor = update_shape(
            level,
            random,
            neighbor_state,
            &neighbor_pos,
            direction.get_opposite(),
            new_state,
        );
        if neighbor_state != new_neighbor {
            level.set_block(&neighbor_pos, new_neighbor, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;
    use rivet_world::level::height_accessor::LevelHeightAccessor;

    #[derive(Default)]
    struct Level {
        blocks: HashMap<BlockPos, BlockState>,
        writes: Vec<(BlockPos, BlockState, u32)>,
        ticks: Vec<(BlockPos, &'static str, i32)>,
    }
    impl LevelHeightAccessor for Level {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }
    impl WorldGenLevel for Level {
        fn get_seed(&self) -> i64 {
            42
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
        fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
            if matches!(
                state.block().name(),
                "minecraft:small_dripleaf"
                    | "minecraft:big_dripleaf"
                    | "minecraft:big_dripleaf_stem"
            ) {
                return crate::server::level::world_gen_region::region_can_survive(
                    self, state, pos,
                );
            }
            self.get_block_state(&pos.below())
                .is_in_tag("minecraft:supports_vegetation")
        }
        fn schedule_tick(&mut self, pos: &BlockPos, _: FluidId, delay: i32) {
            self.ticks.push((*pos, "water", delay));
        }
        fn schedule_block_tick(&mut self, pos: &BlockPos, _: Block, delay: i32) {
            self.ticks.push((*pos, "block", delay));
        }
        fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
            matches!(
                self.get_block_state(pos).block().name(),
                "minecraft:bee_nest" | "minecraft:beehive"
            )
        }
    }

    fn random() -> AlgorithmRandomSource {
        AlgorithmRandomSource::Legacy(LegacyRandomSource::new(42))
    }

    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).expect("registered fixture block"))
    }

    #[test]
    fn dripleaf_support_distinguishes_source_water_halves_and_stem_neighbors() {
        let pos = BlockPos::new(0, 64, 0);
        let lower = state("minecraft:small_dripleaf");
        let upper = lower
            .set_value(
                BlockStateProperties::DOUBLE_BLOCK_HALF,
                PropertyValue::Enum("upper"),
            )
            .unwrap();
        let mut level = Level::default();
        level.blocks.insert(pos.below(), state("minecraft:dirt"));
        level.blocks.insert(pos, state("minecraft:water"));
        assert!(level.can_survive(&lower, &pos));
        level.blocks.insert(
            pos,
            state("minecraft:water")
                .set_value(BlockStateProperties::LEVEL, 1)
                .unwrap(),
        );
        assert!(
            !level.can_survive(&lower, &pos),
            "flowing water is not a source"
        );
        level.blocks.insert(pos, lower);
        assert!(!level.can_survive(&lower, &pos));
        level.blocks.insert(pos.below(), state("minecraft:clay"));
        assert!(
            level.can_survive(&lower, &pos),
            "small-dripleaf support needs no water"
        );
        level.blocks.insert(pos.below(), upper);
        assert!(!level.can_survive(&upper, &pos));
        level.blocks.insert(pos.below(), lower);
        assert!(level.can_survive(&upper, &pos));

        let leaf = state("minecraft:big_dripleaf");
        let stem = state("minecraft:big_dripleaf_stem");
        level.blocks.insert(pos.below(), leaf);
        level.blocks.insert(pos.above(), leaf);
        assert!(level.can_survive(&leaf, &pos));
        assert!(
            !level.can_survive(&stem, &pos),
            "a leaf is not stem support"
        );
        level.blocks.insert(pos.below(), stem);
        assert!(level.can_survive(&stem, &pos));
        level.blocks.remove(&pos.above());
        assert!(!level.can_survive(&stem, &pos));
    }

    #[test]
    fn dripleaf_neighbor_updates_preserve_tick_order_and_transition_properties() {
        let pos = BlockPos::new(0, 64, 0);
        let air = Blocks::AIR.default_block_state();
        let leaf = state("minecraft:big_dripleaf")
            .set_value(BlockStateProperties::WATERLOGGED, true)
            .unwrap()
            .set_value(BlockStateProperties::HORIZONTAL_FACING, Direction::East)
            .unwrap();
        let mut level = Level::default();
        assert_eq!(
            update_shape(&mut level, &mut random(), leaf, &pos, Direction::Down, air),
            air
        );
        assert!(
            level.ticks.is_empty(),
            "support failure precedes water ticking"
        );
        let stem = update_shape(&mut level, &mut random(), leaf, &pos, Direction::Up, leaf);
        assert_eq!(stem.block().name(), "minecraft:big_dripleaf_stem");
        assert_eq!(
            stem.get_value(BlockStateProperties::HORIZONTAL_FACING),
            leaf.get_value(BlockStateProperties::HORIZONTAL_FACING)
        );
        assert_eq!(
            stem.get_value(BlockStateProperties::WATERLOGGED),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(level.ticks, [(pos, "water", 5)]);
        level.ticks.clear();
        assert_eq!(
            update_shape(&mut level, &mut random(), stem, &pos, Direction::Up, air),
            stem
        );
        assert_eq!(level.ticks, [(pos, "block", 1), (pos, "water", 5)]);
        level.ticks.clear();
        assert_eq!(
            update_shape(&mut level, &mut random(), stem, &pos, Direction::East, air),
            stem
        );
        assert_eq!(level.ticks, [(pos, "water", 5)]);
        level.ticks.clear();
        let small = state("minecraft:small_dripleaf")
            .set_value(BlockStateProperties::WATERLOGGED, true)
            .unwrap();
        assert_eq!(
            update_shape(&mut level, &mut random(), small, &pos, Direction::Up, air),
            air
        );
        assert_eq!(
            level.ticks,
            [(pos, "water", 5)],
            "small dripleaf ticks before rejecting its paired half"
        );
    }

    #[test]
    fn unchanged_leaves_schedule_fluid_and_block_ticks_inside_then_outside() {
        let origin = BlockPos::new(0, 64, 0);
        let leaves = Blocks::OAK_LEAVES
            .default_block_state()
            .set_value(BlockStateProperties::WATERLOGGED, true)
            .unwrap();
        let mut level = Level::default();
        level.blocks.insert(origin, leaves);
        level.blocks.insert(origin.below(), leaves);
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert!(
            level.writes.is_empty(),
            "unchanged leaf state must not be rewritten"
        );
        assert_eq!(level.ticks.len(), 14);
        // NORTH, SOUTH, DOWN inside updates precede DOWN's outside leaf.
        for pair in level.ticks[..6].chunks_exact(2) {
            assert_eq!(pair, [(origin, "water", 5), (origin, "block", 1)]);
        }
        assert_eq!(
            &level.ticks[6..8],
            [(origin.below(), "water", 5), (origin.below(), "block", 1)]
        );
    }

    #[test]
    fn hive_neighbor_changes_only_require_entity_release_for_actual_fire() {
        let pos = BlockPos::new(0, 64, 0);
        let hive = state("minecraft:bee_nest");
        let mut level = Level::default();
        level.blocks.insert(pos, hive);
        for name in ["minecraft:air", "minecraft:soul_fire"] {
            let neighbor = state(name);
            level
                .blocks
                .insert(pos.relative(&Direction::East), neighbor);
            assert_eq!(
                update_shape(
                    &mut level,
                    &mut random(),
                    hive,
                    &pos,
                    Direction::East,
                    neighbor
                ),
                hive
            );
        }
        level
            .blocks
            .insert(pos.relative(&Direction::East), state("minecraft:fire"));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                update_shape(
                    &mut level,
                    &mut random(),
                    hive,
                    &pos,
                    Direction::East,
                    state("minecraft:air"),
                )
            }))
            .is_err(),
            "the live reread must not silently discard fire-triggered bee release"
        );
    }

    #[test]
    fn unsupported_vegetation_is_removed_and_snowy_top_is_updated_with_flag_two() {
        let origin = BlockPos::new(0, 64, 0);
        let mut level = Level::default();
        level.blocks.insert(origin, state("minecraft:short_grass"));
        level
            .blocks
            .insert(origin.below(), Blocks::STONE.default_block_state());
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert_eq!(
            level.writes,
            [(origin, Blocks::AIR.default_block_state(), 2)]
        );

        level.writes.clear();
        level
            .blocks
            .insert(origin, Blocks::GRASS_BLOCK.default_block_state());
        // Query a face set whose top neighbour is genuine base-behavior SNOW_BLOCK;
        // SNOW has its own still-unavailable override, so preserve that boundary.
        level
            .blocks
            .insert(origin.above(), Blocks::SNOW_BLOCK.default_block_state());
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].2, 2);
        assert_eq!(
            level.writes[0].1.get_value(BlockStateProperties::SNOWY),
            Some(PropertyValue::Bool(true))
        );
    }

    #[test]
    #[should_panic(
        expected = "Block.updateShape is not implemented for minecraft:oak_fence (declared by FenceBlock)"
    )]
    fn an_unported_declaring_override_never_defaults_to_unchanged() {
        let origin = BlockPos::new(0, 64, 0);
        let mut level = Level::default();
        level.blocks.insert(origin, state("minecraft:oak_fence"));
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
    }

    #[test]
    fn vines_inherit_the_face_above_but_cocoa_requires_its_facing_log() {
        let origin = BlockPos::new(0, 64, 0);
        let vine = state("minecraft:vine")
            .set_value(BlockStateProperties::WEST, true)
            .unwrap();
        let mut level = Level::default();
        level.blocks.insert(origin, vine);
        level.blocks.insert(origin.above(), vine);
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert!(
            level.writes.is_empty(),
            "vine above supports the same hanging face"
        );
        level.blocks.remove(&origin.above());
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert_eq!(
            level.get_block_state(&origin),
            Blocks::AIR.default_block_state()
        );

        let cocoa = state("minecraft:cocoa")
            .set_value(BlockStateProperties::HORIZONTAL_FACING, Direction::East)
            .unwrap();
        level.blocks.insert(origin, cocoa);
        level.blocks.insert(
            origin.relative(&Direction::East),
            state("minecraft:jungle_log"),
        );
        level.writes.clear();
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert!(level.writes.is_empty());
        level.blocks.remove(&origin.relative(&Direction::East));
        update_shape_at_edge(&mut level, &mut random(), &HashSet::from([origin]));
        assert_eq!(
            level.get_block_state(&origin),
            Blocks::AIR.default_block_state()
        );
    }

    #[test]
    fn boundary_faces_at_coordinate_limits_keep_positive_faces_without_overflow() {
        for coordinate in [i32::MIN, i32::MAX] {
            let pos = BlockPos::new(coordinate, coordinate, coordinate);
            let faces = boundary_faces(&HashSet::from([pos]));
            assert_eq!(
                faces,
                [
                    (Direction::North, pos),
                    (Direction::South, pos),
                    (Direction::Down, pos),
                    (Direction::Up, pos),
                    (Direction::West, pos),
                    (Direction::East, pos),
                ]
            );
            if coordinate == i32::MAX {
                assert_eq!(faces[5].1.relative(&Direction::East).get_x(), i32::MIN);
            }
        }
    }

    #[test]
    fn boundary_face_order_matches_live_paper() {
        let filled = HashSet::from([
            BlockPos::new(0, 0, 0),
            BlockPos::new(0, 0, 1),
            BlockPos::new(2, 1, 2),
        ]);
        let actual: Vec<_> = boundary_faces(&filled)
            .into_iter()
            .map(|(direction, pos)| {
                format!(
                    "{}\t{}\t{}\t{}",
                    direction.get_serialized_name(),
                    pos.get_x(),
                    pos.get_y(),
                    pos.get_z()
                )
            })
            .collect();
        let expected: Vec<_> =
            include_str!("../../../../../tools/rivet-oracle/fixtures/voxel-edge-faces.tsv")
                .lines()
                .filter(|line| !line.starts_with('#') && !line.is_empty())
                .collect();
        assert_eq!(actual, expected);
    }
}
