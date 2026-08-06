//! Port of `com.mojang.brigadier.context.CommandContext` (upstream brigadier-1.3.10).

use std::collections::HashMap;
use std::sync::Arc;

use crate::command::Command;
use crate::context::parsed_argument::ParsedArgument;
use crate::context::parsed_command_node::ParsedCommandNode;
use crate::context::string_range::StringRange;
use crate::redirect_modifier::RedirectModifier;
use crate::tree::{CommandNode, command_eq, node_eq, node_hash};

/// Java `CommandContext<S>` — the immutable result of parsing a command, built from
/// a `CommandContextBuilder`.
pub struct CommandContext<S> {
    source: S,
    input: String,
    command: Option<Arc<dyn Command<S>>>,
    arguments: HashMap<String, ParsedArgument>,
    root_node: Arc<dyn CommandNode<S>>,
    nodes: Vec<ParsedCommandNode<S>>,
    range: StringRange,
    child: Option<Arc<CommandContext<S>>>,
    modifier: Option<Arc<dyn RedirectModifier<S>>>,
    forks: bool,
}

impl<S: 'static> CommandContext<S> {
    /// Java `CommandContext(S, String, Map, Command, CommandNode, List, StringRange,
    /// CommandContext, RedirectModifier, boolean)`.
    #[allow(clippy::too_many_arguments)] // mirrors Java's 10-parameter constructor
    pub fn new(
        source: S,
        input: String,
        arguments: HashMap<String, ParsedArgument>,
        command: Option<Arc<dyn Command<S>>>,
        root_node: Arc<dyn CommandNode<S>>,
        nodes: Vec<ParsedCommandNode<S>>,
        range: StringRange,
        child: Option<Arc<CommandContext<S>>>,
        modifier: Option<Arc<dyn RedirectModifier<S>>>,
        forks: bool,
    ) -> Self {
        CommandContext {
            source,
            input,
            arguments,
            command,
            root_node,
            nodes,
            range,
            child,
            modifier,
            forks,
        }
    }

    /// Java `copyFor(S source)` — a new context with the same state but a new
    /// source. Java shares the (immutable) arguments/nodes/child references; Rust
    /// clones them (`Arc`/value clones, behaviorally identical for read-only state).
    pub fn copy_for(&self, source: S) -> Self {
        CommandContext {
            source,
            input: self.input.clone(),
            arguments: self.arguments.clone(),
            command: self.command.as_ref().map(Arc::clone),
            root_node: Arc::clone(&self.root_node),
            nodes: self.nodes.clone(),
            range: self.range,
            child: self.child.as_ref().map(Arc::clone),
            modifier: self.modifier.as_ref().map(Arc::clone),
            forks: self.forks,
        }
    }

    /// Java `getChild()`.
    pub fn get_child(&self) -> Option<&CommandContext<S>> {
        self.child.as_deref()
    }

    /// Java `getLastChild()`.
    pub fn get_last_child(&self) -> &CommandContext<S> {
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

    /// Java `getSource()`.
    pub fn get_source(&self) -> &S {
        &self.source
    }

    /// Java `getArgument(String, Class<V>)`.
    ///
    /// Java looks up the parsed argument by name, checks the erased result is
    /// assignable to the requested class (with primitive→wrapper mapping), and
    /// returns it; a missing or type-mismatched argument throws
    /// `IllegalArgumentException`. Rust recovers the erased result by downcast;
    /// the exceptions become panics with Java's message shape.
    pub fn get_argument<T: std::any::Any + Clone>(&self, name: &str) -> T {
        let argument = self
            .arguments
            .get(name)
            .unwrap_or_else(|| panic!("No such argument '{}' exists on this command", name));
        let result = argument.result_as_any();
        if let Some(value) = result.downcast_ref::<T>() {
            return value.clone();
        }
        panic!(
            "Argument '{}' is defined as {}, not {}",
            name,
            argument.type_name(),
            std::any::type_name::<T>()
        )
    }

    /// Java `getRedirectModifier()`.
    pub fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }

    /// Java `getInput()`.
    pub fn get_input(&self) -> &str {
        &self.input
    }

    /// Java `getRootNode()`.
    pub fn get_root_node(&self) -> &Arc<dyn CommandNode<S>> {
        &self.root_node
    }

    /// Java `getNodes()`.
    pub fn get_nodes(&self) -> &[ParsedCommandNode<S>] {
        &self.nodes
    }

    /// Java `hasNodes()`.
    pub fn has_nodes(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// Java `isForked()`.
    pub fn is_forked(&self) -> bool {
        self.forks
    }

    /// Java `hashCode()` — the exact chain from the Java source:
    /// `source`, then `31 * h + arguments.hashCode()`,
    /// `31 * h + (command != null ? command.hashCode() : 0)`,
    /// `31 * h + rootNode.hashCode()`, `31 * h + nodes.hashCode()`,
    /// `31 * h + (child != null ? child.hashCode() : 0)`.
    pub fn hash_code(&self) -> i32
    where
        S: std::hash::Hash,
    {
        let mut result = java_source_hash(&self.source);
        result = 31_i32
            .wrapping_mul(result)
            .wrapping_add(self.arguments_hash_code());
        result = 31_i32.wrapping_mul(result).wrapping_add(
            self.command
                .as_ref()
                .map_or(0, |c| command_identity_hash(c)),
        );
        result = 31_i32
            .wrapping_mul(result)
            .wrapping_add(node_hash(&*self.root_node));
        result = 31_i32
            .wrapping_mul(result)
            .wrapping_add(self.nodes_hash_code());
        result = 31_i32
            .wrapping_mul(result)
            .wrapping_add(self.child.as_ref().map_or(0, |c| c.hash_code()));
        result
    }

    fn arguments_hash_code(&self) -> i32 {
        // Java `Map.hashCode` = sum over entries of `key.hashCode() ^ value.hashCode()`.
        self.arguments.iter().fold(0i32, |acc, (name, arg)| {
            acc.wrapping_add(crate::java_hash::string_hash(name) ^ arg.hash_code())
        })
    }

    fn nodes_hash_code(&self) -> i32 {
        // Java `List.hashCode` = `1`, then `31 * h + element.hashCode()` for each.
        self.nodes.iter().fold(1i32, |acc, node| {
            31_i32
                .wrapping_mul(acc)
                .wrapping_add(parsed_command_node_hash(node))
        })
    }
}

impl<S: PartialEq + 'static> PartialEq for CommandContext<S> {
    fn eq(&self, other: &Self) -> bool {
        self.arguments == other.arguments
            && node_eq(&*self.root_node, &*other.root_node)
            && self.nodes == other.nodes
            && command_eq(&self.command, &other.command)
            && self.source == other.source
            && self.child == other.child
    }
}

impl<S: Eq + 'static> Eq for CommandContext<S> {}

impl<S: 'static> std::fmt::Debug for CommandContext<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandContext")
            .field("input", &self.input)
            .field("range", &self.range)
            .field("nodes", &self.nodes)
            .finish_non_exhaustive()
    }
}

/// Java source `.hashCode()` for the source types used in this crate (a general
/// `S: Hash` can't reproduce Java's value hashes, but `hash_code` only needs the
/// equal-implies-equal-hash invariant).
fn java_source_hash<S: std::hash::Hash>(source: &S) -> i32 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish() as i32
}

/// `Command.hashCode()` in Java is identity for non-`equals`-overriding commands
/// (lambdas, mocks). Reproduce with the Arc address, so equal contexts (same Arc)
/// hash equal.
fn command_identity_hash<S>(command: &Arc<dyn Command<S>>) -> i32 {
    Arc::as_ptr(command) as *const () as usize as i32
}

/// `ParsedCommandNode.hashCode()` = `Objects.hash(node, range)` — but Java's node
/// `hashCode` is structural, so use `node_hash` for the node component.
fn parsed_command_node_hash<S>(node: &ParsedCommandNode<S>) -> i32 {
    let node_part = node_hash(&*node.get_node());
    crate::java_hash::objects_hash(&[node_part, node.get_range().hash_code()])
}
