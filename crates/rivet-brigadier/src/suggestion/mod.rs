//! Port of the `com.mojang.brigadier.suggestion` package (upstream brigadier-1.3.10).
//!
//! Java's `getCompletionSuggestions` returns a `CompletableFuture<Suggestions>`.
//! All suggestion sources here are synchronous (the built-in argument types and the
//! node suggestions build immediately), so the future is modelled as a plain
//! `Suggestions` value — no async machinery is needed. `SuggestionProvider`
//! accordingly returns `Suggestions` directly.
//!
//! `IntegerSuggestion` (Java) is folded into `Suggestion` as an optional integer
//! kind — see `suggestion.rs`.

// `suggestion::suggestion` mirrors the Java package layout
// (`com.mojang.brigadier.suggestion.Suggestion`), so the module-inception name is
// intentional.
#[allow(clippy::module_inception)]
pub mod suggestion;
pub mod suggestion_provider;
pub mod suggestions;
pub mod suggestions_builder;

#[cfg(test)]
mod suggestion_tests;
#[cfg(test)]
mod suggestions_builder_tests;
#[cfg(test)]
mod suggestions_tests;

pub use suggestion::Suggestion;
pub use suggestion_provider::SuggestionProvider;
pub use suggestions::Suggestions;
pub use suggestions_builder::SuggestionsBuilder;
