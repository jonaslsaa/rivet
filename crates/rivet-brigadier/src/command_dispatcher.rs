//! Port of `com.mojang.brigadier.CommandDispatcher` (upstream brigadier-1.3.10).

use std::sync::Arc;

use crate::ambiguity_consumer::AmbiguityConsumer;
use crate::builder::literal_argument_builder::LiteralArgumentBuilder;
use crate::context::command_context_builder::CommandContextBuilder;
use crate::context::context_chain::ContextChain;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::parse_results::ParseResults;
use crate::result_consumer::ResultConsumer;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestions, SuggestionsBuilder};
use crate::tree::{CommandNode, LiteralCommandNode, RootCommandNode};

/// Java `CommandDispatcher.ARGUMENT_SEPARATOR`.
pub const ARGUMENT_SEPARATOR: &str = " ";

/// Java `CommandDispatcher.ARGUMENT_SEPARATOR_CHAR`.
pub const ARGUMENT_SEPARATOR_CHAR: char = ' ';

const USAGE_OPTIONAL_OPEN: &str = "[";
const USAGE_OPTIONAL_CLOSE: &str = "]";
const USAGE_REQUIRED_OPEN: &str = "(";
const USAGE_REQUIRED_CLOSE: &str = ")";
const USAGE_OR: &str = "|";

/// Java `CommandDispatcher<S>` — the core command dispatcher for registering,
/// parsing and executing commands.
pub struct CommandDispatcher<S> {
    root: Arc<RootCommandNode<S>>,
    root_dyn: Arc<dyn CommandNode<S>>,
    consumer: Arc<dyn ResultConsumer<S>>,
}

impl<S: 'static> Default for CommandDispatcher<S> {
    fn default() -> Self {
        CommandDispatcher::new()
    }
}

impl<S: 'static> CommandDispatcher<S> {
    /// Java `CommandDispatcher(RootCommandNode)`.
    pub fn new_with_root(root: Arc<RootCommandNode<S>>) -> Self {
        let root_dyn = root.clone() as Arc<dyn CommandNode<S>>;
        CommandDispatcher {
            root,
            root_dyn,
            consumer: Arc::new(NoopConsumer),
        }
    }

    /// Java `CommandDispatcher()`.
    pub fn new() -> Self {
        CommandDispatcher::new_with_root(Arc::new(RootCommandNode::new()))
    }

    /// Java `register(LiteralArgumentBuilder)`.
    pub fn register(
        &mut self,
        command: &mut LiteralArgumentBuilder<S>,
    ) -> Arc<LiteralCommandNode<S>> {
        let build = command.build_arc();
        let literal = Arc::clone(&build);
        self.root.add_child(build);
        literal
    }

    /// Java `setConsumer(ResultConsumer)`.
    pub fn set_consumer(&mut self, consumer: Arc<dyn ResultConsumer<S>>) {
        self.consumer = consumer;
    }

    /// Java `execute(String, S)`.
    pub fn execute_string(
        &self,
        input: &str,
        source: S,
    ) -> Result<i32, CommandSyntaxException<'static>>
    where
        S: Clone,
    {
        self.execute(StringReader::new(input), source)
    }

    /// Java `execute(StringReader, S)`.
    pub fn execute(
        &self,
        input: StringReader,
        source: S,
    ) -> Result<i32, CommandSyntaxException<'static>>
    where
        S: Clone,
    {
        let parse = self.parse(input, source);
        self.execute_parse(parse)
    }

    /// Java `execute(ParseResults)`.
    pub fn execute_parse(
        &self,
        parse: ParseResults<S>,
    ) -> Result<i32, CommandSyntaxException<'static>>
    where
        S: Clone,
    {
        if parse.get_reader().can_read() {
            if parse.exceptions_len() == 1 {
                let ex = &parse.get_exceptions()[0].1;
                return Err(ex.clone());
            } else if parse.get_context().get_range().is_empty() {
                return Err(CommandSyntaxException::built_in_exceptions()
                    .dispatcher_unknown_command()
                    .create_with_context(parse.get_reader()));
            } else {
                return Err(CommandSyntaxException::built_in_exceptions()
                    .dispatcher_unknown_argument()
                    .create_with_context(parse.get_reader()));
            }
        }

        let original = parse
            .get_context()
            .build(parse.get_reader().get_string().to_string());

        let flat_context = ContextChain::try_flatten(&original);
        if flat_context.is_none() {
            self.consumer.on_command_complete(&original, false, 0);
            return Err(CommandSyntaxException::built_in_exceptions()
                .dispatcher_unknown_command()
                .create_with_context(parse.get_reader()));
        }

        flat_context
            .expect("checked above")
            .execute_all(original.get_source().clone(), self.consumer.as_ref())
    }

    /// Java `parse(String, S)`.
    pub fn parse_string(&self, command: &str, source: S) -> ParseResults<S>
    where
        S: Clone,
    {
        self.parse(StringReader::new(command), source)
    }

    /// Java `parse(StringReader, S)`.
    pub fn parse(&self, command: StringReader, source: S) -> ParseResults<S>
    where
        S: Clone,
    {
        let context =
            CommandContextBuilder::new(source, Arc::clone(&self.root_dyn), command.get_cursor());
        self.parse_nodes(Arc::clone(&self.root_dyn), command, context)
    }

    /// Java `parseNodes(CommandNode, StringReader, CommandContextBuilder)` — the
    /// core recursive parse. Returns a `ParseResults` whose reader may still have
    /// input (an incomplete parse) and whose exceptions map collects the per-child
    /// errors.
    ///
    /// Java wraps a `RuntimeException` escaping `child.parse` into a
    /// `dispatcherParseException`; a Rust panic cannot be caught here (no
    /// `panic=unwind` guarantee is relied upon), so the wrap is not reproduced —
    /// the parse surface in this crate returns `Result` and never panics.
    fn parse_nodes(
        &self,
        node: Arc<dyn CommandNode<S>>,
        original_reader: StringReader,
        context_so_far: CommandContextBuilder<S>,
    ) -> ParseResults<S>
    where
        S: Clone,
    {
        let source = context_so_far.get_source().clone();
        let mut errors: Vec<(Arc<dyn CommandNode<S>>, CommandSyntaxException<'static>)> =
            Vec::new();
        let mut potentials: Option<Vec<ParseResults<S>>> = None;
        let cursor = original_reader.get_cursor();

        let relevant = node.get_relevant_nodes(&mut original_reader.clone());
        for child in relevant {
            if !child.can_use(&source) {
                continue;
            }
            let mut context = context_so_far.copy();
            let mut reader = original_reader.clone();
            let mut errored = false;
            // RivetTodo(#210): Paper's `TagParseCommandSyntaxException` short-circuit
            // (parse failure of a Minecraft tag argument aborts dispatch instead of
            // falling through) is not ported — it depends on the Paper exception type.
            if let Err(ex) = child.parse(child.clone(), &mut reader, &mut context) {
                errors.push((child.clone(), ex));
                reader.set_cursor(cursor);
                errored = true;
            }
            if errored {
                continue;
            }
            if reader.can_read() && reader.peek() != ARGUMENT_SEPARATOR_CHAR {
                errors.push((
                    child.clone(),
                    CommandSyntaxException::built_in_exceptions()
                        .dispatcher_expected_argument_separator()
                        .create_with_context(&reader),
                ));
                reader.set_cursor(cursor);
                continue;
            }

            context.with_command(child.get_command());
            if reader.can_read_with_length(if child.get_redirect().is_none() { 2 } else { 1 }) {
                reader.skip();
                if let Some(redirect) = child.get_redirect() {
                    let child_context = CommandContextBuilder::new(
                        source.clone(),
                        Arc::clone(&redirect),
                        reader.get_cursor(),
                    );
                    let parse = self.parse_nodes(redirect, reader, child_context);
                    context.with_child(parse.get_context().clone());
                    return ParseResults::new(
                        context,
                        parse.get_reader().clone(),
                        parse.get_exceptions().to_vec(),
                    );
                } else {
                    let parse = self.parse_nodes(child.clone(), reader, context);
                    potentials
                        .get_or_insert_with(|| Vec::with_capacity(1))
                        .push(parse);
                }
            } else {
                potentials
                    .get_or_insert_with(|| Vec::with_capacity(1))
                    .push(ParseResults::new(context, reader, Vec::new()));
            }
        }

        if let Some(mut potentials) = potentials {
            if potentials.len() > 1 {
                potentials.sort_by(|a, b| {
                    let a_done = !a.get_reader().can_read();
                    let b_done = !b.get_reader().can_read();
                    if a_done && !b_done {
                        return std::cmp::Ordering::Less;
                    }
                    if !a_done && b_done {
                        return std::cmp::Ordering::Greater;
                    }
                    let a_empty = a.exceptions_is_empty();
                    let b_empty = b.exceptions_is_empty();
                    if a_empty && !b_empty {
                        return std::cmp::Ordering::Less;
                    }
                    if !a_empty && b_empty {
                        return std::cmp::Ordering::Greater;
                    }
                    std::cmp::Ordering::Equal
                });
            }
            return potentials.remove(0);
        }

        ParseResults::new(context_so_far, original_reader, errors)
    }

    /// Java `getAllUsage(CommandNode, S, boolean)`.
    pub fn get_all_usage(
        &self,
        node: &dyn CommandNode<S>,
        source: &S,
        restricted: bool,
    ) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        self.get_all_usage_rec(node, source, &mut result, "", restricted);
        result
    }

    fn get_all_usage_rec(
        &self,
        node: &dyn CommandNode<S>,
        source: &S,
        result: &mut Vec<String>,
        prefix: &str,
        restricted: bool,
    ) {
        if restricted && !node.can_use(source) {
            return;
        }

        if node.get_command().is_some() {
            result.push(prefix.to_string());
        }

        if let Some(redirect) = node.get_redirect() {
            let redirect_str = if Arc::ptr_eq(&redirect, &self.root_dyn) {
                "...".to_string()
            } else {
                format!("-> {}", redirect.get_usage_text())
            };
            if prefix.is_empty() {
                result.push(format!(
                    "{}{}{}",
                    node.get_usage_text(),
                    ARGUMENT_SEPARATOR,
                    redirect_str
                ));
            } else {
                result.push(format!("{}{}{}", prefix, ARGUMENT_SEPARATOR, redirect_str));
            }
        } else if !node.get_children().is_empty() {
            for child in node.get_children() {
                let child_prefix = if prefix.is_empty() {
                    child.get_usage_text()
                } else {
                    format!("{}{}{}", prefix, ARGUMENT_SEPARATOR, child.get_usage_text())
                };
                self.get_all_usage_rec(&*child, source, result, &child_prefix, restricted);
            }
        }
    }

    /// Java `getSmartUsage(CommandNode, S)` — a `Map<CommandNode, String>`; the
    /// insertion-ordered list of `(node, usage)` pairs preserves the `LinkedHashMap`
    /// ordering.
    pub fn get_smart_usage(
        &self,
        node: &dyn CommandNode<S>,
        source: &S,
    ) -> Vec<(Arc<dyn CommandNode<S>>, String)> {
        let optional = node.get_command().is_some();
        let mut result: Vec<(Arc<dyn CommandNode<S>>, String)> = Vec::new();
        for child in node.get_children() {
            if let Some(usage) = self.get_smart_usage_rec(&*child, source, optional, false) {
                result.push((child.clone(), usage));
            }
        }
        result
    }

    fn get_smart_usage_rec(
        &self,
        node: &dyn CommandNode<S>,
        source: &S,
        optional: bool,
        deep: bool,
    ) -> Option<String> {
        if !node.can_use(source) {
            return None;
        }

        let self_usage = if optional {
            format!(
                "{}{}{}",
                USAGE_OPTIONAL_OPEN,
                node.get_usage_text(),
                USAGE_OPTIONAL_CLOSE
            )
        } else {
            node.get_usage_text()
        };
        let child_optional = node.get_command().is_some();
        let open = if child_optional {
            USAGE_OPTIONAL_OPEN
        } else {
            USAGE_REQUIRED_OPEN
        };
        let close = if child_optional {
            USAGE_OPTIONAL_CLOSE
        } else {
            USAGE_REQUIRED_CLOSE
        };

        if !deep {
            if let Some(redirect) = node.get_redirect() {
                let redirect_str = if Arc::ptr_eq(&redirect, &self.root_dyn) {
                    "...".to_string()
                } else {
                    format!("-> {}", redirect.get_usage_text())
                };
                return Some(format!(
                    "{}{}{}",
                    self_usage, ARGUMENT_SEPARATOR, redirect_str
                ));
            } else {
                let children: Vec<Arc<dyn CommandNode<S>>> = node
                    .get_children()
                    .iter()
                    .filter(|c| c.can_use(source))
                    .cloned()
                    .collect();
                if children.len() == 1 {
                    if let Some(usage) = self.get_smart_usage_rec(
                        &*children[0],
                        source,
                        child_optional,
                        child_optional,
                    ) {
                        return Some(format!("{}{}{}", self_usage, ARGUMENT_SEPARATOR, usage));
                    }
                } else if children.len() > 1 {
                    let mut child_usage: Vec<String> = Vec::new();
                    for child in &children {
                        if let Some(usage) =
                            self.get_smart_usage_rec(child, source, child_optional, true)
                            && !child_usage.contains(&usage)
                        {
                            child_usage.push(usage);
                        }
                    }
                    if child_usage.len() == 1 {
                        let usage = child_usage[0].clone();
                        if child_optional {
                            return Some(format!(
                                "{}{}{}{}{}",
                                self_usage,
                                ARGUMENT_SEPARATOR,
                                USAGE_OPTIONAL_OPEN,
                                usage,
                                USAGE_OPTIONAL_CLOSE
                            ));
                        } else {
                            return Some(format!("{}{}{}", self_usage, ARGUMENT_SEPARATOR, usage));
                        }
                    } else if child_usage.len() > 1 {
                        let mut builder = String::from(open);
                        let mut count = 0;
                        for child in &children {
                            if count > 0 {
                                builder.push_str(USAGE_OR);
                            }
                            builder.push_str(&child.get_usage_text());
                            count += 1;
                        }
                        if count > 0 {
                            builder.push_str(close);
                            return Some(format!(
                                "{}{}{}",
                                self_usage, ARGUMENT_SEPARATOR, builder
                            ));
                        }
                    }
                }
            }
        }

        Some(self_usage)
    }

    /// Java `getCompletionSuggestions(ParseResults)` — suggestions at the end of the
    /// parsed input. Synchronous (see the suggestion package doc); Java's future is a
    /// plain `Suggestions`.
    pub fn get_completion_suggestions(&self, parse: &ParseResults<S>) -> Suggestions
    where
        S: Clone,
    {
        self.get_completion_suggestions_with_cursor(parse, parse.get_reader().get_total_length())
    }

    /// Java `getCompletionSuggestions(ParseResults, int cursor)`.
    pub fn get_completion_suggestions_with_cursor(
        &self,
        parse: &ParseResults<S>,
        cursor: i32,
    ) -> Suggestions
    where
        S: Clone,
    {
        let context = parse.get_context();

        let node_before_cursor = context.find_suggestion_context(cursor);
        let context = &node_before_cursor.context;
        let parent = node_before_cursor.parent;
        let start = i32::min(node_before_cursor.start_pos, cursor);

        let full_input = parse.get_reader().get_string();
        let truncated_input = substring_utf16(full_input, 0, cursor);
        let truncated_input_lower_case = truncated_input.to_lowercase();

        // Paper: don't suggest root-level children whose requirement isn't met
        // (`parent != root || node.canUse(source)`). An unmet root child contributes
        // an empty future in Java; skipping it is identical for the merge.
        let parent_is_root = Arc::ptr_eq(&parent, &self.root_dyn);
        let mut suggestions: Vec<Suggestions> = Vec::new();
        for node in parent.get_children() {
            if parent_is_root && !node.can_use(context.get_source()) {
                continue;
            }
            let mut builder = SuggestionsBuilder::new(
                truncated_input.clone(),
                truncated_input_lower_case.clone(),
                start,
            );
            // Java catches `CommandSyntaxException` thrown by listSuggestions (a
            // provider can throw) and treats it as an empty future.
            match node.list_suggestions(&context.build(truncated_input.clone()), &mut builder) {
                Ok(s) => suggestions.push(s),
                Err(_) => suggestions.push(Suggestions::empty()),
            }
        }

        Suggestions::merge(full_input, &suggestions)
    }

    /// Java `getRoot()`.
    pub fn get_root(&self) -> &Arc<RootCommandNode<S>> {
        &self.root
    }

    /// Java `getPath(CommandNode)`.
    ///
    /// Java compares node identity with `==`; the equivalent is `Arc::ptr_eq` against
    /// the node handle. The handle is the `Arc<LiteralCommandNode>`/`Arc<ArgumentCommandNode>`
    /// returned by `register`/`build` (which the dispatcher stores as its trait-object
    /// `Arc`, so pointer equality holds).
    pub fn get_path(&self, target: &Arc<dyn CommandNode<S>>) -> Vec<String> {
        let mut nodes: Vec<Vec<Arc<dyn CommandNode<S>>>> = Vec::new();
        self.add_paths(Arc::clone(&self.root_dyn), &mut nodes, Vec::new());

        for list in nodes {
            if Arc::ptr_eq(&list[list.len() - 1], target) {
                let result: Vec<String> = list
                    .iter()
                    .filter(|n| !Arc::ptr_eq(n, &self.root_dyn))
                    .map(|n| n.get_name().to_string())
                    .collect();
                return result;
            }
        }
        Vec::new()
    }

    fn add_paths(
        &self,
        node: Arc<dyn CommandNode<S>>,
        result: &mut Vec<Vec<Arc<dyn CommandNode<S>>>>,
        parents: Vec<Arc<dyn CommandNode<S>>>,
    ) {
        let mut current = parents;
        current.push(node.clone());
        result.push(current.clone());
        for child in node.get_children() {
            self.add_paths(child.clone(), result, current.clone());
        }
    }

    /// Java `findNode(Collection<String>)`.
    pub fn find_node(&self, path: &[String]) -> Option<Arc<dyn CommandNode<S>>> {
        let mut node: Arc<dyn CommandNode<S>> = Arc::clone(&self.root_dyn);
        for name in path {
            node = node.get_child(name)?;
        }
        Some(node)
    }

    /// Java `findAmbiguities(AmbiguityConsumer)`.
    pub fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>) {
        self.root_dyn.find_ambiguities(consumer);
    }
}

/// Java's `hasCommand` predicate — an input has a command if it or any descendant
/// has one. Not part of the observable parse/execute surface; retained as a
/// module-private fn for parity documentation.
#[allow(dead_code)]
fn has_command<S>(node: &dyn CommandNode<S>) -> bool {
    node.get_command().is_some() || node.get_children().iter().any(|c| has_command(&**c))
}

/// The default `ResultConsumer` — Java's `(c, s, r) -> {}`.
struct NoopConsumer;

impl<S> ResultConsumer<S> for NoopConsumer {
    fn on_command_complete(
        &self,
        _context: &crate::context::CommandContext<S>,
        _success: bool,
        _result: i32,
    ) {
    }
}

/// `input.substring(start, end)` in UTF-16 code units.
fn substring_utf16(input: &str, start: i32, end: i32) -> String {
    let units: Vec<u16> = input.encode_utf16().collect();
    let start = i32::max(0, start) as usize;
    let end = i32::min(units.len() as i32, end) as usize;
    crate::immutable_string_reader::utf16_units_to_string(&units[start..end])
}
