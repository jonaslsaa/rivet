//! Unit tests ported from the upstream brigadier `LiteralCommandNodeTest` (MIT),
//! plus the shared `AbstractCommandNodeTest` cases.

use std::sync::Arc;

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::context::{CommandContextBuilder, StringRange};
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestion, SuggestionsBuilder};
use crate::tree::tests as shared;
use crate::tree::{CommandNode, LiteralCommandNode, RootCommandNode};

fn node() -> Arc<dyn CommandNode<i32>> {
    Arc::new(LiteralCommandNode::<i32>::new(
        "foo".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    ))
}

fn context_builder() -> CommandContextBuilder<i32> {
    CommandContextBuilder::new(0, Arc::new(RootCommandNode::<i32>::new()), 0)
}

#[test]
fn test_add_child() {
    let n = node();
    shared::test_add_child(&n);
}

#[test]
fn test_add_child_merges_grandchildren() {
    let n = node();
    shared::test_add_child_merges_grandchildren(&n);
}

#[test]
fn test_add_child_preserves_command() {
    let n = node();
    shared::test_add_child_preserves_command(&n);
}

#[test]
fn test_add_child_overwrites_command() {
    let n = node();
    shared::test_add_child_overwrites_command(&n);
}

#[test]
fn test_parse() {
    let n = node();
    let mut reader = StringReader::new("foo bar");
    n.parse(Arc::clone(&n), &mut reader, &mut context_builder())
        .unwrap();
    assert_eq!(reader.get_remaining(), " bar");
}

#[test]
fn test_parse_exact() {
    let n = node();
    let mut reader = StringReader::new("foo");
    n.parse(Arc::clone(&n), &mut reader, &mut context_builder())
        .unwrap();
    assert_eq!(reader.get_remaining(), "");
}

#[test]
fn test_parse_similar() {
    let n = node();
    let mut reader = StringReader::new("foobar");
    let err = n
        .parse(Arc::clone(&n), &mut reader, &mut context_builder())
        .unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().literal_incorrect()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn test_parse_invalid() {
    let n = node();
    let mut reader = StringReader::new("bar");
    let err = n
        .parse(Arc::clone(&n), &mut reader, &mut context_builder())
        .unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().literal_incorrect()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn test_usage() {
    let n = node();
    assert_eq!(n.get_usage_text(), "foo");
}

#[test]
fn test_suggestions() {
    let n = node();
    let mut b0 = SuggestionsBuilder::new_with_input("".to_string(), 0);
    let empty = n
        .list_suggestions(&context_builder().build("".to_string()), &mut b0)
        .unwrap();
    assert_eq!(
        empty.get_list(),
        &[Suggestion::new(StringRange::at(0), "foo")]
    );

    let mut b1 = SuggestionsBuilder::new_with_input("foo".to_string(), 0);
    let foo = n
        .list_suggestions(&context_builder().build("foo".to_string()), &mut b1)
        .unwrap();
    assert!(foo.is_empty());

    let mut b2 = SuggestionsBuilder::new_with_input("food".to_string(), 0);
    let food = n
        .list_suggestions(&context_builder().build("food".to_string()), &mut b2)
        .unwrap();
    assert!(food.is_empty());

    // Upstream Java's fourth case re-asserts `food` (a copy-paste bug); the
    // intended check is that "b" (not a prefix of "foo") yields no suggestions.
    let mut b3 = SuggestionsBuilder::new_with_input("b".to_string(), 0);
    let b = n
        .list_suggestions(&context_builder().build("b".to_string()), &mut b3)
        .unwrap();
    assert!(b.is_empty());
}

#[test]
fn test_equals() {
    let command = shared::command_arc();

    let foo1 = shared::literal_node("foo");
    let foo2 = shared::literal_node("foo");
    let foo_then_bar = |name: &str| {
        let mut b = LiteralArgumentBuilder::<i32>::literal(name);
        b.then(shared::literal("bar"));
        let node = b.build();
        Arc::from(node) as Arc<dyn CommandNode<i32>>
    };

    let bar_cmd = |name: &str, cmd: Arc<dyn crate::command::Command<i32>>| {
        let mut b = LiteralArgumentBuilder::<i32>::literal(name);
        b.executes(Some(cmd));
        Arc::from(b.build()) as Arc<dyn CommandNode<i32>>
    };
    let bar_plain = |name: &str| {
        let b = LiteralArgumentBuilder::<i32>::literal(name);
        Arc::from(b.build()) as Arc<dyn CommandNode<i32>>
    };

    // (foo, foo) equal.
    assert!(foo1.equals(&*foo2));
    // (bar with command, bar with command) equal.
    assert!(bar_cmd("bar", Arc::clone(&command)).equals(&*bar_cmd("bar", Arc::clone(&command))));
    // (bar plain, bar plain) equal.
    assert!(bar_plain("bar").equals(&*bar_plain("bar")));
    // (foo with bar child, foo with bar child) equal.
    assert!(foo_then_bar("foo").equals(&*foo_then_bar("foo")));
    // Across groups: foo vs bar differs.
    assert!(!foo1.equals(&*bar_plain("bar")));
}

#[test]
fn test_create_builder() {
    let n = Arc::new(LiteralCommandNode::<i32>::new(
        "foo".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    ));
    let builder = n.create_builder();
    let builder = builder
        .as_any()
        .downcast_ref::<LiteralArgumentBuilder<i32>>()
        .unwrap();
    assert_eq!(builder.get_literal(), "foo");
    assert!(Arc::ptr_eq(
        &builder.get_requirement(),
        &n.get_requirement()
    ));
    assert!(builder.get_command().is_none());
}
