//! Port of `com.mojang.brigadier.SingleRedirectModifier` (upstream).
//!
//! // STUB(brigadier.builder): full port is the root `com.mojang.brigadier` unit; this
//! is the surface the builder cluster references.

use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;

/// Java `SingleRedirectModifier<S>` — maps a context to the single source to
/// redirect to.
pub trait SingleRedirectModifier<S>: Send + Sync {
    /// Java `apply(CommandContext<S>) throws CommandSyntaxException`.
    fn apply(&self, context: &CommandContext<S>) -> Result<S, CommandSyntaxException<'static>>;
}
