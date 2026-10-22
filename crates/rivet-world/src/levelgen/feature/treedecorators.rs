//! Executable tree decorators and their vanilla dispatch codec (Paper 26.2).
//! Context ordering reproduces Java HashSet iteration followed by stable Y-sort.
use super::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec, block_state_provider_get_optional_state,
    block_state_provider_get_state,
};
use crate::level::WorldGenLevel;
use crate::levelgen::heightmap::Types;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::BlockStateProperties,
    core::{BlockPos, Direction, JavaBlockPosSet, Plane, direction_codec},
};
use rivet_serialization::{
    codec::{self, Codec},
    data_result::DataResult,
    dynamic_ops::DynamicOps,
    key_dispatch_codec,
    map_codec::{self, MapCodec},
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::RandomSource;
use std::sync::Arc;
mod attached_to_leaves;
pub use attached_to_leaves::AttachedToLeavesDecorator;
mod pale_moss;
pub use pale_moss::PaleMossDecorator;
mod creaking_heart;
pub use creaking_heart::CreakingHeartDecorator;

#[derive(Debug, Clone)]
pub struct AttachedToLogsDecorator {
    pub probability: f32,
    pub block_provider: Arc<dyn ErasedBlockStateProvider>,
    pub directions: Vec<Direction>,
}
#[derive(Debug, Clone, Copy)]
pub struct TrunkVineDecorator;

#[derive(Debug, Clone, Copy)]
pub struct LeaveVineDecorator {
    pub probability: f32,
}
#[derive(Debug, Clone, Copy)]
pub struct CocoaDecorator {
    pub probability: f32,
}
#[derive(Debug, Clone, Copy)]
pub struct BeehiveDecorator {
    pub probability: f32,
}
#[derive(Debug, Clone)]
pub struct AlterGroundDecorator {
    pub provider: Arc<dyn ErasedBlockStateProvider>,
}

#[derive(Debug, Clone)]
pub struct PlaceOnGroundDecorator {
    pub tries: i32,
    pub radius: i32,
    pub height: i32,
    pub block_state_provider: Arc<dyn ErasedBlockStateProvider>,
}

#[derive(Debug, Clone)]
pub enum TreeDecorator {
    AttachedToLeaves(AttachedToLeavesDecorator),
    AttachedToLogs(AttachedToLogsDecorator),
    TrunkVine(TrunkVineDecorator),
    LeaveVine(LeaveVineDecorator),
    Cocoa(CocoaDecorator),
    Beehive(BeehiveDecorator),
    PaleMoss(PaleMossDecorator),
    CreakingHeart(CreakingHeartDecorator),
    AlterGround(AlterGroundDecorator),
    PlaceOnGround(PlaceOnGroundDecorator),
    /// Known registry identity with an untranslated payload/behavior.
    Unavailable {
        type_name: String,
    },
}
impl TreeDecorator {
    pub fn require_supported(&self) {
        if let Self::Unavailable { type_name } = self {
            panic!("tree decorator {type_name} is not implemented");
        }
    }
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        match self {
            Self::AttachedToLeaves(decorator) => decorator.place(context),
            Self::AttachedToLogs(decorator) => decorator.place(context),
            Self::TrunkVine(decorator) => decorator.place(context),
            Self::LeaveVine(decorator) => decorator.place(context),
            Self::Cocoa(decorator) => decorator.place(context),
            Self::Beehive(decorator) => decorator.place(context),
            Self::PaleMoss(decorator) => decorator.place(context),
            Self::CreakingHeart(decorator) => decorator.place(context),
            Self::AlterGround(decorator) => decorator.place(context),
            Self::PlaceOnGround(decorator) => decorator.place(context),
            Self::Unavailable { type_name } => {
                panic!("tree decorator {type_name} is not implemented")
            }
        }
    }
}
pub struct TreeDecoratorContext<'a, R: RandomSource> {
    pub level: &'a dyn WorldGenLevel,
    pub setter: &'a mut dyn FnMut(&BlockPos, BlockState),
    pub random: &'a mut R,
    pub logs: Vec<BlockPos>,
    pub leaves: Vec<BlockPos>,
    pub roots: Vec<BlockPos>,
    callbacks: TreeDecoratorCallbacks<'a, R>,
}
/// Java shares one mutable world through both reader and mutation callbacks.
/// These callbacks release the read borrow before invoking world mutations.
pub struct TreeDecoratorCallbacks<'a, R: RandomSource> {
    pub height_at: &'a dyn Fn(Types, i32, i32) -> i32,
    pub has_beehive_block_entity: &'a dyn Fn(&BlockPos) -> bool,
    pub store_beehive_occupant: &'a dyn Fn(&BlockPos, i32),
    pub place_configured_feature: &'a dyn Fn(&str, &BlockPos, &mut R) -> bool,
}
impl<'a, R: RandomSource> TreeDecoratorContext<'a, R> {
    pub fn new(
        level: &'a dyn WorldGenLevel,
        setter: &'a mut dyn FnMut(&BlockPos, BlockState),
        random: &'a mut R,
        logs: &[BlockPos],
        leaves: &[BlockPos],
        roots: &[BlockPos],
        callbacks: TreeDecoratorCallbacks<'a, R>,
    ) -> Self {
        fn sorted_positions(insertions: &[BlockPos]) -> Vec<BlockPos> {
            let mut set = JavaBlockPosSet::new();
            for pos in insertions {
                set.try_insert(*pos)
                    .unwrap_or_else(|error| std::panic::panic_any(error));
            }
            let mut positions: Vec<_> = set.iter().copied().collect();
            positions.sort_by_key(BlockPos::get_y);
            positions
        }
        Self {
            level,
            setter,
            random,
            logs: sorted_positions(logs),
            leaves: sorted_positions(leaves),
            roots: sorted_positions(roots),
            callbacks,
        }
    }
    pub fn is_air(&self, pos: &BlockPos) -> bool {
        self.level.get_block_state(pos).is_air()
    }
    pub fn set_block(&mut self, pos: &BlockPos, state: BlockState) {
        (self.setter)(pos, state);
    }
    fn place_configured_feature(&mut self, name: &str, pos: &BlockPos) -> bool {
        (self.callbacks.place_configured_feature)(name, pos, self.random)
    }
    /// TreeFeature.getLowestTrunkOrRootOfTree: roots take precedence unless
    /// their first Y equals the lowest log's Y, when logs come first.
    pub fn lowest_trunk_or_root(&self) -> Vec<BlockPos> {
        if self.roots.is_empty() {
            self.logs.clone()
        } else if self
            .logs
            .first()
            .is_some_and(|log| log.get_y() == self.roots[0].get_y())
        {
            self.logs.iter().chain(&self.roots).copied().collect()
        } else {
            self.roots.clone()
        }
    }
    fn place_vine(&mut self, pos: &BlockPos, attachment_direction: Direction) {
        let property = match attachment_direction {
            Direction::North => BlockStateProperties::NORTH,
            Direction::South => BlockStateProperties::SOUTH,
            Direction::West => BlockStateProperties::WEST,
            Direction::East => BlockStateProperties::EAST,
            _ => panic!("horizontal vine attachment expected"),
        };
        let vine = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:vine")
                .expect("vine registered"),
        )
        .set_value(property, true)
        .expect("vine attachment property");
        self.set_block(pos, vine);
    }
}
fn shuffle<R: RandomSource>(positions: &mut [BlockPos], random: &mut R) {
    for size in (2..=positions.len()).rev() {
        positions.swap(size - 1, random.next_int_bound(size as i32) as usize);
    }
}
impl AttachedToLogsDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let mut logs = context.logs.clone();
        shuffle(&mut logs, context.random);
        for log in logs {
            let direction = self.directions
                [context.random.next_int_bound(self.directions.len() as i32) as usize];
            let pos = log.relative(&direction);
            if context.random.next_float() <= self.probability && context.is_air(&pos) {
                let state = block_state_provider_get_state(
                    self.block_provider.as_ref(),
                    context.level,
                    context.random,
                    &pos,
                );
                context.set_block(&pos, state);
            }
        }
    }
}
impl TrunkVineDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        for log in context.logs.clone() {
            for direction in [
                Direction::West,
                Direction::East,
                Direction::North,
                Direction::South,
            ] {
                if context.random.next_int_bound(3) > 0 {
                    let pos = log.relative(&direction);
                    if context.is_air(&pos) {
                        context.place_vine(&pos, direction.get_opposite());
                    }
                }
            }
        }
    }
}

impl LeaveVineDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        for leaf in context.leaves.clone() {
            for direction in [
                Direction::West,
                Direction::East,
                Direction::North,
                Direction::South,
            ] {
                if context.random.next_float() < self.probability {
                    let mut pos = leaf.relative(&direction);
                    if context.is_air(&pos) {
                        context.place_vine(&pos, direction.get_opposite());
                        pos = pos.below();
                        let mut remaining = 4;
                        while context.is_air(&pos) && remaining > 0 {
                            context.place_vine(&pos, direction.get_opposite());
                            pos = pos.below();
                            remaining -= 1;
                        }
                    }
                }
            }
        }
    }
}
impl CocoaDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        // Java tests probability before checking for an empty log list.
        if context.random.next_float() >= self.probability {
            return;
        }
        let Some(first) = context.logs.first() else {
            return;
        };
        let lowest_y = first.get_y();
        for log in context
            .logs
            .clone()
            .into_iter()
            .filter(|pos| pos.get_y() - lowest_y <= 2)
        {
            for direction in Plane::Horizontal.stream() {
                if context.random.next_float() <= 0.25 {
                    let pos = log.relative(&direction.get_opposite());
                    if context.is_air(&pos) {
                        let state = BlockState::of(
                            rivet_registry::generated::blocks::BlockId::from_name(
                                "minecraft:cocoa",
                            )
                            .expect("cocoa registered"),
                        )
                        .set_value(
                            BlockStateProperties::AGE_2,
                            context.random.next_int_bound(3),
                        )
                        .expect("cocoa age")
                        .set_value(BlockStateProperties::HORIZONTAL_FACING, direction)
                        .expect("cocoa facing");
                        context.set_block(&pos, state);
                    }
                }
            }
        }
    }
}

impl BeehiveDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let Some(first_log) = context.logs.first() else {
            return;
        };
        if context.random.next_float() >= self.probability {
            return;
        }
        let hive_y = if let Some(first_leaf) = context.leaves.first() {
            first_leaf
                .get_y()
                .wrapping_sub(1)
                .max(first_log.get_y().wrapping_add(1))
        } else {
            first_log
                .get_y()
                .wrapping_add(1)
                .wrapping_add(context.random.next_int_bound(3))
                .min(context.logs.last().expect("nonempty logs").get_y())
        };
        // Plane.HORIZONTAL order is N,E,S,W, excluding north (opposite the
        // worldgen south-facing entrance), hence E,S,W for every log.
        let mut placements: Vec<_> = context
            .logs
            .iter()
            .filter(|pos| pos.get_y() == hive_y)
            .flat_map(|pos| {
                [Direction::East, Direction::South, Direction::West].map(|dir| pos.relative(&dir))
            })
            .collect();
        if placements.is_empty() {
            return;
        }
        shuffle(&mut placements, context.random);
        let Some(pos) = placements
            .into_iter()
            .find(|pos| context.is_air(pos) && context.is_air(&pos.relative(&Direction::South)))
        else {
            return;
        };
        let nest = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:bee_nest")
                .expect("bee nest registered"),
        )
        .set_value(BlockStateProperties::HORIZONTAL_FACING, Direction::South)
        .expect("bee nest facing");
        context.set_block(&pos, nest);
        // Java's typed getBlockEntity(..., BEEHIVE).ifPresent gates BOTH the
        // count draw and occupant draws. A placed nest alone is not a hive.
        if (context.callbacks.has_beehive_block_entity)(&pos) {
            let count = 2 + context.random.next_int_bound(2);
            for _ in 0..count {
                let ticks = context.random.next_int_bound(599);
                (context.callbacks.store_beehive_occupant)(&pos, ticks);
            }
        }
    }
}

impl AlterGroundDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let positions = context.lowest_trunk_or_root();
        let Some(first) = positions.first() else {
            return;
        };
        let lowest_y = first.get_y();
        for pos in positions.into_iter().filter(|pos| pos.get_y() == lowest_y) {
            for (x, z) in [(-1, -1), (2, -1), (-1, 2), (2, 2)] {
                self.place_circle(context, &pos.offset(x, 0, z));
            }
            for _ in 0..5 {
                let placement = context.random.next_int_bound(64);
                let x = placement % 8;
                let z = placement / 8;
                if x == 0 || x == 7 || z == 0 || z == 7 {
                    self.place_circle(context, &pos.offset(-3 + x, 0, -3 + z));
                }
            }
        }
    }
    fn place_circle<R: RandomSource>(
        &self,
        context: &mut TreeDecoratorContext<'_, R>,
        pos: &BlockPos,
    ) {
        for x in -2_i32..=2 {
            for z in -2_i32..=2 {
                if x.abs() != 2 || z.abs() != 2 {
                    self.place_block_at(context, &pos.offset(x, 0, z));
                }
            }
        }
    }
    fn place_block_at<R: RandomSource>(
        &self,
        context: &mut TreeDecoratorContext<'_, R>,
        pos: &BlockPos,
    ) {
        for dy in (-3..=2).rev() {
            let cursor = pos.above_steps(dy);
            if let Some(state) = block_state_provider_get_optional_state(
                self.provider.as_ref(),
                context.level,
                context.random,
                &cursor,
            ) {
                context.set_block(&cursor, state);
                break;
            }
            if !context.is_air(&cursor) && dy < 0 {
                break;
            }
        }
    }
}

impl PlaceOnGroundDecorator {
    fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let positions = context.lowest_trunk_or_root();
        let Some(origin) = positions.first() else {
            return;
        };
        let y = origin.get_y();
        let (mut min_x, mut max_x) = (origin.get_x(), origin.get_x());
        let (mut min_z, mut max_z) = (origin.get_z(), origin.get_z());
        for pos in &positions {
            if pos.get_y() == y {
                min_x = min_x.min(pos.get_x());
                max_x = max_x.max(pos.get_x());
                min_z = min_z.min(pos.get_z());
                max_z = max_z.max(pos.get_z());
            }
        }
        // BoundingBox.inflatedBy uses Java int arithmetic, then its
        // constructor normalizes inverted endpoints (including overflow).
        let inflate = |min: i32, max: i32, amount: i32| {
            let a = min.wrapping_sub(amount);
            let b = max.wrapping_add(amount);
            (a.min(b), a.max(b))
        };
        let (min_x, max_x) = inflate(min_x, max_x, self.radius);
        let (min_y, max_y) = inflate(y, y, self.height);
        let (min_z, max_z) = inflate(min_z, max_z, self.radius);
        for _ in 0..self.tries {
            let pos = BlockPos::new(
                context.random.next_int_between_inclusive(min_x, max_x),
                context.random.next_int_between_inclusive(min_y, max_y),
                context.random.next_int_between_inclusive(min_z, max_z),
            );
            let above = pos.above();
            let state = context.level.get_block_state(&above);
            if (state.is_air() || state.block().name() == "minecraft:vine")
                && context.level.get_block_state(&pos).solid_render()
                && (context.callbacks.height_at)(
                    Types::MotionBlockingNoLeaves,
                    pos.get_x(),
                    pos.get_z(),
                ) <= above.get_y()
            {
                let state = block_state_provider_get_state(
                    self.block_state_provider.as_ref(),
                    context.level,
                    context.random,
                    &above,
                );
                context.set_block(&above, state);
            }
        }
    }
}

pub fn tree_decorator_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<TreeDecorator, Ops>> {
    map_codec::codec_of(key_dispatch_codec::dispatch_map(
        "type",
        codec::string_codec::<Ops>(),
        Arc::new(|decorator: &TreeDecorator| match decorator {
            TreeDecorator::AttachedToLeaves(_) => {
                DataResult::success("minecraft:attached_to_leaves".to_string())
            }
            TreeDecorator::AttachedToLogs(_) => {
                DataResult::success("minecraft:attached_to_logs".to_string())
            }
            TreeDecorator::TrunkVine(_) => DataResult::success("minecraft:trunk_vine".to_string()),
            TreeDecorator::LeaveVine(_) => DataResult::success("minecraft:leave_vine".to_string()),
            TreeDecorator::Cocoa(_) => DataResult::success("minecraft:cocoa".to_string()),
            TreeDecorator::Beehive(_) => DataResult::success("minecraft:beehive".to_string()),
            TreeDecorator::PaleMoss(_) => DataResult::success("minecraft:pale_moss".to_string()),
            TreeDecorator::CreakingHeart(_) => {
                DataResult::success("minecraft:creaking_heart".to_string())
            }
            TreeDecorator::AlterGround(_) => {
                DataResult::success("minecraft:alter_ground".to_string())
            }
            TreeDecorator::PlaceOnGround(_) => {
                DataResult::success("minecraft:place_on_ground".to_string())
            }
            TreeDecorator::Unavailable { type_name } => {
                DataResult::error(format!("tree decorator {type_name} is not implemented"))
            }
        }),
        Arc::new(|name: &String| decorator_map_codec::<Ops>(name)),
    ))
}
fn decorator_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    name: &str,
) -> DataResult<Arc<dyn MapCodec<TreeDecorator, Ops>>> {
    let path = name.strip_prefix("minecraft:").unwrap_or(name);
    if name.contains(':') && !name.starts_with("minecraft:") {
        return DataResult::error(format!("unknown tree decorator type {name}"));
    }
    match path {
        "trunk_vine" => DataResult::success(map_codec::unit(TreeDecorator::TrunkVine(
            TrunkVineDecorator,
        ))),
        "leave_vine" => DataResult::success(map_codec::xmap(
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "probability".to_string(),
            ),
            Arc::new(|probability: &f32| {
                TreeDecorator::LeaveVine(LeaveVineDecorator {
                    probability: *probability,
                })
            }),
            Arc::new(|decorator: &TreeDecorator| match decorator {
                TreeDecorator::LeaveVine(d) => d.probability,
                _ => unreachable!("leave vine codec dispatch"),
            }),
        )),
        "cocoa" => DataResult::success(map_codec::xmap(
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "probability".to_string(),
            ),
            Arc::new(|probability: &f32| {
                TreeDecorator::Cocoa(CocoaDecorator {
                    probability: *probability,
                })
            }),
            Arc::new(|decorator: &TreeDecorator| match decorator {
                TreeDecorator::Cocoa(d) => d.probability,
                _ => unreachable!("cocoa codec dispatch"),
            }),
        )),
        "beehive" => DataResult::success(map_codec::xmap(
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "probability".to_string(),
            ),
            Arc::new(|probability: &f32| {
                TreeDecorator::Beehive(BeehiveDecorator {
                    probability: *probability,
                })
            }),
            Arc::new(|decorator: &TreeDecorator| match decorator {
                TreeDecorator::Beehive(d) => d.probability,
                _ => unreachable!("beehive codec dispatch"),
            }),
        )),
        "alter_ground" => DataResult::success(map_codec::xmap(
            codec::field_of(block_state_provider_codec::<Ops>(), "provider".to_string()),
            Arc::new(|provider: &Arc<dyn ErasedBlockStateProvider>| {
                TreeDecorator::AlterGround(AlterGroundDecorator {
                    provider: provider.clone(),
                })
            }),
            Arc::new(|decorator: &TreeDecorator| match decorator {
                TreeDecorator::AlterGround(d) => d.provider.clone(),
                _ => unreachable!("alter ground codec dispatch"),
            }),
        )),
        "attached_to_logs" => {
            let inner = record_builder::map_codec(|i| {
                i.group(RecordCodecBuilder::of(
                    Arc::new(|d: &AttachedToLogsDecorator| d.probability),
                    codec::field_of(
                        codec::float_range::<Ops>(0.0, 1.0),
                        "probability".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &AttachedToLogsDecorator| d.block_provider.clone()),
                    codec::field_of(
                        block_state_provider_codec::<Ops>(),
                        "block_provider".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &AttachedToLogsDecorator| d.directions.clone()),
                    codec::field_of(
                        codec::list_with_range(direction_codec::<Ops>(), 1, i32::MAX),
                        "directions".to_string(),
                    ),
                ))
                .apply(
                    i,
                    Arc::new(
                        |probability, block_provider, directions| AttachedToLogsDecorator {
                            probability,
                            block_provider,
                            directions,
                        },
                    ),
                )
            });
            DataResult::success(map_codec::xmap(
                inner,
                Arc::new(|d: &AttachedToLogsDecorator| TreeDecorator::AttachedToLogs(d.clone())),
                Arc::new(|d: &TreeDecorator| match d {
                    TreeDecorator::AttachedToLogs(d) => d.clone(),
                    _ => unreachable!("attached decorator dispatch"),
                }),
            ))
        }
        "place_on_ground" => {
            let inner = record_builder::map_codec(|i| {
                i.group(RecordCodecBuilder::of(
                    Arc::new(|d: &PlaceOnGroundDecorator| d.tries),
                    codec::optional_field_of("tries", codec::int_range::<Ops>(1, i32::MAX), 128),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &PlaceOnGroundDecorator| d.radius),
                    codec::optional_field_of("radius", codec::int_range::<Ops>(0, i32::MAX), 2),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &PlaceOnGroundDecorator| d.height),
                    codec::optional_field_of("height", codec::int_range::<Ops>(0, i32::MAX), 1),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &PlaceOnGroundDecorator| d.block_state_provider.clone()),
                    codec::field_of(
                        block_state_provider_codec::<Ops>(),
                        "block_state_provider".to_string(),
                    ),
                ))
                .apply(
                    i,
                    Arc::new(|tries, radius, height, block_state_provider| {
                        PlaceOnGroundDecorator {
                            tries,
                            radius,
                            height,
                            block_state_provider,
                        }
                    }),
                )
            });
            DataResult::success(map_codec::xmap(
                inner,
                Arc::new(|d: &PlaceOnGroundDecorator| TreeDecorator::PlaceOnGround(d.clone())),
                Arc::new(|d: &TreeDecorator| match d {
                    TreeDecorator::PlaceOnGround(d) => d.clone(),
                    _ => unreachable!("ground decorator dispatch"),
                }),
            ))
        }
        "attached_to_leaves" => {
            DataResult::success(attached_to_leaves::attached_to_leaves_codec::<Ops>())
        }
        "pale_moss" => DataResult::success(pale_moss::pale_moss_codec::<Ops>()),
        "creaking_heart" => DataResult::success(creaking_heart::creaking_heart_codec::<Ops>()),
        _ => DataResult::error(format!("unknown tree decorator type {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::test_support::{RecordingRandom, RngCall};
    use std::{cell::RefCell, collections::HashMap};

    #[test]
    fn beehive_absent_typed_entity_does_not_consume_occupant_randomness() {
        struct HiveLevel {
            states: RefCell<HashMap<BlockPos, BlockState>>,
            queries: RefCell<usize>,
        }
        impl LevelHeightAccessor for HiveLevel {
            fn get_height(&self) -> i32 {
                384
            }
            fn get_min_y(&self) -> i32 {
                -64
            }
        }
        impl WorldGenLevel for HiveLevel {
            fn get_seed(&self) -> i64 {
                0
            }
            fn get_block_state(&self, pos: &BlockPos) -> BlockState {
                self.states.borrow().get(pos).copied().unwrap_or_else(|| {
                    BlockState::of(
                        rivet_registry::generated::blocks::BlockId::from_name("minecraft:air")
                            .unwrap(),
                    )
                })
            }
            fn set_block(&mut self, pos: &BlockPos, state: BlockState, _: u32) -> bool {
                self.states.borrow_mut().insert(*pos, state);
                true
            }
            fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
                assert_eq!(
                    self.get_block_state(pos).block().name(),
                    "minecraft:bee_nest",
                    "typed query follows the actual nest write"
                );
                *self.queries.borrow_mut() += 1;
                false
            }
        }
        let mut level = HiveLevel {
            states: RefCell::new(HashMap::new()),
            queries: RefCell::new(0),
        };
        let mut random = RecordingRandom::new(42);
        {
            let shared = super::super::tree_feature::TreeLevel(RefCell::new(&mut level));
            let mut setter = |pos: &BlockPos, state| shared.write(pos, state);
            let height_at = |_, _, _| panic!("beehive decorator does not query heightmaps");
            let has_hive = |pos: &BlockPos| shared.0.borrow_mut().has_beehive_block_entity(pos);
            let store = |_: &BlockPos, _: i32| panic!("missing typed entity must not receive bees");
            let nested = |_: &str, _: &BlockPos, _: &mut RecordingRandom| {
                panic!("beehive decorator does not place nested features")
            };
            let mut context = TreeDecoratorContext::new(
                &shared,
                &mut setter,
                &mut random,
                &[BlockPos::new(0, 64, 0), BlockPos::new(0, 65, 0)],
                &[BlockPos::new(0, 66, 0)],
                &[],
                TreeDecoratorCallbacks {
                    height_at: &height_at,
                    has_beehive_block_entity: &has_hive,
                    store_beehive_occupant: &store,
                    place_configured_feature: &nested,
                },
            );
            BeehiveDecorator { probability: 1.0 }.place(&mut context);
        }
        assert_eq!(*level.queries.borrow(), 1);
        assert_eq!(
            random.calls,
            [RngCall::Float, RngCall::IntBound(3), RngCall::IntBound(2)]
        );
    }
}
