//! Port of `com.mojang.brigadier.context.CommandContext` (upstream).
//!
//! // STUB(brigadier.builder): full port is the root `com.mojang.brigadier` unit. The
//! builder cluster only passes a `CommandContext` to `Command.run` /
//! `RedirectModifier.apply`; it never inspects one.

/// Java `CommandContext<S>` — the result of parsing a command, passed to the
/// command and redirect modifiers.
pub struct CommandContext<S> {
    source: S,
}

impl<S> CommandContext<S> {
    /// Java `CommandContext(...)` — source plus the parsed command state. STUB: only
    /// `source` is carried; the rest is the root unit's port.
    pub fn new(source: S) -> Self {
        CommandContext { source }
    }

    /// Java `getSource()`.
    pub fn get_source(&self) -> &S {
        &self.source
    }
}
