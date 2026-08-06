//! Unit tests ported from the upstream brigadier `CommandContextTest` (MIT).
//!
//! Java's `@Mock CommandNode`/`Command` mocks have identity semantics; concrete
//! `LiteralCommandNode`/closure commands are used instead (identity via `Arc::ptr_eq`
//! on the stored command, structural via node `equals`).

use std::sync::Arc;

use crate::command::Command;
use crate::context::{CommandContextBuilder, ParsedArgument, StringRange};
use crate::exceptions::CommandSyntaxException;
use crate::tree::{CommandNode, LiteralCommandNode, RootCommandNode};

/// A `Command` recording the source it saw (Java mock verify).
struct SourceRecordingCommand<C> {
    seen: Arc<std::sync::Mutex<Option<C>>>,
    result: i32,
}

impl<C: Clone + Send + Sync + 'static> Command<C> for SourceRecordingCommand<C> {
    fn run(
        &self,
        context: &crate::context::CommandContext<C>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        let mut seen = self.seen.lock().unwrap();
        *seen = Some(context.get_source().clone());
        Ok(self.result)
    }
}

fn root_node() -> Arc<dyn CommandNode<i32>> {
    Arc::new(RootCommandNode::<i32>::new()) as Arc<dyn CommandNode<i32>>
}

fn builder(source: i32) -> CommandContextBuilder<i32> {
    CommandContextBuilder::new(source, root_node(), 0)
}

#[test]
fn test_get_argument_nonexistent() {
    let context = builder(0).build("".to_string());
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        context.get_argument::<String>("foo");
    }));
    assert!(err.is_err());
}

#[test]
fn test_get_argument_wrong_type() {
    let mut b = builder(0);
    b.with_argument("foo", ParsedArgument::new(0, 1, 123));
    let context = b.build("123".to_string());
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        context.get_argument::<String>("foo");
    }));
    assert!(err.is_err());
}

#[test]
fn test_get_argument() {
    let mut b = builder(0);
    b.with_argument("foo", ParsedArgument::new(0, 1, 123));
    let context = b.build("123".to_string());
    assert_eq!(context.get_argument::<i32>("foo"), 123);
}

#[test]
fn test_source() {
    assert_eq!(*builder(42).build("".to_string()).get_source(), 42);
}

#[test]
fn test_root_node() {
    let b = builder(0);
    let context = b.build("".to_string());
    assert!(Arc::ptr_eq(context.get_root_node(), b.get_root_node()));
}

#[test]
fn test_equals() {
    // Java's EqualsTester groups: equal contexts within a group, unequal across
    // groups. Sources are `i32` values; nodes/commands are distinct `Arc`s.
    let root = root_node();
    // A structurally-different root (has a child).
    let other_root: Arc<dyn CommandNode<i32>> = {
        let r = Arc::new(RootCommandNode::<i32>::new());
        let child = Arc::new(LiteralCommandNode::<i32>::new(
            "x".to_string(),
            None,
            Arc::new(|_| true),
            None,
            None,
            false,
        )) as Arc<dyn CommandNode<i32>>;
        r.add_child(child);
        r as Arc<dyn CommandNode<i32>>
    };

    let node_a = Arc::new(LiteralCommandNode::<i32>::new(
        "a".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;
    // A structurally-different node (different literal).
    let other_node = Arc::new(LiteralCommandNode::<i32>::new(
        "b".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    let mk = |source: i32, root: Arc<dyn CommandNode<i32>>, node: Arc<dyn CommandNode<i32>>| {
        let mut b = CommandContextBuilder::new(source, root, 0);
        b.with_node(node, StringRange::between(0, 3));
        b.build("123".to_string())
    };

    // (source, rootNode, node) equal.
    assert_eq!(
        mk(0, Arc::clone(&root), Arc::clone(&node_a)),
        mk(0, Arc::clone(&root), Arc::clone(&node_a))
    );
    // Different root -> unequal. Java's mock roots are identity-unequal; the Rust
    // structural equality needs roots that differ (one has a child).
    assert_ne!(
        mk(0, Arc::clone(&root), Arc::clone(&node_a)),
        mk(0, Arc::clone(&other_root), Arc::clone(&node_a))
    );
    // Different source -> unequal.
    assert_ne!(
        mk(0, Arc::clone(&root), Arc::clone(&node_a)),
        mk(1, Arc::clone(&root), Arc::clone(&node_a))
    );
    // Same command Arc -> equal.
    let command: Arc<dyn Command<i32>> = Arc::new(SourceRecordingCommand {
        seen: Arc::new(std::sync::Mutex::new(None)),
        result: 0,
    });
    let other_command: Arc<dyn Command<i32>> = Arc::new(SourceRecordingCommand {
        seen: Arc::new(std::sync::Mutex::new(None)),
        result: 0,
    });
    let with_cmd = |c: Arc<dyn Command<i32>>| {
        let mut b = builder(0);
        b.with_command(Some(c));
        b.build("".to_string())
    };
    assert_eq!(
        with_cmd(Arc::clone(&command)),
        with_cmd(Arc::clone(&command))
    );
    assert_ne!(
        with_cmd(Arc::clone(&command)),
        with_cmd(Arc::clone(&other_command))
    );
    // Nodes differ structurally -> unequal.
    assert_ne!(
        mk(0, Arc::clone(&root), Arc::clone(&node_a)),
        mk(0, Arc::clone(&root), Arc::clone(&other_node))
    );
}
