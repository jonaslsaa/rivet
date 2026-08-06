//! Port of `com.mojang.brigadier.context.SuggestionContext` (upstream master; the
//! master `context` field is an enhancement over 1.3.10, required by the upstream
//! contextual-argument suggestions test).
//!
//! The `context` builder is the builder current at the cursor — it carries the
//! arguments parsed so far, so a custom `SuggestionProvider` can read them (upstream
//! `getCompletionSuggestions_redirect_contextualArgument`). 1.3.10 dropped it; the
//! master `CommandDispatcher` builds the suggestions context from it.

use std::sync::Arc;

use crate::context::command_context_builder::CommandContextBuilder;
use crate::tree::CommandNode;

/// Java `SuggestionContext<S>` — the context builder at the cursor, the parent
/// node, and the start position for building suggestions.
pub struct SuggestionContext<S> {
    /// Java public final field `context`.
    pub context: CommandContextBuilder<S>,
    /// Java public final field `parent`.
    pub parent: Arc<dyn CommandNode<S>>,
    /// Java public final field `startPos`.
    pub start_pos: i32,
}

impl<S: Clone + 'static> SuggestionContext<S> {
    /// Java `SuggestionContext(CommandContextBuilder, CommandNode<S>, int)`.
    pub fn new(
        context: CommandContextBuilder<S>,
        parent: Arc<dyn CommandNode<S>>,
        start_pos: i32,
    ) -> Self {
        SuggestionContext {
            context,
            parent,
            start_pos,
        }
    }
}
