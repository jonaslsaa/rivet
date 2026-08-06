//! Unit tests ported from the upstream brigadier
//! `DynamicCommandSyntaxExceptionTypeTest` (MIT).

use std::sync::Arc;

use crate::exceptions::{CommandExceptionType, DynamicCommandExceptionType, exception_type_eq};
use crate::literal_message::LiteralMessage;
use crate::string_reader::StringReader;

#[test]
fn create_with_context() {
    let type_ = DynamicCommandExceptionType::new(|name| {
        Arc::new(LiteralMessage::new(format!("Hello, {}!", name)))
    });
    let mut reader = StringReader::new("Foo bar");
    reader.set_cursor(5);
    let exception = type_.create_with_context(&reader, "World");
    assert!(exception_type_eq(
        exception.get_type(),
        &type_ as &dyn CommandExceptionType
    ));
    assert_eq!(exception.get_input(), Some("Foo bar"));
    assert_eq!(exception.get_cursor(), 5);
    assert_eq!(exception.get_raw_message().get_string(), "Hello, World!");
}
