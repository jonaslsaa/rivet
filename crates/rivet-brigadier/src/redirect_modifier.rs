//! Port of `com.mojang.brigadier.RedirectModifier` (upstream).
//!
//! // STUB(brigadier.builder): full port is the root `com.mojang.brigadier` unit; this
//! is the surface the builder cluster references.

use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;

/// Java `RedirectModifier<S>` — maps a context to the collection of sources to
/// redirect to.
pub trait RedirectModifier<S>: Send + Sync {
    /// Java `apply(CommandContext<S>) throws CommandSyntaxException`.
    fn apply(&self, context: &CommandContext<S>)
    -> Result<Vec<S>, CommandSyntaxException<'static>>;
}
