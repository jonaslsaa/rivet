//! Port of the `com.mojang.brigadier.builder` package.
//!
//! `ArgumentBuilder` is Paper-patched (adds `defaultRequirement()`, Paper's "Vanilla
//! command permission fixes" patch); `LiteralArgumentBuilder` and
//! `RequiredArgumentBuilder` are upstream. The cross-package types the builders
//! reference (`tree`, `command`, `suggestion`, `arguments`, `context`) are not yet
//! ported in this crate — minimal `// STUB(brigadier.builder)` declarations for the
//! surfaces the builders touch live alongside.

pub mod argument_builder;
pub mod literal_argument_builder;
pub mod required_argument_builder;

pub use argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior, Predicate};
pub use literal_argument_builder::LiteralArgumentBuilder;
pub use required_argument_builder::RequiredArgumentBuilder;

#[cfg(test)]
mod tests;
