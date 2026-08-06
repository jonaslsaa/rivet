//! Port of `com.mojang.brigadier.context.CommandContextBuilder` (upstream
//! brigadier-1.3.10).

use std::collections::HashMap;
use std::sync::Arc;

use crate::command::Command;
use crate::context::command_context::CommandContext;
use crate::context::parsed_argument::ParsedArgument;
use crate::context::parsed_command_node::ParsedCommandNode;
use crate::context::string_range::StringRange;
use crate::context::suggestion_context::SuggestionContext;
use crate::redirect_modifier::RedirectModifier;
use crate::tree::CommandNode;

/// Java `CommandContextBuilder<S>` — the mutable accumulation state of a parse,
/// built into a `CommandContext` by `build(input)`.
///
/// Java's constructor takes the `CommandDispatcher` (stored, exposed via
/// `getDispatcher()`, never read). It is dropped here — the crate has no caller
/// that observes it.
pub struct CommandContextBuilder<S> {
    arguments: HashMap<String, ParsedArgument>,
    root_node: Arc<dyn CommandNode<S>>,
    nodes: Vec<ParsedCommandNode<S>>,
    source: S,
    command: Option<Arc<dyn Command<S>>>,
    child: Option<Box<CommandContextBuilder<S>>>,
    range: StringRange,
    modifier: Option<Arc<dyn RedirectModifier<S>>>,
    forks: bool,
}

impl<S: 'static> CommandContextBuilder<S> {
    /// Java `CommandContextBuilder(CommandDispatcher, S, CommandNode, int start)`.
    pub fn new(source: S, root_node: Arc<dyn CommandNode<S>>, start: i32) -> Self {
        CommandContextBuilder {
            arguments: HashMap::new(),
            root_node,
            nodes: Vec::new(),
            source,
            command: None,
            child: None,
            range: StringRange::at(start),
            modifier: None,
            forks: false,
        }
    }

    /// Java `withSource(S)`.
    pub fn with_source(&mut self, source: S) -> &mut Self {
        self.source = source;
        self
    }

    /// Java `getSource()`.
    pub fn get_source(&self) -> &S {
        &self.source
    }

    /// Java `getRootNode()`.
    pub fn get_root_node(&self) -> &Arc<dyn CommandNode<S>> {
        &self.root_node
    }

    /// Java `withArgument(String, ParsedArgument)`.
    pub fn with_argument(&mut self, name: &str, argument: ParsedArgument) -> &mut Self {
        self.arguments.insert(name.to_string(), argument);
        self
    }

    /// Java `getArguments()`.
    pub fn get_arguments(&self) -> &HashMap<String, ParsedArgument> {
        &self.arguments
    }

    /// Java `withCommand(Command)`.
    pub fn with_command(&mut self, command: Option<Arc<dyn Command<S>>>) -> &mut Self {
        self.command = command;
        self
    }

    /// Java `withNode(CommandNode, StringRange)`.
    pub fn with_node(&mut self, node: Arc<dyn CommandNode<S>>, range: StringRange) -> &mut Self {
        self.nodes.push(ParsedCommandNode::new(node.clone(), range));
        self.range = StringRange::encompassing(&self.range, &range);
        self.modifier = node.get_redirect_modifier();
        self.forks = node.is_fork();
        self
    }

    /// Java `copy()` — copies command, arguments, nodes, child, range and forks.
    /// Java deliberately does NOT copy `modifier` (the copied builder's modifier is
    /// reset to null); replicating that exactly.
    pub fn copy(&self) -> Self
    where
        S: Clone,
    {
        let mut copy = CommandContextBuilder::new(
            self.source.clone(),
            Arc::clone(&self.root_node),
            self.range.get_start(),
        );
        copy.command = self.command.as_ref().map(Arc::clone);
        copy.arguments = self.arguments.clone();
        copy.nodes = self.nodes.clone();
        copy.child = self.child.as_ref().map(|c| Box::new(c.copy()));
        copy.range = self.range;
        copy.forks = self.forks;
        copy
    }

    /// Java `withChild(CommandContextBuilder)`.
    pub fn with_child(&mut self, child: CommandContextBuilder<S>) -> &mut Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Java `getChild()`.
    pub fn get_child(&self) -> Option<&CommandContextBuilder<S>> {
        self.child.as_deref()
    }

    /// Java `getLastChild()`.
    pub fn get_last_child(&self) -> &CommandContextBuilder<S> {
        let mut result = self;
        while let Some(child) = result.get_child() {
            result = child;
        }
        result
    }

    /// Java `getCommand()`.
    pub fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.command.as_ref().map(Arc::clone)
    }

    /// Java `getNodes()`.
    pub fn get_nodes(&self) -> &[ParsedCommandNode<S>] {
        &self.nodes
    }

    /// Java `build(String)` — builds the `CommandContext` tree, recursing into the
    /// child.
    pub fn build(&self, input: String) -> CommandContext<S>
    where
        S: Clone,
    {
        CommandContext::new(
            self.source.clone(),
            input.clone(),
            self.arguments.clone(),
            self.command.as_ref().map(Arc::clone),
            Arc::clone(&self.root_node),
            self.nodes.clone(),
            self.range,
            self.child.as_ref().map(|c| Arc::new(c.build(input))),
            self.modifier.as_ref().map(Arc::clone),
            self.forks,
        )
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }

    /// Java `findSuggestionContext(int)` — carries the builder current at the cursor
    /// (master `SuggestionContext`), so `getCompletionSuggestions` builds the
    /// suggestions context with the arguments parsed so far.
    pub fn find_suggestion_context(&self, cursor: i32) -> SuggestionContext<S>
    where
        S: Clone,
    {
        if self.range.get_start() <= cursor {
            if self.range.get_end() < cursor {
                if let Some(child) = self.get_child() {
                    return child.find_suggestion_context(cursor);
                } else if !self.nodes.is_empty() {
                    let last = self.nodes.last().expect("nodes non-empty");
                    return SuggestionContext::new(
                        self.clone(),
                        last.get_node(),
                        last.get_range().get_end() + 1,
                    );
                } else {
                    return SuggestionContext::new(
                        self.clone(),
                        Arc::clone(&self.root_node),
                        self.range.get_start(),
                    );
                }
            } else {
                let mut prev = Arc::clone(&self.root_node);
                for node in &self.nodes {
                    let node_range = node.get_range();
                    if node_range.get_start() <= cursor && cursor <= node_range.get_end() {
                        return SuggestionContext::new(self.clone(), prev, node_range.get_start());
                    }
                    prev = node.get_node();
                }
                return SuggestionContext::new(self.clone(), prev, self.range.get_start());
            }
        }
        panic!("Can't find node before cursor");
    }
}

impl<S: Clone + 'static> Clone for CommandContextBuilder<S> {
    /// A faithful field clone (keeps `modifier`). Java's `copy()` — used by the
    /// dispatcher's per-child context snapshot — is the one that resets `modifier`
    /// to null; that is a separate method, `copy()`, not `Clone`.
    fn clone(&self) -> Self {
        CommandContextBuilder {
            arguments: self.arguments.clone(),
            root_node: Arc::clone(&self.root_node),
            nodes: self.nodes.clone(),
            source: self.source.clone(),
            command: self.command.as_ref().map(Arc::clone),
            child: self.child.as_ref().map(|c| Box::new((**c).clone())),
            range: self.range,
            modifier: self.modifier.as_ref().map(Arc::clone),
            forks: self.forks,
        }
    }
}

impl<S: 'static> std::fmt::Debug for CommandContextBuilder<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandContextBuilder")
            .field("range", &self.range)
            .field("nodes", &self.nodes)
            .finish_non_exhaustive()
    }
}
