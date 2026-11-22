//! Native plugin lifecycle, panic containment, ownership, and synthetic host (#506).
//!
//! Mirrors Paper's plugin lifecycle states (`org.bukkit.plugin.Plugin` /
//! `io.papermc.paper.plugin.lifecycle.event.LifecycleEventOwner`) without
//! exposing CraftBukkit object graphs, arena pointers, or Rust references
//! across the plugin boundary.

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::commands::{
    CommandDispatchOutcome, CommandError, CommandHandle, CommandRegistry, CommandSender,
    CommandSpec,
};
use super::events::{
    EventBus, EventDispatchReport, EventError, EventListenerHandle, EventPriority, NativeEvent,
};
use super::handle::{Handle, HandleError, HandleTable, PluginId};
use super::scheduler::{ScheduleError, SyncScheduler, TaskContext, TaskHandle, TickReport};
use super::version::{ApiVersion, Capability, CapabilitySet, LoadingMode, NegotiationError};

/// Marker type for handles referring to registered plugins.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PluginMarker;

/// Marker type for host-owned opaque synthetic resources allocated by a plugin.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ResourceMarker;

/// Generational handle to a registered plugin managed by [`NativePluginHost`].
pub type PluginHandle = Handle<PluginMarker>;

/// Generational handle to a host-owned resource allocated through [`PluginContext`].
pub type ResourceHandle = Handle<ResourceMarker>;

/// Lifecycle phase of a registered native plugin.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PluginState {
    Registered,
    Loaded,
    Enabled,
    Disabled,
    Failed,
}

/// Lifecycle hook phase reported in errors.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LifecyclePhase {
    Register,
    Load,
    Enable,
    Disable,
    Unload,
}

/// Static metadata declared by a native plugin at registration time.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: String,
    pub target_api: ApiVersion,
    pub required_capabilities: CapabilitySet,
    pub optional_capabilities: CapabilitySet,
    pub loading_mode: LoadingMode,
}

impl PluginDescriptor {
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            target_api: ApiVersion::CURRENT,
            required_capabilities: CapabilitySet::synthetic_default(),
            optional_capabilities: CapabilitySet::new(),
            loading_mode: LoadingMode::StaticInProcess,
        }
    }
}

/// Errors produced by [`NativePluginHost`] lifecycle operations.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum LifecycleError {
    Negotiation(NegotiationError),
    Handle(HandleError),
    InvalidStateTransition {
        plugin: String,
        from: PluginState,
        phase: LifecyclePhase,
    },
    PluginReturnedError {
        plugin: String,
        phase: LifecyclePhase,
        message: String,
    },
    PluginPanicked {
        plugin: String,
        phase: LifecyclePhase,
        message: String,
    },
    CapabilityNotGranted(Capability),
    Event(EventError),
    Command(CommandError),
    Schedule(ScheduleError),
}

impl From<EventError> for LifecycleError {
    fn from(value: EventError) -> Self {
        Self::Event(value)
    }
}

impl From<CommandError> for LifecycleError {
    fn from(value: CommandError) -> Self {
        Self::Command(value)
    }
}

impl From<ScheduleError> for LifecycleError {
    fn from(value: ScheduleError) -> Self {
        Self::Schedule(value)
    }
}

impl From<NegotiationError> for LifecycleError {
    fn from(value: NegotiationError) -> Self {
        Self::Negotiation(value)
    }
}

impl From<HandleError> for LifecycleError {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Negotiation(err) => write!(f, "negotiation failed: {err}"),
            Self::Handle(err) => write!(f, "handle error: {err}"),
            Self::InvalidStateTransition {
                plugin,
                from,
                phase,
            } => write!(
                f,
                "plugin {plugin}: cannot run {phase:?} from state {from:?}"
            ),
            Self::PluginReturnedError {
                plugin,
                phase,
                message,
            } => write!(f, "plugin {plugin} failed in {phase:?}: {message}"),
            Self::PluginPanicked {
                plugin,
                phase,
                message,
            } => write!(f, "plugin {plugin} panicked in {phase:?}: {message}"),
            Self::CapabilityNotGranted(cap) => {
                write!(f, "capability not granted to plugin: {cap:?}")
            }
            Self::Event(err) => write!(f, "{err}"),
            Self::Command(err) => write!(f, "{err}"),
            Self::Schedule(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

/// Safe, value-oriented context passed into native plugin lifecycle hooks.
///
/// All host state is accessed via copyable generational handles and owned
/// values; no arena pointer or borrowed host reference escapes to the plugin.
pub struct PluginContext<'a> {
    plugin_id: PluginId,
    plugin_name: &'a str,
    host_api: ApiVersion,
    granted_capabilities: &'a CapabilitySet,
    resources: &'a mut HandleTable<ResourceMarker, String>,
    events: &'a mut EventBus,
    commands: &'a mut CommandRegistry,
    scheduler: &'a mut SyncScheduler,
    logs: &'a mut Vec<String>,
}

impl<'a> PluginContext<'a> {
    #[must_use]
    pub const fn plugin_id(&self) -> PluginId {
        self.plugin_id
    }

    #[must_use]
    pub const fn host_api_version(&self) -> ApiVersion {
        self.host_api
    }

    #[must_use]
    pub const fn granted_capabilities(&self) -> &CapabilitySet {
        self.granted_capabilities
    }

    pub fn alloc_resource(
        &mut self,
        payload: impl Into<String>,
    ) -> Result<ResourceHandle, HandleError> {
        self.resources.insert(self.plugin_id, payload.into())
    }

    pub fn read_resource(&self, handle: ResourceHandle) -> Result<String, HandleError> {
        self.resources.get(handle, Some(self.plugin_id)).cloned()
    }

    pub fn update_resource(
        &mut self,
        handle: ResourceHandle,
        payload: impl Into<String>,
    ) -> Result<(), HandleError> {
        let slot = self.resources.get_mut(handle, Some(self.plugin_id))?;
        *slot = payload.into();
        Ok(())
    }

    pub fn free_resource(&mut self, handle: ResourceHandle) -> Result<String, HandleError> {
        self.resources.remove(handle, Some(self.plugin_id))
    }

    pub fn log(&mut self, message: impl AsRef<str>) {
        self.logs
            .push(format!("[{}] {}", self.plugin_name, message.as_ref()));
    }

    pub fn subscribe_event<E, F>(
        &mut self,
        priority: EventPriority,
        ignore_cancelled: bool,
        handler: F,
    ) -> Result<EventListenerHandle, LifecycleError>
    where
        E: NativeEvent,
        F: FnMut(&mut E) + 'static,
    {
        if !self
            .granted_capabilities
            .contains(Capability::EventSubscription)
        {
            return Err(LifecycleError::CapabilityNotGranted(
                Capability::EventSubscription,
            ));
        }
        Ok(self
            .events
            .register(self.plugin_id, priority, ignore_cancelled, handler)?)
    }

    pub fn unsubscribe_event(&mut self, handle: EventListenerHandle) -> Result<(), LifecycleError> {
        Ok(self.events.unregister(self.plugin_id, handle)?)
    }

    pub fn register_command<F>(
        &mut self,
        spec: CommandSpec,
        executor: F,
    ) -> Result<CommandHandle, LifecycleError>
    where
        F: FnMut(&CommandSender, &str, &[&str]) -> Result<(), String> + 'static,
    {
        if !self
            .granted_capabilities
            .contains(Capability::CommandRegistration)
        {
            return Err(LifecycleError::CapabilityNotGranted(
                Capability::CommandRegistration,
            ));
        }
        Ok(self
            .commands
            .register(self.plugin_id, self.plugin_name, spec, executor)?)
    }

    pub fn unregister_command(&mut self, handle: CommandHandle) -> Result<(), LifecycleError> {
        Ok(self.commands.unregister(self.plugin_id, handle)?)
    }

    pub fn schedule_delayed<F>(
        &mut self,
        delay_ticks: u64,
        callback: F,
    ) -> Result<TaskHandle, LifecycleError>
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        if !self
            .granted_capabilities
            .contains(Capability::TickScheduler)
        {
            return Err(LifecycleError::CapabilityNotGranted(
                Capability::TickScheduler,
            ));
        }
        Ok(self
            .scheduler
            .schedule_delayed(self.plugin_id, delay_ticks, callback)?)
    }

    pub fn schedule_repeating<F>(
        &mut self,
        delay_ticks: u64,
        period_ticks: u64,
        callback: F,
    ) -> Result<TaskHandle, LifecycleError>
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        if !self
            .granted_capabilities
            .contains(Capability::TickScheduler)
        {
            return Err(LifecycleError::CapabilityNotGranted(
                Capability::TickScheduler,
            ));
        }
        Ok(self.scheduler.schedule_repeating(
            self.plugin_id,
            delay_ticks,
            period_ticks,
            callback,
        )?)
    }

    pub fn cancel_task(&mut self, handle: TaskHandle) -> Result<(), LifecycleError> {
        Ok(self.scheduler.cancel(self.plugin_id, handle)?)
    }
}

/// Synchronous native Rust plugin contract executed on the tick/host thread.
pub trait NativePlugin {
    fn descriptor(&self) -> PluginDescriptor;

    fn on_load(&mut self, _ctx: &mut PluginContext<'_>) -> Result<(), String> {
        Ok(())
    }

    fn on_enable(&mut self, _ctx: &mut PluginContext<'_>) -> Result<(), String> {
        Ok(())
    }

    fn on_disable(&mut self, _ctx: &mut PluginContext<'_>) -> Result<(), String> {
        Ok(())
    }
}

struct PluginSlot {
    id: PluginId,
    descriptor: PluginDescriptor,
    granted_capabilities: CapabilitySet,
    state: PluginState,
    instance: Box<dyn NativePlugin>,
}

/// Synthetic host backend implementing registration, version/capability
/// negotiation, generational handle tables, panic containment, and automatic
/// ownership cleanup.
pub struct NativePluginHost {
    api_version: ApiVersion,
    capabilities: CapabilitySet,
    next_plugin_id: u32,
    registered_names: BTreeSet<String>,
    plugins: HandleTable<PluginMarker, PluginSlot>,
    resources: HandleTable<ResourceMarker, String>,
    events: EventBus,
    commands: CommandRegistry,
    scheduler: SyncScheduler,
    logs: Vec<String>,
}

impl NativePluginHost {
    #[must_use]
    pub fn new(api_version: ApiVersion, capabilities: CapabilitySet) -> Self {
        Self {
            api_version,
            capabilities,
            next_plugin_id: 1,
            registered_names: BTreeSet::new(),
            plugins: HandleTable::new(),
            resources: HandleTable::new(),
            events: EventBus::new(),
            commands: CommandRegistry::new(),
            scheduler: SyncScheduler::new(),
            logs: Vec::new(),
        }
    }

    #[must_use]
    pub fn synthetic_default() -> Self {
        Self::new(ApiVersion::CURRENT, CapabilitySet::synthetic_default())
    }

    /// Creates a synthetic host with lifecycle, synthetic resources, events,
    /// commands, and synchronous tick-scheduler capabilities enabled.
    #[must_use]
    pub fn with_callback_capabilities() -> Self {
        let mut caps = CapabilitySet::synthetic_default();
        caps.insert(Capability::EventSubscription);
        caps.insert(Capability::CommandRegistration);
        caps.insert(Capability::TickScheduler);
        Self::new(ApiVersion::CURRENT, caps)
    }

    #[must_use]
    pub const fn events(&self) -> &EventBus {
        &self.events
    }

    #[must_use]
    pub fn events_mut(&mut self) -> &mut EventBus {
        &mut self.events
    }

    #[must_use]
    pub const fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    #[must_use]
    pub fn commands_mut(&mut self) -> &mut CommandRegistry {
        &mut self.commands
    }

    #[must_use]
    pub const fn scheduler(&self) -> &SyncScheduler {
        &self.scheduler
    }

    #[must_use]
    pub fn scheduler_mut(&mut self) -> &mut SyncScheduler {
        &mut self.scheduler
    }

    /// Dispatches a typed synchronous event across all registered listeners.
    pub fn dispatch_event<E: NativeEvent>(&mut self, event: &mut E) -> EventDispatchReport {
        self.events.dispatch(event)
    }

    /// Dispatches a command line to the registered command registry.
    pub fn dispatch_command(
        &mut self,
        sender: &CommandSender,
        command_line: &str,
    ) -> CommandDispatchOutcome {
        self.commands.dispatch(sender, command_line)
    }

    /// Advances the synchronous scheduler by one server tick.
    pub fn tick_scheduler(&mut self) -> TickReport {
        self.scheduler.tick()
    }

    /// Shuts down all callbacks, listeners, commands, and scheduled tasks cleanly.
    pub fn shutdown_callbacks(&mut self) {
        let _ = self.events.clear();
        let _ = self.commands.clear();
        let _ = self.scheduler.shutdown();
    }

    #[must_use]
    pub const fn api_version(&self) -> ApiVersion {
        self.api_version
    }

    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    #[must_use]
    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    #[must_use]
    pub const fn live_resource_count(&self) -> usize {
        self.resources.live_count()
    }

    pub fn live_resource_count_for(&self, handle: PluginHandle) -> Result<usize, LifecycleError> {
        let slot = self.plugins.get(handle, None)?;
        Ok(self.resources.live_count_for(slot.id))
    }

    pub fn state(&self, handle: PluginHandle) -> Result<PluginState, LifecycleError> {
        Ok(self.plugins.get(handle, None)?.state)
    }

    pub fn granted_capabilities(
        &self,
        handle: PluginHandle,
    ) -> Result<&CapabilitySet, LifecycleError> {
        Ok(&self.plugins.get(handle, None)?.granted_capabilities)
    }

    /// Reads a host-owned resource on behalf of `plugin_handle`, enforcing both
    /// generational validity and per-plugin ownership.
    pub fn read_resource_as(
        &self,
        plugin_handle: PluginHandle,
        resource_handle: ResourceHandle,
    ) -> Result<String, LifecycleError> {
        let slot = self.plugins.get(plugin_handle, None)?;
        Ok(self.resources.get(resource_handle, Some(slot.id))?.clone())
    }

    /// Registers a plugin after validating its name, loading mode, API version,
    /// and required capabilities.
    pub fn register(
        &mut self,
        plugin: Box<dyn NativePlugin>,
    ) -> Result<PluginHandle, LifecycleError> {
        let descriptor = match catch_unwind(AssertUnwindSafe(|| plugin.descriptor())) {
            Ok(descriptor) => descriptor,
            Err(payload) => {
                let message = extract_panic_message_and_drop(payload);
                let _ = drop_plugin_contained(plugin);
                return Err(LifecycleError::PluginPanicked {
                    plugin: "<unregistered>".to_owned(),
                    phase: LifecyclePhase::Register,
                    message,
                });
            }
        };

        let granted = match self.validate_registration(&descriptor) {
            Ok(granted) => granted,
            Err(err) => {
                let _ = drop_plugin_contained(plugin);
                return Err(err.into());
            }
        };

        let Some(next_id) = self.next_plugin_id.checked_add(1) else {
            let _ = drop_plugin_contained(plugin);
            return Err(HandleError::TableExhausted.into());
        };
        let id = PluginId::new(self.next_plugin_id);

        let name = descriptor.name.clone();
        let slot = PluginSlot {
            id,
            descriptor,
            granted_capabilities: granted,
            state: PluginState::Registered,
            instance: plugin,
        };
        let handle = self.plugins.insert(id, slot)?;
        self.next_plugin_id = next_id;
        self.registered_names.insert(name);
        Ok(handle)
    }

    fn validate_registration(
        &self,
        descriptor: &PluginDescriptor,
    ) -> Result<CapabilitySet, NegotiationError> {
        validate_plugin_name(&descriptor.name)?;
        if self.registered_names.contains(&descriptor.name) {
            return Err(NegotiationError::DuplicatePluginName(
                descriptor.name.clone(),
            ));
        }
        match &descriptor.loading_mode {
            LoadingMode::StaticInProcess => {}
            other @ LoadingMode::DynamicCdylib { .. } => {
                return Err(NegotiationError::UnsupportedLoadingMode(other.clone()));
            }
        }
        if !descriptor
            .target_api
            .is_compatible_with_host(self.api_version)
        {
            return Err(NegotiationError::IncompatibleApiVersion {
                host: self.api_version,
                requested: descriptor.target_api,
            });
        }
        let missing = descriptor
            .required_capabilities
            .missing_from(&self.capabilities);
        if !missing.is_empty() {
            return Err(NegotiationError::MissingCapabilities(missing));
        }

        let mut granted = descriptor.required_capabilities.clone();
        for cap in descriptor
            .optional_capabilities
            .intersection(&self.capabilities)
            .iter()
        {
            granted.insert(cap);
        }
        Ok(granted)
    }

    pub fn load(&mut self, handle: PluginHandle) -> Result<(), LifecycleError> {
        let slot = self.plugins.get_mut(handle, None)?;
        if slot.state != PluginState::Registered {
            return Err(LifecycleError::InvalidStateTransition {
                plugin: slot.descriptor.name.clone(),
                from: slot.state,
                phase: LifecyclePhase::Load,
            });
        }
        let plugin_id = slot.id;
        let plugin_name = slot.descriptor.name.clone();
        let outcome = run_hook_contained(
            slot,
            self.api_version,
            &mut self.resources,
            &mut self.events,
            &mut self.commands,
            &mut self.scheduler,
            &mut self.logs,
            LifecyclePhase::Load,
            |plugin, ctx| plugin.on_load(ctx),
        );
        match outcome {
            Ok(()) => {
                slot.state = PluginState::Loaded;
                Ok(())
            }
            Err(err) => {
                slot.state = PluginState::Failed;
                let _ = self.resources.revoke_owned_by(plugin_id);
                let _ = self.events.unregister_all_for_plugin(plugin_id);
                let _ = self.commands.unregister_all_for_plugin(plugin_id);
                let _ = self.scheduler.cancel_all_for_plugin(plugin_id);
                let _ = plugin_name;
                Err(err)
            }
        }
    }

    pub fn enable(&mut self, handle: PluginHandle) -> Result<(), LifecycleError> {
        let slot = self.plugins.get_mut(handle, None)?;
        if !matches!(slot.state, PluginState::Loaded | PluginState::Disabled) {
            return Err(LifecycleError::InvalidStateTransition {
                plugin: slot.descriptor.name.clone(),
                from: slot.state,
                phase: LifecyclePhase::Enable,
            });
        }
        let plugin_id = slot.id;
        let outcome = run_hook_contained(
            slot,
            self.api_version,
            &mut self.resources,
            &mut self.events,
            &mut self.commands,
            &mut self.scheduler,
            &mut self.logs,
            LifecyclePhase::Enable,
            |plugin, ctx| plugin.on_enable(ctx),
        );
        match outcome {
            Ok(()) => {
                slot.state = PluginState::Enabled;
                Ok(())
            }
            Err(err) => {
                slot.state = PluginState::Failed;
                let _ = self.resources.revoke_owned_by(plugin_id);
                let _ = self.events.unregister_all_for_plugin(plugin_id);
                let _ = self.commands.unregister_all_for_plugin(plugin_id);
                let _ = self.scheduler.cancel_all_for_plugin(plugin_id);
                Err(err)
            }
        }
    }

    /// Disables an enabled plugin and unconditionally revokes all host
    /// resources owned by it, even if `on_disable` returns an error or panics.
    pub fn disable(&mut self, handle: PluginHandle) -> Result<(), LifecycleError> {
        let slot = self.plugins.get_mut(handle, None)?;
        if slot.state != PluginState::Enabled {
            return Err(LifecycleError::InvalidStateTransition {
                plugin: slot.descriptor.name.clone(),
                from: slot.state,
                phase: LifecyclePhase::Disable,
            });
        }
        let plugin_id = slot.id;
        let outcome = run_hook_contained(
            slot,
            self.api_version,
            &mut self.resources,
            &mut self.events,
            &mut self.commands,
            &mut self.scheduler,
            &mut self.logs,
            LifecyclePhase::Disable,
            |plugin, ctx| plugin.on_disable(ctx),
        );
        let _ = self.resources.revoke_owned_by(plugin_id);
        let _ = self.events.unregister_all_for_plugin(plugin_id);
        let _ = self.commands.unregister_all_for_plugin(plugin_id);
        let _ = self.scheduler.cancel_all_for_plugin(plugin_id);
        match outcome {
            Ok(()) => {
                slot.state = PluginState::Disabled;
                Ok(())
            }
            Err(err) => {
                slot.state = PluginState::Failed;
                Err(err)
            }
        }
    }

    /// Unloads a non-enabled plugin, revokes any remaining resources, removes
    /// its name reservation, and bumps its [`PluginHandle`] generation so any
    /// retained handle immediately fails with [`HandleError::Stale`].
    pub fn unload(&mut self, handle: PluginHandle) -> Result<(), LifecycleError> {
        {
            let slot = self.plugins.get(handle, None)?;
            if slot.state == PluginState::Enabled {
                return Err(LifecycleError::InvalidStateTransition {
                    plugin: slot.descriptor.name.clone(),
                    from: slot.state,
                    phase: LifecyclePhase::Unload,
                });
            }
        }
        let PluginSlot {
            id,
            descriptor,
            instance,
            ..
        } = self.plugins.remove(handle, None)?;
        let _ = self.resources.revoke_owned_by(id);
        let _ = self.events.unregister_all_for_plugin(id);
        let _ = self.commands.unregister_all_for_plugin(id);
        let _ = self.scheduler.cancel_all_for_plugin(id);
        self.registered_names.remove(&descriptor.name);
        drop_plugin_contained(instance).map_err(|message| LifecycleError::PluginPanicked {
            plugin: descriptor.name,
            phase: LifecyclePhase::Unload,
            message,
        })?;
        Ok(())
    }
}

fn validate_plugin_name(name: &str) -> Result<(), NegotiationError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if !valid {
        return Err(NegotiationError::InvalidPluginName(name.to_owned()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_hook_contained<F>(
    slot: &mut PluginSlot,
    host_api: ApiVersion,
    resources: &mut HandleTable<ResourceMarker, String>,
    events: &mut EventBus,
    commands: &mut CommandRegistry,
    scheduler: &mut SyncScheduler,
    logs: &mut Vec<String>,
    phase: LifecyclePhase,
    hook: F,
) -> Result<(), LifecycleError>
where
    F: FnOnce(&mut dyn NativePlugin, &mut PluginContext<'_>) -> Result<(), String>,
{
    let plugin_id = slot.id;
    let plugin_name = slot.descriptor.name.as_str();
    let granted = &slot.granted_capabilities;
    let instance: &mut dyn NativePlugin = slot.instance.as_mut();

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut ctx = PluginContext {
            plugin_id,
            plugin_name,
            host_api,
            granted_capabilities: granted,
            resources,
            events,
            commands,
            scheduler,
            logs,
        };
        hook(instance, &mut ctx)
    }));

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(message)) => Err(LifecycleError::PluginReturnedError {
            plugin: slot.descriptor.name.clone(),
            phase,
            message,
        }),
        Err(payload) => {
            let message = extract_panic_message_and_drop(payload);
            Err(LifecycleError::PluginPanicked {
                plugin: slot.descriptor.name.clone(),
                phase,
                message,
            })
        }
    }
}

fn drop_plugin_contained(plugin: Box<dyn NativePlugin>) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(move || drop(plugin))).map_err(extract_panic_message_and_drop)
}

pub(crate) fn extract_panic_message_and_drop(payload: Box<dyn Any + Send>) -> String {
    let message = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    };
    if let Err(secondary) = catch_unwind(AssertUnwindSafe(move || drop(payload))) {
        std::mem::forget(secondary);
    }
    message
}

/// Minimal lifecycle fixture plugin used for synthetic conformance verification.
pub struct MinimalLifecyclePlugin {
    descriptor: PluginDescriptor,
    pub allocated_handle: Option<ResourceHandle>,
    pub observed_events: Vec<&'static str>,
}

impl MinimalLifecyclePlugin {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            descriptor: PluginDescriptor::new(name, "0.1.0"),
            allocated_handle: None,
            observed_events: Vec::new(),
        }
    }
}

impl NativePlugin for MinimalLifecyclePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn on_load(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        self.observed_events.push("load");
        ctx.log("loaded");
        Ok(())
    }

    fn on_enable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        self.observed_events.push("enable");
        let handle = ctx
            .alloc_resource("minimal-state")
            .map_err(|e| e.to_string())?;
        self.allocated_handle = Some(handle);
        ctx.log("enabled");
        Ok(())
    }

    fn on_disable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        self.observed_events.push("disable");
        if let Some(handle) = self.allocated_handle.take() {
            let _ = ctx.free_resource(handle);
        }
        ctx.log("disabled");
        Ok(())
    }
}
