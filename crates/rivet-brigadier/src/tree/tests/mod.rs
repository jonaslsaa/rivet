//! Unit tests ported from the upstream brigadier `tree/AbstractCommandNodeTest` (MIT),
//! shared by the literal, root and argument node test modules.

pub mod argument_command_node_tests;
pub mod literal_command_node_tests;
pub mod root_command_node_tests;

use std::sync::Arc;

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::command::Command;
use crate::exceptions::CommandSyntaxException;
use crate::tree::{CommandNode, LiteralCommandNode};

/// A concrete `Command` (Java `@Mock Command` replaced by identity closures).
pub struct UnitCommand;

impl Command<i32> for UnitCommand {
    fn run(
        &self,
        _context: &crate::context::CommandContext<i32>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(1)
    }
}

pub fn command_arc() -> Arc<dyn Command<i32>> {
    Arc::new(UnitCommand)
}

pub fn literal(name: &str) -> LiteralArgumentBuilder<i32> {
    LiteralArgumentBuilder::literal(name)
}

/// Java `AbstractCommandNodeTest.testAddChild` — duplicate names collapse.
pub fn test_add_child(node: &Arc<dyn CommandNode<i32>>) {
    let c1 = literal("child1").build();
    let c2 = literal("child2").build();
    let c3 = literal("child1").build();
    node.add_child(Arc::from(c1));
    node.add_child(Arc::from(c2));
    node.add_child(Arc::from(c3));
    assert_eq!(node.get_children().len(), 2);
}

/// Java `AbstractCommandNodeTest.testAddChildMergesGrandchildren` — merging keeps
/// both grandchildren.
pub fn test_add_child_merges_grandchildren(node: &Arc<dyn CommandNode<i32>>) {
    let mut child1 = literal("child");
    child1.then(literal("grandchild1"));
    let child1 = child1.build();

    let mut child2 = literal("child");
    child2.then(literal("grandchild2"));
    let child2 = child2.build();

    node.add_child(Arc::from(child1));
    node.add_child(Arc::from(child2));

    let children = node.get_children();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_children().len(), 2);
}

/// Java `AbstractCommandNodeTest.testAddChildPreservesCommand` — first command wins.
pub fn test_add_child_preserves_command(node: &Arc<dyn CommandNode<i32>>) {
    let command = command_arc();

    let mut child1 = literal("child");
    child1.executes(Some(Arc::clone(&command)));
    let child1 = child1.build();

    let child2 = literal("child").build();

    node.add_child(Arc::from(child1));
    node.add_child(Arc::from(child2));

    let children = node.get_children();
    assert_eq!(children.len(), 1);
    assert!(Arc::ptr_eq(&children[0].get_command().unwrap(), &command));
}

/// Java `AbstractCommandNodeTest.testAddChildOverwritesCommand` — later command wins.
pub fn test_add_child_overwrites_command(node: &Arc<dyn CommandNode<i32>>) {
    let command = command_arc();

    let child1 = literal("child").build();

    let mut child2 = literal("child");
    child2.executes(Some(Arc::clone(&command)));
    let child2 = child2.build();

    node.add_child(Arc::from(child1));
    node.add_child(Arc::from(child2));

    let children = node.get_children();
    assert_eq!(children.len(), 1);
    assert!(Arc::ptr_eq(&children[0].get_command().unwrap(), &command));
}

/// `LiteralCommandNode` for use in `equals` tests.
pub fn literal_node(name: &str) -> Arc<LiteralCommandNode<i32>> {
    Arc::new(LiteralCommandNode::new(
        name.to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    ))
}
