//! Port of `net.minecraft.resources.RegistryOps<T>` + `DelegatingOps<T>` (MC 26.2).
//!
//! PROVENANCE: `RegistryOps.java` (139 lines) and `DelegatingOps.java` (363
//! lines), both leaves of the `mc.resources` manifest unit.
//!
//! #124 scope (ownership D — serialization context): the `DelegatingOps` base,
//! `RegistryOps<T>`, `RegistryInfo<T>`, `RegistryInfoLookup`,
//! `HolderLookupAdapter`, and the `retrieve_getter`/`retrieve_element` context
//! codecs. **#126 (holder codecs)**: the holder-view placeholders
//! (`HolderOwner<T>`/`HolderGetter<T>` structs) are replaced by the real
//! `holder_lookup` views (`RegistryOwner`/`RegistryGetter<E>`), and
//! `retrieve_element` now returns `Holder.Reference<E>` (a `Holder<E>`) instead
//! of the narrowed element value. `RegistryFileCodec`/`HolderSetCodec`/
//! `RegistryFixedCodec` and all protocol `StreamCodec`s are #126 and live in
//! `registry_file_codec.rs` / `rivet-protocol` respectively — `rivet-registry`
//! never depends on `rivet-protocol`. `RegistryDataLoader`
//! (`net.minecraft.server.packs.resources.RegistryDataLoader`) is a server
//! pack-loading class, *not* part of the #126 holder codecs, and is deferred
//! with its owning unit.
//!
//! Binding-model deviations (documented, see PORTING.md drift checklist):
//! - Rust `DynamicOps` is not object-safe, so `RegistryOps<T, D>` is generic
//!   over the concrete delegate ops `D: DynamicOps<Output = T> + Clone` (stored
//!   by value). Java's `RegistryOps<T> extends DelegatingOps<T>` reference model
//!   cannot be expressed; `JsonOps`/`NbtOps` are Copy singletons so the `Clone`
//!   bound is free in practice.
//! - `RegistryOps` does not implement `PartialEq`/`Hash` (Java `equals`/
//!   `hashCode` compare the delegate ops and the lookup): the ops are not
//!   `PartialEq` and `RegistryInfoLookup` is a trait object. The only Java
//!   consumer of `equals` is `DelegatingOps.convertTo`'s
//!   `Objects.equals(outOps, delegate)` identity shortcut, which is likewise not
//!   portable (see `convert_to`).
//! - Java's `retrieveGetter`/`retrieveElement` `ops instanceof RegistryOps<?>`
//!   guard becomes a compile-time bound (`RegistryOpsLookup`): a context codec
//!   is built for a concrete `RegistryOps` and can only be used with one.
//! - The `RegistryInfoLookup::lookup_erased` erasure (`RegistryKey<E>` →
//!   `RegistryKey<()>`) re-creates the erased key from `key.identifier()`
//!   (ownership A's `ResourceKey` is fully implemented). The codec cores are
//!   exposed as `retrieve_getter_for_erased`/`retrieve_element_for_erased` so
//!   tests exercise the real decode path with a pre-erased key; the public
//!   wrappers add the `identifier()`/`registry()` erasure.

use crate::ResourceKey;
use crate::access::RegistryAccess;
use crate::holder::{Holder, RegistryId};
use crate::holder_lookup::{HolderGetter, RegistryGetter, RegistryOwner};
use crate::registry::RegistryKey;

use rivet_serialization::Number;
use rivet_serialization::Pair;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, ListBuilder, MapLike, RecordBuilder};
use rivet_serialization::extra_codecs;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec::MapCodec;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// DelegatingOps
// ---------------------------------------------------------------------------

/// `DelegatingOps<T>` — the delegating `DynamicOps` base.
///
/// Java's abstract `DelegatingOps<T> implements DynamicOps<T>` forwards every
/// method to `protected final DynamicOps<T> delegate`, and `mapBuilder()`/
/// `listBuilder()` wrap the delegate's builders in `DelegateRecordBuilder`/
/// `DelegateListBuilder` whose `ops()` is the *outer* ops. Rust `DynamicOps` is
/// not object-safe, so the delegate is stored by value (`D`) and `Self` is a
/// concrete `DelegatingOps<T, D>`. The `Output` type parameter `T` appears only
/// in the phantom (the delegate carries the element type via `D::Output = T`).
pub struct DelegatingOps<T, D: DynamicOps<Output = T>> {
    pub(crate) delegate: D,
    _marker: PhantomData<fn() -> T>,
}

impl<T, D: DynamicOps<Output = T>> std::fmt::Debug for DelegatingOps<T, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DelegatingOps")
            .field(&self.delegate)
            .finish()
    }
}

impl<T, D: DynamicOps<Output = T>> DelegatingOps<T, D> {
    /// `DelegatingOps(DynamicOps<T> delegate)`.
    pub fn new(delegate: D) -> Self {
        DelegatingOps {
            delegate,
            _marker: PhantomData,
        }
    }
}

impl<T, D> DynamicOps for DelegatingOps<T, D>
where
    T: Debug + Clone + PartialEq,
    D: DynamicOps<Output = T>,
{
    type Output = T;

    fn empty(&self) -> T {
        self.delegate.empty()
    }

    fn empty_map(&self) -> T {
        self.delegate.empty_map()
    }

    fn empty_list(&self) -> T {
        self.delegate.empty_list()
    }

    /// `DelegatingOps.convertTo(DynamicOps<U>, T)`.
    ///
    /// Java short-circuits with `Objects.equals(outOps, this.delegate) ? input :
    /// this.delegate.convertTo(outOps, input)`; Rust ops are not `PartialEq`, so
    /// the port always delegates. Observationally equivalent: converting a value
    /// through the same ops instance is a structural no-op for the value-based
    /// ops in this crate.
    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &T) -> U::Output {
        self.delegate.convert_to(out_ops, input)
    }

    fn get_number_value(&self, input: &T) -> DataResult<Number> {
        self.delegate.get_number_value(input)
    }

    fn create_numeric(&self, value: Number) -> T {
        self.delegate.create_numeric(value)
    }

    fn create_byte(&self, value: i8) -> T {
        self.delegate.create_byte(value)
    }

    fn create_short(&self, value: i16) -> T {
        self.delegate.create_short(value)
    }

    fn create_int(&self, value: i32) -> T {
        self.delegate.create_int(value)
    }

    fn create_long(&self, value: i64) -> T {
        self.delegate.create_long(value)
    }

    fn create_float(&self, value: f32) -> T {
        self.delegate.create_float(value)
    }

    fn create_double(&self, value: f64) -> T {
        self.delegate.create_double(value)
    }

    fn get_boolean_value(&self, input: &T) -> DataResult<bool> {
        self.delegate.get_boolean_value(input)
    }

    fn create_boolean(&self, value: bool) -> T {
        self.delegate.create_boolean(value)
    }

    fn get_string_value(&self, input: &T) -> DataResult<String> {
        self.delegate.get_string_value(input)
    }

    fn create_string(&self, value: String) -> T {
        self.delegate.create_string(value)
    }

    fn merge_to_list(&self, list: &T, value: T) -> DataResult<T> {
        self.delegate.merge_to_list(list, value)
    }

    fn merge_to_map(&self, map: &T, key: T, value: T) -> DataResult<T> {
        self.delegate.merge_to_map(map, key, value)
    }

    fn merge_to_primitive(&self, prefix: &T, value: T) -> DataResult<T> {
        self.delegate.merge_to_primitive(prefix, value)
    }

    fn get_map_values(&self, input: &T) -> DataResult<Vec<Pair<T, T>>> {
        self.delegate.get_map_values(input)
    }

    fn get_map_entries(&self, input: &T) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&T, &T))>> {
        self.delegate.get_map_entries(input)
    }

    fn create_map(&self, map: Vec<Pair<T, T>>) -> T {
        self.delegate.create_map(map)
    }

    fn get_map(&self, input: &T) -> DataResult<Box<dyn MapLike<T>>> {
        self.delegate.get_map(input)
    }

    fn get_stream(&self, input: &T) -> DataResult<Vec<T>> {
        self.delegate.get_stream(input)
    }

    fn get_list(&self, input: &T) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&T))>> {
        self.delegate.get_list(input)
    }

    fn create_list(&self, input: Vec<T>) -> T {
        self.delegate.create_list(input)
    }

    fn get_byte_buffer(&self, input: &T) -> DataResult<Vec<u8>> {
        self.delegate.get_byte_buffer(input)
    }

    fn create_byte_list(&self, input: &[u8]) -> T {
        self.delegate.create_byte_list(input)
    }

    fn get_int_stream(&self, input: &T) -> DataResult<Vec<i32>> {
        self.delegate.get_int_stream(input)
    }

    fn create_int_list(&self, input: Vec<i32>) -> T {
        self.delegate.create_int_list(input)
    }

    fn get_long_stream(&self, input: &T) -> DataResult<Vec<i64>> {
        self.delegate.get_long_stream(input)
    }

    fn create_long_list(&self, input: Vec<i64>) -> T {
        self.delegate.create_long_list(input)
    }

    fn remove(&self, input: T, key: &str) -> T {
        self.delegate.remove(input, key)
    }

    fn compress_maps(&self) -> bool {
        self.delegate.compress_maps()
    }

    /// `DelegatingOps.listBuilder()` — wraps the delegate's list builder.
    fn list_builder(&self) -> Box<dyn ListBuilder<Output = T> + '_> {
        Box::new(DelegateListBuilder::new(self.delegate.list_builder()))
    }

    /// `DelegatingOps.mapBuilder()` — wraps the delegate's record builder.
    fn map_builder(&self) -> Box<dyn RecordBuilder<Output = T> + '_> {
        Box::new(DelegateRecordBuilder::new(self.delegate.map_builder()))
    }
}

/// `DelegatingOps.DelegateListBuilder` — wraps the delegate's `ListBuilder`.
///
/// Java's wrapper also overrides `ops()` to return the *outer* ops (so encoders
/// run against the context-preserving ops). The Rust `ListBuilder` trait has no
/// `ops()` method and its `add` methods take pre-encoded values, so the wrapper
/// is behaviorally transparent; it is kept to mirror the Java structure.
pub struct DelegateListBuilder<'a, T> {
    original: Box<dyn ListBuilder<Output = T> + 'a>,
}

impl<'a, T> DelegateListBuilder<'a, T> {
    fn new(original: Box<dyn ListBuilder<Output = T> + 'a>) -> Self {
        DelegateListBuilder { original }
    }
}

impl<'a, T> std::fmt::Debug for DelegateListBuilder<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DelegateListBuilder")
    }
}

impl<'a, T> ListBuilder for DelegateListBuilder<'a, T> {
    type Output = T;

    fn add(&mut self, value: T) {
        self.original.add(value);
    }

    fn add_result(&mut self, value: DataResult<T>) {
        self.original.add_result(value);
    }

    fn with_errors_from(&mut self, result: &DataResult<()>) {
        self.original.with_errors_from(result);
    }

    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        self.original.map_error(on_error);
    }

    fn build(&mut self, prefix: T) -> DataResult<T> {
        self.original.build(prefix)
    }

    fn build_result(&mut self, prefix: DataResult<T>) -> DataResult<T> {
        self.original.build_result(prefix)
    }
}

/// `DelegatingOps.DelegateRecordBuilder` — wraps the delegate's `RecordBuilder`.
///
/// See `DelegateListBuilder` for why the Rust wrapper is behaviorally
/// transparent (the `RecordBuilder` trait has no `ops()` method).
pub struct DelegateRecordBuilder<'a, T> {
    original: Box<dyn RecordBuilder<Output = T> + 'a>,
}

impl<'a, T> DelegateRecordBuilder<'a, T> {
    fn new(original: Box<dyn RecordBuilder<Output = T> + 'a>) -> Self {
        DelegateRecordBuilder { original }
    }
}

impl<'a, T> std::fmt::Debug for DelegateRecordBuilder<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DelegateRecordBuilder")
    }
}

impl<'a, T> RecordBuilder for DelegateRecordBuilder<'a, T> {
    type Output = T;

    fn build(&mut self, prefix: Option<T>) -> DataResult<T> {
        self.original.build(prefix)
    }

    fn build_result(&mut self, prefix: DataResult<T>) -> DataResult<T> {
        self.original.build_result(prefix)
    }

    fn add(&mut self, key: T, value: T) {
        self.original.add(key, value);
    }

    fn add_string(&mut self, key: &str, value: T) {
        self.original.add_string(key, value);
    }

    fn add_result(&mut self, key: T, value: DataResult<T>) {
        self.original.add_result(key, value);
    }

    fn add_result_result(&mut self, key: DataResult<T>, value: DataResult<T>) {
        self.original.add_result_result(key, value);
    }

    fn add_string_result(&mut self, key: &str, value: DataResult<T>) {
        self.original.add_string_result(key, value);
    }

    fn with_errors_from(&mut self, result: &DataResult<()>) {
        self.original.with_errors_from(result);
    }

    fn set_lifecycle(&mut self, lifecycle: Lifecycle) {
        self.original.set_lifecycle(lifecycle);
    }

    fn map_error(&mut self, on_error: Box<dyn Fn(String) -> String>) {
        self.original.map_error(on_error);
    }
}

// ---------------------------------------------------------------------------
// RegistryInfo + RegistryInfoLookup
// ---------------------------------------------------------------------------

/// `RegistryOps.RegistryInfo<T>` — `(owner, getter, elementsLifecycle)`.
///
/// `owner`/`getter` are the `HolderOwner`/`HolderGetter` views of the owning
/// registry. #126: `owner` is the `RegistryOwner` (the per-instance
/// `RegistryId`, for the O(1) `canSerializeIn` check) and `getter` is the
/// `RegistryGetter<E>` view over the owning `RegistryAccess`. `access` is the
/// whole owning access: the getter resolves the frozen registry through it at
/// the sanctioned erased boundary (`RegistryAccess::lookup`).
#[derive(Debug, Clone)]
pub struct RegistryInfo<T> {
    /// `RegistryInfo.elementsLifecycle()` — the registry's lifecycle.
    pub elements_lifecycle: Lifecycle,
    /// The owning registry's per-instance `RegistryId` (the owner view).
    pub registry_id: RegistryId,
    /// The owning `RegistryAccess` (the getter view resolves through it).
    pub access: RegistryAccess,
    _marker: PhantomData<fn() -> T>,
}

impl<T> RegistryInfo<T> {
    /// `RegistryInfo(HolderOwner, HolderGetter, Lifecycle)` — built by
    /// `HolderLookupAdapter` from the erased registry's id + lifecycle and the
    /// owning access.
    pub fn new(
        elements_lifecycle: Lifecycle,
        registry_id: RegistryId,
        access: RegistryAccess,
    ) -> Self {
        RegistryInfo {
            elements_lifecycle,
            registry_id,
            access,
            _marker: PhantomData,
        }
    }
}

/// `RegistryOps.RegistryInfoLookup` — `lookup(ResourceKey) -> Optional<RegistryInfo>`.
///
/// The trait is dyn-compatible (no generic methods) so `RegistryOps` can hold
/// `Box<dyn RegistryInfoLookup>`. Lookups are erased to `RegistryKey<()>`; the
/// implementor narrows the key into its per-instance map.
pub trait RegistryInfoLookup: std::fmt::Debug {
    /// `RegistryInfoLookup.lookup(ResourceKey)` — erased registry-key form.
    fn lookup_erased(&self, registry_key: &RegistryKey<()>) -> Option<RegistryInfo<()>>;

    /// Clone the trait object (Java shares the `lookupProvider` reference
    /// across `withParent`; Rust owns it, so the concrete lookup must be
    /// cloneable).
    fn clone_box(&self) -> Box<dyn RegistryInfoLookup>;
}

impl Clone for Box<dyn RegistryInfoLookup> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// ---------------------------------------------------------------------------
// HolderLookupAdapter
// ---------------------------------------------------------------------------

/// `RegistryOps.HolderLookupAdapter` — a `RegistryInfoLookup` over a
/// `HolderLookup.Provider` (`RegistryAccess` in this crate).
///
/// Java memoizes each `lookup` with `ConcurrentHashMap.computeIfAbsent` — the
/// *lazy* behavior this port preserves: each registry key is resolved through
/// the provider at most once, and the `Option` (including a miss) is cached.
/// The adapter is used inside a single decode, so the single-threaded
/// `RefCell<HashMap>` matches Java's per-instance memo map (no cross-thread
/// sharing; OWNERSHIP's single sync tick).
#[derive(Debug)]
pub struct HolderLookupAdapter {
    lookup_provider: RegistryAccess,
    lookups: RefCell<HashMap<RegistryKey<()>, Option<RegistryInfo<()>>>>,
}

impl HolderLookupAdapter {
    /// `new HolderLookupAdapter(HolderLookup.Provider)`.
    pub fn new(lookup_provider: RegistryAccess) -> Self {
        HolderLookupAdapter {
            lookup_provider,
            lookups: RefCell::new(HashMap::new()),
        }
    }

    /// `HolderLookupAdapter.createLookup(ResourceKey)` — the memoized function.
    fn create_lookup(&self, registry_key: &RegistryKey<()>) -> Option<RegistryInfo<()>> {
        self.lookup_provider
            .lookup_erased(registry_key)
            .map(|registry| {
                // Java `RegistryInfo.fromRegistryLookup` (RegistryOps.java:129-131):
                // `registry.registryLifecycle()`. The erased `&dyn AnyRegistry` exposes
                // the real lifecycle and the per-instance `RegistryId` (root.rs), so
                // the owner view and lifecycle are reported without a downcast.
                RegistryInfo::new(
                    registry.registry_lifecycle(),
                    registry.registry_id(),
                    self.lookup_provider.clone(),
                )
            })
    }

    /// The memoized-cache size — test support for the lazy/`computeIfAbsent`
    /// behavior (a key is resolved through the provider at most once).
    #[cfg(test)]
    pub(crate) fn cache_len(&self) -> usize {
        self.lookups.borrow().len()
    }
}

impl RegistryInfoLookup for HolderLookupAdapter {
    fn lookup_erased(&self, registry_key: &RegistryKey<()>) -> Option<RegistryInfo<()>> {
        if let Some(cached) = self.lookups.borrow().get(registry_key).cloned() {
            return cached;
        }
        let computed = self.create_lookup(registry_key);
        self.lookups
            .borrow_mut()
            .insert(registry_key.clone(), computed.clone());
        computed
    }

    fn clone_box(&self) -> Box<dyn RegistryInfoLookup> {
        // Java shares the memo map reference; here the clone gets a fresh memo
        // cache, which is observationally equivalent for reads (each registry is
        // still resolved from the same provider).
        Box::new(HolderLookupAdapter {
            lookup_provider: self.lookup_provider.clone(),
            lookups: RefCell::new(HashMap::new()),
        })
    }
}

// ---------------------------------------------------------------------------
// RegistryOps
// ---------------------------------------------------------------------------

/// `RegistryOps<T>` — a `DelegatingOps<T>` that carries a `RegistryInfoLookup`.
///
/// `RegistryOps<T, D>` wraps a delegate ops `D` (the parent `DynamicOps`) plus
/// the `lookup_provider` context. It is itself a `DynamicOps<Output = T>` that
/// delegates every method to the wrapped ops — the serialization context
/// codecs (`retrieve_getter`/`retrieve_element`) read `lookup_provider` through
/// `RegistryOpsLookup`.
pub struct RegistryOps<T, D: DynamicOps<Output = T>> {
    pub(crate) base: DelegatingOps<T, D>,
    pub(crate) lookup_provider: Box<dyn RegistryInfoLookup>,
}

impl<T, D: DynamicOps<Output = T>> std::fmt::Debug for RegistryOps<T, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryOps")
            .field("base", &self.base)
            .field("lookup_provider", &self.lookup_provider)
            .finish()
    }
}

impl<T, D: DynamicOps<Output = T> + Clone> RegistryOps<T, D> {
    /// `RegistryOps.create(DynamicOps<T> parent, RegistryInfoLookup)`.
    ///
    /// The delegate is stored by value, so `D` must be `Clone` (JsonOps/NbtOps
    /// are Copy singletons).
    pub fn create(parent: &D, lookup_provider: Box<dyn RegistryInfoLookup>) -> RegistryOps<T, D> {
        RegistryOps {
            base: DelegatingOps::new(parent.clone()),
            lookup_provider,
        }
    }

    /// `RegistryOps.create(DynamicOps<T> parent, HolderLookup.Provider)` — the
    /// overload that wraps a `RegistryAccess` in a `HolderLookupAdapter`.
    pub fn create_from_access(parent: &D, access: RegistryAccess) -> RegistryOps<T, D> {
        RegistryOps::create(parent, Box::new(HolderLookupAdapter::new(access)))
    }
}

impl<T, D: DynamicOps<Output = T>> RegistryOps<T, D> {
    /// `RegistryOps.withParent(DynamicOps<U>)` — change the delegate ops.
    ///
    /// Java short-circuits `parent == this.delegate ? this : new RegistryOps<>`
    /// (ops are not `PartialEq` here, so the identity check is not portable);
    /// the lookup is always shared.
    pub fn with_parent<U, O>(&self, parent: &O) -> RegistryOps<U, O>
    where
        O: DynamicOps<Output = U> + Clone,
    {
        RegistryOps::create(parent, self.lookup_provider.clone())
    }

    /// `RegistryOps.owner(ResourceKey)` — `Optional<HolderOwner<E>>`.
    ///
    /// The #126 owner view is the type-erased `RegistryOwner` carrying the
    /// registry's per-instance `RegistryId` (the O(1) `canSerializeIn` check).
    pub fn owner<E>(&self, registry_key: &RegistryKey<E>) -> Option<RegistryOwner> {
        let erased = erase_registry_key(registry_key);
        self.owner_for_erased(&erased)
    }

    /// `RegistryOps.owner` for a pre-erased registry key (the test seam; the
    /// public method erases through the key's identifier). `RegistryOwner` is
    /// type-erased, so the erased form carries no element type.
    pub(crate) fn owner_for_erased(&self, erased: &RegistryKey<()>) -> Option<RegistryOwner> {
        self.lookup_provider
            .lookup_erased(erased)
            .map(|info| RegistryOwner {
                registry_id: info.registry_id,
            })
    }

    /// `RegistryOps.getter(ResourceKey)` — `Optional<HolderGetter<E>>`.
    ///
    /// The #126 getter view is the `RegistryGetter<E>` over the owning access,
    /// resolving the frozen registry through the sanctioned erased downcast.
    pub fn getter<E>(&self, registry_key: &RegistryKey<E>) -> Option<RegistryGetter<E>>
    where
        E: Send + Sync + 'static,
    {
        let erased = erase_registry_key(registry_key);
        self.getter_for_erased(&erased)
    }

    /// `RegistryOps.getter` for a pre-erased registry key (see `owner_for_erased`).
    pub(crate) fn getter_for_erased<E>(&self, erased: &RegistryKey<()>) -> Option<RegistryGetter<E>>
    where
        E: Send + Sync + 'static,
    {
        self.lookup_provider.lookup_erased(erased).map(|info| {
            RegistryGetter::new(
                info.access.clone(),
                ResourceKey::create_registry_key(erased.identifier().clone()),
            )
        })
    }
}

impl<T, D> DynamicOps for RegistryOps<T, D>
where
    T: Debug + Clone + PartialEq,
    D: DynamicOps<Output = T>,
{
    type Output = T;

    fn empty(&self) -> T {
        self.base.empty()
    }

    fn empty_map(&self) -> T {
        self.base.empty_map()
    }

    fn empty_list(&self) -> T {
        self.base.empty_list()
    }

    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &T) -> U::Output {
        self.base.convert_to(out_ops, input)
    }

    fn get_number_value(&self, input: &T) -> DataResult<Number> {
        self.base.get_number_value(input)
    }

    fn create_numeric(&self, value: Number) -> T {
        self.base.create_numeric(value)
    }

    fn create_byte(&self, value: i8) -> T {
        self.base.create_byte(value)
    }

    fn create_short(&self, value: i16) -> T {
        self.base.create_short(value)
    }

    fn create_int(&self, value: i32) -> T {
        self.base.create_int(value)
    }

    fn create_long(&self, value: i64) -> T {
        self.base.create_long(value)
    }

    fn create_float(&self, value: f32) -> T {
        self.base.create_float(value)
    }

    fn create_double(&self, value: f64) -> T {
        self.base.create_double(value)
    }

    fn get_boolean_value(&self, input: &T) -> DataResult<bool> {
        self.base.get_boolean_value(input)
    }

    fn create_boolean(&self, value: bool) -> T {
        self.base.create_boolean(value)
    }

    fn get_string_value(&self, input: &T) -> DataResult<String> {
        self.base.get_string_value(input)
    }

    fn create_string(&self, value: String) -> T {
        self.base.create_string(value)
    }

    fn merge_to_list(&self, list: &T, value: T) -> DataResult<T> {
        self.base.merge_to_list(list, value)
    }

    fn merge_to_map(&self, map: &T, key: T, value: T) -> DataResult<T> {
        self.base.merge_to_map(map, key, value)
    }

    fn merge_to_primitive(&self, prefix: &T, value: T) -> DataResult<T> {
        self.base.merge_to_primitive(prefix, value)
    }

    fn get_map_values(&self, input: &T) -> DataResult<Vec<Pair<T, T>>> {
        self.base.get_map_values(input)
    }

    fn get_map_entries(&self, input: &T) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&T, &T))>> {
        self.base.get_map_entries(input)
    }

    fn create_map(&self, map: Vec<Pair<T, T>>) -> T {
        self.base.create_map(map)
    }

    fn get_map(&self, input: &T) -> DataResult<Box<dyn MapLike<T>>> {
        self.base.get_map(input)
    }

    fn get_stream(&self, input: &T) -> DataResult<Vec<T>> {
        self.base.get_stream(input)
    }

    fn get_list(&self, input: &T) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&T))>> {
        self.base.get_list(input)
    }

    fn create_list(&self, input: Vec<T>) -> T {
        self.base.create_list(input)
    }

    fn get_byte_buffer(&self, input: &T) -> DataResult<Vec<u8>> {
        self.base.get_byte_buffer(input)
    }

    fn create_byte_list(&self, input: &[u8]) -> T {
        self.base.create_byte_list(input)
    }

    fn get_int_stream(&self, input: &T) -> DataResult<Vec<i32>> {
        self.base.get_int_stream(input)
    }

    fn create_int_list(&self, input: Vec<i32>) -> T {
        self.base.create_int_list(input)
    }

    fn get_long_stream(&self, input: &T) -> DataResult<Vec<i64>> {
        self.base.get_long_stream(input)
    }

    fn create_long_list(&self, input: Vec<i64>) -> T {
        self.base.create_long_list(input)
    }

    fn remove(&self, input: T, key: &str) -> T {
        self.base.remove(input, key)
    }

    fn compress_maps(&self) -> bool {
        self.base.compress_maps()
    }

    fn list_builder(&self) -> Box<dyn ListBuilder<Output = T> + '_> {
        self.base.list_builder()
    }

    fn map_builder(&self) -> Box<dyn RecordBuilder<Output = T> + '_> {
        self.base.map_builder()
    }
}

/// `ops instanceof RegistryOps<?>` — the #124 seam for the context codecs.
///
/// Java's `retrieveGetter`/`retrieveElement` runtime-guard `ops instanceof
/// RegistryOps<?>` (erroring `"Not a registry ops"` otherwise). Here the ops
/// type is pinned at compile time, so the guard becomes this trait bound: a
/// context codec can only be built for — and used with — a `RegistryOps`. Only
/// `RegistryOps<T, D>` implements it.
pub trait RegistryOpsLookup {
    /// The wrapped `RegistryInfoLookup`.
    fn lookup_provider(&self) -> &dyn RegistryInfoLookup;
}

impl<T, D: DynamicOps<Output = T>> RegistryOpsLookup for RegistryOps<T, D> {
    fn lookup_provider(&self) -> &dyn RegistryInfoLookup {
        self.lookup_provider.as_ref()
    }
}

// ---------------------------------------------------------------------------
// retrieveGetter / retrieveElement
// ---------------------------------------------------------------------------

/// Erase the element type of a registry key — `ResourceKey.createRegistryKey
/// (key.identifier())`.
fn erase_registry_key<E>(key: &RegistryKey<E>) -> RegistryKey<()> {
    let erased: RegistryKey<()> = ResourceKey::create_registry_key(key.identifier().clone());
    erased
}

/// `RegistryOps.retrieveGetter(ResourceKey)` — the context codec.
///
/// Decodes the owning `HolderGetter<E>` (`RegistryGetter<E>`) straight from the
/// ops' lookup provider; `"Unknown registry: <key>"` when the registry is
/// absent. The decode result carries the registry's `elementsLifecycle` (Java
/// `DataResult.success(r.getter(), r.elementsLifecycle())`).
pub fn retrieve_getter<E, Ops>(
    registry_key: &RegistryKey<E>,
) -> Arc<dyn MapCodec<RegistryGetter<E>, Ops>>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    retrieve_getter_for_erased(erase_registry_key(registry_key))
}

/// `retrieveGetter` for a pre-erased registry key — the test seam (the public
/// wrapper erases through the key's identifier; the decode logic below is the
/// real behavior).
pub(crate) fn retrieve_getter_for_erased<E, Ops>(
    erased_registry_key: RegistryKey<()>,
) -> Arc<dyn MapCodec<RegistryGetter<E>, Ops>>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    extra_codecs::retrieve_context(Arc::new(move |ops: &Ops| {
        match ops.lookup_provider().lookup_erased(&erased_registry_key) {
            Some(info) => DataResult::success_with_lifecycle(
                RegistryGetter::new(
                    info.access.clone(),
                    ResourceKey::create_registry_key(erased_registry_key.identifier().clone()),
                ),
                info.elements_lifecycle,
            ),
            None => DataResult::error(format!("Unknown registry: {}", erased_registry_key)),
        }
    }))
}

/// `RegistryOps.retrieveElement(ResourceKey<E>)` — the context codec.
///
/// Decodes a single element, returning its **`Holder.Reference<E>`** (Java
/// `retrieveElement` returns `Holder.Reference<E>`; #126 widens the SCC's
/// narrowed element-value form). Either an unknown registry or a missing
/// element reports Java's `"Can't find value: <key>"` — Java's
/// `flatMap(r -> r.getter().get(key)).orElseGet(...)` collapses both to the
/// same message.
pub fn retrieve_element<E, Ops>(key: &ResourceKey<E>) -> Arc<dyn MapCodec<Holder<E>, Ops>>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    let registry_key: RegistryKey<E> = ResourceKey::create_registry_key(key.registry().clone());
    retrieve_element_for_erased(erase_registry_key(&registry_key), key.clone())
}

/// `retrieveElement` for a pre-erased registry key (see
/// `retrieve_getter_for_erased`).
pub(crate) fn retrieve_element_for_erased<E, Ops>(
    erased_registry_key: RegistryKey<()>,
    key: ResourceKey<E>,
) -> Arc<dyn MapCodec<Holder<E>, Ops>>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    extra_codecs::retrieve_context(Arc::new(move |ops: &Ops| {
        let lookup = ops.lookup_provider();
        match lookup.lookup_erased(&erased_registry_key) {
            Some(info) => {
                // `RegistryInfo.getter().get(key)` — resolve the holder through the
                // owning access's frozen registry at the sanctioned erased
                // boundary (`RegistryAccess::lookup`, the sole downcast). The
                // result is the `Holder::Reference` value (holder id == element
                // id, OWNERSHIP.md).
                let registry = info.access.lookup::<E>(&ResourceKey::create_registry_key(
                    erased_registry_key.identifier().clone(),
                ));
                match registry.and_then(|registry| HolderGetter::get(registry, &key)) {
                    Some(holder) => DataResult::success(holder),
                    None => DataResult::error(format!("Can't find value: {}", key)),
                }
            }
            None => DataResult::error(format!("Can't find value: {}", key)),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identifier;
    use crate::builder::RegistryBuilder;
    use crate::registry::Registry;
    use crate::root::AnyBox;

    use rivet_serialization::data_result::DataResult;
    use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, Pair};
    use rivet_serialization::lifecycle::Lifecycle;

    // -----------------------------------------------------------------------
    // A minimal self-contained DynamicOps for the tests. rivet-registry has no
    // ops of its own and Cargo.toml carries no serde_json dev-dependency, so
    // the tests use this tiny JSON-like value type.
    // -----------------------------------------------------------------------

    #[derive(Debug, Clone, PartialEq)]
    enum TestVal {
        Num(f64),
        Str(String),
        Bool(bool),
        Map(Vec<(TestVal, TestVal)>),
        List(Vec<TestVal>),
        Null,
    }

    #[derive(Debug, Clone, Copy, Default)]
    struct TestOps;

    #[derive(Debug)]
    struct TestMapLike(Vec<(TestVal, TestVal)>);

    impl MapLike<TestVal> for TestMapLike {
        fn get(&self, key: &TestVal) -> Option<TestVal> {
            self.0
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        }

        fn get_string(&self, key: &str) -> Option<TestVal> {
            self.0
                .iter()
                .find(|(k, _)| matches!(k, TestVal::Str(s) if s == key))
                .map(|(_, v)| v.clone())
        }

        fn entries(&self) -> Vec<Pair<TestVal, TestVal>> {
            self.0
                .iter()
                .map(|(k, v)| Pair::of(k.clone(), v.clone()))
                .collect()
        }
    }

    impl DynamicOps for TestOps {
        type Output = TestVal;

        fn empty(&self) -> TestVal {
            TestVal::Null
        }

        fn empty_map(&self) -> TestVal {
            TestVal::Map(Vec::new())
        }

        fn empty_list(&self) -> TestVal {
            TestVal::List(Vec::new())
        }

        fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &TestVal) -> U::Output {
            match input {
                TestVal::Num(n) => out_ops.create_numeric(Number::Double(*n)),
                TestVal::Str(s) => out_ops.create_string(s.clone()),
                TestVal::Bool(b) => out_ops.create_boolean(*b),
                TestVal::Map(entries) => out_ops.create_map(
                    entries
                        .iter()
                        .map(|(k, v)| {
                            Pair::of(self.convert_to(out_ops, k), self.convert_to(out_ops, v))
                        })
                        .collect(),
                ),
                TestVal::List(items) => {
                    out_ops.create_list(items.iter().map(|v| self.convert_to(out_ops, v)).collect())
                }
                TestVal::Null => out_ops.empty(),
            }
        }

        fn get_number_value(&self, input: &TestVal) -> DataResult<Number> {
            match input {
                TestVal::Num(n) => DataResult::success(Number::Double(*n)),
                _ => DataResult::error(format!("Not a number: {:?}", input)),
            }
        }

        fn create_numeric(&self, value: Number) -> TestVal {
            TestVal::Num(value.double_value())
        }

        fn get_boolean_value(&self, input: &TestVal) -> DataResult<bool> {
            match input {
                TestVal::Bool(b) => DataResult::success(*b),
                _ => DataResult::error(format!("Not a bool: {:?}", input)),
            }
        }

        fn create_boolean(&self, value: bool) -> TestVal {
            TestVal::Bool(value)
        }

        fn get_string_value(&self, input: &TestVal) -> DataResult<String> {
            match input {
                TestVal::Str(s) => DataResult::success(s.clone()),
                _ => DataResult::error(format!("Not a string: {:?}", input)),
            }
        }

        fn create_string(&self, value: String) -> TestVal {
            TestVal::Str(value)
        }

        fn merge_to_list(&self, list: &TestVal, value: TestVal) -> DataResult<TestVal> {
            match list {
                TestVal::List(items) => {
                    let mut items = items.clone();
                    items.push(value);
                    DataResult::success(TestVal::List(items))
                }
                TestVal::Null => DataResult::success(TestVal::List(vec![value])),
                _ => DataResult::error(format!("Not a list: {:?}", list)),
            }
        }

        fn merge_to_map(&self, map: &TestVal, key: TestVal, value: TestVal) -> DataResult<TestVal> {
            match map {
                TestVal::Map(entries) => {
                    let mut entries = entries.clone();
                    entries.retain(|(k, _)| k != &key);
                    entries.push((key, value));
                    DataResult::success(TestVal::Map(entries))
                }
                TestVal::Null => DataResult::success(TestVal::Map(vec![(key, value)])),
                _ => DataResult::error(format!("Not a map: {:?}", map)),
            }
        }

        fn get_map_values(&self, input: &TestVal) -> DataResult<Vec<Pair<TestVal, TestVal>>> {
            match input {
                TestVal::Map(entries) => DataResult::success(
                    entries
                        .iter()
                        .map(|(k, v)| Pair::of(k.clone(), v.clone()))
                        .collect(),
                ),
                _ => DataResult::error(format!("Not a map: {:?}", input)),
            }
        }

        fn get_map_entries(
            &self,
            input: &TestVal,
        ) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&TestVal, &TestVal))>> {
            match input {
                TestVal::Map(entries) => {
                    let entries = entries.clone();
                    DataResult::success(Box::new(move |consumer| {
                        for (k, v) in &entries {
                            consumer(k, v);
                        }
                    }))
                }
                _ => DataResult::error(format!("Not a map: {:?}", input)),
            }
        }

        fn create_map(&self, map: Vec<Pair<TestVal, TestVal>>) -> TestVal {
            TestVal::Map(map.into_iter().map(|p| (p.first, p.second)).collect())
        }

        fn get_map(&self, input: &TestVal) -> DataResult<Box<dyn MapLike<TestVal>>> {
            match input {
                TestVal::Map(entries) => {
                    DataResult::success(Box::new(TestMapLike(entries.clone())))
                }
                _ => DataResult::error(format!("Not a map: {:?}", input)),
            }
        }

        fn get_stream(&self, input: &TestVal) -> DataResult<Vec<TestVal>> {
            match input {
                TestVal::List(items) => DataResult::success(items.clone()),
                _ => DataResult::error(format!("Not a list: {:?}", input)),
            }
        }

        fn get_list(&self, input: &TestVal) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&TestVal))>> {
            match input {
                TestVal::List(items) => {
                    let items = items.clone();
                    DataResult::success(Box::new(move |consumer| {
                        for v in &items {
                            consumer(v);
                        }
                    }))
                }
                _ => DataResult::error(format!("Not a list: {:?}", input)),
            }
        }

        fn create_list(&self, input: Vec<TestVal>) -> TestVal {
            TestVal::List(input)
        }

        fn get_byte_buffer(&self, input: &TestVal) -> DataResult<Vec<u8>> {
            self.get_stream(input).map(|items| {
                items
                    .iter()
                    .filter_map(|v| {
                        self.get_number_value(v)
                            .result()
                            .map(|n| n.byte_value() as u8)
                    })
                    .collect()
            })
        }

        fn create_byte_list(&self, input: &[u8]) -> TestVal {
            TestVal::List(input.iter().map(|b| TestVal::Num(*b as f64)).collect())
        }

        fn get_int_stream(&self, input: &TestVal) -> DataResult<Vec<i32>> {
            self.get_stream(input).map(|items| {
                items
                    .iter()
                    .filter_map(|v| self.get_number_value(v).result().map(|n| n.int_value()))
                    .collect()
            })
        }

        fn create_int_list(&self, input: Vec<i32>) -> TestVal {
            TestVal::List(input.into_iter().map(|i| TestVal::Num(i as f64)).collect())
        }

        fn get_long_stream(&self, input: &TestVal) -> DataResult<Vec<i64>> {
            self.get_stream(input).map(|items| {
                items
                    .iter()
                    .filter_map(|v| self.get_number_value(v).result().map(|n| n.long_value()))
                    .collect()
            })
        }

        fn create_long_list(&self, input: Vec<i64>) -> TestVal {
            TestVal::List(input.into_iter().map(|l| TestVal::Num(l as f64)).collect())
        }

        fn remove(&self, input: TestVal, key: &str) -> TestVal {
            match input {
                TestVal::Map(entries) => TestVal::Map(
                    entries
                        .into_iter()
                        .filter(|(k, _)| !matches!(k, TestVal::Str(s) if s == key))
                        .collect(),
                ),
                other => other,
            }
        }

        fn compress_maps(&self) -> bool {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------

    #[derive(Debug, Clone)]
    struct TestElement;

    fn element_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn erased_key(id: &str) -> RegistryKey<()> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(id))
    }

    fn access_with_one_registry() -> RegistryAccess {
        let registry: Registry<TestElement> = RegistryBuilder::new(&element_key()).freeze();
        RegistryAccess::from_pairs(vec![(erased_key("test"), Box::new(registry) as AnyBox)])
    }

    fn empty_ops() -> RegistryOps<TestVal, TestOps> {
        RegistryOps::create(
            &TestOps,
            Box::new(HolderLookupAdapter::new(RegistryAccess::empty())),
        )
    }

    fn ops_over(access: RegistryAccess) -> RegistryOps<TestVal, TestOps> {
        RegistryOps::create(&TestOps, Box::new(HolderLookupAdapter::new(access)))
    }

    fn empty_map_of(_ops: &RegistryOps<TestVal, TestOps>) -> Box<dyn MapLike<TestVal>> {
        // The context codecs ignore the map input, so an empty map suffices.
        Box::new(TestMapLike(Vec::new()))
    }

    // -----------------------------------------------------------------------
    // Delegation
    // -----------------------------------------------------------------------

    #[test]
    fn registry_ops_delegates_ops_methods_to_the_wrapped_ops() {
        let ops = empty_ops();
        assert_eq!(ops.empty(), TestVal::Null);
        assert_eq!(ops.empty_map(), TestVal::Map(Vec::new()));
        assert_eq!(ops.empty_list(), TestVal::List(Vec::new()));
        assert_eq!(ops.create_int(7), TestVal::Num(7.0));
        assert_eq!(
            ops.get_number_value(&TestVal::Num(2.5)).result(),
            Some(&Number::Double(2.5))
        );
        assert_eq!(
            ops.get_string_value(&ops.create_string("hi".to_string()))
                .result(),
            Some(&"hi".to_string())
        );
        assert!(!ops.compress_maps());
    }

    #[test]
    fn registry_ops_convert_to_delegates() {
        let ops = empty_ops();
        let converted = ops.convert_to(&TestOps, &TestVal::Num(1.0));
        assert_eq!(converted, TestVal::Num(1.0));
    }

    #[test]
    fn registry_ops_map_builder_builds_through_the_delegate_builder() {
        let ops = empty_ops();
        let mut builder = ops.map_builder();
        builder.add_string("a", TestVal::Num(1.0));
        let built = builder.build(Some(ops.empty_map()));
        assert!(built.is_success());
        assert_eq!(
            built.result().unwrap(),
            &TestVal::Map(vec![(TestVal::Str("a".to_string()), TestVal::Num(1.0))])
        );
    }

    #[test]
    fn registry_ops_list_builder_builds_through_the_delegate_builder() {
        let ops = empty_ops();
        let mut builder = ops.list_builder();
        builder.add(TestVal::Num(1.0));
        let built = builder.build(ops.empty_list());
        assert!(built.is_success());
        assert_eq!(
            built.result().unwrap(),
            &TestVal::List(vec![TestVal::Num(1.0)])
        );
    }

    #[test]
    fn with_parent_replaces_the_delegate_and_keeps_the_lookup() {
        let access = access_with_one_registry();
        let ops = ops_over(access.clone());
        let ops2 = ops.with_parent(&TestOps);
        // The lookup is shared across the two ops (cloned through the adapter):
        // both resolve the same registry from the same owning access.
        assert!(
            ops2.lookup_provider()
                .lookup_erased(&erased_key("test"))
                .is_some()
        );
        assert!(
            ops.lookup_provider()
                .lookup_erased(&erased_key("test"))
                .is_some()
        );
    }

    // -----------------------------------------------------------------------
    // HolderLookupAdapter (lazy memoization)
    // -----------------------------------------------------------------------

    #[test]
    fn holder_lookup_adapter_resolves_and_memoizes() {
        let access = access_with_one_registry();
        let adapter = HolderLookupAdapter::new(access);
        let key = erased_key("test");
        let first = adapter.lookup_erased(&key);
        let second = adapter.lookup_erased(&key);
        assert!(first.is_some());
        assert!(second.is_some());
        // `computeIfAbsent`: the provider lookup happened once, the miss/entry
        // is cached.
        assert_eq!(adapter.cache_len(), 1);
    }

    #[test]
    fn holder_lookup_adapter_caches_misses() {
        let adapter = HolderLookupAdapter::new(RegistryAccess::empty());
        let key = erased_key("missing");
        assert!(adapter.lookup_erased(&key).is_none());
        assert!(adapter.lookup_erased(&key).is_none());
        assert_eq!(adapter.cache_len(), 1);
    }

    // -----------------------------------------------------------------------
    // owner / getter
    // -----------------------------------------------------------------------

    #[test]
    fn owner_and_getter_reflect_the_owning_registry() {
        let access = access_with_one_registry();
        let ops = ops_over(access.clone());
        let erased = erased_key("test");
        let owner = ops.owner_for_erased(&erased);
        let getter = ops.getter_for_erased::<TestElement>(&erased);
        assert!(owner.is_some());
        assert!(getter.is_some());
        // The owner is the registry's per-instance RegistryId (Java `context ==
        // this`, OWNERSHIP.md §Registries).
        let registered_id = access
            .lookup(&element_key())
            .expect("frozen registry")
            .registry_id();
        assert_eq!(owner.unwrap().registry_id, registered_id);
        // The getter resolves the same registry through the owning access: the
        // frozen registry is the shared instance.
        let getter = getter.unwrap();
        assert_eq!(getter.registry().unwrap().registry_id(), registered_id);
    }

    #[test]
    fn owner_and_getter_return_none_for_an_unknown_access() {
        let ops = empty_ops();
        assert!(ops.owner_for_erased(&erased_key("test")).is_none());
        assert!(
            ops.getter_for_erased::<TestElement>(&erased_key("test"))
                .is_none()
        );
    }

    // -----------------------------------------------------------------------
    // retrieveGetter / retrieveElement
    // -----------------------------------------------------------------------

    #[test]
    fn retrieve_getter_yields_the_getter_with_the_registry_lifecycle() {
        let access = access_with_one_registry();
        let ops = ops_over(access.clone());
        let codec = retrieve_getter_for_erased::<TestElement, _>(erased_key("test"));
        let decoded = codec.decode(&ops, empty_map_of(&ops).as_ref());
        assert!(decoded.is_success());
        // The decode result carries the REAL registry lifecycle (read through the
        // erased `&dyn AnyRegistry` boundary, Java's `registryLifecycle()`).
        assert_eq!(decoded.lifecycle(), Lifecycle::Stable);
        let getter = decoded.get_or_throw("decode");
        // The getter resolves the same frozen registry through the owning access.
        let registered_id = access
            .lookup(&element_key())
            .expect("frozen registry")
            .registry_id();
        assert_eq!(getter.registry().unwrap().registry_id(), registered_id);
    }

    #[test]
    fn retrieve_getter_unknown_registry_diagnostic() {
        let ops = empty_ops();
        let codec = retrieve_getter_for_erased::<TestElement, _>(erased_key("missing"));
        let result = codec.decode(&ops, empty_map_of(&ops).as_ref());
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.starts_with("Unknown registry:"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn retrieve_element_missing_registry_diagnostic() {
        let ops = empty_ops();
        let key: ResourceKey<TestElement> =
            ResourceKey::create(&element_key(), Identifier::with_default_namespace("thing"));
        let codec =
            retrieve_element_for_erased::<TestElement, _>(erased_key("missing"), key.clone());
        let result = codec.decode(&ops, empty_map_of(&ops).as_ref());
        assert!(result.is_error());
        // Java: `.orElseGet(() -> DataResult.error(() -> "Can't find value: " + key))`.
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Can't find value: {}", key)
        );
    }

    #[test]
    fn retrieve_element_missing_element_in_known_registry_diagnostic() {
        // The registry is present but the element is not registered, so the
        // element lookup fails — Java's `r.getter().get(key)` empty path.
        let access = access_with_one_registry();
        let ops = ops_over(access);
        let key: ResourceKey<TestElement> =
            ResourceKey::create(&element_key(), Identifier::with_default_namespace("thing"));
        let codec = retrieve_element_for_erased::<TestElement, _>(erased_key("test"), key.clone());
        let result = codec.decode(&ops, empty_map_of(&ops).as_ref());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Can't find value: {}", key)
        );
    }
}
