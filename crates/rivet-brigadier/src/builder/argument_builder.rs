//! Port of the Paper-patched `com.mojang.brigadier.builder.ArgumentBuilder` (upstream
//! class plus the Paper `defaultRequirement()` addition).
//!
//! Java's self-type `T extends ArgumentBuilder<S, T>` (CRTP) makes every fluent method
//! return the concrete builder. In Rust the base fields live in the embedded
//! `ArgumentBuilder<S>` struct, and the fluent surface is the object-safe
//! `ArgumentBuilderBehavior<S>` trait: its methods return `&mut Self` (Java's
//! `getThis()`) and dispatch the mutations through `base_mut()`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::command::Command;
use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::redirect_modifier::RedirectModifier;
use crate::single_redirect_modifier::SingleRedirectModifier;
use crate::tree::{CommandNode, RootCommandNode};

/// Java `java.util.function.Predicate<S>` — the `requirement` test.
pub type Predicate<S> = Arc<dyn Fn(&S) -> bool + Send + Sync>;

/// Java `ArgumentBuilder<S, T>` — the base-class fields.
pub struct ArgumentBuilder<S> {
    arguments: RootCommandNode<S>,
    command: Option<Arc<dyn Command<S>>>,
    requirement: Predicate<S>,
    target: Option<Arc<dyn CommandNode<S>>>,
    modifier: Option<Arc<dyn RedirectModifier<S>>>,
    forks: bool,
}

impl<S: 'static> ArgumentBuilder<S> {
    /// Java `ArgumentBuilder()` — implicit constructor; `requirement` starts as
    /// `defaultRequirement()` (Paper).
    pub fn new() -> Self {
        ArgumentBuilder {
            arguments: RootCommandNode::new(),
            command: None,
            requirement: Self::default_requirement::<S>(),
            target: None,
            modifier: None,
            forks: false,
        }
    }

    /// Java `ArgumentBuilder.defaultRequirement()` (Paper) — the shared
    /// `Predicate<Object>` `s -> true`, downcast per `S`.
    ///
    /// Paper's "Vanilla command permission fixes" (`Commands.java:312`) compares the
    /// node requirement to the default by **identity** (`node.getRequirement() ==
    /// defaultRequirement()`). Java guarantees one shared static instance, so a
    /// `node.getRequirement()` from any builder `new()` *is* the default instance.
    /// This returns one lazily-allocated shared closure per `S` so that
    /// `Arc::ptr_eq(node.get_requirement(), default_requirement())` matches Java's
    /// `==` — every builder-initialized node's requirement is the default instance.
    pub fn default_requirement<U: 'static>() -> Predicate<U> {
        let type_id = TypeId::of::<U>();
        let cache = DEFAULT_REQUIREMENTS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache.lock().unwrap();
        cache
            .entry(type_id)
            .or_insert_with(|| Arc::new(DefaultRequirement::<U>(Arc::new(|_: &U| true))))
            .clone()
            .downcast::<DefaultRequirement<U>>()
            .unwrap()
            .0
            .clone()
    }

    /// Java `getArguments()` — `arguments.getChildren()`.
    pub fn get_arguments(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        self.arguments.get_children()
    }

    /// Java `getCommand()`.
    pub fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.command.as_ref().map(Arc::clone)
    }

    /// Java `getRequirement()`.
    pub fn get_requirement(&self) -> Predicate<S> {
        Arc::clone(&self.requirement)
    }

    /// Java `getRedirect()`.
    pub fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.target.as_ref().map(Arc::clone)
    }

    /// Java `getRedirectModifier()`.
    pub fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.modifier.as_ref().map(Arc::clone)
    }

    /// Java `isFork()`.
    pub fn is_fork(&self) -> bool {
        self.forks
    }
}

impl<S: 'static> Default for ArgumentBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Java `ArgumentBuilder`'s fluent methods (`then`, `executes`, `requires`,
/// `redirect`, `fork`, `forward`) and the abstract `build()`, exposed on the concrete
/// builders. Java's protected `getThis()` returns the concrete `T`; here each fluent
/// method returns `&mut Self`.
pub trait ArgumentBuilderBehavior<S: 'static> {
    fn base(&self) -> &ArgumentBuilder<S>;
    fn base_mut(&mut self) -> &mut ArgumentBuilder<S>;
    /// Java `build()`.
    fn build(&self) -> Box<dyn CommandNode<S>>;

    /// Java `getArguments()` (inherited public getter).
    fn get_arguments(&self) -> Vec<Arc<dyn CommandNode<S>>> {
        self.base().get_arguments()
    }

    /// Java `getCommand()` (inherited public getter).
    fn get_command(&self) -> Option<Arc<dyn Command<S>>> {
        self.base().get_command()
    }

    /// Java `getRequirement()` (inherited public getter).
    fn get_requirement(&self) -> Predicate<S> {
        self.base().get_requirement()
    }

    /// Java `getRedirect()` (inherited public getter).
    fn get_redirect(&self) -> Option<Arc<dyn CommandNode<S>>> {
        self.base().get_redirect()
    }

    /// Java `getRedirectModifier()` (inherited public getter).
    fn get_redirect_modifier(&self) -> Option<Arc<dyn RedirectModifier<S>>> {
        self.base().get_redirect_modifier()
    }

    /// Java `isFork()` (inherited public getter).
    fn is_fork(&self) -> bool {
        self.base().is_fork()
    }

    /// Java `then(ArgumentBuilder<S, ?>)`. Java takes the abstract builder type; here
    /// it's generic over the concrete builder, calling only `build()` on it.
    fn then(&mut self, argument: impl ArgumentBuilderBehavior<S>) -> &mut Self {
        if self.base().target.is_some() {
            panic!("Cannot add children to a redirected node");
        }
        self.base_mut()
            .arguments
            .add_child(Arc::from(argument.build()));
        self
    }

    /// Java `then(CommandNode<S>)`.
    fn then_node(&mut self, argument: Arc<dyn CommandNode<S>>) -> &mut Self {
        if self.base().target.is_some() {
            panic!("Cannot add children to a redirected node");
        }
        self.base_mut().arguments.add_child(argument);
        self
    }

    /// Java `executes(Command<S>)` — `this.command = command`; Java's legal
    /// `executes(null)` (clearing the command) is modelled by `None`.
    fn executes(&mut self, command: Option<Arc<dyn Command<S>>>) -> &mut Self {
        self.base_mut().command = command;
        self
    }

    /// Java `requires(Predicate<S>)`.
    fn requires(&mut self, requirement: Predicate<S>) -> &mut Self {
        self.base_mut().requirement = requirement;
        self
    }

    /// Java `redirect(CommandNode<S>)`.
    fn redirect(&mut self, target: Arc<dyn CommandNode<S>>) -> &mut Self {
        self.forward(Some(target), None, false)
    }

    /// Java `redirect(CommandNode<S>, SingleRedirectModifier<S>)`.
    fn redirect_with_modifier(
        &mut self,
        target: Arc<dyn CommandNode<S>>,
        modifier: Option<Arc<dyn SingleRedirectModifier<S>>>,
    ) -> &mut Self {
        let modifier = modifier.map(|m| {
            let adapter: Arc<dyn RedirectModifier<S>> =
                Arc::new(SingleRedirectModifierAdapter { inner: m });
            adapter
        });
        self.forward(Some(target), modifier, false)
    }

    /// Java `fork(CommandNode<S>, RedirectModifier<S>)`.
    fn fork(
        &mut self,
        target: Arc<dyn CommandNode<S>>,
        modifier: Arc<dyn RedirectModifier<S>>,
    ) -> &mut Self {
        self.forward(Some(target), Some(modifier), true)
    }

    /// Java `forward(CommandNode<S>, RedirectModifier<S>, boolean)`. Java passes
    /// `getRedirect()` which may be null (a node without a redirect); `target` is
    /// the nullable `Option` equivalent.
    fn forward(
        &mut self,
        target: Option<Arc<dyn CommandNode<S>>>,
        modifier: Option<Arc<dyn RedirectModifier<S>>>,
        fork: bool,
    ) -> &mut Self {
        if !self.base().get_arguments().is_empty() {
            panic!("Cannot forward a node with children");
        }
        let base = self.base_mut();
        base.target = target;
        base.modifier = modifier;
        base.forks = fork;
        self
    }
}

/// Java `redirect(CommandNode, SingleRedirectModifier)`'s wrapping closure
/// `o -> Collections.singleton(modifier.apply(o))` — a `RedirectModifier` that runs a
/// `SingleRedirectModifier` and wraps the single result.
struct SingleRedirectModifierAdapter<S> {
    inner: Arc<dyn SingleRedirectModifier<S>>,
}

impl<S: 'static> RedirectModifier<S> for SingleRedirectModifierAdapter<S> {
    fn apply(
        &self,
        context: &CommandContext<S>,
    ) -> Result<Vec<S>, CommandSyntaxException<'static>> {
        self.inner.apply(context).map(|s| vec![s])
    }
}

/// Java `ArgumentBuilder.DEFAULT_REQUIREMENT` (Paper) — one shared `Predicate<Object>`
/// `s -> true`, downcast per `S` in `defaultRequirement()`. Keyed by the downcast
/// `TypeId` of `S` so every call for a given `S` yields the same `Arc` allocation and
/// `Arc::ptr_eq` reproduces Java's `==` identity check in `Commands.java:312`.
static DEFAULT_REQUIREMENTS: OnceLock<Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>> =
    OnceLock::new();

/// The per-`S` default requirement closure, boxed as a concrete type so it can be
/// stored in the `dyn Any` cache and recovered by `downcast` (a bare closure type is
/// unnameable).
struct DefaultRequirement<S>(Arc<dyn Fn(&S) -> bool + Send + Sync>);
