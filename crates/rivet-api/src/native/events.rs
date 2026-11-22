//! Synchronous tick-thread typed event bus for native Rust plugins (#507).
//!
//! Mirrors Paper/Bukkit's `org.bukkit.event.EventPriority` ordering
//! (`Lowest -> Low -> Normal -> High -> Highest -> Monitor`) and
//! `ignoreCancelled` semantics, with generational listener handles,
//! per-plugin ownership, reentrancy safety, panic containment, and
//! automatic cleanup on plugin disable/unload.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::handle::{Handle, HandleError, HandleTable, PluginId};
use super::lifecycle::extract_panic_message_and_drop;

/// Marker type for handles referring to registered event listeners.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct EventListenerMarker;

/// Generational handle to a registered event listener.
pub type EventListenerHandle = Handle<EventListenerMarker>;

/// Execution priority for event listeners, mirroring `org.bukkit.event.EventPriority`.
///
/// Listeners run in ascending slot order:
/// `Lowest (0) -> Low (1) -> Normal (2) -> High (3) -> Highest (4) -> Monitor (5)`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
#[repr(u8)]
pub enum EventPriority {
    Lowest = 0,
    Low = 1,
    #[default]
    Normal = 2,
    High = 3,
    Highest = 4,
    Monitor = 5,
}

impl EventPriority {
    /// Returns the Paper/Bukkit slot index (`0..=5`).
    #[must_use]
    pub const fn slot(self) -> u8 {
        self as u8
    }
}

/// Optional cancellation protocol for events that can be cancelled by listeners.
pub trait Cancellable {
    fn is_cancelled(&self) -> bool;
    fn set_cancelled(&mut self, cancelled: bool);
}

/// Trait implemented by typed synchronous events dispatched on [`EventBus`].
pub trait NativeEvent: Any + 'static {
    /// Stable human-readable event name for diagnostics and panic reports.
    const NAME: &'static str;

    /// Returns whether this event is currently cancelled (non-cancellable events return `false`).
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Panic captured while executing or dropping an event listener.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ListenerPanic {
    pub owner: PluginId,
    pub handle: EventListenerHandle,
    pub event_name: &'static str,
    pub priority: EventPriority,
    pub message: String,
}

/// Summary returned by [`EventBus::dispatch`].
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct EventDispatchReport {
    pub invoked: usize,
    pub skipped_cancelled: usize,
    pub panics: Vec<ListenerPanic>,
}

type ErasedListenerFn = Box<dyn FnMut(&mut dyn Any)>;

struct ListenerEntry {
    event_type: TypeId,
    priority: EventPriority,
    ignore_cancelled: bool,
    seq: u64,
    callback: Option<ErasedListenerFn>,
}

/// Errors produced by [`EventBus`] operations.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum EventError {
    Handle(HandleError),
}

impl From<HandleError> for EventError {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handle(err) => write!(f, "event listener handle error: {err}"),
        }
    }
}

impl std::error::Error for EventError {}

/// Synchronous tick-thread event bus with deterministic priority ordering,
/// reentrancy safety, panic containment, and per-plugin cleanup.
#[derive(Default)]
pub struct EventBus {
    listeners: HandleTable<EventListenerMarker, ListenerEntry>,
    /// Deterministic dispatch index: `(event_type, priority, seq) -> handle`.
    by_event: BTreeMap<(TypeId, EventPriority, u64), EventListenerHandle>,
    next_seq: u64,
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn listener_count(&self) -> usize {
        self.listeners.live_count()
    }

    #[must_use]
    pub fn listener_count_for(&self, owner: PluginId) -> usize {
        self.listeners.live_count_for(owner)
    }

    /// Registers a typed listener owned by `owner`.
    pub fn register<E, F>(
        &mut self,
        owner: PluginId,
        priority: EventPriority,
        ignore_cancelled: bool,
        mut handler: F,
    ) -> Result<EventListenerHandle, EventError>
    where
        E: NativeEvent,
        F: FnMut(&mut E) + 'static,
    {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let event_type = TypeId::of::<E>();
        let callback: ErasedListenerFn = Box::new(move |any_event: &mut dyn Any| {
            if let Some(event) = any_event.downcast_mut::<E>() {
                handler(event);
            }
        });
        let entry = ListenerEntry {
            event_type,
            priority,
            ignore_cancelled,
            seq,
            callback: Some(callback),
        };
        let handle = self.listeners.insert(owner, entry)?;
        self.by_event.insert((event_type, priority, seq), handle);
        Ok(handle)
    }

    /// Unregisters a single listener after verifying generational validity and ownership.
    ///
    /// Any panic in the listener closure's `Drop` implementation is contained.
    pub fn unregister(
        &mut self,
        owner: PluginId,
        handle: EventListenerHandle,
    ) -> Result<(), EventError> {
        let entry = self.listeners.remove(handle, Some(owner))?;
        self.by_event
            .remove(&(entry.event_type, entry.priority, entry.seq));
        drop_listener_contained(entry.callback);
        Ok(())
    }

    /// Unregisters and drops all listeners owned by `owner`.
    ///
    /// Called automatically when a plugin is disabled, fails, or unloads.
    pub fn unregister_all_for_plugin(&mut self, owner: PluginId) -> usize {
        let removed = self.listeners.revoke_owned_by(owner);
        let count = removed.len();
        for entry in removed {
            self.by_event
                .remove(&(entry.event_type, entry.priority, entry.seq));
            drop_listener_contained(entry.callback);
        }
        count
    }

    /// Dispatches `event` synchronously to all registered listeners for `E`
    /// in `(EventPriority, registration_seq)` order.
    ///
    /// Snapshot semantics ensure deterministic ordering even if a listener
    /// triggers reentrant registrations or unregistrations. Listener panics
    /// are contained so subsequent listeners still run.
    pub fn dispatch<E: NativeEvent>(&mut self, event: &mut E) -> EventDispatchReport {
        let event_type = TypeId::of::<E>();
        let snapshot: Vec<EventListenerHandle> = self
            .by_event
            .range(
                (event_type, EventPriority::Lowest, 0)
                    ..=(event_type, EventPriority::Monitor, u64::MAX),
            )
            .map(|(_, &handle)| handle)
            .collect();

        let mut report = EventDispatchReport::default();

        for handle in snapshot {
            let Ok(owner) = self.listeners.owner_of(handle) else {
                continue;
            };
            let Ok(entry) = self.listeners.get_mut(handle, None) else {
                continue;
            };
            let priority = entry.priority;
            let ignore_cancelled = entry.ignore_cancelled;
            if ignore_cancelled && event.is_cancelled() {
                report.skipped_cancelled += 1;
                continue;
            }
            let Some(mut callback) = entry.callback.take() else {
                // Reentrant dispatch targeting a listener already on the call stack.
                continue;
            };

            report.invoked += 1;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                callback(event as &mut dyn Any);
            }));

            // Restore the callback if the listener is still registered; otherwise drop it safely.
            if let Ok(slot) = self.listeners.get_mut(handle, None) {
                slot.callback = Some(callback);
            } else {
                drop_listener_contained(Some(callback));
            }

            if let Err(payload) = outcome {
                let message = extract_panic_message_and_drop(payload);
                report.panics.push(ListenerPanic {
                    owner,
                    handle,
                    event_name: E::NAME,
                    priority,
                    message,
                });
            }
        }

        report
    }

    /// Unregisters and drops every listener across all plugins during shutdown.
    pub fn clear(&mut self) -> usize {
        let all_handles: Vec<EventListenerHandle> = self.by_event.values().copied().collect();
        self.by_event.clear();
        let mut count = 0usize;
        for handle in all_handles {
            if let Ok(entry) = self.listeners.remove(handle, None) {
                drop_listener_contained(entry.callback);
                count += 1;
            }
        }
        count
    }
}

fn drop_listener_contained(callback: Option<ErasedListenerFn>) {
    if let Some(cb) = callback
        && let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(cb)))
    {
        let _ = extract_panic_message_and_drop(payload);
    }
}
