//! Minimal safe native Rust plugin boundary over Rivet-owned IDs (#506).
//!
//! This module defines plugin registration, load/enable/disable/unload
//! lifecycle transitions, API/capability version negotiation, generational
//! handles, panic containment, per-plugin ownership, and automatic cleanup,
//! together with a synthetic host backend ([`NativePluginHost`]) and minimal
//! lifecycle fixture ([`MinimalLifecyclePlugin`]).
//!
//! It is intentionally independent of `rivet-ffi` and the JVM adapter.

pub mod handle;
pub mod lifecycle;
pub mod version;

pub use handle::{Handle, HandleError, HandleTable, PluginId};
pub use lifecycle::{
    LifecycleError, LifecyclePhase, MinimalLifecyclePlugin, NativePlugin, NativePluginHost,
    PluginContext, PluginDescriptor, PluginHandle, PluginMarker, PluginState, ResourceHandle,
    ResourceMarker,
};
pub use version::{ApiVersion, Capability, CapabilitySet, LoadingMode, NegotiationError};
