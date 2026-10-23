//! Actual synchronous native block-event channels sharing the plugin host's
//! lifecycle, priority, disable/unregister and exception semantics.
use super::{
    structure_spawn_events::{
        NativeListenerError, NativeListenerId, NativePluginMetadata, NativeStructureEventRegistry,
        StructureEventDiagnostics, StructureSpawnDispatchError,
    },
    structure_transformer::CraftBlockSnapshot,
};
use crate::server::tick::shutdown::Shutdown;
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, Direction},
};
use rivet_util::mth::Uuid;
use std::{sync::Arc, thread};
#[cfg(test)]
#[path = "world_events_test.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldEventKind {
    Physics,
    FluidLevel,
    FromTo,
    Form,
}
impl WorldEventKind {
    pub(super) fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Physics => "BlockPhysicsEvent",
            Self::FluidLevel => "FluidLevelChangeEvent",
            Self::FromTo => "BlockFromToEvent",
            Self::Form => "BlockFormEvent",
        }
    }
}
#[derive(Clone, Debug)]
pub enum WorldEventData {
    Physics {
        position: BlockPos,
        changed: BlockState,
        source: BlockPos,
    },
    FluidLevel {
        position: BlockPos,
        new_data: BlockState,
    },
    FromTo {
        source: BlockPos,
        direction: Direction,
    },
    Form {
        snapshot: CraftBlockSnapshot,
    },
}
#[derive(Clone, Debug)]
pub struct NativeWorldEvent {
    world: Uuid,
    data: WorldEventData,
    cancelled: bool,
}
impl NativeWorldEvent {
    pub fn new(world: Uuid, data: WorldEventData) -> Self {
        Self {
            world,
            data,
            cancelled: false,
        }
    }
    pub fn world_uuid(&self) -> Uuid {
        self.world
    }
    pub fn data(&self) -> &WorldEventData {
        &self.data
    }
    pub fn kind(&self) -> WorldEventKind {
        match self.data {
            WorldEventData::Physics { .. } => WorldEventKind::Physics,
            WorldEventData::FluidLevel { .. } => WorldEventKind::FluidLevel,
            WorldEventData::FromTo { .. } => WorldEventKind::FromTo,
            WorldEventData::Form { .. } => WorldEventKind::Form,
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn set_cancelled(&mut self, cancelled: bool) {
        self.cancelled = cancelled;
    }
    /// FluidLevelChangeEvent.setNewData clones value data and forbids changing
    /// its current Material, even when the originally proposed state is air.
    pub fn set_fluid_data(&mut self, state: BlockState) -> Result<(), NativeListenerError> {
        let WorldEventData::FluidLevel { new_data, .. } = &mut self.data else {
            return Err(NativeListenerError::Failure(
                "not FluidLevelChangeEvent".into(),
            ));
        };
        if new_data.block() != state.block() {
            return Err(NativeListenerError::Failure(
                "Cannot change fluid type".into(),
            ));
        }
        *new_data = state;
        Ok(())
    }
    pub fn set_form_data(&mut self, state: BlockState) -> Result<(), NativeListenerError> {
        let WorldEventData::Form { snapshot } = &mut self.data else {
            return Err(NativeListenerError::Failure("not BlockFormEvent".into()));
        };
        snapshot.state = state;
        Ok(())
    }
    pub fn form_block_entity_mut(&mut self) -> Option<&mut rivet_nbt::compound_tag::CompoundTag> {
        match &mut self.data {
            WorldEventData::Form { snapshot } => snapshot.block_entity.as_mut(),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct WorldEventFailure {
    pub plugin: NativePluginMetadata,
    pub listener: NativeListenerId,
    pub message: String,
}
/// The actual same-world operations available inside a native listener. The
/// production implementation borrows the *currently executing* services and
/// level, so reentrant writes reach the original neighbor/scheduler queues.
pub trait WorldEventAccess {
    fn world_uuid(&self) -> Option<Uuid>;
    fn block(&mut self, pos: BlockPos) -> rivet_world::fluid::FluidResult<BlockState>;
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        limit: i32,
    ) -> rivet_world::fluid::FluidResult<bool>;
}
pub type WorldListener = dyn Fn(&mut NativeWorldEvent, &mut dyn WorldEventAccess) -> Result<(), NativeListenerError>
    + Send
    + Sync;
pub type WorldExceptionListener = dyn Fn(
        &WorldEventFailure,
        &mut NativeWorldEvent,
        &mut dyn WorldEventAccess,
    ) -> Result<(), NativeListenerError>
    + Send
    + Sync;

/// Constructed by the real tick owner, before native plugins load. This is a
/// native host, not an assertion that a Bukkit/JVM plugin bridge was loaded.
pub struct NativeWorldEventHost {
    registry: Arc<NativeStructureEventRegistry>,
    shutdown: Arc<Shutdown>,
}
impl NativeWorldEventHost {
    pub fn start_on_current_thread(
        diagnostics: Arc<dyn StructureEventDiagnostics>,
        shutdown: Arc<Shutdown>,
    ) -> Self {
        Self {
            registry: Arc::new(NativeStructureEventRegistry::new(
                thread::current().id(),
                diagnostics,
            )),
            shutdown,
        }
    }
    pub fn registry(&self) -> &Arc<NativeStructureEventRegistry> {
        &self.registry
    }
    pub fn finish_loading(&self) {
        self.registry.finish_loading();
    }
    pub fn check_block_place_thread(&self) -> Result<(), StructureSpawnDispatchError> {
        self.registry.check_tick_thread()
    }
    pub fn dispatch(
        &self,
        world: &mut dyn WorldEventAccess,
        event: &mut NativeWorldEvent,
    ) -> Result<(), StructureSpawnDispatchError> {
        if world.world_uuid() != Some(event.world_uuid()) {
            return Err(StructureSpawnDispatchError::AuthorityUnavailable(
                "block event belongs to a different world".into(),
            ));
        }
        self.registry
            .dispatch_world_on_current_thread(event, world, self.shutdown.is_stopping())
    }
}
