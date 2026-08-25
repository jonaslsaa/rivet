//! Port of `net.minecraft.core.RegistryBuilder<T>` + the pre-freeze phase of
//! `MappedRegistry<T>` (MC 26.2).
//!
//! PROVENANCE: `MappedRegistry.java` (548 lines) is the Java implementation of
//! `WritableRegistry`; `DefaultedMappedRegistry.java` (74 lines) adds the
//! default-key fallbacks. There is no Java `RegistryBuilder` class — the
//! builder is the **pre-freeze phase** of `MappedRegistry`, consumed by
//! `freeze()`. The Rust port names the mutable phase `RegistryBuilder<T>`
//! (issue #107: "mutable `RegistryBuilder<T>` → `freeze()` → frozen
//! `Registry<T>`") to make the compile-time phase split explicit.
//!
//! Binding model (OWNERSHIP.md §Registries, #107):
//! - Pre-freeze: `register(key, value, info)` (appends to `by_id`, fills
//!   `by_location`/`by_key`/`by_value`/`to_id`), `get_or_create_holder`,
//!   `create_intrusive_holder`, `bind_tags`, `create_registration_lookup`.
//!   Duplicate-key / duplicate-value checks panic with Java's exact messages
//!   (`"Adding duplicate key '...' to registry"`).
//! - **Holder ids are assigned at `register` time, never at holder-creation
//!   time.** The returned id is `values.len()` — Java's `newId = byId.size()`
//!   (MappedRegistry.java:136) — for stand-alone and intrusive holders alike.
//!   An intrusive holder or an unbound-created holder carries no pre-assigned
//!   id; only `register` decides the id, so element id == holder id == network
//!   id == insertion index for every interleaving of create/register.
//! - `freeze()` consumes the builder → `Registry<T>`, binding values to
//!   holders, and **panics with sorted unbound keys** like
//!   `MappedRegistry.freeze()` (`"Unbound values in registry ...: [...]"`),
//!   plus the unbound-tags and leftover-intrusive-holders checks.
//! - `DefaultedRegistry` folds to `Option<u32> default_id` (a field on the
//!   frozen `Registry<T>`, not a subclass) with asymmetric fallbacks:
//!   `get_value(Identifier)`/`by_id`/`get_key` fall back to the default;
//!   `get_optional(Identifier)` and `get_value(ResourceKey)` do NOT;
//!   `get_id` of a missing value returns the default's id, not `-1`;
//!   `get_any` returns the default holder.
//! - The `frozen` boolean + `validateWrite()` are compile-time phase types, not
//!   runtime checks.
//! - **Identity-sensitive value mapping** (OWNERSHIP §Registries): the
//!   duplicate-value check and `toId` lookups are keyed by element identity.
//!   Elements are held as `Arc<T>` so each registration has a stable identity
//!   address; value-keyed maps key by `Arc::as_ptr`. No `Arc<RwLock>`, no
//!   `Cell`/`RefCell` — the builder is exclusively mutable, the frozen registry
//!   is exclusively immutable.

use crate::Identifier;
use crate::ResourceKey;
use crate::TagKey;
use crate::holder::{HolderId, RegistryId};
use crate::registration_info::RegistrationInfo;
use crate::registry::{Registry, RegistryKey, StagedRegistryLease};

use rivet_serialization::lifecycle::Lifecycle;

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Monotonic per-instance registry identity source (OWNERSHIP.md §Registries:
/// `RegistryId` is a per-instance u32, distinct from the registry key). A
/// builder assigns one at construction.
static NEXT_REGISTRY_ID: AtomicU32 = AtomicU32::new(0);

/// A pre-freeze holder placeholder (see `registry.rs`'s `HolderReference`).
pub type BuilderHolder = HolderId;

/// The `RegistryBuilder<T>` — the mutable pre-freeze phase of `MappedRegistry`.
///
/// Consumed by `freeze()`; there is no way to mutate a frozen `Registry<T>`.
#[derive(Debug)]
pub struct RegistryBuilder<T> {
    /// `MappedRegistry.key`.
    key: RegistryKey<T>,
    /// Per-instance identity, fixed at construction.
    registry_id: RegistryId,
    /// `MappedRegistry.byId` — insertion order; element id == holder id ==
    /// network id == index.
    values: Vec<Arc<T>>,
    /// Element keys parallel to `values`.
    keys: Vec<ResourceKey<T>>,
    /// `MappedRegistry.byLocation`.
    by_location: HashMap<Identifier, u32>,
    /// `MappedRegistry.byKey`.
    by_key: HashMap<ResourceKey<T>, u32>,
    /// `MappedRegistry.toId`/`byValue` — element identity (`Arc::as_ptr`) -> id.
    by_value: HashMap<usize, u32>,
    /// `MappedRegistry.registrationInfos`.
    registration_infos: HashMap<ResourceKey<T>, RegistrationInfo>,
    /// `MappedRegistry.registryLifecycle`.
    lifecycle: Lifecycle,
    /// `DefaultedRegistry` fold — the default element's id.
    default_id: Option<u32>,
    /// `DefaultedMappedRegistry.defaultKey`.
    default_key: Option<Identifier>,
    /// Tag -> bound member holder ids (pre-freeze; `HolderSet.Named` is #126).
    tags: HashMap<TagKey<T>, Vec<HolderId>>,
    /// `MappedRegistry.unregisteredIntrusiveHolders` — element identity ->
    /// held Arc. The builder OWNS the value until `register` moves it into
    /// `values` (Java's map holds the value itself, keeping its identity
    /// stable), and only the pointer is keyed so leaving the holder
    /// unregistered keeps the allocation alive. **No id is stored**: Java
    /// assigns a `Holder.Reference`'s numeric id at `register` time
    /// (`byId.size()`), so `register` — never this map — decides the id.
    intrusive: Option<HashMap<usize, Arc<T>>>,
    /// Stand-alone holders created via `get_or_create_holder` but not yet
    /// registered — key -> provisional holder id. `computeIfAbsent` semantics:
    /// a repeat call returns the same provisional id, and `freeze()` reports
    /// each unbound key exactly once.
    pending_unbound: HashMap<ResourceKey<T>, HolderId>,
    /// Set while a staged registry with this builder's identity is alive.
    /// Owner mutation/freezing is blocked until the staged registry is dropped.
    stage_active: Arc<AtomicBool>,
}

/// A key-only registry builder borrowed from its final owner.
///
/// This type exists only for mutually recursive registry decoding. Its lifetime
/// is tied to the final [`RegistryBuilder`], so the owner cannot be frozen or
/// otherwise reused until this staged view is dropped.
pub struct StagedRegistryBuilder<'a, T> {
    inner: RegistryBuilder<T>,
    owner: PhantomData<&'a mut RegistryBuilder<T>>,
    lease: Option<StagedRegistryLease>,
}

impl<'a, T> StagedRegistryBuilder<'a, T> {
    /// The staged view's reserved registry identity.
    pub fn registry_id(&self) -> RegistryId {
        self.inner.registry_id()
    }

    /// Register a key-only placeholder in the staged view.
    pub fn register(
        &mut self,
        key: &ResourceKey<T>,
        value: Arc<T>,
        info: RegistrationInfo,
    ) -> BuilderHolder {
        self.inner.register(key, value, info)
    }

    /// Freeze the staged view after all recursive keys have been reserved.
    pub fn freeze(mut self) -> Registry<T> {
        let lease = self
            .lease
            .take()
            .expect("staged registry lease already consumed");
        self.inner.freeze_with_lease(Some(lease))
    }
}

impl<T> RegistryBuilder<T> {
    /// `MappedRegistry(ResourceKey, Lifecycle)` / `DefaultedMappedRegistry(...)`.
    ///
    /// Java's constructors take an initial `Lifecycle`; the SCC's Stable-only
    /// usage hardcodes `Lifecycle::Stable` here (a later unit widens the
    /// signature when a non-Stable initial lifecycle exists).
    pub fn new(key: &ResourceKey<Registry<T>>) -> Self {
        Self::with_intrusive(key, false)
    }

    /// Borrow this builder while constructing a key-only staged registry view.
    ///
    /// The staged view reuses the owner's `RegistryId` only while the owner is
    /// reserved by the lease. It snapshots all owner entries so pre-existing
    /// registrations remain visible during decoding; the owner cannot mutate or
    /// freeze until the temporary view is dropped.
    pub fn staged(&mut self) -> StagedRegistryBuilder<'_, T> {
        assert!(
            !self.stage_active.swap(true, Ordering::AcqRel),
            "registry builder already has a staged view"
        );
        let mut inner =
            Self::with_intrusive_and_id(&self.key, self.intrusive.is_some(), self.registry_id);
        inner.values = self.values.clone();
        inner.keys = self.keys.clone();
        inner.by_location = self.by_location.clone();
        inner.by_key = self.by_key.clone();
        inner.by_value = self.by_value.clone();
        inner.registration_infos = self.registration_infos.clone();
        inner.lifecycle = self.lifecycle;
        inner.default_id = self.default_id;
        inner.default_key = self.default_key.clone();
        inner.tags = self.tags.clone();
        inner.intrusive = self.intrusive.clone();
        inner.pending_unbound = self.pending_unbound.clone();
        StagedRegistryBuilder {
            inner,
            owner: PhantomData,
            lease: Some(StagedRegistryLease::new(Arc::clone(&self.stage_active))),
        }
    }

    /// `MappedRegistry(ResourceKey, Lifecycle, boolean intrusiveHolders)`.
    ///
    /// The initial `Lifecycle` param is likewise Stable-only for #124 (see
    /// `new`).
    pub fn new_with_intrusive(key: &ResourceKey<Registry<T>>) -> Self {
        Self::with_intrusive(key, true)
    }

    fn with_intrusive(key: &ResourceKey<Registry<T>>, intrusive_holders: bool) -> Self {
        let registry_id = RegistryId(NEXT_REGISTRY_ID.fetch_add(1, Ordering::Relaxed));
        Self::with_intrusive_and_id(key, intrusive_holders, registry_id)
    }

    fn with_intrusive_and_id(
        key: &ResourceKey<Registry<T>>,
        intrusive_holders: bool,
        registry_id: RegistryId,
    ) -> Self {
        RegistryBuilder {
            key: key.clone(),
            registry_id,
            values: Vec::new(),
            keys: Vec::new(),
            by_location: HashMap::new(),
            by_key: HashMap::new(),
            by_value: HashMap::new(),
            registration_infos: HashMap::new(),
            lifecycle: Lifecycle::Stable,
            default_id: None,
            default_key: None,
            tags: HashMap::new(),
            intrusive: intrusive_holders.then(HashMap::new),
            pending_unbound: HashMap::new(),
            stage_active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// `DefaultedMappedRegistry(String defaultKey, ResourceKey, Lifecycle, boolean)`.
    pub fn new_defaulted(default_key: &Identifier, key: &ResourceKey<Registry<T>>) -> Self {
        let mut builder = Self::with_intrusive(key, false);
        builder.default_key = Some(default_key.clone());
        builder
    }

    /// The builder's `RegistryId`.
    pub fn registry_id(&self) -> RegistryId {
        self.registry_id
    }

    fn assert_stage_inactive(&self) {
        assert!(
            !self.stage_active.load(Ordering::Acquire),
            "registry builder cannot be reused while its staged registry is alive"
        );
    }

    /// Identity helper — `Arc::as_ptr` of a freshly-wrapped value.
    fn identity(value: &Arc<T>) -> usize {
        Arc::as_ptr(value) as usize
    }

    /// `WritableRegistry.register(ResourceKey<T>, Arc<T>, RegistrationInfo)` →
    /// `Holder.Reference<T>`.
    ///
    /// The value arrives already wrapped in an `Arc` — the registry's element
    /// storage is `Vec<Arc<T>>` (OWNERSHIP §Registries), and the `Arc` IS the
    /// element identity: Java's `createIntrusiveHolder(this)` + `register(key,
    /// this, ...)` pairs flow the same object through both calls, which Rust
    /// reproduces by passing `Arc::clone`s (same allocation, same
    /// `Arc::as_ptr`).
    ///
    /// The returned holder id is **always `values.len()`** — the registration
    /// insertion index, Java's `newId = byId.size()` (MappedRegistry.java:136)
    /// — for intrusive and stand-alone holders alike. An intrusive holder
    /// created earlier contributes no pre-assigned id: `register` only removes
    /// its value from the unregistered map (liveness + identity check) and the
    /// id is the index where it lands. Interleaved
    /// `create(A); create(B); register(A); register(B)` therefore yields
    /// `A -> 0`, `B -> 1`, never two holders aliasing one placeholder.
    ///
    /// Panics with Java's exact duplicate messages:
    /// `"Adding duplicate key '<key>' to registry"` /
    /// `"Adding duplicate value '<value>' to registry"` (identity-sensitive: a
    /// value that aliases a stored element — the same `Arc` — not an
    /// `Eq`-equal one).
    pub fn register(
        &mut self,
        key: &ResourceKey<T>,
        value: Arc<T>,
        info: RegistrationInfo,
    ) -> BuilderHolder {
        self.assert_stage_inactive();
        if self.by_location.contains_key(key.identifier()) {
            panic!("Adding duplicate key '{}' to registry", key);
        }

        let arc = value;
        let identity = Self::identity(&arc);
        if self.by_value.contains_key(&identity) {
            panic!("Adding duplicate value to registry");
        }

        // The intrusive map holds the Arc (same allocation as `arc` — the
        // identity matched), so the value moves from the map into `values`.
        if let Some(intrusive) = &mut self.intrusive
            && intrusive.remove(&identity).is_none()
        {
            panic!("Missing intrusive holder for {}:{}", key, "value");
        }

        let id = self.values.len() as u32;
        self.values.push(arc);
        self.keys.push(key.clone());
        self.by_location.insert(key.identifier().clone(), id);
        self.by_key.insert(key.clone(), id);
        self.by_value.insert(identity, id);
        self.registration_infos.insert(key.clone(), info.clone());
        self.lifecycle = self.lifecycle.add(info.lifecycle());
        if self.default_key.as_ref() == Some(key.identifier()) {
            self.default_id = Some(id);
        }
        self.pending_unbound.remove(key);
        HolderId(id)
    }

    /// `WritableRegistry.getOrCreateHolder(ResourceKey<T>)` — the builder-side
    /// of `getOrCreateHolderOrThrow`. A key not yet registered gets a
    /// stand-alone holder placeholder; if never registered, `freeze()` panics
    /// with it as an unbound value. The returned `HolderId` is provisional
    /// (`values.len()` at creation) and stable across repeat calls
    /// (`computeIfAbsent`). The #126 `Holder::Reference` is built on demand by
    /// `RegistryLookup::get` once the key registers — the builder's placeholder
    /// is a bare key, not a `Holder`.
    ///
    /// Java's `getOrCreateHolderOrThrow` (MappedRegistry.java:198-207) throws
    /// `"This registry can't create new holders without value"` on an intrusive
    /// builder — an intrusive registry only ever creates holders from values,
    /// never from bare keys. Preserved here exactly.
    pub fn get_or_create_holder(&mut self, key: &ResourceKey<T>) -> BuilderHolder {
        self.assert_stage_inactive();
        if let Some(&id) = self.by_key.get(key) {
            return HolderId(id);
        }
        if self.intrusive.is_some() {
            panic!("This registry can't create new holders without value");
        }
        if let Some(&id) = self.pending_unbound.get(key) {
            return id;
        }
        let provisional = HolderId(self.values.len() as u32);
        self.pending_unbound.insert(key.clone(), provisional);
        provisional
    }

    /// `WritableRegistry.createIntrusiveHolder(Arc<T>)` — pre-registers a value
    /// as an intrusive holder; the value must later be `register`ed with the
    /// same allocation (Java panics "Missing intrusive holder" otherwise).
    ///
    /// **No id is assigned here.** Java's `createIntrusiveHolder`
    /// (MappedRegistry.java:347-354) builds a `Holder.Reference` via
    /// `computeIfAbsent` and returns it WITHOUT binding an id — the numeric id
    /// does not exist until `register` adds the holder to `byId`. Returning a
    /// `HolderId(self.values.len())` snapshot here would alias later holders
    /// (interleaved create/register), so the returned placeholder id is always
    /// `0` and the real id is decided by `register` (`values.len()`). Callers
    /// of `create_intrusive_holder` must not treat the return as a final id —
    /// match `register`'s return, as Java callers match `register`'s.
    pub fn create_intrusive_holder(&mut self, value: Arc<T>) -> BuilderHolder {
        self.assert_stage_inactive();
        let intrusive = self
            .intrusive
            .as_mut()
            .expect("This registry can't create intrusive holders");
        let identity = Self::identity(&value);
        // Java keeps the value itself (by identity) in the map, so the object
        // stays alive until registered; we hold the Arc for the same reason.
        intrusive.insert(identity, value);
        HolderId(0)
    }

    /// `WritableRegistry.createRegistrationLookup()`.
    ///
    /// Java (MappedRegistry.java:395-418) returns a live `HolderGetter<T>` whose
    /// `get`/`getOrThrow` create holders on demand and throw on intrusive
    /// registries. RivetTodo(#126): the frozen `Registry<T>`'s
    /// `RegistryLookup` (`holder_lookup.rs`) covers the post-freeze getter; the
    /// pre-freeze registration-lookup (a live builder getter) is not ported —
    /// a placeholder would be a plausible-but-wrong holder, so this fails
    /// loudly.
    pub fn create_registration_lookup(&self) -> BuilderHolder {
        panic!(
            "createRegistrationLookup's pre-freeze HolderGetter is #126 (holder \
             codecs); not implemented"
        )
    }

    /// `WritableRegistry.bindTags(Map<TagKey<T>, List<Holder<T>>>)` — stores the
    /// tag member ids (pre-freeze). Member holders must reference registered
    /// elements in this minimal shape; `HolderSet.Named` binding is #126.
    pub fn bind_tags(&mut self, pending: Vec<(TagKey<T>, Vec<BuilderHolder>)>) {
        self.assert_stage_inactive();
        for (tag, holders) in pending {
            self.tags.insert(tag, holders);
        }
    }

    /// `WritableRegistry.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `RegistryBuilder` → `Registry<T>`.
    ///
    /// Consumes the builder. Panics exactly like `MappedRegistry.freeze()`:
    /// - `"Unbound values in registry {key}: [{sorted identifiers}]"` for keys
    ///   created via `get_or_create_holder` but never registered. Java sorts
    ///   the `List<Identifier>` by `Identifier.compareTo` (**path first, then
    ///   namespace**); the sort uses `Identifier`'s `Ord` (same order), and the
    ///   list renders as `"ns:path"` entries joined like `List.toString()`.
    /// - `"Some intrusive holders were not registered: ..."` for leftover
    ///   intrusive holders.
    ///
    /// (The "Tags already present" / "Unbound tags" panics are structurally
    /// impossible here: tags only exist after `bind_tags` in this minimal
    /// shape, and a consumed builder cannot be frozen twice.)
    pub fn freeze(self) -> Registry<T> {
        self.assert_stage_inactive();
        self.freeze_with_lease(None)
    }

    fn freeze_with_lease(self, staged_lease: Option<StagedRegistryLease>) -> Registry<T> {
        let mut unbound: Vec<Identifier> = self
            .pending_unbound
            .keys()
            .filter(|key| !self.by_key.contains_key(*key))
            .map(|key| key.identifier().clone())
            .collect();
        unbound.sort();
        if !unbound.is_empty() {
            let rendered: Vec<String> = unbound.iter().map(ToString::to_string).collect();
            panic!(
                "Unbound values in registry {}: [{}]",
                self.key,
                rendered.join(", ")
            );
        }

        if let Some(intrusive) = &self.intrusive
            && !intrusive.is_empty()
        {
            // Java prints the leftover `Holder.Reference` values; the SCC's
            // minimal shape prints a per-entry count (the intrusive map no
            // longer stores ids — Java assigns them at register time — and T
            // is unbounded, so the values themselves are not renderable).
            let leftover = intrusive.len();
            panic!(
                "Some intrusive holders were not registered: {} leftover",
                leftover
            );
        }

        Registry::from_builder(
            self.key,
            self.registry_id,
            self.values,
            self.keys,
            self.by_location,
            self.by_key,
            self.by_value,
            self.registration_infos,
            self.lifecycle,
            self.default_id,
            self.default_key,
            self.tags,
            staged_lease,
        )
    }

    /// `DefaultedRegistry.getDefaultKey()`.
    pub fn get_default_key(&self) -> Option<Identifier> {
        self.default_key.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryKey;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn element_key(id: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&key(), Identifier::with_default_namespace(id))
    }

    fn register(builder: &mut RegistryBuilder<TestElement>, id: &str, value: u8) {
        builder.register(
            &element_key(id),
            Arc::new(TestElement(value)),
            RegistrationInfo::BUILT_IN,
        );
    }

    fn catch(f: impl FnOnce()) -> String {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let msg = result.expect_err("expected a panic");
        msg.downcast_ref::<String>()
            .cloned()
            .or_else(|| msg.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    #[test]
    fn each_builder_gets_a_distinct_registry_id() {
        let a = RegistryBuilder::<TestElement>::new(&key());
        let b = RegistryBuilder::<TestElement>::new(&key());
        assert_ne!(a.registry_id(), b.registry_id());
        assert_ne!(a.registry_id(), RegistryId(u32::MAX));
    }

    #[test]
    fn staged_builder_reserves_owner_identity_until_drop() {
        let mut owner = RegistryBuilder::<TestElement>::new(&key());
        register(&mut owner, "existing", 7);
        let owner_id = owner.registry_id();
        let staged = owner.staged();
        assert_eq!(staged.registry_id(), owner_id);
        let staged_registry = staged.freeze();
        assert_eq!(staged_registry.registry_id(), owner_id);
        assert_eq!(staged_registry.size(), 1);
        assert_eq!(
            staged_registry.get_value(&element_key("existing")),
            Some(&TestElement(7))
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            register(&mut owner, "new", 8);
        }));
        assert!(
            result.is_err(),
            "owner reuse must be blocked by the staged lease"
        );

        drop(staged_registry);
        register(&mut owner, "new", 8);
        let frozen = owner.freeze();
        assert_eq!(frozen.registry_id(), owner_id);
        assert_eq!(frozen.size(), 2);
    }

    #[test]
    fn staged_registration_failure_does_not_publish_or_corrupt_owner() {
        let mut owner = RegistryBuilder::<TestElement>::new(&key());
        register(&mut owner, "existing", 7);
        let owner_id = owner.registry_id();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut staged = owner.staged();
            // The snapshot includes the pre-existing key, so a failed staged
            // registration cannot be mistaken for a successful publication.
            staged.register(
                &element_key("existing"),
                Arc::new(TestElement(8)),
                RegistrationInfo::BUILT_IN,
            );
        }));
        assert!(result.is_err());
        assert_eq!(owner.registry_id(), owner_id);
        let frozen = owner.freeze();
        assert_eq!(frozen.registry_id(), owner_id);
        assert_eq!(frozen.size(), 1);
        assert_eq!(
            frozen.get_value(&element_key("existing")),
            Some(&TestElement(7))
        );
    }

    #[test]
    fn register_assigns_ids_in_insertion_order() {
        let mut builder = RegistryBuilder::new(&key());
        let one = element_key("one");
        let two = element_key("two");
        assert_eq!(
            builder.register(&one, Arc::new(TestElement(1)), RegistrationInfo::BUILT_IN),
            HolderId(0)
        );
        assert_eq!(
            builder.register(&two, Arc::new(TestElement(2)), RegistrationInfo::BUILT_IN),
            HolderId(1)
        );
    }

    #[test]
    fn duplicate_key_panics_with_exact_message() {
        let mut builder = RegistryBuilder::new(&key());
        let k = element_key("one");
        register(&mut builder, "one", 1);
        // Java: "Adding duplicate key '<key>' to registry".
        let msg = catch(|| {
            builder.register(&k, Arc::new(TestElement(1)), RegistrationInfo::BUILT_IN);
        });
        assert!(msg.contains("Adding duplicate key"));
        assert!(msg.contains("minecraft:one"));
    }

    #[test]
    fn duplicate_value_panics_by_identity() {
        let mut builder = RegistryBuilder::new(&key());
        // Two distinct allocations with Eq-equal values are DIFFERENT
        // identities (Java IdentityHashMap) — both register fine.
        register(&mut builder, "one", 1);
        register(&mut builder, "two", 1);
        // Re-registering the SAME allocation is a duplicate — identity is the
        // Arc address, and Arc::clone preserves it.
        let shared = Arc::new(TestElement(3));
        builder.register(
            &element_key("three"),
            shared.clone(),
            RegistrationInfo::BUILT_IN,
        );
        let msg = catch(|| {
            builder.register(&element_key("four"), shared, RegistrationInfo::BUILT_IN);
        });
        assert!(msg.contains("Adding duplicate value to registry"));
    }

    #[test]
    fn get_or_create_holder_registers_unbound_then_resolves() {
        let mut builder = RegistryBuilder::new(&key());
        let k = element_key("one");
        let holder = builder.get_or_create_holder(&k);
        assert_eq!(holder, HolderId(0));
        // After registration the holder resolves to the element's id.
        register(&mut builder, "one", 1);
        assert_eq!(builder.get_or_create_holder(&k), HolderId(0));
        // A key that is registered never joins the unbound list.
        let frozen = builder.freeze();
        assert_eq!(frozen.get_value(&k), Some(&TestElement(1)));
    }

    #[test]
    #[should_panic(expected = "This registry can't create new holders without value")]
    fn intrusive_builder_rejects_get_or_create_holder() {
        // Java `getOrCreateHolderOrThrow` (MappedRegistry.java:198-207): an
        // intrusive registry only creates holders from values, never from bare
        // keys — the unregistered-intrusive-holders guard throws.
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        let _ = builder.get_or_create_holder(&element_key("one"));
    }

    #[test]
    #[should_panic(expected = "pre-freeze HolderGetter is #126 (holder codecs)")]
    fn create_registration_lookup_is_deferred_to_126() {
        // Java returns a real HolderGetter (MappedRegistry.java:395-418); the
        // SCC must not return a plausible-but-wrong holder placeholder, so the
        // deferred surface fails loudly instead. #126 delivers the frozen
        // `RegistryLookup` (`holder_lookup.rs`); the pre-freeze live builder
        // getter stays deferred.
        let mut builder = RegistryBuilder::new(&key());
        register(&mut builder, "one", 1);
        let _ = builder.create_registration_lookup();
    }

    #[test]
    fn intrusive_holders_are_bound_to_the_registered_value() {
        // Java: Block calls `createIntrusiveHolder(this)`, then `register(key,
        // this, ...)`. The same object flows through both — Rust reproduces it
        // with Arc::clone (same allocation, same identity). The id is assigned
        // by `register` (`values.len()`), not by `create_intrusive_holder`.
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        let value = Arc::new(TestElement(7));
        let placeholder = builder.create_intrusive_holder(value.clone());
        // The placeholder carries no final id — only register's return matters.
        assert_eq!(placeholder, HolderId(0));
        let registered = builder.register(
            &element_key("seven"),
            value.clone(),
            RegistrationInfo::BUILT_IN,
        );
        assert_eq!(registered, HolderId(0));
        let frozen = builder.freeze();
        assert_eq!(frozen.get_id(Arc::as_ref(&value)), 0);
        assert_eq!(
            frozen.get_value(&element_key("seven")),
            Some(&TestElement(7))
        );
    }

    #[test]
    fn interleaved_intrusive_creates_and_registers_assign_distinct_ids() {
        // The regression: create(A); create(B); register(A); register(B) must
        // yield A -> 0, B -> 1. A holder created before B must not pin the
        // stale `values.len()` snapshot so B aliases A's id.
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        let a = Arc::new(TestElement(1));
        let b = Arc::new(TestElement(2));
        let placeholder_a = builder.create_intrusive_holder(a.clone());
        let placeholder_b = builder.create_intrusive_holder(b.clone());
        // Both placeholders are equal (no id yet) — Java's `createIntrusiveHolder`
        // returns an id-less `Holder.Reference`.
        assert_eq!(placeholder_a, placeholder_b);
        assert_eq!(placeholder_a, HolderId(0));

        let registered_a =
            builder.register(&element_key("a"), a.clone(), RegistrationInfo::BUILT_IN);
        let registered_b =
            builder.register(&element_key("b"), b.clone(), RegistrationInfo::BUILT_IN);
        assert_eq!(registered_a, HolderId(0));
        assert_eq!(registered_b, HolderId(1));

        let frozen = builder.freeze();
        assert_eq!(frozen.get_id(Arc::as_ref(&a)), 0);
        assert_eq!(frozen.get_id(Arc::as_ref(&b)), 1);
        assert_eq!(frozen.get_any(), Some(HolderId(0)));
        assert_eq!(
            frozen.get_key(Arc::as_ref(&b)),
            Some(Identifier::with_default_namespace("b"))
        );
    }

    #[test]
    fn reversed_intrusive_create_and_register_order_still_uses_insertion_index() {
        // create(A); create(B); register(B); register(A): the id is the
        // insertion index at REGISTER time, not the creation order.
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        let a = Arc::new(TestElement(1));
        let b = Arc::new(TestElement(2));
        builder.create_intrusive_holder(a.clone());
        builder.create_intrusive_holder(b.clone());
        let registered_b =
            builder.register(&element_key("b"), b.clone(), RegistrationInfo::BUILT_IN);
        let registered_a =
            builder.register(&element_key("a"), a.clone(), RegistrationInfo::BUILT_IN);
        assert_eq!(registered_b, HolderId(0));
        assert_eq!(registered_a, HolderId(1));
        let frozen = builder.freeze();
        assert_eq!(frozen.get_id(Arc::as_ref(&b)), 0);
        assert_eq!(frozen.get_id(Arc::as_ref(&a)), 1);
    }

    #[test]
    fn interleaved_intrusive_registration_tag_bind_freeze_and_lookups() {
        // Full lifecycle over the interleaved order: tag/bind, freeze, and
        // every lookup sees the same id space.
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        let a = Arc::new(TestElement(1));
        let b = Arc::new(TestElement(2));
        builder.create_intrusive_holder(a.clone());
        builder.create_intrusive_holder(b.clone());
        let registered_a =
            builder.register(&element_key("a"), a.clone(), RegistrationInfo::BUILT_IN);
        let registered_b =
            builder.register(&element_key("b"), b.clone(), RegistrationInfo::BUILT_IN);
        let tag = TagKey::create(&key(), Identifier::with_default_namespace("group"));
        builder.bind_tags(vec![(tag.clone(), vec![registered_a, registered_b])]);

        let frozen = builder.freeze();
        assert_eq!(frozen.size(), 2);
        // element id == holder id == network id == insertion index.
        assert_eq!(frozen.get_id(Arc::as_ref(&a)), 0);
        assert_eq!(frozen.get_id(Arc::as_ref(&b)), 1);
        assert_eq!(frozen.get_tag(&tag), Some(&[HolderId(0), HolderId(1)][..]));
        assert_eq!(frozen.get_tag_or_empty(&tag), &[HolderId(0), HolderId(1)]);
        assert_eq!(frozen.list_tags(), vec![tag.clone()]);
        assert_eq!(frozen.get_value(&element_key("a")), Some(&TestElement(1)));
        assert_eq!(frozen.get_value(&element_key("b")), Some(&TestElement(2)));
        assert_eq!(frozen.by_id(0), Some(&TestElement(1)));
        assert_eq!(frozen.by_id(1), Some(&TestElement(2)));
        assert_eq!(
            frozen.key_set(),
            vec![
                Identifier::with_default_namespace("a"),
                Identifier::with_default_namespace("b")
            ]
        );
        assert_eq!(
            frozen.registry_key_set(),
            vec![element_key("a"), element_key("b")]
        );
        assert_eq!(frozen.get_any(), Some(HolderId(0)));
    }

    #[test]
    fn interleaved_unbound_holder_and_registrations_assign_insertion_index_ids() {
        // An unbound-created holder (stand-alone, via get_or_create_holder)
        // carries no pre-assigned id either: the key registered LAST lands at
        // the insertion index at register time. create(x); register(a);
        // register(x) => x -> 1, not 0.
        let mut builder = RegistryBuilder::new(&key());
        let x = builder.get_or_create_holder(&element_key("x"));
        // Placeholder equals 0 (values.len() at creation), but that is NOT the
        // final id.
        assert_eq!(x, HolderId(0));
        register(&mut builder, "a", 1);
        let registered_x = builder.register(
            &element_key("x"),
            Arc::new(TestElement(2)),
            RegistrationInfo::BUILT_IN,
        );
        assert_eq!(registered_x, HolderId(1));
        // get_or_create_holder now resolves to the registered id.
        assert_eq!(builder.get_or_create_holder(&element_key("x")), HolderId(1));
        let frozen = builder.freeze();
        assert_eq!(frozen.get_any(), Some(HolderId(0)));
        assert_eq!(frozen.by_id(1), Some(&TestElement(2)));
        assert_eq!(frozen.by_id(0), Some(&TestElement(1)));
    }

    #[test]
    fn intrusive_holder_for_unregistered_value_panics_on_register() {
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        // Java: createIntrusiveHolder(v) pre-registers v; register(k, v) must
        // then come in with the SAME allocation. A different allocation misses
        // the intrusive holder -> "Missing intrusive holder for key:value".
        builder.create_intrusive_holder(Arc::new(TestElement(1)));
        let msg = catch(|| {
            builder.register(
                &element_key("one"),
                Arc::new(TestElement(2)),
                RegistrationInfo::BUILT_IN,
            );
        });
        assert!(msg.contains("Missing intrusive holder"));
    }

    #[test]
    #[should_panic(expected = "This registry can't create intrusive holders")]
    fn non_intrusive_registry_rejects_intrusive_holders() {
        let mut builder = RegistryBuilder::<TestElement>::new(&key());
        let _ = builder.create_intrusive_holder(Arc::new(TestElement(1)));
    }

    #[test]
    fn freeze_panics_with_sorted_unbound_keys() {
        let mut builder = RegistryBuilder::new(&key());
        // A key that is unbound-created AND then registered is not reported.
        builder.get_or_create_holder(&element_key("zeta"));
        register(&mut builder, "zeta", 1);
        // Unbound keys are reported once each (computeIfAbsent returns the same
        // holder on repeat calls), sorted path-first like Identifier.compareTo —
        // the HashMap iteration order is irrelevant because the list is sorted.
        builder.get_or_create_holder(&element_key("alpha"));
        builder.get_or_create_holder(&element_key("mid"));
        builder.get_or_create_holder(&element_key("alpha"));
        let msg = catch(|| {
            builder.freeze();
        });
        assert!(msg.contains("Unbound values in registry"));
        let alpha = msg.find("alpha").unwrap();
        let mid = msg.find("mid").unwrap();
        assert!(alpha < mid);
        assert!(msg.contains("[minecraft:alpha, minecraft:mid]"));
        assert!(!msg.contains("zeta"));
    }

    #[test]
    fn get_or_create_holder_is_stable_for_an_unbound_key() {
        let mut builder = RegistryBuilder::new(&key());
        let k = element_key("one");
        let first = builder.get_or_create_holder(&k);
        // A repeat call returns the same provisional holder (computeIfAbsent),
        // even if a new element was registered in between.
        register(&mut builder, "two", 2);
        assert_eq!(builder.get_or_create_holder(&k), first);
        // The key's provisional id stays its original value.len().
        assert_eq!(first, HolderId(0));
    }

    #[test]
    fn freeze_rejects_leftover_intrusive_holders() {
        let mut builder = RegistryBuilder::new_with_intrusive(&key());
        builder.create_intrusive_holder(Arc::new(TestElement(1)));
        let msg = catch(|| {
            builder.freeze();
        });
        assert!(msg.contains("Some intrusive holders were not registered"));
    }

    #[test]
    fn defaulted_registry_sets_default_id_when_default_key_registers() {
        let mut builder =
            RegistryBuilder::new_defaulted(&Identifier::with_default_namespace("air"), &key());
        assert_eq!(
            builder.get_default_key(),
            Some(Identifier::with_default_namespace("air"))
        );
        // Before the default key registers, there is no default element.
        register(&mut builder, "stone", 1);
        let frozen = builder.freeze();
        assert_eq!(
            frozen.get_value_by_id(&Identifier::with_default_namespace("missing")),
            None
        );

        let mut builder =
            RegistryBuilder::new_defaulted(&Identifier::with_default_namespace("air"), &key());
        register(&mut builder, "air", 0);
        register(&mut builder, "stone", 1);
        let frozen = builder.freeze();
        // Asymmetric fallbacks: by_id / get_value(Identifier) / get_key fall
        // back to the default; get_value(ResourceKey) and get_optional do not.
        assert_eq!(frozen.by_id(99), Some(&TestElement(0)));
        assert_eq!(frozen.by_id(-1), Some(&TestElement(0)));
        assert_eq!(
            frozen.get_value_by_id(&Identifier::with_default_namespace("missing")),
            Some(&TestElement(0))
        );
        assert_eq!(frozen.get_id(&TestElement(99)), 0);
        assert_eq!(
            frozen.get_key(&TestElement(99)),
            Some(Identifier::with_default_namespace("air"))
        );
        assert_eq!(frozen.get_value(&element_key("missing")), None);
        assert_eq!(
            frozen.get_optional(&Identifier::with_default_namespace("missing")),
            None
        );
        assert_eq!(frozen.get_any(), Some(HolderId(0)));
        assert_eq!(frozen.key_set().len(), 2);
        assert_eq!(frozen.registry_key_set().len(), 2);
    }

    #[test]
    fn bind_tags_stores_members_for_frozen_lookup() {
        let mut builder = RegistryBuilder::new(&key());
        register(&mut builder, "one", 1);
        let tag = TagKey::create(&key(), Identifier::with_default_namespace("group"));
        builder.bind_tags(vec![(tag.clone(), vec![HolderId(0)])]);
        let frozen = builder.freeze();
        assert_eq!(frozen.list_tags(), vec![tag.clone()]);
        assert_eq!(frozen.get_tag(&tag), Some(&[HolderId(0)][..]));
        assert_eq!(frozen.get_tag_or_empty(&tag), &[HolderId(0)]);
    }

    #[test]
    fn lifecycle_accumulates_across_registrations() {
        let mut builder = RegistryBuilder::new(&key());
        assert_eq!(builder.lifecycle, Lifecycle::Stable);
        builder.register(
            &element_key("one"),
            Arc::new(TestElement(1)),
            RegistrationInfo::new(None, Lifecycle::Experimental),
        );
        builder.register(
            &element_key("two"),
            Arc::new(TestElement(2)),
            RegistrationInfo::new(None, Lifecycle::Deprecated(12)),
        );
        // Experimental wins (Lifecycle.add).
        assert_eq!(builder.lifecycle, Lifecycle::Experimental);
        assert_eq!(
            builder.freeze().registry_lifecycle(),
            Lifecycle::Experimental
        );
    }

    #[test]
    fn builder_is_empty_until_first_register() {
        let mut builder = RegistryBuilder::<TestElement>::new(&key());
        assert!(builder.is_empty());
        register(&mut builder, "one", 1);
        assert!(!builder.is_empty());
    }
}
