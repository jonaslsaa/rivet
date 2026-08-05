//! Port of `com.mojang.brigadier.builder.RequiredArgumentBuilder` (upstream).

use std::sync::Arc;

use crate::arguments::ArgumentType;
use crate::builder::argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior};
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

impl<S: 'static, T: 'static> RequiredArgumentBuilder<S, T> {
    /// Java `argument(String name, ArgumentType<T> type)`.
    pub fn argument(name: impl Into<String>, type_: Arc<dyn ArgumentType<T>>) -> Self {
        RequiredArgumentBuilder {
            argument_builder: ArgumentBuilder::new(),
            name: name.into(),
            type_,
            suggestions_provider: None,
        }
    }

    /// Java `suggests(SuggestionProvider<S>)`.
    pub fn suggests(&mut self, provider: Arc<dyn SuggestionProvider<S>>) -> &mut Self {
        self.suggestions_provider = Some(provider);
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

    /// Java `getName()`.
    pub fn get_name(&self) -> &str {
        &self.name
    }
}

impl<S: 'static, T: 'static> ArgumentBuilderBehavior<S> for RequiredArgumentBuilder<S, T> {
    fn base(&self) -> &ArgumentBuilder<S> {
        &self.argument_builder
    }

    fn base_mut(&mut self) -> &mut ArgumentBuilder<S> {
        &mut self.argument_builder
    }

    /// Java `build()`.
    fn build(&self) -> Box<dyn CommandNode<S>> {
        let mut result = ArgumentCommandNode::new(
            self.name.clone(),
            Arc::clone(&self.type_),
            self.argument_builder.get_command(),
            self.argument_builder.get_requirement(),
            self.argument_builder.get_redirect(),
            self.argument_builder.get_redirect_modifier(),
            self.argument_builder.is_fork(),
            self.get_suggestions_provider().cloned(),
        );
        for argument in self.argument_builder.get_arguments() {
            result.add_child(Arc::clone(argument));
        }
        Box::new(result)
    }
}
