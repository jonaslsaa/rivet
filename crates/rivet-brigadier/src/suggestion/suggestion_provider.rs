//! Port of `com.mojang.brigadier.suggestion.SuggestionProvider` (upstream
//! brigadier-1.3.10).

use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::suggestion::Suggestions;
use crate::suggestion::suggestions_builder::SuggestionsBuilder;

/// Java `SuggestionProvider<S>` — produces suggestions for a command context.
///
/// Java returns `CompletableFuture<Suggestions>`; all providers in this crate build
/// synchronously, so the future is modelled as a plain `Suggestions` value.
pub trait SuggestionProvider<S>: Send + Sync {
    /// Java `getSuggestions(CommandContext<S>, SuggestionsBuilder) throws
    /// CommandSyntaxException`.
    fn get_suggestions(
        &self,
        context: &CommandContext<S>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>>;
}
