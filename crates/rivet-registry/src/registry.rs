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

use crate::holder::{Holder, HolderId, RegistryId};
use crate::id_map::{DEFAULT_ID, IdMap};
use crate::identifier::Identifier;
use crate::registration_info::RegistrationInfo;
use crate::resource_key::ResourceKey;
use crate::tag_key::TagKey;

use rivet_serialization::data_result::DataResult;
use rivet_serialization::functions::{DecoderFn, Fn1};
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
/// live here (#394 — a registry-owned prerequisite for the #126 by-name codec
/// surface; `FeatureSize.CODEC` builds on `byNameCodec`).
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

    /// `Registry.byId(int)` with the stored `Arc<T>` handle, not the deref.
    ///
    /// Java's `byId` hands out the stored element object, and the `Arc` is that
    /// stored element (the allocation `by_value` keys on). The registry element
    /// codec decodes through this so a decoded element re-encodes to its own id
    /// (`getId` is identity-sensitive); the plain `&T` deref cannot. Same
    /// `DefaultedRegistry` fallback as `by_id`.
    pub fn by_id_arc(&self, id: i32) -> Option<&Arc<T>> {
        self.by_id_inner(id)
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

    /// `Registry.byNameCodec()` — a `Codec<Arc<T>>` whose decode resolves a
    /// serialized namespaced identifier against **this** registry (by-location
    /// lookup, `MappedRegistry.get(Identifier)`), and whose encode emits the
    /// identifier of the registered element.
    ///
    /// This is the #394 slice: `FeatureSize.CODEC` (FeatureSize.java:10) is
    /// `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec().dispatch(FeatureSize::type,
    /// FeatureSizeType::codec)`.
    ///
    /// Java composition (Registry.java:29-31):
    /// ```java
    /// default Codec<T> byNameCodec() {
    ///     return this.referenceHolderWithLifecycle().flatComapMap(
    ///         Holder.Reference::value, value -> this.safeCastToReference(this.wrapAsHolder((T)value)));
    /// }
    /// ```
    /// where `referenceHolderWithLifecycle()` (lines 37-46) is
    /// `ExtraCodecs.overrideLifecycle(Identifier.CODEC.comapFlatMap(name ->
    /// this.get(name) ..., holder -> holder.key().identifier()), e ->
    /// registrationInfo(e.key()).map(...).orElse(experimental()))`.
    ///
    /// The `Arc<T>` value form is the Rust stand-in for Java's *stored object
    /// reference* (identical to `ByteBufCodecs.registry` in `rivet-protocol`):
    /// decode returns a clone of the stored `Arc` (same allocation), so encode
    /// resolves the identity-keyed lookup off that allocation for a real round
    /// trip. Java needs no `T` bound (the registry holds the shared object);
    /// the port needs `T: Debug` only for the unregistered-value error message.
    ///
    /// Behavior mirrors Java exactly:
    /// - Decode of an unknown name errors `"Unknown registry key in <key>:
    ///   <name>"` — the lookup is `get_optional` (`by_location`), which a
    ///   `DefaultedRegistry` does **not** fall back on (Java
    ///   `DefaultedMappedRegistry` overrides `getValue`/`getOptional` but not
    ///   `get(Identifier)`).
    /// - Decode lifecycle: a registered element carries its
    ///   `RegistrationInfo.lifecycle`; an element with no registration info
    ///   falls back to `Lifecycle::experimental()` (the override applies on a
    ///   full success only). Errors from the identifier codec keep its stable
    ///   lifecycle (a malformed identifier); an unknown-name error is
    ///   experimental, since `DataResult::error` defaults to experimental and
    ///   the lifecycle monoid gives experimental precedence.
    /// - Encode of an unregistered value errors `"Unregistered holder in <key>:
    ///   Direct{...}"` (`safeCastToReference` on a `wrapAsHolder` that produced
    ///   a direct holder — identity lookup, so a defaulted registry does not
    ///   mask it).
    /// - Encode lifecycle is the same registration-info getter (the override's
    ///   `co_apply` always applies).
    pub fn by_name_codec<Ops>(&self) -> Arc<dyn rivet_serialization::codec::Codec<Arc<T>, Ops>>
    where
        T: std::fmt::Debug + Send + Sync + 'static,
        Ops: rivet_serialization::dynamic_ops::DynamicOps + 'static,
    {
        let holder_codec = self.reference_holder_with_lifecycle::<Ops>();
        // Owned snapshots: the frozen registry is immutable, so owned clones of
        // its private fields are observationally identical to capturing `&self`.
        let registry_id = self.registry_id;
        let key = self.key.clone();
        let values = self.values.clone();
        let by_value = self.by_value.clone();
        // `flatComapMap` decode mapper: `Holder.Reference::value` — resolve the
        // reference's element by id. The reference codec decodes only
        // `Reference`s; a `Direct` reaching here is a Java `ClassCastException`
        // (the method reference binds `Reference`), so panicking is faithful.
        let to: Fn1<Holder<T>, Arc<T>> = Arc::new(move |holder: &Holder<T>| match holder {
            Holder::Direct(_) => panic!("byNameCodec decode produced a Direct holder"),
            Holder::Reference { id, .. } => values
                .get(*id as usize)
                .unwrap_or_else(|| panic!("Reference holder has no value: {}", holder))
                .clone(),
        });
        // `flatComapMap` encode mapper: `value -> safeCastToReference(wrapAsHolder
        // (value))` — identity lookup; a missing element is a Direct holder ->
        // the exact Java error.
        let from: DecoderFn<Arc<T>, Holder<T>> = Arc::new(move |value: &Arc<T>| {
            let id = by_value
                .get(&(value.as_ref() as *const T as usize))
                .copied();
            match id {
                Some(id) => DataResult::success(Holder::reference(registry_id, id)),
                None => DataResult::error(format!(
                    "Unregistered holder in {}: Direct{{{:?}}}",
                    key, value
                )),
            }
        });
        rivet_serialization::codec::flat_comap_map(holder_codec, to, from)
    }

    /// `Registry.holderByNameCodec()` — the `Codec<Holder<T>>` twin of
    /// `by_name_codec` (Registry.java:33-35):
    /// ```java
    /// return this.referenceHolderWithLifecycle().flatComapMap(holder ->
    /// (Holder<T>)holder, this::safeCastToReference);
    /// ```
    ///
    /// Decode produces `Holder::Reference` holders for registered names (and
    /// errors on unknown ones with the same `"Unknown registry key in <key>:
    /// <name>"`); encode accepts a same-registry `Reference` (emitting its
    /// identifier) and errors on a `Direct` (`"Unregistered holder in <key>:
    /// Direct{...}"`) or a `Reference` of another registry (which has no key
    /// this registry can emit).
    pub fn holder_by_name_codec<Ops>(
        &self,
    ) -> Arc<dyn rivet_serialization::codec::Codec<Holder<T>, Ops>>
    where
        T: std::fmt::Debug + Send + Sync + 'static,
        Ops: rivet_serialization::dynamic_ops::DynamicOps + 'static,
    {
        let reference_codec = self.reference_holder_with_lifecycle::<Ops>();
        let key = self.key.clone();
        let registry_id = self.registry_id;
        // `flatComapMap` decode mapper: `holder -> (Holder<T>)holder` — a
        // no-op on the decoded reference holder (Copy `(RegistryId, u32)` pair,
        // reconstructed without touching `T`); a `Direct` is unreachable here
        // (the decode only ever produces `Reference`s).
        let to: Fn1<Holder<T>, Holder<T>> = Arc::new(|h: &Holder<T>| match h {
            Holder::Reference { registry, id } => Holder::reference(*registry, *id),
            Holder::Direct(_) => panic!("holderByNameCodec decode produced a Direct holder"),
        });
        // `flatComapMap` encode mapper: `safeCastToReference` — a same-registry
        // Reference passes through and a Direct is the exact Java error. A
        // Reference of ANOTHER registry is rejected here too: the port's
        // `Reference` stores only `(RegistryId, id)` (no stored key), so this
        // registry cannot emit the foreign key that Java's
        // `Holder.Reference.key()` carries — the `can_serialize_in` owner rule
        // applies instead of silently emitting this registry's key (or
        // panicking on an out-of-range foreign id).
        let from: DecoderFn<Holder<T>, Holder<T>> =
            Arc::new(move |holder: &Holder<T>| match holder {
                Holder::Reference { registry, id } if *registry == registry_id => {
                    DataResult::success(Holder::reference(*registry, *id))
                }
                Holder::Direct(value) => DataResult::error(format!(
                    "Unregistered holder in {}: Direct{{{:?}}}",
                    key, value
                )),
                Holder::Reference { registry, id } => DataResult::error(format!(
                    "Unregistered holder in {}: foreign Reference from registry {} with id {}",
                    key, registry.0, id
                )),
            });
        rivet_serialization::codec::flat_comap_map(reference_codec, to, from)
    }

    /// `Registry.referenceHolderWithLifecycle()` — the private helper behind
    /// both by-name codecs (Registry.java:37-46):
    /// ```java
    /// Codec<Holder.Reference<T>> referenceCodec = Identifier.CODEC.comapFlatMap(
    ///     name -> this.get(name).map(DataResult::success).orElseGet(() ->
    ///         DataResult.error(() -> "Unknown registry key in " + this.key() + ": " + name)),
    ///     holder -> holder.key().identifier());
    /// return ExtraCodecs.overrideLifecycle(referenceCodec, e ->
    ///     this.registrationInfo(e.key()).map(RegistrationInfo::lifecycle).orElse(Lifecycle.experimental()));
    /// ```
    ///
    /// The decode lookup is `MappedRegistry.get(Identifier)` = `by_location`
    /// (`get_optional`), which a `DefaultedRegistry` does **not** override.
    ///
    /// The `Ops` codec is `identifier_codec` (`Identifier.CODEC` — `.stable()`),
    /// so a malformed identifier error propagates verbatim (e.g.
    /// `"Not a valid resource location: ..."`), and an unknown name is the
    /// exact Java message.
    ///
    /// Private like Java's helper: both public codecs feed it only
    /// same-registry references (identity lookup / owner-checked), so the
    /// `Direct`/out-of-range panic paths in its encode mapper are unreachable.
    fn reference_holder_with_lifecycle<Ops>(
        &self,
    ) -> Arc<dyn rivet_serialization::codec::Codec<Holder<T>, Ops>>
    where
        T: std::fmt::Debug + Send + Sync + 'static,
        Ops: rivet_serialization::dynamic_ops::DynamicOps + 'static,
    {
        // Owned snapshots of the frozen registry's private fields.
        let key = self.key.clone();
        let registry_id = self.registry_id;
        let by_location = self.by_location.clone();
        let keys = self.keys.clone();
        let registration_infos = self.registration_infos.clone();
        let identifier_codec = crate::identifier::identifier_codec::<Ops>();
        // `name -> this.get(name)` (decode) and `holder -> holder.key().identifier()` (encode).
        let reference_codec = rivet_serialization::codec::comap_flat_map(
            identifier_codec,
            Arc::new(
                move |identifier: &Identifier| match by_location.get(identifier).copied() {
                    Some(id) => DataResult::success(Holder::reference(registry_id, id)),
                    None => DataResult::error(format!(
                        "Unknown registry key in {}: {}",
                        key, identifier
                    )),
                },
            ),
            {
                let keys = keys.clone();
                Arc::new(move |holder: &Holder<T>| {
                    let id = match holder {
                        Holder::Reference { id, .. } => *id,
                        Holder::Direct(_) => panic!("Direct holder has no key"),
                    };
                    // Strict id -> key (no DefaultedRegistry fallback), the same
                    // rule as holder resolution (`holder_lookup::by_id_strict`);
                    // a lookup-constructed reference always resolves.
                    keys.get(id as usize)
                        .unwrap_or_else(|| panic!("Reference holder has no key: {}", holder))
                        .identifier()
                        .clone()
                })
            },
        );
        // `ExtraCodecs.overrideLifecycle(...)` — decode lifecycle overridden on
        // a full success only (an error/partial passes through untouched),
        // encode lifecycle always overridden. Registered elements carry their
        // registration lifecycle; anything unresolvable is experimental.
        rivet_serialization::extra_codecs::override_lifecycle_single(
            reference_codec,
            Arc::new(move |holder: &Holder<T>| {
                let id = match holder {
                    Holder::Reference { id, .. } => *id,
                    Holder::Direct(_) => return Lifecycle::experimental(),
                };
                keys.get(id as usize)
                    .and_then(|element_key| registration_infos.get(element_key))
                    .map(|info| info.lifecycle())
                    .unwrap_or(Lifecycle::experimental())
            }),
        )
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
    fn by_id_arc_returns_the_stored_allocation_that_resolves_by_identity() {
        // `by_id_arc` hands out the stored `Arc<T>` — the exact allocation the
        // identity-keyed `by_value`/`getId` map keys on, the element-codec round
        // trip's guarantee (decode -> re-encode must resolve the same id).
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        for id in 0..2 {
            let arc = registry.by_id_arc(id).expect("in-range");
            assert_eq!(registry.get_id(arc.as_ref()), id);
        }
        assert_eq!(registry.by_id_arc(-1), None);
        assert_eq!(registry.by_id_arc(2), None);
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

/// `Registry.byNameCodec()`/`holder_by_name_codec()`/`reference_holder_with_lifecycle()`
/// — the #394 by-name codec surface. These codecs resolve identifiers against
/// the frozen registry they were built from (owned snapshots of its private
/// tables), so no `RegistryOps`/`RegistryGetter` context is needed — unlike the
/// #126 holder codecs, which resolve through the ops' provider.
///
/// Paper grounding: `FeatureSize.CODEC` (FeatureSize.java:10) is
/// `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec().dispatch(...)`; the
/// decode lookup is `MappedRegistry.get(Identifier)` (by-location, strict), the
/// encode is `safeCastToReference(wrapAsHolder(value))` (identity), and the
/// lifecycle is `ExtraCodecs.overrideLifecycle` over `registrationInfo`.
#[cfg(test)]
mod by_name_codec_tests {
    use super::*;
    use crate::builder::RegistryBuilder;
    use rivet_serialization::codec::Codec;
    use rivet_serialization::decoder::Decoder;
    use rivet_serialization::dynamic_ops::DynamicOps;
    use rivet_serialization::encoder::Encoder;
    use rivet_serialization::json_ops::JsonOps;

    type TestOps = JsonOps;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn element_key(id: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn ops() -> TestOps {
        JsonOps::INSTANCE
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

    /// A defaulted registry whose default is a registered element.
    fn defaulted_registry() -> Registry<TestElement> {
        let mut builder = RegistryBuilder::new_defaulted(
            &Identifier::with_default_namespace("air"),
            &registry_key(),
        );
        builder.register(
            &element_key("air"),
            Arc::new(TestElement(0)),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &element_key("stone"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        builder.freeze()
    }

    fn encode_value<E, Ops: DynamicOps + 'static>(
        codec: &dyn Codec<E, Ops>,
        value: &E,
        ops: &Ops,
    ) -> rivet_serialization::data_result::DataResult<Ops::Output> {
        Encoder::encode(codec, value, ops, &ops.empty())
    }

    fn decode_value<E, Ops: DynamicOps + 'static>(
        codec: &dyn Codec<E, Ops>,
        ops: &Ops,
        input: &Ops::Output,
    ) -> rivet_serialization::data_result::DataResult<(E, Ops::Output)> {
        Decoder::decode(codec, ops, input)
    }

    // -----------------------------------------------------------------------
    // byNameCodec — decode
    // -----------------------------------------------------------------------

    #[test]
    fn decode_resolves_a_registered_identifier() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:stone".to_string());
        let decoded = decode_value(codec.as_ref(), &ops(), &input)
            .get_or_throw("decode")
            .0
            .clone();
        // The stored `Arc` allocation — the same object identity the registry
        // encodes by.
        let expected = registry.by_id_arc(1).cloned().unwrap();
        assert!(Arc::ptr_eq(&decoded, &expected));
        assert_eq!(decoded.as_ref(), &TestElement(1));
    }

    #[test]
    fn decode_unknown_name_errors_with_java_message() {
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:nope".to_string());
        let result = decode_value(codec.as_ref(), &ops(), &input);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Unknown registry key in {}: minecraft:nope", registry_key())
        );
    }

    #[test]
    fn decode_defaulted_registry_does_not_fall_back_for_unknown_names() {
        // Java `DefaultedMappedRegistry` overrides `getValue`/`getOptional` but
        // NOT `get(Identifier)` — `referenceHolderWithLifecycle` decodes via
        // `this.get(name)` (by-location), so an unknown name on a defaulted
        // registry is a strict error, never the default element.
        let registry = defaulted_registry();
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:nope".to_string());
        let result = decode_value(codec.as_ref(), &ops(), &input);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Unknown registry key in {}: minecraft:nope", registry_key())
        );
    }

    #[test]
    fn decode_lifecycle_is_the_registration_info() {
        // BUILT_IN registration info -> Lifecycle::stable() (Java
        // `RegistrationInfo.BUILT_IN.lifecycle()` = `Lifecycle.stable()`).
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:air".to_string());
        let decoded = decode_value(codec.as_ref(), &ops(), &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Stable);
    }

    #[test]
    fn decode_error_lifecycle_distinguishes_unknown_name_from_malformed() {
        // Decode error lifecycles follow the Java composition:
        // - Unknown-name: produced by the outer comapFlatMap's mapper via
        //   `DataResult.error(...)` (experimental), and `Success.flatMap`
        //   re-adds the identifier codec's stable — experimental wins the
        //   monoid. `overrideLifecycle` overrides only a full success, so the
        //   error stays experimental.
        // - Malformed identifier: the error originates inside the identifier
        //   codec, whose `.stable()` wrapper sets the whole result to stable.
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.by_name_codec::<TestOps>();
        let unknown = ops().create_string("minecraft:nope".to_string());
        let unknown_result = decode_value(codec.as_ref(), &ops(), &unknown);
        assert!(unknown_result.is_error());
        assert_eq!(
            unknown_result.error_ref().unwrap().lifecycle(),
            Lifecycle::Experimental
        );
        let malformed = ops().create_string("a b:c".to_string());
        let malformed_result = decode_value(codec.as_ref(), &ops(), &malformed);
        assert!(malformed_result.is_error());
        assert_eq!(
            malformed_result.error_ref().unwrap().lifecycle(),
            Lifecycle::Stable
        );
    }

    #[test]
    fn decode_malformed_identifier_propagates_the_identifier_error() {
        // The identifier codec's error passes through verbatim (Java
        // `Identifier.CODEC.decode(...)` inside the comapFlatMap).
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.by_name_codec::<TestOps>();
        // A non-string.
        let input = ops().create_int(42);
        let result = decode_value(codec.as_ref(), &ops(), &input);
        assert!(result.is_error());
        assert_eq!(result.error_ref().unwrap().message(), "Not a string: 42");
        // An invalid resource location.
        let input = ops().create_string("a b:c".to_string());
        let result = decode_value(codec.as_ref(), &ops(), &input);
        assert!(result.is_error());
        assert!(
            result
                .error_ref()
                .unwrap()
                .message()
                .contains("Not a valid resource location"),
            "unexpected message: {}",
            result.error_ref().unwrap().message()
        );
    }

    // -----------------------------------------------------------------------
    // byNameCodec — encode
    // -----------------------------------------------------------------------

    #[test]
    fn encode_registered_value_emits_its_identifier() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let codec = registry.by_name_codec::<TestOps>();
        let value = registry.by_id_arc(1).cloned().unwrap();
        let encoded = encode_value(codec.as_ref(), &value, &ops())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops().create_string("minecraft:stone".to_string()));
    }

    #[test]
    fn encode_unregistered_value_errors_with_java_message() {
        // `wrapAsHolder` is an identity lookup: a fresh `Arc` is not the stored
        // allocation, so it becomes a Direct holder -> the exact Java error
        // (even on a defaulted registry — `DefaultedMappedRegistry.getId` is
        // NOT what `wrapAsHolder` uses).
        let registry = defaulted_registry();
        let codec = registry.by_name_codec::<TestOps>();
        let unregistered = Arc::new(TestElement(99));
        let result = encode_value(codec.as_ref(), &unregistered, &ops());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Unregistered holder in {}: Direct{{TestElement(99)}}",
                registry_key()
            )
        );
    }

    #[test]
    fn encode_lifecycle_is_experimental() {
        // Java `byNameCodec` encode lifecycle: the outer `flatComapMap` mapper
        // `safeCastToReference(wrapAsHolder(value))` returns
        // `DataResult.success(reference)` (experimental by default), and
        // `Success.flatMap` re-adds that outer lifecycle over the inner
        // override's `co_apply` Stable — experimental wins the monoid. So even
        // for a BUILT_IN element the *encode* result is experimental (decode,
        // by contrast, is Stable — the override's `apply` replaces, not adds).
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.by_name_codec::<TestOps>();
        let value = registry.by_id_arc(0).cloned().unwrap();
        let encoded = encode_value(codec.as_ref(), &value, &ops());
        assert!(encoded.is_success());
        assert_eq!(encoded.lifecycle(), Lifecycle::Experimental);
    }

    #[test]
    fn decode_lifecycle_reflects_experimental_registration_info() {
        // A registry whose element has an experimental registration info decodes
        // to experimental (the override's `apply` replaces the decode lifecycle
        // with the element's registration lifecycle).
        let mut builder = RegistryBuilder::new(&registry_key());
        let key = element_key("one");
        builder.register(
            &key,
            Arc::new(TestElement(1)),
            RegistrationInfo::new(None, Lifecycle::Experimental),
        );
        let registry = builder.freeze();
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:one".to_string());
        let decoded = decode_value(codec.as_ref(), &ops(), &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Experimental);
    }

    #[test]
    fn encode_decode_round_trips_through_the_same_allocation() {
        // The codec decodes to the stored `Arc` (identity), so re-encoding the
        // decode result reproduces the original identifier.
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let codec = registry.by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:stone".to_string());
        let decoded = decode_value(codec.as_ref(), &ops(), &input)
            .get_or_throw("decode")
            .0
            .clone();
        let reencoded = encode_value(codec.as_ref(), &decoded, &ops())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            reencoded,
            ops().create_string("minecraft:stone".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // holderByNameCodec
    // -----------------------------------------------------------------------

    #[test]
    fn holder_codec_decodes_to_a_reference_holder() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let codec = registry.holder_by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:stone".to_string());
        let decoded = decode_value(codec.as_ref(), &ops(), &input)
            .get_or_throw("decode")
            .0
            .clone();
        assert_eq!(decoded, Holder::reference(registry.registry_id(), 1));
    }

    #[test]
    fn holder_codec_encodes_a_reference_holder_as_its_identifier() {
        let registry = registry_of(&[("air", 0), ("stone", 1)]);
        let codec = registry.holder_by_name_codec::<TestOps>();
        let holder = Holder::reference(registry.registry_id(), 1);
        let encoded = encode_value(codec.as_ref(), &holder, &ops())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops().create_string("minecraft:stone".to_string()));
    }

    #[test]
    fn holder_codec_encodes_a_direct_holder_as_unregistered() {
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.holder_by_name_codec::<TestOps>();
        let holder = Holder::direct(TestElement(7));
        let result = encode_value(codec.as_ref(), &holder, &ops());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Unregistered holder in {}: Direct{{TestElement(7)}}",
                registry_key()
            )
        );
    }

    #[test]
    fn holder_codec_encodes_a_foreign_reference_as_unregistered() {
        // A `Reference` bound to ANOTHER registry has no key this registry can
        // emit (the port's `Reference` stores only `(RegistryId, id)`), so it is
        // rejected here — Java would throw `IllegalStateException` from
        // `Holder.Reference.key()` on a foreign `can_serialize_in` owner. The
        // message names the foreign registry rather than this one's key.
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.holder_by_name_codec::<TestOps>();
        let foreign = Holder::reference(RegistryId(999), 0);
        let result = encode_value(codec.as_ref(), &foreign, &ops());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Unregistered holder in {}: foreign Reference from registry 999 with id 0",
                registry_key()
            )
        );
    }

    #[test]
    fn holder_codec_unknown_name_errors_with_java_message() {
        let registry = registry_of(&[("air", 0)]);
        let codec = registry.holder_by_name_codec::<TestOps>();
        let input = ops().create_string("minecraft:nope".to_string());
        let result = decode_value(codec.as_ref(), &ops(), &input);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Unknown registry key in {}: minecraft:nope", registry_key())
        );
    }

    // -----------------------------------------------------------------------
    // referenceHolderWithLifecycle — registration order is the id space
    // -----------------------------------------------------------------------

    #[test]
    fn decode_registration_order_is_the_id_space() {
        // Entries are decoded to `Reference{registry, id}` where id == insertion
        // order (element id == holder id == network id == insertion index).
        let registry = registry_of(&[("first", 10), ("second", 20)]);
        let codec = registry.by_name_codec::<TestOps>();
        let first = decode_value(
            codec.as_ref(),
            &ops(),
            &ops().create_string("minecraft:first".to_string()),
        )
        .get_or_throw("decode")
        .0
        .clone();
        let second = decode_value(
            codec.as_ref(),
            &ops(),
            &ops().create_string("minecraft:second".to_string()),
        )
        .get_or_throw("decode")
        .0
        .clone();
        assert!(Arc::ptr_eq(
            &first,
            &registry.by_id_arc(0).cloned().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &second,
            &registry.by_id_arc(1).cloned().unwrap()
        ));
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
