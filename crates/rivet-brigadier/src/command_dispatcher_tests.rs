//! Unit tests ported from the upstream brigadier `CommandDispatcherTest` (MIT).
//!
//! Java's `@Mock Command`/`ResultConsumer` mocks are replaced by concrete closures
//! that record their calls; identity semantics are preserved via `Arc::ptr_eq` on
//! the shared recording cells. The `contextSourceMatches` matcher is a per-call
//! source check on the executed context.

use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex};

use crate::arguments::integer_argument_type::IntegerArgumentType;
use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::builder::required_argument_builder::RequiredArgumentBuilder;
use crate::command::Command;
use crate::command_dispatcher::CommandDispatcher;
use crate::context::CommandContext;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::result_consumer::ResultConsumer;
use crate::string_reader::StringReader;
use crate::tree::CommandNode;

/// A `Command` recording the source string it saw and returning a fixed result.
struct RecordingCommand {
    calls: Arc<Mutex<Vec<String>>>,
    result: i32,
}

impl Command<String> for RecordingCommand {
    fn run(
        &self,
        context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        self.calls
            .lock()
            .unwrap()
            .push(context.get_source().clone());
        Ok(self.result)
    }
}

/// A `Command` throwing a fixed exception.
struct ThrowingCommand {
    exception: CommandSyntaxException<'static>,
}

impl Command<String> for ThrowingCommand {
    fn run(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Err(self.exception.clone())
    }
}

/// A `Command` returning the current source (as a string length).
struct SourceLengthCommand;

impl Command<String> for SourceLengthCommand {
    fn run(
        &self,
        context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(context.get_source().len() as i32)
    }
}

/// The default noop-ish result consumer, recording `(success, result, source)`.
struct RecordingConsumer {
    calls: Arc<Mutex<Vec<(bool, i32, String)>>>,
}

impl ResultConsumer<String> for RecordingConsumer {
    fn on_command_complete(&self, context: &CommandContext<String>, success: bool, result: i32) {
        self.calls
            .lock()
            .unwrap()
            .push((success, result, context.get_source().clone()));
    }
}

fn literal<S: 'static>(name: &str) -> LiteralArgumentBuilder<S> {
    LiteralArgumentBuilder::literal(name)
}

fn argument<S: 'static>(name: &str) -> RequiredArgumentBuilder<S, i32> {
    RequiredArgumentBuilder::argument(name, IntegerArgumentType::integer())
}

fn input_with_offset(input: &str, offset: i32) -> StringReader {
    let mut result = StringReader::new(input);
    result.set_cursor(offset);
    result
}

/// `subject.register(literal("foo").executes(command))`.
fn register_executes(
    subject: &mut CommandDispatcher<String>,
    name: &str,
    command: Arc<dyn Command<String>>,
) {
    let mut builder = literal(name);
    builder.executes(Some(command));
    subject.register(&mut builder);
}

#[test]
fn test_create_and_execute_command() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 42,
    });
    let mut subject = CommandDispatcher::<String>::new();
    register_executes(&mut subject, "foo", command);

    assert_eq!(
        subject.execute_string("foo", "source".to_string()).unwrap(),
        42
    );
    assert_eq!(*calls.lock().unwrap(), vec!["source"]);
}

#[test]
fn test_create_and_execute_offset_command() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 42,
    });
    let mut subject = CommandDispatcher::<String>::new();
    register_executes(&mut subject, "foo", command);

    assert_eq!(
        subject
            .execute(input_with_offset("/foo", 1), "source".to_string())
            .unwrap(),
        42
    );
    assert_eq!(*calls.lock().unwrap(), vec!["source"]);
}

#[test]
fn test_create_and_merge_commands() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 42,
    });
    let mut subject = CommandDispatcher::<String>::new();

    let mut base1 = literal("base");
    let mut foo = literal("foo");
    foo.executes(Some(Arc::clone(&command)));
    base1.then(foo);
    subject.register(&mut base1);

    let mut base2 = literal("base");
    let mut bar = literal("bar");
    bar.executes(Some(Arc::clone(&command)));
    base2.then(bar);
    subject.register(&mut base2);

    assert_eq!(
        subject
            .execute_string("base foo", "source".to_string())
            .unwrap(),
        42
    );
    assert_eq!(
        subject
            .execute_string("base bar", "source".to_string())
            .unwrap(),
        42
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
}

fn assert_unknown_command_err(subject: &CommandDispatcher<String>, input: &str, cursor: i32) {
    match subject.execute_string(input, "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert!(crate::exceptions::exception_type_eq(
                ex.get_type(),
                CommandSyntaxException::built_in_exceptions().dispatcher_unknown_command()
            ));
            assert_eq!(ex.get_cursor(), cursor);
        }
    }
}

#[test]
fn test_execute_unknown_command() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut bar = literal("bar");
    subject.register(&mut bar);
    let mut baz = literal("baz");
    subject.register(&mut baz);
    assert_unknown_command_err(&subject, "foo", 0);
}

#[test]
fn test_execute_impermissible_command() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    foo.requires(Arc::new(|_| false));
    subject.register(&mut foo);
    assert_unknown_command_err(&subject, "foo", 0);
}

#[test]
fn test_execute_empty_command() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut empty = literal("");
    subject.register(&mut empty);
    assert_unknown_command_err(&subject, "", 0);
}

fn assert_unknown_argument_err(subject: &CommandDispatcher<String>, input: &str, cursor: i32) {
    match subject.execute_string(input, "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert!(crate::exceptions::exception_type_eq(
                ex.get_type(),
                CommandSyntaxException::built_in_exceptions().dispatcher_unknown_argument()
            ));
            assert_eq!(ex.get_cursor(), cursor);
        }
    }
}

#[test]
fn test_execute_unknown_subcommand() {
    let mut subject = CommandDispatcher::<String>::new();
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 0,
    });
    register_executes(&mut subject, "foo", command);
    assert_unknown_argument_err(&subject, "foo bar", 4);
}

#[test]
fn test_execute_incorrect_literal() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(SourceLengthCommand)));
    let bar = literal("bar");
    foo.then(bar);
    subject.register(&mut foo);
    assert_unknown_argument_err(&subject, "foo baz", 4);
}

#[test]
fn test_execute_ambiguous_incorrect_argument() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(SourceLengthCommand)));
    let bar = literal("bar");
    foo.then(bar);
    let baz = literal("baz");
    foo.then(baz);
    subject.register(&mut foo);
    assert_unknown_argument_err(&subject, "foo unknown", 4);
}

#[test]
fn test_execute_subcommand() {
    let mut subject = CommandDispatcher::<String>::new();
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let sub: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 100,
    });

    let mut foo = literal("foo");
    let a = literal("a");
    foo.then(a);
    let mut eq = literal("=");
    eq.executes(Some(Arc::clone(&sub)));
    foo.then(eq);
    let c = literal("c");
    foo.then(c);
    foo.executes(Some(Arc::new(SourceLengthCommand)));
    subject.register(&mut foo);

    assert_eq!(
        subject
            .execute_string("foo =", "source".to_string())
            .unwrap(),
        100
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn test_parse_incomplete_literal() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let mut bar = literal("bar");
    bar.executes(Some(Arc::new(SourceLengthCommand)));
    foo.then(bar);
    subject.register(&mut foo);

    let parse = subject.parse_string("foo ", "source".to_string());
    assert_eq!(parse.get_reader().get_remaining(), " ");
    assert_eq!(parse.get_context().get_nodes().len(), 1);
}

#[test]
fn test_parse_incomplete_argument() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let mut bar = argument("bar");
    bar.executes(Some(Arc::new(SourceLengthCommand)));
    foo.then(bar);
    subject.register(&mut foo);

    let parse = subject.parse_string("foo ", "source".to_string());
    assert_eq!(parse.get_reader().get_remaining(), " ");
    assert_eq!(parse.get_context().get_nodes().len(), 1);
}

#[test]
fn test_execute_ambiguous_parent_subcommand() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let sub: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 100,
    });
    let wrong_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let wrong: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&wrong_calls),
        result: 0,
    });

    let mut subject = CommandDispatcher::<String>::new();
    let mut test = literal("test");
    let mut incorrect = argument("incorrect");
    incorrect.executes(Some(Arc::clone(&wrong)));
    test.then(incorrect);
    let mut right = argument("right");
    let mut sub_arg = argument("sub");
    sub_arg.executes(Some(Arc::clone(&sub)));
    right.then(sub_arg);
    test.then(right);
    subject.register(&mut test);

    assert_eq!(
        subject
            .execute_string("test 1 2", "source".to_string())
            .unwrap(),
        100
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(wrong_calls.lock().unwrap().len(), 0);
}

#[test]
fn test_execute_ambiguous_parent_subcommand_via_redirect() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let sub: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 100,
    });
    let wrong_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let wrong: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&wrong_calls),
        result: 0,
    });

    let mut subject = CommandDispatcher::<String>::new();
    let mut test = literal("test");
    let mut incorrect = argument("incorrect");
    incorrect.executes(Some(Arc::clone(&wrong)));
    test.then(incorrect);
    let mut right = argument("right");
    let mut sub_arg = argument("sub");
    sub_arg.executes(Some(Arc::clone(&sub)));
    right.then(sub_arg);
    test.then(right);
    let real = subject.register(&mut test);

    let mut redirect = literal("redirect");
    redirect.redirect(real as Arc<dyn CommandNode<String>>);
    subject.register(&mut redirect);

    assert_eq!(
        subject
            .execute_string("redirect 1 2", "source".to_string())
            .unwrap(),
        100
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(wrong_calls.lock().unwrap().len(), 0);
}

#[test]
fn test_execute_redirected_multiple_times() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 42,
    });

    let mut subject = CommandDispatcher::<String>::new();
    let concrete = subject.register(&mut literal("actual").with_command_chain(command.clone()));
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut redirected = literal("redirected");
    redirected.redirect(root_dyn);
    let redirect_node = subject.register(&mut redirected);

    let input = "redirected redirected actual";
    let parse = subject.parse_string(input, "source".to_string());
    let root_dyn: Arc<dyn CommandNode<String>> = subject.get_root().clone();
    let redirect_dyn: Arc<dyn CommandNode<String>> = redirect_node;
    let ctx = parse.get_context();
    assert_eq!(ctx.get_range().get_string(input), "redirected");
    assert_eq!(ctx.get_nodes().len(), 1);
    assert!(Arc::ptr_eq(ctx.get_root_node(), &root_dyn));
    assert_eq!(ctx.get_nodes()[0].get_range(), ctx.get_range());
    assert!(Arc::ptr_eq(&ctx.get_nodes()[0].get_node(), &redirect_dyn));

    let child1 = ctx.get_child().unwrap();
    assert_eq!(child1.get_range().get_string(input), "redirected");
    assert_eq!(child1.get_nodes().len(), 1);
    assert!(Arc::ptr_eq(child1.get_root_node(), &root_dyn));
    assert_eq!(child1.get_nodes()[0].get_range(), child1.get_range());
    assert!(Arc::ptr_eq(
        &child1.get_nodes()[0].get_node(),
        &redirect_dyn
    ));

    let child2 = child1.get_child().unwrap();
    assert_eq!(child2.get_range().get_string(input), "actual");
    assert_eq!(child2.get_nodes().len(), 1);
    assert!(Arc::ptr_eq(child2.get_root_node(), &root_dyn));
    assert_eq!(child2.get_nodes()[0].get_range(), child2.get_range());
    assert!(Arc::ptr_eq(
        &child2.get_nodes()[0].get_node(),
        &(concrete as Arc<dyn CommandNode<String>>)
    ));

    assert_eq!(subject.execute_parse(parse).unwrap(), 42);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

/// Helper: build a `LiteralArgumentBuilder` with a command set, returning the
/// builder for `register`.
trait WithCommandChain {
    fn with_command_chain(self, command: Arc<dyn Command<String>>) -> Self;
}

impl WithCommandChain for LiteralArgumentBuilder<String> {
    fn with_command_chain(mut self, command: Arc<dyn Command<String>>) -> Self {
        self.executes(Some(command));
        self
    }
}

#[test]
fn test_correct_execute_context_after_redirect() {
    let mut subject = CommandDispatcher::<i32>::new();
    let root = subject.get_root().clone() as Arc<dyn CommandNode<i32>>;

    // add <value> redirects to root with source + value.
    let mut add = literal("add");
    let mut add_arg =
        RequiredArgumentBuilder::<i32, i32>::argument("value", IntegerArgumentType::integer());
    let add_redirect = root.clone();
    let add_modifier: Arc<dyn crate::single_redirect_modifier::SingleRedirectModifier<i32>> =
        Arc::new(crate::command_dispatcher_tests::AddValueModifier);
    add_arg.redirect_with_modifier(add_redirect, Some(add_modifier));
    add.then(add_arg);
    subject.register(&mut add);

    let mut blank = literal("blank");
    blank.redirect(root.clone());
    subject.register(&mut blank);

    let mut run = literal("run");
    run.executes(Some(Arc::new(
        crate::command_dispatcher_tests::IdentitySourceCommand,
    )));
    subject.register(&mut run);

    assert_eq!(subject.execute_string("run", 0).unwrap(), 0);
    assert_eq!(subject.execute_string("run", 1).unwrap(), 1);
    assert_eq!(subject.execute_string("add 5 run", 1).unwrap(), 6);
    assert_eq!(subject.execute_string("add 5 add 6 run", 2).unwrap(), 13);
    assert_eq!(subject.execute_string("add 5 blank run", 1).unwrap(), 6);
    assert_eq!(subject.execute_string("blank add 5 run", 1).unwrap(), 6);
    assert_eq!(
        subject.execute_string("add 5 blank add 6 run", 2).unwrap(),
        13
    );
    assert_eq!(
        subject
            .execute_string("add 5 blank blank add 6 run", 2)
            .unwrap(),
        13
    );
}

/// `c -> c.getSource() + getInteger(c, "value")`.
struct AddValueModifier;

impl crate::single_redirect_modifier::SingleRedirectModifier<i32> for AddValueModifier {
    fn apply(&self, context: &CommandContext<i32>) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(*context.get_source() + IntegerArgumentType::get_integer(context, "value"))
    }
}

/// `CommandContext::getSource` for an `i32` source.
struct IdentitySourceCommand;

impl Command<i32> for IdentitySourceCommand {
    fn run(&self, context: &CommandContext<i32>) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(*context.get_source())
    }
}

#[test]
fn test_shared_redirect_and_execute_nodes() {
    let mut subject = CommandDispatcher::<i32>::new();
    let root = subject.get_root().clone() as Arc<dyn CommandNode<i32>>;

    let mut add = literal("add");
    let mut add_arg =
        RequiredArgumentBuilder::<i32, i32>::argument("value", IntegerArgumentType::integer());
    let add_modifier: Arc<dyn crate::single_redirect_modifier::SingleRedirectModifier<i32>> =
        Arc::new(AddValueModifier);
    add_arg.redirect_with_modifier(root, Some(add_modifier));
    add_arg.executes(Some(Arc::new(IdentitySourceCommand)));
    add.then(add_arg);
    subject.register(&mut add);

    assert_eq!(subject.execute_string("add 5", 1).unwrap(), 1);
    assert_eq!(subject.execute_string("add 5 add 6", 1).unwrap(), 6);
}

#[test]
fn test_execute_redirected() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 0,
    });

    let mut subject = CommandDispatcher::<String>::new();
    subject.register(&mut literal("actual").with_command_chain(command.clone()));

    // fork redirect with a modifier returning [source1, source2].
    let modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(TwoSourceModifier);
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut redirected = literal("redirected");
    redirected.fork(root_dyn, modifier);
    subject.register(&mut redirected);

    let input = "redirected actual";
    let parse = subject.parse_string(input, "source".to_string());
    let root_dyn: Arc<dyn CommandNode<String>> = subject.get_root().clone();
    let ctx = parse.get_context();
    assert_eq!(ctx.get_range().get_string(input), "redirected");
    assert_eq!(ctx.get_nodes().len(), 1);
    assert!(Arc::ptr_eq(ctx.get_root_node(), &root_dyn));
    assert_eq!(ctx.get_nodes()[0].get_range(), ctx.get_range());
    assert!(ctx.get_nodes()[0].get_node().get_usage_text() == "redirected");
    assert_eq!(ctx.get_source(), &"source".to_string());

    let parent = ctx.get_child().unwrap();
    assert_eq!(parent.get_range().get_string(input), "actual");
    assert_eq!(parent.get_nodes().len(), 1);
    assert!(Arc::ptr_eq(parent.get_root_node(), &root_dyn));
    assert_eq!(parent.get_nodes()[0].get_range(), parent.get_range());
    assert!(parent.get_nodes()[0].get_node().get_usage_text() == "actual");
    assert_eq!(parent.get_source(), &"source".to_string());

    assert_eq!(subject.execute_parse(parse).unwrap(), 2);
    let mut seen = calls.lock().unwrap();
    seen.sort();
    assert_eq!(*seen, vec!["source1".to_string(), "source2".to_string()]);
}

/// Fork modifier returning the two fixed sources.
struct TwoSourceModifier;

impl crate::redirect_modifier::RedirectModifier<String> for TwoSourceModifier {
    fn apply(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        Ok(vec!["source1".to_string(), "source2".to_string()])
    }
}

#[test]
fn test_incomplete_redirect_should_throw() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let mut bar = literal("bar");
    let mut value = argument("value");
    value.executes(Some(Arc::new(GetIntegerCommand)));
    bar.then(value);
    foo.then(bar);
    let mut awa = literal("awa");
    awa.executes(Some(Arc::new(
        crate::command_dispatcher_tests::Const2Command,
    )));
    foo.then(awa);
    let foo_node = subject.register(&mut foo);

    let mut baz = literal("baz");
    baz.redirect(foo_node as Arc<dyn CommandNode<String>>);
    subject.register(&mut baz);

    match subject.execute_string("baz bar", "source".to_string()) {
        Ok(_) => panic!("Should have thrown an exception"),
        Err(e) => {
            assert!(crate::exceptions::exception_type_eq(
                e.get_type(),
                CommandSyntaxException::built_in_exceptions().dispatcher_unknown_command()
            ));
        }
    }
}

/// `context -> IntegerArgumentType.getInteger(context, "value")`.
struct GetIntegerCommand;

impl Command<String> for GetIntegerCommand {
    fn run(
        &self,
        context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(IntegerArgumentType::get_integer(context, "value"))
    }
}

/// `context -> 2`.
struct Const2Command;

impl Command<String> for Const2Command {
    fn run(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(2)
    }
}

#[test]
fn test_redirect_modifier_empty_result() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let mut bar = literal("bar");
    let mut value = argument("value");
    value.executes(Some(Arc::new(GetIntegerCommand)));
    bar.then(value);
    foo.then(bar);
    let mut awa = literal("awa");
    awa.executes(Some(Arc::new(Const2Command)));
    foo.then(awa);
    let foo_node = subject.register(&mut foo);

    let empty_modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(EmptyModifier);
    let mut baz = literal("baz");
    baz.fork(foo_node as Arc<dyn CommandNode<String>>, empty_modifier);
    subject.register(&mut baz);

    assert_eq!(
        subject
            .execute_string("baz bar 100", "source".to_string())
            .unwrap(),
        0
    );
}

/// `context -> Collections.emptyList()`.
struct EmptyModifier;

impl crate::redirect_modifier::RedirectModifier<String> for EmptyModifier {
    fn apply(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        Ok(Vec::new())
    }
}

#[test]
fn test_execute_orphaned_subcommand() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let bar = argument("bar");
    foo.then(bar);
    foo.executes(Some(Arc::new(SourceLengthCommand)));
    subject.register(&mut foo);

    match subject.execute_string("foo 5", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert!(crate::exceptions::exception_type_eq(
                ex.get_type(),
                CommandSyntaxException::built_in_exceptions().dispatcher_unknown_command()
            ));
            assert_eq!(ex.get_cursor(), 5);
        }
    }
}

#[test]
fn test_execute_invalid_other() {
    let wrong_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let wrong: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&wrong_calls),
        result: 0,
    });
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 42,
    });

    let mut subject = CommandDispatcher::<String>::new();
    register_executes(&mut subject, "w", wrong);
    register_executes(&mut subject, "world", command);

    assert_eq!(
        subject
            .execute_string("world", "source".to_string())
            .unwrap(),
        42
    );
    assert_eq!(wrong_calls.lock().unwrap().len(), 0);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn parse_no_space_separator() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let mut bar = argument("bar");
    bar.executes(Some(Arc::new(GetIntegerCommand)));
    foo.then(bar);
    subject.register(&mut foo);

    match subject.execute_string("foo$", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert!(crate::exceptions::exception_type_eq(
                ex.get_type(),
                CommandSyntaxException::built_in_exceptions().dispatcher_unknown_command()
            ));
            assert_eq!(ex.get_cursor(), 0);
        }
    }
}

#[test]
fn test_execute_invalid_subcommand() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let bar = argument("bar");
    foo.then(bar);
    foo.executes(Some(Arc::new(SourceLengthCommand)));
    subject.register(&mut foo);

    match subject.execute_string("foo bar", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert!(crate::exceptions::exception_type_eq(
                ex.get_type(),
                CommandSyntaxException::built_in_exceptions().reader_expected_int()
            ));
            assert_eq!(ex.get_cursor(), 4);
        }
    }
}

#[test]
fn test_get_path() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let bar = literal("bar");
    foo.then(bar);
    let foo_node = subject.register(&mut foo);

    // Find the `bar` node handle from the tree.
    let bar_node = foo_node.get_child("bar").unwrap();
    let path = subject.get_path(&bar_node);
    assert_eq!(path, ["foo", "bar"]);
}

#[test]
fn test_find_node_exists() {
    let mut subject = CommandDispatcher::<String>::new();
    let mut foo = literal("foo");
    let bar = literal("bar");
    foo.then(bar);
    subject.register(&mut foo);

    let found = subject
        .find_node(&["foo".to_string(), "bar".to_string()])
        .unwrap();
    assert_eq!(found.get_usage_text(), "bar");
}

#[test]
fn test_find_node_doesnt_exist() {
    let subject = CommandDispatcher::<String>::new();
    assert!(
        subject
            .find_node(&["foo".to_string(), "bar".to_string()])
            .is_none()
    );
}

#[test]
fn test_result_consumer_in_non_error_run() {
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 5,
    });
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "foo", command);

    assert_eq!(
        subject.execute_string("foo", "source".to_string()).unwrap(),
        5
    );
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(true, 5, "source".to_string())]
    );
}

#[test]
fn test_result_consumer_in_forked_non_error_run() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, i32)>::new()));
    let consumer: Arc<dyn ResultConsumer<i32>> = Arc::new(RecordingConsumerI32 {
        calls: Arc::clone(&consumer_calls),
    });

    let mut subject = CommandDispatcher::<i32>::new();
    subject.set_consumer(consumer);

    // `foo` executes to the source value (Java `c -> (Integer)(c.getSource())`).
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(IdentitySourceCommand)));
    subject.register(&mut foo);

    // `repeat` forks root with the three sources.
    let contexts: Vec<i32> = vec![9, 10, 11];
    let modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<i32>> =
        Arc::new(FixedSourcesModifierI32 {
            sources: contexts.clone(),
        });
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<i32>>;
    let mut repeat = literal("repeat");
    repeat.fork(root_dyn, modifier);
    subject.register(&mut repeat);

    let result = subject.execute_string("repeat foo", 0).unwrap();
    assert_eq!(result, 3);
    let mut calls = consumer_calls.lock().unwrap().clone();
    calls.sort_by_key(|a| a.2);
    assert_eq!(calls, vec![(true, 9, 9), (true, 10, 10), (true, 11, 11),]);
}

/// A `ResultConsumer<i32>` recording `(success, result, source)` triples.
struct RecordingConsumerI32 {
    calls: Arc<Mutex<Vec<(bool, i32, i32)>>>,
}

impl ResultConsumer<i32> for RecordingConsumerI32 {
    fn on_command_complete(&self, context: &CommandContext<i32>, success: bool, result: i32) {
        self.calls
            .lock()
            .unwrap()
            .push((success, result, *context.get_source()));
    }
}

/// Fork modifier returning a fixed list of `i32` sources.
struct FixedSourcesModifierI32 {
    sources: Vec<i32>,
}

impl crate::redirect_modifier::RedirectModifier<i32> for FixedSourcesModifierI32 {
    fn apply(
        &self,
        _context: &CommandContext<i32>,
    ) -> Result<Vec<i32>, CommandSyntaxException<'static>> {
        Ok(self.sources.clone())
    }
}

/// Fork modifier returning a fixed list of sources.
struct FixedSourcesModifier {
    sources: Vec<String>,
}

impl crate::redirect_modifier::RedirectModifier<String> for FixedSourcesModifier {
    fn apply(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        Ok(self.sources.clone())
    }
}

#[test]
fn test_exception_in_non_forked_command() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();
    let command: Arc<dyn Command<String>> = Arc::new(ThrowingCommand { exception });

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "crash", command);

    match subject.execute_string("crash", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert_eq!(ex.get_raw_message().get_string(), "Expected bool");
        }
    }
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(false, 0, "source".to_string())]
    );
}

#[test]
fn test_exception_in_non_forked_redirected_command() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();
    let command: Arc<dyn Command<String>> = Arc::new(ThrowingCommand { exception });

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "crash", command);
    let mut redirect = literal("redirect");
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    redirect.redirect(root_dyn);
    subject.register(&mut redirect);

    match subject.execute_string("redirect crash", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert_eq!(ex.get_raw_message().get_string(), "Expected bool");
        }
    }
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(false, 0, "source".to_string())]
    );
}

#[test]
fn test_exception_in_forked_redirected_command() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();
    let command: Arc<dyn Command<String>> = Arc::new(ThrowingCommand { exception });

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "crash", command);

    // `redirect` forks root with `Collections::singleton` (identity source).
    let modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(SingletonModifier);
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut redirect = literal("redirect");
    redirect.fork(root_dyn, modifier);
    subject.register(&mut redirect);

    assert_eq!(
        subject
            .execute_string("redirect crash", "source".to_string())
            .unwrap(),
        0
    );
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(false, 0, "source".to_string())]
    );
}

/// `Collections::singleton(source)` — identity single-source fork.
struct SingletonModifier;

impl crate::redirect_modifier::RedirectModifier<String> for SingletonModifier {
    fn apply(
        &self,
        context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        Ok(vec![context.get_source().clone()])
    }
}

#[test]
fn test_exception_in_non_forked_redirect() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 3,
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "noop", command);

    let throwing_modifier: Arc<
        dyn crate::single_redirect_modifier::SingleRedirectModifier<String>,
    > = Arc::new(ThrowingSingleModifier { exception });
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut redirect = literal("redirect");
    redirect.redirect_with_modifier(root_dyn, Some(throwing_modifier));
    subject.register(&mut redirect);

    match subject.execute_string("redirect noop", "source".to_string()) {
        Ok(_) => panic!("expected error"),
        Err(ex) => {
            assert_eq!(ex.get_raw_message().get_string(), "Expected bool");
        }
    }
    assert_eq!(calls.lock().unwrap().len(), 0);
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(false, 0, "source".to_string())]
    );
}

/// A `SingleRedirectModifier` that throws the fixed exception.
struct ThrowingSingleModifier {
    exception: CommandSyntaxException<'static>,
}

impl crate::single_redirect_modifier::SingleRedirectModifier<String> for ThrowingSingleModifier {
    fn apply(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<String, CommandSyntaxException<'static>> {
        Err(self.exception.clone())
    }
}

#[test]
fn test_exception_in_forked_redirect() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 3,
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "noop", command);

    let throwing_modifier: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(ThrowingRedirectModifier { exception });
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut redirect = literal("redirect");
    redirect.fork(root_dyn, throwing_modifier);
    subject.register(&mut redirect);

    assert_eq!(
        subject
            .execute_string("redirect noop", "source".to_string())
            .unwrap(),
        0
    );
    assert_eq!(calls.lock().unwrap().len(), 0);
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![(false, 0, "source".to_string())]
    );
}

/// A `RedirectModifier` that throws the fixed exception.
struct ThrowingRedirectModifier {
    exception: CommandSyntaxException<'static>,
}

impl crate::redirect_modifier::RedirectModifier<String> for ThrowingRedirectModifier {
    fn apply(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        Err(self.exception.clone())
    }
}

#[test]
fn test_partial_exception_in_forked_redirect() {
    let consumer_calls = Arc::new(Mutex::new(Vec::<(bool, i32, String)>::new()));
    let consumer: Arc<dyn ResultConsumer<String>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let command: Arc<dyn Command<String>> = Arc::new(RecordingCommand {
        calls: Arc::clone(&calls),
        result: 3,
    });
    let exception = CommandSyntaxException::built_in_exceptions()
        .reader_expected_bool()
        .create();

    let mut subject = CommandDispatcher::<String>::new();
    subject.set_consumer(consumer);
    register_executes(&mut subject, "run", command);

    let split_sources = vec![
        "source".to_string(),
        "rejected".to_string(),
        "other".to_string(),
    ];
    let split: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(FixedSourcesModifier {
            sources: split_sources,
        });
    let root_dyn = subject.get_root().clone() as Arc<dyn CommandNode<String>>;
    let mut split_node = literal("split");
    split_node.fork(root_dyn.clone(), split);
    subject.register(&mut split_node);

    // `filter` forks root, throwing for `rejected`, else singleton identity.
    let filter: Arc<dyn crate::redirect_modifier::RedirectModifier<String>> =
        Arc::new(FilterModifier { exception });
    let mut filter_node = literal("filter");
    filter_node.fork(root_dyn, filter);
    subject.register(&mut filter_node);

    let result = subject
        .execute_string("split filter run", "source".to_string())
        .unwrap();
    assert_eq!(result, 2);

    let mut runs = calls.lock().unwrap().clone();
    runs.sort();
    assert_eq!(runs, vec!["other".to_string(), "source".to_string()]);

    let mut consumer_events = consumer_calls.lock().unwrap().clone();
    consumer_events.sort_by(|a, b| a.2.cmp(&b.2).then(a.1.cmp(&b.1)));
    // `rejected` fails during the filter stage; `source` and `other` execute.
    // Sorted by source: "other" < "rejected" < "source".
    assert_eq!(
        consumer_events,
        vec![
            (true, 3, "other".to_string()),
            (false, 0, "rejected".to_string()),
            (true, 3, "source".to_string()),
        ]
    );
}

/// A fork modifier throwing for the `rejected` source, else identity singleton.
struct FilterModifier {
    exception: CommandSyntaxException<'static>,
}

impl crate::redirect_modifier::RedirectModifier<String> for FilterModifier {
    fn apply(
        &self,
        context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        let current = context.get_source();
        if current == "rejected" {
            return Err(self.exception.clone());
        }
        Ok(vec![current.clone()])
    }
}

// Keep AtomicI32 import used (parity with upstream `thenThrow` semantics; some
// tests may need a shared counter).
#[allow(dead_code)]
fn _atomic(_: &AtomicI32) {}

/// A `(child, sibling, inputs)` triple recorded by `RecordingAmbiguityConsumer`.
type AmbiguityCall = (String, String, Vec<String>);

/// A recording `AmbiguityConsumer` capturing `(child, sibling, inputs)` triples.
struct RecordingAmbiguityConsumer {
    calls: Arc<Mutex<Vec<AmbiguityCall>>>,
}

impl crate::ambiguity_consumer::AmbiguityConsumer<String> for RecordingAmbiguityConsumer {
    fn ambiguous(
        &self,
        _parent: &dyn CommandNode<String>,
        child: &dyn CommandNode<String>,
        sibling: &dyn CommandNode<String>,
        inputs: &[String],
    ) {
        self.calls.lock().unwrap().push((
            child.get_name().to_string(),
            sibling.get_name().to_string(),
            inputs.to_vec(),
        ));
    }
}

#[test]
fn test_find_ambiguities() {
    // A word-string argument and a literal with an overlapping example. The word
    // arg's example "word" is a valid input for the literal "word" (and vice
    // versa), so the base node's two children are ambiguous in both directions.
    let mut subject = CommandDispatcher::<String>::new();
    let mut base = literal("base");
    let x = crate::builder::required_argument_builder::RequiredArgumentBuilder::<String, String>::argument(
        "x",
        crate::arguments::string_argument_type::StringArgumentType::word(),
    );
    base.then(x);
    let word = literal("word");
    base.then(word);
    subject.register(&mut base);

    let calls = Arc::new(Mutex::new(Vec::<AmbiguityCall>::new()));
    subject.find_ambiguities(&RecordingAmbiguityConsumer {
        calls: Arc::clone(&calls),
    });

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.contains(&(
        "x".to_string(),
        "word".to_string(),
        vec!["word".to_string()]
    )));
    assert!(calls.contains(&(
        "word".to_string(),
        "x".to_string(),
        vec!["word".to_string()]
    )));
}
