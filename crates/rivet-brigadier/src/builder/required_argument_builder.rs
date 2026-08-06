//! Port of `com.mojang.brigadier.builder.RequiredArgumentBuilder` (upstream).

use std::sync::Arc;

use crate::arguments::ArgumentType;
use crate::builder::argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior, Predicate};
use crate::command::Command;
use crate::redirect_modifier::RedirectModifier;
use crate::suggestion::SuggestionProvider;
use crate::tree::{ArgumentCommandNode, CommandNode};

/// Java `RequiredArgumentBuilder<S, T>`.
pub struct RequiredArgumentBuilder<S, T> {
    argument_builder: ArgumentBuilder<S>,
    name: String,
    // `type` is a Rust keyword — Java field `type`.
    type_: Arc<dyn ArgumentType<T>>,
    suggestions_provider: Option<Arc<dyn SuggestionProvider<S>>>,
}

impl<S: 'static, T: 'static + Send + Sync> RequiredArgumentBuilder<S, T> {
    /// Java `argument(String name, ArgumentType<T> type)`.
    pub fn argument(name: impl Into<String>, type_: Arc<dyn ArgumentType<T>>) -> Self {
        RequiredArgumentBuilder {
            argument_builder: ArgumentBuilder::new(),
            name: name.into(),
            type_,
            suggestions_provider: None,
        }
    }

    /// Java `suggests(SuggestionProvider<S>)`. Java's `createBuilder` passes
    /// `getCustomSuggestions()` which may be null; `provider` is the nullable
    /// `Option` equivalent.
    pub fn suggests(&mut self, provider: Option<Arc<dyn SuggestionProvider<S>>>) -> &mut Self {
        self.suggestions_provider = provider;
        self
    }

    /// Java `getSuggestionsProvider()`.
    pub fn get_suggestions_provider(&self) -> Option<&Arc<dyn SuggestionProvider<S>>> {
        self.suggestions_provider.as_ref()
    }

    /// Java `getType()`.
    pub fn get_type(&self) -> &Arc<dyn ArgumentType<T>> {
        &self.type_
    }

    /// Java `build()` — same node as `build()`, but already wrapped in an `Arc`.
    pub fn build_arc(&self) -> Arc<ArgumentCommandNode<S, T>> {
        let result = ArgumentCommandNode::new(
            self.name.clone(),
            Arc::clone(&self.type_),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
            self.get_suggestions_provider().cloned(),
        );
        for argument in &self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Arc::new(result)
    }

    /// Java `getName()`.
    pub fn get_name(&self) -> &str {
        &self.name
    }
}

impl<S: 'static, T: 'static + Send + Sync> ArgumentBuilderBehavior<S>
    for RequiredArgumentBuilder<S, T>
{
    fn base(&self) -> &ArgumentBuilder<S> {
        &self.argument_builder
    }

    fn base_mut(&mut self) -> &mut ArgumentBuilder<S> {
        &mut self.argument_builder
    }

    /// Java `build()`.
    fn build(&self) -> Box<dyn CommandNode<S>> {
        let result = ArgumentCommandNode::new(
            self.name.clone(),
            Arc::clone(&self.type_),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
            self.get_suggestions_provider().cloned(),
        );
        for argument in &self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Box::new(result)
    }
}

/// `createBuilder()` result — Java's `RequiredArgumentBuilder` implements the
/// `NodeBuilder` surface so the node's `createBuilder()` can return it boxed.
impl<S: 'static, T: 'static> crate::tree::NodeBuilder<S> for RequiredArgumentBuilder<S, T> {
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
