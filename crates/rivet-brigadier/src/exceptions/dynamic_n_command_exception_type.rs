//! Port of `com.mojang.brigadier.exceptions.DynamicNCommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::Message;
use crate::exceptions::{CommandExceptionType, CommandSyntaxException};

/// Java `DynamicNCommandExceptionType` — a message function taking a variable number
/// of dynamic args.
pub struct DynamicNCommandExceptionType {
    function: Function,
}

/// Java `DynamicNCommandExceptionType.Function`.
type Function = Box<dyn Fn(&[String]) -> Arc<dyn Message> + Send + Sync>;

impl DynamicNCommandExceptionType {
    /// Java `DynamicNCommandExceptionType(Function)`. Args are pre-stringified
    /// (`String.valueOf`) — see `DynamicCommandExceptionType::new`.
    pub fn new(function: impl Fn(&[String]) -> Arc<dyn Message> + Send + Sync + 'static) -> Self {
        DynamicNCommandExceptionType {
            function: Box::new(function),
        }
    }

    /// Java `create(Object a, Object... args)`.
    ///
    /// Upstream quirk: Java ignores the leading `a` and applies the function to
    /// only the varargs. A Java call `create(x, y)` applies just `[y]`; port it as
    /// `create(&[y])`, not `create(&[x, y])`.
    pub fn create(&self, args: &[String]) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, (self.function)(args))
    }

    /// Java `createWithContext(ImmutableStringReader, Object... args)`.
    pub fn create_with_context(
        &self,
        reader: &dyn ImmutableStringReader,
        args: &[String],
    ) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(
            self,
            (self.function)(args),
            reader.get_string(),
            reader.get_cursor(),
        )
    }
}

impl fmt::Debug for DynamicNCommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicNCommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for DynamicNCommandExceptionType {}
