//! Port of `com.mojang.brigadier.AmbiguityConsumer` (upstream).
//!
//! // STUB(brigadier): full port is the root `com.mojang.brigadier` unit; this is a
//! placeholder so the `tree` module's `findAmbiguities` can reference it.

/// Java `AmbiguityConsumer<S>`.
pub trait AmbiguityConsumer<S>: Send + Sync {
    /// Java `ambiguous(...)`.
    fn ambiguous(&self);
}
