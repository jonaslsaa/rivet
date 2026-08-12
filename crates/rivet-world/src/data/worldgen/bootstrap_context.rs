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
//!   &RegistryKey<S>) -> Option<&dyn HolderGetter<S>>`. The `Option` is a
//!   documented seam deviation: Java always returns a getter (the empty
//!   `UniversalLookup` fallback for unknown registries), but an owned getter
//!   value is not constructible outside `rivet-registry` (`RegistryGetter::new`
//!   is `pub(crate)`), so the borrow is resolved through the access and an
//!   absent registry reports `None` — the empty answer. The deferred
//!   `RegistrySetBuilder` implementation (inside `rivet-registry`) returns
//!   `Some` for every key, matching `getOrDefault`.
//! - The default `register(key, value)` delegates with `Lifecycle::stable()`.

use rivet_registry::holder::{Holder, RegistryId};
use rivet_registry::registry::RegistryKey;
use rivet_registry::{HolderGetter, RegistryAccess, ResourceKey};
use rivet_serialization::lifecycle::Lifecycle;
use std::collections::VecDeque;

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
    /// RivetTodo(#126): Java's `lookup` always returns a non-null `HolderGetter`
    /// (falling back to an empty `UniversalLookup` for unknown registries), but
    /// this trait returns `Option<&dyn HolderGetter<S>>` — an owned getter is
    /// not constructible outside `rivet-registry` (`RegistryGetter::new` is
    /// `pub(crate)`), so the borrow resolves through the `RegistryAccess` and an
    /// absent registry reports `None`. The deferred `RegistrySetBuilder`
    /// implementation (inside `rivet-registry`) returns `Some` for every key,
    /// matching Java's `getOrDefault`; removable when that implementation lands
    /// and the empty-fallback guarantee is reproduced.
    fn lookup<S: Send + Sync + 'static>(
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
    owner: RegistryId,
    access: RegistryAccess,
    next_id: u32,
    registrations: VecDeque<RecordedRegistration<T>>,
}

impl<T> RecordingContext<T> {
    /// Build a recording context over a `RegistryAccess` (answers `lookup`)
    /// with the given owner `RegistryId`.
    pub fn new(owner: RegistryId, access: RegistryAccess) -> Self {
        RecordingContext {
            owner,
            access,
            next_id: 0,
            registrations: VecDeque::new(),
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

impl<T: Send + Sync + 'static> BootstrapContext<T> for RecordingContext<T> {
    /// RivetTodo(#126): Java's `BuildState.register` returns
    /// `lookup.getOrCreate(key)` — a holder keyed by `ResourceKey`, so a
    /// duplicate key returns the *same* holder. This recording context assigns
    /// a fresh id per call (see `RecordingContext`), a deliberate test-seam
    /// deviation; the keyed `getOrCreate` semantics land with the deferred
    /// `RegistrySetBuilder` implementation.
    fn register(&mut self, key: &ResourceKey<T>, value: T, lifecycle: Lifecycle) -> Holder<T> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.registrations.push_back(RecordedRegistration {
            key: key.clone(),
            value,
            lifecycle,
        });
        Holder::reference(self.owner, id)
    }

    fn lookup<S: Send + Sync + 'static>(
        &self,
        key: &RegistryKey<S>,
    ) -> Option<&dyn HolderGetter<S>> {
        self.access
            .lookup(key)
            .map(|registry| registry as &dyn HolderGetter<S>)
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
        let mut context = RecordingContext::<Element>::new(RegistryId(7), RegistryAccess::empty());
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
        let mut context = RecordingContext::<Element>::new(RegistryId(7), RegistryAccess::empty());
        let k = key("a");
        context.register_default(&k, Element(5));
        let reg = context.registrations().front().unwrap();
        assert_eq!(reg.lifecycle, Lifecycle::stable());
    }

    #[test]
    fn lookup_resolves_registries_in_the_access_and_is_none_for_absent() {
        let context = RecordingContext::<Element>::new(RegistryId(7), RegistryAccess::empty());
        // An absent registry reports `None` (Java's empty UniversalLookup).
        assert!(context.lookup(&registry_key()).is_none());

        // A registry present in the access resolves to its getter; a missing
        // element inside it is `Optional.empty`.
        let mut builder = rivet_registry::RegistryBuilder::new(&registry_key());
        builder.register(
            &key("a"),
            std::sync::Arc::new(Element(1)),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(registry_key(), registry);
        let context = RecordingContext::<Element>::new(RegistryId(7), access);
        let getter = context.lookup(&registry_key()).expect("registry present");
        assert!(getter.get(&key("a")).is_some());
        assert!(getter.get(&key("missing")).is_none());
    }
}
