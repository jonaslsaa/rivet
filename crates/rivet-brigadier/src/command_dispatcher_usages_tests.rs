//! Unit tests ported from the upstream brigadier `CommandDispatcherUsagesTest` (MIT).

use crate::builder::argument_builder::ArgumentBuilderBehavior;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::command::Command;
use crate::command_dispatcher::CommandDispatcher;
use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::tree::{CommandNode, RootCommandNode};

/// A no-op `Command` (Java `@Mock Command`).
struct NoopCommand;

impl Command<String> for NoopCommand {
    fn run(
        &self,
        _context: &CommandContext<String>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(0)
    }
}

fn literal(name: &str) -> LiteralArgumentBuilder<String> {
    LiteralArgumentBuilder::literal(name)
}

fn command() -> std::sync::Arc<dyn Command<String>> {
    std::sync::Arc::new(NoopCommand)
}

fn noop_require() -> std::sync::Arc<dyn Fn(&String) -> bool + Send + Sync> {
    std::sync::Arc::new(|_: &String| false)
}

/// Build the upstream `setUp()` command tree.
fn build_subject() -> CommandDispatcher<String> {
    let mut subject = CommandDispatcher::<String>::new();

    // a: (1 (i|ii)) | (2 (i|ii))
    let mut a = literal("a");
    let mut a1 = literal("1");
    let mut a1i = literal("i");
    a1i.executes(Some(command()));
    a1.then(a1i);
    let mut a1ii = literal("ii");
    a1ii.executes(Some(command()));
    a1.then(a1ii);
    a.then(a1);
    let mut a2 = literal("2");
    let mut a2i = literal("i");
    a2i.executes(Some(command()));
    a2.then(a2i);
    let mut a2ii = literal("ii");
    a2ii.executes(Some(command()));
    a2.then(a2ii);
    a.then(a2);
    subject.register(&mut a);

    let mut b = literal("b");
    let mut b1 = literal("1");
    b1.executes(Some(command()));
    b.then(b1);
    subject.register(&mut b);

    let mut c = literal("c");
    c.executes(Some(command()));
    subject.register(&mut c);

    let mut d = literal("d");
    d.requires(noop_require());
    d.executes(Some(command()));
    subject.register(&mut d);

    // e: executes + (1 (i|ii))
    let mut e = literal("e");
    e.executes(Some(command()));
    let mut e1 = literal("1");
    e1.executes(Some(command()));
    let mut e1i = literal("i");
    e1i.executes(Some(command()));
    e1.then(e1i);
    let mut e1ii = literal("ii");
    e1ii.executes(Some(command()));
    e1.then(e1ii);
    e.then(e1);
    subject.register(&mut e);

    // f: (1 (i | ii-with-false-requirement)) | (2 (i-with-false-requirement | ii))
    let mut f = literal("f");
    let mut f1 = literal("1");
    let mut f1i = literal("i");
    f1i.executes(Some(command()));
    f1.then(f1i);
    let mut f1ii = literal("ii");
    f1ii.requires(noop_require());
    f1ii.executes(Some(command()));
    f1.then(f1ii);
    f.then(f1);
    let mut f2 = literal("2");
    let mut f2i = literal("i");
    f2i.requires(noop_require());
    f2i.executes(Some(command()));
    f2.then(f2i);
    let mut f2ii = literal("ii");
    f2ii.executes(Some(command()));
    f2.then(f2ii);
    f.then(f2);
    subject.register(&mut f);

    let mut g = literal("g");
    g.executes(Some(command()));
    let mut g1 = literal("1");
    let mut g1i = literal("i");
    g1i.executes(Some(command()));
    g1.then(g1i);
    g.then(g1);
    subject.register(&mut g);

    let mut h = literal("h");
    h.executes(Some(command()));
    let mut h1 = literal("1");
    let mut h1i = literal("i");
    h1i.executes(Some(command()));
    h1.then(h1i);
    h.then(h1);
    let mut h2 = literal("2");
    let mut h2i = literal("i");
    let mut h2ii = literal("ii");
    h2ii.executes(Some(command()));
    h2i.then(h2ii);
    h2.then(h2i);
    h.then(h2);
    let mut h3 = literal("3");
    h3.executes(Some(command()));
    h.then(h3);
    subject.register(&mut h);

    let mut i = literal("i");
    i.executes(Some(command()));
    let mut i1 = literal("1");
    i1.executes(Some(command()));
    i.then(i1);
    let mut i2 = literal("2");
    i2.executes(Some(command()));
    i.then(i2);
    subject.register(&mut i);

    let mut j = literal("j");
    let j_root = subject.get_root().clone() as std::sync::Arc<dyn CommandNode<String>>;
    j.redirect(j_root);
    subject.register(&mut j);

    let mut k = literal("k");
    let h_node = get(&subject, "h");
    k.redirect(h_node);
    subject.register(&mut k);

    subject
}

/// `get(command)` — the last node of parsing `command`.
fn get(
    subject: &CommandDispatcher<String>,
    command: &str,
) -> std::sync::Arc<dyn CommandNode<String>> {
    let parse = subject.parse_string(command, "source".to_string());
    parse.get_context().get_nodes().last().unwrap().get_node()
}

#[test]
fn test_all_usage_no_commands() {
    let subject = CommandDispatcher::<String>::new();
    let root: &dyn CommandNode<String> = &**subject.get_root();
    let results = subject.get_all_usage(root, &"source".to_string(), true);
    assert!(results.is_empty());
}

#[test]
fn test_smart_usage_no_commands() {
    let subject = CommandDispatcher::<String>::new();
    let root: &dyn CommandNode<String> = &**subject.get_root();
    let results = subject.get_smart_usage(root, &"source".to_string());
    assert!(results.is_empty());
}

#[test]
fn test_all_usage_root() {
    let subject = build_subject();
    let root: &dyn CommandNode<String> = &**subject.get_root();
    let results = subject.get_all_usage(root, &"source".to_string(), true);
    assert_eq!(
        results,
        [
            "a 1 i", "a 1 ii", "a 2 i", "a 2 ii", "b 1", "c", "e", "e 1", "e 1 i", "e 1 ii",
            "f 1 i", "f 2 ii", "g", "g 1 i", "h", "h 1 i", "h 2 i ii", "h 3", "i", "i 1", "i 2",
            "j ...", "k -> h",
        ]
    );
}

#[test]
fn test_smart_usage_root() {
    let subject = build_subject();
    let root: &dyn CommandNode<String> = &**subject.get_root();
    let results = subject.get_smart_usage(root, &"source".to_string());
    let usage: Vec<(String, String)> = results
        .iter()
        .map(|(node, usage)| (node.get_name().to_string(), usage.clone()))
        .collect();
    assert_eq!(
        usage,
        [
            ("a".to_string(), "a (1|2)".to_string()),
            ("b".to_string(), "b 1".to_string()),
            ("c".to_string(), "c".to_string()),
            ("e".to_string(), "e [1]".to_string()),
            ("f".to_string(), "f (1|2)".to_string()),
            ("g".to_string(), "g [1]".to_string()),
            ("h".to_string(), "h [1|2|3]".to_string()),
            ("i".to_string(), "i [1|2]".to_string()),
            ("j".to_string(), "j ...".to_string()),
            ("k".to_string(), "k -> h".to_string()),
        ]
    );
}

#[test]
fn test_smart_usage_h() {
    let subject = build_subject();
    let h = get(&subject, "h");
    let results = subject.get_smart_usage(&*h, &"source".to_string());
    let usage: Vec<(String, String)> = results
        .iter()
        .map(|(node, usage)| (node.get_name().to_string(), usage.clone()))
        .collect();
    assert_eq!(
        usage,
        [
            ("1".to_string(), "[1] i".to_string()),
            ("2".to_string(), "[2] i ii".to_string()),
            ("3".to_string(), "[3]".to_string()),
        ]
    );
}

#[test]
fn test_smart_usage_offset_h() {
    let subject = build_subject();
    let mut offset_h = crate::string_reader::StringReader::new("/|/|/h");
    offset_h.set_cursor(5);
    let parse = subject.parse(offset_h, "source".to_string());
    let h = parse.get_context().get_nodes().last().unwrap().get_node();
    let results = subject.get_smart_usage(&*h, &"source".to_string());
    let usage: Vec<(String, String)> = results
        .iter()
        .map(|(node, usage)| (node.get_name().to_string(), usage.clone()))
        .collect();
    assert_eq!(
        usage,
        [
            ("1".to_string(), "[1] i".to_string()),
            ("2".to_string(), "[2] i ii".to_string()),
            ("3".to_string(), "[3]".to_string()),
        ]
    );
}

// Keep the RootCommandNode import used (parity with upstream's `new RootCommandNode<>()`).
#[allow(dead_code)]
fn _unused(_: &RootCommandNode<String>) {}
