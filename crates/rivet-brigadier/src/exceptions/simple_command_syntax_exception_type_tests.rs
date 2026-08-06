//! Unit tests ported from the upstream brigadier
//! `SimpleCommandSyntaxExceptionTypeTest` (MIT).

use crate::exceptions::{
    CommandExceptionType, CommandSyntaxException, SimpleCommandExceptionType, exception_type_eq,
};
use crate::literal_message::LiteralMessage;
use crate::string_reader::StringReader;

/// Java `mock(CommandExceptionType.class)` — a concrete non-singleton type.
struct MockCommandExceptionType;

impl CommandExceptionType for MockCommandExceptionType {}

impl std::fmt::Debug for MockCommandExceptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockCommandExceptionType").finish()
    }
}

#[test]
fn create_with_context() {
    let type_ = SimpleCommandExceptionType::new(LiteralMessage::new("error"));
    let mut reader = StringReader::new("Foo bar");
    reader.set_cursor(5);
    let exception = type_.create_with_context(&reader);
    assert!(exception_type_eq(
        exception.get_type(),
        &type_ as &dyn CommandExceptionType
    ));
    assert_eq!(exception.get_input(), Some("Foo bar"));
    assert_eq!(exception.get_cursor(), 5);
}

#[test]
fn get_context_none() {
    let type_ = MockCommandExceptionType;
    let exception =
        CommandSyntaxException::new(&type_, std::sync::Arc::new(LiteralMessage::new("error")));
    assert_eq!(exception.get_context(), None);
}

#[test]
fn get_context_short() {
    let type_ = MockCommandExceptionType;
    let exception = CommandSyntaxException::new_with_context(
        &type_,
        std::sync::Arc::new(LiteralMessage::new("error")),
        "Hello world!",
        5,
    );
    assert_eq!(exception.get_context().unwrap(), "Hello<--[HERE]");
}

#[test]
fn get_context_long() {
    let type_ = MockCommandExceptionType;
    let exception = CommandSyntaxException::new_with_context(
        &type_,
        std::sync::Arc::new(LiteralMessage::new("error")),
        "Hello world! This has an error in it. Oh dear!",
        20,
    );
    assert_eq!(exception.get_context().unwrap(), "...d! This ha<--[HERE]");
}
