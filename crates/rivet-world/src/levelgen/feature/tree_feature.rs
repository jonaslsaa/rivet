//! Port of TreeFeature's placement, clearance and leaf-distance algorithms.
//! Source: Paper's pinned decompileJar.jar, TreeFeature.java (26.2).
//! Roots and supported decorators run against immediate world writes.
//! Untranslated decorators remain explicit capability boundaries, not no-ops.

use super::configurations::TreeConfiguration;
use super::foliageplacers::foliage_placer::FoliageSetter;
use super::treedecorators::{TreeDecoratorCallbacks, TreeDecoratorContext};
use super::{FeatureBehavior, FeaturePlaceContext};
use crate::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
#[path = "tree_placer_runtime.rs"]
mod runtime;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::{BlockPos, Direction, JavaBlockPosSet};
use rivet_util::RandomSource;
use std::cell::RefCell;
use std::collections::HashSet;

pub fn valid_tree_pos(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.is_state_at_position(pos, &|state: &BlockState| {
        state.is_air() || state.is_in_tag("minecraft:replaceable_by_trees")
    })
}
pub fn is_air_or_leaves(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.is_state_at_position(pos, &|state: &BlockState| {
        state.is_air() || state.is_in_tag("minecraft:leaves")
    })
}
pub fn is_vine(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.get_block_state(pos).block()
        == rivet_registry::generated::blocks::BlockId::from_name("minecraft:vine")
            .expect("vine registered")
}

/// Immediate read/write sharing for Java's setter callbacks. Each borrow is
/// released before invoking a provider/placer, so their reads see prior writes.
/// It deliberately forwards the read seams reached by trunk/foliage providers.
pub(super) struct TreeLevel<'a>(pub RefCell<&'a mut dyn WorldGenLevel>);
impl LevelHeightAccessor for TreeLevel<'_> {
    fn get_height(&self) -> i32 {
        self.0.borrow().get_height()
    }
    fn get_min_y(&self) -> i32 {
        self.0.borrow().get_min_y()
    }
}
impl WorldGenLevel for TreeLevel<'_> {
    fn get_seed(&self) -> i64 {
        self.0.borrow().get_seed()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.0.borrow().get_block_state(pos)
    }
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        self.0.borrow().ensure_can_write(pos)
    }
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.0.borrow().is_empty_block(pos)
    }
    fn get_biome(
        &self,
        pos: &BlockPos,
    ) -> rivet_registry::Holder<rivet_registry::biome_id::BiomeId> {
        self.0.borrow().get_biome(pos)
    }
    fn registry_access(&self) -> rivet_registry::access::RegistryAccess {
        self.0.borrow().registry_access()
    }
    fn get_sea_level(&self) -> i32 {
        self.0.borrow().get_sea_level()
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        self.0.borrow().can_survive(state, pos)
    }
    fn is_face_sturdy(&self, pos: &BlockPos, state: &BlockState, direction: &Direction) -> bool {
        self.0.borrow().is_face_sturdy(pos, state, direction)
    }
}
impl TreeLevel<'_> {
    pub(super) fn write(&self, pos: &BlockPos, state: BlockState) {
        self.0.borrow_mut().set_block(pos, state, 19);
    }
}
struct Leaves<'a, 'b> {
    level: &'a TreeLevel<'b>,
    positions: HashSet<BlockPos>,
    insertions: Vec<BlockPos>,
}
impl FoliageSetter for Leaves<'_, '_> {
    fn set(&mut self, pos: &BlockPos, state: BlockState) {
        self.positions.insert(*pos);
        self.insertions.push(*pos);
        self.level.write(pos, state);
    }
    fn is_set(&self, pos: &BlockPos) -> bool {
        self.positions.contains(pos)
    }
}

#[derive(Debug)]
pub struct TreeFeature;
pub const TREE: TreeFeature = TreeFeature;

impl FeatureBehavior<TreeConfiguration> for TreeFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, TreeConfiguration, R>,
    ) -> bool {
        let config = context.config;
        config.require_supported_extensions();
        let random = &mut *context.random;
        let origin = *context.origin;
        let level = TreeLevel(RefCell::new(&mut *context.level));
        let height = runtime::tree_height(config, random);
        let foliage_height = runtime::foliage_height(config, random, height);
        let leaf_radius = runtime::foliage_radius(config, random, height - foliage_height);
        let trunk_origin = config
            .root_placer
            .as_ref()
            .map(|placer| placer.get_trunk_origin(&origin, random))
            .unwrap_or(origin);
        let min_y = origin.get_y().min(trunk_origin.get_y());
        let max_y = origin
            .get_y()
            .max(trunk_origin.get_y())
            .wrapping_add(height)
            .wrapping_add(1);
        if min_y < level.get_min_y().wrapping_add(1) || max_y > level.get_max_y().wrapping_add(1) {
            return false;
        }
        let clipped_height = get_max_free_tree_height(&level, height, &trunk_origin, config);
        if clipped_height < height
            && !config
                .minimum_size
                .min_clipped_height()
                .is_some_and(|min| clipped_height >= min)
        {
            return false;
        }
        let mut roots = HashSet::new();
        let mut root_insertions = Vec::new();
        if let Some(placer) = &config.root_placer {
            let mut setter = |pos: &BlockPos, state: BlockState| {
                roots.insert(*pos);
                root_insertions.push(*pos);
                level.write(pos, state);
            };
            if !placer.place_roots(&level, &mut setter, random, &origin, &trunk_origin) {
                return false;
            }
        }
        let mut trunks = HashSet::new();
        let mut trunk_insertions = Vec::new();
        let mut trunk_setter = |pos: &BlockPos, state: BlockState| {
            trunks.insert(*pos);
            trunk_insertions.push(*pos);
            level.write(pos, state);
        };
        let attachments = runtime::place_trunk(
            config,
            &level,
            &mut trunk_setter,
            random,
            clipped_height,
            &trunk_origin,
        );
        let mut foliage = Leaves {
            level: &level,
            positions: HashSet::new(),
            insertions: Vec::new(),
        };
        for attachment in attachments {
            runtime::create_foliage(
                config,
                &level,
                &mut foliage,
                random,
                clipped_height,
                &attachment,
                foliage_height,
                leaf_radius,
            );
        }
        if trunks.is_empty() && foliage.positions.is_empty() {
            return false;
        }
        let mut decorations = HashSet::new();
        if !config.decorators.is_empty() {
            let mut setter = |pos: &BlockPos, state: BlockState| {
                decorations.insert(*pos);
                level.write(pos, state);
            };
            let height_at = |ty, x, z| level.0.borrow_mut().get_height_at(ty, x, z);
            let has_beehive_block_entity =
                |pos: &BlockPos| level.0.borrow_mut().has_beehive_block_entity(pos);
            let store_beehive_occupant = |pos: &BlockPos, ticks| {
                level.0.borrow_mut().store_beehive_occupant(pos, ticks);
            };
            let mut decorator_context = TreeDecoratorContext::new(
                &level,
                &mut setter,
                random,
                &trunk_insertions,
                &foliage.insertions,
                &root_insertions,
                TreeDecoratorCallbacks {
                    height_at: &height_at,
                    has_beehive_block_entity: &has_beehive_block_entity,
                    store_beehive_occupant: &store_beehive_occupant,
                },
            );
            for decorator in &config.decorators {
                decorator.place(&mut decorator_context);
            }
        }
        let mut bounds_positions: HashSet<_> = trunks.union(&foliage.positions).copied().collect();
        bounds_positions.extend(&decorations);
        bounds_positions.extend(&roots);
        let shape = update_leaves(
            &level,
            &bounds_positions,
            &trunk_insertions,
            &decorations,
            &roots,
        );
        level.0.borrow_mut().update_tree_shape_at_edge(&shape);
        true
    }
}

/// The size provider controls clearance at every height, including the extra
/// layer above the requested tree. Obstruction clips to y - 2, not y - 1.
pub fn get_max_free_tree_height(
    level: &dyn WorldGenLevel,
    height: i32,
    origin: &BlockPos,
    config: &TreeConfiguration,
) -> i32 {
    for y in 0..=height + 1 {
        let radius = config.minimum_size.get_size_at_height(height, y);
        for x in -radius..=radius {
            for z in -radius..=radius {
                let pos = origin.offset(x, y, z);
                if !runtime::is_free(config, level, &pos)
                    || (!config.ignore_vines && is_vine(level, &pos))
                {
                    return y - 2;
                }
            }
        }
    }
    height
}

/// Returns the filled voxel shape in absolute coordinates. Unlike a simple
/// breadth-first walk this honors an existing leaf distance smaller than the
/// current frontier, exactly as Paper's seven priority buckets do.
fn update_leaves(
    level: &TreeLevel<'_>,
    bounds_positions: &HashSet<BlockPos>,
    logs: &[BlockPos],
    decorations: &HashSet<BlockPos>,
    roots: &HashSet<BlockPos>,
) -> HashSet<BlockPos> {
    let min_x = bounds_positions
        .iter()
        .map(|p| p.get_x())
        .min()
        .expect("nonempty bounds");
    let min_y = bounds_positions.iter().map(|p| p.get_y()).min().unwrap();
    let min_z = bounds_positions.iter().map(|p| p.get_z()).min().unwrap();
    let max_x = bounds_positions.iter().map(|p| p.get_x()).max().unwrap();
    let max_y = bounds_positions.iter().map(|p| p.get_y()).max().unwrap();
    let max_z = bounds_positions.iter().map(|p| p.get_z()).max().unwrap();
    let inside = |p: &BlockPos| {
        p.get_x() >= min_x
            && p.get_x() <= max_x
            && p.get_y() >= min_y
            && p.get_y() <= max_y
            && p.get_z() >= min_z
            && p.get_z() <= max_z
    };
    let mut shape: HashSet<_> = decorations
        .union(roots)
        .filter(|p| inside(p))
        .copied()
        .collect();
    let mut log_set = JavaBlockPosSet::new();
    for pos in logs {
        log_set
            .try_insert(*pos)
            .unwrap_or_else(|error| std::panic::panic_any(error));
    }
    let mut to_check: [JavaBlockPosSet; 7] = std::array::from_fn(|_| JavaBlockPosSet::new());
    for pos in log_set.iter() {
        to_check[0]
            .try_insert(*pos)
            .unwrap_or_else(|error| std::panic::panic_any(error));
    }
    let mut smallest = 0;
    while smallest < 7 {
        let Some(pos) = to_check[smallest].pop_first() else {
            smallest += 1;
            continue;
        };
        if !inside(&pos) {
            continue;
        }
        if smallest != 0 {
            let state = level
                .get_block_state(&pos)
                .set_value(BlockStateProperties::DISTANCE, smallest as i32)
                .expect("leaf distance property");
            level.write(&pos, state);
        }
        shape.insert(pos);
        for direction in Direction::VALUES {
            let neighbor = pos.relative(&direction);
            if !inside(&neighbor) || shape.contains(&neighbor) {
                continue;
            }
            let state = level.get_block_state(&neighbor);
            let distance = if state.is_in_tag("minecraft:prevents_nearby_leaf_decay") {
                Some(0)
            } else {
                match state.get_value(BlockStateProperties::DISTANCE) {
                    Some(PropertyValue::Int(n)) => Some(n),
                    _ => None,
                }
            };
            if let Some(distance) = distance {
                let next = distance.min(smallest as i32 + 1) as usize;
                if next < 7 {
                    to_check[next]
                        .try_insert(neighbor)
                        .unwrap_or_else(|error| std::panic::panic_any(error));
                    smallest = smallest.min(next);
                }
            }
        }
    }
    shape
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::featuresize::TwoLayersFeatureSize;
    use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
    use crate::levelgen::feature::test_support::TestGenerator;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;
    use std::{collections::HashMap, sync::Arc};

    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).expect("registered fixture block"))
    }
    #[derive(Default)]
    struct TestLevel {
        blocks: HashMap<BlockPos, BlockState>,
        writes: Vec<(BlockPos, BlockState, u32)>,
        edge_shape: Option<HashSet<BlockPos>>,
    }
    impl LevelHeightAccessor for TestLevel {
        fn get_min_y(&self) -> i32 {
            -64
        }
        fn get_height(&self) -> i32 {
            384
        }
    }
    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.blocks
                .get(pos)
                .copied()
                .unwrap_or_else(|| state("minecraft:air"))
        }
        fn is_empty_block(&self, pos: &BlockPos) -> bool {
            self.get_block_state(pos).is_air()
        }
        fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
            self.blocks.insert(*pos, state);
            self.writes.push((*pos, state, flags));
            true
        }
        fn update_tree_shape_at_edge(&mut self, filled: &HashSet<BlockPos>) {
            self.edge_shape = Some(filled.clone());
        }
    }

    #[test]
    fn tree_clearance_matches_pinned_paper_private_method_fixtures() {
        for row in PAPER_TREE_CORE
            .lines()
            .filter(|line| line.starts_with("clearance\t"))
        {
            let fields: Vec<_> = row.split('\t').collect();
            let [_, name, height, x, y, z, block, ignore_vines, expected] = fields.as_slice()
            else {
                panic!("invalid Java clearance fixture: {row}");
            };
            let mut config = TreeConfiguration::stub();
            config.minimum_size = Arc::new(TwoLayersFeatureSize::new(1, 0, 1));
            config.ignore_vines = ignore_vines.parse().unwrap();
            let mut level = TestLevel::default();
            let origin = BlockPos::new(0, 1, 0);
            level.blocks.insert(
                origin.offset(x.parse().unwrap(), y.parse().unwrap(), z.parse().unwrap()),
                state(&format!("minecraft:{block}")),
            );
            assert_eq!(
                get_max_free_tree_height(&level, height.parse().unwrap(), &origin, &config),
                expected.parse::<i32>().unwrap(),
                "Paper clearance case {name}",
            );
        }
    }

    const PAPER_TREE_CORE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/rivet-oracle/fixtures/tree-core.tsv"
    ));

    #[test]
    fn tree_leaf_distances_and_filled_shape_match_pinned_paper_private_method_fixtures() {
        for row in PAPER_TREE_CORE
            .lines()
            .filter(|line| line.starts_with("leaves\t"))
        {
            let fields: Vec<_> = row.split('\t').collect();
            let [_, name, max_x, distances, filled_x] = fields.as_slice() else {
                panic!("invalid Java leaf fixture: {row}");
            };
            let pos = |x| BlockPos::new(x, 1, 0);
            let mut level = TestLevel::default();
            level.blocks.insert(pos(0), state("minecraft:oak_log"));
            for x in 1..=8 {
                level.blocks.insert(pos(x), state("minecraft:oak_leaves"));
            }
            match *name {
                "priority" => {
                    level.blocks.insert(
                        pos(2),
                        state("minecraft:oak_leaves")
                            .set_value(BlockStateProperties::DISTANCE, 1)
                            .unwrap(),
                    );
                }
                "extra_log" => {
                    level.blocks.insert(pos(2), state("minecraft:oak_log"));
                }
                "decoration" | "root" => {
                    level.blocks.insert(pos(2), state("minecraft:stone"));
                }
                "normal" | "clipped" => {}
                _ => panic!("unknown Java leaf fixture setup: {name}"),
            }
            let decorations = if *name == "decoration" {
                HashSet::from([pos(2)])
            } else {
                HashSet::new()
            };
            let roots = if *name == "root" {
                HashSet::from([pos(2)])
            } else {
                HashSet::new()
            };
            let filled = update_leaves(
                &TreeLevel(RefCell::new(&mut level)),
                &HashSet::from([pos(0), pos(max_x.parse().unwrap())]),
                &[pos(0)],
                &decorations,
                &roots,
            );
            let expected_filled: HashSet<_> = filled_x
                .split(',')
                .map(|x| pos(x.parse().unwrap()))
                .collect();
            assert_eq!(filled, expected_filled, "Paper filled shape case {name}");
            for (x, expected) in distances.split(',').enumerate() {
                let expected = if expected == "-" {
                    None
                } else {
                    Some(PropertyValue::Int(expected.parse().unwrap()))
                };
                assert_eq!(
                    level
                        .get_block_state(&pos(x as i32))
                        .get_value(BlockStateProperties::DISTANCE),
                    expected,
                    "Paper leaf distance case {name}, x={x}"
                );
            }
            assert!(
                level.writes.iter().all(|(_, _, flags)| *flags == 19),
                "Paper write flags case {name}"
            );
        }
    }

    #[test]
    fn tree_places_real_trunk_foliage_and_passes_filled_shape_to_world() {
        let mut config = TreeConfiguration::stub();
        config.trunk_provider = Arc::new(SimpleStateProvider::new(state("minecraft:oak_log")));
        config.foliage_provider = Arc::new(SimpleStateProvider::new(state("minecraft:oak_leaves")));
        config.below_trunk_provider = Arc::new(SimpleStateProvider::new(state("minecraft:dirt")));
        let mut level = TestLevel::default();
        let origin = BlockPos::new(0, 1, 0);
        let mut random = LegacyRandomSource::new(42);
        assert!(TREE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &TestGenerator,
            &mut random,
            &origin,
            &config
        )));
        assert_eq!(
            level.get_block_state(&origin).block(),
            state("minecraft:oak_log").block()
        );
        assert_eq!(
            level.get_block_state(&origin.below()),
            state("minecraft:dirt")
        );
        assert!(
            level
                .blocks
                .values()
                .any(|s| s.block() == state("minecraft:oak_leaves").block())
        );
        let shape = level
            .edge_shape
            .as_ref()
            .expect("neighbor edge update called");
        assert!(shape.contains(&origin));
        assert!(level.writes.iter().all(|(_, _, flags)| *flags == 19));
    }
}
