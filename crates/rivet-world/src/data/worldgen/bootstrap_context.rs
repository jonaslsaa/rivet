//! Port of `net.minecraft.data.worldgen.BootstrapContext`.
//!
//! The bootstrap contract every `net.minecraft.data.worldgen` bootstrap method
//! consumes: register a value under a `ResourceKey` (with a `Lifecycle`), or
//! look up an existing registry view (`HolderGetter`).
//!
//! Java's production implementation is the anonymous `BootstrapContext` created
//! by `RegistrySetBuilder.BuildState` (the register collects into the build
//! state, duplicate keys record an error, and `lookup` falls back to the
//! `UniversalLookup` for unknown registries). `RegistrySetBuilder` is a
//! separate, later unit — this slice ports the trait surface and, as the
//! test-only seam until then, `RecordingContext`: a deterministic
//! `BootstrapContext` that records `register` calls in order and answers
//! `lookup` from the owning `RegistryAccess`.
//!
//! Translation notes:
//! - The interface is generic over the element type `T`. `register` mutates
//!   the underlying build state, so the Rust trait takes `&mut self`; `lookup`
//!   is a pure view and takes `&self`.
//! - `register` returns a `Holder<T>` (Java `Holder.Reference<T>`). The Rust
//!   `Holder::reference` carries only a `(RegistryId, id)` pair (OWNERSHIP's
//!   back-reference rule), so the concrete reference id is produced by the
//!   implementation. Java's production `register` returns
//!   `BuildState.lookup.getOrCreate(key)` — a holder keyed by `ResourceKey`, so
//!   a duplicate key returns the *same* holder. The recording context instead
//!   assigns a fresh id per call (see `RecordingContext`); the real keyed id is
//!   produced by the deferred `RegistrySetBuilder` implementation.
//! - `lookup` is Java's `<S> HolderGetter<S> lookup(ResourceKey<? extends
//!   Registry<? extends S>> key)` — generic over the *element* type `S`. The
//!   Rust signature is `fn lookup<S: Send + Sync + 'static>(&self, key:
//!   &RegistryKey<S>) -> Option<&dyn HolderGetter<S>>`. The `Option` is
//!   a documented seam deviation: Java always returns a getter (the empty
//!   `UniversalLookup` fallback for unknown registries), but an owned getter
//!   value is not constructible outside `rivet-registry` (`RegistryGetter::new`
//!   is `pub(crate)`), so the getter resolves through the access and an
//!   absent registry reports `None` — the empty answer. The deferred
//!   `RegistrySetBuilder` implementation (inside `rivet-registry`) returns
//!   `Some` for every key, matching `getOrDefault`.
//! - A `BootstrapContext` answers `lookup` for the registry it is *building*
//!   with the in-progress registrations — Java's `BuildState` universal lookup,
//!   which the `NoiseRouterData`/`NoiseGeneratorSettings` bootstraps rely on
//!   (they read back functions registered moments earlier). `RecordingContext`
//!   stores the building registry's `RegistryKey<T>` and serves the pending
//!   values as `Direct` holders (Java's `getOrCreate`: a duplicate key is a
//!   `Duplicate registration` error — unreachable in this slice — so the
//!   first registration stands).
//! - The default `register(key, value)` delegates with `Lifecycle::stable()`.

use rivet_registry::holder::{Holder, RegistryId};
use rivet_registry::registry::RegistryKey;
use rivet_registry::{HolderGetter, HolderSet, Identifier, RegistryAccess, ResourceKey, TagKey};
use rivet_serialization::lifecycle::Lifecycle;
use std::any::Any;
use std::collections::{HashMap, VecDeque};

/// `net.minecraft.data.worldgen.BootstrapContext<T>` — the registry bootstrap
/// contract.
pub trait BootstrapContext<T> {
    /// `BootstrapContext.register(ResourceKey<T>, T, Lifecycle)` — `Holder.Reference<T>`.
    fn register(&mut self, key: &ResourceKey<T>, value: T, lifecycle: Lifecycle) -> Holder<T>;

    /// `BootstrapContext.register(ResourceKey<T>, T)` — the `Lifecycle.stable()`
    /// default.
    fn register_default(&mut self, key: &ResourceKey<T>, value: T) -> Holder<T> {
        self.register(key, value, Lifecycle::stable())
    }

    /// `BootstrapContext.lookup(ResourceKey<? extends Registry<? extends S>>)`
    /// — `HolderGetter<S>` (borrowed; see the module docs for the `Option`).
    ///
    /// Java's `lookup` always returns a non-null `HolderGetter` (falling back to
    /// an empty `UniversalLookup` for unknown registries). The Rust signature is
    /// `Option<&dyn HolderGetter<S>>` — the `Option` is a documented seam
    /// deviation (an absent registry reports `None`, the empty answer; an owned
    /// getter is not constructible outside `rivet-registry`, `RegistryGetter::new`
    /// is `pub(crate)`). The deferred `RegistrySetBuilder` implementation
    /// (inside `rivet-registry`) returns `Some` for every key, matching Java's
    /// `getOrDefault`; removable when that implementation lands and the
    /// empty-fallback guarantee is reproduced.
    ///
    /// The `S: Clone` bound is the `RecordingContext` Direct-holder seam:
    /// `lookup` hands out `Holder::Direct(S)` values (owned copies), because the
    /// `Holder` back-reference model (`OWNERSHIP`) cannot share a registry-keyed
    /// reference without a real build state. Every registry element type in this
    /// slice (`NoiseParameters`, the erased `DensityFunction` carrier) is
    /// `Clone`; the deferred production implementation resolves references by id
    /// and can relax it.
    fn lookup<S: Send + Sync + Clone + 'static>(
        &self,
        key: &RegistryKey<S>,
    ) -> Option<&dyn HolderGetter<S>>;
}

/// A single recorded `register` — Java's `RegistrySetBuilder` collect-then-
/// error path records `(key, value, lifecycle)` for duplicate detection. The
/// recording context preserves the call order so consumers can assert the
/// declaration order.
#[derive(Debug, Clone)]
pub struct RecordedRegistration<T> {
    /// The registered `ResourceKey`.
    pub key: ResourceKey<T>,
    /// The registered value.
    pub value: T,
    /// The registration lifecycle.
    pub lifecycle: Lifecycle,
}

/// A test-only recording `BootstrapContext` — the seam until
/// `RegistrySetBuilder` lands.
///
/// Java's production context stores into the build state and `register`
/// returns the *keyed* holder from `UniversalLookup.getOrCreate` (a duplicate
/// key returns the same holder). This record instead stores the value and
/// returns a fresh reference id per call — a deliberate test-seam
/// simplification; the real keyed id is produced by the deferred
/// `RegistrySetBuilder` implementation. `lookup` resolves through the owning
/// `RegistryAccess`; a registry absent from the access reports `None` (Java's
/// empty `UniversalLookup` fallback).
pub struct RecordingContext<T> {
    /// The owning `RegistryId` of the registry being built (returned `register`
    /// references).
    owner: RegistryId,
    /// The registry being built — `lookup` answers this key with the
    /// in-progress registrations (Java's `BuildState` universal lookup), and
    /// every other key from the access.
    key: RegistryKey<T>,
    access: RegistryAccess,
    next_id: u32,
    registrations: VecDeque<RecordedRegistration<T>>,
    /// The in-progress keyed holders — Java's `UniversalLookup.getOrCreate`
    /// view: a value registered earlier in the same pass is visible to
    /// `lookup`; a duplicate key is a `Duplicate registration` error
    /// (unreachable here), so the first holder stands. Stored as
    /// the type-erased `PendingLookup` getter so `lookup` can borrow it
    /// directly for any requested element type.
    pending: PendingLookup,
}

impl<T> RecordingContext<T> {
    /// Build a recording context over the registry `key` (the registry being
    /// built) and a `RegistryAccess` (answers `lookup` for every other key),
    /// with the given owner `RegistryId`.
    pub fn new(owner: RegistryId, key: RegistryKey<T>, access: RegistryAccess) -> Self {
        RecordingContext {
            owner,
            key,
            access,
            next_id: 0,
            registrations: VecDeque::new(),
            pending: PendingLookup::default(),
        }
    }

    /// The `register` calls in order.
    pub fn registrations(&self) -> &VecDeque<RecordedRegistration<T>> {
        &self.registrations
    }

    /// Pop the front of the recorded registrations (in-order drain).
    pub fn pop_front(&mut self) -> Option<RecordedRegistration<T>> {
        self.registrations.pop_front()
    }
}

impl<T: Send + Sync + Clone + 'static> BootstrapContext<T> for RecordingContext<T> {
    /// RivetTodo(#126): Java's `BuildState.register` returns
    /// `lookup.getOrCreate(key)` — a holder keyed by `ResourceKey`, so a
    /// duplicate key returns the *same* holder. This recording context assigns
    /// a fresh id per call (see `RecordingContext`), a deliberate test-seam
    /// deviation; the keyed `getOrCreate` semantics land with the deferred
    /// `RegistrySetBuilder` implementation. The in-progress `pending` view is
    /// keyed (a duplicate key is a `Duplicate registration` error, unreachable
    /// here, so the first registration stands).
    fn register(&mut self, key: &ResourceKey<T>, value: T, lifecycle: Lifecycle) -> Holder<T> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        // `Holder::Direct` carries the value (no lookup needed at read time);
        // the keyed reference id lands with the deferred `RegistrySetBuilder`.
        self.pending.register(key, value.clone());
        self.registrations.push_back(RecordedRegistration {
            key: key.clone(),
            value,
            lifecycle,
        });
        Holder::reference(self.owner, id)
    }

    fn lookup<S: Send + Sync + Clone + 'static>(
        &self,
        key: &RegistryKey<S>,
    ) -> Option<&dyn HolderGetter<S>> {
        // The registry being built: Java's `BuildState` universal lookup serves
        // values registered earlier in the same pass. `PendingLookup` is
        // type-erased, so the borrow works for any requested `S`; a mismatch
        // (wrong key for `S`) yields the empty answer.
        if key.identifier() == self.key.identifier() {
            return Some(self.pending.as_getter::<S>());
        }
        self.access
            .lookup(key)
            .map(|registry| registry as &dyn HolderGetter<S>)
    }
}

/// The type-erased in-progress `HolderGetter` view — serves the values
/// registered earlier in the same bootstrap pass as `Direct` holders (Java's
/// `BuildState` universal lookup over the registry being built). Values are
/// stored erased (`Box<dyn Any>`) so the same pending map serves any requested
/// element type, downcasting per entry on read (the erased-boundary seam this
/// crate's `OWNERSHIP` model sanctions).
#[derive(Debug, Default)]
struct PendingLookup {
    entries: HashMap<Identifier, Box<dyn Any>>,
}

impl PendingLookup {
    /// Record a value under its key — the first registration stands (Java's
    /// `BuildState` collect path is last-wins for a duplicate key and records
    /// a `Duplicate registration` error; unreachable in this slice, so the
    /// pending view keeps the first value, matching `getOrCreate`'s holder
    /// identity).
    fn register<T: Any>(&mut self, key: &ResourceKey<T>, value: T) {
        self.entries
            .entry(key.identifier().clone())
            .or_insert_with(|| Box::new(value));
    }

    /// The erased-boundary `&dyn HolderGetter<S>` view over the pending map.
    ///
    /// The `Box<dyn Any>` values downcast per key on read (the erased-registry
    /// downcast seam); a `S` mismatched with the stored element type reports
    /// `None` for every key, matching Java's empty `UniversalLookup`.
    fn as_getter<S: Send + Sync + Clone + 'static>(&self) -> &dyn HolderGetter<S> {
        self
    }
}

impl<S: Send + Sync + Clone + 'static> HolderGetter<S> for PendingLookup {
    fn get(&self, key: &ResourceKey<S>) -> Option<Holder<S>> {
        let erased = self.entries.get(key.identifier())?;
        // `S: 'static`, so the type-erased box downcasts to `S` if — and only
        // if — the value was registered under this key with element type `S`
        // (the erased-boundary downcast, same soundness argument as
        // `RegistryAccess::lookup`).
        let value = erased.downcast_ref::<S>()?;
        Some(Holder::Direct(value.clone()))
    }

    fn get_tag(&self, _tag: &TagKey<S>) -> Option<HolderSet<S>> {
        // No named sets exist for in-progress registrations.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;

    #[derive(Debug, Clone, PartialEq)]
    struct Element(u8);

    fn registry_key() -> RegistryKey<Element> {
        rivet_registry::ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn key(id: &str) -> ResourceKey<Element> {
        ResourceKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    #[test]
    fn register_records_order_lifecycle_and_returns_incrementing_references() {
        let mut context = RecordingContext::<Element>::new(
            RegistryId(7),
            registry_key(),
            RegistryAccess::empty(),
        );
        let k1 = key("a");
        let k2 = key("b");
        context.register(&k1, Element(1), Lifecycle::stable());
        context.register(&k2, Element(2), Lifecycle::deprecated(3));

        let holder = context.register(&k1, Element(9), Lifecycle::experimental());
        // References are assigned per call — a fresh id even for the duplicate
        // key `k1`. (Java's `UniversalLookup.getOrCreate` is keyed: a duplicate
        // key returns the *same* holder; the deferred production impl does that.)
        assert_eq!(holder, Holder::reference(RegistryId(7), 2));

        let regs: Vec<_> = context.registrations().iter().cloned().collect();
        assert_eq!(regs.len(), 3);
        assert_eq!(regs[0].key, k1);
        assert_eq!(regs[0].value, Element(1));
        assert_eq!(regs[0].lifecycle, Lifecycle::stable());
        assert_eq!(regs[1].key, k2);
        assert_eq!(regs[1].value, Element(2));
        assert_eq!(regs[1].lifecycle, Lifecycle::deprecated(3));
        assert_eq!(regs[2].key, k1);
        assert_eq!(regs[2].value, Element(9));
        assert_eq!(regs[2].lifecycle, Lifecycle::experimental());
    }

    #[test]
    fn register_default_uses_stable_lifecycle() {
        let mut context = RecordingContext::<Element>::new(
            RegistryId(7),
            registry_key(),
            RegistryAccess::empty(),
        );
        let k = key("a");
        context.register_default(&k, Element(5));
        let reg = context.registrations().front().unwrap();
        assert_eq!(reg.lifecycle, Lifecycle::stable());
    }

    #[test]
    fn lookup_resolves_registries_in_the_access_and_is_none_for_absent() {
        let context = RecordingContext::<Element>::new(
            RegistryId(7),
            registry_key(),
            RegistryAccess::empty(),
        );
        // A key that is neither the built registry nor in the access reports
        // `None` (Java's empty UniversalLookup). The built registry key itself
        // resolves to the in-progress (pending) view, always `Some`.
        let unrelated_key: rivet_registry::ResourceKey<rivet_registry::Registry<Element>> =
            rivet_registry::ResourceKey::create_registry_key(
                rivet_registry::Identifier::with_default_namespace("unrelated"),
            );
        assert!(context.lookup(&unrelated_key).is_none());
        assert!(context.lookup(&registry_key()).is_some());

        // A registry present in the access resolves to its getter; a missing
        // element inside it is `Optional.empty`. The access registry lives under
        // a *different* key than the built registry — `lookup` serves the built
        // key with the in-progress (pending) view (Java's `BuildState`), and
        // every other key from the access.
        let other_key = rivet_registry::ResourceKey::create_registry_key(
            rivet_registry::Identifier::with_default_namespace("other"),
        );
        let mut builder = rivet_registry::RegistryBuilder::new(&other_key);
        builder.register(
            &key("a"),
            std::sync::Arc::new(Element(1)),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(other_key.clone(), registry);
        let context = RecordingContext::<Element>::new(RegistryId(7), registry_key(), access);
        let getter = context.lookup(&other_key).expect("registry present");
        assert!(getter.get(&key("a")).is_some());
        assert!(getter.get(&key("missing")).is_none());
    }

    #[test]
    fn lookup_serves_in_progress_registrations_for_the_built_registry() {
        // Java's `BuildState` universal lookup: values registered earlier in the
        // same pass are visible through `lookup` (the `NoiseRouterData`/
        // `NoiseGeneratorSettings` bootstraps read back functions this way).
        let mut context = RecordingContext::<Element>::new(
            RegistryId(7),
            registry_key(),
            RegistryAccess::empty(),
        );
        let k = key("a");
        context.register(&k, Element(1), Lifecycle::stable());
        let getter = context
            .lookup(&registry_key())
            .expect("built registry present");
        let holder = getter.get_or_throw(&k);
        // Served as a `Direct` holder carrying the registered value.
        match holder {
            Holder::Direct(value) => assert_eq!(value, Element(1)),
            other => panic!("expected a Direct in-progress holder, got {other:?}"),
        }
    }
}
