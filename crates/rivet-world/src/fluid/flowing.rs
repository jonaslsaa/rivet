//! FlowingFluid.tick/spread/getNewLiquid and Paper's no-load lateral searches.
//! World services execute the real mutation/event/drop bodies. Every operation
//! can refuse missing authority; no successful empty event or fluid tick exists.
use super::{FluidError, FluidKind, FluidResult, FluidState};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, Direction},
    fluid_id::FluidId,
};
use std::collections::HashMap;

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

pub trait FluidWorld {
    fn block(&mut self, pos: BlockPos) -> FluidResult<BlockState>;
    fn block_if_loaded(&mut self, pos: BlockPos) -> FluidResult<Option<BlockState>>;
    fn set_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> FluidResult<bool>;
    fn schedule(&mut self, pos: BlockPos, fluid: FluidId, delay: i32) -> FluidResult<()>;
    fn random_bound(&mut self, bound: i32) -> i32;
    fn fast_lava(&self) -> bool;
    fn source_conversion(&self, kind: FluidKind) -> bool;
    /// Actual union of opposing COLLISION faces (not each face independently).
    fn can_pass_wall(
        &mut self,
        from: BlockPos,
        from_state: BlockState,
        direction: Direction,
        to: BlockPos,
        to_state: BlockState,
    ) -> FluidResult<bool>;
    fn is_liquid_container(&self, state: BlockState) -> FluidResult<bool>;
    /// Source user=null, including the source-vs-flowing fluid distinction.
    fn can_place_liquid(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidId,
    ) -> FluidResult<bool>;
    fn place_liquid(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool>;
    fn excluded_from_holding(&self, state: BlockState) -> FluidResult<bool>;
    fn level_change_event(
        &mut self,
        pos: BlockPos,
        new_state: BlockState,
    ) -> FluidResult<Option<BlockState>>;
    fn dispatch_from_to_event(
        &mut self,
        source: BlockPos,
        direction: Direction,
    ) -> FluidResult<bool>;
    /// handleBlockFormEvent includes the actual event-altered world write.
    fn form_block_event(
        &mut self,
        pos: BlockPos,
        new_state: BlockState,
        flags: u32,
    ) -> FluidResult<bool>;
    fn drop_resources(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> FluidResult<()>;
    fn lava_fizz(&mut self, pos: BlockPos) -> FluidResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub struct FlowingFluid {
    pub kind: FluidKind,
}
impl FlowingFluid {
    pub const fn new(kind: FluidKind) -> Self {
        Self { kind }
    }
    fn same(self, state: FluidState) -> bool {
        state.kind() == Some(self.kind)
    }
    fn source(self, state: FluidState) -> bool {
        self.same(state) && state.is_source()
    }
    fn can_hold_any(self, world: &impl FluidWorld, state: BlockState) -> FluidResult<bool> {
        Ok(world.is_liquid_container(state)?
            || !state.blocks_motion() && !world.excluded_from_holding(state)?)
    }
    fn can_hold_specific(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        state: BlockState,
        id: FluidId,
    ) -> FluidResult<bool> {
        if world.is_liquid_container(state)? {
            world.can_place_liquid(pos, state, id)
        } else {
            Ok(true)
        }
    }
    // Preserve source canMaybePassThrough's paired positions/states/fluid.
    #[allow(clippy::too_many_arguments)]
    fn maybe_pass(
        self,
        world: &mut impl FluidWorld,
        from: BlockPos,
        state: BlockState,
        direction: Direction,
        to: BlockPos,
        to_state: BlockState,
        to_fluid: FluidState,
    ) -> FluidResult<bool> {
        Ok(!self.source(to_fluid)
            && self.can_hold_any(world, to_state)?
            && world.can_pass_wall(from, state, direction, to, to_state)?)
    }
    // Same source argument tuple, with specific-fluid container validation.
    #[allow(clippy::too_many_arguments)]
    fn can_pass(
        self,
        world: &mut impl FluidWorld,
        from: BlockPos,
        state: BlockState,
        direction: Direction,
        to: BlockPos,
        to_state: BlockState,
        to_fluid: FluidState,
    ) -> FluidResult<bool> {
        Ok(
            self.maybe_pass(world, from, state, direction, to, to_state, to_fluid)?
                && self.can_hold_specific(world, to, to_state, self.kind.flowing_id())?,
        )
    }

    pub fn new_liquid(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<FluidState> {
        let mut highest = 0;
        let mut sources = 0;
        for direction in HORIZONTAL {
            let neighbor = pos.relative(&direction);
            let Some(block) = world.block_if_loaded(neighbor)? else {
                continue;
            };
            let fluid = FluidState::from_block(block)?;
            if self.same(fluid) && world.can_pass_wall(pos, state, direction, neighbor, block)? {
                if fluid.is_source() {
                    sources += 1;
                }
                highest = highest.max(i32::from(fluid.amount()));
            }
        }
        if sources >= 2 && world.source_conversion(self.kind) {
            let below = world.block(pos.below())?;
            if below.is_solid() || self.source(FluidState::from_block(below)?) {
                return Ok(FluidState::source(self.kind, false));
            }
        }
        let above_pos = pos.above();
        let above = world.block(above_pos)?;
        let fluid = FluidState::from_block(above)?;
        if !fluid.is_empty()
            && self.same(fluid)
            && world.can_pass_wall(pos, state, Direction::Up, above_pos, above)?
        {
            return FluidState::flowing(self.kind, 8, true);
        }
        let amount = highest - self.kind.drop_off(world.fast_lava());
        if amount <= 0 {
            Ok(FluidState::Empty)
        } else {
            FluidState::flowing(self.kind, amount as u8, false)
        }
    }

    fn height(world: &mut impl FluidWorld, fluid: FluidState, pos: BlockPos) -> FluidResult<f32> {
        let above = FluidState::from_block(world.block(pos.above())?)?;
        Ok(if fluid.kind() == above.kind() {
            1.0
        } else {
            fluid.own_height()
        })
    }
    fn spread_delay(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        old: FluidState,
        new: FluidState,
    ) -> FluidResult<i32> {
        let mut delay = self.kind.tick_delay(world.fast_lava());
        if self.kind == FluidKind::Lava
            && !old.is_empty()
            && !new.is_empty()
            && !old.falling()
            && !new.falling()
            && Self::height(world, new, pos)? > Self::height(world, old, pos)?
            && world.random_bound(4) != 0
        {
            delay *= 4;
        }
        Ok(delay)
    }

    pub fn tick(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        mut state: BlockState,
        mut fluid: FluidState,
    ) -> FluidResult<()> {
        if !fluid.is_source() {
            let current = world.block(pos)?;
            let new = self.new_liquid(world, pos, current)?;
            let delay = self.spread_delay(world, pos, fluid, new)?;
            if new.is_empty() {
                fluid = new;
                let Some(changed) =
                    world.level_change_event(pos, FluidState::Empty.legacy_block())?
                else {
                    return Ok(());
                };
                state = changed;
                world.set_block(pos, state, 3)?;
            } else if new != fluid {
                fluid = new;
                let Some(changed) = world.level_change_event(pos, fluid.legacy_block())? else {
                    return Ok(());
                };
                state = changed;
                world.set_block(pos, state, 3)?;
                world.schedule(pos, fluid.id(), delay)?;
            }
        }
        // Keep calculated fluid even if a plugin replaced its new block data.
        self.spread(world, pos, state, fluid)
    }

    fn water_hole(
        self,
        world: &mut impl FluidWorld,
        top: BlockPos,
        top_state: BlockState,
        bottom: BlockPos,
        bottom_state: BlockState,
    ) -> FluidResult<bool> {
        if !world.can_pass_wall(top, top_state, Direction::Down, bottom, bottom_state)? {
            return Ok(false);
        }
        Ok(self.same(FluidState::from_block(bottom_state)?)
            || self.can_hold_any(world, bottom_state)?
                && self.can_hold_specific(world, bottom, bottom_state, self.kind.flowing_id())?)
    }
    fn source_neighbors(self, world: &mut impl FluidWorld, pos: BlockPos) -> FluidResult<i32> {
        let mut count = 0;
        for direction in HORIZONTAL {
            if self.source(FluidState::from_block(
                world.block(pos.relative(&direction))?,
            )?) {
                count += 1;
            }
        }
        Ok(count)
    }
    fn spread(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<()> {
        if fluid.is_empty() {
            return Ok(());
        }
        let below_pos = pos.below();
        let below = world.block(below_pos)?;
        let below_fluid = FluidState::from_block(below)?;
        if self.maybe_pass(
            world,
            pos,
            state,
            Direction::Down,
            below_pos,
            below,
            below_fluid,
        )? {
            let new = self.new_liquid(world, below_pos, below)?;
            if self.replaceable(world, below_pos, below_fluid, new, Direction::Down)?
                && self.can_hold_specific(world, below_pos, below, new.id())?
            {
                if !world.dispatch_from_to_event(pos, Direction::Down)? {
                    return Ok(());
                }
                self.spread_to(world, below_pos, below, Direction::Down, new)?;
                if self.source_neighbors(world, pos)? >= 3 {
                    self.spread_sides(world, pos, fluid, state)?;
                }
                return Ok(());
            }
        }
        if fluid.is_source() || !self.water_hole(world, pos, state, below_pos, below)? {
            self.spread_sides(world, pos, fluid, state)?;
        }
        Ok(())
    }
    fn replaceable(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        old: FluidState,
        new: FluidState,
        direction: Direction,
    ) -> FluidResult<bool> {
        match old.kind() {
            None => Ok(true),
            Some(FluidKind::Water) => {
                Ok(direction == Direction::Down && new.kind() != Some(FluidKind::Water))
            }
            Some(FluidKind::Lava) => Ok(Self::height(world, old, pos)? >= 0.444_444_45
                && new.kind() == Some(FluidKind::Water)),
        }
    }
    fn spread_to(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        state: BlockState,
        direction: Direction,
        target: FluidState,
    ) -> FluidResult<()> {
        if self.kind == FluidKind::Lava
            && direction == Direction::Down
            && FluidState::from_block(world.block(pos)?)?.kind() == Some(FluidKind::Water)
        {
            if matches!(state.block().name(), "minecraft:water" | "minecraft:lava") {
                let stone = BlockState::of(
                    rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone")
                        .unwrap(),
                );
                if !world.form_block_event(pos, stone, 3)? {
                    return Ok(());
                }
            }
            world.lava_fizz(pos)?;
            return Ok(());
        }
        if world.is_liquid_container(state)? {
            world.place_liquid(pos, state, target)?;
        } else {
            if !state.is_air() {
                if self.kind == FluidKind::Water {
                    world.drop_resources(pos, state, pos.relative(&direction.get_opposite()))?;
                } else {
                    world.lava_fizz(pos)?;
                }
            }
            world.set_block(pos, target.legacy_block(), 3)?;
        }
        Ok(())
    }
    fn spread_sides(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        fluid: FluidState,
        state: BlockState,
    ) -> FluidResult<()> {
        let neighbor = if fluid.falling() {
            7
        } else {
            i32::from(fluid.amount()) - self.kind.drop_off(world.fast_lava())
        };
        if neighbor <= 0 {
            return Ok(());
        }
        let spreads = self.get_spread(world, pos, state)?;
        // Java EnumMap iterates enum declaration order, NOT HORIZONTAL order.
        for direction in Direction::VALUES {
            let Some(fluid) = spreads[direction_index(direction)] else {
                continue;
            };
            let next = pos.relative(&direction);
            let Some(state) = world.block_if_loaded(next)? else {
                continue;
            };
            if !world.dispatch_from_to_event(pos, direction)? {
                continue;
            }
            self.spread_to(world, next, state, direction, fluid)?;
        }
        Ok(())
    }
    pub fn get_spread(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<[Option<FluidState>; 6]> {
        let mut lowest = 1000;
        let mut result = [None; 6];
        let mut context = None;
        for direction in HORIZONTAL {
            let next = pos.relative(&direction);
            let Some(block) = world.block_if_loaded(next)? else {
                continue;
            };
            let old = FluidState::from_block(block)?;
            if self.maybe_pass(world, pos, state, direction, next, block, old)? {
                let fluid = self.new_liquid(world, next, block)?;
                if self.can_hold_specific(world, next, block, fluid.id())? {
                    let context = context.get_or_insert_with(|| SpreadContext::new(pos));
                    let distance = if context.is_hole(self, world, next)? {
                        0
                    } else {
                        self.slope(world, next, 1, direction.get_opposite(), block, context)?
                    };
                    if distance < lowest {
                        result.fill(None);
                    }
                    if distance <= lowest {
                        if self.replaceable(world, next, old, fluid, direction)? {
                            result[direction_index(direction)] = Some(fluid);
                        }
                        lowest = distance;
                    }
                }
            }
        }
        Ok(result)
    }
    fn slope(
        self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        pass: i32,
        from: Direction,
        state: BlockState,
        context: &mut SpreadContext,
    ) -> FluidResult<i32> {
        let mut lowest = 1000;
        for direction in HORIZONTAL {
            if direction == from {
                continue;
            }
            let next = pos.relative(&direction);
            let Some(block) = context.state(world, next, false)? else {
                continue;
            };
            let fluid = FluidState::from_block(block)?;
            if self.can_pass(world, pos, state, direction, next, block, fluid)? {
                if context.is_hole(self, world, next)? {
                    return Ok(pass);
                }
                if pass < self.kind.slope_distance(world.fast_lava()) {
                    lowest = lowest.min(self.slope(
                        world,
                        next,
                        pass + 1,
                        direction.get_opposite(),
                        block,
                        context,
                    )?);
                }
            }
        }
        Ok(lowest)
    }
}

fn direction_index(direction: Direction) -> usize {
    Direction::VALUES
        .iter()
        .position(|value| *value == direction)
        .unwrap()
}
struct SpreadContext {
    origin: BlockPos,
    states: HashMap<i16, BlockState>,
    holes: HashMap<i16, bool>,
}
impl SpreadContext {
    fn new(origin: BlockPos) -> Self {
        Self {
            origin,
            states: HashMap::new(),
            holes: HashMap::new(),
        }
    }
    fn key(&self, pos: BlockPos) -> i16 {
        let x = pos.get_x().wrapping_sub(self.origin.get_x());
        let z = pos.get_z().wrapping_sub(self.origin.get_z());
        (((x.wrapping_add(128) & 255) << 8) | (z.wrapping_add(128) & 255)) as i16
    }
    fn state(
        &mut self,
        world: &mut impl FluidWorld,
        pos: BlockPos,
        load: bool,
    ) -> FluidResult<Option<BlockState>> {
        let key = self.key(pos);
        if let Some(state) = self.states.get(&key) {
            return Ok(Some(*state));
        }
        let state = if load {
            Some(world.block(pos)?)
        } else {
            world.block_if_loaded(pos)?
        };
        if let Some(state) = state {
            self.states.insert(key, state);
        }
        Ok(state)
    }
    fn is_hole(
        &mut self,
        fluid: FlowingFluid,
        world: &mut impl FluidWorld,
        pos: BlockPos,
    ) -> FluidResult<bool> {
        let key = self.key(pos);
        if let Some(value) = self.holes.get(&key) {
            return Ok(*value);
        }
        let state = self
            .state(world, pos, true)?
            .ok_or_else(|| FluidError::Unavailable("loaded hole lookup absent".into()))?;
        let below = world.block(pos.below())?;
        let value = fluid.water_hole(world, pos, state, pos.below(), below)?;
        self.holes.insert(key, value);
        Ok(value)
    }
}
