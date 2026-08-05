//! Port of `com.mojang.brigadier.ResultConsumer` (upstream).
//!
//! // STUB(brigadier): full port is the root `com.mojang.brigadier` unit; this is a
//! placeholder so the `command_dispatcher` module can reference it.

/// Java `ResultConsumer<S>`.
pub trait ResultConsumer<S>: Send + Sync {
    /// Java `onCommandComplete(...)`.
    fn on_command_complete(&self);
}
