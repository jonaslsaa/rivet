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
//! ## Shared-layer semantics (`copy()` and the read/write views)
//!
//! Java's map stores *references* to `DataLayer` objects, and `copy()` is a
//! shallow map clone sharing those references — mutating a layer through the
//! copy is visible in the original (fastutil's `Long2ObjectOpenHashMap.clone`
//! copies the value *array* but shares the `DataLayer` objects). This is
//! load-bearing, not incidental: the engine storages (`LayerLightSectionStorage`)
//! clone the map on every `swapSectionMap()` so the visible/updating maps keep
//! pointing at the same layer objects, and `getLayer` hands out the stored
//! object for in-place mutation. The port reproduces it with
//! `Rc<RefCell<DataLayer>>` — the same single-threaded shared-mutable pattern
//! `RegionFileVersion`'s scratch sink already uses, and within OWNERSHIP.md's
//! tick-thread model (both maps belong to one layer storage on one thread;
//! there is no `Sync` requirement). `get_layer` returns a cloned `Rc` handle,
//! so a caller can hold a layer *while* mutating the map (Java callers hold the
//! object across a `setLayer`, e.g. `getDataLayerToWrite`). `copy_data_layer`
//! returns the stored copy's handle, matching Java's "the map now stores this
//! exact layer".
//!
//! `copy()` is `abstract` in Java (each subclass returns its own type); the
//! base-class port makes it concrete: a fresh storage map sharing the same
//! layers. `getLayer` returns the map's layer or Java `null` (`None`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::chunk::data_layer::DataLayer;

/// `DataLayerStorageMap` — section-node → `DataLayer` storage.
pub struct DataLayerStorageMap {
    /// `map` — the `Long2ObjectOpenHashMap<DataLayer>` backing store. The
    /// layers are shared handles (Java references), so `copy()` and
    /// `getLayer` preserve Java's aliasing.
    map: HashMap<u64, Rc<RefCell<DataLayer>>>,
}

impl DataLayerStorageMap {
    /// `DataLayerStorageMap(Long2ObjectOpenHashMap<DataLayer> map)` — Java's
    /// constructor takes the pre-built map (its subclasses construct it) and
    /// enables the (here omitted) cache. The values are wrapped into shared
    /// handles.
    pub fn new(map: HashMap<u64, DataLayer>) -> Self {
        DataLayerStorageMap {
            map: map
                .into_iter()
                .map(|(k, v)| (k, Rc::new(RefCell::new(v))))
                .collect(),
        }
    }

    /// An empty storage map — the natural value default for the base class.
    pub fn empty() -> Self {
        DataLayerStorageMap {
            map: HashMap::new(),
        }
    }

    /// `copy()` — a fresh storage map. Java's concrete subclasses define the
    /// shape; `SkyDataLayerStorageMap.copy()` is `new ...(this.map.clone(), ...)`
    /// and `BlockDataLayerStorageMap.copy()` is `new ...(this.map.clone())` —
    /// a *shallow* map clone sharing the stored `DataLayer` references. The
    /// port's `Rc` clone does the same: a `DataLayer` mutated through one map
    /// is visible in the other, exactly as Java's `swapSectionMap()` relies on
    /// (it clones the map, not the layers).
    pub fn copy(&self) -> DataLayerStorageMap {
        DataLayerStorageMap {
            map: self.map.clone(),
        }
    }

    /// `copyDataLayer(long sectionNode)` — copy the layer at `sectionNode`,
    /// store the copy in place of the original, and return it. Java NPEs when
    /// the section has no layer (`map.get(...).copy()`); the port panics with
    /// the same contract (Java exceptions surface as panics, e.g.
    /// `DataLayer::with_data`). The returned handle is the map's stored copy,
    /// so mutating it through the handle is visible to later `get_layer` calls.
    pub fn copy_data_layer(&mut self, section_node: u64) -> Rc<RefCell<DataLayer>> {
        let copied = self
            .map
            .get(&section_node)
            .expect("copyDataLayer called on a missing section")
            .borrow()
            .copy();
        let handle = Rc::new(RefCell::new(copied));
        self.map.insert(section_node, handle.clone());
        handle
    }

    /// `hasLayer(long sectionNode)`.
    pub fn has_layer(&self, section_node: u64) -> bool {
        self.map.contains_key(&section_node)
    }

    /// `getLayer(long sectionNode)` — a shared handle to the layer, or `None`
    /// (Java `null`). The handle is a clone of the stored one, so the caller
    /// can read (`borrow`) or write (`borrow_mut`) the stored layer even while
    /// mutating the map itself.
    pub fn get_layer(&self, section_node: u64) -> Option<Rc<RefCell<DataLayer>>> {
        self.map.get(&section_node).cloned()
    }

    /// `removeLayer(long sectionNode)` — the removed layer's handle, or `None`.
    pub fn remove_layer(&mut self, section_node: u64) -> Option<Rc<RefCell<DataLayer>>> {
        self.map.remove(&section_node)
    }

    /// `setLayer(long sectionNode, DataLayer layer)`.
    pub fn set_layer(&mut self, section_node: u64, layer: DataLayer) {
        self.map.insert(section_node, Rc::new(RefCell::new(layer)));
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
        assert_eq!(storage.get_layer(key).unwrap().borrow().get_data()[0], 0xAB);
        let removed = storage.remove_layer(key);
        assert!(removed.is_some());
        assert!(!storage.has_layer(key));
    }

    #[test]
    fn copy_data_layer_replaces_and_returns_the_stored_copy() {
        let mut storage = DataLayerStorageMap::empty();
        let key = 7u64;
        storage.set_layer(key, filled_layer(0x11));
        let copied = storage.copy_data_layer(key);
        // The returned handle is the map's stored layer: mutate it and a later
        // read sees the change (Java's shared-reference contract).
        copied.borrow_mut().fill(9);
        assert!(
            storage
                .get_layer(key)
                .unwrap()
                .borrow()
                .is_definitely_filled_with(9)
        );
        // The copy is independent of any earlier handle: filling it did not
        // disturb the original content of the pre-copy layer.
        storage.set_layer(key, filled_layer(0x22));
        let copied = storage.copy_data_layer(key);
        assert_eq!(copied.borrow().get_data()[0], 0x22);
        assert_eq!(storage.get_layer(key).unwrap().borrow().get_data()[0], 0x22);
    }

    #[test]
    fn copy_data_layer_absent_panics_like_java_npe() {
        // Java NPEs on `map.get(...).copy()` for a missing section; the port
        // panics with the same contract.
        let mut storage = DataLayerStorageMap::empty();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _ = storage.copy_data_layer(99u64);
            }))
            .is_err()
        );
    }

    #[test]
    fn copy_shares_layer_references_like_java() {
        let mut storage = DataLayerStorageMap::empty();
        let key = 3u64;
        storage.set_layer(key, filled_layer(0x77));
        let copied = storage.copy();
        // Java's `copy()` clones the map, sharing the stored DataLayer
        // references (`getLayer` through both maps returns the *same* object,
        // and `swapSectionMap()` relies on it): mutating a layer through the
        // copy is visible in the original.
        assert!(Rc::ptr_eq(
            &storage.get_layer(key).unwrap(),
            &copied.get_layer(key).unwrap()
        ));
        copied.get_layer(key).unwrap().borrow_mut().fill(0);
        assert!(
            storage
                .get_layer(key)
                .unwrap()
                .borrow()
                .is_definitely_filled_with(0)
        );
        assert!(
            copied
                .get_layer(key)
                .unwrap()
                .borrow()
                .is_definitely_filled_with(0)
        );
    }

    #[test]
    fn new_wraps_prebuilt_map_values_into_shared_handles() {
        let mut map = HashMap::new();
        map.insert(1u64, filled_layer(0x31));
        let storage = DataLayerStorageMap::new(map);
        assert!(storage.has_layer(1));
        assert_eq!(storage.get_layer(1).unwrap().borrow().get_data()[0], 0x31);
        // The same layer is shared after copy, like every other path.
        let copied = storage.copy();
        assert!(Rc::ptr_eq(
            &storage.get_layer(1).unwrap(),
            &copied.get_layer(1).unwrap()
        ));
    }
}
