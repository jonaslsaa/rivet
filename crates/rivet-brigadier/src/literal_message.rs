//! Port of `com.mojang.brigadier.LiteralMessage` (upstream).

use std::sync::Arc;

use crate::Message;

/// Java `LiteralMessage` — a `Message` wrapping a constant string.
///
/// Java does not override `equals`/`hashCode`, so instances compare by reference
/// identity. Accordingly this type derives no `PartialEq`/`Eq` — compare
/// instances by reference (`Arc::ptr_eq`) to keep Java's identity semantics.
#[derive(Debug, Clone)]
pub struct LiteralMessage {
    string: String,
}

impl LiteralMessage {
    pub fn new(string: impl Into<String>) -> Self {
        LiteralMessage {
            string: string.into(),
        }
    }

    /// `getString()`.
    pub fn get_string(&self) -> &str {
        &self.string
    }
}

impl Message for LiteralMessage {
    fn get_string(&self) -> &str {
        &self.string
    }
}

impl std::fmt::Display for LiteralMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.string)
    }
}

/// `LiteralMessage` -> `Arc<dyn Message>`, so exception-type constructors accept a
/// `LiteralMessage` directly (the only `Message` produced in this crate).
impl From<LiteralMessage> for Arc<dyn Message> {
    fn from(message: LiteralMessage) -> Self {
        Arc::new(message)
    }
}
