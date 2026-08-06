//! Unit tests ported from the upstream brigadier `ContextChainTest` (MIT).

use std::sync::Arc;

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::command::Command;
use crate::command_dispatcher::CommandDispatcher;
use crate::context::ContextChain;
use crate::exceptions::CommandSyntaxException;
use crate::result_consumer::ResultConsumer;
use crate::single_redirect_modifier::SingleRedirectModifier;

/// A `Command` recording the source it saw (Java mock verify).
struct SourceRecordingCommand<C> {
    seen: Arc<std::sync::Mutex<Vec<C>>>,
    result: i32,
}

impl<C: Clone + Send + Sync + 'static> Command<C> for SourceRecordingCommand<C> {
    fn run(
        &self,
        context: &crate::context::CommandContext<C>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        self.seen.lock().unwrap().push(context.get_source().clone());
        Ok(self.result)
    }
}

/// A `ResultConsumer` recording `(source, success, result)` triples (Java mock).
struct RecordingConsumer<C> {
    calls: Arc<std::sync::Mutex<Vec<(C, bool, i32)>>>,
}

impl<C: Clone + Send + Sync + 'static> ResultConsumer<C> for RecordingConsumer<C> {
    fn on_command_complete(
        &self,
        context: &crate::context::CommandContext<C>,
        success: bool,
        result: i32,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push((context.get_source().clone(), success, result));
    }
}

/// A `SingleRedirectModifier` returning a fixed source.
struct FixedSingleRedirectModifier<C> {
    source: C,
}

impl<C: Clone + Send + Sync + 'static> SingleRedirectModifier<C>
    for FixedSingleRedirectModifier<C>
{
    fn apply(
        &self,
        _context: &crate::context::CommandContext<C>,
    ) -> Result<C, CommandSyntaxException<'static>> {
        Ok(self.source.clone())
    }
}

fn literal<C: 'static>(name: &str) -> LiteralArgumentBuilder<C> {
    LiteralArgumentBuilder::literal(name)
}

#[test]
fn test_execute_all_for_single_command() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));
    let command: Arc<dyn Command<&str>> = Arc::new(SourceRecordingCommand {
        seen: Arc::clone(&seen),
        result: 4,
    });
    let consumer_calls = Arc::new(std::sync::Mutex::new(Vec::<(_, bool, i32)>::new()));
    let consumer: Arc<dyn ResultConsumer<&str>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });

    let mut dispatcher = CommandDispatcher::<&str>::new();
    let mut cmd = literal("foo");
    cmd.executes(Some(Arc::clone(&command)));
    dispatcher.register(&mut cmd);

    let result = dispatcher.parse_string("foo", "compile_source");
    let top_context = result.get_context().build("foo".to_string());
    let chain = ContextChain::try_flatten(&top_context).expect("flattenable");

    let returned = chain
        .execute_all("runtime_source", consumer.as_ref())
        .unwrap();
    assert_eq!(returned, 4);
    assert_eq!(*seen.lock().unwrap(), vec!["runtime_source"]);
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![("runtime_source", true, 4)]
    );
}

#[test]
fn test_execute_all_for_redirected_command() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));
    let command: Arc<dyn Command<&str>> = Arc::new(SourceRecordingCommand {
        seen: Arc::clone(&seen),
        result: 4,
    });
    let consumer_calls = Arc::new(std::sync::Mutex::new(Vec::<(_, bool, i32)>::new()));
    let consumer: Arc<dyn ResultConsumer<&str>> = Arc::new(RecordingConsumer {
        calls: Arc::clone(&consumer_calls),
    });

    let mut dispatcher = CommandDispatcher::<&str>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::clone(&command)));
    dispatcher.register(&mut foo);

    let root = dispatcher.get_root().clone() as Arc<dyn crate::tree::CommandNode<&str>>;
    let modifier: Arc<dyn SingleRedirectModifier<&str>> = Arc::new(FixedSingleRedirectModifier {
        source: "redirected_source",
    });
    let mut bar = literal("bar");
    bar.redirect_with_modifier(root, Some(modifier));
    dispatcher.register(&mut bar);

    let result = dispatcher.parse_string("bar foo", "compile_source");
    let top_context = result.get_context().build("bar foo".to_string());
    let chain = ContextChain::try_flatten(&top_context).expect("flattenable");

    let returned = chain
        .execute_all("runtime_source", consumer.as_ref())
        .unwrap();
    assert_eq!(returned, 4);
    assert_eq!(*seen.lock().unwrap(), vec!["redirected_source"]);
    assert_eq!(
        *consumer_calls.lock().unwrap(),
        vec![("redirected_source", true, 4)]
    );
}

#[test]
fn test_single_stage_execution() {
    let mut dispatcher = CommandDispatcher::<i32>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(IdentitySourceCommand)));
    dispatcher.register(&mut foo);

    let result = dispatcher.parse_string("foo", 1);
    let top_context = result.get_context().build("foo".to_string());
    let chain = ContextChain::try_flatten(&top_context).expect("flattenable");

    assert_eq!(chain.get_stage(), crate::context::Stage::Execute);
    // get_top_context returns the executable for a single stage.
    assert!(std::ptr::eq(chain.get_top_context(), &top_context));
    assert!(chain.next_stage().is_none());
}

#[test]
fn test_multi_stage_execution() {
    let mut dispatcher = CommandDispatcher::<i32>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(IdentitySourceCommand)));
    dispatcher.register(&mut foo);

    let mut bar = literal("bar");
    let root = dispatcher.get_root().clone() as Arc<dyn crate::tree::CommandNode<i32>>;
    bar.redirect(root);
    dispatcher.register(&mut bar);

    let result = dispatcher.parse_string("bar bar foo", 1);
    let top_context = result.get_context().build("bar bar foo".to_string());
    let stage0 = ContextChain::try_flatten(&top_context).expect("flattenable");

    assert_eq!(stage0.get_stage(), crate::context::Stage::Modify);
    assert!(std::ptr::eq(stage0.get_top_context(), &top_context));

    let stage1 = stage0.next_stage().expect("stage 1");
    assert_eq!(stage1.get_stage(), crate::context::Stage::Modify);
    assert!(std::ptr::eq(
        stage1.get_top_context(),
        top_context.get_child().unwrap()
    ));

    let stage2 = stage1.next_stage().expect("stage 2");
    assert_eq!(stage2.get_stage(), crate::context::Stage::Execute);
    assert!(std::ptr::eq(
        stage2.get_top_context(),
        top_context.get_child().unwrap().get_child().unwrap()
    ));

    assert!(stage2.next_stage().is_none());
}

#[test]
fn test_missing_execute() {
    let mut dispatcher = CommandDispatcher::<i32>::new();
    let mut foo = literal("foo");
    foo.executes(Some(Arc::new(IdentitySourceCommand)));
    dispatcher.register(&mut foo);

    let mut bar = literal("bar");
    let root = dispatcher.get_root().clone() as Arc<dyn crate::tree::CommandNode<i32>>;
    bar.redirect(root);
    dispatcher.register(&mut bar);

    let result = dispatcher.parse_string("bar bar", 1);
    let top_context = result.get_context().build("bar bar".to_string());
    assert!(ContextChain::try_flatten(&top_context).is_none());
}

/// `CommandContext::getSource` — runs to the current source value.
struct IdentitySourceCommand;

impl Command<i32> for IdentitySourceCommand {
    fn run(
        &self,
        context: &crate::context::CommandContext<i32>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(*context.get_source())
    }
}
