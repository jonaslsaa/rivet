//! Unit tests ported from the upstream brigadier `RootCommandNodeTest` (MIT),
//! plus the shared `AbstractCommandNodeTest` cases.

use std::sync::Arc;

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::context::{CommandContextBuilder, StringRange};
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestion, SuggestionsBuilder};
use crate::tree::tests as shared;
use crate::tree::{CommandNode, RootCommandNode};

fn node() -> Arc<RootCommandNode<i32>> {
    Arc::new(RootCommandNode::<i32>::new())
}

fn root_dyn() -> Arc<dyn CommandNode<i32>> {
    Arc::new(RootCommandNode::<i32>::new())
}

#[test]
fn test_add_child() {
    let n: Arc<dyn CommandNode<i32>> = node();
    shared::test_add_child(&n);
}

#[test]
fn test_add_child_merges_grandchildren() {
    let n: Arc<dyn CommandNode<i32>> = node();
    shared::test_add_child_merges_grandchildren(&n);
}

#[test]
fn test_add_child_preserves_command() {
    let n: Arc<dyn CommandNode<i32>> = node();
    shared::test_add_child_preserves_command(&n);
}

#[test]
fn test_add_child_overwrites_command() {
    let n: Arc<dyn CommandNode<i32>> = node();
    shared::test_add_child_overwrites_command(&n);
}

#[test]
fn test_parse() {
    let n: Arc<dyn CommandNode<i32>> = node();
    let mut reader = StringReader::new("hello world");
    n.parse(Arc::clone(&n), &mut reader, &mut context_builder())
        .unwrap();
    assert_eq!(reader.get_cursor(), 0);
}

#[test]
#[should_panic]
fn test_add_child_no_root() {
    let n = node();
    let other_root: Arc<dyn CommandNode<i32>> = Arc::new(RootCommandNode::<i32>::new());
    n.add_child(other_root);
}

#[test]
fn test_usage() {
    let n = node();
    assert_eq!(n.get_usage_text(), "");
}

#[test]
fn test_suggestions() {
    let n = node();
    let mut builder = SuggestionsBuilder::new_with_input("".to_string(), 0);
    let result = n
        .list_suggestions(&context_builder().build("".to_string()), &mut builder)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
#[should_panic(expected = "Cannot convert root into a builder")]
fn test_create_builder() {
    let n = node();
    let _ = n.create_builder();
}

#[test]
fn test_equals() {
    let a = node();
    let b = node();
    assert!(a.equals(&*b));

    let c = node();
    let foo = shared::literal("foo").build();
    c.add_child(Arc::from(foo));
    let d = node();
    let foo2 = shared::literal("foo").build();
    d.add_child(Arc::from(foo2));
    assert!(c.equals(&*d));

    // Root without children vs root with children differ.
    assert!(!a.equals(&*c));
}

fn context_builder() -> CommandContextBuilder<i32> {
    CommandContextBuilder::new(0, root_dyn(), 0)
}

// Keep the shared module import used for `literal` even though `StringRange` /
// `Suggestion` are unused in the tests above (parity with the upstream file).
#[allow(dead_code)]
fn _unused(_: StringRange, _: Suggestion) {}
