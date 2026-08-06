//! Unit tests ported from the upstream brigadier `ArgumentCommandNodeTest` (MIT),
//! plus the shared `AbstractCommandNodeTest` cases.

use std::sync::Arc;

use crate::arguments::integer_argument_type::IntegerArgumentType;
use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::context::CommandContextBuilder;
use crate::suggestion::SuggestionsBuilder;
use crate::tree::tests as shared;
use crate::tree::{ArgumentCommandNode, CommandNode, RootCommandNode};

fn node() -> Arc<ArgumentCommandNode<i32, i32>> {
    Arc::new(ArgumentCommandNode::new(
        "foo".to_string(),
        IntegerArgumentType::integer(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
        None,
    ))
}

fn node_dyn() -> Arc<dyn CommandNode<i32>> {
    node()
}

#[test]
fn test_add_child() {
    let n = node_dyn();
    shared::test_add_child(&n);
}

#[test]
fn test_add_child_merges_grandchildren() {
    let n = node_dyn();
    shared::test_add_child_merges_grandchildren(&n);
}

#[test]
fn test_add_child_preserves_command() {
    let n = node_dyn();
    shared::test_add_child_preserves_command(&n);
}

#[test]
fn test_add_child_overwrites_command() {
    let n = node_dyn();
    shared::test_add_child_overwrites_command(&n);
}

#[test]
fn test_parse() {
    let n = node();
    let mut reader = crate::string_reader::StringReader::new("123 456");
    let mut context_builder =
        CommandContextBuilder::new(0, Arc::new(RootCommandNode::<i32>::new()), 0);
    n.parse(
        Arc::clone(&n) as Arc<dyn CommandNode<i32>>,
        &mut reader,
        &mut context_builder,
    )
    .unwrap();

    assert!(context_builder.get_arguments().contains_key("foo"));
    let parsed = context_builder.get_arguments().get("foo").unwrap();
    assert_eq!(parsed.get_result::<i32>(), &123);
}

/// An `ArgumentType` whose `parse_with_source` override echoes the command source —
/// Java `ArgumentCommandNode.parse` calls `type.parse(reader, source)`; proving the
/// source reaches the type (the source-free `parse` would lose it).
struct SourceEchoType;

impl crate::arguments::ArgumentType<i32> for SourceEchoType {
    fn parse(
        &self,
        reader: &mut crate::string_reader::StringReader,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        reader.read_int()
    }

    fn parse_with_source(
        &self,
        _reader: &mut crate::string_reader::StringReader,
        source: &dyn std::any::Any,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        // Test-only: the node's source is always `i32` here.
        Ok(*source.downcast_ref::<i32>().expect("test source is i32"))
    }

    fn to_string(&self) -> String {
        "source_echo".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_parse_forwards_source() {
    let n = Arc::new(ArgumentCommandNode::<i32, i32>::new(
        "foo".to_string(),
        Arc::new(SourceEchoType),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
        None,
    ));
    let mut reader = crate::string_reader::StringReader::new("999");
    let mut context_builder =
        CommandContextBuilder::new(7, Arc::new(RootCommandNode::<i32>::new()), 0);
    n.parse(
        Arc::clone(&n) as Arc<dyn CommandNode<i32>>,
        &mut reader,
        &mut context_builder,
    )
    .unwrap();

    let parsed = context_builder.get_arguments().get("foo").unwrap();
    assert_eq!(parsed.get_result::<i32>(), &7);
}

#[test]
fn test_usage() {
    let n = node();
    assert_eq!(n.get_usage_text(), "<foo>");
}

#[test]
fn test_suggestions() {
    let n = node();
    let mut builder = SuggestionsBuilder::new_with_input("".to_string(), 0);
    let context = CommandContextBuilder::new(0, Arc::new(RootCommandNode::<i32>::new()), 0)
        .build("".to_string());
    let result = n.list_suggestions(&context, &mut builder).unwrap();
    assert!(result.is_empty());
}

/// An `ArgumentType` whose `list_suggestions` override echoes the command source —
/// Java `ArgumentCommandNode.listSuggestions` calls
/// `type.listSuggestions(context, builder)`; proving the context reaches the type.
struct SourceSuggestType;

impl crate::arguments::ArgumentType<i32> for SourceSuggestType {
    fn parse(
        &self,
        reader: &mut crate::string_reader::StringReader,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        reader.read_int()
    }

    fn list_suggestions(
        &self,
        context: &dyn std::any::Any,
        builder: &mut SuggestionsBuilder,
    ) -> crate::suggestion::Suggestions {
        // Test-only: the node's context source is always `i32` here.
        let source = context
            .downcast_ref::<crate::context::CommandContext<i32>>()
            .expect("test context source is i32")
            .get_source();
        builder.suggest(&format!("value:{}", source));
        builder.build()
    }

    fn to_string(&self) -> String {
        "source_suggest".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_suggestions_forwards_context() {
    let n = Arc::new(ArgumentCommandNode::<i32, i32>::new(
        "foo".to_string(),
        Arc::new(SourceSuggestType),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
        None,
    ));
    let mut builder = SuggestionsBuilder::new_with_input("".to_string(), 0);
    let context = CommandContextBuilder::new(7, Arc::new(RootCommandNode::<i32>::new()), 0)
        .build("".to_string());
    let result = n.list_suggestions(&context, &mut builder).unwrap();
    assert_eq!(result.get_list()[0].get_text(), "value:7");
}

#[test]
fn test_equals() {
    let command = shared::command_arc();

    let mk =
        |name: &str, min: i32, max: i32, cmd: Option<Arc<dyn crate::command::Command<i32>>>| {
            Arc::new(ArgumentCommandNode::<i32, i32>::new(
                name.to_string(),
                IntegerArgumentType::integer_with_bounds(min, max),
                cmd,
                Arc::new(|_| true),
                None,
                None,
                false,
                None,
            )) as Arc<dyn CommandNode<i32>>
        };
    let with_child = |name: &str| {
        let mut b = crate::builder::required_argument_builder::RequiredArgumentBuilder::<i32, i32>::argument(
            name,
            IntegerArgumentType::integer(),
        );
        b.then(crate::builder::required_argument_builder::RequiredArgumentBuilder::<i32, i32>::argument(
            "bar",
            IntegerArgumentType::integer(),
        ));
        Arc::from(b.build()) as Arc<dyn CommandNode<i32>>
    };

    // (foo, foo) equal.
    assert!(mk("foo", i32::MIN, i32::MAX, None).equals(&*mk("foo", i32::MIN, i32::MAX, None)));
    // (foo executes command, foo executes command) equal.
    assert!(
        mk("foo", i32::MIN, i32::MAX, Some(Arc::clone(&command))).equals(&*mk(
            "foo",
            i32::MIN,
            i32::MAX,
            Some(Arc::clone(&command))
        ))
    );
    // (bar bounded, bar bounded) equal.
    assert!(mk("bar", -100, 100, None).equals(&*mk("bar", -100, 100, None)));
    // (foo bounded, foo bounded) equal.
    assert!(mk("foo", -100, 100, None).equals(&*mk("foo", -100, 100, None)));
    // (foo with child, foo with child) equal.
    assert!(with_child("foo").equals(&*with_child("foo")));

    // Cross-group inequalities.
    assert!(!mk("foo", i32::MIN, i32::MAX, None).equals(&*mk("bar", i32::MIN, i32::MAX, None)));
    assert!(!mk("foo", i32::MIN, i32::MAX, None).equals(&*mk("foo", -100, 100, None)));
}

#[test]
fn test_create_builder() {
    let n = node();
    let builder = n.create_builder();
    let builder = builder
        .as_any()
        .downcast_ref::<crate::builder::required_argument_builder::RequiredArgumentBuilder<i32, i32>>()
        .unwrap();
    assert_eq!(builder.get_name(), "foo");
    assert!(Arc::ptr_eq(builder.get_type(), n.get_type()));
    assert!(Arc::ptr_eq(
        &builder.get_requirement(),
        &n.get_requirement()
    ));
    assert!(builder.get_command().is_none());
}
