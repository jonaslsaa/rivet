//! Port of `com.mojang.brigadier.exceptions.BuiltInExceptionProvider` (upstream).

use crate::exceptions::{
    Dynamic2CommandExceptionType, DynamicCommandExceptionType, SimpleCommandExceptionType,
};

/// Java `BuiltInExceptionProvider` interface.
pub trait BuiltInExceptionProvider: Send + Sync {
    fn double_too_low(&self) -> &Dynamic2CommandExceptionType;
    fn double_too_high(&self) -> &Dynamic2CommandExceptionType;
    fn float_too_low(&self) -> &Dynamic2CommandExceptionType;
    fn float_too_high(&self) -> &Dynamic2CommandExceptionType;
    fn integer_too_low(&self) -> &Dynamic2CommandExceptionType;
    fn integer_too_high(&self) -> &Dynamic2CommandExceptionType;
    fn long_too_low(&self) -> &Dynamic2CommandExceptionType;
    fn long_too_high(&self) -> &Dynamic2CommandExceptionType;
    fn literal_incorrect(&self) -> &DynamicCommandExceptionType;
    fn reader_expected_start_of_quote(&self) -> &SimpleCommandExceptionType;
    fn reader_expected_end_of_quote(&self) -> &SimpleCommandExceptionType;
    fn reader_invalid_escape(&self) -> &DynamicCommandExceptionType;
    fn reader_invalid_bool(&self) -> &DynamicCommandExceptionType;
    fn reader_invalid_int(&self) -> &DynamicCommandExceptionType;
    fn reader_expected_int(&self) -> &SimpleCommandExceptionType;
    fn reader_invalid_long(&self) -> &DynamicCommandExceptionType;
    fn reader_expected_long(&self) -> &SimpleCommandExceptionType;
    fn reader_invalid_double(&self) -> &DynamicCommandExceptionType;
    fn reader_expected_double(&self) -> &SimpleCommandExceptionType;
    fn reader_invalid_float(&self) -> &DynamicCommandExceptionType;
    fn reader_expected_float(&self) -> &SimpleCommandExceptionType;
    fn reader_expected_bool(&self) -> &SimpleCommandExceptionType;
    fn reader_expected_symbol(&self) -> &DynamicCommandExceptionType;
    fn dispatcher_unknown_command(&self) -> &SimpleCommandExceptionType;
    fn dispatcher_unknown_argument(&self) -> &SimpleCommandExceptionType;
    fn dispatcher_expected_argument_separator(&self) -> &SimpleCommandExceptionType;
    fn dispatcher_parse_exception(&self) -> &DynamicCommandExceptionType;
}
