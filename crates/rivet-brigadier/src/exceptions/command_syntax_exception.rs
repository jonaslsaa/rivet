//! Port of the Paper-patched `com.mojang.brigadier.exceptions.CommandSyntaxException`.
//!
//! Java's class extends `Exception` and carries `(type, message, input, cursor)`.
//! The exception type and the message are shared references in Java: the type
//! object lives in the exception type instance, and `create()`/`createWithContext()`
//! hand the same `Message` reference to every produced exception. Here the type is
//! borrowed (`&'a dyn CommandExceptionType`, preserving Java's reference identity)
//! and the message is `Arc<dyn Message>` so repeated `create()` calls from one
//! exception type share the same message object.
//!
//! The Java `getMessage()` post-processes `message.getString()` by appending the
//! `" at position N: ..."` context; that is reproduced by `get_message()` and by
//! `Display`.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock};

use crate::Message;
use crate::exceptions::{BuiltInExceptions, CommandExceptionType};
use crate::immutable_string_reader::utf16_units_to_string;

/// Java `CommandSyntaxException.CONTEXT_AMOUNT`.
pub const CONTEXT_AMOUNT: i32 = 10;

/// Java `CommandSyntaxException.ENABLE_COMMAND_STACK_TRACES`. Java reads it when
/// constructing the exception to decide whether to retain a Java stack trace; Rust
/// has no Java stack traces, so the flag is inert, kept for API parity — setting
/// or clearing it has no observable effect here.
pub static ENABLE_COMMAND_STACK_TRACES: AtomicBool = AtomicBool::new(true);

/// Java `CommandSyntaxException.BUILT_IN_EXCEPTIONS`.
pub static BUILT_IN_EXCEPTIONS: LazyLock<BuiltInExceptions> = LazyLock::new(BuiltInExceptions::new);

/// Java `CommandSyntaxException`.
pub struct CommandSyntaxException<'a> {
    exception_type: &'a dyn CommandExceptionType,
    message: Arc<dyn Message>,
    input: Option<String>,
    cursor: i32,
}

impl CommandSyntaxException<'_> {
    /// Java `CommandSyntaxException(CommandExceptionType, Message)` — no input, cursor -1.
    pub fn new(
        exception_type: &dyn CommandExceptionType,
        message: Arc<dyn Message>,
    ) -> CommandSyntaxException<'_> {
        CommandSyntaxException {
            exception_type,
            message,
            input: None,
            cursor: -1,
        }
    }

    /// Java `CommandSyntaxException(CommandExceptionType, Message, String input, int cursor)`.
    pub fn new_with_context<'a>(
        exception_type: &'a dyn CommandExceptionType,
        message: Arc<dyn Message>,
        input: &str,
        cursor: i32,
    ) -> CommandSyntaxException<'a> {
        CommandSyntaxException {
            exception_type,
            message,
            input: Some(input.to_string()),
            cursor,
        }
    }

    /// Java `CommandSyntaxException.getType()`.
    pub fn get_type(&self) -> &dyn CommandExceptionType {
        self.exception_type
    }

    /// Java `CommandSyntaxException.getMessage()` — the raw message plus the
    /// `" at position N: <context>"` suffix when a context exists.
    pub fn get_message(&self) -> String {
        let mut message = self.message.get_string().to_string();
        if let Some(context) = self.get_context() {
            message.push_str(&format!(" at position {}: {}", self.cursor, context));
        }
        message
    }

    /// Java `CommandSyntaxException.getRawMessage()`.
    pub fn get_raw_message(&self) -> &dyn Message {
        self.message.as_ref()
    }

    /// Java `CommandSyntaxException.getContext()` — `null` when there is no input or
    /// the cursor is negative. Indices are UTF-16 code units. When the cursor splits
    /// a surrogate pair, Java's substring returns the raw lone surrogate while
    /// `from_utf16_lossy` renders U+FFFD (inherent to Rust's String model).
    pub fn get_context(&self) -> Option<String> {
        let input = self.input.as_ref()?;
        if self.cursor < 0 {
            return None;
        }
        let units: Vec<u16> = input.encode_utf16().collect();
        let cursor = std::cmp::min(units.len() as i32, self.cursor);
        let mut builder = String::new();
        if cursor > CONTEXT_AMOUNT {
            builder.push_str("...");
        }
        let start = std::cmp::max(0, cursor - CONTEXT_AMOUNT) as usize;
        builder.push_str(&utf16_units_to_string(&units[start..cursor as usize]));
        builder.push_str("<--[HERE]");
        Some(builder)
    }

    /// Java `CommandSyntaxException.getInput()` — `null` when created without input.
    pub fn get_input(&self) -> Option<&str> {
        self.input.as_deref()
    }

    /// Java `CommandSyntaxException.getCursor()`.
    pub fn get_cursor(&self) -> i32 {
        self.cursor
    }

    /// Java `CommandSyntaxException.BUILT_IN_EXCEPTIONS` accessor.
    pub fn built_in_exceptions() -> &'static BuiltInExceptions {
        &BUILT_IN_EXCEPTIONS
    }

    /// Access to the message `Arc` for `Clone` (the message is shared by reference
    /// in Java; cloning the exception shares it).
    pub fn get_raw_message_arc(&self) -> &Arc<dyn Message> {
        &self.message
    }
}

/// Java exceptions are `Throwable` references, freely shared. The Rust exception
/// holds a borrowed type instance and an `Arc` message; cloning shares both.
impl Clone for CommandSyntaxException<'_> {
    fn clone(&self) -> Self {
        CommandSyntaxException {
            exception_type: self.exception_type,
            message: Arc::clone(&self.message),
            input: self.input.clone(),
            cursor: self.cursor,
        }
    }
}

impl std::fmt::Display for CommandSyntaxException<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.get_message())
    }
}

impl std::fmt::Debug for CommandSyntaxException<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSyntaxException")
            .field("message", &self.get_message())
            .field("input", &self.input)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl std::error::Error for CommandSyntaxException<'_> {}

// Paper - Brigadier API
// RivetTodo(#85): `componentMessage()` (via
// `net.kyori.adventure.util.ComponentMessageThrowable`) is Paper-only and
// depends on Adventure text types (rivet-text, epic #12) not yet ported.
