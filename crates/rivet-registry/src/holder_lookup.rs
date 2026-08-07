//! `HolderOwner` / `HolderGetter` / `HolderLookup` / `RegistryLookup` /
//! `HolderLookup.Provider` — the #126 holder-view surface of `net.minecraft.core`.
//!
//! PROVENANCE: `HolderOwner.java` (3 lines), `HolderGetter.java` (48 lines),
//! `HolderLookup.java` (148 lines), all leaves of the `mc.core` manifest unit.
//!
//! Java's interfaces are implemented by `Registry<T>` (`RegistryLookup extends
//! HolderLookup + HolderOwner`) and `RegistryAccess` (`HolderLookup.Provider`).
//! Rust allows the impls to live in a different file than the type, so this
//! module adds the #126 surface **without editing `registry.rs`/`access.rs`**
//! (their #124 ownership): `impl RegistryLookup<T> for Registry<T>` and
//! `impl HolderLookupProvider for RegistryAccess` use only the SCC's public
//! methods.
//!
//! The two concrete views are the #126 replacement for the SCC's placeholder
//! `registry_ops::HolderOwner`/`HolderGetter` value structs (which this module
//! also owns the replacement of — `registry_ops.rs` re-exports them):
//! - `RegistryOwner` — the type-erased owner view (a `RegistryId`), used by the
//!   `RegistryOps.owner()` codec path for the O(1) `canSerializeIn` owner check.
//! - `RegistryGetter<E>` — the getter view over the owning `RegistryAccess`,
//!   used by `RegistryOps.getter()` and the `retrieve_getter` context codec. It
//!   resolves through the access's single sanctioned erased downcast
//!   (`RegistryAccess::lookup`).
//!
//! Binding-model deviations (documented, PORTING.md drift checklist):
//! - **Back-reference seam:** Java holders store their key/value/tags; the Rust
//!   `Holder::Reference` is a pure `(RegistryId, id)` pair, so `HolderLookup<T>`
//!   carries three Rust-specific methods (`value_of`/`key_of`/`tags_of`) that
//!   resolve a holder's data by id. `Holder::value()/key()/is()/tags()/unwrap()`
//!   take a `&dyn HolderLookup<T>` (OWNERSHIP's back-reference rule).
//! - `IdMap<Holder<T>>` (`Registry.asHolderIdMap()`) is **not** provided: its
//!   `byId` must return a reference to a stored holder object, but the Rust
//!   `Holder::Reference` is a value constructed on demand. The ByteBuf holder
//!   codec (protocol, #126) resolves through `&Registry<T>` instead.
//!   The SCC's `HolderIdMap` (`IdMap<HolderId>`) remains the id<->holder-id
//!   adapter.
//! - `HolderLookup.RegistryLookup.filterFeatures`/`filterElements` (FeatureFlags)
//!   are deferred with the world/feature flag units — not stubbed (no feature
//!   flag type exists to reference).

use crate::ResourceKey;
use crate::TagKey;
use crate::access::RegistryAccess;
use crate::holder::{Holder, RegistryId};
use crate::holder_set::HolderSet;
use crate::registry::{Registry, RegistryKey};
use crate::root::AnyBox;

use rivet_serialization::lifecycle::Lifecycle;

use std::marker::PhantomData;

/// `net.minecraft.core.HolderOwner<T>` — the owner a holder must serialize in.
///
/// Rust's owner identity is the per-instance `RegistryId` (OWNERSHIP.md:
/// holder serialization-owner checks compare `RegistryId`, Java's `context ==
/// this` pointer identity preserved).
pub trait HolderOwner<T> {
    /// The owning registry's per-instance `RegistryId`.
    fn registry_id(&self) -> RegistryId;

    /// `HolderOwner.canSerializeIn(context)` — `context == this`.
    fn can_serialize_in(&self, context: &dyn HolderOwner<T>) -> bool {
        context.registry_id() == self.registry_id()
    }
}

/// `net.minecraft.core.HolderGetter<T>` — resolve a holder by `ResourceKey` or a
/// named set by `TagKey`.
pub trait HolderGetter<T> {
    /// `HolderGetter.get(ResourceKey)` — `Optional<Holder.Reference<T>>`.
    fn get(&self, key: &ResourceKey<T>) -> Option<Holder<T>>;

    /// `HolderGetter.get(TagKey)` — `Optional<HolderSet.Named<T>>`.
    fn get_tag(&self, tag: &TagKey<T>) -> Option<HolderSet<T>>;

    /// `HolderGetter.getOrThrow(ResourceKey)` — `"Missing element <key>"`.
    fn get_or_throw(&self, key: &ResourceKey<T>) -> Holder<T> {
        self.get(key)
            .unwrap_or_else(|| panic!("Missing element {}", key))
    }

    /// `HolderGetter.getOrThrow(TagKey)` — `"Missing tag <tag>"`.
    fn get_tag_or_throw(&self, tag: &TagKey<T>) -> HolderSet<T> {
        self.get_tag(tag)
            .unwrap_or_else(|| panic!("Missing tag {}", tag))
    }
}

// `HolderGetter.getRandomElementOf(TagKey, RandomSource)` is **not** ported:
// it takes a concrete `&mut R: RandomSource` (Java's `RandomSource` interface),
// which would make the trait non-dyn-compatible. `HolderLookup<T>` must stay a
// trait object (`Holder::value`/`key` resolve through `&dyn HolderLookup<T>`),
// so the convenience default is omitted; callers compose
// `holder_set::get_random_element` directly.

/// `net.minecraft.core.HolderLookup<T>` — a `HolderGetter` that also lists its
/// elements and named tag sets.
///
/// The three back-reference methods (`value_of`/`key_of`/`tags_of`) are the Rust
/// value-model seam: `Holder::value()/key()/is()/tags()` resolve through a
/// `&dyn HolderLookup<T>` (OWNERSHIP's back-reference rule).
pub trait HolderLookup<T>: HolderGetter<T> {
    /// `HolderLookup.listElements()` — the registry-backed references.
    fn list_elements(&self) -> Vec<Holder<T>>
    where
        Self: Sized;

    /// `HolderLookup.listTags()` — the bound named sets.
    fn list_tags(&self) -> Vec<HolderSet<T>>
    where
        Self: Sized;

    /// `HolderLookup.listElementIds()`.
    fn list_element_ids(&self) -> Vec<ResourceKey<T>>
    where
        Self: Sized,
    {
        self.list_elements()
            .iter()
            .filter_map(|holder| holder.unwrap_key(self))
            .collect()
    }

    /// `HolderLookup.listTagIds()`.
    fn list_tag_ids(&self) -> Vec<TagKey<T>>
    where
        Self: Sized,
    {
        self.list_tags()
            .iter()
            .filter_map(HolderSet::unwrap_key)
            .collect()
    }

    /// Back-reference seam — resolve a holder's value by id (`Direct` returns
    /// the inline value).
    fn value_of<'a>(&'a self, holder: &'a Holder<T>) -> Option<&'a T>;

    /// Back-reference seam — resolve a holder's key by id (`Direct`: `None`).
    ///
    /// Owned (a clone): the Rust `Holder::Reference` stores no key, and the
    /// holder-view impls may not reach into `Registry`'s private key vec, so
    /// the key is resolved through the public by-id surface and owned. Java
    /// `Reference.key()` returns the holder's stored key reference; cloning is
    /// the value-model equivalent.
    fn key_of(&self, holder: &Holder<T>) -> Option<ResourceKey<T>>;

    /// Back-reference seam — resolve the named sets a holder belongs to.
    fn tags_of(&self, holder: &Holder<T>) -> Vec<TagKey<T>>;
}

/// `net.minecraft.core.HolderLookup.RegistryLookup<T>` — the per-registry
/// lookup view (`Registry<T>` implements it).
pub trait RegistryLookup<T>: HolderLookup<T> + HolderOwner<T> {
    /// `RegistryLookup.key()` — the registry's `ResourceKey`.
    fn key(&self) -> &RegistryKey<T>;

    /// `RegistryLookup.registryLifecycle()`.
    fn registry_lifecycle(&self) -> Lifecycle;
}

/// `net.minecraft.core.HolderLookup.Provider` — the heterogeneous lookup set
/// (`RegistryAccess` implements it).
pub trait HolderLookupProvider {
    /// `Provider.listRegistryKeys()`.
    fn list_registry_keys(&self) -> Vec<RegistryKey<()>>;

    /// `Provider.lookup(ResourceKey)` — the typed per-registry lookup.
    fn lookup<E: Send + Sync + 'static>(
        &self,
        key: &RegistryKey<E>,
    ) -> Option<&dyn RegistryLookup<E>>;

    /// `Provider.lookupOrThrow(ResourceKey)` — `"Registry <id> not found"`.
    fn lookup_or_throw<E: Send + Sync + 'static>(
        &self,
        key: &RegistryKey<E>,
    ) -> &dyn RegistryLookup<E> {
        self.lookup(key)
            .unwrap_or_else(|| panic!("Registry {} not found", key.identifier()))
    }
}

// ---------------------------------------------------------------------------
// Registry<T>: the per-registry lookup + owner
// ---------------------------------------------------------------------------

/// Strict by-id lookup for holder resolution — **no** `DefaultedRegistry`
/// fallback.
///
/// `Registry::by_id` (ownership B) inherits `DefaultedRegistry`'s asymmetric
/// fallback: an out-of-range id on a defaulted registry returns the *default*
/// element. A `Holder.Reference`'s id is only meaningful within the range the
/// registry actually holds — Java's `Reference` stores its own key/value and an
/// unresolvable reference is *unbound* (null), never the default element. So
/// holder resolution (`value_of`/`key_of`, and through them the codec encode
/// path) must treat an out-of-range id as absent, exactly like a non-defaulted
/// `by_id`. Built from the public `size()`/`by_id()` surface (ownership B stays
/// untouched). Lookup-constructed references always resolve; a hand-built
/// out-of-range id reports `None`, matching Java's unbound-reference state.
fn by_id_strict<T>(registry: &Registry<T>, id: u32) -> Option<&T> {
    let in_range = (id as usize) < registry.size() as usize;
    in_range.then(|| registry.by_id(id as i32)).flatten()
}

impl<T: Send + Sync + 'static> HolderOwner<T> for Registry<T> {
    fn registry_id(&self) -> RegistryId {
        Registry::registry_id(self)
    }
}

impl<T: Send + Sync + 'static> HolderGetter<T> for Registry<T> {
    fn get(&self, key: &ResourceKey<T>) -> Option<Holder<T>> {
        let value = self.get_value(key)?;
        let id = self.get_id(value) as u32;
        Some(Holder::reference(Registry::registry_id(self), id))
    }

    fn get_tag(&self, tag: &TagKey<T>) -> Option<HolderSet<T>> {
        let ids = Registry::get_tag(self, tag)?;
        Some(HolderSet::named_from_ids(
            Registry::registry_id(self),
            tag.clone(),
            ids,
        ))
    }
}

impl<T: Send + Sync + 'static> HolderLookup<T> for Registry<T> {
    fn list_elements(&self) -> Vec<Holder<T>> {
        (0..self.size())
            .map(|id| Holder::reference(Registry::registry_id(self), id as u32))
            .collect()
    }

    fn list_tags(&self) -> Vec<HolderSet<T>> {
        Registry::list_tags(self)
            .into_iter()
            .map(|tag| {
                let ids = Registry::get_tag(self, &tag).unwrap_or(&[]);
                HolderSet::named_from_ids(Registry::registry_id(self), tag, ids)
            })
            .collect()
    }

    fn value_of<'a>(&'a self, holder: &'a Holder<T>) -> Option<&'a T> {
        match holder {
            Holder::Direct(value) => Some(value),
            Holder::Reference { id, .. } => by_id_strict(self, *id),
        }
    }

    fn key_of(&self, holder: &Holder<T>) -> Option<ResourceKey<T>> {
        match holder {
            Holder::Direct(_) => None,
            Holder::Reference { id, .. } => {
                let value = by_id_strict(self, *id)?;
                Registry::get_resource_key(self, value)
            }
        }
    }

    fn tags_of(&self, holder: &Holder<T>) -> Vec<TagKey<T>> {
        let id = match holder {
            Holder::Reference { id, .. } => *id,
            Holder::Direct(_) => return Vec::new(),
        };
        Registry::list_tags(self)
            .into_iter()
            .filter(|tag| {
                Registry::get_tag(self, tag).is_some_and(|ids| ids.iter().any(|h| h.0 == id))
            })
            .collect()
    }
}

impl<T: Send + Sync + 'static> RegistryLookup<T> for Registry<T> {
    fn key(&self) -> &RegistryKey<T> {
        Registry::key(self)
    }

    fn registry_lifecycle(&self) -> Lifecycle {
        Registry::registry_lifecycle(self)
    }
}

// ---------------------------------------------------------------------------
// RegistryAccess: the HolderLookup.Provider
// ---------------------------------------------------------------------------

impl HolderLookupProvider for RegistryAccess {
    fn list_registry_keys(&self) -> Vec<RegistryKey<()>> {
        RegistryAccess::list_registry_keys(self)
    }

    fn lookup<E: Send + Sync + 'static>(
        &self,
        key: &RegistryKey<E>,
    ) -> Option<&dyn RegistryLookup<E>> {
        RegistryAccess::lookup(self, key).map(|registry| registry as &dyn RegistryLookup<E>)
    }
}

impl RegistryAccess {
    /// `HolderLookup.Provider.createSerializationContext(DynamicOps)` — the
    /// `RegistryOps` over this access (the #126 widening of the provider view;
    /// `registry_ops::RegistryOps::create_from_access` is the construction
    /// path).
    pub fn create_serialization_context<
        Ops: rivet_serialization::dynamic_ops::DynamicOps + Clone,
    >(
        &self,
        parent: &Ops,
    ) -> crate::registry_ops::RegistryOps<Ops::Output, Ops> {
        crate::registry_ops::RegistryOps::create_from_access(parent, self.clone())
    }

    /// Build an access from a single frozen typed registry — the minimal
    /// construction path for the #126 protocol codecs in `rivet-protocol` (a
    /// `RegistryFriendlyByteBuf` resolves the codec's registry key through the
    /// access's sanctioned erased downcast) and their tests. `key` is erased to
    /// the stored `RegistryKey<()>` form (`lookup` re-erases a typed key the
    /// same way), so `access.lookup(&key)` resolves it back.
    ///
    /// `T: Send + Sync + 'static` is the erased-boundary requirement
    /// (`Registry<T>: AnyRegistry`), same as `lookup`.
    pub fn from_single_registry<T: Send + Sync + 'static>(
        key: RegistryKey<T>,
        registry: Registry<T>,
    ) -> Self {
        let erased = ResourceKey::create_registry_key(key.identifier().clone());
        RegistryAccess::from_pairs(vec![(erased, Box::new(registry) as AnyBox)])
    }
}

// ---------------------------------------------------------------------------
// Concrete codec views (the #126 replacement for registry_ops' placeholders)
// ---------------------------------------------------------------------------

/// The type-erased owner view — the value `RegistryOps.owner()` yields. Carries
/// the owning registry's `RegistryId` for the O(1) `canSerializeIn` check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryOwner {
    /// The owning registry's per-instance `RegistryId`.
    pub registry_id: RegistryId,
}

impl<T> HolderOwner<T> for RegistryOwner {
    fn registry_id(&self) -> RegistryId {
        self.registry_id
    }
}

/// The getter view — the value `RegistryOps.getter()` and the
/// `retrieve_getter` context codec yield. Resolves elements and named sets
/// through the owning `RegistryAccess`'s sanctioned erased downcast.
#[derive(Debug, Clone)]
pub struct RegistryGetter<E> {
    /// The owning `RegistryAccess`.
    pub(crate) access: RegistryAccess,
    /// The typed registry key, for the downcast.
    pub(crate) registry_key: RegistryKey<E>,
    _marker: PhantomData<fn() -> E>,
}

impl<E> RegistryGetter<E> {
    /// Build a getter over an access for a typed registry key (the
    /// `RegistryOps.getter()`/`retrieve_getter` construction path).
    pub(crate) fn new(access: RegistryAccess, registry_key: RegistryKey<E>) -> Self {
        RegistryGetter {
            access,
            registry_key,
            _marker: PhantomData,
        }
    }
}

impl<E: Send + Sync + 'static> RegistryGetter<E> {
    /// The typed frozen registry this getter resolves through.
    pub fn registry(&self) -> Option<&Registry<E>> {
        self.access.lookup(&self.registry_key)
    }
}

impl<E: Send + Sync + 'static> HolderGetter<E> for RegistryGetter<E> {
    fn get(&self, key: &ResourceKey<E>) -> Option<Holder<E>> {
        self.registry()?.get(key)
    }

    fn get_tag(&self, tag: &TagKey<E>) -> Option<HolderSet<E>> {
        let registry = self.registry()?;
        let ids = Registry::get_tag(registry, tag)?;
        Some(HolderSet::named_from_ids(
            Registry::registry_id(registry),
            tag.clone(),
            ids,
        ))
    }
}

impl<E: Send + Sync + 'static> HolderLookup<E> for RegistryGetter<E> {
    fn list_elements(&self) -> Vec<Holder<E>> {
        self.registry()
            .map(|r| r.list_elements())
            .unwrap_or_default()
    }

    fn list_tags(&self) -> Vec<HolderSet<E>> {
        match self.registry() {
            Some(registry) => Registry::list_tags(registry)
                .into_iter()
                .map(|tag| {
                    let ids = Registry::get_tag(registry, &tag).unwrap_or(&[]);
                    HolderSet::named_from_ids(Registry::registry_id(registry), tag, ids)
                })
                .collect(),
            None => Vec::new(),
        }
    }

    fn value_of<'a>(&'a self, holder: &'a Holder<E>) -> Option<&'a E> {
        self.registry()?.value_of(holder)
    }

    fn key_of(&self, holder: &Holder<E>) -> Option<ResourceKey<E>> {
        self.registry()?.key_of(holder)
    }

    fn tags_of(&self, holder: &Holder<E>) -> Vec<TagKey<E>> {
        self.registry()
            .map(|r| r.tags_of(holder))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RegistryBuilder;
    use crate::holder::{Holder, RegistryId};
    use crate::registration_info::RegistrationInfo;
    use crate::root::AnyBox;
    use crate::{Identifier, ResourceKey, TagKey};

    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn element_key(id: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn tag_key(id: &str) -> TagKey<TestElement> {
        TagKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn registry_with_tagged() -> Registry<TestElement> {
        let mut builder = RegistryBuilder::new(&registry_key());
        let a = builder.register(
            &element_key("a"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        let b = builder.register(
            &element_key("b"),
            Arc::new(TestElement(2)),
            RegistrationInfo::BUILT_IN,
        );
        builder.bind_tags(vec![(tag_key("group"), vec![a, b])]);
        builder.freeze()
    }

    // -----------------------------------------------------------------------
    // HolderLookup on the frozen Registry (Java `Registry implements
    // RegistryLookup<T>`)
    // -----------------------------------------------------------------------

    #[test]
    fn registry_lookup_get_resolves_a_reference_holder_by_key() {
        let registry = registry_with_tagged();
        // `lookup.get(ResourceKey)` → `Optional<Holder.Reference<T>>`.
        let holder = registry.get(&element_key("a")).expect("registered");
        match holder {
            Holder::Reference {
                registry: owner,
                id,
            } => {
                assert_eq!(owner, registry.registry_id());
                assert_eq!(id, 0);
            }
            Holder::Direct(_) => panic!("get must return a Reference, not Direct"),
        }
        // A missing key is absent (Java `Optional.empty`).
        assert!(registry.get(&element_key("missing")).is_none());
    }

    #[test]
    fn registry_lookup_get_or_throw_panics_on_missing_key_with_java_message() {
        let registry = registry_with_tagged();
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.get_or_throw(&element_key("missing"))
        }));
        let msg = err.unwrap_err().downcast_ref::<String>().cloned().unwrap();
        assert_eq!(msg, format!("Missing element {}", element_key("missing")));
    }

    #[test]
    fn registry_lookup_value_and_key_resolve_through_the_lookup() {
        let registry = registry_with_tagged();
        let holder = registry.get(&element_key("b")).unwrap();
        // `Holder.value()` resolves through the owning lookup (back-reference).
        assert_eq!(holder.value(&registry), &TestElement(2));
        // `Holder.key()` resolves the registry key.
        assert_eq!(holder.key(&registry), element_key("b"));
        // `unwrap` gives Either.left(key) for a Reference.
        assert_eq!(
            holder.unwrap(&registry),
            rivet_serialization::either::Either::Left(element_key("b"))
        );
        // `getRegisteredName` = the identifier.
        assert_eq!(
            holder.get_registered_name(&registry),
            "minecraft:b".to_string()
        );
    }

    #[test]
    fn holder_resolution_does_not_inherit_the_defaulted_registry_fallback() {
        // Java `Reference` stores its own key/value: an unresolvable reference
        // is *unbound*, never the default element. `Registry::by_id` inherits
        // `DefaultedRegistry`'s asymmetric fallback (out-of-range → default), so
        // holder resolution (`value_of`/`key_of`, and through them the codec
        // encode path) must bypass it: an out-of-range reference id on a
        // defaulted registry resolves to `None`, not the default's value/key.
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
        let registry = builder.freeze();
        // The defaulted fallback is live for the raw by-id surface...
        assert_eq!(registry.by_id(99), Some(&TestElement(0)));
        // ...but holder resolution reports the out-of-range reference unbound.
        let reference: Holder<TestElement> = Holder::reference(registry.registry_id(), 99);
        assert_eq!(registry.value_of(&reference), None);
        assert_eq!(registry.key_of(&reference), None);
        // An in-range id still resolves normally.
        let in_range: Holder<TestElement> = Holder::reference(registry.registry_id(), 0);
        assert_eq!(registry.value_of(&in_range), Some(&TestElement(0)));
        assert_eq!(registry.key_of(&in_range), Some(element_key("air")));
    }

    #[test]
    fn registry_lookup_is_identifier_key_and_tag_checks() {
        let registry = registry_with_tagged();
        let holder = registry.get(&element_key("a")).unwrap();
        assert!(holder.is_identifier(&registry, &Identifier::with_default_namespace("a")));
        assert!(!holder.is_identifier(&registry, &Identifier::with_default_namespace("x")));
        assert!(holder.is_key(&registry, &element_key("a")));
        assert!(!holder.is_key(&registry, &element_key("b")));
        // The holder is a member of the bound "group" tag.
        assert!(holder.is_tag(&registry, &tag_key("group")));
        assert!(!holder.is_tag(&registry, &tag_key("other")));
        assert_eq!(holder.tags(&registry), vec![tag_key("group")]);
    }

    #[test]
    fn registry_lookup_get_tag_returns_a_bound_named_set() {
        let registry = registry_with_tagged();
        // Route through the `HolderGetter`/`HolderLookup` trait view (the
        // inherent `Registry::get_tag` returns the raw `&[HolderId]`; the holder
        // view returns a bound `HolderSet::Named`).
        let lookup = &registry as &dyn HolderLookup<TestElement>;
        let set = lookup.get_tag(&tag_key("group")).unwrap();
        assert!(set.is_bound());
        assert_eq!(set.size(), 2);
        assert_eq!(set.unwrap_key(), Some(tag_key("group")));
        // Members resolve by id (holder id == element id).
        let members: Vec<_> = set.iter().collect();
        assert!(members.contains(&&registry.get(&element_key("a")).unwrap()));
        // A missing tag is absent.
        assert!(lookup.get_tag(&tag_key("nope")).is_none());
    }

    #[test]
    fn registry_lookup_list_elements_and_tags_are_the_id_and_tag_spaces() {
        let registry = registry_with_tagged();
        // The list methods are on `HolderLookup` (`Self: Sized`, called on the
        // concrete `Registry`).
        let elements =
            <Registry<TestElement> as HolderLookup<TestElement>>::list_elements(&registry);
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0], Holder::reference(registry.registry_id(), 0));
        assert_eq!(elements[1], Holder::reference(registry.registry_id(), 1));
        let ids = <Registry<TestElement> as HolderLookup<TestElement>>::list_element_ids(&registry);
        assert_eq!(ids, vec![element_key("a"), element_key("b")]);
        let tags = <Registry<TestElement> as HolderLookup<TestElement>>::list_tags(&registry);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].unwrap_key(), Some(tag_key("group")));
        let tag_ids = <Registry<TestElement> as HolderLookup<TestElement>>::list_tag_ids(&registry);
        assert_eq!(tag_ids, vec![tag_key("group")]);
    }

    // -----------------------------------------------------------------------
    // RegistryOwner / RegistryGetter (the #126 codec views)
    // -----------------------------------------------------------------------

    #[test]
    fn registry_owner_serializes_in_by_registry_id() {
        let registry = registry_with_tagged();
        let owner = RegistryOwner {
            registry_id: registry.registry_id(),
        };
        // Java `context == this`: the same RegistryId serializes in.
        let holder: Holder<TestElement> = Holder::reference(registry.registry_id(), 0);
        assert!(holder.can_serialize_in(&owner));
        // A Direct holder serializes anywhere.
        assert!(Holder::direct(TestElement(9)).can_serialize_in(&owner));
        // A different owner (different RegistryId) rejects the reference.
        let other = RegistryOwner {
            registry_id: RegistryId(999),
        };
        assert!(!holder.can_serialize_in(&other));
    }

    #[test]
    fn registry_getter_resolves_through_the_owning_access() {
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("test")),
            Box::new(registry_with_tagged()) as AnyBox,
        )]);
        let getter = RegistryGetter::new(access, registry_key());
        // The getter resolves the frozen registry via the sanctioned downcast.
        let registry = getter.registry().expect("frozen registry");
        assert_eq!(registry.key(), &registry_key());
        let holder = getter.get(&element_key("a")).unwrap();
        assert!(matches!(holder, Holder::Reference { id: 0, .. }));
        // `getOrThrow` on a missing key panics with Java's message.
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            getter.get_or_throw(&element_key("zzz"))
        }));
        let msg = err.unwrap_err().downcast_ref::<String>().cloned().unwrap();
        assert_eq!(msg, format!("Missing element {}", element_key("zzz")));
    }
}
