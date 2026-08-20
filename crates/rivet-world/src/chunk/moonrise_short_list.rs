//! Port of `ca.spottedleaf.moonrise.common.list.ShortList` (MC 26.2) — the
//! Moonrise insertion-ordered short list with O(1) membership/index lookup
//! (issue #216). `LevelChunkSection`'s Moonrise `tickingBlocks` bookkeeping
//! feeds this; it is a distinct type from the fastutil `ShortList` interface
//! the chunk `postProcessing` arrays use.
//!
//! Java: `working/Paper/paper-server/src/main/java/ca/spottedleaf/moonrise/common/list/ShortList.java`.
//! A value-to-index map keeps `add`/`remove` at O(1): adding appends and
//! records the index; removing a middle element swaps the tail into its slot,
//! so iteration order is preserved across `add`s but not across `remove`s
//! (Java's `end` swap).

use std::collections::HashMap;

/// `ca.spottedleaf.moonrise.common.list.ShortList`.
#[derive(Clone)]
pub struct ShortList {
    /// `map` — value → current index (Java's `Short2ShortOpenHashMap` with the
    /// `Short.MIN_VALUE` default-return sentinel; `None` is the Rust
    /// equivalent).
    map: HashMap<i16, i16>,
    /// `byIndex` — index → value, grown on demand (Java starts at an empty
    /// `short[0]`).
    by_index: Vec<i16>,
    /// `count` — the live size (Java's `short count`; values after `count`
    /// are stale and ignored).
    count: i16,
}

impl ShortList {
    /// A fresh empty list.
    pub fn new() -> Self {
        ShortList {
            map: HashMap::new(),
            by_index: Vec::new(),
            count: 0,
        }
    }

    /// `size()` — `(int) count`.
    pub fn size(&self) -> usize {
        self.count as usize
    }

    /// `getRaw(int)` — the value at `index` (valid only for `index < size()`,
    /// like Java's unchecked `byIndex[index]`).
    pub fn get_raw(&self, index: usize) -> i16 {
        self.by_index[index]
    }

    /// `setMinCapacity(int)` — grows the backing arrays so at least `len`
    /// entries fit (an allocation hint; the observable contents are unchanged).
    pub fn set_min_capacity(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if self.by_index.len() < len {
            self.by_index.resize(len, 0);
        }
        self.map.reserve(len.saturating_sub(self.map.len()));
    }

    /// `add(short)` — appends `value` if absent, returning `false` when it was
    /// already present (Java's `putIfAbsent` against the `Short.MIN_VALUE`
    /// sentinel). Resizes `byIndex` exactly like Java: to `max(4, count * 2)`
    /// when the list is full.
    pub fn add(&mut self, value: i16) -> bool {
        let count = self.count as usize;
        if self.map.contains_key(&value) {
            return false;
        }
        self.map.insert(value, count as i16);
        if self.by_index.len() == count {
            self.by_index.resize(std::cmp::max(4usize, count * 2), 0);
        }
        self.by_index[count] = value;
        self.count = (count + 1) as i16;
        true
    }

    /// `remove(short)` — removes `value`, swapping the tail into its slot.
    /// Returns `false` when `value` was not present.
    pub fn remove(&mut self, value: i16) -> bool {
        let Some(index) = self.map.remove(&value) else {
            return false;
        };
        let index = index as usize;
        self.count -= 1;
        let end_index = self.count as usize;
        let end = self.by_index[end_index];
        if index != end_index {
            self.map.insert(end, index as i16);
        }
        self.by_index[index] = end;
        self.by_index[end_index] = 0;
        true
    }

    /// `clear()` — drops every entry (Java leaves the stale `byIndex` cells
    /// in place; only `count` and the map are reset).
    pub fn clear(&mut self) {
        self.count = 0;
        self.map.clear();
    }
}

impl Default for ShortList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_appends_and_dedupes_in_order() {
        let mut list = ShortList::new();
        assert_eq!(list.size(), 0);
        assert!(list.add(5));
        assert!(list.add(3));
        assert!(list.add(300));
        assert_eq!(list.size(), 3);
        // Duplicate add returns false and leaves the list unchanged.
        assert!(!list.add(3));
        assert_eq!(list.size(), 3);
        // `add` appends in call order.
        assert_eq!(list.get_raw(0), 5);
        assert_eq!(list.get_raw(1), 3);
        assert_eq!(list.get_raw(2), 300);
    }

    #[test]
    fn remove_swaps_the_tail_into_the_slot() {
        let mut list = ShortList::new();
        for v in [10i16, 20, 30, 40] {
            list.add(v);
        }
        // Removing a middle element moves the tail (40) into its slot.
        assert!(list.remove(20));
        assert_eq!(list.size(), 3);
        assert_eq!(list.get_raw(0), 10);
        assert_eq!(list.get_raw(1), 40); // tail swapped in
        assert_eq!(list.get_raw(2), 30);
        // Removing the same value again returns false.
        assert!(!list.remove(20));
        // The moved tail's index was updated: removing it by value still works.
        assert!(list.remove(40));
        assert_eq!(list.size(), 2);
        assert_eq!(list.get_raw(0), 10);
        assert_eq!(list.get_raw(1), 30);
    }

    #[test]
    fn remove_absent_returns_false_and_clear_resets() {
        let mut list = ShortList::new();
        assert!(!list.remove(7));
        list.add(1);
        list.add(2);
        list.clear();
        assert_eq!(list.size(), 0);
        assert!(!list.remove(1));
        // After clear, values can be re-added fresh.
        assert!(list.add(1));
        assert_eq!(list.size(), 1);
    }

    #[test]
    fn set_min_capacity_does_not_change_contents() {
        let mut list = ShortList::new();
        list.add(1);
        list.set_min_capacity(100);
        assert_eq!(list.size(), 1);
        assert_eq!(list.get_raw(0), 1);
        assert!(list.add(2));
        assert_eq!(list.size(), 2);
    }

    #[test]
    fn add_resizes_from_empty_through_many_entries() {
        // Exercises the grow path: `max(4, count * 2)` from an empty `byIndex`,
        // covering 0→4→8→16... resizes (Java's `Arrays.copyOf`).
        let mut list = ShortList::new();
        let count = 4096;
        for v in 0..count {
            assert!(list.add(v as i16), "add {v} must succeed");
        }
        assert_eq!(list.size(), count);
        for v in 0..count {
            assert_eq!(list.get_raw(v), v as i16);
        }
        // Every value is present exactly once.
        assert!(!list.add(0));
    }
}
