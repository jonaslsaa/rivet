//! Port of the `com.mojang.brigadier.builder` package.
//!
//! `ArgumentBuilder` is Paper-patched (adds `defaultRequirement()`, Paper's "Vanilla
//! command permission fixes" patch); `LiteralArgumentBuilder` and
//! `RequiredArgumentBuilder` are upstream.

pub mod argument_builder;
pub mod literal_argument_builder;
pub mod required_argument_builder;

pub use argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior, Predicate};
pub use literal_argument_builder::LiteralArgumentBuilder;
pub use required_argument_builder::RequiredArgumentBuilder;

#[cfg(test)]
mod tests;
