//! Port of Paper's `io.papermc.paper.brigadier.TagParseCommandSyntaxException`.
//!
//! Java's class extends `CommandSyntaxException`, constructed with a fixed
//! `SimpleCommandExceptionType` (message "Error parsing NBT") and a per-instance
//! message carrying the SNBT parse error. The Rust `CommandSyntaxException` is a
//! value type holding `(exception_type, message, input, cursor)`, so the subclass
//! becomes a constructor that stamps the fixed type onto the value plus a type-
//! identity check.
//!
//! `CommandDispatcher.parseNodes` (#210) catches this exception and aborts dispatch
//! ("Handle non-recoverable exceptions") instead of falling through to the next
//! child. Java detects it with `instanceof TagParseCommandSyntaxException`; here the
//! equivalent is type identity with the private `EXCEPTION_TYPE`, which is faithful
//! because the type is `private static final` and used by no other class.
//!
//! The Java message is `Component.literal(message)`; a `LiteralMessage` carries the
//! same string through `Message.getString()`, so the observable `getMessage()` /
//! `getRawMessage()` surface is identical.

use std::sync::Arc;
use std::sync::LazyLock;

use crate::exceptions::CommandSyntaxException;
use crate::exceptions::exception_type_eq;
use crate::exceptions::simple_command_exception_type::SimpleCommandExceptionType;
use crate::literal_message::LiteralMessage;

/// Java `TagParseCommandSyntaxException.EXCEPTION_TYPE`.
pub static EXCEPTION_TYPE: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Error parsing NBT")));

/// Java `new TagParseCommandSyntaxException(String message)` — a
/// `CommandSyntaxException` stamped with the tag-parse type and the SNBT parse
/// error as its message. The NBT argument parser (a Minecraft type, ported with
/// the command-dispatch units) throws this on a tag-argument parse failure.
pub fn tag_parse_exception(message: impl Into<String>) -> CommandSyntaxException<'static> {
    CommandSyntaxException::new(
        &*EXCEPTION_TYPE,
        Arc::new(LiteralMessage::new(message.into())),
    )
}

/// Java `e instanceof TagParseCommandSyntaxException` — exception-type identity
/// with the private `EXCEPTION_TYPE`.
pub fn is_tag_parse_exception(ex: &CommandSyntaxException<'_>) -> bool {
    exception_type_eq(ex.get_type(), &*EXCEPTION_TYPE)
}
