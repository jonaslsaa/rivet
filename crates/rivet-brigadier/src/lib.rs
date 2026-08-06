//! Port of `com.mojang.brigadier` (Mojang Brigadier, MIT licensed).
//!
//! Source of truth: the Java files under
//! `working/Paper/paper-server/src/minecraft/java/com/mojang/brigadier/` (the seven
//! Paper-patched files) plus the upstream `brigadier-1.3.10` sources jar for the
//! remaining classes of the package. This is a direct port (MIT), not Paper-derived;
//! Paper patches that depend on Minecraft types are stubbed and noted `// STUB(brigadier)`.
//! (The two dependency-blocked Paper-only features are `ENABLE_COMMAND_STACK_TRACES`
//! and `componentMessage()` in the exceptions package; the `minecraft:` prefix
//! prioritization and the `TagParseCommandSyntaxException` short-circuit in the
//! dispatcher/tree packages are likewise not ported — all four need Minecraft types.)
//!
//! Naming follows PORTING.md: one Rust module per Java class, snake_case, names
//! translated only by case convention. The Java `S` source type parameter is kept
//! generic (`CommandDispatcher<S>`), with `S: Clone` where sources must be copied
//! (Java shares references; a value clone is behaviorally equivalent for the
//! non-mutating source objects used here).

pub mod ambiguity_consumer;
pub mod arguments;
pub mod builder;
pub mod command;
pub mod command_dispatcher;
pub mod context;
pub mod exceptions;
pub mod immutable_string_reader;
pub mod java_hash;
pub mod literal_message;
pub mod message;
pub mod parse_results;
pub mod redirect_modifier;
pub mod result_consumer;
pub mod single_redirect_modifier;
pub mod string_reader;
pub mod suggestion;
pub mod tree;

/// `com.mojang.brigadier.AmbiguityConsumer<S>`.
pub use ambiguity_consumer::AmbiguityConsumer;
/// `com.mojang.brigadier.arguments.BoolArgumentType` etc.
pub use arguments::ArgumentType;
pub use arguments::BoolArgumentType;
pub use arguments::DoubleArgumentType;
pub use arguments::FloatArgumentType;
pub use arguments::IntegerArgumentType;
pub use arguments::LongArgumentType;
pub use arguments::StringArgumentType;
/// `com.mojang.brigadier.Command` — functional interface returning an `int` result.
pub use command::{Command, CommandFn};
/// `com.mojang.brigadier.CommandDispatcher<S>`.
pub use command_dispatcher::CommandDispatcher;
/// `com.mojang.brigadier.context.CommandContext<S>`.
pub use context::CommandContext;
/// `com.mojang.brigadier.context.CommandContextBuilder<S>`.
pub use context::CommandContextBuilder;
/// `com.mojang.brigadier.context.ContextChain<S>`.
pub use context::ContextChain;
/// `com.mojang.brigadier.context.ParsedArgument`.
pub use context::ParsedArgument;
/// `com.mojang.brigadier.context.ParsedCommandNode<S>`.
pub use context::ParsedCommandNode;
/// `com.mojang.brigadier.context.StringRange`.
pub use context::StringRange;
/// `com.mojang.brigadier.context.SuggestionContext<S>`.
pub use context::SuggestionContext;
/// `com.mojang.brigadier.ImmutableStringReader`.
pub use immutable_string_reader::ImmutableStringReader;
/// `com.mojang.brigadier.LiteralMessage`.
pub use literal_message::LiteralMessage;
/// `com.mojang.brigadier.Message` — Java interface with a single `getString()`.
pub use message::Message;
/// `com.mojang.brigadier.ParseResults<S>`.
pub use parse_results::ParseResults;
/// `com.mojang.brigadier.RedirectModifier<S>`.
pub use redirect_modifier::RedirectModifier;
/// `com.mojang.brigadier.ResultConsumer<S>`.
pub use result_consumer::ResultConsumer;
/// `com.mojang.brigadier.SingleRedirectModifier<S>`.
pub use single_redirect_modifier::SingleRedirectModifier;
/// `com.mojang.brigadier.StringReader`.
pub use string_reader::StringReader;
/// `com.mojang.brigadier.suggestion.Suggestion`.
pub use suggestion::Suggestion;
/// `com.mojang.brigadier.suggestion.SuggestionProvider<S>`.
pub use suggestion::SuggestionProvider;
/// `com.mojang.brigadier.suggestion.Suggestions`.
pub use suggestion::Suggestions;
/// `com.mojang.brigadier.suggestion.SuggestionsBuilder`.
pub use suggestion::SuggestionsBuilder;

#[cfg(test)]
mod command_dispatcher_tests;
#[cfg(test)]
mod command_dispatcher_usages_tests;
#[cfg(test)]
mod command_suggestions_tests;
#[cfg(test)]
mod tests;
