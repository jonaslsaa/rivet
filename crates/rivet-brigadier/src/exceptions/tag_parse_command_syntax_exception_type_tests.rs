//! Tests for Paper's `TagParseCommandSyntaxException` (#210). Paper's class is a
//! `CommandSyntaxException` stamped with a fixed private type and an SNBT-parse
//! message; the `instanceof` detection used by `CommandDispatcher.parseNodes`
//! becomes type-identity with the private `EXCEPTION_TYPE`.

use crate::exceptions::tag_parse_command_syntax_exception::{
    EXCEPTION_TYPE, is_tag_parse_exception, tag_parse_exception,
};
use crate::exceptions::{BuiltInExceptionProvider, CommandSyntaxException, exception_type_eq};

#[test]
fn tag_parse_exception_carries_fixed_type_and_message() {
    let ex = tag_parse_exception("Unknown tag type");
    // Java: the private static final EXCEPTION_TYPE.
    assert!(exception_type_eq(ex.get_type(), &*EXCEPTION_TYPE));
    // Java: Component.literal(message) -> getString() is the SNBT parse error.
    assert_eq!(ex.get_message(), "Unknown tag type");
    assert_eq!(ex.get_raw_message().get_string(), "Unknown tag type");
    // No input/cursor — created via the `(CommandExceptionType, Message)` ctor.
    assert_eq!(ex.get_input(), None);
    assert_eq!(ex.get_cursor(), -1);
}

#[test]
fn tag_parse_exception_is_tag_parse_exception() {
    let ex = tag_parse_exception("boom");
    assert!(is_tag_parse_exception(&ex));
}

#[test]
fn built_in_exceptions_are_not_tag_parse() {
    let reader = crate::string_reader::StringReader::new("foo");
    let ex = CommandSyntaxException::built_in_exceptions()
        .dispatcher_unknown_command()
        .create_with_context(&reader);
    assert!(!is_tag_parse_exception(&ex));
}
