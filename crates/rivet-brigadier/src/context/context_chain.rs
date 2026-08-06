//! Port of `com.mojang.brigadier.context.ContextChain` (upstream brigadier-1.3.10).
//!
//! Java's `ContextChain` holds references into the built `CommandContext` tree; the
//! chain borrows it (Rust lifetime `'a`). The Java `nextStageCache` memoization is a
//! pure optimization — dropped, since `next_stage` recomputes from the same
//! immutable slices.

use crate::context::command_context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::result_consumer::ResultConsumer;

/// Java `ContextChain<S>` — a flattened list of modifier contexts plus the final
/// executable context, run left to right.
pub struct ContextChain<'a, S> {
    modifiers: Vec<&'a CommandContext<S>>,
    executable: &'a CommandContext<S>,
}

impl<'a, S: 'static> ContextChain<'a, S> {
    /// Java `ContextChain(List, CommandContext)`.
    pub fn new(modifiers: Vec<&'a CommandContext<S>>, executable: &'a CommandContext<S>) -> Self {
        if executable.get_command().is_none() {
            panic!("Last command in chain must be executable");
        }
        ContextChain {
            modifiers,
            executable,
        }
    }

    /// Java `tryFlatten(CommandContext)` — walks the child chain collecting the
    /// modifier contexts; the last context must be executable.
    pub fn try_flatten(root_context: &'a CommandContext<S>) -> Option<ContextChain<'a, S>> {
        let mut modifiers: Vec<&'a CommandContext<S>> = Vec::new();
        let mut current = root_context;
        loop {
            match current.get_child() {
                Some(child) => {
                    modifiers.push(current);
                    current = child;
                }
                None => {
                    current.get_command()?;
                    return Some(ContextChain {
                        modifiers,
                        executable: current,
                    });
                }
            }
        }
    }

    /// Java `runModifier(CommandContext, S, ResultConsumer, boolean)`.
    pub fn run_modifier(
        modifier: &CommandContext<S>,
        source: S,
        result_consumer: &dyn ResultConsumer<S>,
        forked_mode: bool,
    ) -> Result<Vec<S>, CommandSyntaxException<'static>> {
        let source_modifier = modifier.get_redirect_modifier();
        // Note: the source currently in the context is irrelevant here, since it may
        // have been updated in an earlier stage (Java comment).
        if source_modifier.is_none() {
            // Simple redirect — propagate the source to the next node.
            return Ok(vec![source]);
        }

        let context_to_use = modifier.copy_for(source);
        match source_modifier
            .expect("checked above")
            .apply(&context_to_use)
        {
            Ok(result) => Ok(result),
            Err(ex) => {
                result_consumer.on_command_complete(&context_to_use, false, 0);
                if forked_mode { Ok(Vec::new()) } else { Err(ex) }
            }
        }
    }

    /// Java `runExecutable(CommandContext, S, ResultConsumer, boolean)`.
    pub fn run_executable(
        executable: &CommandContext<S>,
        source: S,
        result_consumer: &dyn ResultConsumer<S>,
        forked_mode: bool,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        let context_to_use = executable.copy_for(source);
        let command = executable
            .get_command()
            .expect("last command in chain must be executable");
        match command.run(&context_to_use) {
            Ok(result) => {
                result_consumer.on_command_complete(&context_to_use, true, result);
                Ok(if forked_mode { 1 } else { result })
            }
            Err(ex) => {
                result_consumer.on_command_complete(&context_to_use, false, 0);
                if forked_mode { Ok(0) } else { Err(ex) }
            }
        }
    }

    /// Java `executeAll(S, ResultConsumer)`.
    pub fn execute_all(
        &self,
        source: S,
        result_consumer: &dyn ResultConsumer<S>,
    ) -> Result<i32, CommandSyntaxException<'static>> {
        if self.modifiers.is_empty() {
            // Fast path — just a single stage.
            return Self::run_executable(self.executable, source, result_consumer, false);
        }

        let mut forked_mode = false;
        let mut current_sources: Vec<S> = vec![source];

        for modifier in &self.modifiers {
            forked_mode |= modifier.is_forked();

            let mut next_sources: Vec<S> = Vec::new();
            for source_to_run in current_sources {
                next_sources.extend(Self::run_modifier(
                    modifier,
                    source_to_run,
                    result_consumer,
                    forked_mode,
                )?);
            }
            if next_sources.is_empty() {
                return Ok(0);
            }
            current_sources = next_sources;
        }

        let mut result = 0i32;
        for execution_source in current_sources {
            result = result.wrapping_add(Self::run_executable(
                self.executable,
                execution_source,
                result_consumer,
                forked_mode,
            )?);
        }

        Ok(result)
    }

    /// Java `getStage()`.
    pub fn get_stage(&self) -> Stage {
        if self.modifiers.is_empty() {
            Stage::Execute
        } else {
            Stage::Modify
        }
    }

    /// Java `getTopContext()`.
    pub fn get_top_context(&self) -> &'a CommandContext<S> {
        if self.modifiers.is_empty() {
            self.executable
        } else {
            self.modifiers[0]
        }
    }

    /// Java `nextStage()` — a chain of the remaining modifiers; `None` when there
    /// are none.
    pub fn next_stage(&self) -> Option<ContextChain<'a, S>> {
        if self.modifiers.is_empty() {
            return None;
        }
        Some(ContextChain {
            modifiers: self.modifiers[1..].to_vec(),
            executable: self.executable,
        })
    }
}

/// Java `ContextChain.Stage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Modify,
    Execute,
}
