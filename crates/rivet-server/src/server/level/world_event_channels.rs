//! Native block HandlerLists inside the same actual plugin registry.
use super::*;
use crate::server::level::world_events::{NativeWorldEvent, WorldEventAccess, WorldEventFailure};
impl NativeStructureEventRegistry {
    pub fn check_tick_thread(&self) -> Result<(), StructureSpawnDispatchError> {
        if thread::current().id() != self.primary_thread {
            return Err(StructureSpawnDispatchError::SynchronousThreadRequired);
        }
        Ok(())
    }
    pub fn register_world_listener(
        &self,
        plugin: NativePluginId,
        kind: WorldEventKind,
        priority: EventPriority,
        ignore_cancelled: bool,
        callback: Arc<WorldListener>,
    ) -> Result<NativeListenerId, StructureSpawnDispatchError> {
        self.register(
            plugin,
            priority,
            ignore_cancelled,
            Callback::World(kind, callback),
        )
    }
    pub fn register_world_exception_listener(
        &self,
        plugin: NativePluginId,
        priority: EventPriority,
        callback: Arc<WorldExceptionListener>,
    ) -> Result<NativeListenerId, StructureSpawnDispatchError> {
        self.register(plugin, priority, false, Callback::WorldException(callback))
    }
    fn world_snapshot(
        &self,
        kind: WorldEventKind,
        exception: bool,
    ) -> Result<Vec<Arc<Registration>>, StructureSpawnDispatchError> {
        let lists = self
            .listeners
            .read()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let buckets = if exception {
            &lists.world_exception
        } else {
            &lists.world[kind.index()]
        };
        Ok(buckets.iter().flatten().cloned().collect())
    }
    pub fn dispatch_world_on_current_thread(
        &self,
        event: &mut NativeWorldEvent,
        level: &mut dyn WorldEventAccess,
        stopping: bool,
    ) -> Result<(), StructureSpawnDispatchError> {
        if !stopping {
            self.check_tick_thread()?;
        }
        if self.loading.load(Ordering::Acquire) {
            return Err(StructureSpawnDispatchError::LifecycleLoading);
        }
        for entry in self.world_snapshot(event.kind(), false)? {
            if !entry.plugin.enabled.load(Ordering::Acquire)
                || entry.ignore_cancelled && event.is_cancelled()
            {
                continue;
            }
            let Callback::World(_, callback) = &entry.callback else {
                unreachable!()
            };
            if let Err(error) = call_listener(|| callback(event, level)) {
                self.world_listener_failed(&entry, error, event, level, false)?;
            }
        }
        Ok(())
    }
    fn world_listener_failed(
        &self,
        entry: &Registration,
        error: NativeListenerError,
        event: &mut NativeWorldEvent,
        level: &mut dyn WorldEventAccess,
        is_exception: bool,
    ) -> Result<(), StructureSpawnDispatchError> {
        let message = match error {
            NativeListenerError::AuthorNag(message) => {
                if entry.plugin.naggable.swap(false, Ordering::AcqRel) {
                    self.diagnostics
                        .report(StructureEventDiagnostic::AuthorNag {
                            plugin: entry.plugin.metadata.clone(),
                            message,
                        });
                }
                return Ok(());
            }
            NativeListenerError::Failure(message) => message,
        };
        self.diagnostics
            .report(StructureEventDiagnostic::ListenerFailure {
                plugin: entry.plugin.metadata.clone(),
                listener: entry.id,
                event_name: if is_exception {
                    "ServerExceptionEvent"
                } else {
                    event.kind().name()
                },
                message: message.clone(),
            });
        if !is_exception {
            let failure = WorldEventFailure {
                plugin: entry.plugin.metadata.clone(),
                listener: entry.id,
                message,
            };
            for listener in self.world_snapshot(event.kind(), true)? {
                if !listener.plugin.enabled.load(Ordering::Acquire) {
                    continue;
                }
                let Callback::WorldException(callback) = &listener.callback else {
                    unreachable!()
                };
                if let Err(error) = call_listener(|| callback(&failure, event, level)) {
                    self.world_listener_failed(&listener, error, event, level, true)?;
                }
            }
        }
        Ok(())
    }
}
