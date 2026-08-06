//! Port of `com.mojang.brigadier.builder.LiteralArgumentBuilder` (upstream).

use std::sync::Arc;

use crate::builder::argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior, Predicate};
use crate::command::Command;
use crate::redirect_modifier::RedirectModifier;
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

    /// Java `build()` — same node as `build()`, but already wrapped in an `Arc` so
    /// `CommandDispatcher.register` can return the `Arc<LiteralCommandNode>` handle
    /// Java returns.
    pub fn build_arc(&self) -> Arc<LiteralCommandNode<S>> {
        let result = LiteralCommandNode::new(
            self.literal.clone(),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
        );
        for argument in &self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Arc::new(result)
    }
}

impl<S: 'static> ArgumentBuilderBehavior<S> for LiteralArgumentBuilder<S> {
    fn base(&self) -> &ArgumentBuilder<S> {
        &self.argument_builder
    }

    fn base_mut(&mut self) -> &mut ArgumentBuilder<S> {
        &mut self.argument_builder
    }

    fn build(&self) -> Box<dyn CommandNode<S>> {
        let result = LiteralCommandNode::new(
            self.literal.clone(),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
        );
        for argument in &self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Box::new(result)
    }
}

/// `createBuilder()` result — Java's `LiteralArgumentBuilder` implements the
/// `NodeBuilder` surface so the node's `createBuilder()` can return it boxed.
impl<S: 'static> crate::tree::NodeBuilder<S> for LiteralArgumentBuilder<S> {
    fn get_requirement(&self) -> Predicate<S> {
        self.argument_builder.get_requirement()
    }

    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.argument_builder.get_command()
    }

    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.argument_builder.get_redirect()
    }

    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.argument_builder.get_redirect_modifier()
    }

    fn is_fork(&self) -> bool {
        self.argument_builder.is_fork()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
