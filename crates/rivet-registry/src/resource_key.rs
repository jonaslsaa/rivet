//! Port of `net.minecraft.resources.ResourceKey<T>` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.resources` manifest unit. Java source:
//! `net/minecraft/resources/ResourceKey.java` (78 lines, 26.2).
//!
//! Binding model (OWNERSHIP.md §Registries, #107):
//! - `ResourceKey<T>` is a **value type**. Java's weak interning
//!   (`VALUES.computeIfAbsent`) makes `==` value equality; Rust models it with
//!   derived value semantics over the two `Identifier` fields. No interning, no
//!   pointer comparison, no interning map.
//! - `Eq`/`Hash`/`PartialEq`/`Clone` are **hand-written with no `T` bound** —
//!   the `_marker: PhantomData<fn() -> T>` field is the *only* place `T`
//!   appears, so `derive(PartialEq)`/`derive(Clone)` would add a spurious
//!   `T: PartialEq`/`T: Clone` bound (rustc does not special-case
//!   `PhantomData<fn() -> T>` for these traits). The hand-written impls compare
//!   the two `Identifier` fields only.
//! - `registry()`/`identifier()`/`is_for`/`cast`/`dependent`/`registry_key`
//!   mirror the Java accessors.
//!
//! Codec boundary: `ResourceKey.codec(registryName)` / `streamCodec(registryName)`
//! both map over `Identifier`; the `StreamCodec` version is `rivet-protocol`
//! (#126), NOT here. `codec` (a `Codec<ResourceKey<T>>` given the registry key)
//! belongs here.

use crate::Identifier;
use crate::identifier::identifier_codec;
use crate::registry::Registry;

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;

use std::marker::PhantomData;
use std::sync::Arc;

/// `net.minecraft.resources.ResourceKey<T>`.
///
/// `registry_name` is the *registry's* key (`Registries.ROOT_REGISTRY_NAME` for
/// a registry key, or the element registry's key for an element key);
/// `identifier` is this key's own identifier. Both are `pub(crate)` so the
/// sibling ownership modules (`tag_key.rs`'s `cast` re-erasure, `registries.rs`
/// key construction) can build/read them.
///
/// `Eq`/`Hash`/`PartialEq` are **hand-written with no `T` bound** — the #107
/// binding (OWNERSHIP.md §Registries): "`PhantomData<fn() -> T>`, no `T` bound
/// on Eq/Hash". `derive(PartialEq)` would add a `T: PartialEq` bound (rustc
/// does not special-case `PhantomData` for `PartialEq`), which would forbid
/// keys over not-yet-`PartialEq` element types — exactly what the binding
/// forbids.
#[derive(Debug)]
pub struct ResourceKey<T> {
    /// The registry's key identifier (Java `ResourceKey.registryName`).
    pub(crate) registry_name: Identifier,
    /// This key's own identifier (Java `ResourceKey.identifier`).
    pub(crate) identifier: Identifier,
    /// Phantom marker — the only place `T` appears, so the hand-written
    /// `Eq`/`Hash`/`Clone` impls carry no `T` bound.
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for ResourceKey<T> {
    fn clone(&self) -> Self {
        ResourceKey {
            registry_name: self.registry_name.clone(),
            identifier: self.identifier.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> PartialEq for ResourceKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.registry_name == other.registry_name && self.identifier == other.identifier
    }
}

impl<T> Eq for ResourceKey<T> {}

impl<T> std::hash::Hash for ResourceKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.registry_name.hash(state);
        self.identifier.hash(state);
    }
}

/// Java's `toString()` — `"ResourceKey[" + registryName + " / " + identifier + "]"`.
impl<T> std::fmt::Display for ResourceKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResourceKey[{} / {}]",
            self.registry_name, self.identifier
        )
    }
}

impl<T> ResourceKey<T> {
    /// `ResourceKey.create(registryName, location)` — `create(registryName.identifier, location)`.
    pub fn create(
        registry_name: &ResourceKey<Registry<T>>,
        location: Identifier,
    ) -> ResourceKey<T> {
        ResourceKey {
            registry_name: registry_name.identifier().clone(),
            identifier: location,
            _marker: PhantomData,
        }
    }

    /// `ResourceKey.createRegistryKey(identifier)` — `create(Registries.ROOT_REGISTRY_NAME, identifier)`,
    /// the ROOT_REGISTRY_NAME-rooted registry key.
    pub fn create_registry_key(identifier: Identifier) -> ResourceKey<Registry<T>> {
        ResourceKey {
            registry_name: (*crate::registries::ROOT_REGISTRY_NAME).clone(),
            identifier,
            _marker: PhantomData,
        }
    }

    /// `ResourceKey.identifier()`.
    pub fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    /// `ResourceKey.registry()` — the registry *name* identifier.
    pub fn registry(&self) -> &Identifier {
        &self.registry_name
    }

    /// `ResourceKey.isFor(ResourceKey<? extends Registry<?>>)`.
    ///
    /// Java compares `this.registryName.equals(registry.identifier())` — the
    /// stored registry name against the passed key's identifier. Generic over
    /// the registry element type (`? extends Registry<?>`).
    pub fn is_for<E>(&self, registry: &ResourceKey<Registry<E>>) -> bool {
        self.registry_name == *registry.identifier()
    }

    /// `ResourceKey.cast(ResourceKey<? extends Registry<E>>)` — `Optional`.
    ///
    /// Java's `cast` delegates to `isFor(registry)` — which compares
    /// `this.registryName.equals(registry.identifier())` only — and then
    /// re-interprets the value under the new type. Rust re-constructs the
    /// value with the same two `Identifier` fields and a new phantom marker
    /// (no `unsafe`, no `Any`).
    pub fn cast<E>(&self, registry: &ResourceKey<Registry<E>>) -> Option<ResourceKey<E>> {
        if self.is_for(registry) {
            Some(ResourceKey {
                registry_name: self.registry_name.clone(),
                identifier: self.identifier.clone(),
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    /// `ResourceKey.dependent(registryKey, suffix)`.
    pub fn dependent<E>(
        &self,
        registry_key: &ResourceKey<Registry<E>>,
        suffix: &str,
    ) -> ResourceKey<E> {
        ResourceKey::create(registry_key, self.identifier.with_suffix(suffix))
    }

    /// `ResourceKey.dependent(registryKey, UnaryOperator<String>)`.
    pub fn dependent_with<E>(
        &self,
        registry_key: &ResourceKey<Registry<E>>,
        decoration: &dyn Fn(&str) -> String,
    ) -> ResourceKey<E> {
        ResourceKey::create(registry_key, self.identifier.with_path_fn(decoration))
    }

    /// `ResourceKey.registryKey()`.
    pub fn registry_key(&self) -> ResourceKey<Registry<T>> {
        ResourceKey::create_registry_key(self.registry_name.clone())
    }
}

/// `ResourceKey.codec(ResourceKey<? extends Registry<T>>)` —
/// `Identifier.CODEC.xmap(name -> create(registryName, name), ResourceKey::identifier)`.
pub fn resource_key_codec<T, Ops: DynamicOps + 'static>(
    registry_name: &ResourceKey<Registry<T>>,
) -> Arc<dyn Codec<ResourceKey<T>, Ops>>
where
    T: 'static,
{
    let registry_name = registry_name.clone();
    codec::xmap::<Identifier, ResourceKey<T>, Ops>(
        identifier_codec::<Ops>(),
        Arc::new(move |name: &Identifier| ResourceKey::create(&registry_name, name.clone())),
        Arc::new(|key: &ResourceKey<T>| key.identifier().clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::registry::Registry;
    use rivet_serialization::json_ops::JsonOps;
    use std::collections::HashSet;

    #[derive(Debug)]
    struct TestElement;
    #[derive(Debug)]
    struct OtherElement;

    fn test_registry(path: &str) -> ResourceKey<Registry<TestElement>> {
        ResourceKey::create_registry_key(Identifier::parse(path))
    }

    fn other_registry(path: &str) -> ResourceKey<Registry<OtherElement>> {
        ResourceKey::create_registry_key(Identifier::parse(path))
    }

    fn element_key(path: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&test_registry(path), Identifier::parse(path))
    }

    /// A `ResourceKey<TestElement>` under the `minecraft:block` registry.
    fn block_key(location: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(
            &test_registry("minecraft:block"),
            Identifier::parse(location),
        )
    }

    #[test]
    fn create_sets_registry_name_to_registry_key_identifier() {
        let registry = test_registry("minecraft:block");
        let key = ResourceKey::create(&registry, Identifier::parse("minecraft:stone"));
        assert_eq!(key.registry(), &Identifier::parse("minecraft:block"));
        assert_eq!(key.identifier(), &Identifier::parse("minecraft:stone"));
    }

    #[test]
    fn create_registry_key_roots_at_root_registry_name() {
        let key: ResourceKey<Registry<TestElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:block"));
        assert_eq!(key.registry(), &Identifier::parse("minecraft:root"));
        assert_eq!(key.identifier(), &Identifier::parse("minecraft:block"));
    }

    #[test]
    fn value_equality_no_t_bound() {
        let a = block_key("minecraft:stone");
        let b = block_key("minecraft:stone");
        assert_eq!(a, b);
        assert_ne!(a, block_key("minecraft:dirt"));

        // A value-equal key of a DIFFERENT element type compares equal — no
        // `T: PartialEq` bound. `assert_eq!` needs same-typed operands, so
        // compare the erased identifiers instead.
        let other: ResourceKey<OtherElement> = ResourceKey::create(
            &other_registry("minecraft:block"),
            Identifier::parse("minecraft:stone"),
        );
        assert_eq!(a.identifier(), other.identifier());
        assert_eq!(a.registry(), other.registry());
    }

    #[test]
    fn hash_is_consistent_with_eq() {
        let a = element_key("minecraft:stone");
        let b = element_key("minecraft:stone");
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&element_key("minecraft:dirt")));
    }

    #[test]
    fn clone_has_no_t_bound() {
        let a = element_key("minecraft:stone");
        let _b: ResourceKey<OtherElement> = {
            // `Clone` on a `ResourceKey<OtherElement>` must compile without
            // `OtherElement: Clone` — the bound is absent.
            let c: ResourceKey<OtherElement> = ResourceKey::create(
                &other_registry("minecraft:block"),
                Identifier::parse("minecraft:stone"),
            );
            c.clone()
        };
        let _ = a;
    }

    #[test]
    fn display_matches_java() {
        let key = block_key("minecraft:stone");
        assert_eq!(
            key.to_string(),
            "ResourceKey[minecraft:block / minecraft:stone]"
        );
        let reg: ResourceKey<Registry<TestElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:block"));
        assert_eq!(
            reg.to_string(),
            "ResourceKey[minecraft:root / minecraft:block]"
        );
    }

    #[test]
    fn is_for_compares_registry_name_to_key_identifier() {
        let block_registry = test_registry("minecraft:block");
        let stone = ResourceKey::create(&block_registry, Identifier::parse("minecraft:stone"));
        assert!(stone.is_for(&block_registry));
        let dirt_registry = test_registry("minecraft:dirt");
        assert!(!stone.is_for(&dirt_registry));
    }

    #[test]
    fn cast_reinterprets_when_is_for() {
        let block_registry = test_registry("minecraft:block");
        let stone: ResourceKey<TestElement> =
            ResourceKey::create(&block_registry, Identifier::parse("minecraft:stone"));
        let cast: ResourceKey<OtherElement> =
            stone.cast(&other_registry("minecraft:block")).unwrap();
        assert_eq!(cast.identifier(), &Identifier::parse("minecraft:stone"));
        let wrong_registry = other_registry("minecraft:dirt");
        assert!(stone.cast::<OtherElement>(&wrong_registry).is_none());
    }

    #[test]
    fn dependent_applies_suffix_to_identifier() {
        let block_registry = test_registry("minecraft:block");
        let stone = ResourceKey::create(&block_registry, Identifier::parse("minecraft:stone"));
        let dependent: ResourceKey<OtherElement> =
            stone.dependent(&other_registry("minecraft:item"), "_item");
        assert_eq!(dependent.registry(), &Identifier::parse("minecraft:item"));
        assert_eq!(
            dependent.identifier(),
            &Identifier::parse("minecraft:stone_item")
        );
    }

    #[test]
    fn dependent_with_applies_decoration_to_identifier() {
        let block_registry = test_registry("minecraft:block");
        let stone = ResourceKey::create(&block_registry, Identifier::parse("minecraft:stone"));
        let dependent: ResourceKey<OtherElement> = stone
            .dependent_with(&other_registry("minecraft:item"), &|p: &str| {
                format!("{}_item", p)
            });
        assert_eq!(
            dependent.identifier(),
            &Identifier::parse("minecraft:stone_item")
        );
    }

    #[test]
    fn registry_key_roots_at_root_registry_name() {
        let block_registry = test_registry("minecraft:block");
        let stone = ResourceKey::create(&block_registry, Identifier::parse("minecraft:stone"));
        let reg_key = stone.registry_key();
        assert_eq!(reg_key.registry(), &Identifier::parse("minecraft:root"));
        assert_eq!(reg_key.identifier(), &Identifier::parse("minecraft:block"));
    }

    #[test]
    fn resource_key_codec_roundtrips() {
        let ops = JsonOps::INSTANCE;
        let registry = test_registry("minecraft:block");
        let codec = resource_key_codec::<TestElement, JsonOps>(&registry);
        let key = ResourceKey::create(&registry, Identifier::parse("minecraft:stone"));
        let encoded = codec
            .encode_start(&ops, &key)
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:stone".to_string()));
        let input = ops.create_string("minecraft:stone".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(
            decoded.0.identifier(),
            &Identifier::parse("minecraft:stone")
        );
    }
}
