//! Port of `com.mojang.brigadier.ParseResults` (upstream brigadier-1.3.10).

use std::sync::Arc;

use crate::context::command_context_builder::CommandContextBuilder;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;
use crate::tree::CommandNode;

/// Java `ParseResults<S>` — the result of a parse: the accumulated context builder,
/// the reader position where parsing stopped, and the per-child-node exceptions.
///
/// Java's exceptions are a `Map<CommandNode, CommandSyntaxException>`. A Rust
/// `Arc<dyn CommandNode>` has no `Hash`/`Eq` (nodes are trait objects with
/// structural equality), so the map is a `Vec<(Arc<dyn CommandNode>, exception)>`
/// in insertion order (Java's `LinkedHashMap` order). The `len() == 1` and
/// ordering-adjacent usages behave identically.
pub struct ParseResults<S> {
    context: CommandContextBuilder<S>,
    exceptions: Vec<(Arc<dyn CommandNode<S>>, CommandSyntaxException<'static>)>,
    reader: StringReader,
}

impl<S: 'static> ParseResults<S> {
    /// Java `ParseResults(CommandContextBuilder, ImmutableStringReader, Map)`.
    pub fn new(
        context: CommandContextBuilder<S>,
        reader: StringReader,
        exceptions: Vec<(Arc<dyn CommandNode<S>>, CommandSyntaxException<'static>)>,
    ) -> Self {
        ParseResults {
            context,
            exceptions,
            reader,
        }
    }

    /// Java `ParseResults(CommandContextBuilder)`.
    pub fn new_empty(context: CommandContextBuilder<S>) -> Self {
        ParseResults {
            context,
            exceptions: Vec::new(),
            reader: StringReader::new(""),
        }
    }

    /// Java `getContext()`.
    pub fn get_context(&self) -> &CommandContextBuilder<S> {
        &self.context
    }

    /// Java `getReader()`.
    pub fn get_reader(&self) -> &StringReader {
        &self.reader
    }

    /// Java `getExceptions()` — `Map` semantics reduced to the pair list.
    pub fn get_exceptions(&self) -> &[(Arc<dyn CommandNode<S>>, CommandSyntaxException<'static>)] {
        &self.exceptions
    }

    /// Java `getExceptions().size()`.
    pub fn exceptions_len(&self) -> usize {
        self.exceptions.len()
    }

    /// Java `getExceptions().isEmpty()`.
    pub fn exceptions_is_empty(&self) -> bool {
        self.exceptions.is_empty()
    }
}

impl<S: 'static> Clone for ParseResults<S>
where
    CommandContextBuilder<S>: Clone,
{
    fn clone(&self) -> Self {
        ParseResults {
            context: self.context.clone(),
            exceptions: self.exceptions.clone(),
            reader: self.reader.clone(),
        }
    }
}

impl<S> std::fmt::Debug for ParseResults<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParseResults")
            .field("reader_remaining", &self.reader.get_remaining())
            .field("exception_count", &self.exceptions.len())
            .finish_non_exhaustive()
    }
}
