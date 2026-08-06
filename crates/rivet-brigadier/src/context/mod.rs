//! Port of the `com.mojang.brigadier.context` package (upstream brigadier-1.3.10).
//!
//! Java context objects form a tree: `CommandContextBuilder` accumulates parsed
//! arguments/nodes during `CommandDispatcher.parse`, then `build(input)` produces the
//! immutable `CommandContext` tree that `execute` walks via `ContextChain`.
//!
//! Storage model per the crate conventions: nodes are shared `Arc<dyn CommandNode>`
//! references (Java shares references freely); parsed arguments erase their `T` as
//! `Arc<dyn Any>` and recover it by downcast (Java erases `T` in the
//! `Map<String, ParsedArgument<S, ?>>` and recovers by unchecked cast).

pub mod command_context;
pub mod command_context_builder;
pub mod context_chain;
pub mod parsed_argument;
pub mod parsed_command_node;
pub mod string_range;
pub mod suggestion_context;

pub use command_context::CommandContext;
pub use command_context_builder::CommandContextBuilder;
pub use context_chain::ContextChain;
pub use context_chain::Stage;
pub use parsed_argument::ParsedArgument;
pub use parsed_command_node::ParsedCommandNode;
pub use string_range::StringRange;
pub use suggestion_context::SuggestionContext;

#[cfg(test)]
mod command_context_tests;
#[cfg(test)]
mod context_chain_tests;
#[cfg(test)]
mod parsed_argument_tests;
