//! Port of `com.mojang.brigadier.builder.LiteralArgumentBuilder` (upstream).

use std::sync::Arc;

use crate::builder::argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior};
use crate::tree::{CommandNode, LiteralCommandNode};

/// Java `LiteralArgumentBuilder<S>`.
pub struct LiteralArgumentBuilder<S> {
    argument_builder: ArgumentBuilder<S>,
    literal: String,
}

impl<S: 'static> LiteralArgumentBuilder<S> {
    /// Java `literal(String name)`.
    pub fn literal(name: impl Into<String>) -> Self {
        LiteralArgumentBuilder {
            argument_builder: ArgumentBuilder::new(),
            literal: name.into(),
        }
    }

    /// Java `getLiteral()`.
    pub fn get_literal(&self) -> &str {
        &self.literal
    }
}

impl<S: 'static> ArgumentBuilderBehavior<S> for LiteralArgumentBuilder<S> {
    fn base(&self) -> &ArgumentBuilder<S> {
        &self.argument_builder
    }

    fn base_mut(&mut self) -> &mut ArgumentBuilder<S> {
        &mut self.argument_builder
    }

    /// Java `build()` — constructs a `LiteralCommandNode` sharing this builder's
    /// command, requirement, redirect, modifier and forks, then re-adds each child
    /// (the same node references the builder's `RootCommandNode` holds).
    fn build(&self) -> Box<dyn CommandNode<S>> {
        let mut result = LiteralCommandNode::new(
            self.literal.clone(),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
        );
        for argument in self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Box::new(result)
    }
}
