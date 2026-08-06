//! Port of `com.mojang.brigadier.exceptions.BuiltInExceptions` (upstream).

use std::sync::Arc;

use crate::LiteralMessage;
use crate::exceptions::{
    BuiltInExceptionProvider, Dynamic2CommandExceptionType, DynamicCommandExceptionType,
    SimpleCommandExceptionType,
};

/// Java `BuiltInExceptions` — the singleton `CommandSyntaxException.BUILT_IN_EXCEPTIONS`.
///
/// Each exception type is a distinct instance in Java; here the type objects are
/// initialized by `LazyLock` so the per-object identity that downstream code
/// relies on (e.g. `ex.getType() is type`) is a single shared instance, matching
/// Java. The exception types implement no `PartialEq`; identity is compared by
/// data pointer with `exception_type_eq` (see the note on vtable dedup there).
pub struct BuiltInExceptions {
    double_too_small: Dynamic2CommandExceptionType,
    double_too_big: Dynamic2CommandExceptionType,

    float_too_small: Dynamic2CommandExceptionType,
    float_too_big: Dynamic2CommandExceptionType,

    integer_too_small: Dynamic2CommandExceptionType,
    integer_too_big: Dynamic2CommandExceptionType,

    long_too_small: Dynamic2CommandExceptionType,
    long_too_big: Dynamic2CommandExceptionType,

    literal_incorrect: DynamicCommandExceptionType,

    reader_expected_start_of_quote: SimpleCommandExceptionType,
    reader_expected_end_of_quote: SimpleCommandExceptionType,
    reader_invalid_escape: DynamicCommandExceptionType,
    reader_invalid_bool: DynamicCommandExceptionType,
    reader_invalid_int: DynamicCommandExceptionType,
    reader_expected_int: SimpleCommandExceptionType,
    reader_invalid_long: DynamicCommandExceptionType,
    reader_expected_long: SimpleCommandExceptionType,
    reader_invalid_double: DynamicCommandExceptionType,
    reader_expected_double: SimpleCommandExceptionType,
    reader_invalid_float: DynamicCommandExceptionType,
    reader_expected_float: SimpleCommandExceptionType,
    reader_expected_bool: SimpleCommandExceptionType,
    reader_expected_symbol: DynamicCommandExceptionType,

    dispatcher_unknown_command: SimpleCommandExceptionType,
    dispatcher_unknown_argument: SimpleCommandExceptionType,
    dispatcher_expected_argument_separator: SimpleCommandExceptionType,
    dispatcher_parse_exception: DynamicCommandExceptionType,
}

impl BuiltInExceptions {
    /// Java's `BuiltInExceptions()` constructor — creates the static fields.
    pub fn new() -> Self {
        BuiltInExceptions {
            double_too_small: Dynamic2CommandExceptionType::new(|found, min| {
                Arc::new(LiteralMessage::new(format!(
                    "Double must not be less than {}, found {}",
                    min, found
                )))
            }),
            double_too_big: Dynamic2CommandExceptionType::new(|found, max| {
                Arc::new(LiteralMessage::new(format!(
                    "Double must not be more than {}, found {}",
                    max, found
                )))
            }),

            float_too_small: Dynamic2CommandExceptionType::new(|found, min| {
                Arc::new(LiteralMessage::new(format!(
                    "Float must not be less than {}, found {}",
                    min, found
                )))
            }),
            float_too_big: Dynamic2CommandExceptionType::new(|found, max| {
                Arc::new(LiteralMessage::new(format!(
                    "Float must not be more than {}, found {}",
                    max, found
                )))
            }),

            integer_too_small: Dynamic2CommandExceptionType::new(|found, min| {
                Arc::new(LiteralMessage::new(format!(
                    "Integer must not be less than {}, found {}",
                    min, found
                )))
            }),
            integer_too_big: Dynamic2CommandExceptionType::new(|found, max| {
                Arc::new(LiteralMessage::new(format!(
                    "Integer must not be more than {}, found {}",
                    max, found
                )))
            }),

            long_too_small: Dynamic2CommandExceptionType::new(|found, min| {
                Arc::new(LiteralMessage::new(format!(
                    "Long must not be less than {}, found {}",
                    min, found
                )))
            }),
            long_too_big: Dynamic2CommandExceptionType::new(|found, max| {
                Arc::new(LiteralMessage::new(format!(
                    "Long must not be more than {}, found {}",
                    max, found
                )))
            }),

            literal_incorrect: DynamicCommandExceptionType::new(|expected| {
                Arc::new(LiteralMessage::new(format!(
                    "Expected literal {}",
                    expected
                )))
            }),

            reader_expected_start_of_quote: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected quote to start a string",
            )),
            reader_expected_end_of_quote: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Unclosed quoted string",
            )),
            reader_invalid_escape: DynamicCommandExceptionType::new(|character| {
                Arc::new(LiteralMessage::new(format!(
                    "Invalid escape sequence '{}' in quoted string",
                    character
                )))
            }),
            reader_invalid_bool: DynamicCommandExceptionType::new(|value| {
                Arc::new(LiteralMessage::new(format!(
                    "Invalid bool, expected true or false but found '{}'",
                    value
                )))
            }),
            reader_invalid_int: DynamicCommandExceptionType::new(|value| {
                Arc::new(LiteralMessage::new(format!("Invalid integer '{}'", value)))
            }),
            reader_expected_int: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected integer",
            )),
            reader_invalid_long: DynamicCommandExceptionType::new(|value| {
                Arc::new(LiteralMessage::new(format!("Invalid long '{}'", value)))
            }),
            reader_expected_long: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected long",
            )),
            reader_invalid_double: DynamicCommandExceptionType::new(|value| {
                Arc::new(LiteralMessage::new(format!("Invalid double '{}'", value)))
            }),
            reader_expected_double: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected double",
            )),
            reader_invalid_float: DynamicCommandExceptionType::new(|value| {
                Arc::new(LiteralMessage::new(format!("Invalid float '{}'", value)))
            }),
            reader_expected_float: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected float",
            )),
            reader_expected_bool: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Expected bool",
            )),
            reader_expected_symbol: DynamicCommandExceptionType::new(|symbol| {
                Arc::new(LiteralMessage::new(format!("Expected '{}'", symbol)))
            }),

            dispatcher_unknown_command: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Unknown command",
            )),
            dispatcher_unknown_argument: SimpleCommandExceptionType::new(LiteralMessage::new(
                "Incorrect argument for command",
            )),
            dispatcher_expected_argument_separator: SimpleCommandExceptionType::new(
                LiteralMessage::new(
                    "Expected whitespace to end one argument, but found trailing data",
                ),
            ),
            dispatcher_parse_exception: DynamicCommandExceptionType::new(|message| {
                Arc::new(LiteralMessage::new(format!(
                    "Could not parse command: {}",
                    message
                )))
            }),
        }
    }
}

/// Java's no-arg `BuiltInExceptions()` constructor.
impl Default for BuiltInExceptions {
    fn default() -> Self {
        Self::new()
    }
}

impl BuiltInExceptionProvider for BuiltInExceptions {
    fn double_too_low(&self) -> &Dynamic2CommandExceptionType {
        &self.double_too_small
    }

    fn double_too_high(&self) -> &Dynamic2CommandExceptionType {
        &self.double_too_big
    }

    fn float_too_low(&self) -> &Dynamic2CommandExceptionType {
        &self.float_too_small
    }

    fn float_too_high(&self) -> &Dynamic2CommandExceptionType {
        &self.float_too_big
    }

    fn integer_too_low(&self) -> &Dynamic2CommandExceptionType {
        &self.integer_too_small
    }

    fn integer_too_high(&self) -> &Dynamic2CommandExceptionType {
        &self.integer_too_big
    }

    fn long_too_low(&self) -> &Dynamic2CommandExceptionType {
        &self.long_too_small
    }

    fn long_too_high(&self) -> &Dynamic2CommandExceptionType {
        &self.long_too_big
    }

    fn literal_incorrect(&self) -> &DynamicCommandExceptionType {
        &self.literal_incorrect
    }

    fn reader_expected_start_of_quote(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_start_of_quote
    }

    fn reader_expected_end_of_quote(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_end_of_quote
    }

    fn reader_invalid_escape(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_escape
    }

    fn reader_invalid_bool(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_bool
    }

    fn reader_invalid_int(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_int
    }

    fn reader_expected_int(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_int
    }

    fn reader_invalid_long(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_long
    }

    fn reader_expected_long(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_long
    }

    fn reader_invalid_double(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_double
    }

    fn reader_expected_double(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_double
    }

    fn reader_invalid_float(&self) -> &DynamicCommandExceptionType {
        &self.reader_invalid_float
    }

    fn reader_expected_float(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_float
    }

    fn reader_expected_bool(&self) -> &SimpleCommandExceptionType {
        &self.reader_expected_bool
    }

    fn reader_expected_symbol(&self) -> &DynamicCommandExceptionType {
        &self.reader_expected_symbol
    }

    fn dispatcher_unknown_command(&self) -> &SimpleCommandExceptionType {
        &self.dispatcher_unknown_command
    }

    fn dispatcher_unknown_argument(&self) -> &SimpleCommandExceptionType {
        &self.dispatcher_unknown_argument
    }

    fn dispatcher_expected_argument_separator(&self) -> &SimpleCommandExceptionType {
        &self.dispatcher_expected_argument_separator
    }

    fn dispatcher_parse_exception(&self) -> &DynamicCommandExceptionType {
        &self.dispatcher_parse_exception
    }
}
