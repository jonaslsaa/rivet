//! Port of `net.minecraft.world.level.lighting.DataLayerStorageMap`
//! (MC 26.2, Paper) — the section-node → `DataLayer` storage backing a light
//! layer engine.
//!
//! Java: `DataLayerStorageMap.java` in `working/Paper`. A `long`-keyed map of
//! per-section light layers (keyed by section node), plus a 2-entry LRU cache
//! of the most recently read layers and a `cacheEnabled` flag.
//!
//! ## Cache omission
//!
//! Java's `lastSectionKeys[2]`/`lastSections[2]` cache is a pure read
//! optimization: a hit returns the same `DataLayer` object a map lookup would
//! (the cache stores *references* into the map, never copies). Nothing about
//! the cache is observable to a caller that goes through the map: the engine
//! storages call `clearCache()` after every mutator (`setLayer`/`removeLayer`/
//! `copyDataLayer`), so a cache hit always returns the map's current layer.
//! The port drops it — the `HashMap` is the single source of truth, exactly as
//! the `SWMRNibbleArray` port drops Java's thread-local buffer pooling for the
//! same reason. `clearCache()`/`disableCache()` become obsolete paths and are
//! not ported.
//!
//! ## `copy()` semantics (value, not Java's reference sharing)
//!
//! Java's `copy()` is a shallow map clone: fastutil's `clone()` copies the
//! value *array* but shares the `DataLayer` objects, so a layer mutated through
//! the copy is visible in the original. The port instead deep-copies each
//! layer (`DataLayer::copy`). The two are behaviorally identical for the real
//! engine: every in-place mutation of a stored layer happens on a freshly
//! copied layer (`getDataLayerToWrite` copies then `setLayer`; `copyDataLayer`
//! copies then stores), never on a layer aliased across two maps, so the
//! sharing Java's shallow clone provides is never observed. The oracle probe's
//! `storage.copy.same.reference=false` / `original.filled=false` goldens are
//! actually cache-staleness artifacts (the probe's `removeLayer`/`setLayer`
//! leave the stale cached layer behind), and a deep-copy port reproduces those
//! exact lines. Deep value semantics also keep the map within OWNERSHIP.md's
//! no-shared-mutable-state model and mirror the `SWMRNibbleArray` sibling
//! ("Java shares the reference; the port clones").
//!
//! `copy()` is `abstract` in Java (each subclass returns its own type); the
//! base-class port makes it concrete: a fresh storage map of independent
//! `DataLayer` copies. `getLayer` returns the map's layer or Java `null`; the
//! port splits the read/mutate views into `get_layer`/`get_layer_mut`.
//!
//! RivetTodo(#184): this is the `mc.world.level.lighting.core` storage-map
//! unit. Its subclass consumers (`LayerLightSectionStorage`/`BlockLightSection
//! Storage`/`SkyLightSectionStorage`) are vanilla dead jar-surface under
//! Starlight and defer with the `mc.world.level.lighting.engine` unit; the
//! engine's hold-a-layer-across-`setLayer` pattern (`getDataLayerToWrite`)
//! will need interior mutability when that unit lands — noted, not built here.

use std::collections::HashMap;

use crate::chunk::data_layer::DataLayer;

/// `DataLayerStorageMap` — section-node → `DataLayer` storage.
pub struct DataLayerStorageMap {
    /// `map` — the `Long2ObjectOpenHashMap<DataLayer>` backing store.
    map: HashMap<u64, DataLayer>,
}

impl DataLayerStorageMap {
    /// `DataLayerStorageMap(Long2ObjectOpenHashMap<DataLayer> map)` — Java's
    /// constructor takes the pre-built map (its subclasses construct it) and
    /// enables the (here omitted) cache.
    pub fn new(map: HashMap<u64, DataLayer>) -> Self {
        DataLayerStorageMap { map }
    }

    /// An empty storage map — the natural value default for the base class.
    pub fn empty() -> Self {
        DataLayerStorageMap {
            map: HashMap::new(),
        }
    }

    /// `copy()` — a fresh storage map of independent `DataLayer` copies. Java's
    /// concrete subclasses define the shape; `SkyDataLayerStorageMap.copy()` is
    /// `new ...(this.map.clone(), ...)` and `BlockDataLayerStorageMap.copy()` is
    /// `new ...(this.map.clone())`. See the module docs for why the port
    /// deep-copies the layers instead of sharing them.
    pub fn copy(&self) -> DataLayerStorageMap {
        DataLayerStorageMap {
            map: self.map.iter().map(|(&k, v)| (k, v.copy())).collect(),
        }
    }

    /// `copyDataLayer(long sectionNode)` — copy the layer at `sectionNode`,
    /// store the copy in place of the original, and return it. Java NPEs when
    /// the section has no layer (`map.get(...).copy()`); the port panics with
    /// the same contract (Java exceptions surface as panics, e.g.
    /// `DataLayer::with_data`). The returned reference is the map's stored
    /// copy, so mutating it is visible to later `get_layer` calls (Java's
    /// shared-reference contract for the returned object).
    pub fn copy_data_layer(&mut self, section_node: u64) -> &mut DataLayer {
        let copied = self
            .map
            .get(&section_node)
            .expect("copyDataLayer called on a missing section")
            .copy();
        self.map.insert(section_node, copied);
        self.map.get_mut(&section_node).unwrap()
    }

    /// `hasLayer(long sectionNode)`.
    pub fn has_layer(&self, section_node: u64) -> bool {
        self.map.contains_key(&section_node)
    }

    /// `getLayer(long sectionNode)` — the layer, or `None` (Java `null`).
    pub fn get_layer(&self, section_node: u64) -> Option<&DataLayer> {
        self.map.get(&section_node)
    }

    /// The mutable view of `getLayer` — Java returns one shared object for
    /// reading and writing; the port splits the borrows.
    pub fn get_layer_mut(&mut self, section_node: u64) -> Option<&mut DataLayer> {
        self.map.get_mut(&section_node)
    }

    /// `removeLayer(long sectionNode)` — the removed layer, or `None`.
    pub fn remove_layer(&mut self, section_node: u64) -> Option<DataLayer> {
        self.map.remove(&section_node)
    }

    /// `setLayer(long sectionNode, DataLayer layer)`.
    pub fn set_layer(&mut self, section_node: u64, layer: DataLayer) {
        self.map.insert(section_node, layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2048-byte layer with a recognizable first byte.
    fn filled_layer(first_byte: u8) -> DataLayer {
        let mut data = vec![0u8; crate::chunk::data_layer::SIZE as usize];
        data[0] = first_byte;
        DataLayer::with_data(data)
    }

    #[test]
    fn set_get_remove_round_trip() {
        let mut storage = DataLayerStorageMap::empty();
        let key = 0x123456789u64;
        assert!(!storage.has_layer(key));
        assert!(storage.get_layer(key).is_none());
        storage.set_layer(key, filled_layer(0xAB));
        assert!(storage.has_layer(key));
        assert_eq!(storage.get_layer(key).unwrap().get_data()[0], 0xAB);
        let removed = storage.remove_layer(key);
        assert!(removed.is_some());
        assert!(!storage.has_layer(key));
    }

    #[test]
    fn copy_data_layer_replaces_and_returns_the_stored_copy() {
        // The `storage.copyDataLayer.filled` / `storage.get.after.mutate.filled`
        // goldens from the Paper oracle: the returned layer is the map's stored
        // copy, so mutating it is visible to a later read.
        let mut storage = DataLayerStorageMap::empty();
        let key = 7u64;
        storage.set_layer(key, filled_layer(0x11));
        {
            let copied = storage.copy_data_layer(key);
            assert_eq!(copied.get_data()[0], 0x11);
            copied.fill(9);
        }
        assert!(storage.get_layer(key).unwrap().is_definitely_filled_with(9));
    }

    #[test]
    fn copy_data_layer_absent_panics_like_java_npe() {
        // Java NPEs on `map.get(...).copy()` for a missing section; the port
        // panics with the same contract (`storage.copyDataLayer.absent=throws:NPE`).
        let mut storage = DataLayerStorageMap::empty();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _ = storage.copy_data_layer(99u64);
            }))
            .is_err()
        );
    }

    #[test]
    fn copy_is_deep_like_the_oracle_golden() {
        // `storage.copy.same.reference=false`, `storage.copy.original.filled=
        // false`, `storage.copy.copied.filled=true` from the Paper oracle: a
        // fresh `copy()` gives an independent layer, so mutating it through the
        // copy leaves the original untouched.
        let mut storage = DataLayerStorageMap::empty();
        let key = 3u64;
        storage.set_layer(key, filled_layer(0x77));
        let mut copied = storage.copy();
        assert!(!std::ptr::eq(
            storage.get_layer(key).unwrap(),
            copied.get_layer(key).unwrap()
        ));
        copied.get_layer_mut(key).unwrap().fill(0);
        assert!(!storage.get_layer(key).unwrap().is_definitely_filled_with(0));
        assert!(copied.get_layer(key).unwrap().is_definitely_filled_with(0));
    }

    #[test]
    fn new_wraps_prebuilt_map_values() {
        let mut map = HashMap::new();
        map.insert(1u64, filled_layer(0x31));
        let storage = DataLayerStorageMap::new(map);
        assert!(storage.has_layer(1));
        assert_eq!(storage.get_layer(1).unwrap().get_data()[0], 0x31);
    }
}
