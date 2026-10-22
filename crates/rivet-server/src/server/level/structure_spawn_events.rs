//! Explicit native authority for Paper's AsyncStructureSpawnEvent boundary.
//! No default plugin policy: an unavailable/JVM-pending host cannot masquerade
//! as an empty listener registry. Native hosts explicitly register lifecycle,
//! finish loading, and install this authority. World chunks stay tick-owned;
//! only immutable event identity/data cross onto the dispatch worker.
use rivet_registry::{
    Holder, Identifier, RegistryAccess, ResourceKey, core::ChunkPos, holder_lookup::HolderGetter,
};
use rivet_world::levelgen::structure::{
    BoundingBox, registry_keys::STRUCTURE, structure_registry::Structure,
};
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};

#[cfg(test)]
#[path = "structure_spawn_events_test.rs"]
mod tests;

static NEXT_WORLD: AtomicU64 = AtomicU64::new(1);
static NEXT_REGISTRATION: AtomicU64 = AtomicU64::new(1);
fn next_id(counter: &AtomicU64) -> u64 {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("event identity exhausted")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureWorldIdentity {
    owner: u64,
    seed: i64,
    dimension: Identifier,
}
impl StructureWorldIdentity {
    pub fn new(seed: i64, dimension: Identifier) -> Self {
        Self {
            owner: next_id(&NEXT_WORLD),
            seed,
            dimension,
        }
    }
    pub fn seed(&self) -> i64 {
        self.seed
    }
    pub fn dimension(&self) -> &Identifier {
        &self.dimension
    }
}
#[derive(Clone)]
pub struct StructureSpawnPayload {
    world: StructureWorldIdentity,
    access: RegistryAccess,
    structure: Holder<Structure>,
    key: Identifier,
    bounds: BoundingBox,
    chunk: ChunkPos,
}
impl StructureSpawnPayload {
    pub fn new(
        world: StructureWorldIdentity,
        access: &RegistryAccess,
        key: Identifier,
        bounds: BoundingBox,
        chunk: ChunkPos,
    ) -> Result<Self, StructureSpawnDispatchError> {
        let registry = access.lookup(&STRUCTURE).ok_or_else(|| {
            StructureSpawnDispatchError::AuthorityUnavailable("STRUCTURE registry absent".into())
        })?;
        let structure = registry
            .get(&ResourceKey::create(&STRUCTURE, key.clone()))
            .ok_or_else(|| {
                StructureSpawnDispatchError::AuthorityUnavailable(format!(
                    "unregistered structure {key}"
                ))
            })?;
        Ok(Self {
            world,
            access: access.clone(),
            structure,
            key,
            bounds,
            chunk,
        })
    }
    pub fn world(&self) -> &StructureWorldIdentity {
        &self.world
    }
    pub fn structure_key(&self) -> &Identifier {
        &self.key
    }
    pub fn structure(&self) -> &Structure {
        self.structure.value(
            self.access
                .lookup(&STRUCTURE)
                .expect("payload retains registry"),
        )
    }
    pub fn bounds(&self) -> BoundingBox {
        self.bounds
    }
    pub fn chunk(&self) -> ChunkPos {
        self.chunk
    }
}
impl std::fmt::Debug for StructureSpawnPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructureSpawnPayload")
            .field("world", &self.world)
            .field("structure", &self.key)
            .field("bounds", &self.bounds)
            .field("chunk", &self.chunk)
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnDecision {
    Accepted,
    Cancelled,
}
#[derive(Default, Debug)]
pub struct StructureSpawnControl {
    cancelled: bool,
}
impl StructureSpawnControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn set_cancelled(&mut self, value: bool) {
        self.cancelled = value;
    }
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StructureSpawnDispatchError {
    #[error("structure event authority unavailable: {0}")]
    AuthorityUnavailable(String),
    #[error("AsyncStructureSpawnEvent may only be triggered asynchronously.")]
    PrimaryThread,
    #[error("native plugin listener lifecycle has not finished loading")]
    LifecycleLoading,
    #[error("plugin {0} is not enabled")]
    PluginDisabled(u64),
    #[error("unknown plugin {0}")]
    UnknownPlugin(u64),
    #[error("listener registry is poisoned")]
    RegistryPoisoned,
    #[error("event worker could not start: {0}")]
    WorkerUnavailable(String),
    #[error("event worker failed outside a listener: {0}")]
    WorkerFailed(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativePluginId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeListenerId(u64);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePluginMetadata {
    pub display_name: String,
    pub authors: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventPriority {
    Lowest,
    Low,
    Normal,
    High,
    Highest,
    Monitor,
}
impl EventPriority {
    fn index(self) -> usize {
        match self {
            Self::Lowest => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::High => 3,
            Self::Highest => 4,
            Self::Monitor => 5,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeListenerError {
    AuthorNag(String),
    Failure(String),
}
#[derive(Clone, Debug)]
pub struct StructureServerEventException {
    pub plugin: NativePluginMetadata,
    pub listener: NativeListenerId,
    pub event: Arc<StructureSpawnPayload>,
    pub cancelled_at_failure: bool,
    pub message: String,
}
#[derive(Clone, Debug)]
pub enum StructureEventDiagnostic {
    AuthorNag {
        plugin: NativePluginMetadata,
        message: String,
    },
    ListenerFailure {
        plugin: NativePluginMetadata,
        listener: NativeListenerId,
        event_name: &'static str,
        message: String,
    },
}
/// A real host logger is required; dispatch never silently discards failures.
pub trait StructureEventDiagnostics: Send + Sync {
    fn report(&self, diagnostic: StructureEventDiagnostic);
}
pub struct TracingStructureEventDiagnostics;
impl StructureEventDiagnostics for TracingStructureEventDiagnostics {
    fn report(&self, diagnostic: StructureEventDiagnostic) {
        tracing::error!(?diagnostic, "native structure event listener failure");
    }
}
type SpawnListener = dyn Fn(&StructureSpawnPayload, &mut StructureSpawnControl) -> Result<(), NativeListenerError>
    + Send
    + Sync;
// ServerEventException.getEvent() exposes the original cancellable event.
// Metadata stays immutable; its original cancellation control remains live.
type ExceptionListener = dyn Fn(
        &StructureServerEventException,
        &mut StructureSpawnControl,
    ) -> Result<(), NativeListenerError>
    + Send
    + Sync;
struct Plugin {
    id: NativePluginId,
    metadata: NativePluginMetadata,
    enabled: AtomicBool,
    naggable: AtomicBool,
}
enum Callback {
    Spawn(Arc<SpawnListener>),
    Exception(Arc<ExceptionListener>),
}
struct Registration {
    id: NativeListenerId,
    plugin: Arc<Plugin>,
    ignore_cancelled: bool,
    callback: Callback,
}
struct Listeners {
    plugins: HashMap<NativePluginId, Arc<Plugin>>,
    spawn: [Vec<Arc<Registration>>; 6],
    exception: [Vec<Arc<Registration>>; 6],
}
pub struct NativeStructureEventRegistry {
    primary_thread: ThreadId,
    loading: AtomicBool,
    listeners: RwLock<Listeners>,
    diagnostics: Arc<dyn StructureEventDiagnostics>,
}
impl NativeStructureEventRegistry {
    /// Primary thread identity must come from the actual server runtime, not
    /// from whichever worker happens to construct a pending bridge.
    pub fn new(primary_thread: ThreadId, diagnostics: Arc<dyn StructureEventDiagnostics>) -> Self {
        Self {
            primary_thread,
            loading: AtomicBool::new(true),
            listeners: RwLock::new(Listeners {
                plugins: HashMap::new(),
                spawn: std::array::from_fn(|_| Vec::new()),
                exception: std::array::from_fn(|_| Vec::new()),
            }),
            diagnostics,
        }
    }
    /// Explicit host lifecycle transition. This certifies only this actual
    /// native registry, never the presence or completion of a JVM bridge.
    pub fn finish_loading(&self) {
        self.loading.store(false, Ordering::Release);
    }
    pub fn enable_plugin(
        &self,
        metadata: NativePluginMetadata,
    ) -> Result<NativePluginId, StructureSpawnDispatchError> {
        let id = NativePluginId(next_id(&NEXT_REGISTRATION));
        self.listeners
            .write()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?
            .plugins
            .insert(
                id,
                Arc::new(Plugin {
                    id,
                    metadata,
                    enabled: AtomicBool::new(true),
                    naggable: AtomicBool::new(true),
                }),
            );
        Ok(id)
    }
    pub fn disable_plugin(&self, id: NativePluginId) -> Result<(), StructureSpawnDispatchError> {
        let mut lists = self
            .listeners
            .write()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let plugin = lists
            .plugins
            .get(&id)
            .ok_or(StructureSpawnDispatchError::UnknownPlugin(id.0))?;
        plugin.enabled.store(false, Ordering::Release);
        for bucket in lists.spawn.iter_mut() {
            bucket.retain(|entry| entry.plugin.id != id);
        }
        for bucket in lists.exception.iter_mut() {
            bucket.retain(|entry| entry.plugin.id != id);
        }
        Ok(())
    }
    /// Re-enable the same plugin identity; the host registers its listeners
    /// again, since disabling removed them from the live handler lists.
    pub fn reenable_plugin(&self, id: NativePluginId) -> Result<(), StructureSpawnDispatchError> {
        let lists = self
            .listeners
            .read()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let plugin = lists
            .plugins
            .get(&id)
            .ok_or(StructureSpawnDispatchError::UnknownPlugin(id.0))?;
        plugin.enabled.store(true, Ordering::Release);
        Ok(())
    }
    pub fn unregister_listener(
        &self,
        id: NativeListenerId,
    ) -> Result<bool, StructureSpawnDispatchError> {
        let mut lists = self
            .listeners
            .write()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let mut removed = false;
        for bucket in lists.spawn.iter_mut() {
            let old = bucket.len();
            bucket.retain(|entry| entry.id != id);
            removed |= old != bucket.len();
        }
        for bucket in lists.exception.iter_mut() {
            let old = bucket.len();
            bucket.retain(|entry| entry.id != id);
            removed |= old != bucket.len();
        }
        Ok(removed)
    }
    pub fn register_spawn_listener(
        &self,
        plugin: NativePluginId,
        priority: EventPriority,
        ignore_cancelled: bool,
        callback: Arc<SpawnListener>,
    ) -> Result<NativeListenerId, StructureSpawnDispatchError> {
        self.register(
            plugin,
            priority,
            ignore_cancelled,
            Callback::Spawn(callback),
        )
    }
    pub fn register_exception_listener(
        &self,
        plugin: NativePluginId,
        priority: EventPriority,
        callback: Arc<ExceptionListener>,
    ) -> Result<NativeListenerId, StructureSpawnDispatchError> {
        self.register(plugin, priority, false, Callback::Exception(callback))
    }
    fn register(
        &self,
        plugin: NativePluginId,
        priority: EventPriority,
        ignore_cancelled: bool,
        callback: Callback,
    ) -> Result<NativeListenerId, StructureSpawnDispatchError> {
        let mut lists = self
            .listeners
            .write()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let plugin = Arc::clone(
            lists
                .plugins
                .get(&plugin)
                .ok_or(StructureSpawnDispatchError::UnknownPlugin(plugin.0))?,
        );
        if !plugin.enabled.load(Ordering::Acquire) {
            return Err(StructureSpawnDispatchError::PluginDisabled(plugin.id.0));
        }
        let id = NativeListenerId(next_id(&NEXT_REGISTRATION));
        let exception = matches!(callback, Callback::Exception(_));
        let entry = Arc::new(Registration {
            id,
            plugin,
            ignore_cancelled,
            callback,
        });
        if exception {
            lists.exception[priority.index()].push(entry);
        } else {
            lists.spawn[priority.index()].push(entry);
        }
        Ok(id)
    }
    fn snapshot(
        &self,
        exception: bool,
    ) -> Result<Vec<Arc<Registration>>, StructureSpawnDispatchError> {
        let lists = self
            .listeners
            .read()
            .map_err(|_| StructureSpawnDispatchError::RegistryPoisoned)?;
        let buckets = if exception {
            &lists.exception
        } else {
            &lists.spawn
        };
        Ok(buckets.iter().flatten().cloned().collect())
    }
    /// Exact event-manager thread guard runs BEFORE consulting listeners,
    /// including a real explicitly registered empty native listener registry.
    pub fn dispatch_on_current_thread(
        &self,
        event: StructureSpawnPayload,
    ) -> Result<SpawnDecision, StructureSpawnDispatchError> {
        if thread::current().id() == self.primary_thread {
            return Err(StructureSpawnDispatchError::PrimaryThread);
        }
        if self.loading.load(Ordering::Acquire) {
            return Err(StructureSpawnDispatchError::LifecycleLoading);
        }
        let event = Arc::new(event);
        let mut control = StructureSpawnControl::default();
        for entry in self.snapshot(false)? {
            if !entry.plugin.enabled.load(Ordering::Acquire)
                || (control.cancelled && entry.ignore_cancelled)
            {
                continue;
            }
            let Callback::Spawn(callback) = &entry.callback else {
                unreachable!()
            };
            if let Err(error) = call_listener(|| callback(&event, &mut control)) {
                self.handle_failure(&entry, error, Arc::clone(&event), &mut control, false)?;
            }
        }
        Ok(if control.cancelled {
            SpawnDecision::Cancelled
        } else {
            SpawnDecision::Accepted
        })
    }
    fn handle_failure(
        &self,
        entry: &Registration,
        error: NativeListenerError,
        event: Arc<StructureSpawnPayload>,
        control: &mut StructureSpawnControl,
        exception_event: bool,
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
                event_name: if exception_event {
                    "ServerExceptionEvent"
                } else {
                    "AsyncStructureSpawnEvent"
                },
                message: message.clone(),
            });
        if !exception_event {
            let failure = StructureServerEventException {
                plugin: entry.plugin.metadata.clone(),
                listener: entry.id,
                event,
                message,
                cancelled_at_failure: control.cancelled,
            };
            for listener in self.snapshot(true)? {
                if !listener.plugin.enabled.load(Ordering::Acquire) {
                    continue;
                }
                let Callback::Exception(callback) = &listener.callback else {
                    unreachable!()
                };
                if let Err(error) = call_listener(|| callback(&failure, control)) {
                    // Paper logs failures in ServerExceptionEvent listeners
                    // but never recursively emits another exception event.
                    self.handle_failure(
                        &listener,
                        error,
                        Arc::clone(&failure.event),
                        control,
                        true,
                    )?;
                }
            }
        }
        Ok(())
    }
}
// Keep the panic boundary local to listeners: infrastructure failures refuse
// generation, whereas source listener exceptions are reported and continued.
fn call_listener(
    call: impl FnOnce() -> Result<(), NativeListenerError>,
) -> Result<(), NativeListenerError> {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|payload| {
        Err(NativeListenerError::Failure(
            if let Some(message) = payload.downcast_ref::<String>() {
                message.clone()
            } else if let Some(message) = payload.downcast_ref::<&str>() {
                (*message).to_owned()
            } else {
                "non-string listener panic".into()
            },
        ))
    })
}

#[derive(Clone)]
pub enum StructureSpawnAuthority {
    Unavailable(String),
    RegisteredNative(Arc<NativeStructureEventRegistry>),
}
impl StructureSpawnAuthority {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable(reason.into())
    }
    pub fn registered_native(registry: Arc<NativeStructureEventRegistry>) -> Self {
        Self::RegisteredNative(registry)
    }
    pub fn dispatch(
        &self,
        event: StructureSpawnPayload,
    ) -> Result<SpawnDecision, StructureSpawnDispatchError> {
        let registry = match self {
            Self::RegisteredNative(registry) => Arc::clone(registry),
            Self::Unavailable(reason) => {
                return Err(StructureSpawnDispatchError::AuthorityUnavailable(
                    reason.clone(),
                ));
            }
        };
        thread::Builder::new()
            .name("structure-spawn-event".into())
            .spawn(move || registry.dispatch_on_current_thread(event))
            .map_err(|error| StructureSpawnDispatchError::WorkerUnavailable(error.to_string()))?
            .join()
            .map_err(|payload| {
                StructureSpawnDispatchError::WorkerFailed(
                    if let Some(message) = payload.downcast_ref::<&str>() {
                        (*message).into()
                    } else if let Some(message) = payload.downcast_ref::<String>() {
                        message.clone()
                    } else {
                        "non-string dispatcher panic".into()
                    },
                )
            })?
    }
}
