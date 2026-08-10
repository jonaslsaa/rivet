//! Port of `net.minecraft.core.RegistryAccess` + `LayeredRegistryAccess` (MC 26.2).
//!
//! PROVENANCE: `RegistryAccess.java` (107 lines) and `LayeredRegistryAccess.java`
//! (103 lines), both leaves of the `mc.core` manifest unit.
//!
//! #124 scope (ownership C — access/provider): implemented.
//!
//! Binding model (OWNERSHIP.md §Registries, #107):
//! - Heterogeneous registry sets (`RegistryAccess.ImmutableRegistryAccess`, the
//!   ROOT `WritableRegistry<AnyRegistry>`) use `trait AnyRegistry: Any` +
//!   `Box<dyn AnyRegistry>`, downcast at those **two erased boundaries only**.
//!   Here that means the access stores `Box<dyn AnyRegistry>` per registry key
//!   and the only downcast seam is `lookup` (and the crate-internal
//!   `lookup_erased` for tests).
//! - `RegistryAccess` is the `HolderLookup.Provider` view: `lookup(ResourceKey)`
//!   → `Option<&Registry<E>>`, `lookup_or_throw`, `registries()`,
//!   `list_registry_keys()`, `freeze()`.
//! - Layer order STATIC → WORLDGEN → DIMENSIONS → RELOADABLE is observable —
//!   keep an explicit ordered vec, never a `HashMap`.
//! - Registry-instance identity: accesses share the same frozen `Registry<T>`
//!   by sharing the (key, erased-value) entry behind an `Arc`. The `Arc` wraps
//!   the pair, not the erased registry value — the value stays a unique
//!   `Box<dyn AnyRegistry>` (OWNERSHIP forbids `Arc<dyn AnyRegistry>`), so
//!   holder owner-checks (which compare `RegistryId`) see one instance per key
//!   no matter which access resolves it.

use crate::ResourceKey;
use crate::registry::{Registry, RegistryKey};
use crate::root::{AnyBox, AnyRegistry};

use std::sync::Arc;

/// An erased (registry-key, registry-value) entry, shared across accesses so
/// every access sees the same frozen registry instance (registry-instance
/// identity). The `Arc` wraps the pair, never the erased registry value.
pub(crate) type ErasedEntry = Arc<(RegistryKey<()>, AnyBox)>;

/// `net.minecraft.core.RegistryAccess` — a heterogeneous set of frozen
/// registries, ordered per layer.
///
/// This is the erased boundary #1: it stores `Box<dyn AnyRegistry>` per
/// registry key and downcasts via `lookup`. Clone shares the underlying entries
/// (a cheap `Arc` bump), so cloning the access is the same sharing the server
/// gets from `Arc<GameData>`.
#[derive(Debug)]
pub struct RegistryAccess {
    /// The ordered erased registry map (`RegistryAccess.Frozen`).
    pub(crate) registries: Vec<ErasedEntry>,
}

impl Clone for RegistryAccess {
    fn clone(&self) -> Self {
        RegistryAccess {
            registries: self.registries.clone(),
        }
    }
}

impl Default for RegistryAccess {
    fn default() -> Self {
        RegistryAccess::empty()
    }
}

impl RegistryAccess {
    /// `RegistryAccess.EMPTY`.
    pub fn empty() -> Self {
        RegistryAccess {
            registries: Vec::new(),
        }
    }

    /// Build an access from raw (key, erased-registry) pairs.
    ///
    /// Used by `from_registry_of_registries` (the ROOT view), the layered
    /// composite, and — since #382 — by external consumers (the level-storage
    /// value layer) to build a populated provider for a serialization context.
    /// Typed entry points go through `lookup`.
    pub fn from_pairs(pairs: Vec<(RegistryKey<()>, AnyBox)>) -> Self {
        RegistryAccess {
            registries: pairs.into_iter().map(Arc::new).collect(),
        }
    }

    /// Wrap already-shared entries (the layered composite).
    fn from_entries(entries: Vec<ErasedEntry>) -> Self {
        RegistryAccess {
            registries: entries,
        }
    }

    /// The erased lookup — boundary #1. Finds the registry stored under `key`
    /// and returns it erased. Typed callers downcast via
    /// `as_any().downcast_ref` (the sanctioned downcast site for an access);
    /// the public typed path is `lookup`.
    pub(crate) fn lookup_erased(&self, key: &RegistryKey<()>) -> Option<&dyn AnyRegistry> {
        self.registries
            .iter()
            .find(|entry| entry.0 == *key)
            .map(|entry| entry.1.as_ref())
    }

    /// `RegistryAccess.lookup(ResourceKey)` — `Optional<Registry<E>>`.
    ///
    /// Returns a reference to the frozen registry — the value table is shared,
    /// never copied (registry-instance identity). `E` must be `'static` so the
    /// downcast across the erased boundary is sound.
    pub fn lookup<E>(&self, key: &RegistryKey<E>) -> Option<&Registry<E>>
    where
        E: 'static,
    {
        if self.registries.is_empty() {
            return None;
        }
        // Erase the typed key to the stored key type by re-reading its name.
        let erased = ResourceKey::create_registry_key(key.identifier().clone());
        self.lookup_erased(&erased)
            .and_then(|registry| registry.as_any().downcast_ref::<Registry<E>>())
    }

    /// `RegistryAccess.lookupOrThrow(ResourceKey)`.
    pub fn lookup_or_throw<E>(&self, name: &RegistryKey<E>) -> &Registry<E>
    where
        E: 'static,
    {
        self.lookup(name)
            .unwrap_or_else(|| panic!("Missing registry: {}", name))
    }

    /// `RegistryAccess.registries()` — the ordered entry list.
    pub fn registries(&self) -> Vec<RegistryEntry<'_>> {
        self.registries
            .iter()
            .map(|entry| RegistryEntry {
                key: entry.0.clone(),
                value: entry.1.as_ref(),
            })
            .collect()
    }

    /// `RegistryAccess.listRegistryKeys()`.
    pub fn list_registry_keys(&self) -> Vec<RegistryKey<()>> {
        self.registries
            .iter()
            .map(|entry| entry.0.clone())
            .collect()
    }

    /// `RegistryAccess.freeze()` — registries are frozen on entry, so this is
    /// the identity.
    pub fn freeze(self) -> RegistryAccess {
        self
    }

    /// `RegistryAccess.fromRegistryOfRegistries(Registry<? extends Registry<?>>)` —
    /// the access view over the ROOT registry-of-registries. Each ROOT entry is
    /// an erased registry; this erases its key (`ResourceKey<Box<dyn
    /// AnyRegistry>>`) back to `RegistryKey<()>`.
    ///
    /// #124 boundary: the ROOT holds `AnyBox = Box<dyn AnyRegistry>`, which is
    /// not `Clone`, so the erased registries cannot be moved out of the frozen
    /// registry or shared by reference through an owning `RegistryAccess`.
    /// `Registry::entry_set` therefore fails loudly (panics) on a non-empty
    /// ROOT — a later unit that registers built-ins in the ROOT must also
    /// provide the sharing form (#126 holder codecs). In #124 scope the ROOT
    /// starts empty and this yields the empty STATIC-layer view.
    pub fn from_registry_of_registries(registries: &Registry<AnyBox>) -> RegistryAccess {
        let mut pairs: Vec<(RegistryKey<()>, AnyBox)> = Vec::new();
        for (key, value) in registries.entry_set() {
            pairs.push((
                ResourceKey::create_registry_key(key.identifier().clone()),
                value,
            ));
        }
        RegistryAccess::from_pairs(pairs)
    }
}

/// `RegistryAccess.RegistryEntry<T>` — `key` + erased `value`.
///
/// The value is the erased registry (`&dyn AnyRegistry`); typed access is via
/// `RegistryAccess::lookup`, not by downcasting here.
#[derive(Debug, Clone)]
pub struct RegistryEntry<'a> {
    pub key: RegistryKey<()>,
    pub value: &'a dyn AnyRegistry,
}

/// `net.minecraft.core.LayeredRegistryAccess<T>` — the explicit ordered layer
/// vec, plus the eagerly-built composite.
///
/// Layer order STATIC → WORLDGEN → DIMENSIONS → RELOADABLE is observable
/// (OWNERSHIP §Registries); never a HashMap. The composite and the per-layer
/// accesses share the same erased entries, so all resolves return the same
/// registry instances.
#[derive(Debug, Clone)]
pub struct LayeredRegistryAccess<T> {
    keys: Vec<T>,
    values: Vec<RegistryAccess>,
    composite: RegistryAccess,
}

impl<T: Clone + PartialEq + std::fmt::Debug> LayeredRegistryAccess<T> {
    /// `LayeredRegistryAccess(List<T> keys)` — all layers empty.
    pub fn new(keys: Vec<T>) -> Self {
        let values = vec![RegistryAccess::empty(); keys.len()];
        Self::from_parts(keys, values)
    }

    /// Build from keys + values, computing the composite (Java's private
    /// constructor).
    fn from_parts(keys: Vec<T>, values: Vec<RegistryAccess>) -> Self {
        let composite = RegistryAccess::from_entries(collect_registries(&values));
        LayeredRegistryAccess {
            keys,
            values,
            composite,
        }
    }

    /// `LayeredRegistryAccess.getLayerIndexOrThrow(T)`.
    fn get_layer_index_or_throw(&self, layer: &T) -> usize {
        self.keys
            .iter()
            .position(|candidate| candidate == layer)
            .unwrap_or_else(|| panic!("Can't find {:?} inside {:?}", layer, self.keys))
    }

    /// `LayeredRegistryAccess.getLayer(T)`.
    pub fn get_layer(&self, layer: T) -> RegistryAccess {
        let index = self.get_layer_index_or_throw(&layer);
        self.values[index].clone()
    }

    /// `LayeredRegistryAccess.getAccessForLoading(T)` — the layers strictly
    /// below `for_layer` (what is already loaded when loading `for_layer`).
    pub fn get_access_for_loading(&self, for_layer: T) -> RegistryAccess {
        let index = self.get_layer_index_or_throw(&for_layer);
        self.composite_for_layers(0, index)
    }

    /// `LayeredRegistryAccess.getAccessFrom(T)` — `for_layer` and everything
    /// above it.
    pub fn get_access_from(&self, for_layer: T) -> RegistryAccess {
        let index = self.get_layer_index_or_throw(&for_layer);
        self.composite_for_layers(index, self.values.len())
    }

    /// `LayeredRegistryAccess.getCompositeAccessForLayers(from, to)`.
    fn composite_for_layers(&self, from: usize, to: usize) -> RegistryAccess {
        RegistryAccess::from_entries(collect_registries(&self.values[from..to]))
    }

    /// `LayeredRegistryAccess.replaceFrom(T, List<Frozen>)` — replaces the
    /// layers at and above `from_layer`, padding the tail with `EMPTY`.
    pub fn replace_from(&self, from_layer: T, layers: &[RegistryAccess]) -> Self {
        let index = self.get_layer_index_or_throw(&from_layer);
        if layers.len() > self.values.len() - index {
            panic!("Too many values to replace");
        }
        let mut new_values: Vec<RegistryAccess> = self.values[..index].to_vec();
        new_values.extend_from_slice(layers);
        while new_values.len() < self.values.len() {
            new_values.push(RegistryAccess::empty());
        }
        Self::from_parts(self.keys.clone(), new_values)
    }

    /// `LayeredRegistryAccess.compositeAccess()`.
    pub fn composite_access(&self) -> RegistryAccess {
        self.composite.clone()
    }
}

/// `RegistryLayer` — the observable layer order (OWNERSHIP.md §Registries,
/// `RegistryLayer.java`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryLayer {
    /// STATIC — the built-in registries.
    Static,
    /// WORLDGEN.
    Worldgen,
    /// DIMENSIONS.
    Dimensions,
    /// RELOADABLE — the datapack-reloadable registries.
    Reloadable,
}

impl RegistryLayer {
    /// `RegistryLayer.VALUES` — the ordered layer list.
    pub const VALUES: [RegistryLayer; 4] = [
        RegistryLayer::Static,
        RegistryLayer::Worldgen,
        RegistryLayer::Dimensions,
        RegistryLayer::Reloadable,
    ];
}

impl LayeredRegistryAccess<RegistryLayer> {
    /// `RegistryLayer.createRegistryAccess()` — the layered access seeded with
    /// the STATIC layer from the ROOT registry.
    pub fn create_registry_access() -> Self {
        let static_access =
            RegistryAccess::from_registry_of_registries(&crate::root::RootRegistry::root());
        LayeredRegistryAccess::new(RegistryLayer::VALUES.to_vec())
            .replace_from(RegistryLayer::Static, &[static_access])
    }
}

/// Merge the layer accesses into one ordered entry list, panicking on a
/// duplicated registry key (Java's `collectRegistries`
/// `IllegalStateException`). Shares the entries (an `Arc` bump), so merged
/// accesses see the same registry instances.
#[doc(hidden)]
pub(crate) fn collect_registries(layers: &[RegistryAccess]) -> Vec<ErasedEntry> {
    let mut merged: Vec<ErasedEntry> = Vec::new();
    for layer in layers {
        for entry in &layer.registries {
            if let Some(existing) = merged.iter().find(|candidate| candidate.0 == entry.0) {
                panic!("Duplicated registry {}", existing.0);
            }
            merged.push(entry.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identifier;
    use crate::ResourceKey;
    use crate::builder::RegistryBuilder;
    use crate::registry::{Registry, RegistryKey};
    use crate::root::AnyBox;

    #[derive(Debug)]
    struct TestElement;

    fn element_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn erased_key() -> RegistryKey<()> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn access_with_one_registry() -> RegistryAccess {
        let registry: Registry<TestElement> = RegistryBuilder::new(&element_key()).freeze();
        RegistryAccess::from_pairs(vec![(erased_key(), Box::new(registry) as AnyBox)])
    }

    #[test]
    fn empty_access_has_no_registries() {
        let access = RegistryAccess::empty();
        assert!(access.lookup(&element_key()).is_none());
        assert!(access.lookup_erased(&erased_key()).is_none());
        assert!(access.list_registry_keys().is_empty());
        assert!(access.registries().is_empty());
    }

    #[test]
    #[should_panic(expected = "Missing registry")]
    fn lookup_or_throw_missing_registry_panics() {
        let access = RegistryAccess::empty();
        let _ = access.lookup_or_throw(&element_key());
    }

    #[test]
    fn erased_boundary_downcasts_to_the_stored_registry() {
        let access = access_with_one_registry();

        let erased = access.lookup_erased(&erased_key()).expect("erased lookup");
        let typed: &Registry<TestElement> = erased
            .as_any()
            .downcast_ref()
            .expect("downcast to the stored element type");
        let _ = typed;

        // The erased boundary is exact: a different element type must not land.
        assert!(
            erased
                .as_any()
                .downcast_ref::<Registry<crate::registries::BlockType>>()
                .is_none()
        );
    }

    #[test]
    fn registries_and_keys_are_ordered() {
        let access = access_with_one_registry();
        assert_eq!(access.list_registry_keys(), vec![erased_key()]);
        let entries = access.registries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, erased_key());
        assert!(
            entries[0]
                .value
                .as_any()
                .downcast_ref::<Registry<TestElement>>()
                .is_some()
        );
    }

    #[test]
    fn cloned_access_shares_the_same_registry_instances() {
        let access = access_with_one_registry();
        let copy = access.clone();
        let a = copy.lookup_erased(&erased_key()).expect("copy resolves");
        let b = access
            .lookup_erased(&erased_key())
            .expect("original resolves");
        assert!(std::ptr::eq(
            a.as_any().downcast_ref::<Registry<TestElement>>().unwrap(),
            b.as_any().downcast_ref::<Registry<TestElement>>().unwrap(),
        ));
    }

    #[test]
    fn layered_access_preserves_static_to_reloadable_order() {
        let layers = LayeredRegistryAccess::new(RegistryLayer::VALUES.to_vec())
            .replace_from(RegistryLayer::Static, &[access_with_one_registry()]);

        // get_access_for_loading(layer) = the layers strictly below it.
        assert!(
            layers
                .get_access_for_loading(RegistryLayer::Static)
                .list_registry_keys()
                .is_empty()
        );
        assert_eq!(
            layers
                .get_access_for_loading(RegistryLayer::Worldgen)
                .list_registry_keys(),
            vec![erased_key()]
        );

        // get_access_from(layer) = the layer and everything above it.
        assert_eq!(
            layers
                .get_access_from(RegistryLayer::Static)
                .list_registry_keys(),
            vec![erased_key()]
        );
        assert!(
            layers
                .get_access_from(RegistryLayer::Worldgen)
                .list_registry_keys()
                .is_empty()
        );

        // The composite merges all layers, in order, sharing instances.
        assert_eq!(
            layers.composite_access().list_registry_keys(),
            vec![erased_key()]
        );
    }

    #[test]
    fn replace_from_pads_with_empty_layers() {
        let layers = LayeredRegistryAccess::new(RegistryLayer::VALUES.to_vec())
            .replace_from(RegistryLayer::Worldgen, &[access_with_one_registry()]);

        assert!(
            layers
                .get_layer(RegistryLayer::Static)
                .list_registry_keys()
                .is_empty()
        );
        assert_eq!(
            layers
                .get_layer(RegistryLayer::Worldgen)
                .list_registry_keys(),
            vec![erased_key()]
        );
        assert!(
            layers
                .get_layer(RegistryLayer::Dimensions)
                .list_registry_keys()
                .is_empty()
        );
        assert!(
            layers
                .get_layer(RegistryLayer::Reloadable)
                .list_registry_keys()
                .is_empty()
        );
    }

    #[test]
    #[should_panic(expected = "Too many values to replace")]
    fn replace_from_with_too_many_values_panics() {
        let layers = LayeredRegistryAccess::new(RegistryLayer::VALUES.to_vec());
        let too_many: Vec<RegistryAccess> = (0..5).map(|_| RegistryAccess::empty()).collect();
        let _ = layers.replace_from(RegistryLayer::Static, &too_many);
    }

    #[test]
    #[should_panic(expected = "Duplicated registry")]
    fn composite_detects_duplicated_registry_keys() {
        let layers = LayeredRegistryAccess::from_parts(
            vec![RegistryLayer::Static, RegistryLayer::Worldgen],
            vec![access_with_one_registry(), access_with_one_registry()],
        );
        let _ = layers;
    }
}
