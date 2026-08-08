//! Port of the `com.mojang.brigadier.tree` package (upstream brigadier-1.3.10).
//!
//! Java's `CommandNode` base class stores `children`/`literals`/`arguments` as
//! `LinkedHashMap`s, and `addChild` mutates an existing child in place. Here the
//! maps are replaced by a shared `NodeChildren<S>` value guarded by a `RwLock`
//! inside each node, so `addChild` takes `&self` (Java's in-place mutation) and an
//! `Arc`-shared node — including the dispatcher root, which `register` mutates —
//! stays mutable. `getChildren()` returns an owned snapshot (Java's live view; the
//! snapshot is behaviorally identical for read-only parse traversals).
//!
//! The in-place merge of a duplicate-named child (`addChild` on an existing child)
//! is reproduced by `merge_child` — the existing child is mutated directly (its
//! command overwritten if the new node has one, then the new node's grandchildren
//! added), matching Java's `CommandNode.addChild`. Because the stored child is
//! shared by `Arc`, every reference to it — including an earlier redirect target or
//! a context's node chain — observes the merge.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::ImmutableStringReader;
use crate::ambiguity_consumer::AmbiguityConsumer;
use crate::builder::argument_builder::{ArgumentBuilderBehavior, Predicate};
use crate::command::Command;
use crate::context::StringRange;
use crate::context::command_context::CommandContext;
use crate::context::command_context_builder::CommandContextBuilder;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::redirect_modifier::RedirectModifier;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestions, SuggestionsBuilder};

#[cfg(test)]
mod tests;

/// Paper's `CommandNode.getRelevantNodes(StringReader, Object source)` dispatch —
/// the source's literal resolution for the `minecraft:` prefix prioritization (#211).
/// Paper checks the concrete `CommandSourceStack` runtime type and two `source`
/// values; the Minecraft `CommandSourceStack` type lands with the command-dispatch
/// units (#211's dependency), so this crate generalizes the condition as the `S`
/// type implementing this trait (the Java `instanceof CommandSourceStack` becomes
/// the per-`S` impl choice).
///
/// Java's conditions are: `source instanceof CommandSourceStack css && css.source ==
/// CommandSource.NULL` (the function-parsing compilation context) and `source
/// instanceof CommandSourceStack css && css.source instanceof
/// CloseableCommandBlockSource` (command blocks). For every other `CommandSourceStack`
/// — notably a player, the server console, or RCON — the vanilla exact-literal
/// lookup applies and an unprefixed input does NOT match the `minecraft:` literal.
///
/// The default `resolve_literal` is the identity — a source that is not one of
/// Paper's command-block/function kinds performs the vanilla exact lookup. The
/// future `CommandSourceStack` port overrides it to apply Paper's per-source
/// conditions (the `source` field value and `getCommandBlockOverride`).
pub trait CommandSource: Send + Sync {
    /// Given the word read from the input, the source may map it to a
    /// `minecraft:`-prefixed literal before the lookup. Java decides per-word:
    /// `source == CommandSource.NULL` (function parsing) and `source instanceof
    /// CloseableCommandBlockSource` (command blocks, gated by
    /// `getCommandBlockOverride(word)`) map a non-`:`-containing word to
    /// `"minecraft:" + word`; a player or console source leaves it unchanged.
    fn resolve_literal(&self, text: &str) -> String {
        text.to_string()
    }
}

// The crate's in-tree `S` instantiations (`String`, `&str`, `i32`) are not
// Paper `CommandSourceStack`s — `instanceof CommandSourceStack` is false — so
// they use the default identity resolution: an unprefixed input matches only
// the exact literal.
impl CommandSource for String {}
impl CommandSource for &str {}
impl CommandSource for i32 {}

/// Java `CommandNode<S>` — the abstract tree node, modelled as an object-safe trait.
///
/// Java's `parse` uses the implicit `this` reference to record itself in the context
/// (`contextBuilder.withNode(this, ...)`); here the caller passes the node's `Arc`
/// so the node can hand itself to `with_node`.
pub trait CommandNode<S>: Send + Sync {
    /// Java `getName()`.
    fn get_name(&self) -> &str;
    /// Java `getUsageText()`.
    fn get_usage_text(&self) -> String;
    /// Java `getChildren()` — a snapshot of the insertion-ordered children.
    fn get_children(&self) -> Vec<Arc<dyn CommandNode<S>>>;
    /// Java `getChild(String)`.
    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>>;
    /// Java `getCommand()`.
    fn get_command(&self) -> Option<Arc<dyn Command<S>>>;
    /// Java `getRedirect()`.
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>>;
    /// Java `getRequirement()`.
    fn get_requirement(&self) -> Predicate<S>;
    /// Java `getRedirectModifier()`.
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>>;
    /// Java `isFork()`.
    fn is_fork(&self) -> bool;
    /// Java `canUse(S)`.
    fn can_use(&self, source: &S) -> bool;
    /// Java `addChild(CommandNode)` — merge/insert under a read-write lock.
    fn add_child(&self, node: Arc<dyn CommandNode<S>>);
    /// Java `parse(StringReader, CommandContextBuilder) throws CommandSyntaxException`.
    fn parse(
        &self,
        node: Arc<dyn CommandNode<S>>,
        reader: &mut StringReader,
        context_builder: &mut CommandContextBuilder<S>,
    ) -> Result<(), CommandSyntaxException<'static>>;
    /// Java `listSuggestions(CommandContext, SuggestionsBuilder) throws
    /// CommandSyntaxException`.
    fn list_suggestions(
        &self,
        context: &CommandContext<S>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>>;
    /// Java `getRelevantNodes(StringReader)` — the one-arg overload, which Paper
    /// keeps for compatibility. It forwards with no source so the `minecraft:`
    /// prefix prioritization is inactive (Java passes `null`).
    fn get_relevant_nodes(&self, reader: &mut StringReader) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        self.get_relevant_nodes_with_source(reader, None)
    }
    /// Java `getRelevantNodes(StringReader, Object source)` — Paper's two-arg
    /// overload. The `source` is the command source (`CommandSourceStack` in
    /// Paper); a source whose type implements `CommandSource` resolves a
    /// `minecraft:`-prefixed literal against its unprefixed twin (see
    /// `CommandSource::resolve_literal`). Passing `None` reproduces the vanilla
    /// exact-literal path (Java's `null` source).
    fn get_relevant_nodes_with_source(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource;
    /// Java `isValidInput(String)`.
    fn is_valid_input(&self, input: &str) -> bool;
    /// Java `getExamples()`.
    fn get_examples(&self) -> Vec<String>;
    /// Java `createBuilder()`.
    fn create_builder(&self) -> Box<dyn NodeBuilder<S>>;
    /// Java `findAmbiguities(AmbiguityConsumer)`.
    fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>);
    /// Java `equals(Object)` — structural per concrete class.
    fn equals(&self, other: &dyn CommandNode<S>) -> bool;
    /// Java `hashCode()`.
    fn hash_code(&self) -> i32;
    /// Java `toString()` — `<literal ...>` / `<argument ...:type>` / `<root>`.
    fn to_string(&self) -> String;
    /// Java `addChild(CommandNode)` merge, called on the existing child with the new
    /// node — mutates this node in place (overwrite command if the new node has one,
    /// then merge the grandchildren). Java mutates the stored child so every
    /// reference sees the update.
    fn merge_child(&self, other: &dyn CommandNode<S>);
    /// Downcast helper (`instanceof` in Java).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Blanket `CommandNode` impl for `Arc<dyn CommandNode<S>>` — Java passes nodes by
/// reference everywhere; the Arc deref reproduces that so `&Arc<dyn CommandNode>`
/// can be used as `&dyn CommandNode` in recursive helpers.
impl<S: 'static> CommandNode<S> for Arc<dyn CommandNode<S>> {
    fn get_name(&self) -> &str {
        (**self).get_name()
    }
    fn get_usage_text(&self) -> String {
        (**self).get_usage_text()
    }
    fn get_children(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        (**self).get_children()
    }
    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>> {
        (**self).get_child(name)
    }
    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        (**self).get_command()
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        (**self).get_redirect()
    }
    fn get_requirement(&self) -> Predicate<S> {
        (**self).get_requirement()
    }
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        (**self).get_redirect_modifier()
    }
    fn is_fork(&self) -> bool {
        (**self).is_fork()
    }
    fn can_use(&self, source: &S) -> bool {
        (**self).can_use(source)
    }
    fn add_child(&self, node: Arc<dyn CommandNode<S>>) {
        (**self).add_child(node);
    }
    fn parse(
        &self,
        node: Arc<dyn CommandNode<S>>,
        reader: &mut StringReader,
        context_builder: &mut CommandContextBuilder<S>,
    ) -> Result<(), CommandSyntaxException<'static>> {
        (**self).parse(node, reader, context_builder)
    }
    fn list_suggestions(
        &self,
        context: &CommandContext<S>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>> {
        (**self).list_suggestions(context, builder)
    }
    fn get_relevant_nodes(&self, reader: &mut StringReader) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        (**self).get_relevant_nodes(reader)
    }
    fn get_relevant_nodes_with_source(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        (**self).get_relevant_nodes_with_source(reader, source)
    }
    fn is_valid_input(&self, input: &str) -> bool {
        (**self).is_valid_input(input)
    }
    fn get_examples(&self) -> Vec<String> {
        (**self).get_examples()
    }
    fn create_builder(&self) -> Box<dyn NodeBuilder<S>> {
        (**self).create_builder()
    }
    fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>) {
        (**self).find_ambiguities(consumer);
    }
    fn equals(&self, other: &dyn CommandNode<S>) -> bool {
        (**self).equals(other)
    }
    fn hash_code(&self) -> i32 {
        (**self).hash_code()
    }
    fn to_string(&self) -> String {
        (**self).to_string()
    }
    fn merge_child(&self, other: &dyn CommandNode<S>) {
        (**self).merge_child(other)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        (**self).as_any()
    }
}

/// The public surface a `createBuilder()` result exposes, common to both builder
/// kinds. Java returns the concrete builder; tests downcast via `as_any`.
pub trait NodeBuilder<S>: Send + Sync {
    fn get_requirement(&self) -> Predicate<S>;
    fn get_command(&self) -> Option<Arc<dyn Command<S>>>;
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>>;
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>>;
    fn is_fork(&self) -> bool;
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Shared children storage replacing Java's `children`/`literals`/`arguments` maps.
/// `children` keeps registration order (Java `LinkedHashMap` insertion order);
/// `literals` maps literal names to child indices, `arguments` lists argument child
/// indices in insertion order (Java `arguments.values()` order).
struct NodeChildren<S> {
    children: Vec<Arc<dyn CommandNode<S>>>,
    literals: HashMap<String, usize>,
    arguments: Vec<usize>,
}

impl<S: 'static> NodeChildren<S> {
    fn new() -> Self {
        NodeChildren {
            children: Vec::new(),
            literals: HashMap::new(),
            arguments: Vec::new(),
        }
    }

    /// Java `CommandNode.addChild(CommandNode)`.
    fn add(&mut self, node: Arc<dyn CommandNode<S>>)
    where
        S: 'static,
    {
        if node.as_any().downcast_ref::<RootCommandNode<S>>().is_some() {
            panic!("Cannot add a RootCommandNode as a child to any other CommandNode");
        }
        let name = node.get_name().to_string();
        if let Some(i) = self.children.iter().position(|c| c.get_name() == name) {
            // Java mutates the stored child in place, so every reference to it
            // (including an earlier redirect target) observes the merge.
            self.children[i].merge_child(&*node);
        } else {
            let index = self.children.len();
            let is_literal = node
                .as_any()
                .downcast_ref::<LiteralCommandNode<S>>()
                .is_some();
            self.children.push(node);
            if is_literal {
                self.literals.insert(name, index);
            } else {
                self.arguments.push(index);
            }
        }
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>> {
        self.children.iter().find(|c| c.get_name() == name).cloned()
    }

    /// Java `getRelevantNodes(StringReader, Object source)` — the exact literal whose
    /// text matches the input's next word, else all argument children. Paper's
    /// patch (#211) lets a command source map the word to a `minecraft:`-prefixed
    /// twin before the lookup; the source's `resolve_literal` decides that.
    fn get_relevant_nodes(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        if !self.literals.is_empty() {
            let cursor = reader.get_cursor();
            while reader.can_read() && reader.peek() != ' ' {
                reader.skip();
            }
            let text =
                StringRange::between(cursor, reader.get_cursor()).get_string(reader.get_string());
            reader.set_cursor(cursor);
            // Paper: try the source-resolved key first (e.g. "minecraft:foo"), then
            // the literal word ("foo"); either missing falls through to the arguments.
            let resolved = source.map_or(text.clone(), |s| s.resolve_literal(&text));
            if let Some(&i) = self.literals.get(&resolved) {
                return vec![self.children[i].clone()];
            }
            if resolved != text
                && let Some(&i) = self.literals.get(&text)
            {
                return vec![self.children[i].clone()];
            }
            return self
                .arguments
                .iter()
                .map(|&i| self.children[i].clone())
                .collect();
        }
        self.arguments
            .iter()
            .map(|&i| self.children[i].clone())
            .collect()
    }
}

/// Java `CommandNode.equals` — the children map and command (identity) comparison
/// shared by all concrete nodes.
pub(crate) fn node_eq<S>(a: &dyn CommandNode<S>, b: &dyn CommandNode<S>) -> bool {
    a.equals(b)
}

/// Java `CommandNode.hashCode`.
pub(crate) fn node_hash<S>(node: &dyn CommandNode<S>) -> i32 {
    node.hash_code()
}

/// Java command `.equals` — commands in this crate (closures) have identity
/// equality, reproduced with `Arc::ptr_eq`.
pub(crate) fn command_eq<S>(
    a: &Option<Arc<dyn Command<S>>>,
    b: &Option<Arc<dyn Command<S>>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// Java `Map<String, CommandNode>.equals` — order-independent name→node equality.
pub(crate) fn children_eq<S: 'static>(
    a: &[Arc<dyn CommandNode<S>>],
    b: &[Arc<dyn CommandNode<S>>],
) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for child_a in a {
        match b
            .iter()
            .find(|child_b| child_b.get_name() == child_a.get_name())
        {
            Some(child_b) => {
                if !child_a.equals(&**child_b) {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// Java `Map.hashCode` for the children map — `sum(key.hashCode() ^ value.hashCode())`.
pub(crate) fn children_hash_code<S: 'static>(children: &[Arc<dyn CommandNode<S>>]) -> i32 {
    children.iter().fold(0i32, |acc, c| {
        acc.wrapping_add(crate::java_hash::string_hash(c.get_name()) ^ c.hash_code())
    })
}

/// Java `RootCommandNode<S>`.
pub struct RootCommandNode<S> {
    children: RwLock<NodeChildren<S>>,
    /// Java root constructor passes `c -> true` (a distinct lambda, not the builder's
    /// defaultRequirement).
    requirement: Predicate<S>,
}

impl<S: 'static> RootCommandNode<S> {
    /// Java `RootCommandNode()`.
    pub fn new() -> Self {
        RootCommandNode {
            children: RwLock::new(NodeChildren::new()),
            requirement: Arc::new(|_: &S| true),
        }
    }
}

impl<S: 'static> Default for RootCommandNode<S> {
    fn default() -> Self {
        RootCommandNode::new()
    }
}

impl<S: 'static> CommandNode<S> for RootCommandNode<S> {
    fn get_name(&self) -> &str {
        ""
    }
    fn get_usage_text(&self) -> String {
        String::new()
    }
    fn get_children(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").children.clone()
    }
    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").get_child(name)
    }
    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        None
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        None
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        // Java's root constructor stores `s -> Collections.singleton(s.getSource())`
        // and `getRedirectModifier()` returns it. Root is never added as a parsed node
        // (`parse` is a no-op), so the modifier is unobservable in this crate; storing
        // it would force `S: Clone` onto `RootCommandNode::new` for no behavioral gain.
        None
    }
    fn is_fork(&self) -> bool {
        false
    }
    fn can_use(&self, source: &S) -> bool {
        (self.requirement)(source)
    }
    fn add_child(&self, node: Arc<dyn CommandNode<S>>) {
        self.children.write().expect("lock").add(node);
    }
    fn parse(
        &self,
        _node: Arc<dyn CommandNode<S>>,
        _reader: &mut StringReader,
        _context_builder: &mut CommandContextBuilder<S>,
    ) -> Result<(), CommandSyntaxException<'static>> {
        Ok(())
    }
    fn list_suggestions(
        &self,
        _context: &CommandContext<S>,
        _builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>> {
        Ok(Suggestions::empty())
    }
    fn get_relevant_nodes_with_source(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        self.children
            .read()
            .expect("lock")
            .get_relevant_nodes(reader, source)
    }
    fn is_valid_input(&self, _input: &str) -> bool {
        false
    }
    fn get_examples(&self) -> Vec<String> {
        Vec::new()
    }
    fn create_builder(&self) -> Box<dyn NodeBuilder<S>> {
        panic!("Cannot convert root into a builder");
    }
    fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>) {
        find_ambiguities_default(self, consumer)
    }
    fn equals(&self, other: &dyn CommandNode<S>) -> bool {
        if !other
            .as_any()
            .downcast_ref::<RootCommandNode<S>>()
            .is_some()
        {
            return false;
        }
        children_eq(&self.get_children(), &other.get_children())
            && command_eq(&None, &other.get_command())
    }
    fn hash_code(&self) -> i32 {
        // Java: `31 * children.hashCode() + (command != null ? command.hashCode() : 0)`.
        31_i32
            .wrapping_mul(children_hash_code(&self.get_children()))
            .wrapping_add(0)
    }
    fn to_string(&self) -> String {
        "<root>".to_string()
    }
    fn merge_child(&self, _other: &dyn CommandNode<S>) {
        panic!("Cannot merge a RootCommandNode");
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `LiteralCommandNode<S>`.
pub struct LiteralCommandNode<S> {
    literal: String,
    literal_lower_case: String,
    /// Paper's `nonPrefixed` (#211): for a `minecraft:`-prefixed literal, the text
    /// after the prefix. `parse` accepts the full literal first, then this twin —
    /// so a function-parse input "foo" matches the literal "minecraft:foo".
    non_prefixed: Option<String>,
    children: RwLock<NodeChildren<S>>,
    command: RwLock<Option<Arc<dyn Command<S>>>>,
    requirement: Predicate<S>,
    redirect: Option<Arc<dyn CommandNode<S>>>,
    modifier: Option<Arc<dyn RedirectModifier<S>>>,
    forks: bool,
}

impl<S: 'static> LiteralCommandNode<S> {
    /// Java `LiteralCommandNode(String, Command, Predicate, CommandNode, RedirectModifier, boolean)`.
    pub fn new(
        literal: String,
        command: Option<Arc<dyn Command<S>>>,
        requirement: Predicate<S>,
        redirect: Option<Arc<dyn CommandNode<S>>>,
        modifier: Option<Arc<dyn RedirectModifier<S>>>,
        forks: bool,
    ) -> Self {
        let literal_lower_case = literal.to_lowercase();
        let non_prefixed = literal.strip_prefix("minecraft:").map(str::to_string);
        LiteralCommandNode {
            literal,
            literal_lower_case,
            non_prefixed,
            children: RwLock::new(NodeChildren::new()),
            command: RwLock::new(command),
            requirement,
            redirect,
            modifier,
            forks,
        }
    }

    /// Java `getLiteral()`.
    pub fn get_literal(&self) -> &str {
        &self.literal
    }

    /// Java's private `parse(StringReader, boolean secondPass)` — matches `literal`
    /// against the reader, consuming it only on a word-boundary match. Lengths are
    /// Java `String.length()` (UTF-16 code units).
    fn parse_pass(&self, reader: &mut StringReader, literal: &str) -> i32 {
        let start = reader.get_cursor();
        let literal_len = literal.encode_utf16().count() as i32;
        if reader.can_read_with_length(literal_len) {
            let end = start.wrapping_add(literal_len);
            if substring_utf16(reader.get_string(), start, end) == literal {
                reader.set_cursor(end);
                if !reader.can_read() || reader.peek() == ' ' {
                    return end;
                }
                reader.set_cursor(start);
            }
        }
        -1
    }
}

impl<S: 'static> CommandNode<S> for LiteralCommandNode<S> {
    fn get_name(&self) -> &str {
        &self.literal
    }
    fn get_usage_text(&self) -> String {
        self.literal.clone()
    }
    fn get_children(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").children.clone()
    }
    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").get_child(name)
    }
    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.command.read().expect("lock").as_ref().map(Arc::clone)
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.redirect.as_ref().map(Arc::clone)
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }
    fn is_fork(&self) -> bool {
        self.forks
    }
    fn can_use(&self, source: &S) -> bool {
        (self.requirement)(source)
    }
    fn add_child(&self, node: Arc<dyn CommandNode<S>>) {
        self.children.write().expect("lock").add(node);
    }
    fn parse(
        &self,
        node: Arc<dyn CommandNode<S>>,
        reader: &mut StringReader,
        context_builder: &mut CommandContextBuilder<S>,
    ) -> Result<(), CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        // Paper (#211): first pass against the full literal, then against the
        // `nonPrefixed` twin, so "foo" matches the literal "minecraft:foo".
        let mut end = self.parse_pass(reader, &self.literal);
        if end == -1
            && let Some(non_prefixed) = &self.non_prefixed
        {
            end = self.parse_pass(reader, non_prefixed);
        }
        if end > -1 {
            context_builder.with_node(node, StringRange::between(start, end));
            return Ok(());
        }
        Err(CommandSyntaxException::built_in_exceptions()
            .literal_incorrect()
            .create_with_context(reader, &self.literal))
    }
    fn list_suggestions(
        &self,
        _context: &CommandContext<S>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>> {
        if self
            .literal_lower_case
            .starts_with(builder.get_remaining_lower_case())
        {
            builder.suggest(&self.literal);
            Ok(builder.build())
        } else {
            Ok(Suggestions::empty())
        }
    }
    fn get_relevant_nodes_with_source(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        self.children
            .read()
            .expect("lock")
            .get_relevant_nodes(reader, source)
    }
    fn is_valid_input(&self, input: &str) -> bool {
        // Paper: `parse(new StringReader(input), false)` — first pass only.
        let mut reader = StringReader::new(input);
        self.parse_pass(&mut reader, &self.literal) > -1
    }
    fn get_examples(&self) -> Vec<String> {
        vec![self.literal.clone()]
    }
    fn create_builder(&self) -> Box<dyn NodeBuilder<S>> {
        let mut builder =
            crate::builder::LiteralArgumentBuilder::<S>::literal(self.literal.clone());
        builder.requires(self.get_requirement());
        builder.forward(
            self.get_redirect(),
            self.get_redirect_modifier(),
            self.is_fork(),
        );
        if let Some(command) = self.get_command() {
            builder.executes(Some(command));
        }
        Box::new(builder)
    }
    fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>) {
        find_ambiguities_default(self, consumer)
    }
    fn equals(&self, other: &dyn CommandNode<S>) -> bool {
        let Some(other) = other.as_any().downcast_ref::<LiteralCommandNode<S>>() else {
            return false;
        };
        self.literal == other.literal
            && children_eq(&self.get_children(), &other.get_children())
            && command_eq(
                &self.command.read().expect("lock"),
                &other.command.read().expect("lock"),
            )
    }
    fn hash_code(&self) -> i32 {
        // Java: `result = literal.hashCode(); result = 31 * result + super.hashCode()`.
        let mut result = crate::java_hash::string_hash(&self.literal);
        let super_hash = 31_i32
            .wrapping_mul(children_hash_code(&self.get_children()))
            .wrapping_add(
                self.command
                    .read()
                    .expect("lock")
                    .as_ref()
                    .map_or(0, command_identity_hash),
            );
        result = 31_i32.wrapping_mul(result).wrapping_add(super_hash);
        result
    }
    fn to_string(&self) -> String {
        format!("<literal {}>", self.literal)
    }
    fn merge_child(&self, other: &dyn CommandNode<S>) {
        // Java: `if (node.getCommand() != null) child.command = node.getCommand();`
        if let Some(command) = other.get_command() {
            *self.command.write().expect("lock") = Some(command);
        }
        // Java: `for (grandchild : node.getChildren()) child.addChild(grandchild);`
        for child in other.get_children() {
            self.add_child(child);
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `ArgumentCommandNode<S, T>`.
pub struct ArgumentCommandNode<S, T> {
    name: String,
    type_: Arc<dyn crate::arguments::ArgumentType<T>>,
    children: RwLock<NodeChildren<S>>,
    command: RwLock<Option<Arc<dyn Command<S>>>>,
    requirement: Predicate<S>,
    redirect: Option<Arc<dyn CommandNode<S>>>,
    modifier: Option<Arc<dyn RedirectModifier<S>>>,
    forks: bool,
    custom_suggestions: Option<Arc<dyn crate::suggestion::SuggestionProvider<S>>>,
    // `fn() -> T` keeps `Send + Sync` independent of `T` (Java's `T` is a type-only
    // parameter here — the node never stores a `T`).
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<S: 'static, T: 'static> ArgumentCommandNode<S, T> {
    /// Java `ArgumentCommandNode(String, ArgumentType, Command, Predicate, CommandNode,
    /// RedirectModifier, boolean, SuggestionProvider)`.
    #[allow(clippy::too_many_arguments)] // mirrors Java's 8-parameter constructor exactly
    pub fn new(
        name: String,
        type_: Arc<dyn crate::arguments::ArgumentType<T>>,
        command: Option<Arc<dyn Command<S>>>,
        requirement: Predicate<S>,
        redirect: Option<Arc<dyn CommandNode<S>>>,
        modifier: Option<Arc<dyn RedirectModifier<S>>>,
        forks: bool,
        custom_suggestions: Option<Arc<dyn crate::suggestion::SuggestionProvider<S>>>,
    ) -> Self {
        ArgumentCommandNode {
            name,
            type_,
            children: RwLock::new(NodeChildren::new()),
            command: RwLock::new(command),
            requirement,
            redirect,
            modifier,
            forks,
            custom_suggestions,
            _marker: std::marker::PhantomData,
        }
    }

    /// Java `getName()` (concrete, so tests can call it without the trait in scope).
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Java `getType()`.
    pub fn get_type(&self) -> &Arc<dyn crate::arguments::ArgumentType<T>> {
        &self.type_
    }

    /// Java `getCustomSuggestions()` — on the subclass, not the base `CommandNode`.
    pub fn get_custom_suggestions(
        &self,
    ) -> Option<Arc<dyn crate::suggestion::SuggestionProvider<S>>> {
        self.custom_suggestions.as_ref().map(Arc::clone)
    }
}

impl<S: 'static, T: 'static + Send + Sync> CommandNode<S> for ArgumentCommandNode<S, T> {
    fn get_name(&self) -> &str {
        &self.name
    }
    fn get_usage_text(&self) -> String {
        format!("<{}>", self.name)
    }
    fn get_children(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").children.clone()
    }
    fn get_child(&self, name: &str) -> Option<Arc<dyn CommandNode<S>>> {
        self.children.read().expect("lock").get_child(name)
    }
    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.command.read().expect("lock").as_ref().map(Arc::clone)
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.redirect.as_ref().map(Arc::clone)
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }
    fn is_fork(&self) -> bool {
        self.forks
    }
    fn can_use(&self, source: &S) -> bool {
        (self.requirement)(source)
    }
    fn add_child(&self, node: Arc<dyn CommandNode<S>>) {
        self.children.write().expect("lock").add(node);
    }
    fn parse(
        &self,
        node: Arc<dyn CommandNode<S>>,
        reader: &mut StringReader,
        context_builder: &mut CommandContextBuilder<S>,
    ) -> Result<(), CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        // Java: `type.parse(reader, contextBuilder.getSource())`. The default
        // `parse_with_source` forwards to `parse`; a custom ArgumentType that reads
        // the source gets it here.
        let result = self
            .type_
            .parse_with_source(reader, context_builder.get_source())?;
        let parsed = crate::context::ParsedArgument::new(start, reader.get_cursor(), result);
        context_builder.with_argument(&self.name, parsed.clone());
        context_builder.with_node(node, parsed.get_range());
        Ok(())
    }
    fn list_suggestions(
        &self,
        context: &CommandContext<S>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>> {
        if let Some(provider) = &self.custom_suggestions {
            provider.get_suggestions(context, builder)
        } else {
            // Java: `type.listSuggestions(this.context, builder)` — the context is
            // threaded so a custom `ArgumentType` can read the command source.
            Ok(self.type_.list_suggestions(context, builder))
        }
    }
    fn get_relevant_nodes_with_source(
        &self,
        reader: &mut StringReader,
        source: Option<&S>,
    ) -> Vec<Arc<dyn CommandNode<S>>>
    where
        S: CommandSource,
    {
        self.children
            .read()
            .expect("lock")
            .get_relevant_nodes(reader, source)
    }
    fn is_valid_input(&self, input: &str) -> bool {
        let mut reader = StringReader::new(input);
        match self.type_.parse(&mut reader) {
            Ok(_) => !reader.can_read() || reader.peek() == ' ',
            Err(_) => false,
        }
    }
    fn get_examples(&self) -> Vec<String> {
        self.type_.get_examples()
    }
    fn create_builder(&self) -> Box<dyn NodeBuilder<S>> {
        let mut builder = crate::builder::RequiredArgumentBuilder::<S, T>::argument(
            self.name.clone(),
            Arc::clone(&self.type_),
        );
        builder.requires(self.get_requirement());
        builder.forward(
            self.get_redirect(),
            self.get_redirect_modifier(),
            self.is_fork(),
        );
        builder.suggests(self.get_custom_suggestions());
        if let Some(command) = self.get_command() {
            builder.executes(Some(command));
        }
        Box::new(builder)
    }
    fn find_ambiguities(&self, consumer: &dyn AmbiguityConsumer<S>) {
        find_ambiguities_default(self, consumer)
    }
    fn equals(&self, other: &dyn CommandNode<S>) -> bool {
        let Some(other) = other.as_any().downcast_ref::<ArgumentCommandNode<S, T>>() else {
            return false;
        };
        self.name == other.name
            && self.type_.type_equals(other.type_.as_ref())
            && children_eq(&self.get_children(), &other.get_children())
            && command_eq(
                &self.command.read().expect("lock"),
                &other.command.read().expect("lock"),
            )
    }
    fn hash_code(&self) -> i32 {
        // Java: `result = name.hashCode(); result = 31 * result + type.hashCode()` —
        // note ArgumentCommandNode.hashCode does NOT call super.hashCode().
        let mut result = crate::java_hash::string_hash(&self.name);
        result = 31_i32
            .wrapping_mul(result)
            .wrapping_add(self.type_.type_hash_code());
        result
    }
    fn to_string(&self) -> String {
        format!("<argument {}:{}>", self.name, self.type_.to_string())
    }
    fn merge_child(&self, other: &dyn CommandNode<S>) {
        // Java: `if (node.getCommand() != null) child.command = node.getCommand();`
        if let Some(command) = other.get_command() {
            *self.command.write().expect("lock") = Some(command);
        }
        // Java: `for (grandchild : node.getChildren()) child.addChild(grandchild);`
        for child in other.get_children() {
            self.add_child(child);
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `CommandNode.findAmbiguities(AmbiguityConsumer)` — shared by all concrete
/// nodes (the base-class body).
fn find_ambiguities_default<S: 'static>(
    node: &dyn CommandNode<S>,
    consumer: &dyn AmbiguityConsumer<S>,
) {
    let children = node.get_children();
    for child in &children {
        let mut matches: Vec<String> = Vec::new();
        for sibling in &children {
            if Arc::ptr_eq(child, sibling) {
                continue;
            }
            for input in child.get_examples() {
                if sibling.is_valid_input(&input) && !matches.contains(&input) {
                    matches.push(input);
                }
            }
            if !matches.is_empty() {
                consumer.ambiguous(node, child, sibling, &matches);
                matches.clear();
            }
        }
        child.find_ambiguities(consumer);
    }
}

/// `Command.hashCode()` — identity for the closure commands in this crate (Java
/// lambdas have identity `hashCode`), reproduced via the Arc address.
pub(crate) fn command_identity_hash<S>(command: &Arc<dyn Command<S>>) -> i32 {
    Arc::as_ptr(command) as *const () as usize as i32
}

/// `input.substring(start, end)` in UTF-16 code units.
fn substring_utf16(input: &str, start: i32, end: i32) -> String {
    let units: Vec<u16> = input.encode_utf16().collect();
    let start = i32::max(0, start) as usize;
    let end = i32::min(units.len() as i32, end) as usize;
    crate::immutable_string_reader::utf16_units_to_string(&units[start..end])
}
