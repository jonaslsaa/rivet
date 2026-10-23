//! Scheduled fluid bodies over the live world's canonical storage and queues.
//! Required event/drop effects are provided by the actual world services, not
//! implicit accepted events. This adapter never owns chunks or another RNG.
use super::{ServerLevel, server_level::LiveBlockMutationServices};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos, Direction},
    fluid_id::FluidId,
    generated::blocks::BlockId,
};
use rivet_util::random_source::RandomSource;
use rivet_world::{
    biome::gameplay_environment::{GameplayEnvironment, GameplayEnvironmentError},
    block::{Block, liquid_block::LiquidBlockWorld},
    fluid::{
        FluidError, FluidKind, FluidResult, FluidState, containers,
        flowing::{FlowingFluid, FluidWorld},
    },
    level::block_shape_context::{ShapeGeometry, collision_geometry},
    ticks::ScheduledTick,
};

/// These are the source's observable external effects and contextual geometry.
/// Implementations must execute their real event channels and drop/game-event
/// bodies. Having no listeners is a decision of that channel, not this trait.
pub trait FluidRuntimeServices: LiveBlockMutationServices {
    fn fluid_level_change(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<Option<BlockState>>;
    fn fluid_from_to(
        &mut self,
        level: &mut ServerLevel,
        source: BlockPos,
        direction: Direction,
    ) -> FluidResult<bool>;
    fn fluid_form_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> FluidResult<bool>;
    fn fluid_drop_resources(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> FluidResult<()>;
    fn fluid_lava_fizz(&mut self, level: &mut ServerLevel, pos: BlockPos) -> FluidResult<()>;
    fn fluid_contextual_collision(
        &mut self,
        level: &ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<ShapeGeometry>;
    /// Actual resolved Paper per-world environment setting, not a new default.
    fn water_over_lava_flow_speed(&self) -> i32;
    /// Effectful overrides (lit candle/campfire, dried ghast) need the real
    /// extinguish/sound/game-event behavior. This call must not merely set WATERLOGGED.
    fn fluid_place_effectful_container(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool>;
}

pub struct LiveFluidWorld<'a, S> {
    level: &'a mut ServerLevel,
    services: &'a mut S,
    fast_lava: bool,
}
impl<'a, S: FluidRuntimeServices> LiveFluidWorld<'a, S> {
    pub fn new(level: &'a mut ServerLevel, services: &'a mut S) -> FluidResult<Self> {
        let environment = level
            .generation_environment()
            .ok_or_else(|| FluidError::Unavailable("actual dimension/game-rule owner".into()))?;
        let attributes = GameplayEnvironment::from_pinned_registries(
            environment.dimension_type(),
            level.dimension().identifier(),
        )
        .map_err(unavailable)?;
        let fast_lava = attributes
            .fast_lava(|key| {
                level
                    .clock_runtime()
                    .ok_or_else(|| GameplayEnvironmentError::MissingClock(key.clone()))?
                    .clocks()
                    .state(key)
                    .map(|s| s.total_ticks)
                    .map_err(|_| GameplayEnvironmentError::MissingClock(key.clone()))
            })
            .map_err(unavailable)?;
        Ok(Self {
            level,
            services,
            fast_lava,
        })
    }
    fn outside_height(&self, pos: BlockPos) -> bool {
        pos.get_y() < self.level.get_min_y() || pos.get_y() > self.level.get_max_y()
    }
    fn shape(&mut self, pos: BlockPos, state: BlockState) -> FluidResult<ShapeGeometry> {
        if let Some(shape) = collision_geometry(&state, &pos) {
            Ok(shape)
        } else {
            self.services
                .fluid_contextual_collision(self.level, pos, state)
        }
    }
}
fn unavailable(error: impl std::fmt::Display) -> FluidError {
    FluidError::Unavailable(error.to_string())
}
impl<S: FluidRuntimeServices> FluidWorld for LiveFluidWorld<'_, S> {
    fn block(&mut self, pos: BlockPos) -> FluidResult<BlockState> {
        if self.outside_height(pos) {
            return Ok(BlockState::of(
                BlockId::from_name("minecraft:void_air").unwrap(),
            ));
        }
        let chunk = self
            .level
            .chunk_map()
            .get_chunk(ChunkPos::containing(&pos))
            .ok_or_else(|| {
                FluidError::Unavailable(format!(
                    "loaded canonical fluid chunk {:?}",
                    ChunkPos::containing(&pos)
                ))
            })?;
        Ok(BlockState::new(chunk.get_block_state(
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
        )))
    }
    fn block_if_loaded(&mut self, pos: BlockPos) -> FluidResult<Option<BlockState>> {
        // Level's final getBlockStateIfLoaded checks the actual build bounds
        // before consulting the cache; no out-of-height chunk is loaded.
        if self.outside_height(pos) {
            return self.block(pos).map(Some);
        }
        if self
            .level
            .chunk_map()
            .get_chunk(ChunkPos::containing(&pos))
            .is_none()
        {
            return Ok(None);
        }
        self.block(pos).map(Some)
    }
    fn set_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> FluidResult<bool> {
        self.level
            .set_block_live(pos, state, flags, 512, self.services)
            .map_err(unavailable)
    }
    fn schedule(&mut self, pos: BlockPos, fluid: FluidId, delay: i32) -> FluidResult<()> {
        let tick = ScheduledTick::new_normal(
            fluid,
            pos,
            self.level.game_time().wrapping_add(i64::from(delay)),
            self.level.next_sub_tick_count(),
        );
        self.level.schedule_fluid_tick(tick);
        Ok(())
    }
    fn random_bound(&mut self, bound: i32) -> i32 {
        self.level.random_mut().next_int_bound(bound)
    }
    fn fast_lava(&self) -> bool {
        self.fast_lava
    }
    fn source_conversion(&self, kind: FluidKind) -> bool {
        let environment = self
            .level
            .generation_environment()
            .expect("adapter retains installed environment");
        match kind {
            FluidKind::Water => environment.water_source_conversion(),
            FluidKind::Lava => environment.lava_source_conversion(),
        }
    }
    fn can_pass_wall(
        &mut self,
        from: BlockPos,
        a: BlockState,
        direction: Direction,
        to: BlockPos,
        b: BlockState,
    ) -> FluidResult<bool> {
        let a = self.shape(from, a)?;
        let b = self.shape(to, b)?;
        Ok(!a.merged_face_occludes(&b, direction))
    }
    fn is_liquid_container(&self, state: BlockState) -> FluidResult<bool> {
        Ok(containers::behavior(state)?.is_container())
    }
    fn can_place_liquid(
        &mut self,
        _pos: BlockPos,
        state: BlockState,
        fluid: FluidId,
    ) -> FluidResult<bool> {
        containers::can_place(state, fluid)
    }
    fn place_liquid(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool> {
        use rivet_registry::{
            block_state_properties::BlockStateProperties as P, block_state_property::PropertyValue,
        };
        let owner = containers::behavior(state)?.place_owner;
        if owner == "DriedGhastBlock"
            || matches!(owner, "CandleBlock" | "CampfireBlock")
                && state.get_value(P::LIT) == Some(PropertyValue::Bool(true))
        {
            self.services
                .fluid_place_effectful_container(self.level, pos, state, fluid)
        } else {
            containers::place_standard(self, pos, state, fluid)
        }
    }
    fn excluded_from_holding(&self, state: BlockState) -> FluidResult<bool> {
        Ok(containers::behavior(state)?.excluded)
    }
    fn level_change_event(
        &mut self,
        pos: BlockPos,
        new: BlockState,
    ) -> FluidResult<Option<BlockState>> {
        self.services.fluid_level_change(self.level, pos, new)
    }
    fn from_to_event(&mut self, pos: BlockPos, direction: Direction) -> FluidResult<bool> {
        self.services.fluid_from_to(self.level, pos, direction)
    }
    fn form_block_event(
        &mut self,
        pos: BlockPos,
        new: BlockState,
        flags: u32,
    ) -> FluidResult<bool> {
        self.services.fluid_form_block(self.level, pos, new, flags)
    }
    fn drop_resources(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> FluidResult<()> {
        self.services
            .fluid_drop_resources(self.level, pos, state, source)
    }
    fn lava_fizz(&mut self, pos: BlockPos) -> FluidResult<()> {
        self.services.fluid_lava_fizz(self.level, pos)
    }
}
impl<S: FluidRuntimeServices> LiquidBlockWorld for LiveFluidWorld<'_, S> {
    fn water_over_lava_flow_speed(&self) -> i32 {
        self.services.water_over_lava_flow_speed()
    }
    fn schedule_block(&mut self, pos: BlockPos, block: Block, delay: i32) -> FluidResult<()> {
        let tick = ScheduledTick::new_normal(
            block,
            pos,
            self.level.game_time().wrapping_add(i64::from(delay)),
            self.level.next_sub_tick_count(),
        );
        self.level.schedule_block_tick(tick);
        Ok(())
    }
}

impl<S: FluidRuntimeServices> rivet_world::block::bubble_column_block::ColumnWorld
    for LiveFluidWorld<'_, S>
{
    type Error = FluidError;
    fn column_block(&mut self, pos: BlockPos) -> FluidResult<BlockState> {
        FluidWorld::block(self, pos)
    }
    fn column_set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> FluidResult<bool> {
        FluidWorld::set_block(self, pos, state, flags)
    }
}

impl ServerLevel {
    /// Genuine LiquidBlock onPlace/neighborChanged route. Callers supply the
    /// actual state at the source callback boundary (not a synthetic tick).
    pub fn liquid_block_changed_live(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        services: &mut impl FluidRuntimeServices,
    ) -> FluidResult<()> {
        rivet_world::block::liquid_block::on_place_or_neighbor_changed(
            &mut LiveFluidWorld::new(self, services)?,
            pos,
            state,
        )
    }

    pub fn tick_liquid_block_live(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        services: &mut impl FluidRuntimeServices,
    ) -> FluidResult<()> {
        rivet_world::block::bubble_column_block::liquid_tick(
            &mut LiveFluidWorld::new(self, services)?,
            state,
            pos,
        )
    }

    pub fn tick_bubble_column_live(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        services: &mut impl FluidRuntimeServices,
    ) -> FluidResult<()> {
        rivet_world::block::bubble_column_block::bubble_tick(
            &mut LiveFluidWorld::new(self, services)?,
            state,
            pos,
        )
    }
    /// ServerLevel.tickFluid's exact current-fluid check precedes its body.
    pub fn tick_fluid_live(
        &mut self,
        pos: BlockPos,
        scheduled: FluidId,
        services: &mut impl FluidRuntimeServices,
    ) -> FluidResult<()> {
        let mut world = LiveFluidWorld::new(self, services)?;
        let state = world.block(pos)?;
        let fluid = FluidState::from_block(state)?;
        if fluid.id() != scheduled {
            return Ok(());
        }
        let Some(kind) = fluid.kind() else {
            return Ok(());
        };
        FlowingFluid::new(kind).tick(&mut world, pos, state, fluid)
    }

    /// Natural scheduled fluid execution with actual chunk-ticking authority
    /// and the same services used by live block mutations.
    pub fn run_fluid_ticks_live(
        &mut self,
        max: i32,
        check: impl FnMut(ChunkPos) -> bool,
        services: &mut impl FluidRuntimeServices,
    ) -> FluidResult<()> {
        self.run_fluid_ticks_with_world(max, check, |level, pos, fluid| {
            level.tick_fluid_live(pos, fluid, services)
        })
    }
}
