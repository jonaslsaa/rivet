//! CollectingNeighborUpdater and the native live block callback dispatcher.
//! The retained stack is borrowed only between callbacks: reentrant setBlock
//! calls enqueue into the same added-this-layer list, never another updater.
use super::{GenerationPostProcessError, LiveBlockEffect, LiveBlockMutationError, ServerLevel};
use crate::server::level::{
    block_shape_update::update_shape_live, fluid_runtime::FluidRuntimeServices,
    world_gen_region::region_can_survive,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos, Direction},
    fluid_id::FluidId,
};
use rivet_world::{
    block::Block,
    level::{WorldGenLevel, height_accessor::LevelHeightAccessor},
    ticks::ScheduledTick,
};

#[cfg(test)]
#[path = "level_neighbor_updates_test.rs"]
mod tests;

type Result<T> = std::result::Result<T, LiveBlockMutationError>;

const UPDATE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];
const SHAPE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::North,
    Direction::South,
    Direction::Down,
    Direction::Up,
];

#[derive(Clone)]
enum Update {
    Shape {
        direction: Direction,
        neighbor: BlockState,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        flags: u32,
        limit: i32,
    },
    Simple {
        pos: BlockPos,
        source: Block,
    },
    Full {
        pos: BlockPos,
        state: BlockState,
        source: Block,
        moved: bool,
    },
    Multi {
        pos: BlockPos,
        source: Block,
        skip: Option<Direction>,
        index: usize,
    },
}
impl Update {
    fn positions(&self) -> Vec<BlockPos> {
        match self {
            Self::Shape { pos, .. } | Self::Simple { pos, .. } | Self::Full { pos, .. } => {
                vec![*pos]
            }
            Self::Multi { pos, skip, .. } => UPDATE_ORDER
                .into_iter()
                .filter(|d| Some(*d) != *skip)
                .map(|d| pos.relative(&d))
                .collect(),
        }
    }
}

/// Exact source count/stack/addedThisLayer value. The tick owner supplies the
/// actual max-chained-neighbor-updates server property; no private defaults.
pub struct LiveNeighborUpdater {
    max: i32,
    count: i32,
    stack: Vec<Update>,
    added: Vec<Update>,
    debug_listener: bool,
}
impl LiveNeighborUpdater {
    pub fn new(max_chained_updates: i32) -> Self {
        Self {
            max: max_chained_updates,
            count: 0,
            stack: Vec::new(),
            added: Vec::new(),
            debug_listener: false,
        }
    }
    /// Called at the source debug-synchronizer tick point. Initial false is
    /// the actual constructor's null listener, not a disabled debug policy.
    pub fn set_debug_listener_enabled(&mut self, enabled: bool) {
        self.debug_listener = enabled;
    }
    fn enqueue(&mut self, pos: BlockPos, update: Update) -> bool {
        let running = self.count > 0;
        let too_many = self.max >= 0 && self.count >= self.max;
        self.count = self.count.wrapping_add(1);
        if !too_many {
            if running {
                self.added.push(update);
            } else {
                self.stack.push(update);
            }
        } else if self.count.wrapping_sub(1) == self.max {
            tracing::error!(
                ?pos,
                "Too many chained neighbor updates. Skipping the rest. First skipped position"
            );
        }
        !running
    }
    fn clear(&mut self) {
        self.stack.clear();
        self.added.clear();
        self.count = 0;
    }
}

pub trait LiveNeighborServices: FluidRuntimeServices {
    fn neighbor_updater(&mut self) -> &mut LiveNeighborUpdater;
    /// The actual enabled feature set. Experimental orientation construction
    /// is an explicit boundary until its random/orientation runtime is ported.
    fn redstone_experiments_enabled(&self) -> bool;
    /// True means the real BlockPhysicsEvent cancelled neighborChanged.
    fn neighbor_physics_event(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        source_pos: BlockPos,
    ) -> Result<bool>;
    fn neighbor_debug_update(
        &mut self,
        level: &mut ServerLevel,
        positions: &[BlockPos],
    ) -> Result<()>;
    /// Real Level.destroyBlock (BlockDestroyEvent, drops/xp, level/game events
    /// and fluid replacement). Shape-to-air must not silently use setBlock.
    fn neighbor_destroy_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        drop_resources: bool,
        limit: i32,
    ) -> Result<bool>;
}

fn unavailable(message: impl std::fmt::Display) -> LiveBlockMutationError {
    LiveBlockMutationError::Unavailable(message.to_string())
}
fn add_and_run<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    update: Update,
) -> Result<()> {
    if !services.neighbor_updater().enqueue(pos, update) {
        return Ok(());
    }
    drain_and_clear(&mut NativeDriver { level, services })
}
trait NeighborDriver {
    fn updater(&mut self) -> &mut LiveNeighborUpdater;
    fn debug(&mut self, positions: &[BlockPos]) -> Result<()>;
    fn execute(&mut self, update: Update) -> Result<()>;
}
struct NativeDriver<'a, S> {
    level: &'a mut ServerLevel,
    services: &'a mut S,
}
impl<S: LiveNeighborServices> NeighborDriver for NativeDriver<'_, S> {
    fn updater(&mut self) -> &mut LiveNeighborUpdater {
        self.services.neighbor_updater()
    }
    fn debug(&mut self, positions: &[BlockPos]) -> Result<()> {
        self.services.neighbor_debug_update(self.level, positions)
    }
    fn execute(&mut self, update: Update) -> Result<()> {
        execute(self.level, self.services, update)
    }
}
fn drain_and_clear(driver: &mut impl NeighborDriver) -> Result<()> {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_updates(driver)));
    // Source finally only clears scheduling state. Prior canonical effects
    // remain; neither an error nor a panic rolls terrain/RNG/events back.
    driver.updater().clear();
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
fn run_updates(driver: &mut impl NeighborDriver) -> Result<()> {
    loop {
        let (debug, positions) = {
            let updater = driver.updater();
            if updater.stack.is_empty() && updater.added.is_empty() {
                return Ok(());
            }
            for update in std::mem::take(&mut updater.added).into_iter().rev() {
                updater.stack.push(update);
            }
            (
                updater.debug_listener,
                updater
                    .stack
                    .last()
                    .expect("source pending stack")
                    .positions(),
            )
        };
        if debug {
            driver.debug(&positions)?;
        }
        while driver.updater().added.is_empty() {
            let (update, finished) = {
                let next = driver
                    .updater()
                    .stack
                    .last_mut()
                    .expect("source stack retained while callbacks run");
                if let Update::Multi {
                    pos,
                    source,
                    skip,
                    index,
                } = next
                {
                    let direction = UPDATE_ORDER[*index];
                    *index += 1;
                    if *index < 6 && Some(UPDATE_ORDER[*index]) == *skip {
                        *index += 1;
                    }
                    (
                        Update::Multi {
                            pos: *pos,
                            source: *source,
                            skip: Some(direction),
                            index: 0,
                        },
                        *index >= 6,
                    )
                } else {
                    (next.clone(), true)
                }
            };
            driver.execute(update)?;
            if finished {
                driver.updater().stack.pop();
                break;
            }
        }
    }
}
fn execute<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    update: Update,
) -> Result<()> {
    match update {
        Update::Shape {
            direction,
            neighbor,
            pos,
            neighbor_pos,
            flags,
            limit,
        } => {
            let current = read(level, pos)?;
            if flags & 128 != 0 && current.block().name() == "minecraft:redstone_wire" {
                return Ok(());
            }
            if neighbor_pos != pos.relative(&direction) {
                return Err(unavailable(
                    "nonadjacent neighbor shape position is not supported by the translated shape bodies",
                ));
            }
            let new = if matches!(current.block().name(), "minecraft:water" | "minecraft:lava") {
                liquid_shape(level, services, pos, current, direction, neighbor)?
            } else {
                with_world(level, services, |world| {
                    update_shape_live(world, current, &pos, direction, neighbor)
                })?
            };
            if new != current {
                if new.is_air() {
                    services.neighbor_destroy_block(level, pos, flags & 32 == 0, limit)?;
                } else {
                    level.set_block_live(pos, new, flags & !32, limit, services)?;
                }
            }
        }
        Update::Simple { pos, source } => {
            let state = read(level, pos)?;
            execute_neighbor(level, services, pos, state, source, false, pos)?;
        }
        Update::Full {
            pos,
            state,
            source,
            moved,
        } => execute_neighbor(level, services, pos, state, source, moved, pos)?,
        Update::Multi {
            pos,
            source,
            skip: Some(direction),
            ..
        } => {
            let target = pos.relative(&direction);
            let state = read(level, target)?;
            execute_neighbor(level, services, target, state, source, false, pos)?;
        }
        Update::Multi { skip: None, .. } => {
            unreachable!("execution step carries current direction")
        }
    }
    Ok(())
}
fn execute_neighbor<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    state: BlockState,
    _source: Block,
    _moved: bool,
    source_pos: BlockPos,
) -> Result<()> {
    if services.neighbor_physics_event(level, pos, state, source_pos)? {
        return Ok(());
    }
    match super::block_mutation::callback_owner(state, 4) {
        "BlockBehaviour" => Ok(()),
        "LiquidBlock" => level
            .liquid_block_changed_live(pos, state, services)
            .map_err(unavailable),
        owner => Err(unavailable(format!(
            "actual {owner}.neighborChanged for {}",
            state.block().name()
        ))),
    }
}

/// Execute native callback bodies. A returned effect is a genuinely external
/// packet/navigation/debug/drop operation for the owning concrete services;
/// unsupported native block bodies are errors, never delegated as fake no-ops.
pub fn dispatch_live_block_effect<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    effect: LiveBlockEffect,
) -> Result<Option<LiveBlockEffect>> {
    match effect {
        LiveBlockEffect::OnPlace { pos, state, .. } => {
            match super::block_mutation::callback_owner(state, 0) {
                "BlockBehaviour" => services.check_block_place_thread()?,
                "LiquidBlock" => level
                    .liquid_block_changed_live(pos, state, services)
                    .map_err(unavailable)?,
                "FallingBlock" => schedule_block(
                    level,
                    pos,
                    Block::new(state.block()),
                    if state.block().name() == "minecraft:dragon_egg" {
                        5
                    } else {
                        2
                    },
                ),
                owner => {
                    return Err(unavailable(format!(
                        "actual {owner}.onPlace for {}",
                        state.block().name()
                    )));
                }
            }
        }
        LiveBlockEffect::AfterRemoval { old, .. } => {
            if super::block_mutation::callback_owner(old, 1) != "BlockBehaviour" {
                return Err(unavailable(format!(
                    "actual {}.affectNeighborsAfterRemoval",
                    old.block().name()
                )));
            }
        }
        LiveBlockEffect::Neighbors { pos, old } => {
            if services.redstone_experiments_enabled() {
                return Err(unavailable(
                    "experimental redstone orientation construction",
                ));
            }
            add_and_run(
                level,
                services,
                pos,
                Update::Multi {
                    pos,
                    source: Block::new(old.block()),
                    skip: None,
                    index: 0,
                },
            )?;
        }
        LiveBlockEffect::NeighborShapes {
            pos,
            state,
            flags,
            limit,
        } => {
            for direction in SHAPE_ORDER {
                let target = pos.relative(&direction);
                add_and_run(
                    level,
                    services,
                    target,
                    Update::Shape {
                        direction: direction.get_opposite(),
                        neighbor: state,
                        pos: target,
                        neighbor_pos: pos,
                        flags,
                        limit,
                    },
                )?;
            }
        }
        LiveBlockEffect::IndirectShapes { state, .. } => {
            if super::block_mutation::callback_owner(state, 3) != "BlockBehaviour" {
                return Err(unavailable(format!(
                    "actual {}.updateIndirectNeighbourShapes",
                    state.block().name()
                )));
            }
        }
        external => return Ok(Some(external)),
    }
    Ok(None)
}

impl ServerLevel {
    pub fn neighbor_changed_live<S: LiveNeighborServices>(
        &mut self,
        pos: BlockPos,
        source: Block,
        services: &mut S,
    ) -> Result<()> {
        add_and_run(self, services, pos, Update::Simple { pos, source })
    }
    pub fn neighbor_changed_state_live<S: LiveNeighborServices>(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        source: Block,
        moved: bool,
        services: &mut S,
    ) -> Result<()> {
        add_and_run(
            self,
            services,
            pos,
            Update::Full {
                pos,
                state,
                source,
                moved,
            },
        )
    }
    // Keep the source neighbour-update arguments explicit across this boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn neighbor_shape_changed_live<S: LiveNeighborServices>(
        &mut self,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
        flags: u32,
        limit: i32,
        services: &mut S,
    ) -> Result<()> {
        add_and_run(
            self,
            services,
            pos,
            Update::Shape {
                direction,
                neighbor,
                pos,
                neighbor_pos,
                flags,
                limit,
            },
        )
    }
    pub fn neighbors_except_live<S: LiveNeighborServices>(
        &mut self,
        pos: BlockPos,
        source: Block,
        skip: Option<Direction>,
        services: &mut S,
    ) -> Result<()> {
        if services.redstone_experiments_enabled() {
            return Err(unavailable(
                "experimental redstone orientation construction",
            ));
        }
        let index = usize::from(skip == Some(UPDATE_ORDER[0]));
        add_and_run(
            self,
            services,
            pos,
            Update::Multi {
                pos,
                source,
                skip,
                index,
            },
        )
    }
}

fn read(level: &ServerLevel, pos: BlockPos) -> Result<BlockState> {
    if pos.get_y() < level.get_min_y()
        || pos.get_y() > level.get_max_y()
        || !ChunkPos::containing(&pos).is_valid()
    {
        return Ok(Block::from_name("minecraft:void_air")
            .unwrap()
            .default_block_state());
    }
    level
        .chunk_map()
        .get_chunk(ChunkPos::containing(&pos))
        .map(|chunk| BlockState::new(chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z())))
        .ok_or(LiveBlockMutationError::MissingChunk(ChunkPos::containing(
            &pos,
        )))
}
fn liquid_shape<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    state: BlockState,
    direction: Direction,
    neighbor: BlockState,
) -> Result<BlockState> {
    use rivet_world::{
        block::liquid_block::LiquidBlockWorld,
        fluid::{FluidKind, FluidState, flowing::FluidWorld},
    };
    let own = FluidState::from_block(state).map_err(unavailable)?;
    let adjacent = FluidState::from_block(neighbor).map_err(unavailable)?;
    let mut world = crate::server::level::fluid_runtime::LiveFluidWorld::new(level, services)
        .map_err(unavailable)?;
    if own.is_source() || adjacent.is_source() {
        let delay = own
            .kind()
            .expect("LiquidBlock fluid")
            .tick_delay(world.fast_lava());
        world.schedule(pos, own.id(), delay).map_err(unavailable)?;
    }
    if direction == Direction::Down
        && own.kind() == Some(FluidKind::Water)
        && own.is_source()
        && (neighbor.is_in_tag("minecraft:enables_bubble_column_drag_down")
            || neighbor.is_in_tag("minecraft:enables_bubble_column_push_up"))
    {
        world
            .schedule_block(pos, Block::new(state.block()), 20)
            .map_err(unavailable)?;
    }
    Ok(state)
}
fn schedule_block(level: &mut ServerLevel, pos: BlockPos, block: Block, delay: i32) {
    let tick = ScheduledTick::new_normal(
        block,
        pos,
        level.game_time().wrapping_add(i64::from(delay)),
        level.next_sub_tick_count(),
    );
    level.schedule_block_tick(tick);
}
struct ShapeWorld<'a, S> {
    level: &'a mut ServerLevel,
    services: &'a mut S,
}
fn unwrap_world<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|e| std::panic::panic_any(e))
}
impl<S> LevelHeightAccessor for ShapeWorld<'_, S> {
    fn get_height(&self) -> i32 {
        self.level.get_height()
    }
    fn get_min_y(&self) -> i32 {
        self.level.get_min_y()
    }
}
impl<S: LiveNeighborServices> WorldGenLevel for ShapeWorld<'_, S> {
    fn get_seed(&self) -> i64 {
        self.level.seed()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        unwrap_world(read(self.level, *pos))
    }
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        region_can_survive(self, state, pos)
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        unwrap_world(
            self.level
                .set_block_live(*pos, state, flags, 512, self.services),
        )
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        let tick = ScheduledTick::new_normal(
            fluid,
            *pos,
            self.level.game_time().wrapping_add(i64::from(delay)),
            self.level.next_sub_tick_count(),
        );
        self.level.schedule_fluid_tick(tick);
    }
    fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        schedule_block(self.level, *pos, block, delay);
    }
}
fn with_world<S: LiveNeighborServices, T>(
    level: &mut ServerLevel,
    services: &mut S,
    f: impl FnOnce(&mut ShapeWorld<'_, S>) -> T,
) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f(&mut ShapeWorld { level, services })
    })) {
        Ok(value) => Ok(value),
        Err(panic) => match panic.downcast::<LiveBlockMutationError>() {
            Ok(error) => Err(*error),
            Err(panic) => std::panic::resume_unwind(panic),
        },
    }
}
pub fn postprocess_fluid_tick_live<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    state: BlockState,
) -> std::result::Result<(), GenerationPostProcessError> {
    let fluid = rivet_world::fluid::FluidState::from_block(state)
        .map_err(|e| GenerationPostProcessError::Unavailable(e.to_string()))?;
    level
        .tick_fluid_live(pos, fluid.id(), services)
        .map_err(|e| GenerationPostProcessError::Unavailable(e.to_string()))
}
pub fn postprocess_liquid_tick_live<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    state: BlockState,
) -> std::result::Result<(), GenerationPostProcessError> {
    level
        .tick_liquid_block_live(pos, state, services)
        .map_err(|e| GenerationPostProcessError::Unavailable(e.to_string()))
}
pub fn postprocess_set_block_live<S: LiveNeighborServices>(
    level: &mut ServerLevel,
    services: &mut S,
    pos: BlockPos,
    state: BlockState,
    flags: u32,
) -> std::result::Result<bool, GenerationPostProcessError> {
    level
        .set_block_live(pos, state, flags, 512, services)
        .map_err(|e| GenerationPostProcessError::Unavailable(e.to_string()))
}
