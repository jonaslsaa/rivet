//! Port of `com.mojang.brigadier.RedirectModifier` (upstream brigadier-1.3.10).

use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;

/// Java `RedirectModifier<S>` — maps a context to the collection of sources to
/// redirect to.
pub trait RedirectModifier<S>: Send + Sync {
    /// Java `apply(CommandContext<S>) throws CommandSyntaxException`.
    fn apply(&self, context: &CommandContext<S>)
    -> Result<Vec<S>, CommandSyntaxException<'static>>;
}
