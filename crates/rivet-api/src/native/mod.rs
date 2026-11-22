//! Minimal safe native Rust plugin boundary over Rivet-owned IDs (#506).
//!
//! This module defines plugin registration, load/enable/disable/unload
//! lifecycle transitions, API/capability version negotiation, generational
//! handles, panic containment, per-plugin ownership, and automatic cleanup,
//! together with a synthetic host backend ([`NativePluginHost`]) and minimal
//! lifecycle fixture ([`MinimalLifecyclePlugin`]).
//!
//! It is intentionally independent of `rivet-ffi` and the JVM adapter.

pub mod commands;
pub mod events;
pub mod facades;
pub mod handle;
pub mod lifecycle;
pub mod scheduler;
pub mod version;

pub use commands::{
    CommandDispatchOutcome, CommandError, CommandHandle, CommandMarker, CommandRegistry,
    CommandSender, CommandSpec,
};
pub use events::{
    Cancellable, EventBus, EventDispatchReport, EventError, EventListenerHandle,
    EventListenerMarker, EventPriority, ListenerPanic, NativeEvent,
};
pub use facades::{
    BatchMode, BatchReceipt, BlockPos, BlockStateSnapshot, ChunkPos, CommandOutcome, EntityCommand,
    EntityHandle, EntityMarker, EntitySnapshot, FacadeError, GameMode, InventoryCommand,
    InventoryHandle, InventoryKind, InventoryMarker, InventorySnapshot, ItemStackSnapshot,
    MenuCommand, MenuHandle, MenuMarker, MenuSnapshot, NativeFacadeView, PlayerCommand,
    PlayerHandle, PlayerMarker, PlayerSnapshot, SyntheticWorldState, TickCommand, TickCommandBatch,
    WorldCommand, WorldId, WorldSnapshot,
};
pub use handle::{Handle, HandleError, HandleTable, PluginId};
pub use lifecycle::{
    LifecycleError, LifecyclePhase, MinimalLifecyclePlugin, NativePlugin, NativePluginHost,
    PluginContext, PluginDescriptor, PluginHandle, PluginMarker, PluginState, ResourceHandle,
    ResourceMarker,
};
pub use scheduler::{
    ScheduleError, SyncScheduler, TaskContext, TaskHandle, TaskMarker, TaskPanic, TickReport,
};
pub use version::{ApiVersion, Capability, CapabilitySet, LoadingMode, NegotiationError};
