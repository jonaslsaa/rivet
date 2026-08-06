//! Port of `com.mojang.brigadier.Command` (upstream brigadier-1.3.10).

use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;

/// Java `Command.SINGLE_SUCCESS`.
pub const SINGLE_SUCCESS: i32 = 1;

/// Java `Command<S>` — a functional interface running a command for a
/// `CommandContext`, returning an `int` result.
pub trait Command<S>: Send + Sync {
    /// Java `run(CommandContext<S>) throws CommandSyntaxException`.
    fn run(&self, context: &CommandContext<S>) -> Result<i32, CommandSyntaxException<'static>>;
}

/// Java `Command<S>` as a closure (`Command.run`).
pub type CommandFn<S> =
    Box<dyn Fn(&CommandContext<S>) -> Result<i32, CommandSyntaxException<'static>> + Send + Sync>;

/// Adapter turning a closure into a `Command<S>` (Java's lambda-to-interface
/// conversion).
pub struct ClosureCommand<S> {
    function: CommandFn<S>,
}

impl<S> ClosureCommand<S> {
    /// Java lambda conversion — `(context) -> result`.
    pub fn new(function: CommandFn<S>) -> Self {
        ClosureCommand { function }
    }
}

impl<S> Command<S> for ClosureCommand<S> {
    fn run(&self, context: &CommandContext<S>) -> Result<i32, CommandSyntaxException<'static>> {
        (self.function)(context)
    }
}
