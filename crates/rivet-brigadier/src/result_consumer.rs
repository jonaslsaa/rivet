//! Port of `com.mojang.brigadier.ResultConsumer` (upstream brigadier-1.3.10).

use crate::context::CommandContext;

/// Java `ResultConsumer<S>` — notified of the result of every executed command.
pub trait ResultConsumer<S>: Send + Sync {
    /// Java `onCommandComplete(CommandContext<S>, boolean success, int result)`.
    fn on_command_complete(&self, context: &CommandContext<S>, success: bool, result: i32);
}
