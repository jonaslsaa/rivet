//! Port of `net.minecraft.core.Registry<T>` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.core` manifest unit. Java source:
//! `net/minecraft/core/Registry.java` (181 lines, 26.2) + the `WritableRegistry`
//! pre-freeze surface.
//!
//! Binding model (OWNERSHIP.md §Registries, #107):
//! - **One concrete `Registry<T>`** (no trait) owning `Vec<Arc<T>>` by insertion
//!   order (**element id == holder id == network id == insertion index**), plus
//!   `by_location`/`by_key` (`HashMap<_, u32>`). `DefaultedRegistry` folds to
//!   `Option<u32> default_id` with its asymmetric fallbacks preserved.
//! - Builder → freeze: `RegistryBuilder<T>` is consumed by `freeze()` →
//!   `Registry<T>`; `freeze()` panics with sorted unbound keys like
//!   `MappedRegistry.freeze()`. The `frozen` boolean + `validateWrite()` are
//!   **compile-time phase types** (builder vs frozen), not runtime checks.
//! - **Identity-sensitive value mapping:** Java keys `toId`/`byValue` by object
//!   identity (`Reference2IntOpenHashMap`/`IdentityHashMap`). Elements are held
//!   as `Arc<T>` (OWNERSHIP.md allows `Arc` for the immutable registries layer)
//!   and value-keyed maps key by `Arc::as_ptr` — so re-registering the same
//!   allocation is a duplicate, two `Eq`-equal-but-distinct values are not, and
//!   `get_id(&value)` resolves references handed back by `get_value`/`by_id`.
//!   No `Arc<RwLock>`, no `Cell`/`RefCell`; frozen registries are immutable
//!   value tables.
//! - `Holder<T>` is an ID resolved through the owning registry — the *minimal
//!   holder reference shape* this SCC needs is `RegistryId` + `u32 id` (Copy).
//!   The full `Holder<T>`/`HolderSet<T>`/`RegistryLookup` value surface is
//!   **#126 (holder codecs)** (`holder.rs`/`holder_set.rs`/`holder_lookup.rs`);
//!   this module exposes only the registry-internal `HolderId` and the #126
//!   `impl RegistryLookup<T> for Registry<T>` (in `holder_lookup.rs`, which uses
//!   only these public methods) lands on top.

use crate::holder::{HolderId, RegistryId};
use crate::id_map::{DEFAULT_ID, IdMap};
use crate::identifier::Identifier;
use crate::registration_info::RegistrationInfo;
use crate::resource_key::ResourceKey;
use crate::tag_key::TagKey;

use rivet_serialization::lifecycle::Lifecycle;
use rivet_util::random::RandomSource;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// The registry key type — `Registry<T>.key()`.
///
/// A pure alias for `ResourceKey<Registry<T>>` (the registry-of-registries key
/// space), matching Java's `Registry.key(): ResourceKey<? extends Registry<T>>`.
/// It has no distinct wrapper semantics — it is NOT the Paper M4-adapter
/// `RegistryKey` wrapper; use it only to name the return of `key()` /
/// `create_registry_key`. If a real wrapper is ever needed it belongs elsewhere.
pub type RegistryKey<T> = ResourceKey<Registry<T>>;

/// The registry-internal holder reference — the Copy ID the #124 SCC's
/// `Registry<T>`/`RegistryBuilder<T>` return. The full #126 `Holder<T>`
/// (`Direct(T)` | `Reference { registry, id }`) lives in `holder.rs`; the SCC's
/// id space (element id == holder id == network id == insertion index) is
/// already the contract, so this alias is the registry-side handle.
pub type HolderReference = HolderId;

/// `net.minecraft.core.Registry<T>` — the frozen registry surface.
///
/// The full Java method set is implemented here:
///
/// - id space: `get_id`, `by_id`, `size`, `get_any`, `get_random`
/// - lookups: `get_key`, `get_resource_key`, `get_value(ResourceKey/Identifier)`,
///   `get_value_or_throw`, `get_optional`, `registration_info`, `contains_key`,
///   `key_set`, `registry_key_set`, `entry_set_ref`
/// - holder surface: `as_holder_id_map` (the rest is #126)
/// - tags: `get_tag`, `list_tags`, `get_tag_or_empty`
///
/// `by_name_codec`/`holder_by_name_codec`/`reference_holder_with_lifecycle`
/// are codec-surface #126 (`rivet-protocol`), not here.
pub struct Registry<T> {
    /// `MappedRegistry.key`.
    key: RegistryKey<T>,
    /// Per-instance identity (OWNERSHIP.md §Registries).
    registry_id: RegistryId,
    /// `MappedRegistry.byId` — insertion order; element id == holder id ==
    /// network id == index. `Arc` gives each element a stable identity address.
    values: Vec<Arc<T>>,
    /// Element keys parallel to `values` (insertion order, for
    /// `key_set`/`entry_set`/`get_key`).
    keys: Vec<ResourceKey<T>>,
    /// `MappedRegistry.byLocation` — `Identifier -> id`.
    by_location: HashMap<Identifier, u32>,
    /// `MappedRegistry.byKey` — `ResourceKey<T> -> id`.
    by_key: HashMap<ResourceKey<T>, u32>,
    /// `MappedRegistry.toId`/`byValue` — element identity (`Arc::as_ptr`) -> id.
    by_value: HashMap<usize, u32>,
    /// `MappedRegistry.registrationInfos`.
    registration_infos: HashMap<ResourceKey<T>, RegistrationInfo>,
    /// `MappedRegistry.registryLifecycle` — accumulated from registrations.
    lifecycle: Lifecycle,
    /// `DefaultedRegistry` fold — the default element's id.
    default_id: Option<u32>,
    /// `DefaultedMappedRegistry.defaultKey`.
    default_key: Option<Identifier>,
    /// Frozen named tag sets (`HolderSet.Named<T>` members; #126 widens the
    /// surface, the id space is already the contract).
    tags: HashMap<TagKey<T>, Vec<HolderId>>,
}

impl<T> Registry<T> {
    /// Construct a frozen registry from a consumed builder (`freeze()`). The
    /// builder is the only construction path.
    #[allow(clippy::too_many_arguments)] // mirrors the struct's 12 fields 1:1
    pub(crate) fn from_builder(
        key: RegistryKey<T>,
        registry_id: RegistryId,
        values: Vec<Arc<T>>,
        keys: Vec<ResourceKey<T>>,
        by_location: HashMap<Identifier, u32>,
        by_key: HashMap<ResourceKey<T>, u32>,
        by_value: HashMap<usize, u32>,
        registration_infos: HashMap<ResourceKey<T>, RegistrationInfo>,
        lifecycle: Lifecycle,
        default_id: Option<u32>,
        default_key: Option<Identifier>,
        tags: HashMap<TagKey<T>, Vec<HolderId>>,
    ) -> Self {
        Registry {
            key,
            registry_id,
            values,
            keys,
            by_location,
            by_key,
            by_value,
            registration_infos,
            lifecycle,
            default_id,
            default_key,
            tags,
        }
    }

    /// `Registry.key()`.
    pub fn key(&self) -> &RegistryKey<T> {
        &self.key
    }

    /// The registry's per-instance `RegistryId`.
    pub fn registry_id(&self) -> RegistryId {
        self.registry_id
    }

    /// `MappedRegistry.registryLifecycle()`.
    pub fn registry_lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// `Registry.size()`.
    pub fn size(&self) -> i32 {
        self.values.len() as i32
    }

    /// `Registry.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Identity lookup helper — `Arc::as_ptr` of the element the `&T` aliases.
    fn id_of(&self, thing: &T) -> Option<u32> {
        self.by_value.get(&(thing as *const T as usize)).copied()
    }

    /// `Registry.getId(T)` — `-1` when absent.
    ///
    /// Identity-sensitive: `thing` must alias a stored element (as handed back
    /// by `get_value`/`by_id`). `DefaultedRegistry.getId`: a missing value
    /// returns the default's id, not `-1`.
    pub fn get_id(&self, thing: &T) -> i32 {
        match self.id_of(thing) {
            Some(id) => id as i32,
            None => self.default_id.map_or(DEFAULT_ID, |d| d as i32),
        }
    }

    /// `Registry.byId(int)` — `@Nullable`.
    ///
    /// `DefaultedRegistry.byId`: an out-of-range id (including negative)
    /// returns the default element.
    pub fn by_id(&self, id: i32) -> Option<&T> {
        self.by_id_inner(id).map(Arc::as_ref)
    }

    /// In-range element, else the default element (asymmetric fallback).
    ///
    /// A defaulted registry that froze without its default key registered
    /// yields `None` here (and in `get_value_by_id`/`get_key`); Java
    /// `DefaultedMappedRegistry` NPEs on `this.defaultValue.value()` in that
    /// case (`freeze()` has no default-key check). Documented deviation: Rust
    /// returns `None` instead of panicking.
    fn by_id_inner(&self, id: i32) -> Option<&Arc<T>> {
        let in_range = (id >= 0).then(|| self.values.get(id as usize)).flatten();
        in_range.or_else(|| self.default_id.and_then(|d| self.values.get(d as usize)))
    }

    /// `Registry.getKey(T)` — `@Nullable Identifier`.
    ///
    /// `DefaultedRegistry.getKey`: an unregistered value falls back to the
    /// default key.
    pub fn get_key(&self, thing: &T) -> Option<Identifier> {
        self.id_of(thing)
            .and_then(|id| self.keys.get(id as usize))
            .map(|k| k.identifier().clone())
            .or_else(|| self.default_key.clone())
    }

    /// `Registry.getResourceKey(T)` — no default fallback (Java does not
    /// override it).
    pub fn get_resource_key(&self, thing: &T) -> Option<ResourceKey<T>> {
        self.id_of(thing)
            .and_then(|id| self.keys.get(id as usize))
            .cloned()
    }

    /// `Registry.getValue(ResourceKey<T>)` — `@Nullable`. No default fallback.
    pub fn get_value(&self, key: &ResourceKey<T>) -> Option<&T> {
        self.by_key
            .get(key)
            .and_then(|&id| self.values.get(id as usize))
            .map(Arc::as_ref)
    }

    /// `Registry.getValue(Identifier)` — `@Nullable`.
    ///
    /// `DefaultedRegistry.getValue(Identifier)`: a missing identifier returns
    /// the default element.
    pub fn get_value_by_id(&self, key: &Identifier) -> Option<&T> {
        self.by_location
            .get(key)
            .and_then(|&id| self.values.get(id as usize))
            .map(Arc::as_ref)
            .or_else(|| {
                self.default_id
                    .and_then(|d| self.values.get(d as usize))
                    .map(Arc::as_ref)
            })
    }

    /// `Registry.getValueOrThrow(ResourceKey<T>)`.
    pub fn get_value_or_throw(&self, key: &ResourceKey<T>) -> &T {
        match self.get_value(key) {
            Some(value) => value,
            None => panic!("Missing key in {}: {}", self.key, key),
        }
    }

    /// `Registry.getOptional(Identifier)` — no default fallback
    /// (`DefaultedMappedRegistry` overrides with `super.getValue`, deliberately).
    pub fn get_optional(&self, key: &Identifier) -> Option<&T> {
        self.by_location
            .get(key)
            .and_then(|&id| self.values.get(id as usize))
            .map(Arc::as_ref)
    }

    /// `Registry.getOptional(ResourceKey<T>)` — no default fallback.
    pub fn get_optional_by_key(&self, key: &ResourceKey<T>) -> Option<&T> {
        self.get_value(key)
    }

    /// `Registry.registrationInfo(ResourceKey<T>)`.
    pub fn registration_info(&self, element: &ResourceKey<T>) -> Option<RegistrationInfo> {
        self.registration_infos.get(element).cloned()
    }

    /// `Registry.getAny()`.
    ///
    /// `MappedRegistry.getAny`: the first element's holder. A defaulted registry
    /// instead returns the default holder (`Optional.ofNullable(defaultValue)`).
    pub fn get_any(&self) -> Option<HolderReference> {
        if self.default_key.is_some() {
            self.default_id.map(HolderId)
        } else {
            self.values.first().map(|_| HolderId(0))
        }
    }

    /// `Registry.getRandom(RandomSource)` — `Util.getRandomSafe(byId, random)`;
    /// a defaulted registry falls back to the default holder when empty.
    pub fn get_random(&self, random: &mut impl RandomSource) -> Option<HolderReference> {
        let picked = if self.values.is_empty() {
            None
        } else {
            Some(HolderId(
                random.next_int_bound(self.values.len() as i32) as u32
            ))
        };
        picked.or_else(|| self.default_id.map(HolderId))
    }

    /// `Registry.containsKey(Identifier)`.
    pub fn contains_key(&self, key: &Identifier) -> bool {
        self.by_location.contains_key(key)
    }

    /// `Registry.containsKey(ResourceKey<T>)`.
    pub fn contains_key_by_key(&self, key: &ResourceKey<T>) -> bool {
        self.by_key.contains_key(key)
    }

    /// `Registry.keySet()` — insertion order (a deliberate determinism choice:
    /// Java iterates an unspecified `HashMap.keySet()` order; Rust fixes it to
    /// the registration order OWNERSHIP.md mandates).
    pub fn key_set(&self) -> Vec<Identifier> {
        self.keys.iter().map(|k| k.identifier().clone()).collect()
    }

    /// `Registry.registryKeySet()`.
    pub fn registry_key_set(&self) -> Vec<ResourceKey<T>> {
        self.keys.clone()
    }

    /// `Registry.entrySet()` — the owned form required by the
    /// `RegistryAccess::from_registry_of_registries` contract.
    ///
    /// #126 boundary: an owned `Vec<(ResourceKey<T>, T)>` needs `T: Clone`
    /// (elements are `Arc<T>`; there is no way to move a non-`Clone` element
    /// out of `&self`). This is a **silent-wrong-result** seam, so it fails
    /// loudly: `entry_set` panics unless the registry is empty, and
    /// `entry_set_cloned` materializes owned `T: Clone` pairs for real content.
    /// Java `entrySet()` (MappedRegistry.java:250-251) returns a live
    /// unmodifiable view of `byKey`; the owned form is a #126 replacement for
    /// that view. Consumers needing borrowed entries use `entry_set_ref`.
    pub fn entry_set(&self) -> Vec<(ResourceKey<T>, T)> {
        assert!(
            self.values.is_empty(),
            "Registry::entry_set on a non-empty registry is unsupported until #126; \
             use entry_set_ref (borrowed) or entry_set_cloned (T: Clone)"
        );
        Vec::new()
    }

    /// `Registry.entrySet()` — owned `T: Clone` pairs for real content. The
    /// frozen `Registry<T>` is immutable, so cloning the elements is a faithful
    /// materialization of Java's unmodifiable entry view.
    pub fn entry_set_cloned(&self) -> Vec<(ResourceKey<T>, T)>
    where
        T: Clone,
    {
        self.keys
            .iter()
            .zip(self.values.iter())
            .map(|(k, v)| (k.clone(), v.as_ref().clone()))
            .collect()
    }

    /// `Registry.entrySet()` — borrowed values, the faithful form for real
    /// content. Each pair aliases the frozen registry's own element.
    pub fn entry_set_ref(&self) -> Vec<(ResourceKey<T>, &T)> {
        self.keys
            .iter()
            .zip(self.values.iter())
            .map(|(k, v)| (k.clone(), v.as_ref()))
            .collect()
    }

    /// `Registry.getTag(TagKey<T>)` — the bound members of the named set.
    /// `HolderSet.Named<T>` itself is #126; the member-id list is the SCC's
    /// minimal surface.
    pub fn get_tag(&self, tag: &TagKey<T>) -> Option<&[HolderId]> {
        self.tags.get(tag).map(Vec::as_slice)
    }

    /// `Registry.listTags()` — the named tag keys (`Stream<HolderSet.Named<T>>`
    /// narrowed to the keys; #126 widens).
    pub fn list_tags(&self) -> Vec<TagKey<T>> {
        self.tags.keys().cloned().collect()
    }

    /// `Registry.getTagOrEmpty(TagKey<T>)`.
    pub fn get_tag_or_empty(&self, tag: &TagKey<T>) -> &[HolderId] {
        self.get_tag(tag).unwrap_or(&[])
    }

    /// `Registry.asHolderIdMap()` — an `IdMap<HolderId>` over the element-id
    /// space (holder id == element id == insertion index).
    pub fn as_holder_id_map(&self) -> HolderIdMap {
        HolderIdMap {
            ids: (0..self.values.len() as u32).map(HolderId).collect(),
        }
    }
}

impl<T> fmt::Debug for Registry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Hand-written so no `T: Debug` bound leaks onto `Registry<T>`.
        f.debug_struct("Registry")
            .field("key", &self.key)
            .field("size", &self.size())
            .finish()
    }
}

impl<T> fmt::Display for Registry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Java: `MappedRegistry.toString()` =
        // `"Registry[" + key + " (" + registryLifecycle + ")]"`. DFU's STABLE/
        // EXPERIMENTAL singletons override `toString()` to "Stable"/"Experimental",
        // which `{:?}` renders identically; `Deprecated(n)` does NOT override
        // `toString()` in Java (identity hash), a documented minor divergence
        // (`{:?}` renders "Deprecated(n)").
        write!(f, "Registry[{} ({:?})]", self.key, self.lifecycle)
    }
}

impl<T> IdMap<T> for Registry<T> {
    fn get_id(&self, thing: &T) -> i32 {
        Registry::get_id(self, thing)
    }

    fn by_id(&self, id: i32) -> Option<&T> {
        Registry::by_id(self, id)
    }

    fn size(&self) -> i32 {
        Registry::size(self)
    }

    /// `IdMap.getIdOrThrow(T)` — reproduces Java's
    /// `"Can't find id for '" + value + "' in map " + this` via this registry's
    /// `Display` (the value part is unreproducible: `T` is unbounded).
    fn get_id_or_throw(&self, value: &T) -> i32 {
        let id = self.get_id(value);
        if id == DEFAULT_ID {
            panic!("Can't find id for value in map {}", self);
        }
        id
    }
}

/// `Registry.asHolderIdMap()` — the frozen `IdMap<HolderId>` adapter.
///
/// The holder id space is identical to the element id space, so the adapter is
/// a materialized `0..n` id list (the frozen registry never changes).
#[derive(Debug, Clone)]
pub struct HolderIdMap {
    ids: Vec<HolderId>,
}

impl IdMap<HolderId> for HolderIdMap {
    fn get_id(&self, thing: &HolderId) -> i32 {
        if (thing.0 as usize) < self.ids.len() {
            thing.0 as i32
        } else {
            DEFAULT_ID
        }
    }

    fn by_id(&self, id: i32) -> Option<&HolderId> {
        if id < 0 {
            return None;
        }
        self.ids.get(id as usize)
    }

    fn size(&self) -> i32 {
        self.ids.len() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RegistryBuilder;
    use rivet_util::random::LegacyRandomSource;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn element_key(id: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn registry_of(entries: &[(&str, u8)]) -> Registry<TestElement> {
        let mut builder = RegistryBuilder::new(&registry_key());
        for (name, value) in entries {
            builder.register(
                &element_key(name),
                Arc::new(TestElement(*value)),
                RegistrationInfo::BUILT_IN,
            );
        }
        builder.freeze()
    }

    fn empty_registry() -> Registry<TestElement> {
        RegistryBuilder::new(&registry_key()).freeze()
    }

    #[test]
    fn key_and_registry_id_are_set_at_construction() {
        let registry = registry_of(&[("one", 1)]);
        assert_eq!(registry.key(), &registry_key());
        // The per-instance RegistryId (assigned by the builder) survives freeze.
        assert_ne!(registry.registry_id(), RegistryId(u32::MAX));
    }

    #[test]
    fn insertion_order_is_the_id_space() {
        let registry = registry_of(&[("air", 0), ("stone", 1), ("dirt", 2)]);
        assert_eq!(registry.size(), 3);
        assert!(!registry.is_empty());
        for id in 0..3 {
            let value = registry.by_id(id).expect("by_id in range");
            assert_eq!(value, &TestElement(id as u8));
            assert_eq!(registry.get_id(value), id);
        }
        assert_eq!(registry.by_id(3), None);
        assert_eq!(registry.by_id(-1), None);
    }

    #[test]
    fn key_lookups_resolve_registered_elements() {
        let registry = registry_of(&[("one", 1), ("two", 2)]);
        let key = element_key("two");
        let value = registry.get_value(&key).expect("registered");
        assert_eq!(value, &TestElement(2));
        assert_eq!(
            registry.get_value_or_throw(&key) as *const TestElement,
            value
        );
        assert_eq!(
            registry.get_optional(&Identifier::with_default_namespace("one")),
            Some(&TestElement(1))
        );
        assert_eq!(
            registry.get_optional_by_key(&element_key("one")),
            Some(&TestElement(1))
        );
        assert!(registry.contains_key(&Identifier::with_default_namespace("one")));
        assert!(registry.contains_key_by_key(&element_key("one")));
        assert!(!registry.contains_key(&Identifier::with_default_namespace("missing")));
    }

    #[test]
    fn get_value_by_identifier_resolves_by_location() {
        let registry = registry_of(&[("one", 1)]);
        let value = registry.get_value_by_id(&Identifier::with_default_namespace("one"));
        assert_eq!(value, Some(&TestElement(1)));
        assert_eq!(
            registry.get_value_by_id(&Identifier::with_default_namespace("missing")),
            None
        );
    }

    #[test]
    fn get_key_and_resource_key_resolve_by_identity() {
        let registry = registry_of(&[("one", 1)]);
        let value = registry.get_value(&element_key("one")).unwrap();
        assert_eq!(
            registry.get_key(value),
            Some(Identifier::with_default_namespace("one"))
        );
        assert_eq!(registry.get_resource_key(value), Some(element_key("one")));
        // A foreign value (never registered) is absent from every lookup.
        let foreign = TestElement(99);
        assert_eq!(registry.get_key(&foreign), None);
        assert_eq!(registry.get_resource_key(&foreign), None);
    }

    #[test]
    fn get_id_of_unregistered_value_is_negative_one() {
        let registry = registry_of(&[("one", 1)]);
        assert_eq!(registry.get_id(&TestElement(99)), DEFAULT_ID);
        // Two Eq-equal but distinct allocations are different identities.
        let registered = registry.get_value(&element_key("one")).unwrap();
        let copy = TestElement(1);
        assert_ne!(registry.get_id(&copy), registry.get_id(registered));
    }

    #[test]
    #[should_panic(expected = "Missing key in")]
    fn get_value_or_throw_missing_key_panics() {
        let registry = registry_of(&[("one", 1)]);
        let _ = registry.get_value_or_throw(&element_key("missing"));
    }

    #[test]
    #[should_panic(expected = "No value with id 99")]
    fn by_id_or_throw_out_of_range_panics_with_exact_message() {
        let registry = registry_of(&[("one", 1)]);
        let _ = registry.by_id_or_throw(99);
    }

    #[test]
    #[should_panic(expected = "Can't find id for value in map Registry[ResourceKey[")]
    fn get_id_or_throw_unregistered_panics_with_registry_in_message() {
        let registry = registry_of(&[("one", 1)]);
        let _ = registry.get_id_or_throw(&TestElement(99));
    }

    #[test]
    fn key_set_and_registry_key_set_preserve_insertion_order() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        assert_eq!(
            registry.key_set(),
            vec![
                Identifier::with_default_namespace("air"),
                Identifier::with_default_namespace("stone")
            ]
        );
        assert_eq!(
            registry.registry_key_set(),
            vec![element_key("air"), element_key("stone")]
        );
        assert_eq!(registry.entry_set_ref().len(), 2);
    }

    #[test]
    fn get_any_returns_the_first_element() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        assert_eq!(registry.get_any(), Some(HolderId(0)));
        assert_eq!(empty_registry().get_any(), None);
    }

    #[test]
    fn get_random_is_bounded_by_size() {
        let registry = registry_of(&[("air", 0), ("stone", 1), ("dirt", 2)]);
        let mut random = LegacyRandomSource::new(1234);
        for _ in 0..100 {
            let picked = registry.get_random(&mut random).expect("non-empty");
            assert!(picked.0 < 3);
        }
        // An empty (non-defaulted) registry yields no random holder.
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(empty_registry().get_random(&mut random), None);
    }

    #[test]
    fn registration_info_is_preserved() {
        let mut builder = RegistryBuilder::new(&registry_key());
        let key = element_key("one");
        builder.register(
            &key,
            Arc::new(TestElement(1)),
            RegistrationInfo::new(None, Lifecycle::Experimental),
        );
        let registry = builder.freeze();
        assert_eq!(
            registry.registration_info(&key).map(|i| i.lifecycle),
            Some(Lifecycle::Experimental)
        );
        assert_eq!(registry.registration_info(&element_key("missing")), None);
        assert_eq!(registry.registry_lifecycle(), Lifecycle::Experimental);
    }

    #[test]
    fn tags_are_bound_and_listed() {
        let registry = registry_of(&[("one", 1)]);
        let tag = TagKey::create(
            &registry_key(),
            Identifier::with_default_namespace("test_tag"),
        );
        // No tags were bound, so the tag surface is empty (#126 widens it).
        assert_eq!(registry.list_tags(), Vec::<TagKey<TestElement>>::new());
        assert_eq!(registry.get_tag(&tag), None);
        assert_eq!(registry.get_tag_or_empty(&tag), &[]);
    }

    #[test]
    fn as_holder_id_map_is_the_id_space() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let map = registry.as_holder_id_map();
        assert_eq!(map.size(), 2);
        assert_eq!(map.get_id(&HolderId(0)), 0);
        assert_eq!(map.get_id(&HolderId(7)), DEFAULT_ID);
        assert_eq!(map.by_id(1), Some(&HolderId(1)));
        assert_eq!(map.by_id(-1), None);
        assert_eq!(map.by_id(2), None);
    }

    #[test]
    fn display_and_debug_render_java_shapes() {
        let registry = registry_of(&[("one", 1)]);
        // Java `MappedRegistry.toString()`: "Registry[key (lifecycle)]".
        assert_eq!(
            registry.to_string(),
            format!("Registry[{} ({:?})]", registry_key(), Lifecycle::Stable)
        );
        // Hand-written Debug avoids a T: Debug bound on the struct.
        let debug = format!("{:?}", registry);
        assert!(debug.starts_with("Registry { key: "));
        assert!(debug.contains("size: 1"));
    }
}

#[cfg(all(feature = "blocks", test))]
mod generated_block_tests {
    //! Generated-block-table integration tests (ownership B — the registry
    //! lifecycle). These verify that the id space (element id == holder id ==
    //! network id == insertion index) lines up with the codegen-owned
    //! `BLOCK_BY_NAME`/`BLOCK_BY_ID` tables.
    //!
    //! The element type is `BlockId` (not the ZST `BlockType` placeholder):
    //! identity-keyed value maps key by `Arc::as_ptr`, and every `Arc<ZST>`
    //! shares one static allocation, so distinct ZST registrations would look
    //! like one duplicate value (Java allocates a distinct object per value).

    use super::*;
    use crate::builder::RegistryBuilder;
    use crate::generated::blocks::{BLOCK_BY_ID, BLOCK_BY_NAME, BlockId};

    fn block_key() -> RegistryKey<BlockId> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("block"))
    }

    #[test]
    fn generated_block_ids_map_to_registry_insertion_order() {
        let mut builder = RegistryBuilder::new(&block_key());
        let keys: Vec<ResourceKey<BlockId>> = BLOCK_BY_ID
            .iter()
            .map(|name| ResourceKey::create(&block_key(), Identifier::parse(name)))
            .collect();
        for (id, key) in keys.iter().enumerate() {
            builder.register(
                key,
                Arc::new(BlockId::from_id(id as u16)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();

        assert_eq!(registry.size() as usize, BLOCK_BY_ID.len());
        for (id, name) in BLOCK_BY_ID.iter().enumerate() {
            let key = &keys[id];
            let value = registry.get_value(key).unwrap();
            assert_eq!(registry.get_id(value), id as i32);
            assert_eq!(value.id(), id as u16);
            assert_eq!(registry.get_key(value), Some(Identifier::parse(name)));
            assert!(registry.contains_key(&Identifier::parse(name)));
            assert!(registry.contains_key_by_key(key));
            assert_eq!(registry.by_id(id as i32), Some(value));
            assert_eq!(registry.get_optional(&Identifier::parse(name)), Some(value));
            assert_eq!(
                registry.get_value_or_throw(key) as *const BlockId,
                value as *const BlockId
            );
        }
        assert_eq!(registry.key_set().len(), BLOCK_BY_ID.len());
        assert_eq!(registry.registry_key_set().len(), BLOCK_BY_ID.len());
        assert_eq!(registry.entry_set_ref().len(), BLOCK_BY_ID.len());
        assert_eq!(registry.get_any(), Some(HolderId(0)));
    }

    #[test]
    fn generated_block_tables_agree_with_the_id_space() {
        // The two codegen-owned tables must line up with each other and with
        // the registry id space: BLOCK_BY_NAME is a dense 0..len bijection.
        assert_eq!(BLOCK_BY_ID.len(), BLOCK_BY_NAME.len());
        assert_eq!(
            BlockId::from_name("minecraft:air").map(BlockId::id),
            Some(0)
        );
        assert_eq!(BlockId::from_id(0).name(), "minecraft:air");
        assert!(BLOCK_BY_ID.len() > 100);
    }
}
