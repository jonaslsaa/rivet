//! Port of `com.mojang.brigadier.AmbiguityConsumer` (upstream brigadier-1.3.10).

use crate::tree::CommandNode;

/// Java `AmbiguityConsumer<S>` — notified of pairs of children (of a parent) whose
/// example inputs overlap.
pub trait AmbiguityConsumer<S>: Send + Sync {
    /// Java `ambiguous(CommandNode parent, CommandNode child, CommandNode sibling,
    /// Collection<String> inputs)`.
    fn ambiguous(
        &self,
        parent: &dyn CommandNode<S>,
        child: &dyn CommandNode<S>,
        sibling: &dyn CommandNode<S>,
        inputs: &[String],
    );
}
