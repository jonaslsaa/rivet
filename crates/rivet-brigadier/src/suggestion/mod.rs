//! Port of `com.mojang.brigadier.suggestion.SuggestionProvider` (upstream).
//!
//! // STUB(brigadier.builder): full port is the `com.mojang.brigadier.suggestion`
//! unit; this is the surface the builder cluster references.

use crate::context::CommandContext;

/// Java `SuggestionProvider<S>` — produces suggestions for a command context.
pub trait SuggestionProvider<S>: Send + Sync {
    /// Java `getSuggestions(CommandContext<S>, SuggestionsBuilder) throws CommandSyntaxException`.
    fn get_suggestions(
        &self,
        context: &CommandContext<S>,
        builder: &mut dyn crate::suggestion::SuggestionsBuilder,
    ) -> Result<(), crate::exceptions::CommandSyntaxException<'static>>;
}

/// Java `SuggestionsBuilder` — STUB trait so the provider signature above can name
/// it. Full port is the `suggestion` unit.
pub trait SuggestionsBuilder: Send + Sync {}
