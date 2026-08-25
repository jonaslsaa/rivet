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
use crate::registry::{Registry, RegistryKey, RegistryParts};

use rivet_serialization::lifecycle::Lifecycle;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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
}

/// An ownership transaction for mutually recursive registry decoding.
///
/// Construction consumes the one authoritative builder. While the transaction
/// is building, it is the only mutable owner; after `freeze`, it is the only
/// owner of the frozen table. `take_frozen` and `adopt_frozen` are the explicit
/// hand-off around a temporary `RegistryAccess`: callers must adopt the table
/// before mutating it again. No replacement builder or identity lease exists,
/// so one `RegistryId` cannot name two live phases.
pub struct RegistryBuilderTransaction<T> {
    state: TransactionState<T>,
}

enum TransactionState<T> {
    Building(RegistryBuilder<T>),
    Frozen(Registry<T>),
    Empty,
}

impl<T> RegistryBuilderTransaction<T> {
    /// The transaction's single logical registry identity.
    pub fn registry_id(&self) -> RegistryId {
        match &self.state {
            TransactionState::Building(builder) => builder.registry_id(),
            TransactionState::Frozen(registry) => registry.registry_id(),
            TransactionState::Empty => panic!("registry transaction has no owner"),
        }
    }

    /// Borrow the authoritative mutable builder while it is in the building
    /// phase. A frozen transaction has no mutable owner until it is adopted.
    pub fn builder_mut(&mut self) -> &mut RegistryBuilder<T> {
        match &mut self.state {
            TransactionState::Building(builder) => builder,
            TransactionState::Frozen(_) => panic!("registry transaction is frozen"),
            TransactionState::Empty => panic!("registry transaction has no owner"),
        }
    }

    /// Freeze the authoritative builder in place and return the sole frozen
    /// registry view. Validation occurs before the state is moved, so a panic
    /// leaves the builder (including intrusive and pending-unbound holders)
    /// available for recovery.
    pub fn freeze(&mut self) -> &Registry<T> {
        match &self.state {
            TransactionState::Building(builder) => builder.validate_freeze(),
            TransactionState::Frozen(registry) => return registry,
            TransactionState::Empty => panic!("registry transaction has no owner"),
        }
        let state = std::mem::replace(&mut self.state, TransactionState::Empty);
        self.state = match state {
            TransactionState::Building(builder) => TransactionState::Frozen(builder.freeze_validated()),
            TransactionState::Frozen(_) | TransactionState::Empty => {
                unreachable!("validated building transaction was replaced")
            }
        };
        match &self.state {
            TransactionState::Frozen(registry) => registry,
            TransactionState::Building(_) | TransactionState::Empty => {
                unreachable!("freeze always leaves a frozen transaction")
            }
        }
    }

    /// Move the temporary frozen registry into an erased access. The
    /// transaction becomes empty until [`Self::adopt_frozen`] receives it back.
    pub fn take_frozen(&mut self) -> Registry<T> {
        match std::mem::replace(&mut self.state, TransactionState::Empty) {
            TransactionState::Frozen(registry) => registry,
            TransactionState::Building(builder) => {
                self.state = TransactionState::Building(builder);
                panic!("registry transaction must be frozen before take_frozen")
            }
            TransactionState::Empty => panic!("registry transaction has no owner"),
        }
    }

    /// Adopt a uniquely-owned frozen registry back as the authoritative
    /// mutable builder, preserving its `RegistryId` and every registered
    /// holder id. This is the commit half of a recursive decode transaction.
    pub fn adopt_frozen(&mut self, registry: Registry<T>) {
        assert!(
            matches!(self.state, TransactionState::Empty),
            "registry transaction already owns a phase"
        );
        self.state = TransactionState::Building(registry.into_builder());
    }

    /// Recover the transaction's owner after decoding, whether the caller
    /// adopted a temporary frozen registry or never froze the builder.
    pub fn into_builder(self) -> RegistryBuilder<T> {
        match self.state {
            TransactionState::Building(builder) => builder,
            TransactionState::Frozen(registry) => registry.into_builder(),
            TransactionState::Empty => panic!("registry transaction has no owner"),
        }
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

    /// Consume this builder into the sole owner of a recursive decode
    /// transaction. Unlike the removed split-phase staging API, this does not
    /// borrow or replace the caller's builder.
    pub fn into_transaction(self) -> RegistryBuilderTransaction<T> {
        RegistryBuilderTransaction {
            state: TransactionState::Building(self),
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
        }
    }

    /// Reconstitute a mutable builder after consuming a frozen registry. This
    /// is the ownership-safe identity handoff used by mutually recursive
    /// decoding: the temporary registry is consumed before its builder can be
    /// frozen again, so one `RegistryId` never names two live registries.
    pub(crate) fn from_frozen_parts(
        RegistryParts {
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
            intrusive,
            pending_unbound,
        }: RegistryParts<T>,
    ) -> Self {
        RegistryBuilder {
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
            intrusive,
            pending_unbound,
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

    /// Replace the value at an already-registered key without changing its id.
    ///
    /// This is the commit half of a staged recursive decode. The placeholder
    /// table has already reserved every key and insertion slot; replacing the
    /// stored `Arc` updates the identity index atomically while preserving the
    /// holder id and all key/registration metadata.
    pub fn replace_registered(&mut self, key: &ResourceKey<T>, value: Arc<T>) -> BuilderHolder {
        let id = *self
            .by_key
            .get(key)
            .unwrap_or_else(|| panic!("Cannot replace missing key '{}' in registry", key));
        let new_identity = Self::identity(&value);
        if let Some(&existing_id) = self.by_value.get(&new_identity)
            && existing_id != id
        {
            panic!("Adding duplicate value to registry");
        }
        let old = std::mem::replace(&mut self.values[id as usize], value);
        self.by_value.remove(&Self::identity(&old));
        self.by_value.insert(new_identity, id);
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
        self.validate_freeze();
        self.freeze_validated()
    }

    /// Validate the Java `MappedRegistry.freeze()` preconditions without
    /// consuming the builder. Transactions call this before moving the
    /// authoritative builder into the frozen phase, so a validation panic
    /// leaves intrusive and pending-unbound holder state recoverable.
    fn validate_freeze(&self) {
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
    }

    fn freeze_validated(self) -> Registry<T> {
        Registry::from_builder(
            RegistryParts {
                key: self.key,
                registry_id: self.registry_id,
                values: self.values,
                keys: self.keys,
                by_location: self.by_location,
                by_key: self.by_key,
                by_value: self.by_value,
                registration_infos: self.registration_infos,
                lifecycle: self.lifecycle,
                default_id: self.default_id,
                default_key: self.default_key,
                tags: self.tags,
                intrusive: self.intrusive,
                pending_unbound: self.pending_unbound,
            },
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
    fn transaction_transfers_identity_without_a_replacement_builder() {
        let mut transaction = RegistryBuilder::<TestElement>::new(&key()).into_transaction();
        register(transaction.builder_mut(), "existing", 7);
        let registry_id = transaction.registry_id();
        assert_eq!(transaction.freeze().registry_id(), registry_id);
        let staged = transaction.take_frozen();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            transaction.builder_mut();
        }))
        .is_err());
        transaction.adopt_frozen(staged);
        register(transaction.builder_mut(), "new", 8);
        let final_registry = transaction.into_builder().freeze();
        assert_eq!(final_registry.registry_id(), registry_id);
        assert_eq!(final_registry.size(), 2);
    }

    #[test]
    fn transaction_freeze_validation_failure_preserves_pending_holders() {
        let mut transaction = RegistryBuilder::<TestElement>::new(&key()).into_transaction();
        let pending = element_key("never_registered");
        transaction.builder_mut().get_or_create_holder(&pending);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            transaction.freeze();
        }));
        assert!(result.is_err());
        // The same pending holder remains owned by the transaction and can be
        // completed after the failed validation.
        register(transaction.builder_mut(), "never_registered", 7);
        let registry = transaction.into_builder().freeze();
        assert_eq!(registry.get_value(&pending), Some(&TestElement(7)));
    }

    #[test]
    fn transaction_freeze_validation_failure_preserves_intrusive_holders() {
        let mut transaction = RegistryBuilder::<TestElement>::new_with_intrusive(&key())
            .into_transaction();
        let value = Arc::new(TestElement(7));
        transaction
            .builder_mut()
            .create_intrusive_holder(Arc::clone(&value));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            transaction.freeze();
        }));
        assert!(result.is_err());
        transaction
            .builder_mut()
            .register(&element_key("intrusive"), value, RegistrationInfo::BUILT_IN);
        let registry = transaction.into_builder().freeze();
        assert_eq!(registry.get_value(&element_key("intrusive")), Some(&TestElement(7)));
    }

    #[test]
    fn transaction_registration_failure_does_not_corrupt_owner() {
        let mut transaction = RegistryBuilder::<TestElement>::new(&key()).into_transaction();
        register(transaction.builder_mut(), "existing", 7);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            register(transaction.builder_mut(), "existing", 8);
        }));
        assert!(result.is_err());
        let registry = transaction.into_builder().freeze();
        assert_eq!(registry.size(), 1);
        assert_eq!(registry.get_value(&element_key("existing")), Some(&TestElement(7)));
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
