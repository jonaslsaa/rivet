//! Port of `com.mojang.brigadier.exceptions.Dynamic2CommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::exceptions::{CommandExceptionType, CommandSyntaxException};
use crate::ImmutableStringReader;
use crate::Message;

/// Java `Dynamic2CommandExceptionType` — a message function taking two dynamic args.
pub struct Dynamic2CommandExceptionType {
    function: Function,
}

/// Java `Dynamic2CommandExceptionType.Function`.
type Function = Box<dyn Fn(&str, &str) -> Arc<dyn Message> + Send + Sync>;

impl Dynamic2CommandExceptionType {
    /// Java `Dynamic2CommandExceptionType(Function)`. Args are pre-stringified
    /// (`String.valueOf`) — see `DynamicCommandExceptionType::new`.
    pub fn new(function: impl Fn(&str, &str) -> Arc<dyn Message> + Send + Sync + 'static) -> Self {
        Dynamic2CommandExceptionType {
            function: Box::new(function),
        }
    }

    /// Java `create(Object a, Object b)`.
    pub fn create(&self, a: &str, b: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, (self.function)(a, b))
    }

    /// Java `createWithContext(ImmutableStringReader, Object a, Object b)`.
    pub fn create_with_context(&self, reader: &dyn ImmutableStringReader, a: &str, b: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(self, (self.function)(a, b), reader.get_string(), reader.get_cursor())
    }
}

impl fmt::Debug for Dynamic2CommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dynamic2CommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for Dynamic2CommandExceptionType {}
