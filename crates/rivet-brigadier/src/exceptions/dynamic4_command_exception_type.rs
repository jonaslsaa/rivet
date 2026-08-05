//! Port of `com.mojang.brigadier.exceptions.Dynamic4CommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::exceptions::{CommandExceptionType, CommandSyntaxException};
use crate::ImmutableStringReader;
use crate::Message;

/// Java `Dynamic4CommandExceptionType` — a message function taking four dynamic args.
pub struct Dynamic4CommandExceptionType {
    function: Function,
}

/// Java `Dynamic4CommandExceptionType.Function`.
type Function = Box<dyn Fn(&str, &str, &str, &str) -> Arc<dyn Message> + Send + Sync>;

impl Dynamic4CommandExceptionType {
    /// Java `Dynamic4CommandExceptionType(Function)`. Args are pre-stringified
    /// (`String.valueOf`) — see `DynamicCommandExceptionType::new`.
    pub fn new(function: impl Fn(&str, &str, &str, &str) -> Arc<dyn Message> + Send + Sync + 'static) -> Self {
        Dynamic4CommandExceptionType {
            function: Box::new(function),
        }
    }

    /// Java `create(Object a, Object b, Object c, Object d)`.
    pub fn create(&self, a: &str, b: &str, c: &str, d: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, (self.function)(a, b, c, d))
    }

    /// Java `createWithContext(ImmutableStringReader, Object a, Object b, Object c, Object d)`.
    pub fn create_with_context(&self, reader: &dyn ImmutableStringReader, a: &str, b: &str, c: &str, d: &str) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(self, (self.function)(a, b, c, d), reader.get_string(), reader.get_cursor())
    }
}

impl fmt::Debug for Dynamic4CommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dynamic4CommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for Dynamic4CommandExceptionType {}
