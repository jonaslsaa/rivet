//! Port of `com.mojang.brigadier.exceptions.SimpleCommandExceptionType` (upstream).

use std::fmt;
use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::Message;
use crate::exceptions::{CommandExceptionType, CommandSyntaxException};

/// Java `SimpleCommandExceptionType` — a fixed message, no dynamic args.
pub struct SimpleCommandExceptionType {
    message: Arc<dyn Message>,
}

impl SimpleCommandExceptionType {
    /// Java `SimpleCommandExceptionType(Message)`.
    pub fn new(message: impl Into<Arc<dyn Message>>) -> Self {
        SimpleCommandExceptionType {
            message: message.into(),
        }
    }

    /// Java `create()` — a `CommandSyntaxException` with no input and cursor -1.
    pub fn create(&self) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new(self, Arc::clone(&self.message))
    }

    /// Java `createWithContext(ImmutableStringReader)`.
    pub fn create_with_context(
        &self,
        reader: &dyn ImmutableStringReader,
    ) -> CommandSyntaxException<'_> {
        CommandSyntaxException::new_with_context(
            self,
            Arc::clone(&self.message),
            reader.get_string(),
            reader.get_cursor(),
        )
    }
}

impl fmt::Debug for SimpleCommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimpleCommandExceptionType")
            .finish_non_exhaustive()
    }
}

impl CommandExceptionType for SimpleCommandExceptionType {}

/// Java `toString()` returns `message.getString()`.
impl fmt::Display for SimpleCommandExceptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message.get_string())
    }
}
