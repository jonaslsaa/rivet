//! Unit tests ported from the upstream brigadier `ArgumentBuilderTest`,
//! `LiteralArgumentBuilderTest` and `RequiredArgumentBuilderTest` (MIT), translated
//! against the `ArgumentBuilder` cluster.
//!
//! Faithful-behavior tests only. Java's `Mockito.mock(CommandNode.class)` / mocks are
//! replaced by concrete stub nodes; Java `CommandNode.equals`/`hasItem` are asserted
//! via `get_name` because full node equality arrives with the `tree` unit (the stub
//! nodes carry no `PartialEq`).

use std::sync::Arc;

use crate::arguments::ArgumentType;
use crate::builder::argument_builder::{ArgumentBuilder, ArgumentBuilderBehavior, Predicate};
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::builder::required_argument_builder::RequiredArgumentBuilder;
use crate::command::ClosureCommand;
use crate::context::CommandContext;
use crate::string_reader::StringReader;
use crate::tree::{ArgumentCommandNode, CommandNode, LiteralCommandNode};

/// Upstream `IntegerArgumentType.integer()` — a concrete `ArgumentType<Integer>`
/// used by the ported tests.
struct IntegerArgumentType;

impl ArgumentType<i32> for IntegerArgumentType {
    fn parse(
        &self,
        reader: &mut StringReader,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        reader.read_int()
    }
}

fn integer() -> Arc<dyn ArgumentType<i32>> {
    Arc::new(IntegerArgumentType)
}

/// A command recording its context source, for `executes(command)` assertions.
fn command_fn<S: Send + Sync + Clone + 'static>() -> Arc<dyn crate::command::Command<S>> {
    Arc::new(ClosureCommand::new(Box::new(|_| {
        Ok(crate::command::SINGLE_SUCCESS)
    })))
}

/// A concrete `RedirectModifier` for `fork(...)` assertions.
struct SourceRedirectModifier;

impl crate::redirect_modifier::RedirectModifier<i32> for SourceRedirectModifier {
    fn apply(
        &self,
        context: &CommandContext<i32>,
    ) -> Result<Vec<i32>, crate::exceptions::CommandSyntaxException<'static>> {
        Ok(vec![*context.get_source()])
    }
}

/// A concrete `SingleRedirectModifier` for `redirect_with_modifier(...)` assertions.
struct SourceSingleRedirectModifier;

impl crate::single_redirect_modifier::SingleRedirectModifier<i32> for SourceSingleRedirectModifier {
    fn apply(
        &self,
        context: &CommandContext<i32>,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        Ok(*context.get_source())
    }
}

/// A concrete `SuggestionProvider` for `suggests(...)` assertions.
struct EmptySuggestionProvider;

impl crate::suggestion::SuggestionProvider<i32> for EmptySuggestionProvider {
    fn get_suggestions(
        &self,
        _context: &CommandContext<i32>,
        _builder: &mut dyn crate::suggestion::SuggestionsBuilder,
    ) -> Result<(), crate::exceptions::CommandSyntaxException<'static>> {
        Ok(())
    }
}

/// Java `TestableArgumentBuilder` — a concrete builder whose `build()` the base
/// class leaves abstract. The tests only exercise the inherited surface.
struct TestableArgumentBuilder<S> {
    argument_builder: ArgumentBuilder<S>,
}

impl<S: 'static> TestableArgumentBuilder<S> {
    fn new() -> Self {
        TestableArgumentBuilder {
            argument_builder: ArgumentBuilder::new(),
        }
    }
}

impl<S: 'static> ArgumentBuilderBehavior<S> for TestableArgumentBuilder<S> {
    fn base(&self) -> &ArgumentBuilder<S> {
        &self.argument_builder
    }
    fn base_mut(&mut self) -> &mut ArgumentBuilder<S> {
        &mut self.argument_builder
    }
    fn build(&self) -> Box<dyn CommandNode<S>> {
        // Java test returns null; the builder cluster's own `then`/`redirect` never
        // call `build` on `self`, so a stub node is never observed.
        Box::new(LiteralCommandNode::new(
            String::new(),
            None,
            Arc::new(|_| true),
            None,
            None,
            false,
        ))
    }
}

// ---- ArgumentBuilderTest ----

#[test]
fn test_arguments() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let argument = RequiredArgumentBuilder::<i32, i32>::argument("bar", integer());

    builder.then(argument);

    assert_eq!(builder.get_arguments().len(), 1);
    assert_eq!(builder.get_arguments()[0].get_name(), "bar");
}

/// Java `then(CommandNode<S>)` overload — appends the raw node (no merge, no
/// `build()`). The `addChild` merge (duplicate names) is deferred to the tree unit.
#[test]
fn test_then_node_appends_raw_node() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let node = Arc::new(LiteralCommandNode::<i32>::new(
        "foo".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    builder.then_node(Arc::clone(&node));

    assert_eq!(builder.get_arguments().len(), 1);
    assert!(Arc::ptr_eq(&builder.get_arguments()[0], &node));
    // Java `redirect` after a `then_node` child throws (`forward` guards on non-empty
    // children).
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            builder.redirect(target);
        }))
        .is_err()
    );
}

/// Java `redirect(CommandNode, SingleRedirectModifier)` — wraps the single result in
/// a singleton `RedirectModifier` (`o -> Collections.singleton(modifier.apply(o))`),
/// so `getRedirectModifier()` is Some and `isFork()` stays false.
#[test]
fn test_redirect_with_modifier() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    let modifier: Arc<dyn crate::single_redirect_modifier::SingleRedirectModifier<i32>> =
        Arc::new(SourceSingleRedirectModifier);
    builder.redirect_with_modifier(Arc::clone(&target), Some(modifier));

    assert!(Arc::ptr_eq(&builder.get_redirect().unwrap(), &target));
    assert!(builder.get_redirect_modifier().is_some());
    assert!(!builder.is_fork());
}

#[test]
fn test_redirect() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let target = LiteralCommandNode::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    );
    let target: Arc<dyn CommandNode<i32>> = Arc::new(target);

    builder.redirect(Arc::clone(&target));
    assert!(Arc::ptr_eq(&builder.get_redirect().unwrap(), &target));
}

#[test]
#[should_panic(expected = "Cannot forward a node with children")]
fn test_redirect_with_child() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    let child = LiteralArgumentBuilder::<i32>::literal("foo");
    builder.then(child);
    builder.redirect(target);
}

#[test]
#[should_panic(expected = "Cannot add children to a redirected node")]
fn test_then_with_redirect() {
    let mut builder = TestableArgumentBuilder::<i32>::new();
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    builder.redirect(target);
    let child = LiteralArgumentBuilder::<i32>::literal("foo");
    builder.then(child);
}

// ---- build() preserves redirect / fork / requires / suggests (Java constructors
// store every parameter) ----
//
// `CommandNode.addChild` merge semantics (duplicate names, RootCommandNode
// rejection) are ported by the tree unit; the builders' `then`/`then_node`/`build`
// path funnels through `RootCommandNode.add_child`, so they gain it automatically.

#[test]
fn test_literal_build_preserves_redirect() {
    let mut builder = LiteralArgumentBuilder::<i32>::literal("foo");
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;

    builder.redirect(Arc::clone(&target));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<LiteralCommandNode<i32>>()
        .unwrap();
    assert!(Arc::ptr_eq(&node.get_redirect().unwrap(), &target));
}

#[test]
fn test_required_build_preserves_fork_and_modifier() {
    let mut builder = RequiredArgumentBuilder::<i32, i32>::argument("foo", integer());
    let target = Arc::new(LiteralCommandNode::<i32>::new(
        "target".to_string(),
        None,
        Arc::new(|_| true),
        None,
        None,
        false,
    )) as Arc<dyn CommandNode<i32>>;
    let modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<i32>> =
        Arc::new(SourceRedirectModifier);

    builder.fork(target, Arc::clone(&modifier));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<ArgumentCommandNode<i32, i32>>()
        .unwrap();
    assert!(node.is_fork());
    assert!(Arc::ptr_eq(
        &node.get_redirect_modifier().unwrap(),
        &modifier
    ));
}

#[test]
fn test_literal_build_preserves_requirement() {
    let mut builder = LiteralArgumentBuilder::<i32>::literal("foo");
    let requirement: Predicate<i32> = Arc::new(|s| *s > 0);
    builder.requires(Arc::clone(&requirement));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<LiteralCommandNode<i32>>()
        .unwrap();
    assert!(Arc::ptr_eq(&node.get_requirement(), &requirement));
}

#[test]
fn test_required_build_preserves_custom_suggestions() {
    let mut builder = RequiredArgumentBuilder::<i32, i32>::argument("foo", integer());
    let provider: Arc<dyn crate::suggestion::SuggestionProvider<i32>> =
        Arc::new(EmptySuggestionProvider);
    builder.suggests(Arc::clone(&provider));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<ArgumentCommandNode<i32, i32>>()
        .unwrap();
    assert!(Arc::ptr_eq(
        &node.get_custom_suggestions().unwrap(),
        &provider
    ));
}

// ---- LiteralArgumentBuilderTest ----

#[test]
fn test_literal_build() {
    let builder = LiteralArgumentBuilder::<i32>::literal("foo");
    let node = builder.build();

    let node = node
        .as_any()
        .downcast_ref::<LiteralCommandNode<i32>>()
        .unwrap();
    assert_eq!(node.get_literal(), "foo");
}

#[test]
fn test_literal_build_with_executor() {
    let mut builder = LiteralArgumentBuilder::<i32>::literal("foo");
    let command = command_fn::<i32>();
    builder.executes(Some(Arc::clone(&command)));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<LiteralCommandNode<i32>>()
        .unwrap();
    assert_eq!(node.get_literal(), "foo");
    assert!(Arc::ptr_eq(&node.get_command().unwrap(), &command));
}

#[test]
fn test_literal_build_with_children() {
    let mut builder = LiteralArgumentBuilder::<i32>::literal("foo");
    builder.then(RequiredArgumentBuilder::<i32, i32>::argument(
        "bar",
        integer(),
    ));
    builder.then(RequiredArgumentBuilder::<i32, i32>::argument(
        "baz",
        integer(),
    ));

    let node = builder.build();
    assert_eq!(node.get_children().len(), 2);
}

// ---- RequiredArgumentBuilderTest ----

#[test]
fn test_required_build() {
    let type_ = integer();
    let builder = RequiredArgumentBuilder::<i32, i32>::argument("foo", Arc::clone(&type_));
    let node = builder.build();

    let node = node
        .as_any()
        .downcast_ref::<ArgumentCommandNode<i32, i32>>()
        .unwrap();
    assert_eq!(node.get_name(), "foo");
    assert!(Arc::ptr_eq(node.get_type(), &type_));
}

#[test]
fn test_required_build_with_executor() {
    let type_ = integer();
    let mut builder = RequiredArgumentBuilder::<i32, i32>::argument("foo", Arc::clone(&type_));
    let command = command_fn::<i32>();
    builder.executes(Some(Arc::clone(&command)));

    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<ArgumentCommandNode<i32, i32>>()
        .unwrap();
    assert_eq!(node.get_name(), "foo");
    assert!(Arc::ptr_eq(node.get_type(), &type_));
    assert!(Arc::ptr_eq(&node.get_command().unwrap(), &command));
}

#[test]
fn test_required_build_with_children() {
    let mut builder = RequiredArgumentBuilder::<i32, i32>::argument("foo", integer());
    builder.then(RequiredArgumentBuilder::<i32, i32>::argument(
        "bar",
        integer(),
    ));
    builder.then(RequiredArgumentBuilder::<i32, i32>::argument(
        "baz",
        integer(),
    ));

    let node = builder.build();
    assert_eq!(node.get_children().len(), 2);
}

// ---- Paper `defaultRequirement()` ----

#[test]
fn default_requirement_accepts_everything() {
    let requirement = ArgumentBuilder::<i32>::default_requirement::<i32>();
    assert!(requirement(&1));
    assert!(requirement(&i32::MIN));
}

#[test]
fn default_requirement_is_shared_per_source_type() {
    // Paper's Commands.java:312 compares by identity (`node.getRequirement() ==
    // defaultRequirement()`): the static DEFAULT_REQUIREMENT is one instance, and
    // every builder-`new()`ed requirement IS that instance. Model the `==` with
    // Arc::ptr_eq — every call must return the same allocation for a given `S`.
    let a = ArgumentBuilder::<i32>::default_requirement::<i32>();
    let b = ArgumentBuilder::<i32>::default_requirement::<i32>();
    assert!(Arc::ptr_eq(&a, &b));

    // A builder constructed with `new()` starts with the shared default instance
    // (Java field initializer `requirement = defaultRequirement()`).
    let builder = LiteralArgumentBuilder::<i32>::literal("foo");
    assert!(Arc::ptr_eq(
        &builder.get_requirement(),
        &ArgumentBuilder::<i32>::default_requirement::<i32>()
    ));
    // The node's requirement is the builder's requirement (forwarded by `build()`).
    let node = builder.build();
    let node = node
        .as_any()
        .downcast_ref::<LiteralCommandNode<i32>>()
        .unwrap();
    assert!(Arc::ptr_eq(
        &node.get_requirement(),
        &ArgumentBuilder::<i32>::default_requirement::<i32>()
    ));

    // Different `S` types are distinct instances (Java casts the same Object, but
    // per-`S` arcs are a faithful Rust equivalent for the observable identity check).
    let c = ArgumentBuilder::<String>::default_requirement::<String>();
    assert_ne!(Arc::as_ptr(&a) as *const (), Arc::as_ptr(&c) as *const ());
}
