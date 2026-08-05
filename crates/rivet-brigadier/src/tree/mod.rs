//! Port of `com.mojang.brigadier.tree` package — STUB (brigadier.builder).
//!
//! // STUB(brigadier.builder): this package is being ported by the `brigadier.tree`
//! unit. Only the surface the builder cluster references (and its ported tests assert
//! on) is declared here.
//!
//! The stub stores every constructor parameter in the node structs (Java's
//! constructors store all of them), so a redirected/forked/suggests/requires-
//! constrained builder's `build()` output is preserved via
//! `get_redirect`/`is_fork`/`get_redirect_modifier`/`get_requirement`/
//! `get_custom_suggestions`. The one exception is `RootCommandNode`'s constructor
//! `RedirectModifier` — see the deferred list below.
//!
//! Deferred to the tree unit (see per-item notes):
//! - `CommandNode.addChild` merge semantics — Java merges children with equal names
//!   (inheriting command and recursively absorbing grandchildren) and throws
//!   `UnsupportedOperationException` for a `RootCommandNode` child. The builders'
//!   `then`/`then_node`/`build` path funnels through `RootCommandNode.add_child`, so
//!   they automatically gain the merge once the tree unit ports it. Porting it here
//!   would need interior mutability on the `Arc<dyn CommandNode>` children (Java
//!   mutates the stored child in place), which is the tree unit's call.
//! - `RootCommandNode`'s constructor `RedirectModifier` (`s ->
//!   Collections.singleton(s.getSource())`). Java's root `getRedirectModifier()`
//!   returns this non-null modifier; the stub returns `None`. Storing it needs an
//!   `S: Clone` bound (the modifier returns the source by value) that would ripple
//!   onto every builder, so the tree unit's real `RootCommandNode` port should
//!   store it.
//! - `parse`/`listSuggestions`, the `literals`/`arguments` name maps
//!   (`getRelevantNodes`), `equals`/`hashCode`, `createBuilder`, `canUse`,
//!   `getUsageText`, and the argument `:type` in `to_string`.

use std::sync::Arc;

use crate::builder::Predicate;

/// Java `CommandNode<S>` — the abstract tree node. `dyn` object (Java abstract
/// class); the concrete nodes implement it.
///
/// // STUB(brigadier.builder): full method surface is the `brigadier.tree` unit.
pub trait CommandNode<S>: Send + Sync {
    /// Java `getName()`.
    fn get_name(&self) -> &str;
    /// Java `getChildren()`.
    fn get_children(&self) -> &[Arc<dyn CommandNode<S>>];
    /// Java `getCommand()`.
    fn get_command(&self) -> Option<Arc<dyn crate::command::Command<S>>>;
    /// Java `getRedirect()`.
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>>;
    /// Java `getRequirement()`.
    fn get_requirement(&self) -> Predicate<S>;
    /// Java `getRedirectModifier()`.
    fn get_redirect_modifier(
        &self,
    ) -> Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>>;
    /// Java `isFork()`.
    fn is_fork(&self) -> bool;
    /// Java `toString()` — rendered per concrete node (`<literal ...>` /
    /// `<argument ...>`).
    fn to_string(&self) -> String;
    /// Downcast helper for the tests (`instanceof` in Java tests).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Java `RootCommandNode<S>` — holds a builder's child nodes (`getChildren`,
/// `addChild`). The builder stores its `arguments` here.
///
/// // STUB(brigadier.builder): full port is the `brigadier.tree` unit; this is the
/// minimal surface the builders require.
///
/// `addChild` here is a plain push. Java's `CommandNode.addChild` merges duplicate
/// names and rejects `RootCommandNode` children (see the module doc); that merge is
/// deferred to the tree unit.
///
/// Java's root constructor also stores a `RedirectModifier` (`s ->
/// Collections.singleton(s.getSource())`) that `getRedirectModifier` returns; the
/// stub drops it (needs `S: Clone` — see the module doc's deferred list).
pub struct RootCommandNode<S> {
    children: Vec<Arc<dyn CommandNode<S>>>,
    // Java root constructor passes `c -> true` (a distinct lambda, not the builder's
    // defaultRequirement).
    requirement: Predicate<S>,
}

impl<S> RootCommandNode<S> {
    /// Java `RootCommandNode()`.
    pub fn new() -> Self {
        RootCommandNode {
            children: Vec::new(),
            requirement: Arc::new(|_: &S| true),
        }
    }

    /// Java `getChildren()`.
    pub fn get_children(&self) -> &[Arc<dyn CommandNode<S>>] {
        &self.children
    }

    /// Java `addChild(CommandNode<S>)` (inherited from `CommandNode`) — the builders'
    /// `then`/`then_node` funnel through here, and `build()` re-adds these children
    /// to the built node.
    pub fn add_child(&mut self, node: Arc<dyn CommandNode<S>>) {
        // STUB(brigadier.builder): Java's `CommandNode.addChild` merges equal names
        // and rejects `RootCommandNode` children; deferred to the tree unit (see
        // module doc).
        self.children.push(node);
    }
}

impl<S> Default for RootCommandNode<S> {
    fn default() -> Self {
        RootCommandNode::new()
    }
}

impl<S: 'static> CommandNode<S> for RootCommandNode<S> {
    fn get_name(&self) -> &str {
        ""
    }
    fn get_children(&self) -> &[Arc<dyn CommandNode<S>>] {
        &self.children
    }
    fn get_command(&self) -> Option<Arc<dyn crate::command::Command<S>>> {
        None
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        None
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(
        &self,
    ) -> Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>> {
        None
    }
    fn is_fork(&self) -> bool {
        false
    }
    fn to_string(&self) -> String {
        "<root>".to_string()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `LiteralCommandNode<S>` — the node `LiteralArgumentBuilder.build()` creates.
///
/// // STUB(brigadier.builder): full port is the `brigadier.tree` unit; the builder
/// needs the Java-matching constructor (all parameters stored), `addChild`, and the
/// test getters.
pub struct LiteralCommandNode<S> {
    literal: String,
    command: Option<Arc<dyn crate::command::Command<S>>>,
    requirement: Predicate<S>,
    redirect: Option<Arc<dyn CommandNode<S>>>,
    modifier: Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>>,
    forks: bool,
    children: Vec<Arc<dyn CommandNode<S>>>,
}

impl<S: 'static> LiteralCommandNode<S> {
    /// Java `LiteralCommandNode(String, Command, Predicate, CommandNode, RedirectModifier, boolean)`.
    pub fn new(
        literal: String,
        command: Option<Arc<dyn crate::command::Command<S>>>,
        requirement: Predicate<S>,
        redirect: Option<Arc<dyn CommandNode<S>>>,
        modifier: Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>>,
        forks: bool,
    ) -> Self {
        LiteralCommandNode {
            literal,
            command,
            requirement,
            redirect,
            modifier,
            forks,
            children: Vec::new(),
        }
    }

    /// Java `getLiteral()`.
    pub fn get_literal(&self) -> &str {
        &self.literal
    }

    /// Java `addChild(CommandNode<S>)` (inherited from `CommandNode`).
    pub fn add_child(&mut self, node: Arc<dyn CommandNode<S>>) {
        // STUB(brigadier.builder): Java's `CommandNode.addChild` merges equal names;
        // deferred to the tree unit (see module doc).
        self.children.push(node);
    }
}

impl<S: 'static> CommandNode<S> for LiteralCommandNode<S> {
    fn get_name(&self) -> &str {
        &self.literal
    }
    fn get_children(&self) -> &[Arc<dyn CommandNode<S>>] {
        &self.children
    }
    fn get_command(&self) -> Option<Arc<dyn crate::command::Command<S>>> {
        self.command.as_ref().map(Arc::clone)
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.redirect.as_ref().map(Arc::clone)
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(
        &self,
    ) -> Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }
    fn is_fork(&self) -> bool {
        self.forks
    }
    fn to_string(&self) -> String {
        format!("<literal {}>", self.literal)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `ArgumentCommandNode<S, T>` — the node `RequiredArgumentBuilder.build()` creates.
///
/// // STUB(brigadier.builder): full port is the `brigadier.tree` unit; the builder
/// needs the Java-matching constructor (all parameters stored), `addChild`, and the
/// test getters. `to_string` renders `<argument name>`; Java is `<argument name:type>`
/// — the `:type` suffix is dropped because the stub `ArgumentType` trait carries no
/// rendering surface. The tree unit's real port will include it.
pub struct ArgumentCommandNode<S, T> {
    name: String,
    type_: Arc<dyn crate::arguments::ArgumentType<T>>,
    command: Option<Arc<dyn crate::command::Command<S>>>,
    requirement: Predicate<S>,
    redirect: Option<Arc<dyn CommandNode<S>>>,
    modifier: Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>>,
    forks: bool,
    custom_suggestions: Option<Arc<dyn crate::suggestion::SuggestionProvider<S>>>,
    children: Vec<Arc<dyn CommandNode<S>>>,
    // `fn() -> T` keeps `Send + Sync` independent of `T` (Java's `T` is a type-only
    // parameter here — the node never stores a `T`).
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<S: 'static, T: 'static> ArgumentCommandNode<S, T> {
    /// Java `ArgumentCommandNode(String, ArgumentType, Command, Predicate, CommandNode, RedirectModifier, boolean, SuggestionProvider)`.
    #[allow(clippy::too_many_arguments)] // mirrors Java's 8-parameter constructor exactly
    pub fn new(
        name: String,
        type_: Arc<dyn crate::arguments::ArgumentType<T>>,
        command: Option<Arc<dyn crate::command::Command<S>>>,
        requirement: Predicate<S>,
        redirect: Option<Arc<dyn CommandNode<S>>>,
        modifier: Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>>,
        forks: bool,
        custom_suggestions: Option<Arc<dyn crate::suggestion::SuggestionProvider<S>>>,
    ) -> Self {
        ArgumentCommandNode {
            name,
            type_,
            command,
            requirement,
            redirect,
            modifier,
            forks,
            custom_suggestions,
            children: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Java `getName()`.
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

    /// Java `addChild(CommandNode<S>)` (inherited from `CommandNode`).
    pub fn add_child(&mut self, node: Arc<dyn CommandNode<S>>) {
        // STUB(brigadier.builder): Java's `CommandNode.addChild` merges equal names;
        // deferred to the tree unit (see module doc).
        self.children.push(node);
    }
}

impl<S: 'static, T: 'static> CommandNode<S> for ArgumentCommandNode<S, T> {
    fn get_name(&self) -> &str {
        &self.name
    }
    fn get_children(&self) -> &[Arc<dyn CommandNode<S>>] {
        &self.children
    }
    fn get_command(&self) -> Option<Arc<dyn crate::command::Command<S>>> {
        self.command.as_ref().map(Arc::clone)
    }
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.redirect.as_ref().map(Arc::clone)
    }
    fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }
    fn get_redirect_modifier(
        &self,
    ) -> Option<Arc<dyn crate::redirect_modifier::RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }
    fn is_fork(&self) -> bool {
        self.forks
    }
    fn to_string(&self) -> String {
        format!("<argument {}>", self.name)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
