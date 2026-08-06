//! Port of `com.mojang.brigadier.context.ParsedCommandNode` (upstream brigadier-1.3.10).

use std::sync::Arc;

use crate::context::string_range::StringRange;
use crate::tree::{CommandNode, node_eq};

/// Java `ParsedCommandNode<S>` — a parsed command node plus the string range it
/// consumed.
pub struct ParsedCommandNode<S> {
    node: Arc<dyn CommandNode<S>>,
    range: StringRange,
}

impl<S> ParsedCommandNode<S> {
    /// Java `ParsedCommandNode(CommandNode<S>, StringRange)`.
    pub fn new(node: Arc<dyn CommandNode<S>>, range: StringRange) -> Self {
        ParsedCommandNode { node, range }
    }

    /// Java `getNode()`.
    pub fn get_node(&self) -> Arc<dyn CommandNode<S>> {
        Arc::clone(&self.node)
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }
}

impl<S: 'static> std::fmt::Display for ParsedCommandNode<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.node.to_string(), self.range)
    }
}

impl<S: 'static> std::fmt::Debug for ParsedCommandNode<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedCommandNode")
            .field("node", &self.node.to_string())
            .field("range", &self.range)
            .finish()
    }
}

/// Java `equals`: `Objects.equals(node, that.node) && Objects.equals(range,
/// that.range)`. Node equality is Java `CommandNode.equals` (structural); `range`
/// is a value type.
impl<S: 'static> PartialEq for ParsedCommandNode<S> {
    fn eq(&self, other: &Self) -> bool {
        node_eq(&*self.node, &*other.node) && self.range == other.range
    }
}

impl<S: 'static> Eq for ParsedCommandNode<S> {}

impl<S: 'static> Clone for ParsedCommandNode<S> {
    fn clone(&self) -> Self {
        ParsedCommandNode {
            node: Arc::clone(&self.node),
            range: self.range,
        }
    }
}
