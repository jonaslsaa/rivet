//! Unit tests ported from the upstream brigadier `CommandSuggestionsTest` (MIT).

use std::sync::Arc;

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::builder::required_argument_builder::RequiredArgumentBuilder;
use crate::command_dispatcher::CommandDispatcher;
use crate::context::{CommandContext, StringRange};
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestion, Suggestions, SuggestionsBuilder};
use crate::tree::CommandNode;

fn literal(name: &str) -> LiteralArgumentBuilder<String> {
    LiteralArgumentBuilder::literal(name)
}

fn integer() -> Arc<dyn crate::arguments::ArgumentType<i32>> {
    crate::arguments::integer_argument_type::IntegerArgumentType::integer()
}

fn word() -> Arc<dyn crate::arguments::ArgumentType<String>> {
    crate::arguments::string_argument_type::StringArgumentType::word()
}

fn test_suggestions(
    subject: &CommandDispatcher<String>,
    contents: &str,
    cursor: i32,
    range: StringRange,
    suggestions: &[&str],
) {
    let parse = subject.parse_string(contents, "source".to_string());
    let result = subject.get_completion_suggestions_with_cursor(&parse, cursor);
    assert_eq!(result.get_range(), range);

    let expected: Vec<Suggestion> = suggestions
        .iter()
        .map(|s| Suggestion::new(range, s.to_string()))
        .collect();
    assert_eq!(result.get_list(), &expected[..]);
}

fn input_with_offset(input: &str, offset: i32) -> StringReader {
    let mut result = StringReader::new(input);
    result.set_cursor(offset);
    result
}

#[test]
fn get_completion_suggestions_root_commands() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    subject.register(&mut foo);
    let mut bar = literal("bar");
    subject.register(&mut bar);
    let mut baz = literal("baz");
    subject.register(&mut baz);

    let parse = subject.parse_string("", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(0));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::at(0), "bar"),
            Suggestion::new(StringRange::at(0), "baz"),
            Suggestion::new(StringRange::at(0), "foo"),
        ]
    );
}

#[test]
fn get_completion_suggestions_root_commands_with_input_offset() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    subject.register(&mut foo);
    let mut bar = literal("bar");
    subject.register(&mut bar);
    let mut baz = literal("baz");
    subject.register(&mut baz);

    let parse = subject.parse(input_with_offset("OOO", 3), "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(3));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::at(3), "bar"),
            Suggestion::new(StringRange::at(3), "baz"),
            Suggestion::new(StringRange::at(3), "foo"),
        ]
    );
}

#[test]
fn get_completion_suggestions_root_commands_partial() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    subject.register(&mut foo);
    let mut bar = literal("bar");
    subject.register(&mut bar);
    let mut baz = literal("baz");
    subject.register(&mut baz);

    let parse = subject.parse_string("b", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(0, 1));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::between(0, 1), "bar"),
            Suggestion::new(StringRange::between(0, 1), "baz"),
        ]
    );
}

#[test]
fn get_completion_suggestions_root_commands_partial_with_input_offset() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    subject.register(&mut foo);
    let mut bar = literal("bar");
    subject.register(&mut bar);
    let mut baz = literal("baz");
    subject.register(&mut baz);

    let parse = subject.parse(input_with_offset("Zb", 1), "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(1, 2));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::between(1, 2), "bar"),
            Suggestion::new(StringRange::between(1, 2), "baz"),
        ]
    );
}

#[test]
fn get_completion_suggestions_sub_commands() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut parent = literal("parent");
    let foo = literal("foo");
    parent.then(foo);
    let bar = literal("bar");
    parent.then(bar);
    let baz = literal("baz");
    parent.then(baz);
    subject.register(&mut parent);

    let parse = subject.parse_string("parent ", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(7));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::at(7), "bar"),
            Suggestion::new(StringRange::at(7), "baz"),
            Suggestion::new(StringRange::at(7), "foo"),
        ]
    );
}

#[test]
fn get_completion_suggestions_moving_cursor_sub_commands() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut parent_one = literal("parent_one");
    let faz = literal("faz");
    parent_one.then(faz);
    let fbz = literal("fbz");
    parent_one.then(fbz);
    let gaz = literal("gaz");
    parent_one.then(gaz);
    subject.register(&mut parent_one);

    let mut parent_two = literal("parent_two");
    subject.register(&mut parent_two);

    test_suggestions(
        &subject,
        "parent_one faz ",
        0,
        StringRange::at(0),
        &["parent_one", "parent_two"],
    );
    test_suggestions(
        &subject,
        "parent_one faz ",
        1,
        StringRange::between(0, 1),
        &["parent_one", "parent_two"],
    );
    test_suggestions(
        &subject,
        "parent_one faz ",
        7,
        StringRange::between(0, 7),
        &["parent_one", "parent_two"],
    );
    test_suggestions(
        &subject,
        "parent_one faz ",
        8,
        StringRange::between(0, 8),
        &["parent_one"],
    );
    test_suggestions(&subject, "parent_one faz ", 10, StringRange::at(0), &[]);
    test_suggestions(
        &subject,
        "parent_one faz ",
        11,
        StringRange::at(11),
        &["faz", "fbz", "gaz"],
    );
    test_suggestions(
        &subject,
        "parent_one faz ",
        12,
        StringRange::between(11, 12),
        &["faz", "fbz"],
    );
    test_suggestions(
        &subject,
        "parent_one faz ",
        13,
        StringRange::between(11, 13),
        &["faz"],
    );
    test_suggestions(&subject, "parent_one faz ", 14, StringRange::at(0), &[]);
    test_suggestions(&subject, "parent_one faz ", 15, StringRange::at(0), &[]);
}

#[test]
fn get_completion_suggestions_sub_commands_partial() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut parent = literal("parent");
    let foo = literal("foo");
    parent.then(foo);
    let bar = literal("bar");
    parent.then(bar);
    let baz = literal("baz");
    parent.then(baz);
    subject.register(&mut parent);

    let parse = subject.parse_string("parent b", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(7, 8));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::between(7, 8), "bar"),
            Suggestion::new(StringRange::between(7, 8), "baz"),
        ]
    );
}

#[test]
fn get_completion_suggestions_sub_commands_partial_with_input_offset() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut parent = literal("parent");
    let foo = literal("foo");
    parent.then(foo);
    let bar = literal("bar");
    parent.then(bar);
    let baz = literal("baz");
    parent.then(baz);
    subject.register(&mut parent);

    let parse = subject.parse(input_with_offset("junk parent b", 5), "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(12, 13));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::between(12, 13), "bar"),
            Suggestion::new(StringRange::between(12, 13), "baz"),
        ]
    );
}

#[test]
fn get_completion_suggestions_redirect() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut actual = literal("actual");
    let sub = literal("sub");
    actual.then(sub);
    let actual_node = subject.register(&mut actual);

    let mut redirect = literal("redirect");
    redirect.redirect(actual_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect);

    let parse = subject.parse_string("redirect ", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(9));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::at(9), "sub")]
    );
}

#[test]
fn get_completion_suggestions_redirect_partial() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut actual = literal("actual");
    let sub = literal("sub");
    actual.then(sub);
    let actual_node = subject.register(&mut actual);

    let mut redirect = literal("redirect");
    redirect.redirect(actual_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect);

    let parse = subject.parse_string("redirect s", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(9, 10));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::between(9, 10), "sub")]
    );
}

#[test]
fn get_completion_suggestions_moving_cursor_redirect() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut actual_one = literal("actual_one");
    let faz = literal("faz");
    actual_one.then(faz);
    let fbz = literal("fbz");
    actual_one.then(fbz);
    let gaz = literal("gaz");
    actual_one.then(gaz);
    let actual_one_node = subject.register(&mut actual_one);

    let mut actual_two = literal("actual_two");
    subject.register(&mut actual_two);

    let mut redirect_one = literal("redirect_one");
    redirect_one.redirect(actual_one_node.clone() as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect_one);

    let mut redirect_two = literal("redirect_two");
    redirect_two.redirect(actual_one_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect_two);

    test_suggestions(
        &subject,
        "redirect_one faz ",
        0,
        StringRange::at(0),
        &["actual_one", "actual_two", "redirect_one", "redirect_two"],
    );
    test_suggestions(
        &subject,
        "redirect_one faz ",
        9,
        StringRange::between(0, 9),
        &["redirect_one", "redirect_two"],
    );
    test_suggestions(
        &subject,
        "redirect_one faz ",
        10,
        StringRange::between(0, 10),
        &["redirect_one"],
    );
    test_suggestions(&subject, "redirect_one faz ", 12, StringRange::at(0), &[]);
    test_suggestions(
        &subject,
        "redirect_one faz ",
        13,
        StringRange::at(13),
        &["faz", "fbz", "gaz"],
    );
    test_suggestions(
        &subject,
        "redirect_one faz ",
        14,
        StringRange::between(13, 14),
        &["faz", "fbz"],
    );
    test_suggestions(
        &subject,
        "redirect_one faz ",
        15,
        StringRange::between(13, 15),
        &["faz"],
    );
    test_suggestions(&subject, "redirect_one faz ", 16, StringRange::at(0), &[]);
    test_suggestions(&subject, "redirect_one faz ", 17, StringRange::at(0), &[]);
}

#[test]
fn get_completion_suggestions_redirect_partial_with_input_offset() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut actual = literal("actual");
    let sub = literal("sub");
    actual.then(sub);
    let actual_node = subject.register(&mut actual);

    let mut redirect = literal("redirect");
    redirect.redirect(actual_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect);

    let parse = subject.parse(input_with_offset("/redirect s", 1), "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::between(10, 11));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::between(10, 11), "sub")]
    );
}

#[test]
fn get_completion_suggestions_redirect_lots() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut loop_node = literal("redirect");
    let loop_handle = subject.register(&mut loop_node);

    let mut redirect = literal("redirect");
    let mut loop_builder = literal("loop");
    let mut loop_arg = RequiredArgumentBuilder::<String, i32>::argument("loop", integer());
    loop_arg.redirect(loop_handle as Arc<dyn CommandNode<String>>);
    loop_builder.then(loop_arg);
    redirect.then(loop_builder);
    subject.register(&mut redirect);

    let parse = subject.parse_string("redirect loop 1 loop 02 loop 003 ", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(33));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::at(33), "loop")]
    );
}

#[test]
fn get_completion_suggestions_redirect_contextual_argument() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut actual = literal("actual");
    let mut arg_one = RequiredArgumentBuilder::<String, String>::argument("arg_one", word());
    let mut arg_two = RequiredArgumentBuilder::<String, String>::argument("arg_two", word());
    arg_two.suggests(Some(Arc::new(ContextualProvider)));
    arg_one.then(arg_two);
    actual.then(arg_one);
    let actual_node = subject.register(&mut actual);

    let mut redirect = literal("redirect");
    redirect.redirect(actual_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect);

    let parse = subject.parse_string("redirect first ", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(15));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::at(15), "contextual_first")]
    );
}

/// `builder.suggest("contextual_" + arg_one)` — reads `arg_one` from the context.
struct ContextualProvider;

impl crate::suggestion::SuggestionProvider<String> for ContextualProvider {
    fn get_suggestions(
        &self,
        context: &CommandContext<String>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, crate::exceptions::CommandSyntaxException<'static>> {
        let arg_one = crate::arguments::string_argument_type::StringArgumentType::get_string(
            context, "arg_one",
        );
        builder.suggest(&format!("contextual_{}", arg_one));
        Ok(builder.build())
    }
}

#[test]
fn get_completion_suggestions_execute_simulation() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut execute = literal("execute");
    let execute_node = subject.register(&mut execute);

    let mut execute_full = literal("execute");
    let mut as_builder = literal("as");
    let mut as_name = RequiredArgumentBuilder::<String, String>::argument("name", word());
    as_name.redirect(execute_node.clone() as Arc<dyn CommandNode<String>>);
    as_builder.then(as_name);
    execute_full.then(as_builder);

    let mut store_builder = literal("store");
    let mut store_name = RequiredArgumentBuilder::<String, String>::argument("name", word());
    store_name.redirect(execute_node.clone() as Arc<dyn CommandNode<String>>);
    store_builder.then(store_name);
    execute_full.then(store_builder);

    let mut run_builder = literal("run");
    run_builder.executes(Some(Arc::new(Const0Command)));
    execute_full.then(run_builder);
    subject.register(&mut execute_full);

    let parse = subject.parse_string("execute as Dinnerbone as", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert!(result.is_empty());
}

/// `c -> 0`.
struct Const0Command;

impl crate::command::Command<String> for Const0Command {
    fn run(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<i32, crate::exceptions::CommandSyntaxException<'static>> {
        Ok(0)
    }
}

#[test]
fn get_completion_suggestions_execute_simulation_partial() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut execute = literal("execute");
    let execute_node = subject.register(&mut execute);

    let mut execute_full = literal("execute");
    let mut as_builder = literal("as");
    let mut as_bar = literal("bar");
    as_bar.redirect(execute_node.clone() as Arc<dyn CommandNode<String>>);
    as_builder.then(as_bar);
    let mut as_baz = literal("baz");
    as_baz.redirect(execute_node.clone() as Arc<dyn CommandNode<String>>);
    as_builder.then(as_baz);
    execute_full.then(as_builder);

    let mut store_builder = literal("store");
    let mut store_name = RequiredArgumentBuilder::<String, String>::argument("name", word());
    store_name.redirect(execute_node.clone() as Arc<dyn CommandNode<String>>);
    store_builder.then(store_name);
    execute_full.then(store_builder);

    let mut run_builder = literal("run");
    run_builder.executes(Some(Arc::new(Const0Command)));
    execute_full.then(run_builder);
    subject.register(&mut execute_full);

    let parse = subject.parse_string("execute as bar as ", "source".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(18));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::at(18), "bar"),
            Suggestion::new(StringRange::at(18), "baz"),
        ]
    );
}

#[test]
fn get_completion_suggestions_root_child_requirement_false() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    foo.requires(Arc::new(|s: &String| s == "admin"));
    subject.register(&mut foo);
    let mut bar = literal("bar");
    subject.register(&mut bar);

    // Paper: a root-level child whose requirement isn't met produces no suggestion.
    let parse = subject.parse_string("", "player".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(0));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::at(0), "bar")]
    );

    // A source meeting the requirement sees both.
    let parse = subject.parse_string("", "admin".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(0));
    assert_eq!(
        result.get_list(),
        &[
            Suggestion::new(StringRange::at(0), "bar"),
            Suggestion::new(StringRange::at(0), "foo"),
        ]
    );
}

#[test]
fn get_completion_suggestions_nested_child_requirement_not_checked() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut base = literal("base");
    let mut foo = literal("foo");
    foo.requires(Arc::new(|s: &String| s == "admin"));
    base.then(foo);
    subject.register(&mut base);

    // Paper's `parent != root || canUse` short-circuit: a non-root parent's children
    // are always suggested regardless of their requirement.
    let parse = subject.parse_string("base ", "player".to_string());
    let result = subject.get_completion_suggestions(&parse);
    assert_eq!(result.get_range(), StringRange::at(5));
    assert_eq!(
        result.get_list(),
        &[Suggestion::new(StringRange::at(5), "foo")]
    );
}
