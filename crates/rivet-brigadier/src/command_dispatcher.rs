//! Port of `com.mojang.brigadier.CommandDispatcher` (upstream + Paper patches).
//!
//! // STUB(brigadier): full port is the root `com.mojang.brigadier` unit; this is a
//! placeholder so the module path exists. `LiteralArgumentBuilder` is consumed via
//! `register(LiteralArgumentBuilder)` in the Java source; that surface arrives with
//! the real port.

use crate::tree::LiteralCommandNode;

/// Java `CommandDispatcher<S>`.
pub struct CommandDispatcher<S> {
    _marker: std::marker::PhantomData<S>,
}

impl<S: 'static> Default for CommandDispatcher<S> {
    fn default() -> Self {
        CommandDispatcher::new()
    }
}

impl<S: 'static> CommandDispatcher<S> {
    /// Java `CommandDispatcher()`.
    pub fn new() -> Self {
        CommandDispatcher {
            _marker: std::marker::PhantomData,
        }
    }

    /// Java `register(LiteralArgumentBuilder<S>)`.
    pub fn register(&mut self, _command: crate::builder::LiteralArgumentBuilder<S>) -> std::sync::Arc<LiteralCommandNode<S>> {
        // STUB(brigadier): builder unit only needs the signature; real body is the
        // root unit's port.
        std::sync::Arc::new(LiteralCommandNode::new(
            "".to_string(),
            None,
            std::sync::Arc::new(|_| true),
            None,
            None,
            false,
        ))
    }
}
