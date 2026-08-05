//! Port of `com.mojang.brigadier.exceptions.Dynamic3CommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::exceptions::{CommandExceptionType, CommandSyntaxException};
use crate::ImmutableStringReader;
use crate::Message;

/// Java `Dynamic3CommandExceptionType` — a message function taking three dynamic args.
pub struct Dynamic3CommandExceptionType {
    function: Function,
}

/// Java `Dynamic3CommandExceptionType.Function`.
type Function = Box<dyn Fn(&str, &str, &str) -> Arc<dyn Message> + Send + Sync>;

impl Dynamic3CommandExceptionType {
    /// Java `Dynamic3CommandExceptionType(Function)`. Args are pre-stringified
    /// (`String.valueOf`) — see `DynamicCommandExceptionType::new`.
    pub fn new(function: impl Fn(&str, &str, &str) -> Arc<dyn Message> + Send + Sync + 'static) -> Self {
        Dynamic3CommandExceptionType {
            function: Box::new(function),
        }
    }

    /// Java `create(Object a, Object b, Object c)`.
    pub fn create(&self, a: &str, b: &str, c: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, (self.function)(a, b, c))
    }

    /// Java `createWithContext(ImmutableStringReader, Object a, Object b, Object c)`.
    pub fn create_with_context(&self, reader: &dyn ImmutableStringReader, a: &str, b: &str, c: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(self, (self.function)(a, b, c), reader.get_string(), reader.get_cursor())
    }
}

impl fmt::Debug for Dynamic3CommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dynamic3CommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for Dynamic3CommandExceptionType {}
