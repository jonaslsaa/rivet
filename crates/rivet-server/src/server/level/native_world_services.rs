//! One concrete native callback owner. Native algorithms/event dispatch live
//! here; only actual external transport/entity/drop/navigation services are
//! delegated to the session's borrowed world effects. No fallback accepts them.
use super::{
    ChunkMap, ServerLevel,
    fluid_runtime::FluidRuntimeServices,
    level_block_entity_runtime::{
        BlockEntityTickerError, LevelBlockEntityRuntime, NativeTickerKind,
    },
    server_level::{
        BlockEntityWorldServices, GenerationPostProcessError, GenerationPostProcessServices,
        LiveBlockEffect, LiveBlockMutationError, LiveBlockMutationServices, LiveNeighborServices,
        LiveNeighborUpdater, dispatch_live_block_effect,
    },
    world_events::*,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
};
use rivet_util::random::ThreadUnsafeRandom;
use rivet_world::{
    fluid::{FluidError, FluidResult, FluidState},
    level::block_shape_context::ShapeGeometry,
};
use std::sync::Arc;
type MutationResult<T> = Result<T, LiveBlockMutationError>;
type BeResult<T> = Result<T, BlockEntityTickerError>;

pub trait NativeWorldEffects: BlockEntityWorldServices + Send {
    fn external_block_effect(
        &mut self,
        level: &mut ServerLevel,
        effect: LiveBlockEffect,
    ) -> MutationResult<()>;
    fn neighbor_debug_update(
        &mut self,
        level: &mut ServerLevel,
        positions: &[BlockPos],
    ) -> MutationResult<()>;
    fn neighbor_destroy_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        drops: bool,
        limit: i32,
    ) -> MutationResult<bool>;
    fn redstone_experiments_enabled(&self) -> bool;
    fn fluid_drop_resources(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> FluidResult<()>;
    fn lava_fizz(&mut self, level: &mut ServerLevel, pos: BlockPos) -> FluidResult<()>;
    fn contextual_collision(
        &mut self,
        level: &ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<ShapeGeometry>;
    fn water_over_lava_flow_speed(&self) -> i32;
    fn place_effectful_container(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool>;
}
pub struct NativeWorldServices<E> {
    pub events: Arc<NativeWorldEventHost>,
    pub effects: E,
    neighbors: LiveNeighborUpdater,
}
impl<E> NativeWorldServices<E> {
    pub fn new(events: Arc<NativeWorldEventHost>, effects: E, max_chained_updates: i32) -> Self {
        Self {
            events,
            effects,
            neighbors: LiveNeighborUpdater::new(max_chained_updates),
        }
    }
    pub fn neighbor_runtime_mut(&mut self) -> &mut LiveNeighborUpdater {
        &mut self.neighbors
    }
    fn dispatch(
        &mut self,
        level: &mut ServerLevel,
        data: WorldEventData,
    ) -> FluidResult<NativeWorldEvent>
    where
        E: NativeWorldEffects,
    {
        let world = level
            .world_uuid()
            .ok_or_else(|| FluidError::Unavailable("event world UUID unavailable".into()))?;
        let mut event = NativeWorldEvent::new(world, data);
        let host = Arc::clone(&self.events);
        host.dispatch(
            &mut CanonicalEventAccess {
                level,
                services: self,
            },
            &mut event,
        )
        .map_err(|e| FluidError::Unavailable(e.to_string()))?;
        Ok(event)
    }
}
struct CanonicalEventAccess<'a, E> {
    level: &'a mut ServerLevel,
    services: &'a mut NativeWorldServices<E>,
}
impl<E: NativeWorldEffects> WorldEventAccess for CanonicalEventAccess<'_, E> {
    fn world_uuid(&self) -> Option<rivet_util::mth::Uuid> {
        self.level.world_uuid()
    }
    fn block(&mut self, pos: BlockPos) -> FluidResult<BlockState> {
        let cp = ChunkPos::containing(&pos);
        if pos.get_y() < self.level.get_min_y()
            || pos.get_y() > self.level.get_max_y()
            || !cp.is_valid()
        {
            return Ok(BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:void_air")
                    .unwrap(),
            ));
        }
        let chunk = self
            .level
            .chunk_map()
            .get_chunk(cp)
            .ok_or_else(|| FluidError::Unavailable(format!("event canonical chunk {cp}")))?;
        Ok(BlockState::new(chunk.get_block_state(
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
        )))
    }
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        limit: i32,
    ) -> FluidResult<bool> {
        self.level
            .set_block_live(pos, state, flags, limit, self.services)
            .map_err(|error| FluidError::Unavailable(error.to_string()))
    }
}
impl<E: NativeWorldEffects> BlockEntityWorldServices for NativeWorldServices<E> {
    fn on_block_entity_added(&mut self, pos: BlockPos) -> BeResult<()> {
        self.effects.on_block_entity_added(pos)
    }
    fn hive_debug_transport(
        &mut self,
    ) -> BeResult<&mut dyn super::level_debug_synchronizers::HiveDebugTransport> {
        self.effects.hive_debug_transport()
    }
    fn require_ticker_body(&self, kind: NativeTickerKind) -> BeResult<()> {
        self.effects.require_ticker_body(kind)
    }
    fn tick_body(
        &mut self,
        runtime: &mut LevelBlockEntityRuntime,
        chunks: &mut ChunkMap,
        random: &mut ThreadUnsafeRandom,
        pos: BlockPos,
        kind: NativeTickerKind,
    ) -> BeResult<()> {
        self.effects.tick_body(runtime, chunks, random, pos, kind)
    }
    fn report_tick_exception(&mut self, pos: BlockPos, message: &str) -> BeResult<()> {
        self.effects.report_tick_exception(pos, message)
    }
    fn mid_tick_tasks(&mut self) -> BeResult<()> {
        self.effects.mid_tick_tasks()
    }
}
impl<E: NativeWorldEffects> LiveBlockMutationServices for NativeWorldServices<E> {
    fn check_block_place_thread(&self) -> MutationResult<()> {
        self.events
            .check_block_place_thread()
            .map_err(|e| LiveBlockMutationError::Unavailable(e.to_string()))
    }
    fn block_effect(
        &mut self,
        level: &mut ServerLevel,
        effect: LiveBlockEffect,
    ) -> MutationResult<()> {
        if let Some(effect) = dispatch_live_block_effect(level, self, effect)? {
            self.effects.external_block_effect(level, effect)?;
        }
        Ok(())
    }
    fn block_physics_event(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> MutationResult<bool> {
        self.neighbor_physics_event(level, pos, state, pos)
    }
    fn changed_state_keeps_block_entity(
        &mut self,
        state: BlockState,
        old: BlockState,
    ) -> MutationResult<bool> {
        if state.is_in_tag("minecraft:copper_chests") {
            Ok(old.is_in_tag("minecraft:copper_chests"))
        } else if state.is_in_tag("minecraft:copper_golem_statues") {
            Ok(old.is_in_tag("minecraft:copper_golem_statues"))
        } else {
            Err(LiveBlockMutationError::Unavailable(format!(
                "{}.shouldChangedStateKeepBlockEntity",
                state.block().name()
            )))
        }
    }
}
impl<E: NativeWorldEffects> LiveNeighborServices for NativeWorldServices<E> {
    fn neighbor_updater(&mut self) -> &mut LiveNeighborUpdater {
        &mut self.neighbors
    }
    fn neighbor_physics_event(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> MutationResult<bool> {
        self.dispatch(
            level,
            WorldEventData::Physics {
                position: pos,
                changed: state,
                source,
            },
        )
        .map(|event| event.is_cancelled())
        .map_err(|e| LiveBlockMutationError::Unavailable(e.to_string()))
    }
    fn redstone_experiments_enabled(&self) -> bool {
        self.effects.redstone_experiments_enabled()
    }
    fn neighbor_debug_update(
        &mut self,
        level: &mut ServerLevel,
        positions: &[BlockPos],
    ) -> MutationResult<()> {
        self.effects.neighbor_debug_update(level, positions)
    }
    fn neighbor_destroy_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        drops: bool,
        limit: i32,
    ) -> MutationResult<bool> {
        self.effects
            .neighbor_destroy_block(level, pos, drops, limit)
    }
}
impl<E: NativeWorldEffects> FluidRuntimeServices for NativeWorldServices<E> {
    fn fluid_level_change(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<Option<BlockState>> {
        let event = self.dispatch(
            level,
            WorldEventData::FluidLevel {
                position: pos,
                new_data: state,
            },
        )?;
        let WorldEventData::FluidLevel { new_data, .. } = event.data() else {
            unreachable!()
        };
        Ok((!event.is_cancelled()).then_some(*new_data))
    }
    fn fluid_from_to(
        &mut self,
        level: &mut ServerLevel,
        source: BlockPos,
        direction: rivet_registry::core::Direction,
    ) -> FluidResult<bool> {
        Ok(!self
            .dispatch(level, WorldEventData::FromTo { source, direction })?
            .is_cancelled())
    }
    fn fluid_form_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> FluidResult<bool> {
        let chunk = level
            .chunk_map()
            .get_chunk(ChunkPos::containing(&pos))
            .ok_or_else(|| FluidError::Unavailable("form snapshot canonical chunk".into()))?;
        let old = BlockState::new(chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z()));
        if super::structure_snapshot_data::has_block_entity(old) {
            return Err(FluidError::Unavailable(
                "tile-backed Craft BlockForm snapshot requires actual apply-state codec".into(),
            ));
        }
        let snapshot = super::structure_transformer::CraftBlockSnapshot {
            position: pos,
            state,
            block_entity: None,
        };
        let event = self.dispatch(level, WorldEventData::Form { snapshot })?;
        if event.is_cancelled() {
            return Ok(false);
        }
        let WorldEventData::Form { snapshot } = event.data() else {
            unreachable!()
        };
        level
            .set_block_live(pos, snapshot.state, flags, 512, self)
            .map_err(|e| FluidError::Unavailable(e.to_string()))?;
        // CraftEventFactory's default checkSetResult=false deliberately ignores
        // a false setBlock return after the accepted snapshot.place call.
        Ok(true)
    }
    fn fluid_drop_resources(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        source: BlockPos,
    ) -> FluidResult<()> {
        self.effects.fluid_drop_resources(level, pos, state, source)
    }
    fn fluid_lava_fizz(&mut self, level: &mut ServerLevel, pos: BlockPos) -> FluidResult<()> {
        self.effects.lava_fizz(level, pos)
    }
    fn fluid_contextual_collision(
        &mut self,
        level: &ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<ShapeGeometry> {
        self.effects.contextual_collision(level, pos, state)
    }
    fn water_over_lava_flow_speed(&self) -> i32 {
        self.effects.water_over_lava_flow_speed()
    }
    fn fluid_place_effectful_container(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool> {
        self.effects
            .place_effectful_container(level, pos, state, fluid)
    }
}
impl<E: NativeWorldEffects> GenerationPostProcessServices for NativeWorldServices<E> {
    fn fluid_tick(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), GenerationPostProcessError> {
        super::server_level::postprocess_fluid_tick_live(level, self, pos, state)
    }
    fn liquid_block_tick(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), GenerationPostProcessError> {
        super::server_level::postprocess_liquid_tick_live(level, self, pos, state)
    }
    fn set_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, GenerationPostProcessError> {
        super::server_level::postprocess_set_block_live(level, self, pos, state, flags)
    }
}
