//! Port of `com.mojang.brigadier.exceptions.DynamicCommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::Message;
use crate::exceptions::{CommandExceptionType, CommandSyntaxException};

/// Java `DynamicCommandExceptionType` — a message function taking one dynamic arg.
pub struct DynamicCommandExceptionType {
    function: Function,
}

/// Java `DynamicCommandExceptionType.Function`.
type Function = Box<dyn Fn(&str) -> Arc<dyn Message> + Send + Sync>;

impl DynamicCommandExceptionType {
    /// Java `DynamicCommandExceptionType(Function<Object, Message>)`. The Java arg is
    /// an `Object` that the message function string-concatenates; the Rust arg is the
    /// already-stringified value (`String.valueOf`), which yields identical messages.
    pub fn new(function: impl Fn(&str) -> Arc<dyn Message> + Send + Sync + 'static) -> Self {
        DynamicCommandExceptionType {
            function: Box::new(function),
        }
    }

    /// Java `create(Object)`.
    pub fn create(&self, arg: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, (self.function)(arg))
    }

    /// Java `createWithContext(ImmutableStringReader, Object)`.
    pub fn create_with_context(
        &self,
        reader: &dyn ImmutableStringReader,
        arg: &str,
    ) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(
            self,
            (self.function)(arg),
            reader.get_string(),
            reader.get_cursor(),
        )
    }
}

impl fmt::Debug for DynamicCommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicCommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for DynamicCommandExceptionType {}
