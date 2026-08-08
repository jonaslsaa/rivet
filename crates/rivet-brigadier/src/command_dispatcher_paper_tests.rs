//! Focused tests for Paper's Brigadier patches ported here: #211 (the
//! `minecraft:`-prefix literal prioritization in `getRelevantNodes`) and #210 (the
//! `TagParseCommandSyntaxException` dispatch short-circuit).
//!
//! Paper's `getRelevantNodes(StringReader, Object source)` matches an unprefixed
//! input against a `minecraft:`-prefixed literal for two specific command-source
//! kinds (function parsing `source == CommandSource.NULL`; command blocks). The
//! Minecraft `CommandSourceStack` type lands with the command-dispatch units, so
//! the port generalizes the condition as the `CommandSource` trait on `S`; these
//! tests use an `S` whose `resolve_literal` maps an unprefixed word to its
//! `minecraft:` twin, and a plain `String` source (identity — Java's non-minecraft
//! sources perform the vanilla exact lookup).

use std::sync::Arc;

use crate::arguments::ArgumentType;
use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::builder::required_argument_builder::RequiredArgumentBuilder;
use crate::command_dispatcher::CommandDispatcher;
use crate::exceptions::tag_parse_command_syntax_exception::tag_parse_exception;
use crate::exceptions::{BuiltInExceptionProvider, CommandSyntaxException};
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;
use crate::tree::CommandNode;
use crate::tree::CommandSource;

/// An `S` standing in for Paper's function-parse command source: any unprefixed
/// word resolves to its `minecraft:`-prefixed twin (Java's `CommandSource.NULL`
/// branch of `getRelevantNodes`).
#[derive(Clone)]
struct FunctionSource;

impl CommandSource for FunctionSource {
    fn resolve_literal(&self, text: &str) -> String {
        if text.contains(':') {
            text.to_string()
        } else {
            format!("minecraft:{text}")
        }
    }
}

/// An `ArgumentType` that always throws a `TagParseCommandSyntaxException` (the
/// Paper SNBT tag-argument parse failure), consuming the input like Java's
/// `NbtOps` tag reader does before failing.
struct TagParseType;

impl ArgumentType<i32> for TagParseType {
    fn parse(&self, reader: &mut StringReader) -> Result<i32, CommandSyntaxException<'static>> {
        reader.skip();
        Err(tag_parse_exception("Invalid tag"))
    }

    fn to_string(&self) -> String {
        "tag_parse".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// An `ArgumentType` that consumes the rest of the input as a word and succeeds —
/// a sibling argument that would be reached only if dispatch falls through a
/// failed child.
struct ConsumingType;

impl ArgumentType<i32> for ConsumingType {
    fn parse(&self, reader: &mut StringReader) -> Result<i32, CommandSyntaxException<'static>> {
        let value = reader.read_string()?;
        Ok(value.encode_utf16().count() as i32)
    }

    fn to_string(&self) -> String {
        "consuming".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// An `ArgumentType` that always throws a plain `CommandSyntaxException` (not a
/// TagParse).
struct AlwaysFailsType;

impl ArgumentType<i32> for AlwaysFailsType {
    fn parse(&self, reader: &mut StringReader) -> Result<i32, CommandSyntaxException<'static>> {
        Err(CommandSyntaxException::built_in_exceptions()
            .dispatcher_unknown_argument()
            .create_with_context(reader))
    }

    fn to_string(&self) -> String {
        "always_fails".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn register_literal<S: 'static>(subject: &mut CommandDispatcher<S>, name: &str) {
    let mut builder = LiteralArgumentBuilder::<S>::literal(name);
    subject.register(&mut builder);
}

#[test]
fn minecraft_prefixed_literal_matches_unprefixed_input_with_minecraft_source() {
    // Root: `minecraft:foo` (no bare `foo`). A function-parse source ("foo")
    // should match the `minecraft:`-prefixed literal.
    let mut subject = CommandDispatcher::<FunctionSource>::new();
    register_literal(&mut subject, "minecraft:foo");

    let parse = subject.parse_string("foo", FunctionSource);
    assert_eq!(parse.get_context().get_nodes().len(), 1);
    let matched = parse.get_context().get_nodes()[0].get_node();
    assert_eq!(matched.get_name(), "minecraft:foo");
}

#[test]
fn plain_source_does_not_match_minecraft_prefixed_literal() {
    // A non-minecraft `String` source has the default identity `resolve_literal`:
    // "foo" must NOT match the "minecraft:foo" literal (Java's player/console
    // sources do the vanilla exact lookup).
    let mut subject = CommandDispatcher::<String>::new();
    register_literal(&mut subject, "minecraft:foo");

    let parse = subject.parse_string("foo", "console".to_string());
    // No literal matched and there are no argument children, so no child node was
    // ever parsed — the context is empty (Java: no relevant nodes) and the reader
    // still holds the input.
    assert_eq!(parse.get_context().get_nodes().len(), 0);
    assert!(parse.get_reader().can_read());
}

#[test]
fn unprefixed_input_prefers_minecraft_literal_over_bare_literal() {
    // Paper's ordering: when both `minecraft:foo` and `foo` exist, the unprefixed
    // input "foo" resolves to `minecraft:foo` FIRST (the minecraft literal wins).
    let mut subject = CommandDispatcher::<FunctionSource>::new();
    register_literal(&mut subject, "foo");
    register_literal(&mut subject, "minecraft:foo");

    let parse = subject.parse_string("foo", FunctionSource);
    assert_eq!(parse.get_context().get_nodes().len(), 1);
    let matched = parse.get_context().get_nodes()[0].get_node();
    assert_eq!(matched.get_name(), "minecraft:foo");
}

#[test]
fn source_mapping_falls_back_to_bare_literal_when_minecraft_missing() {
    // Paper's fallback (`if (literal == null) literal = literals.get(text)`): a
    // source-mapped word "foo" -> "minecraft:foo" misses when only the bare `foo`
    // literal is registered, and the exact word "foo" is then matched.
    let mut subject = CommandDispatcher::<FunctionSource>::new();
    register_literal(&mut subject, "foo");

    let parse = subject.parse_string("foo", FunctionSource);
    assert_eq!(parse.get_context().get_nodes().len(), 1);
    let matched = parse.get_context().get_nodes()[0].get_node();
    assert_eq!(matched.get_name(), "foo");
}

#[test]
fn already_prefixed_input_matches_minecraft_literal_exactly() {
    // `text.contains(':')` skips the mapping; the exact "minecraft:foo" matches.
    let mut subject = CommandDispatcher::<FunctionSource>::new();
    register_literal(&mut subject, "minecraft:foo");

    let parse = subject.parse_string("minecraft:foo", FunctionSource);
    assert_eq!(parse.get_context().get_nodes().len(), 1);
    let matched = parse.get_context().get_nodes()[0].get_node();
    assert_eq!(matched.get_name(), "minecraft:foo");
}

#[test]
fn is_valid_input_checks_full_literal_only() {
    // Paper's `isValidInput` runs the *first* parse pass only (`parse(reader,
    // false)`), so the unprefixed twin is NOT valid for a `minecraft:`-prefixed
    // literal — a counterfactual boundary distinct from the `parse` fallback.
    let node = crate::tree::LiteralCommandNode::<String>::new(
        "minecraft:foo".to_string(),
        None,
        Arc::new(|_: &String| true),
        None,
        None,
        false,
    );
    assert!(node.is_valid_input("minecraft:foo"));
    assert!(!node.is_valid_input("foo"));
}

#[test]
fn tag_parse_error_aborts_dispatch_and_is_recorded() {
    // #210: a `TagParseCommandSyntaxException` thrown while parsing a child aborts
    // the whole dispatch. Java records the child's error (`errors.put(child, ex)`
    // runs before the `stop` check), then returns immediately with the errors-so-far
    // (`if (stop) return new ParseResults<>(contextSoFar, originalReader, errors)`).
    // Both arguments are candidates (no literal matches "garbage"); the tag-parse
    // failure of the first must stop dispatch before the consuming sibling "b".
    let mut subject = CommandDispatcher::<String>::new();

    let mut cmd = LiteralArgumentBuilder::<String>::literal("cmd");
    let tag_arg = RequiredArgumentBuilder::<String, i32>::argument("a", Arc::new(TagParseType));
    cmd.then(tag_arg);
    let consume_arg =
        RequiredArgumentBuilder::<String, i32>::argument("b", Arc::new(ConsumingType));
    cmd.then(consume_arg);
    subject.register(&mut cmd);

    let parse = subject.parse_string("cmd garbage", "source".to_string());
    assert_eq!(parse.exceptions_len(), 1);
    assert!(crate::exceptions::is_tag_parse_exception(
        &parse.get_exceptions()[0].1
    ));
    // Dispatch stopped: the "b" sibling was never parsed (not in the context), and
    // the reader still holds the unconsumed input.
    assert_eq!(parse.get_context().get_nodes().len(), 1); // just "cmd"
    assert!(parse.get_reader().can_read());
}

#[test]
fn literal_incorrect_error_does_not_abort_dispatch() {
    // Counterfactual: a *non*-TagParse child error (a plain `CommandSyntaxException`)
    // must NOT abort — the dispatcher records it and falls through to the next
    // sibling argument, which parses the input.
    let mut subject = CommandDispatcher::<String>::new();

    let mut cmd = LiteralArgumentBuilder::<String>::literal("cmd");
    let fail_arg = RequiredArgumentBuilder::<String, i32>::argument("a", Arc::new(AlwaysFailsType));
    cmd.then(fail_arg);
    let consume_arg =
        RequiredArgumentBuilder::<String, i32>::argument("b", Arc::new(ConsumingType));
    cmd.then(consume_arg);
    subject.register(&mut cmd);

    let parse = subject.parse_string("cmd abc", "source".to_string());
    // "a" failed (non-aborting), "b" fell through and consumed "abc" — full parse
    // succeeds with no errors.
    assert_eq!(parse.get_context().get_nodes().len(), 2); // "cmd" + "b"
    assert_eq!(
        parse.get_context().get_nodes()[1].get_node().get_name(),
        "b"
    );
    assert!(parse.exceptions_is_empty());
    assert!(!parse.get_reader().can_read());
}
